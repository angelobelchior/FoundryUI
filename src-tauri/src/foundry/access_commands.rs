use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use super::AppState;
use super::agent_commands::{AgentCommandError, AgentCommandResult};
use super::db::agent::NetworkLogPage;

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GlobalReadPreference {
    pub allowed: bool,
}

#[tauri::command]
pub fn access_read_preference(
    state: State<'_, AppState>,
) -> AgentCommandResult<GlobalReadPreference> {
    let database = state.database().map_err(|_| AgentCommandError::NotReady)?;
    Ok(GlobalReadPreference {
        allowed: database.agent_store().global_read_allowed()?,
    })
}

#[tauri::command]
pub fn access_set_read_preference(
    app: AppHandle,
    state: State<'_, AppState>,
    allowed: bool,
) -> AgentCommandResult<GlobalReadPreference> {
    let database = state.database().map_err(|_| AgentCommandError::NotReady)?;
    database
        .agent_store()
        .set_authorization_read_allowed(allowed)?;
    let preference = GlobalReadPreference {
        allowed: database.agent_store().global_read_allowed()?,
    };
    let _ = app.emit("access-read-preference-changed", preference.clone());
    Ok(preference)
}

#[tauri::command]
pub fn access_network_log(
    state: State<'_, AppState>,
    conversation_id: i64,
    before_id: Option<i64>,
) -> AgentCommandResult<NetworkLogPage> {
    let database = state.database().map_err(|_| AgentCommandError::NotReady)?;
    Ok(database
        .agent_store()
        .network_log(conversation_id, before_id, 50)?)
}
