use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde_json::Value;

use super::history::{fingerprint, insert_history, redact, redact_result};
use super::lifecycle::{checked_reference, require_active_run};
use super::{
    ActionOutcome, ActionRecord, AgentEventState, AgentStore, AgentStoreError, AgentStoreResult,
    HistoryContent, HistoryEntry, RunRecord, read_run, store_error, validate_key,
};
use crate::foundry::db::MessageRole;
use crate::foundry::jobs::now_millis;

const MAX_DETAIL_BYTES: usize = 1024;
const MAX_TEXT_BYTES: usize = 1_048_576;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RejectionState {
    Denied,
    Failed,
}

impl RejectionState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Denied => "denied",
            Self::Failed => "failed",
        }
    }
}

impl AgentStore<'_> {
    pub fn record_event(
        &self,
        conversation_id: i64,
        run_id: i64,
        action_id: Option<i64>,
        event: AgentEventState,
        detail: Option<&str>,
    ) -> AgentStoreResult<HistoryEntry> {
        let detail = bounded_detail(detail);
        let mut connection = self.database.connection();
        let transaction = connection.transaction().map_err(store_error)?;
        let run = read_run(&transaction, run_id)?;
        if run.conversation_id != conversation_id {
            return Err(AgentStoreError::Conflict);
        }
        require_active_run(&transaction, &run)?;
        let content = HistoryContent::StateEvent {
            event,
            action_id,
            detail,
        };
        insert_history(&transaction, conversation_id, run_id, action_id, &content)?;
        let sequence = last_sequence(&transaction, conversation_id, run_id)?;
        transaction.commit().map_err(store_error)?;
        Ok(HistoryEntry {
            sequence,
            run_id: Some(run_id),
            content,
        })
    }

    pub fn record_text(
        &self,
        run_id: i64,
        role: MessageRole,
        content: &str,
        reasoning: &str,
    ) -> AgentStoreResult<HistoryEntry> {
        if content.len() > MAX_TEXT_BYTES || reasoning.len() > MAX_TEXT_BYTES {
            return Err(AgentStoreError::InvalidInput);
        }
        let mut connection = self.database.connection();
        let transaction = connection.transaction().map_err(store_error)?;
        let run = read_run(&transaction, run_id)?;
        require_active_run(&transaction, &run)?;
        let entry = HistoryContent::Text {
            role,
            content: content.to_string(),
            reasoning: reasoning.to_string(),
        };
        insert_history(&transaction, run.conversation_id, run_id, None, &entry)?;
        let sequence = last_sequence(&transaction, run.conversation_id, run_id)?;
        transaction.commit().map_err(store_error)?;
        Ok(HistoryEntry {
            sequence,
            run_id: Some(run_id),
            content: entry,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub fn record_unbound_tool_result(
        &self,
        run_id: i64,
        call_id: &str,
        tool: &str,
        arguments: &Value,
        outcome: ActionOutcome,
        summary: &Value,
    ) -> AgentStoreResult<HistoryEntry> {
        validate_key(call_id)?;
        validate_key(tool)?;
        if !arguments.is_object() {
            return Err(AgentStoreError::InvalidInput);
        }
        let mut connection = self.database.connection();
        let transaction = connection.transaction().map_err(store_error)?;
        let run = read_run(&transaction, run_id)?;
        require_active_run(&transaction, &run)?;
        let recorded: bool = transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM agent_history WHERE run_id = ?1
                 AND json_extract(content_json, '$.kind') = 'tool_call'
                 AND json_extract(content_json, '$.call_id') = ?2)",
                params![run_id, call_id],
                |row| row.get(0),
            )
            .map_err(store_error)?;
        if !recorded {
            insert_history(
                &transaction,
                run.conversation_id,
                run_id,
                None,
                &HistoryContent::ToolCall {
                    call_id: call_id.to_string(),
                    tool: tool.to_string(),
                    arguments: redact(arguments),
                },
            )?;
        }
        let content = HistoryContent::ToolResult {
            call_id: call_id.to_string(),
            outcome,
            summary: redact_result(summary),
        };
        insert_history(&transaction, run.conversation_id, run_id, None, &content)?;
        let sequence = last_sequence(&transaction, run.conversation_id, run_id)?;
        transaction.commit().map_err(store_error)?;
        Ok(HistoryEntry {
            sequence,
            run_id: Some(run_id),
            content,
        })
    }

    pub fn record_unexecuted_call(
        &self,
        reference: &ActionRecord,
        summary: &Value,
        state: RejectionState,
    ) -> AgentStoreResult<HistoryEntry> {
        let summary = redact_result(summary);
        let mut connection = self.database.connection();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(store_error)?;
        let action = checked_reference(&transaction, reference)?;
        if let Some(entry) = tool_result_entry(&transaction, action.id)? {
            return Ok(entry);
        }
        require_active_run(&transaction, &read_run(&transaction, action.run_id)?)?;
        if matches!(
            action.state,
            super::ActionState::Proposed | super::ActionState::Approved
        ) {
            transaction
                .execute(
                    "UPDATE agent_actions SET state = ?2, updated_at = ?3 WHERE id = ?1",
                    params![action.id, state.as_str(), now_millis()],
                )
                .map_err(store_error)?;
            transaction
                .execute(
                    "UPDATE agent_authorization_requests SET response = 'denied', decided_at = ?2
                     WHERE action_id = ?1 AND response = 'pending'",
                    params![action.id, now_millis()],
                )
                .map_err(store_error)?;
        }
        let content = HistoryContent::ToolResult {
            call_id: action.call_id.clone(),
            outcome: ActionOutcome::Failed,
            summary,
        };
        insert_history(
            &transaction,
            action.conversation_id,
            action.run_id,
            Some(action.id),
            &content,
        )?;
        let sequence = last_sequence(&transaction, action.conversation_id, action.run_id)?;
        transaction.commit().map_err(store_error)?;
        Ok(HistoryEntry {
            sequence,
            run_id: Some(action.run_id),
            content,
        })
    }

    pub fn tool_result_for(
        &self,
        action_id: i64,
    ) -> AgentStoreResult<Option<(ActionOutcome, Value)>> {
        let connection = self.database.connection();
        let Some(entry) = tool_result_entry(&connection, action_id)? else {
            return Ok(None);
        };
        match entry.content {
            HistoryContent::ToolResult {
                outcome, summary, ..
            } => Ok(Some((outcome, summary))),
            _ => Err(AgentStoreError::Storage),
        }
    }

    pub fn fail_run(&self, id: i64, detail: Option<&str>) -> AgentStoreResult<RunRecord> {
        let detail = bounded_detail(detail);
        let mut connection = self.database.connection();
        let transaction = connection.transaction().map_err(store_error)?;
        let run = read_run(&transaction, id)?;
        if matches!(run.state, super::RunState::Failed) {
            return Ok(run);
        }
        require_active_run(&transaction, &run)?;
        transaction
            .execute(
                "UPDATE agent_runs SET state = 'failed', updated_at = ?1 WHERE id = ?2",
                params![now_millis(), id],
            )
            .map_err(store_error)?;
        insert_history(
            &transaction,
            run.conversation_id,
            id,
            None,
            &HistoryContent::StateEvent {
                event: AgentEventState::RunFailed,
                action_id: None,
                detail,
            },
        )?;
        let run = read_run(&transaction, id)?;
        transaction.commit().map_err(store_error)?;
        Ok(run)
    }

    pub fn last_sequence(&self, run_id: i64) -> AgentStoreResult<i64> {
        let connection = self.database.connection();
        let conversation_id = read_run(&connection, run_id)?.conversation_id;
        last_sequence(&connection, conversation_id, run_id)
    }

    pub fn arguments_fingerprint(&self, arguments: &Value) -> AgentStoreResult<String> {
        fingerprint(arguments)
    }
}

fn bounded_detail(detail: Option<&str>) -> Option<String> {
    detail.map(|value| {
        if value.len() <= MAX_DETAIL_BYTES {
            return value.to_string();
        }
        let mut end = MAX_DETAIL_BYTES;
        while end > 0 && !value.is_char_boundary(end) {
            end -= 1;
        }
        value[..end].to_string()
    })
}

fn last_sequence(
    connection: &Connection,
    conversation_id: i64,
    run_id: i64,
) -> AgentStoreResult<i64> {
    connection
        .query_row(
            "SELECT MAX(seq) FROM agent_history WHERE conversation_id = ?1 AND run_id = ?2",
            params![conversation_id, run_id],
            |row| row.get::<_, Option<i64>>(0),
        )
        .map_err(store_error)?
        .ok_or(AgentStoreError::NotFound)
}

fn tool_result_entry(
    connection: &Connection,
    action_id: i64,
) -> AgentStoreResult<Option<HistoryEntry>> {
    let row: Option<(i64, i64, String)> = connection
        .query_row(
            "SELECT seq, run_id, content_json FROM agent_history WHERE action_id = ?1 AND json_extract(content_json, '$.kind') = 'tool_result' ORDER BY seq DESC LIMIT 1",
            [action_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .optional()
        .map_err(store_error)?;
    row.map(|(sequence, run_id, encoded)| {
        let content: HistoryContent =
            serde_json::from_str(&encoded).map_err(|_| AgentStoreError::Storage)?;
        Ok(HistoryEntry {
            sequence,
            run_id: Some(run_id),
            content,
        })
    })
    .transpose()
}
