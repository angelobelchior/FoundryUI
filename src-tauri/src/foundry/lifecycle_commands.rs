use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::{
    DialogExt, MessageDialogButtons, MessageDialogKind, MessageDialogResult,
};

use super::AppState;
use super::db::Database;
use super::db::agent::{ContextCyclePlan, ProjectRemovalMode};
use super::error::{FoundryError, FoundryResult};
use super::project_commands::{ProjectCommandError, ProjectCommandResult, ProjectContextView};

static REQUEST_COUNTER: AtomicU64 = AtomicU64::new(1);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextCycleView {
    pub contexts: Vec<ProjectContextView>,
    pub removed_project_id: Option<i64>,
    pub deleted_conversation_ids: Vec<i64>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Messages {
    move_title: String,
    move_accept: String,
    move_notice: String,
    remove_project_title: String,
    keep_label: String,
    delete_label: String,
    cancel_label: String,
    folder_notice: String,
    keep_notice: String,
    delete_notice: String,
    recovery_notice: String,
    remove_conversation_title: String,
    delete_conversation_label: String,
    source_label: String,
    destination_label: String,
    standalone_label: String,
    affected_label: String,
}

fn messages(language: Option<&str>) -> ProjectCommandResult<Messages> {
    let mut catalog: HashMap<String, Messages> =
        serde_json::from_str(include_str!("../../lifecycle-messages.json"))
            .map_err(|_| ProjectCommandError::NotReady)?;
    let locale = match language {
        Some("pt-BR") => "pt-BR",
        Some("es") => "es",
        _ => "en",
    };
    catalog.remove(locale).ok_or(ProjectCommandError::NotReady)
}

fn request_key() -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    format!(
        "native-cycle-{}-{nanos}-{}",
        std::process::id(),
        REQUEST_COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

fn sanitized(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_control()
                || matches!(character, '\u{200e}' | '\u{200f}' | '\u{2028}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
            {
                ' '
            } else {
                character
            }
        })
        .collect()
}

fn project_label(
    database: &Database,
    plan: &ContextCyclePlan,
    project_id: Option<i64>,
    text: &Messages,
) -> ProjectCommandResult<String> {
    let Some(id) = project_id else {
        return Ok(text.standalone_label.clone());
    };
    let project = database.agent_store().project(id)?;
    if plan.project_binding_revision(id) != Some(project.binding_revision) {
        return Err(ProjectCommandError::StaleContext);
    }
    Ok(format!(
        "{} (#{id}) — {}",
        sanitized(&project.name),
        sanitized(&project.display_path),
    ))
}

fn roster(database: &Database, plan: &ContextCyclePlan) -> ProjectCommandResult<String> {
    plan.conversations()
        .iter()
        .map(|context| {
            let conversation = database
                .conversation(context.conversation_id)
                .map_err(|_| ProjectCommandError::Storage {
                    kind: None,
                    sqlite_code: None,
                })?;
            Ok(format!(
                "#{} — {}",
                context.conversation_id,
                sanitized(&conversation.title)
            ))
        })
        .collect::<ProjectCommandResult<Vec<_>>>()
        .map(|lines| lines.join("\n"))
}

fn conversation_ids(plan: &ContextCyclePlan) -> Vec<i64> {
    plan.conversations()
        .iter()
        .map(|context| context.conversation_id)
        .collect()
}

fn ensure_idle(state: &AppState, plan: &ContextCyclePlan) -> ProjectCommandResult<()> {
    state
        .generations
        .with_idle_conversations(&conversation_ids(plan), || ())
        .ok_or(ProjectCommandError::Busy)
}

pub fn commit_idle_context_cycle(
    state: &AppState,
    plan: &ContextCyclePlan,
) -> ProjectCommandResult<ContextCycleView> {
    let database = state
        .database()
        .map_err(|_| ProjectCommandError::NotReady)?;
    let result = state
        .generations
        .with_idle_conversations(&conversation_ids(plan), || {
            database.agent_store().commit_context_cycle(plan)
        })
        .ok_or(ProjectCommandError::Busy)??;

    Ok(ContextCycleView {
        contexts: result.contexts().iter().cloned().map(Into::into).collect(),
        removed_project_id: result.removed_project_id(),
        deleted_conversation_ids: result.deleted_conversation_ids().to_vec(),
    })
}

async fn native_dialog(
    app: &AppHandle,
    title: &str,
    body: String,
    buttons: MessageDialogButtons,
) -> ProjectCommandResult<MessageDialogResult> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog()
        .message(body)
        .title(title)
        .kind(MessageDialogKind::Warning)
        .buttons(buttons)
        .show_with_result(move |answer| {
            let _ = sender.send(answer);
        });
    receiver
        .await
        .map_err(|_| ProjectCommandError::DialogUnavailable)
}

async fn confirm(
    app: &AppHandle,
    title: &str,
    body: String,
    accept: &str,
    cancel: &str,
) -> ProjectCommandResult<bool> {
    let answer = native_dialog(
        app,
        title,
        body,
        MessageDialogButtons::OkCancelCustom(accept.to_string(), cancel.to_string()),
    )
    .await?;
    Ok(matches!(answer, MessageDialogResult::Custom(label) if label == accept))
}

#[tauri::command]
pub async fn projects_move_conversation(
    app: AppHandle,
    state: State<'_, AppState>,
    conversation_id: i64,
    destination_project_id: Option<i64>,
    language: Option<String>,
) -> ProjectCommandResult<Option<ContextCycleView>> {
    let text = messages(language.as_deref())?;
    let database = state
        .database()
        .map_err(|_| ProjectCommandError::NotReady)?;
    let plan = database.agent_store().prepare_conversation_move(
        &request_key(),
        conversation_id,
        destination_project_id,
    )?;
    let source = plan
        .conversations()
        .first()
        .ok_or(ProjectCommandError::StaleContext)?
        .project_id;
    let source = project_label(&database, &plan, source, &text)?;
    let destination = project_label(&database, &plan, destination_project_id, &text)?;
    let body = format!(
        "{}: {source}\n{}: {destination}\n\n{}:\n{}\n\n{}\n\n{}",
        text.source_label,
        text.destination_label,
        text.affected_label,
        roster(&database, &plan)?,
        text.move_notice,
        text.folder_notice,
    );
    ensure_idle(&state, &plan)?;
    if !confirm(
        &app,
        &text.move_title,
        body,
        &text.move_accept,
        &text.cancel_label,
    )
    .await?
    {
        return Ok(None);
    }
    commit_idle_context_cycle(&state, &plan).map(Some)
}

#[tauri::command]
pub async fn projects_remove(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
    language: Option<String>,
) -> ProjectCommandResult<Option<ContextCycleView>> {
    let text = messages(language.as_deref())?;
    let database = state
        .database()
        .map_err(|_| ProjectCommandError::NotReady)?;
    let keep = database.agent_store().prepare_project_removal(
        &request_key(),
        id,
        ProjectRemovalMode::Keep,
    )?;
    let delete = database.agent_store().prepare_project_removal(
        &request_key(),
        id,
        ProjectRemovalMode::Delete,
    )?;
    if keep.conversations() != delete.conversations()
        || keep.project_binding_revision(id) != delete.project_binding_revision(id)
    {
        return Err(ProjectCommandError::StaleContext);
    }
    let project = project_label(&database, &keep, Some(id), &text)?;
    let targets = format!(
        "{project}\n\n{}:\n{}",
        text.affected_label,
        roster(&database, &keep)?
    );
    let body = format!(
        "{targets}\n\n{}\n\n{}\n\n{}\n\n{}",
        text.keep_notice, text.delete_notice, text.recovery_notice, text.folder_notice,
    );
    ensure_idle(&state, &keep)?;
    let answer = native_dialog(
        &app,
        &text.remove_project_title,
        body,
        MessageDialogButtons::YesNoCancelCustom(
            text.keep_label.clone(),
            text.delete_label.clone(),
            text.cancel_label.clone(),
        ),
    )
    .await?;
    match answer {
        MessageDialogResult::Custom(label) if label == text.keep_label => {
            commit_idle_context_cycle(&state, &keep).map(Some)
        }
        MessageDialogResult::Custom(label) if label == text.delete_label => {
            ensure_idle(&state, &delete)?;
            let body = format!(
                "{targets}\n\n{}\n\n{}\n\n{}",
                text.delete_notice, text.recovery_notice, text.folder_notice,
            );
            if !confirm(
                &app,
                &text.remove_project_title,
                body,
                &text.delete_label,
                &text.cancel_label,
            )
            .await?
            {
                return Ok(None);
            }
            commit_idle_context_cycle(&state, &delete).map(Some)
        }
        _ => Ok(None),
    }
}

pub async fn delete_conversation_with_confirmation(
    app: &AppHandle,
    state: &AppState,
    id: i64,
    language: Option<&str>,
) -> FoundryResult<()> {
    async fn delete(
        app: &AppHandle,
        state: &AppState,
        id: i64,
        language: Option<&str>,
    ) -> ProjectCommandResult<bool> {
        let text = messages(language)?;
        let database = state
            .database()
            .map_err(|_| ProjectCommandError::NotReady)?;
        let plan = database
            .agent_store()
            .prepare_conversation_deletion(&request_key(), id)?;
        let source = plan
            .conversations()
            .first()
            .ok_or(ProjectCommandError::StaleContext)?
            .project_id;
        let project = project_label(&database, &plan, source, &text)?;
        let body = format!(
            "{}: {project}\n\n{}:\n{}\n\n{}\n\n{}\n\n{}",
            text.source_label,
            text.affected_label,
            roster(&database, &plan)?,
            text.delete_notice,
            text.recovery_notice,
            text.folder_notice,
        );
        ensure_idle(state, &plan)?;
        if !confirm(
            app,
            &text.remove_conversation_title,
            body,
            &text.delete_conversation_label,
            &text.cancel_label,
        )
        .await?
        {
            return Ok(false);
        }
        commit_idle_context_cycle(state, &plan)?;
        Ok(true)
    }

    match delete(app, state, id, language).await {
        Ok(true) => Ok(()),
        Ok(false) => Err(FoundryError::Cancelled),
        Err(error) => Err(FoundryError::Storage {
            detail: error.code().to_string(),
        }),
    }
}
