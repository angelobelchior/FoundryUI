use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::{MessageDialogButtons, MessageDialogResult};

use super::AppState;
use super::agent_commands::{AgentCommandError, AgentCommandResult, native_choice, single_line};
use super::db::agent::{
    ActionOutcome, AgentEventState, AgentStoreError, ConversationSnapshot, HistoryContent,
    HistoryEntry, SnapshotCandidate, SnapshotCapturePlan, SnapshotContentAuthority,
    SnapshotOmission, SnapshotOrigin, SnapshotRemovalPlan,
};
use super::db::{Database, MessageRole};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotOriginView {
    pub conversation_id: i64,
    pub title: String,
    pub project_id: Option<i64>,
    pub project_name: Option<String>,
}

impl From<SnapshotOrigin> for SnapshotOriginView {
    fn from(origin: SnapshotOrigin) -> Self {
        Self {
            conversation_id: origin.conversation_id,
            title: origin.title,
            project_id: origin.project_id,
            project_name: origin.project_name,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum HistoricalContentView {
    HistoricalText {
        role: MessageRole,
        content: String,
        reasoning: String,
    },
    HistoricalToolCall {
        source_call_id: String,
        source_tool: String,
        arguments: serde_json::Value,
    },
    HistoricalToolResult {
        source_call_id: String,
        outcome: ActionOutcome,
        summary: serde_json::Value,
    },
    HistoricalStateEvent {
        event: AgentEventState,
        source_action_id: Option<i64>,
    },
}

impl From<HistoryContent> for HistoricalContentView {
    fn from(content: HistoryContent) -> Self {
        match content {
            HistoryContent::Text {
                role,
                content,
                reasoning,
            } => Self::HistoricalText {
                role,
                content,
                reasoning,
            },
            HistoryContent::ToolCall {
                call_id,
                tool,
                arguments,
            } => Self::HistoricalToolCall {
                source_call_id: call_id,
                source_tool: tool,
                arguments,
            },
            HistoryContent::ToolResult {
                call_id,
                outcome,
                summary,
            } => Self::HistoricalToolResult {
                source_call_id: call_id,
                outcome,
                summary,
            },
            HistoryContent::StateEvent { event, action_id } => Self::HistoricalStateEvent {
                event,
                source_action_id: action_id,
            },
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HistoricalEntryView {
    pub sequence: i64,
    pub source_run_id: Option<i64>,
    pub content: HistoricalContentView,
}

impl From<HistoryEntry> for HistoricalEntryView {
    fn from(entry: HistoryEntry) -> Self {
        Self {
            sequence: entry.sequence,
            source_run_id: entry.run_id,
            content: entry.content.into(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(
    tag = "kind",
    rename_all = "snake_case",
    rename_all_fields = "camelCase"
)]
pub enum SnapshotOmissionView {
    UnsupportedAttachment {
        source_sequence: i64,
        media_kind: String,
        mime: String,
    },
    NestedConversationCopies {
        count: i64,
    },
    PrivateContextFacts {
        count: i64,
    },
}

impl From<SnapshotOmission> for SnapshotOmissionView {
    fn from(omission: SnapshotOmission) -> Self {
        match omission {
            SnapshotOmission::UnsupportedAttachment {
                source_sequence,
                media_kind,
                mime,
            } => Self::UnsupportedAttachment {
                source_sequence,
                media_kind,
                mime,
            },
            SnapshotOmission::NestedConversationCopies { count } => {
                Self::NestedConversationCopies { count }
            }
            SnapshotOmission::PrivateContextFacts { count } => Self::PrivateContextFacts { count },
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotView {
    pub id: i64,
    pub destination_conversation_id: i64,
    pub source_conversation_id: i64,
    pub source_title: String,
    pub captured_at: i64,
    pub read_only: bool,
    pub reference_only: bool,
    pub content_authority: SnapshotContentAuthority,
    pub origin: SnapshotOriginView,
    pub crossed_project_boundary: bool,
    pub source_may_be_partial: bool,
    pub omitted_attachment_count: i64,
    pub omissions: Vec<SnapshotOmissionView>,
    pub entries: Vec<HistoricalEntryView>,
}

impl From<ConversationSnapshot> for SnapshotView {
    fn from(snapshot: ConversationSnapshot) -> Self {
        Self {
            id: snapshot.id,
            destination_conversation_id: snapshot.destination_conversation_id,
            source_conversation_id: snapshot.source_conversation_id,
            source_title: snapshot.source_title,
            captured_at: snapshot.captured_at,
            read_only: true,
            reference_only: true,
            content_authority: SnapshotContentAuthority::HistoricalEvidence,
            origin: snapshot.origin.into(),
            crossed_project_boundary: snapshot.crossed_project_boundary,
            source_may_be_partial: snapshot.source_may_be_partial,
            omitted_attachment_count: snapshot.omitted_attachment_count,
            omissions: snapshot.omissions.into_iter().map(Into::into).collect(),
            entries: snapshot.entries.into_iter().map(Into::into).collect(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SnapshotCandidateView {
    pub origin: SnapshotOriginView,
    pub archived: bool,
    pub updated_at: i64,
}

impl From<SnapshotCandidate> for SnapshotCandidateView {
    fn from(candidate: SnapshotCandidate) -> Self {
        Self {
            origin: candidate.origin.into(),
            archived: candidate.archived,
            updated_at: candidate.updated_at,
        }
    }
}

fn database(state: &AppState) -> AgentCommandResult<std::sync::Arc<Database>> {
    state.database().map_err(|_| AgentCommandError::NotReady)
}

pub fn commit_selected_snapshot(
    state: &AppState,
    plan: &SnapshotCapturePlan,
) -> AgentCommandResult<SnapshotView> {
    let database = database(state)?;
    state
        .generations
        .with_idle_conversations(&[plan.destination_conversation_id()], || {
            database.agent_store().commit_snapshot_capture(plan)
        })
        .ok_or(AgentStoreError::ConversationBusy)?
        .map(Into::into)
        .map_err(Into::into)
}

pub fn remove_selected_snapshot(
    state: &AppState,
    plan: &SnapshotRemovalPlan,
) -> AgentCommandResult<()> {
    let database = database(state)?;
    state
        .generations
        .with_idle_conversations(&[plan.destination_conversation_id()], || {
            database.agent_store().commit_snapshot_removal(plan)
        })
        .ok_or(AgentStoreError::ConversationBusy)?
        .map_err(Into::into)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct SnapshotMessages {
    capture_title: String,
    remove_title: String,
    attach: String,
    remove: String,
    cancel: String,
    source: String,
    destination: String,
    standalone: String,
    reference_notice: String,
    capture_notice: String,
    remove_notice: String,
}

fn messages(language: Option<&str>) -> AgentCommandResult<SnapshotMessages> {
    let mut catalog: std::collections::HashMap<String, SnapshotMessages> =
        serde_json::from_str(include_str!("../../snapshot-messages.json"))
            .map_err(|_| AgentCommandError::NotReady)?;
    catalog
        .remove(match language {
            Some("pt-BR") => "pt-BR",
            Some("es") => "es",
            _ => "en",
        })
        .ok_or(AgentCommandError::NotReady)
}

fn origin_label(origin: &SnapshotOrigin, text: &SnapshotMessages) -> String {
    let project = match (origin.project_id, origin.project_name.as_deref()) {
        (Some(id), Some(name)) => format!("{} (#{id})", single_line(name)),
        _ => text.standalone.clone(),
    };
    format!(
        "#{} — {} ({project})",
        origin.conversation_id,
        single_line(&origin.title)
    )
}

#[tauri::command]
pub fn snapshots_candidates(
    state: State<'_, AppState>,
    project_id: Option<i64>,
    query: Option<String>,
) -> AgentCommandResult<Vec<SnapshotCandidateView>> {
    database(&state)?
        .agent_store()
        .snapshot_candidates(project_id, query.as_deref())
        .map(|items| items.into_iter().map(Into::into).collect())
        .map_err(Into::into)
}

#[tauri::command]
pub fn snapshots_list(
    state: State<'_, AppState>,
    destination_id: i64,
) -> AgentCommandResult<Vec<SnapshotView>> {
    database(&state)?
        .agent_store()
        .list_snapshots(destination_id)
        .map(|items| items.into_iter().map(Into::into).collect())
        .map_err(Into::into)
}

#[tauri::command]
pub fn snapshots_get(
    state: State<'_, AppState>,
    destination_id: i64,
    id: i64,
) -> AgentCommandResult<SnapshotView> {
    database(&state)?
        .agent_store()
        .snapshot(destination_id, id)
        .map(Into::into)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn snapshots_capture(
    app: AppHandle,
    state: State<'_, AppState>,
    source_id: i64,
    destination_id: i64,
    language: Option<String>,
) -> AgentCommandResult<Option<SnapshotView>> {
    let database = database(&state)?;
    let plan = database
        .agent_store()
        .prepare_snapshot_capture(source_id, destination_id)?;
    if plan.crossed_project_boundary() {
        let text = messages(language.as_deref())?;
        let body = format!(
            "{}: {}\n{}: {}\n\n{}\n\n{}",
            text.source,
            origin_label(plan.source_origin(), &text),
            text.destination,
            origin_label(plan.destination_origin(), &text),
            text.reference_notice,
            text.capture_notice
        );
        let answer = native_choice(
            &app,
            &text.capture_title,
            body,
            MessageDialogButtons::OkCancelCustom(text.attach.clone(), text.cancel),
        )
        .await?;
        if !matches!(answer, MessageDialogResult::Custom(label) if label == text.attach) {
            return Ok(None);
        }
    }
    commit_selected_snapshot(&state, &plan).map(Some)
}

#[tauri::command]
pub async fn snapshots_remove(
    app: AppHandle,
    state: State<'_, AppState>,
    destination_id: i64,
    id: i64,
    language: Option<String>,
) -> AgentCommandResult<bool> {
    let database = database(&state)?;
    let plan = database
        .agent_store()
        .prepare_snapshot_removal(destination_id, id)?;
    let text = messages(language.as_deref())?;
    let destination = database
        .conversation(destination_id)
        .map_err(|_| AgentCommandError::Store(AgentStoreError::NotFound))?;
    let body = format!(
        "{}: {}\n{}: #{} — {}\n\n{}",
        text.source,
        origin_label(plan.source_origin(), &text),
        text.destination,
        destination_id,
        single_line(&destination.title),
        text.remove_notice
    );
    let answer = native_choice(
        &app,
        &text.remove_title,
        body,
        MessageDialogButtons::OkCancelCustom(text.remove.clone(), text.cancel),
    )
    .await?;
    if !matches!(answer, MessageDialogResult::Custom(label) if label == text.remove) {
        return Ok(false);
    }
    remove_selected_snapshot(&state, &plan)?;
    Ok(true)
}
