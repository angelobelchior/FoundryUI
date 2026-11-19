use rusqlite::{OptionalExtension, params};
use serde::Serialize;

use super::{
    ActionOutcome, AgentEventState, AgentStore, AgentStoreError, AgentStoreResult, HistoryContent,
    RunRecord, read_run, store_error,
};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionRun {
    pub run: RunRecord,
    pub writer_remaining: bool,
    pub events: Vec<ExecutionEvent>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecutionEvent {
    pub sequence: i64,
    pub event: AgentEventState,
    pub action_id: Option<i64>,
}

impl AgentStore<'_> {
    pub fn execution_run(&self, conversation_id: i64) -> AgentStoreResult<Option<ExecutionRun>> {
        let connection = self.database.connection();
        let id: Option<i64> = connection
            .query_row(
                "SELECT id FROM agent_runs WHERE conversation_id = ?1
             ORDER BY CASE WHEN state IN ('prepared', 'running') OR EXISTS(
               SELECT 1 FROM agent_actions a WHERE a.run_id = agent_runs.id
               AND a.state IN ('started', 'interrupted') AND a.claim_token IS NOT NULL
             ) THEN 0 ELSE 1 END, id DESC LIMIT 1",
                [conversation_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(store_error)?;
        let Some(id) = id else {
            return Ok(None);
        };
        let run = read_run(&connection, id)?;
        let writer_remaining = connection
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM agent_actions WHERE run_id = ?1
             AND state IN ('started', 'interrupted') AND claim_token IS NOT NULL)",
                [id],
                |row| row.get(0),
            )
            .map_err(store_error)?;
        let mut statement = connection
            .prepare(
            "SELECT seq, content_json, action_id FROM agent_history WHERE run_id = ?1
             AND json_extract(content_json, '$.kind') IN ('state_event', 'tool_result') ORDER BY seq DESC LIMIT 100",
            )
            .map_err(store_error)?;
        let rows = statement
            .query_map(params![id], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, Option<i64>>(2)?,
                ))
            })
            .map_err(store_error)?;
        let mut events = Vec::new();
        for row in rows {
            let (sequence, encoded, record_action_id) = row.map_err(store_error)?;
            let (event, action_id) =
                match serde_json::from_str(&encoded).map_err(|_| AgentStoreError::Storage)? {
                    HistoryContent::StateEvent {
                        event, action_id, ..
                    } => (event, action_id),
                    HistoryContent::ToolResult { outcome, .. } => (
                        match outcome {
                            ActionOutcome::Succeeded => AgentEventState::ToolCompleted,
                            ActionOutcome::Failed => AgentEventState::ToolFailed,
                            ActionOutcome::Cancelled => AgentEventState::ActionCancelled,
                        },
                        record_action_id,
                    ),
                    _ => continue,
                };
            events.push(ExecutionEvent {
                sequence,
                event,
                action_id,
            });
        }
        events.reverse();
        Ok(Some(ExecutionRun {
            run,
            writer_remaining,
            events,
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::foundry::db::Database;

    #[test]
    fn execution_snapshot_is_ordered_and_keeps_cancelled_state() {
        let database = Database::open_in_memory().unwrap();
        let conversation = database.create_conversation("fixture").unwrap();
        let store = database.agent_store();
        let run = store.start_run(conversation.id, "one").unwrap();
        store.cancel_run(run.id).unwrap();
        let snapshot = store.execution_run(conversation.id).unwrap().unwrap();
        assert_eq!(snapshot.run.id, run.id);
        assert_eq!(snapshot.run.state, super::super::RunState::Cancelled);
        assert!(!snapshot.writer_remaining);
        assert!(
            snapshot
                .events
                .windows(2)
                .all(|pair| pair[0].sequence < pair[1].sequence)
        );
        assert_eq!(
            snapshot.events.last().unwrap().event,
            AgentEventState::RunCancelled
        );
    }

    #[test]
    fn execution_events_expose_results_without_copying_tool_payloads() {
        let database = Database::open_in_memory().unwrap();
        let conversation = database.create_conversation("fixture").unwrap();
        let store = database.agent_store();
        let run = store.start_run(conversation.id, "one").unwrap();
        store
            .record_unbound_tool_result(
                run.id,
                "call",
                "unknown",
                &serde_json::json!({"token": "synthetic-secret"}),
                ActionOutcome::Failed,
                &serde_json::json!({"content": "synthetic-output"}),
            )
            .unwrap();
        let snapshot = store.execution_run(conversation.id).unwrap().unwrap();
        assert_eq!(
            snapshot.events.last().unwrap().event,
            AgentEventState::ToolFailed
        );
        let encoded = serde_json::to_string(&snapshot).unwrap();
        assert!(!encoded.contains("synthetic-secret"));
        assert!(!encoded.contains("synthetic-output"));
    }
}
