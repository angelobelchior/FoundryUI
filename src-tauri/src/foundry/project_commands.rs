use serde::Serialize;
use serde::ser::{SerializeStruct, Serializer};
use std::sync::Arc;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::{
    DialogExt, FilePath, MessageDialogButtons, MessageDialogKind, MessageDialogResult,
};

use super::AppState;
use super::db::agent::{
    AgentMode, AgentStoreError, ConversationContext, ProjectAvailability, ProjectConversation,
    ProjectRecord, StorageFailureKind,
};
use super::db::{Conversation, Database};
pub use super::project_selection::ProjectFolderSelectionView;
use super::project_selection::{
    begin_selected_project, confirm_selected_project, discard_selected_project,
};
use super::resources::{
    DirectoryEntry, DirectoryEntryKind, RelativePath, ResourceError, ResourceErrorKind, RootHandle,
};

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
pub struct ProjectSelectionView {
    #[serde(flatten)]
    pub project: ProjectView,
    pub created: bool,
}

pub fn register_selected_project(
    database: &Database,
    root: &RootHandle,
    name: Option<&str>,
) -> ProjectCommandResult<ProjectSelectionView> {
    let (project, created) = database.agent_store().select_project(root, name)?;
    Ok(ProjectSelectionView {
        project: project.into(),
        created,
    })
}

pub fn relocate_selected_project(
    state: &AppState,
    expected: &ProjectRecord,
    root: &RootHandle,
) -> ProjectCommandResult<ProjectView> {
    let database = project_database(state)?;
    state.generations.with_generation_barrier(|running| {
        let conversations = [false, true]
            .into_iter()
            .map(|archived| {
                database
                    .agent_store()
                    .list_project_conversations(Some(expected.id), archived)
            })
            .collect::<Result<Vec<_>, _>>()?;
        if conversations
            .into_iter()
            .flatten()
            .any(|conversation| running.contains(&conversation.conversation.id))
        {
            return Err(ProjectCommandError::Busy);
        }
        database
            .agent_store()
            .relocate_project_if_current(expected, root)
            .map(Into::into)
            .map_err(Into::into)
    })
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

pub(crate) async fn pick_native_root(app: &AppHandle) -> ProjectCommandResult<Option<RootHandle>> {
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
pub async fn projects_pick_folder(
    app: AppHandle,
    state: State<'_, AppState>,
) -> ProjectCommandResult<Option<ProjectFolderSelectionView>> {
    let Some(root) = pick_native_root(&app).await? else {
        return Ok(None);
    };
    begin_selected_project(&state, root).map(Some)
}

#[tauri::command]
pub fn projects_confirm_folder(
    state: State<'_, AppState>,
    selection_id: String,
    name: String,
) -> ProjectCommandResult<ProjectSelectionView> {
    confirm_selected_project(&state, &selection_id, &name)
}

#[tauri::command]
pub fn projects_discard_folder(
    state: State<'_, AppState>,
    selection_id: String,
) -> ProjectCommandResult<()> {
    discard_selected_project(&state, &selection_id)
}

#[tauri::command]
pub async fn projects_pick_create(
    app: AppHandle,
    state: State<'_, AppState>,
    name: Option<String>,
) -> ProjectCommandResult<Option<ProjectSelectionView>> {
    let Some(root) = pick_native_root(&app).await? else {
        return Ok(None);
    };
    let selection = begin_selected_project(&state, root)?;
    if let Some(existing) = &selection.existing_project {
        return confirm_selected_project(&state, &selection.selection_id, &existing.name).map(Some);
    }
    let name = name.unwrap_or(selection.default_name);
    if name.trim().is_empty() || name.len() > 128 || name.chars().any(char::is_control) {
        let _ = discard_selected_project(&state, &selection.selection_id);
        return Err(ProjectCommandError::InvalidInput);
    }
    let (sender, receiver) = tokio::sync::oneshot::channel();
    app.dialog()
        .message(format!(
            "Create a project with this name and folder?\n\nName: {}\nFolder: {}\n\nThe folder and its files will not be changed.",
            super::agent_commands::single_line(name.trim()),
            super::agent_commands::single_line(&selection.folder_path),
        ))
        .title("Create project")
        .kind(MessageDialogKind::Info)
        .buttons(MessageDialogButtons::OkCancelCustom("Create project".into(), "Cancel".into()))
        .show_with_result(move |answer| { let _ = sender.send(answer); });
    match receiver.await {
        Ok(MessageDialogResult::Custom(label)) if label == "Create project" => {
            confirm_selected_project(&state, &selection.selection_id, &name).map(Some)
        }
        Ok(_) => {
            discard_selected_project(&state, &selection.selection_id)?;
            Ok(None)
        }
        Err(_) => {
            let _ = discard_selected_project(&state, &selection.selection_id);
            Err(ProjectCommandError::DialogUnavailable)
        }
    }
}

pub fn list_project_entries(
    database: &Database,
    project_id: i64,
    path: Option<&str>,
) -> ProjectCommandResult<Vec<DirectoryEntry>> {
    let root = database.agent_store().open_project_root(project_id)?;
    let relative = match path.map(str::trim) {
        None | Some("") => None,
        Some(value) => Some(RelativePath::parse(value)?),
    };
    let mut entries = root.list_directory(relative.as_ref())?;
    entries.sort_by(|left, right| {
        let rank = |entry: &DirectoryEntry| match entry.kind {
            DirectoryEntryKind::Directory => 0,
            DirectoryEntryKind::File => 1,
            DirectoryEntryKind::Symlink => 2,
            DirectoryEntryKind::Other => 3,
        };
        rank(left)
            .cmp(&rank(right))
            .then_with(|| left.name.to_lowercase().cmp(&right.name.to_lowercase()))
    });
    Ok(entries)
}

#[tauri::command]
pub fn projects_list_entries(
    state: State<'_, AppState>,
    project_id: i64,
    path: Option<String>,
) -> ProjectCommandResult<Vec<DirectoryEntry>> {
    let database = project_database(&state)?;
    list_project_entries(&database, project_id, path.as_deref())
}

const MAX_FILE_READ_BYTES: usize = 2 * 1024 * 1024;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectFileView {
    pub path: String,
    pub content: String,
    pub truncated: bool,
}

pub fn read_project_file(
    database: &Database,
    project_id: i64,
    path: &str,
) -> ProjectCommandResult<ProjectFileView> {
    let root = database.agent_store().open_project_root(project_id)?;
    let relative = RelativePath::parse(path)?;
    let bytes = root.read(&relative, MAX_FILE_READ_BYTES)?;
    let truncated = bytes.len() >= MAX_FILE_READ_BYTES;
    let content = String::from_utf8_lossy(&bytes).into_owned();
    Ok(ProjectFileView {
        path: relative.as_path().to_string_lossy().into_owned(),
        content,
        truncated,
    })
}

pub fn write_project_file(
    database: &Database,
    project_id: i64,
    path: &str,
    content: &str,
) -> ProjectCommandResult<ProjectFileView> {
    let root = database.agent_store().open_project_root(project_id)?;
    let relative = RelativePath::parse(path)?;
    let bytes = content.as_bytes();
    match root.open_read(&relative) {
        Ok(_) => root.rewrite(&relative, bytes)?,
        Err(error) if error.is_missing() => root.create_new(&relative, bytes)?,
        Err(error) => return Err(error.into()),
    }
    Ok(ProjectFileView {
        path: relative.as_path().to_string_lossy().into_owned(),
        content: content.to_string(),
        truncated: false,
    })
}

#[tauri::command]
pub fn projects_file_read(
    state: State<'_, AppState>,
    project_id: i64,
    path: String,
) -> ProjectCommandResult<ProjectFileView> {
    let database = project_database(&state)?;
    read_project_file(&database, project_id, path.trim())
}

#[tauri::command]
pub fn projects_file_write(
    state: State<'_, AppState>,
    project_id: i64,
    path: String,
    content: String,
) -> ProjectCommandResult<ProjectFileView> {
    let database = project_database(&state)?;
    write_project_file(&database, project_id, path.trim(), &content)
}

const SEARCH_DEPTH_LIMIT: usize = 20;
const SEARCH_ENTRY_LIMIT: usize = 5000;
const SEARCH_FILE_LIMIT: usize = 500;
const SEARCH_READ_BYTES: usize = 256 * 1024;
const SEARCH_MATCH_LIMIT: usize = 10;
const SEARCH_SNIPPET_CHARS: usize = 200;
const SEARCH_SKIP_DIRS: [&str; 5] = [".git", "node_modules", "target", "dist", "build"];

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectContentMatch {
    pub line: u32,
    pub snippet: String,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSearchHit {
    pub path: String,
    pub kind: DirectoryEntryKind,
    pub name_match: bool,
    pub matches: Vec<ProjectContentMatch>,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectSearchResult {
    pub hits: Vec<ProjectSearchHit>,
    pub truncated: bool,
}

struct SearchCollector<'a> {
    root: &'a RootHandle,
    query: String,
    hits: Vec<ProjectSearchHit>,
    entries_seen: usize,
    files_seen: usize,
    truncated: bool,
}

impl SearchCollector<'_> {
    fn walk(&mut self, relative: Option<&RelativePath>, base: &str, depth: usize) {
        if depth > SEARCH_DEPTH_LIMIT || self.entries_seen >= SEARCH_ENTRY_LIMIT {
            self.truncated = true;
            return;
        }
        let entries = match self.root.list_directory(relative) {
            Ok(entries) => entries,
            Err(_) => return,
        };
        for entry in entries {
            if self.entries_seen >= SEARCH_ENTRY_LIMIT {
                self.truncated = true;
                return;
            }
            if SEARCH_SKIP_DIRS.contains(&entry.name.as_str()) || entry.name == ".DS_Store" {
                continue;
            }
            self.entries_seen += 1;
            let path = if base.is_empty() {
                entry.name.clone()
            } else {
                format!("{base}/{}", entry.name)
            };
            let name_match = entry.name.to_lowercase().contains(&self.query);
            match entry.kind {
                DirectoryEntryKind::Directory => {
                    if name_match {
                        self.hits.push(ProjectSearchHit {
                            path: path.clone(),
                            kind: DirectoryEntryKind::Directory,
                            name_match: true,
                            matches: Vec::new(),
                        });
                    }
                    if depth < SEARCH_DEPTH_LIMIT {
                        if let Ok(nested) = RelativePath::parse(&path) {
                            self.walk(Some(&nested), &path, depth + 1);
                        }
                    }
                }
                DirectoryEntryKind::File => {
                    let mut matches = Vec::new();
                    if self.files_seen < SEARCH_FILE_LIMIT {
                        self.files_seen += 1;
                        matches = self.content_matches(&path);
                    } else {
                        self.truncated = true;
                    }
                    if name_match || !matches.is_empty() {
                        self.hits.push(ProjectSearchHit {
                            path,
                            kind: DirectoryEntryKind::File,
                            name_match,
                            matches,
                        });
                    }
                }
                _ => {
                    if name_match {
                        self.hits.push(ProjectSearchHit {
                            path,
                            kind: entry.kind,
                            name_match: true,
                            matches: Vec::new(),
                        });
                    }
                }
            }
        }
    }

    fn content_matches(&self, path: &str) -> Vec<ProjectContentMatch> {
        let relative = match RelativePath::parse(path) {
            Ok(relative) => relative,
            Err(_) => return Vec::new(),
        };
        let bytes = match self.root.read(&relative, SEARCH_READ_BYTES) {
            Ok(bytes) => bytes,
            Err(_) => return Vec::new(),
        };
        let text = match std::str::from_utf8(&bytes) {
            Ok(text) => text,
            Err(_) => return Vec::new(),
        };
        let mut matches = Vec::new();
        for (index, line) in text.lines().enumerate() {
            if line.to_lowercase().contains(&self.query) {
                if matches.len() < SEARCH_MATCH_LIMIT {
                    matches.push(ProjectContentMatch {
                        line: (index + 1) as u32,
                        snippet: search_snippet(line, &self.query),
                    });
                }
            }
        }
        matches
    }
}

fn search_snippet(line: &str, query: &str) -> String {
    let trimmed = line.trim();
    if trimmed.chars().count() <= SEARCH_SNIPPET_CHARS {
        return trimmed.to_string();
    }
    let lower = trimmed.to_lowercase();
    let position = lower.find(query).unwrap_or(0);
    let mut start = position.saturating_sub(SEARCH_SNIPPET_CHARS / 2);
    while start > 0 && !trimmed.is_char_boundary(start) {
        start -= 1;
    }
    let mut end = (start + SEARCH_SNIPPET_CHARS).min(trimmed.len());
    while end < trimmed.len() && !trimmed.is_char_boundary(end) {
        end += 1;
    }
    trimmed[start..end].to_string()
}

pub fn search_project_files(
    database: &Database,
    project_id: i64,
    query: &str,
) -> ProjectCommandResult<ProjectSearchResult> {
    let trimmed = query.trim();
    if trimmed.is_empty() || trimmed.chars().any(char::is_control) {
        return Err(ProjectCommandError::InvalidInput);
    }
    let root = database.agent_store().open_project_root(project_id)?;
    let mut collector = SearchCollector {
        root: &root,
        query: trimmed.to_lowercase(),
        hits: Vec::new(),
        entries_seen: 0,
        files_seen: 0,
        truncated: false,
    };
    collector.walk(None, "", 0);
    Ok(ProjectSearchResult {
        hits: collector.hits,
        truncated: collector.truncated,
    })
}

#[tauri::command]
pub fn projects_search(
    state: State<'_, AppState>,
    project_id: i64,
    query: String,
) -> ProjectCommandResult<ProjectSearchResult> {
    let database = project_database(&state)?;
    search_project_files(&database, project_id, &query)
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
    relocate_selected_project(&state, &expected, &root).map(Some)
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

#[tauri::command]
pub fn projects_conversation_context(
    state: State<'_, AppState>,
    conversation_id: i64,
) -> ProjectCommandResult<ProjectContextView> {
    project_database(&state)?
        .agent_store()
        .context(conversation_id)
        .map(Into::into)
        .map_err(Into::into)
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use super::*;
    use crate::foundry::db::Database;
    use crate::foundry::resources::{DirectoryEntryKind, RootHandle};

    #[test]
    fn list_entries_lists_root_and_nested_directories() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("a.txt"), b"a").unwrap();
        std::fs::write(root.join("b.txt"), b"b").unwrap();
        std::fs::create_dir(root.join("sub")).unwrap();
        std::fs::write(root.join("sub").join("inner.txt"), b"inner").unwrap();
        let handle = RootHandle::enroll(&root).unwrap();
        let database = Database::open_in_memory().unwrap();
        let project = database.agent_store().create_project(&handle, None).unwrap();

        let entries = list_project_entries(&database, project.id, None).unwrap();
        let names = entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>();
        assert!(names.contains(&"a.txt"));
        assert!(names.contains(&"b.txt"));
        assert!(names.contains(&"sub"));

        let nested = list_project_entries(&database, project.id, Some("sub")).unwrap();
        assert_eq!(nested.len(), 1);
        assert_eq!(nested[0].name, "inner.txt");
        assert_eq!(nested[0].kind, DirectoryEntryKind::File);
    }

    #[test]
    fn list_entries_sorts_directories_before_files() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("zeta.txt"), b"z").unwrap();
        std::fs::create_dir(root.join("alpha")).unwrap();
        let handle = RootHandle::enroll(&root).unwrap();
        let database = Database::open_in_memory().unwrap();
        let project = database.agent_store().create_project(&handle, None).unwrap();

        let entries = list_project_entries(&database, project.id, None).unwrap();
        let names = entries
            .iter()
            .map(|entry| entry.name.as_str())
            .collect::<Vec<_>>();
        assert_eq!(names, vec!["alpha", "zeta.txt"]);
    }

    #[test]
    fn list_entries_rejects_parent_traversal() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let handle = RootHandle::enroll(&root).unwrap();
        let database = Database::open_in_memory().unwrap();
        let project = database.agent_store().create_project(&handle, None).unwrap();

        assert_eq!(
            list_project_entries(&database, project.id, Some("../escape"))
                .unwrap_err()
                .code(),
            "ProjectFolderUnavailable"
        );
        assert_eq!(
            list_project_entries(&database, project.id, Some("a/../../b"))
                .unwrap_err()
                .code(),
            "ProjectFolderUnavailable"
        );
    }

    #[test]
    fn list_entries_rejects_unknown_project() {
        let database = Database::open_in_memory().unwrap();
        assert_eq!(
            list_project_entries(&database, 42, None).unwrap_err().code(),
            "ProjectNotFound"
        );
    }

    #[test]
    fn read_and_write_project_file_round_trip() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("a.txt"), b"hello").unwrap();
        let handle = RootHandle::enroll(&root).unwrap();
        let database = Database::open_in_memory().unwrap();
        let project = database.agent_store().create_project(&handle, None).unwrap();

        let read = read_project_file(&database, project.id, "a.txt").unwrap();
        assert_eq!(read.content, "hello");
        assert!(!read.truncated);

        write_project_file(&database, project.id, "a.txt", "world").unwrap();
        assert_eq!(
            std::fs::read(root.join("a.txt")).unwrap(),
            b"world".to_vec()
        );

        write_project_file(&database, project.id, "new.txt", "created").unwrap();
        assert_eq!(std::fs::read(root.join("new.txt")).unwrap(), b"created");
    }

    #[test]
    fn read_project_file_rejects_traversal_and_unknown_project() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let handle = RootHandle::enroll(&root).unwrap();
        let database = Database::open_in_memory().unwrap();
        let project = database.agent_store().create_project(&handle, None).unwrap();

        assert!(read_project_file(&database, project.id, "../secret").is_err());
        assert!(read_project_file(&database, 42, "a.txt").is_err());
    }

    #[test]
    fn search_finds_files_by_name_and_content() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("readme.md"), b"# hello world\n").unwrap();
        std::fs::write(root.join("src/main.rs"), b"fn main() {\n    println!(\"hello\");\n}\n").unwrap();
        std::fs::write(root.join("notes.txt"), b"nothing to see\n").unwrap();
        let handle = RootHandle::enroll(&root).unwrap();
        let database = Database::open_in_memory().unwrap();
        let project = database.agent_store().create_project(&handle, None).unwrap();

        let by_name = search_project_files(&database, project.id, "readme").unwrap();
        assert!(by_name.hits.iter().any(|hit| hit.path == "readme.md" && hit.name_match));

        let by_content = search_project_files(&database, project.id, "hello").unwrap();
        assert!(by_content.hits.iter().any(|hit| hit.path == "src/main.rs" && !hit.matches.is_empty()));
        assert!(by_content.hits.iter().any(|hit| hit.path == "readme.md" && !hit.matches.is_empty()));
        assert!(!by_content.hits.iter().any(|hit| hit.path == "notes.txt"));
    }

    #[test]
    fn search_skips_generated_directories() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir_all(root.join("node_modules")).unwrap();
        std::fs::create_dir_all(root.join(".git")).unwrap();
        std::fs::write(root.join("node_modules/dep.txt"), b"match").unwrap();
        std::fs::write(root.join(".git/config"), b"match").unwrap();
        std::fs::write(root.join("app.txt"), b"match").unwrap();
        let handle = RootHandle::enroll(&root).unwrap();
        let database = Database::open_in_memory().unwrap();
        let project = database.agent_store().create_project(&handle, None).unwrap();

        let result = search_project_files(&database, project.id, "match").unwrap();
        assert!(result.hits.iter().any(|hit| hit.path == "app.txt"));
        assert!(!result.hits.iter().any(|hit| hit.path.contains("node_modules")));
        assert!(!result.hits.iter().any(|hit| hit.path.contains(".git")));
    }

    #[test]
    fn search_rejects_empty_query() {
        let directory = tempfile::tempdir().unwrap();
        let root = directory.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let handle = RootHandle::enroll(&root).unwrap();
        let database = Database::open_in_memory().unwrap();
        let project = database.agent_store().create_project(&handle, None).unwrap();

        assert_eq!(
            search_project_files(&database, project.id, "   ").unwrap_err().code(),
            "ProjectInvalidInput"
        );
    }
}
