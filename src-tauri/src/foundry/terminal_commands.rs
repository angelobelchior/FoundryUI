use serde::Serialize;

use super::agent_commands::AgentCommandResult;
use super::agent_runtime::terminal;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TerminalStatus {
    pub available: bool,
    pub sessions: Vec<terminal::TerminalSessionStatus>,
}

#[tauri::command]
pub fn terminal_status() -> AgentCommandResult<TerminalStatus> {
    Ok(TerminalStatus {
        available: terminal::terminal_sandbox_available(),
        sessions: terminal::session_statuses(),
    })
}
