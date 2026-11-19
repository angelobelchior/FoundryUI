use std::sync::Arc;

use serde::Serialize;
use serde_json::json;
use tauri::{AppHandle, Emitter, State};

use super::agent_commands::{AgentCommandError, AgentCommandResult};
use super::agent_runtime::{
    CycleEvent, CycleOutcome, CycleRequest, CycleSink, EndpointTurnProvider, run_agent_cycle,
};
use super::app::AppState;
use super::backend::FoundryBackend;
use super::catalog;
use super::chat::{ChatParams, GenerationStart, SendReceipt};
use super::db::agent::authorization::AuthorizationStatus;
use super::db::agent::{
    ActiveRun, AgentMode, AgentStoreError, CancelReceipt, ExecutionRun, HistoryContent, RunState,
};
use super::db::{ChatMessage, Conversation, Database, MessageRole, MessageStatus};
use super::error::{FoundryError, FoundryResult};
use super::http::StreamTimeouts;
use super::local_inference::VerifiedLocalEndpoint;
use super::params;
use super::permission_commands::AuthorizationRequestView;

const AGENT_RUN_EVENT: &str = "agent-run-event";

#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AgentRunEvent {
    Cycle {
        event: CycleEvent,
    },
    Completed {
        conversation_id: i64,
        outcome: CycleOutcome,
    },
    Error {
        conversation_id: i64,
        message: String,
    },
}

struct AgentRunSink {
    app: AppHandle,
}

impl CycleSink for AgentRunSink {
    fn emit(&self, event: &CycleEvent) -> Result<(), String> {
        self.app
            .emit(
                AGENT_RUN_EVENT,
                AgentRunEvent::Cycle {
                    event: event.clone(),
                },
            )
            .map_err(|error| error.to_string())
    }
}

struct PreparedAgentRun {
    database: Arc<Database>,
    endpoint: VerifiedLocalEndpoint,
    model_id: String,
    supports_tool_calling: bool,
    chat_params: Option<ChatParams>,
    timeouts: StreamTimeouts,
    conversation: Conversation,
}

async fn prepare_agent_run(
    state: &AppState,
    conversation_id: i64,
) -> FoundryResult<PreparedAgentRun> {
    let database = state.database()?;
    let conversation = database.conversation(conversation_id)?;
    let global = database.generation_defaults()?;
    let model_params = database.model_params(&conversation.model_alias)?;
    let conversation_params = database.conversation_params(conversation_id)?;
    let resolved = params::resolve(&conversation_params, &model_params, &global);
    let cli = state.backend()?;
    let endpoint = state.server.verified_endpoint(&cli).await?;
    let loaded = cli.list_loaded_models().await?;
    let requested = conversation
        .model_variant_id
        .as_deref()
        .unwrap_or(&conversation.model_alias);
    let loaded_model = loaded
        .iter()
        .find(|model| model.alias == requested || model.id == requested)
        .ok_or_else(|| FoundryError::ModelNotLoaded {
            alias: requested.to_string(),
        })?;
    let overlay = catalog::overlay()?;
    let supported = catalog::supported_generation_params(
        loaded_model,
        Some(&conversation.model_alias),
        overlay,
    );
    let resolved = params::filter_supported(resolved, &supported);
    Ok(PreparedAgentRun {
        database,
        endpoint,
        model_id: loaded_model.id.clone(),
        supports_tool_calling: loaded_model.supports_tool_calling,
        chat_params: Some(resolved.params()),
        timeouts: state.server.stream_timeouts(),
        conversation,
    })
}

#[tauri::command]
pub async fn agent_run_start(
    app: AppHandle,
    state: State<'_, AppState>,
    conversation_id: i64,
    content: String,
) -> FoundryResult<SendReceipt> {
    let text = content.trim();
    if text.is_empty() {
        return Err(FoundryError::CommandFailed {
            command: "agent_run_start".to_string(),
            message: "the message is empty".to_string(),
            hint: None,
        });
    }
    let prepared = prepare_agent_run(state.inner(), conversation_id).await?;
    if !prepared.supports_tool_calling {
        return Err(FoundryError::CommandFailed {
            command: "agent_run_start".into(),
            message: "this model does not support tools; continue using chat".into(),
            hint: None,
        });
    }
    let store = prepared.database.agent_store();
    if store
        .active_run_for_conversation(conversation_id)
        .map_err(store_error)?
        .is_some()
        || store
            .execution_run(conversation_id)
            .map_err(store_error)?
            .is_some_and(|snapshot| {
                matches!(snapshot.run.state, RunState::Prepared | RunState::Running)
                    || snapshot.writer_remaining
            })
    {
        return Err(store_error(AgentStoreError::ConversationBusy));
    }
    let Some(start) = state.generations.begin(conversation_id) else {
        return Err(FoundryError::CommandFailed {
            command: "agent_run_start".to_string(),
            message: "this conversation is already busy".to_string(),
            hint: None,
        });
    };
    let request_id = start.request_id.clone();
    let user_message = prepared.database.append_message(
        conversation_id,
        MessageRole::User,
        text,
        "",
        Some(&prepared.conversation.model_alias),
        MessageStatus::Complete,
    )?;
    let assistant_message = prepared.database.append_message(
        conversation_id,
        MessageRole::Assistant,
        "",
        "",
        Some(&prepared.conversation.model_alias),
        MessageStatus::Streaming,
    )?;
    let user_message = prepared.database.message(user_message.id)?;
    let assistant_message = prepared.database.message(assistant_message.id)?;
    let conversation = prepared.database.conversation(conversation_id)?;

    let task_assistant = assistant_message.clone();
    let app_for_task = app.clone();
    tauri::async_runtime::spawn(async move {
        run_agent_task(app_for_task, prepared, start, task_assistant).await;
    });

    Ok(SendReceipt {
        request_id,
        conversation,
        user_message,
        assistant_message,
        removed: Vec::new(),
    })
}

async fn run_agent_task(
    app: AppHandle,
    prepared: PreparedAgentRun,
    start: GenerationStart,
    assistant_message: ChatMessage,
) {
    let _generation = start;
    let database = prepared.database;
    let conversation_id = assistant_message.conversation_id;

    let messages = match build_messages(&database, conversation_id, assistant_message.id) {
        Ok(messages) => messages,
        Err(error) => {
            let _ = database.finish_message(
                assistant_message.id,
                MessageStatus::Error,
                "",
                "",
                None,
                Some(&error),
            );
            emit_error(&app, conversation_id, &error.to_string());
            return;
        }
    };

    let mut provider = EndpointTurnProvider {
        endpoint: prepared.endpoint,
        model_id: prepared.model_id,
        params: prepared.chat_params,
        timeouts: prepared.timeouts,
    };
    let sink = AgentRunSink { app: app.clone() };
    let store = database.agent_store();
    let request = CycleRequest {
        conversation_id,
        request_key: format!("agent-{}", assistant_message.id),
        messages,
        model_supports_tools: prepared.supports_tool_calling,
    };

    let cycle = run_agent_cycle(&store, request, &mut provider, &sink);
    tokio::pin!(cycle);
    let mut cancellation_check = tokio::time::interval(std::time::Duration::from_millis(100));
    let outcome = loop {
        tokio::select! {
            outcome = &mut cycle => break outcome,
            _ = cancellation_check.tick() => {
                match store.execution_run(conversation_id) {
                    Ok(Some(snapshot)) if snapshot.run.state == RunState::Cancelled => {
                        break CycleOutcome::AlreadyFinished { run_state: RunState::Cancelled };
                    }
                    Err(_) => break CycleOutcome::Failed { code: "storage_failed".into(), detail: "execution state is unavailable".into() },
                    _ => {}
                }
            }
        }
    };

    if let Err(error) =
        bridge_final_answer(&database, conversation_id, assistant_message.id, &outcome)
    {
        emit_error(&app, conversation_id, &error.to_string());
    }

    let _ = app.emit(
        AGENT_RUN_EVENT,
        AgentRunEvent::Completed {
            conversation_id,
            outcome,
        },
    );
}

fn build_messages(
    database: &Database,
    conversation_id: i64,
    exclude_id: i64,
) -> FoundryResult<Vec<serde_json::Value>> {
    database
        .messages_for_prompt(conversation_id, exclude_id)?
        .into_iter()
        .map(|message| {
            Ok(json!({
                "role": match message.role {
                    MessageRole::User => "user",
                    MessageRole::Assistant => "assistant",
                    MessageRole::Divider => "system",
                },
                "content": message.content,
            }))
        })
        .collect()
}

fn bridge_final_answer(
    database: &Database,
    conversation_id: i64,
    message_id: i64,
    outcome: &CycleOutcome,
) -> FoundryResult<()> {
    let run_id = database
        .agent_store()
        .execution_run(conversation_id)
        .map_err(store_error)?
        .filter(|snapshot| snapshot.run.request_key == format!("agent-{message_id}"))
        .map(|snapshot| snapshot.run.id);
    let answer = database
        .agent_store()
        .history(conversation_id)
        .map_err(store_error)?
        .into_iter()
        .rev()
        .filter(|entry| run_id.is_some() && entry.run_id == run_id)
        .find_map(|entry| match entry.content {
            HistoryContent::Text {
                role: MessageRole::Assistant,
                content,
                reasoning,
            } if !content.trim().is_empty() => Some((content, reasoning)),
            _ => None,
        });
    let (content, reasoning) = answer.unwrap_or_else(|| (String::new(), String::new()));
    let status = match outcome {
        CycleOutcome::Completed { .. } => MessageStatus::Complete,
        CycleOutcome::AlreadyFinished {
            run_state: RunState::Cancelled,
        } => MessageStatus::Cancelled,
        CycleOutcome::AwaitingAuthorization { .. } | CycleOutcome::AlreadyFinished { .. } => {
            MessageStatus::Interrupted
        }
        _ => MessageStatus::Error,
    };
    database.finish_message(message_id, status, &content, &reasoning, None, None)?;
    Ok(())
}

fn emit_error(app: &AppHandle, conversation_id: i64, message: &str) {
    let _ = app.emit(
        AGENT_RUN_EVENT,
        AgentRunEvent::Error {
            conversation_id,
            message: message.to_string(),
        },
    );
}

#[tauri::command]
pub fn agent_run_cancel(
    state: State<'_, AppState>,
    conversation_id: i64,
    run_id: i64,
) -> FoundryResult<CancelReceipt> {
    let database = state.database()?;
    cancel_execution(&database, conversation_id, run_id)
}

fn cancel_execution(
    database: &Database,
    conversation_id: i64,
    run_id: i64,
) -> FoundryResult<CancelReceipt> {
    let store = database.agent_store();
    let run = store.run(run_id).map_err(store_error)?;
    if run.conversation_id != conversation_id {
        return Err(store_error(AgentStoreError::Conflict));
    }
    let receipt = store.cancel_run(run.id).map_err(|error| match error {
        AgentStoreError::InvalidTransition => FoundryError::CommandFailed {
            command: "agent_run_cancel".to_string(),
            message: "this run cannot be cancelled in its current state".to_string(),
            hint: None,
        },
        other => store_error(other),
    })?;
    Ok(receipt)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionStatus {
    pub conversation_id: i64,
    pub mode: AgentMode,
    pub context_revision: i64,
    pub authorization_revision: i64,
    pub mode_revision: i64,
    pub execution: Option<ExecutionRun>,
    pub blocker: Option<ActiveRun>,
    pub blocker_title: Option<String>,
    pub pending_requests: Vec<AuthorizationRequestView>,
    pub continuation_available: bool,
}

#[tauri::command]
pub fn agent_execution_status(
    state: State<'_, AppState>,
    conversation_id: i64,
) -> AgentCommandResult<ExecutionStatus> {
    let database = state.database().map_err(|_| AgentCommandError::NotReady)?;
    let store = database.agent_store();
    let revision = store.authorization_revision(conversation_id)?;
    let blocker = store.active_run_for_conversation(conversation_id)?;
    let blocker_title = blocker
        .as_ref()
        .and_then(|run| database.conversation(run.conversation_id).ok())
        .map(|conversation| conversation.title);
    let execution = store.execution_run(conversation_id)?;
    let pending_requests = store
        .list_requests(conversation_id)?
        .into_iter()
        .filter(|request| {
            request.status == AuthorizationStatus::Pending
                && execution
                    .as_ref()
                    .is_some_and(|execution| execution.run.id == request.run_id)
        })
        .map(Into::into)
        .collect();
    let continuation_available = execution.as_ref().is_some_and(|execution| {
        matches!(execution.run.state, RunState::Prepared | RunState::Running)
    }) && state
        .generations
        .with_generation_barrier(|ids| ids.contains(&conversation_id));
    Ok(ExecutionStatus {
        conversation_id,
        mode: revision.mode(),
        context_revision: revision.key().context_revision,
        authorization_revision: revision.key().authorization_revision,
        mode_revision: revision.key().mode_revision,
        execution,
        blocker,
        blocker_title,
        pending_requests,
        continuation_available,
    })
}

#[tauri::command]
pub fn agent_execution_mode(
    state: State<'_, AppState>,
    conversation_id: i64,
    mode: AgentMode,
    context_revision: i64,
    authorization_revision: i64,
    mode_revision: i64,
) -> AgentCommandResult<()> {
    let database = state.database().map_err(|_| AgentCommandError::NotReady)?;
    let store = database.agent_store();
    let mut key = store.authorization_revision(conversation_id)?.key().clone();
    key.context_revision = context_revision;
    key.authorization_revision = authorization_revision;
    key.mode_revision = mode_revision;
    store.set_authorization_mode(&key, mode)?;
    Ok(())
}

fn store_error(error: AgentStoreError) -> FoundryError {
    FoundryError::Storage {
        detail: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cancellation_cannot_target_another_conversation() {
        let database = Database::open_in_memory().unwrap();
        let first = database.create_conversation("fixture").unwrap();
        let second = database.create_conversation("fixture").unwrap();
        let run = database.agent_store().start_run(second.id, "two").unwrap();
        assert!(cancel_execution(&database, first.id, run.id).is_err());
        assert_eq!(
            database.agent_store().run(run.id).unwrap().state,
            RunState::Prepared
        );
        assert_eq!(
            cancel_execution(&database, second.id, run.id)
                .unwrap()
                .run_id,
            run.id
        );
    }

    #[test]
    fn a_failed_reply_never_copies_an_answer_from_a_previous_run() {
        let database = Database::open_in_memory().unwrap();
        let conversation = database.create_conversation("fixture").unwrap();
        let previous = database
            .agent_store()
            .start_run(conversation.id, "old")
            .unwrap();
        database
            .agent_store()
            .record_text(previous.id, MessageRole::Assistant, "old answer", "")
            .unwrap();
        database.agent_store().finish_run(previous.id).unwrap();
        let reply = database
            .append_message(
                conversation.id,
                MessageRole::Assistant,
                "",
                "",
                None,
                MessageStatus::Streaming,
            )
            .unwrap();
        let run = database
            .agent_store()
            .start_run(conversation.id, &format!("agent-{}", reply.id))
            .unwrap();
        database.agent_store().fail_run(run.id, None).unwrap();
        bridge_final_answer(
            &database,
            conversation.id,
            reply.id,
            &CycleOutcome::Failed {
                code: "fixture".into(),
                detail: "failed".into(),
            },
        )
        .unwrap();
        let reply = database.message(reply.id).unwrap();
        assert!(reply.content.is_empty());
        assert_eq!(reply.status, MessageStatus::Error);
    }

    #[test]
    fn a_cancelled_reply_is_not_marked_complete() {
        let database = Database::open_in_memory().unwrap();
        let conversation = database.create_conversation("fixture").unwrap();
        let reply = database
            .append_message(
                conversation.id,
                MessageRole::Assistant,
                "",
                "",
                None,
                MessageStatus::Streaming,
            )
            .unwrap();
        let run = database
            .agent_store()
            .start_run(conversation.id, &format!("agent-{}", reply.id))
            .unwrap();
        database.agent_store().cancel_run(run.id).unwrap();
        bridge_final_answer(
            &database,
            conversation.id,
            reply.id,
            &CycleOutcome::AlreadyFinished {
                run_state: RunState::Cancelled,
            },
        )
        .unwrap();
        assert_eq!(
            database.message(reply.id).unwrap().status,
            MessageStatus::Cancelled
        );
    }
}
