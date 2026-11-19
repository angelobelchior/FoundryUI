use std::sync::Arc;

use serde::Serialize;
use serde_json::json;
use tauri::{AppHandle, Emitter, State};

use super::agent_runtime::{
    CycleEvent, CycleOutcome, CycleRequest, CycleSink, EndpointTurnProvider, run_agent_cycle,
};
use super::app::AppState;
use super::backend::FoundryBackend;
use super::catalog;
use super::chat::{ChatParams, GenerationStart, SendReceipt};
use super::db::agent::{AgentStoreError, HistoryContent};
use super::db::{ChatMessage, Conversation, Database, MessageRole, MessageStatus};
use super::error::{FoundryError, FoundryResult};
use super::http::StreamTimeouts;
use super::local_inference::VerifiedLocalEndpoint;
use super::params;

const AGENT_RUN_EVENT: &str = "agent-run-event";

#[derive(Debug, Clone, Serialize)]
#[serde(
    tag = "type",
    rename_all = "camelCase",
    rename_all_fields = "camelCase"
)]
pub enum AgentRunEvent {
    Cycle { event: CycleEvent },
    Completed { outcome: CycleOutcome },
    Error { message: String },
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

    let task_user = user_message.clone();
    let task_assistant = assistant_message.clone();
    let task_conversation = conversation.clone();
    let app_for_task = app.clone();
    tauri::async_runtime::spawn(async move {
        run_agent_task(
            app_for_task,
            prepared,
            start,
            task_user,
            task_assistant,
            task_conversation,
        )
        .await;
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
    user_message: ChatMessage,
    assistant_message: ChatMessage,
    conversation: Conversation,
) {
    let _generation = start;
    let _ = user_message;
    let _ = conversation;
    let database = prepared.database;
    let conversation_id = assistant_message.conversation_id;

    let messages = match build_messages(&database, conversation_id, assistant_message.id) {
        Ok(messages) => messages,
        Err(error) => {
            emit_error(&app, &error.to_string());
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

    let outcome = run_agent_cycle(&store, request, &mut provider, &sink).await;

    bridge_final_answer(&database, conversation_id, assistant_message.id);

    let _ = app.emit(AGENT_RUN_EVENT, AgentRunEvent::Completed { outcome });
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

fn bridge_final_answer(database: &Database, conversation_id: i64, message_id: i64) {
    let answer = match database.agent_store().history(conversation_id) {
        Ok(entries) => entries
            .into_iter()
            .rev()
            .find_map(|entry| match entry.content {
                HistoryContent::Text {
                    role: MessageRole::Assistant,
                    content,
                    reasoning,
                } if !content.trim().is_empty() => Some((content, reasoning)),
                _ => None,
            }),
        Err(_) => None,
    };
    let (content, reasoning) = answer.unwrap_or_else(|| (String::new(), String::new()));
    let _ = database.finish_message(
        message_id,
        MessageStatus::Complete,
        &content,
        &reasoning,
        None,
        None,
    );
}

fn emit_error(app: &AppHandle, message: &str) {
    let _ = app.emit(
        AGENT_RUN_EVENT,
        AgentRunEvent::Error {
            message: message.to_string(),
        },
    );
}

#[tauri::command]
pub fn agent_run_cancel(
    state: State<'_, AppState>,
    conversation_id: i64,
) -> FoundryResult<Option<i64>> {
    let database = state.database()?;
    let store = database.agent_store();
    let run = store
        .active_run_for_conversation(conversation_id)
        .map_err(store_error)?;
    let Some(run) = run else {
        return Ok(None);
    };
    let receipt = store.cancel_run(run.run_id).map_err(|error| match error {
        AgentStoreError::InvalidTransition => FoundryError::CommandFailed {
            command: "agent_run_cancel".to_string(),
            message: "this run cannot be cancelled in its current state".to_string(),
            hint: None,
        },
        other => store_error(other),
    })?;
    Ok(Some(receipt.run_id))
}

fn store_error(error: AgentStoreError) -> FoundryError {
    FoundryError::Storage {
        detail: error.to_string(),
    }
}
