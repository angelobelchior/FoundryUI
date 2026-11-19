use serde::Serialize;
use tauri::State;

use super::AppState;
use super::agent_runtime::{
    RecoverableChange, RecoveryError, RecoveryOutcome, list_recoverable, recover,
};

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RecoveryCommandError {
    pub code: String,
    pub message: String,
}

impl From<RecoveryError> for RecoveryCommandError {
    fn from(error: RecoveryError) -> Self {
        Self {
            code: error.code().to_string(),
            message: error.message().to_string(),
        }
    }
}

#[tauri::command]
pub fn recovery_list(
    state: State<'_, AppState>,
    conversation_id: i64,
    run_id: Option<i64>,
) -> Result<Vec<RecoverableChange>, RecoveryCommandError> {
    let store = state.database().map_err(|_| RecoveryCommandError {
        code: "not_ready".to_string(),
        message: "the conversation database is not ready".to_string(),
    })?;
    list_recoverable(&store.agent_store(), conversation_id, run_id).map_err(Into::into)
}

#[tauri::command]
pub fn recovery_undo(
    state: State<'_, AppState>,
    conversation_id: i64,
    action_id: i64,
    confirmed_destructive: bool,
) -> Result<RecoveryOutcome, RecoveryCommandError> {
    let store = state.database().map_err(|_| RecoveryCommandError {
        code: "not_ready".to_string(),
        message: "the conversation database is not ready".to_string(),
    })?;
    recover(
        &store.agent_store(),
        conversation_id,
        action_id,
        confirmed_destructive,
    )
    .map_err(Into::into)
}
