use serde::Serialize;
use tauri::State;

use super::AppState;
use super::agent_commands::AgentCommandError;
use super::db::agent::ContextFact;

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextBudgetView {
    pub id: i64,
    pub key: String,
    pub run_id: Option<i64>,
    pub created_at: i64,
    pub manifest: serde_json::Value,
}

#[tauri::command]
pub fn context_budgets(
    state: State<'_, AppState>,
    conversation_id: i64,
) -> Result<Vec<ContextBudgetView>, AgentCommandError> {
    let database = state.database().map_err(|_| AgentCommandError::NotReady)?;
    let store = database.agent_store();
    let facts = store.context_budgets(conversation_id)?;
    facts
        .into_iter()
        .map(|fact| {
            let manifest = match fact.fact {
                ContextFact::ContextBudget { manifest_json } => {
                    serde_json::from_str(&manifest_json).unwrap_or(serde_json::Value::Null)
                }
                _ => serde_json::Value::Null,
            };
            Ok(ContextBudgetView {
                id: fact.id,
                key: fact.key,
                run_id: fact.run_id,
                created_at: fact.created_at,
                manifest,
            })
        })
        .collect::<Result<Vec<_>, AgentCommandError>>()
}
