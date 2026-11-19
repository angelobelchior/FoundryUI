use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::Serialize;

use super::AppState;
use super::db::Database;
use super::db::agent::ProjectRecord;
use super::project_commands::{
    ProjectCommandError, ProjectCommandResult, ProjectSelectionView, ProjectView,
};
use super::resources::RootHandle;

const MAX_PENDING: usize = 32;
const SELECTION_TTL: Duration = Duration::from_secs(600);

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProjectFolderSelectionView {
    pub selection_id: String,
    pub folder_path: String,
    pub default_name: String,
    pub existing_project: Option<ProjectView>,
}

struct PendingProjectSelection {
    database: Arc<Database>,
    root: Arc<RootHandle>,
    expected: Option<ProjectRecord>,
    created_at: Instant,
}

#[derive(Default)]
pub struct ProjectSelectionRegistry {
    pending: Mutex<HashMap<String, PendingProjectSelection>>,
}

fn prune(pending: &mut HashMap<String, PendingProjectSelection>) {
    pending.retain(|_, selection| selection.created_at.elapsed() < SELECTION_TTL);
}

pub fn begin_selected_project(
    state: &AppState,
    root: impl Into<Arc<RootHandle>>,
) -> ProjectCommandResult<ProjectFolderSelectionView> {
    let mut pending = state
        .project_selections
        .pending
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    prune(&mut pending);
    let database = state
        .database()
        .map_err(|_| ProjectCommandError::NotReady)?;
    let root = root.into();
    root.identity()?;
    let folder_path = root
        .display_path()
        .to_str()
        .filter(|path| {
            !path.is_empty() && path.len() <= 4096 && !path.chars().any(char::is_control)
        })
        .ok_or(ProjectCommandError::InvalidSelection)?
        .to_string();
    let expected = database.agent_store().selected_project_owner(&root)?;
    let default_name = expected
        .as_ref()
        .map(|project| project.name.clone())
        .unwrap_or_else(|| {
            root.display_path()
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or(&folder_path)
                .to_string()
        });
    let token = loop {
        let token = database.agent_store().project_selection_token()?;
        if !pending.contains_key(&token) {
            break token;
        }
    };
    if pending.len() >= MAX_PENDING {
        let oldest = pending
            .iter()
            .min_by_key(|(_, selection)| selection.created_at)
            .map(|(token, _)| token.clone());
        if let Some(oldest) = oldest {
            pending.remove(&oldest);
        }
    }
    let existing_project = expected.clone().map(Into::into);
    pending.insert(
        token.clone(),
        PendingProjectSelection {
            database,
            root,
            expected,
            created_at: Instant::now(),
        },
    );
    Ok(ProjectFolderSelectionView {
        selection_id: token,
        folder_path,
        default_name,
        existing_project,
    })
}

pub fn confirm_selected_project(
    state: &AppState,
    selection_id: &str,
    name: &str,
) -> ProjectCommandResult<ProjectSelectionView> {
    let mut pending = state
        .project_selections
        .pending
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    prune(&mut pending);
    let selection = pending
        .get(selection_id)
        .ok_or(ProjectCommandError::InvalidSelection)?;
    let result = state
        .with_database(|database| {
            if !Arc::ptr_eq(&selection.database, database) {
                return Err(ProjectCommandError::StaleContext);
            }
            selection.root.identity()?;
            let (project, created) = database.agent_store().select_project_if_current(
                &selection.root,
                name,
                selection.expected.as_ref(),
            )?;
            Ok(ProjectSelectionView {
                project: project.into(),
                created,
            })
        })
        .map_err(|_| ProjectCommandError::NotReady)??;
    pending.remove(selection_id);
    Ok(result)
}

pub fn discard_selected_project(state: &AppState, selection_id: &str) -> ProjectCommandResult<()> {
    let mut pending = state
        .project_selections
        .pending
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    prune(&mut pending);
    pending.remove(selection_id);
    Ok(())
}
