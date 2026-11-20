use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};
use tauri_plugin_dialog::{MessageDialogButtons, MessageDialogResult};

use super::AppState;
use super::agent_commands::{AgentCommandError, AgentCommandResult, native_choice, single_line};
use super::db::agent::authorization::{
    AuthorizationGrant, AuthorizationRequest, AuthorizationRevision, AuthorizationStatus,
    DecisionReceipt, RequestDecision, RequirementDisplay, RequirementKind, RevocationReceipt,
};
use super::db::{
    Database,
    agent::{AgentMode, AgentStoreError},
};
use super::resources::RootHandle;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RequirementView {
    pub kind: RequirementKind,
    pub target: String,
    pub constraints_fingerprint: String,
    pub remember_eligible: bool,
    pub recovery_guaranteed: bool,
    pub preview_available: bool,
    pub diff: Option<String>,
}

impl From<RequirementDisplay> for RequirementView {
    fn from(display: RequirementDisplay) -> Self {
        Self {
            kind: display.kind,
            target: display.target,
            constraints_fingerprint: display.constraints_fingerprint,
            remember_eligible: display.remember_eligible,
            recovery_guaranteed: display.recovery_guaranteed,
            preview_available: display.preview_available,
            diff: display.diff,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorizationGrantView {
    pub id: i64,
    pub conversation_id: i64,
    pub context_revision: i64,
    pub requirement: RequirementView,
}

impl From<AuthorizationGrant> for AuthorizationGrantView {
    fn from(grant: AuthorizationGrant) -> Self {
        Self {
            id: grant.id,
            conversation_id: grant.conversation_id,
            context_revision: grant.context_revision,
            requirement: grant.requirement.into(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorizationRequestView {
    pub id: i64,
    pub conversation_id: i64,
    pub action_id: i64,
    pub run_id: i64,
    pub context_revision: i64,
    pub authorization_revision: i64,
    pub mode_revision: i64,
    pub status: AuthorizationStatus,
    pub requirements: Vec<RequirementView>,
    pub remembered: bool,
}

impl From<AuthorizationRequest> for AuthorizationRequestView {
    fn from(request: AuthorizationRequest) -> Self {
        Self {
            id: request.id,
            conversation_id: request.conversation_id,
            action_id: request.action_id,
            run_id: request.run_id,
            context_revision: request.context_revision,
            authorization_revision: request.authorization_revision,
            mode_revision: request.mode_revision,
            status: request.status,
            requirements: request.requirements.into_iter().map(Into::into).collect(),
            remembered: request.remembered,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthorizationRevisionView {
    pub conversation_id: i64,
    pub context_revision: i64,
    pub authorization_revision: i64,
    pub mode_revision: i64,
    pub mode: AgentMode,
    pub project_bound: bool,
    pub global_read_allowed: bool,
}

impl From<&AuthorizationRevision> for AuthorizationRevisionView {
    fn from(revision: &AuthorizationRevision) -> Self {
        let key = revision.key();
        Self {
            conversation_id: key.conversation_id,
            context_revision: key.context_revision,
            authorization_revision: key.authorization_revision,
            mode_revision: key.mode_revision,
            mode: revision.mode(),
            project_bound: revision.project_root_id().is_some(),
            global_read_allowed: revision.global_read_allowed(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DecisionReceiptView {
    pub request_id: i64,
    pub status: AuthorizationStatus,
    pub grant_id: Option<i64>,
    pub dispatch_started: bool,
}

impl From<DecisionReceipt> for DecisionReceiptView {
    fn from(receipt: DecisionReceipt) -> Self {
        Self {
            request_id: receipt.request_id,
            status: receipt.status,
            grant_id: receipt.grant_id,
            dispatch_started: receipt.dispatch_started,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RevocationReceiptView {
    pub authorization_revision: i64,
    pub started_or_uncertain_action_ids: Vec<i64>,
    pub automatically_cancelled: bool,
}

impl From<RevocationReceipt> for RevocationReceiptView {
    fn from(receipt: RevocationReceipt) -> Self {
        Self {
            authorization_revision: receipt.authorization_revision,
            started_or_uncertain_action_ids: receipt.started_or_uncertain_action_ids,
            automatically_cancelled: receipt.automatically_cancelled,
        }
    }
}

fn database(state: &AppState) -> AgentCommandResult<std::sync::Arc<Database>> {
    state.database().map_err(|_| AgentCommandError::NotReady)
}

pub fn commit_selected_authorization_root(
    state: &AppState,
    expected: &AuthorizationRevision,
    root: &RootHandle,
) -> AgentCommandResult<AuthorizationGrantView> {
    database(state)?
        .agent_store()
        .grant_authorization_root(expected, root)
        .map(Into::into)
        .map_err(Into::into)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct PermissionMessages {
    root_title: String,
    grant_root: String,
    root_notice: String,
    cancel: String,
    conversation: String,
}

fn messages(language: Option<&str>) -> AgentCommandResult<PermissionMessages> {
    let mut catalog: std::collections::HashMap<String, PermissionMessages> =
        serde_json::from_str(include_str!("../../permission-messages.json"))
            .map_err(|_| AgentCommandError::NotReady)?;
    catalog
        .remove(match language {
            Some("pt-BR") => "pt-BR",
            Some("es") => "es",
            _ => "en",
        })
        .ok_or(AgentCommandError::NotReady)
}

#[tauri::command]
pub fn authorization_revision(
    state: State<'_, AppState>,
    conversation_id: i64,
) -> AgentCommandResult<AuthorizationRevisionView> {
    let revision = database(&state)?
        .agent_store()
        .authorization_revision(conversation_id)?;
    Ok((&revision).into())
}

#[tauri::command]
pub fn authorization_requests(
    state: State<'_, AppState>,
    conversation_id: i64,
) -> AgentCommandResult<Vec<AuthorizationRequestView>> {
    database(&state)?
        .agent_store()
        .list_requests(conversation_id)
        .map(|items| items.into_iter().map(Into::into).collect())
        .map_err(Into::into)
}

#[tauri::command]
pub fn authorization_grants(
    state: State<'_, AppState>,
    conversation_id: i64,
) -> AgentCommandResult<Vec<AuthorizationGrantView>> {
    database(&state)?
        .agent_store()
        .list_grants(conversation_id)
        .map(|items| items.into_iter().map(Into::into).collect())
        .map_err(Into::into)
}

#[tauri::command]
pub fn authorization_revoke(
    state: State<'_, AppState>,
    conversation_id: i64,
    grant_id: i64,
) -> AgentCommandResult<RevocationReceiptView> {
    database(&state)?
        .agent_store()
        .revoke_grant(conversation_id, grant_id)
        .map(Into::into)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn authorization_pick_root(
    app: AppHandle,
    state: State<'_, AppState>,
    conversation_id: i64,
    language: Option<String>,
) -> AgentCommandResult<Option<AuthorizationGrantView>> {
    let database = database(&state)?;
    let expected = database
        .agent_store()
        .authorization_revision(conversation_id)?;
    if expected.project_root_id().is_some() {
        return Err(AgentStoreError::InvalidInput.into());
    }
    let Some(root) = super::project_commands::pick_native_root(&app)
        .await
        .map_err(|_| AgentCommandError::InvalidSelection)?
    else {
        return Ok(None);
    };
    let text = messages(language.as_deref())?;
    let conversation = database
        .conversation(conversation_id)
        .map_err(|_| AgentStoreError::NotFound)?;
    let path = root
        .display_path()
        .to_str()
        .ok_or(AgentCommandError::InvalidSelection)?;
    let body = format!(
        "{}: #{} — {}\n\n{}\n\n{}",
        text.conversation,
        conversation_id,
        single_line(&conversation.title),
        single_line(path),
        text.root_notice
    );
    let answer = native_choice(
        &app,
        &text.root_title,
        body,
        MessageDialogButtons::OkCancelCustom(text.grant_root.clone(), text.cancel),
    )
    .await?;
    if !matches!(answer, MessageDialogResult::Custom(label) if label == text.grant_root) {
        return Ok(None);
    }
    commit_selected_authorization_root(&state, &expected, &root).map(Some)
}

#[tauri::command]
pub fn authorization_decide(
    state: State<'_, AppState>,
    conversation_id: i64,
    request_id: i64,
    decision: RequestDecision,
) -> AgentCommandResult<Option<DecisionReceiptView>> {
    let database = database(&state)?;
    if let RequestDecision::Approve { remember } = decision {
        let request = database
            .agent_store()
            .list_requests(conversation_id)?
            .into_iter()
            .find(|request| request.id == request_id)
            .ok_or(AgentStoreError::NotFound)?;
        if request.status != AuthorizationStatus::Pending {
            return Err(AgentStoreError::InvalidTransition.into());
        }
        if request.requirements.is_empty()
            || request
                .requirements
                .iter()
                .any(|requirement| !requirement.preview_available)
        {
            return Err(AgentCommandError::PreviewUnavailable);
        }
        if remember
            && request
                .requirements
                .iter()
                .any(|requirement| !requirement.remember_eligible)
        {
            return Err(AgentStoreError::InvalidInput.into());
        }
    }
    database
        .agent_store()
        .decide_request(conversation_id, request_id, decision)
        .map(|receipt| Some(receipt.into()))
        .map_err(Into::into)
}

#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryPermissionsView {
    pub read_auto: bool,
    pub edit_auto: bool,
    pub command_auto: bool,
    pub network_auto: bool,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CategoryPermissionsInput {
    pub read_auto: bool,
    pub edit_auto: bool,
    pub command_auto: bool,
    pub network_auto: bool,
}

impl From<crate::foundry::policy::CategoryPermissions> for CategoryPermissionsView {
    fn from(categories: crate::foundry::policy::CategoryPermissions) -> Self {
        Self {
            read_auto: categories.read_auto,
            edit_auto: categories.edit_auto,
            command_auto: categories.command_auto,
            network_auto: categories.network_auto,
        }
    }
}

impl From<CategoryPermissionsInput> for crate::foundry::policy::CategoryPermissions {
    fn from(input: CategoryPermissionsInput) -> Self {
        Self {
            read_auto: input.read_auto,
            edit_auto: input.edit_auto,
            command_auto: input.command_auto,
            network_auto: input.network_auto,
        }
    }
}

#[tauri::command]
pub fn authorization_categories(
    state: State<'_, AppState>,
) -> AgentCommandResult<CategoryPermissionsView> {
    let database = state.database().map_err(|_| AgentCommandError::NotReady)?;
    Ok(database.agent_store().category_permissions()?.into())
}

#[tauri::command]
pub fn authorization_set_categories(
    state: State<'_, AppState>,
    categories: CategoryPermissionsInput,
) -> AgentCommandResult<CategoryPermissionsView> {
    let database = state.database().map_err(|_| AgentCommandError::NotReady)?;
    database
        .agent_store()
        .set_authorization_categories(categories.into())?;
    Ok(database.agent_store().category_permissions()?.into())
}
