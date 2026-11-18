use serde::Serialize;
use serde::ser::{SerializeStruct, Serializer};
use std::sync::Arc;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::{DialogExt, FilePath};

use super::AppState;
use super::db::agent::{
    AgentMode, AgentStoreError, ConversationContext, ProjectAvailability, ProjectConversation,
    ProjectRecord, StorageFailureKind,
};
use super::db::{Conversation, Database};
use super::resources::{ResourceError, ResourceErrorKind, RootHandle};

pub type ProjectCommandResult<T> = Result<T, ProjectCommandError>;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProjectCommandError {
    #[error("invalid project input")]
    InvalidInput,
    #[error("project record not found")]
    NotFound,
    #[error("project folder conflicts with an existing binding")]
    Conflict,
    #[error("project binding changed while the selection was pending")]
    StaleContext,
    #[error("project has unfinished or uncertain conversations")]
    Busy,
    #[error("project storage operation failed: {kind:?} (SQLite code: {sqlite_code:?})")]
    Storage {
        kind: Option<StorageFailureKind>,
        sqlite_code: Option<i32>,
    },
    #[error("project folder is unavailable: {kind:?}")]
    FolderUnavailable {
        kind: ResourceErrorKind,
        raw_os_error: Option<i32>,
    },
    #[error("invalid native project folder selection")]
    InvalidSelection,
    #[error("native project folder selection did not finish")]
    DialogUnavailable,
    #[error("project storage is not ready")]
    NotReady,
}

impl ProjectCommandError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::InvalidInput => "ProjectInvalidInput",
            Self::NotFound => "ProjectNotFound",
            Self::Conflict => "ProjectConflict",
            Self::StaleContext => "ProjectStaleContext",
            Self::Busy => "ProjectBusy",
            Self::Storage { .. } => "ProjectStorage",
            Self::FolderUnavailable { .. } => "ProjectFolderUnavailable",
            Self::InvalidSelection => "ProjectInvalidSelection",
            Self::DialogUnavailable => "ProjectDialogUnavailable",
            Self::NotReady => "ProjectNotReady",
        }
    }
}

impl From<AgentStoreError> for ProjectCommandError {
    fn from(error: AgentStoreError) -> Self {
        match error {
            AgentStoreError::InvalidInput => Self::InvalidInput,
            AgentStoreError::NotFound => Self::NotFound,
            AgentStoreError::Conflict => Self::Conflict,
            AgentStoreError::StaleContext => Self::StaleContext,
            AgentStoreError::ConversationBusy
            | AgentStoreError::InvalidTransition
            | AgentStoreError::AlreadyClaimed => Self::Busy,
            AgentStoreError::Storage => Self::Storage {
                kind: None,
                sqlite_code: None,
            },
            AgentStoreError::Database { kind, sqlite_code } => Self::Storage {
                kind: Some(kind),
                sqlite_code,
            },
        }
    }
}

impl From<ResourceError> for ProjectCommandError {
    fn from(error: ResourceError) -> Self {
        Self::FolderUnavailable {
            kind: error.kind,
            raw_os_error: error.raw_os_error,
        }
    }
}

impl Serialize for ProjectCommandError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let detail = match self {
            Self::Storage { kind, sqlite_code } => {
                Some(format!("kind:{kind:?};sqlite:{sqlite_code:?}"))
            }
            Self::FolderUnavailable { kind, raw_os_error } => {
                Some(format!("kind:{kind:?};errno:{raw_os_error:?}"))
            }
            _ => None,
        };
        let mut state = serializer.serialize_struct("ProjectCommandError", 4)?;
        state.serialize_field("code", self.code())?;
        state.serialize_field("messageKey", &format!("errors.{}", self.code()))?;
        state.serialize_field("message", &self.to_string())?;
        state.serialize_field("detail", &detail)?;
        state.end()
    }
}

pub fn selected_project_root(
    selection: Option<FilePath>,
) -> ProjectCommandResult<Option<RootHandle>> {
    selection
        .map(|selection| {
            if let FilePath::Url(url) = &selection
                && (url.scheme() != "file"
                    || url.query().is_some()
                    || url.fragment().is_some()
                    || !url.username().is_empty()
                    || url.password().is_some())
            {
                return Err(invalid_selection());
            }
            let path = selection
                .into_path()
                .map_err(|_| ProjectCommandError::InvalidSelection)?;
            if !path.is_absolute()
                || path
                    .to_str()
                    .is_none_or(|value| value.chars().any(char::is_control))
            {
                return Err(invalid_selection());
            }
            RootHandle::enroll(&path).map_err(Into::into)
        })
        .transpose()
}

fn invalid_selection() -> ProjectCommandError {
    ProjectCommandError::InvalidSelection
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectView {
    pub id: i64,
    pub name: String,
    pub folder_path: String,
    pub binding_revision: i64,
    pub availability: ProjectAvailability,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectContextView {
    pub conversation_id: i64,
    pub project_id: Option<i64>,
    pub revision: i64,
    pub mode: AgentMode,
}

impl From<ConversationContext> for ProjectContextView {
    fn from(context: ConversationContext) -> Self {
        Self {
            conversation_id: context.conversation_id,
            project_id: context.project_id,
            revision: context.revision,
            mode: context.mode,
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectConversationView {
    pub conversation: Conversation,
    pub context: ProjectContextView,
}

impl From<ProjectConversation> for ProjectConversationView {
    fn from(value: ProjectConversation) -> Self {
        Self {
            conversation: value.conversation,
            context: value.context.into(),
        }
    }
}

fn project_database(state: &AppState) -> ProjectCommandResult<Arc<Database>> {
    state.database().map_err(|_| ProjectCommandError::NotReady)
}

impl From<ProjectRecord> for ProjectView {
    fn from(project: ProjectRecord) -> Self {
        let availability = project.availability();
        Self {
            id: project.id,
            name: project.name,
            folder_path: project.display_path,
            binding_revision: project.binding_revision,
            availability,
        }
    }
}

async fn pick_native_root(app: &AppHandle) -> ProjectCommandResult<Option<RootHandle>> {
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog().file().pick_folder(move |selection| {
        let _ = sender.send(selection);
    });
    let selection = receiver
        .await
        .map_err(|_| ProjectCommandError::DialogUnavailable)?;
    selected_project_root(selection)
}

#[tauri::command]
pub async fn projects_pick_create(
    app: AppHandle,
    state: State<'_, AppState>,
    name: Option<String>,
) -> ProjectCommandResult<Option<ProjectView>> {
    let Some(root) = pick_native_root(&app).await? else {
        return Ok(None);
    };
    let database = project_database(&state)?;
    let project = database
        .agent_store()
        .create_project(&root, name.as_deref())?;
    Ok(Some(project.into()))
}

#[tauri::command]
pub fn projects_list(state: State<'_, AppState>) -> ProjectCommandResult<Vec<ProjectView>> {
    let database = project_database(&state)?;
    Ok(database
        .agent_store()
        .list_projects()?
        .into_iter()
        .map(Into::into)
        .collect())
}

#[tauri::command]
pub fn projects_get(state: State<'_, AppState>, id: i64) -> ProjectCommandResult<ProjectView> {
    project_database(&state)?
        .agent_store()
        .project(id)
        .map(Into::into)
        .map_err(Into::into)
}

#[tauri::command]
pub fn projects_rename(
    state: State<'_, AppState>,
    id: i64,
    name: String,
) -> ProjectCommandResult<ProjectView> {
    project_database(&state)?
        .agent_store()
        .rename_project(id, &name)
        .map(Into::into)
        .map_err(Into::into)
}

#[tauri::command]
pub async fn projects_pick_relocate(
    app: AppHandle,
    state: State<'_, AppState>,
    id: i64,
) -> ProjectCommandResult<Option<ProjectView>> {
    let database = project_database(&state)?;
    let expected = database.agent_store().project(id)?;
    let Some(root) = pick_native_root(&app).await? else {
        return Ok(None);
    };
    let project = database
        .agent_store()
        .relocate_project_if_current(&expected, &root)?;
    Ok(Some(project.into()))
}

#[tauri::command]
pub fn projects_create_conversation(
    state: State<'_, AppState>,
    project_id: Option<i64>,
    model_alias: String,
) -> ProjectCommandResult<ProjectConversationView> {
    project_database(&state)?
        .agent_store()
        .create_project_conversation(project_id, &model_alias)
        .map(Into::into)
        .map_err(Into::into)
}

#[tauri::command]
pub fn projects_list_conversations(
    state: State<'_, AppState>,
    project_id: Option<i64>,
    archived: bool,
) -> ProjectCommandResult<Vec<ProjectConversationView>> {
    project_database(&state)?
        .agent_store()
        .list_project_conversations(project_id, archived)
        .map(|values| values.into_iter().map(Into::into).collect())
        .map_err(Into::into)
}
