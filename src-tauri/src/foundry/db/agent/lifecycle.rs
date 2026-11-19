use rusqlite::{Connection, OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use super::history::{fingerprint, insert_history, read_action, redact_result};
use super::{
    ActionRecord, ActionState, AgentEventState, AgentStore, AgentStoreError, AgentStoreResult,
    HistoryContent, RunRecord, RunState, read_run, store_error,
};
use crate::foundry::jobs::now_millis;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalDecision {
    Approved,
    Denied,
}

impl ApprovalDecision {
    fn as_str(self) -> &'static str {
        match self {
            Self::Approved => "approved",
            Self::Denied => "denied",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionOutcome {
    Succeeded,
    Failed,
    Cancelled,
}

impl ActionOutcome {
    fn state(self) -> &'static str {
        match self {
            Self::Succeeded => "complete",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }
}

#[derive(Debug)]
pub struct ActionClaim {
    reference: ActionRecord,
    token: String,
}

impl AgentStore<'_> {
    pub fn decide_call(
        &self,
        reference: &ActionRecord,
        arguments: &Value,
        decision: ApprovalDecision,
    ) -> AgentStoreResult<ActionRecord> {
        let mut connection = self.database.connection();
        let transaction = connection.transaction().map_err(store_error)?;
        let action = checked_arguments(&transaction, reference, arguments)?;
        let recorded: Option<String> = transaction
            .query_row(
                "SELECT decision FROM agent_approvals WHERE action_id = ?1",
                [action.id],
                |row| row.get(0),
            )
            .optional()
            .map_err(store_error)?;
        if let Some(recorded) = recorded {
            return if recorded == decision.as_str() {
                Ok(action)
            } else {
                Err(AgentStoreError::Conflict)
            };
        }
        require_active_run(&transaction, &read_run(&transaction, action.run_id)?)?;
        if action.state != ActionState::Proposed {
            return Err(AgentStoreError::InvalidTransition);
        }
        let now = now_millis();
        transaction.execute(
            "INSERT INTO agent_approvals (action_id, run_id, conversation_id, context_revision, arguments_fingerprint, decision, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![action.id, action.run_id, action.conversation_id, action.context_revision, action.arguments_fingerprint, decision.as_str(), now],
        ).map_err(store_error)?;
        transaction
            .execute(
                "UPDATE agent_actions SET state = ?1, updated_at = ?2 WHERE id = ?3",
                params![decision.as_str(), now, action.id],
            )
            .map_err(store_error)?;
        insert_history(
            &transaction,
            action.conversation_id,
            action.run_id,
            Some(action.id),
            &HistoryContent::StateEvent {
                event: match decision {
                    ApprovalDecision::Approved => AgentEventState::Approved,
                    ApprovalDecision::Denied => AgentEventState::Denied,
                },
                action_id: Some(action.id),
                detail: None,
            },
        )?;
        let action = read_action(&transaction, action.id)?;
        transaction.commit().map_err(store_error)?;
        Ok(action)
    }

    pub fn claim_call(
        &self,
        reference: &ActionRecord,
        arguments: &Value,
    ) -> AgentStoreResult<ActionClaim> {
        let mut connection = self.database.connection();
        let transaction = connection.transaction().map_err(store_error)?;
        let claim = claim_call_in_transaction(&transaction, reference, arguments)?;
        transaction.commit().map_err(store_error)?;
        Ok(claim)
    }

    pub fn complete_call(
        &self,
        claim: &ActionClaim,
        outcome: ActionOutcome,
        summary: &Value,
    ) -> AgentStoreResult<ActionRecord> {
        let result_fingerprint =
            fingerprint(&serde_json::json!({"outcome":outcome,"summary":summary}))?;
        let mut connection = self.database.connection();
        let transaction = connection.transaction().map_err(store_error)?;
        let action = checked_reference(&transaction, &claim.reference)?;
        let (token, recorded): (Option<String>, Option<String>) = transaction
            .query_row(
                "SELECT claim_token, result_fingerprint FROM agent_actions WHERE id = ?1",
                [action.id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .map_err(store_error)?;
        if token.as_deref() != Some(&claim.token) {
            return Err(AgentStoreError::Conflict);
        }
        if let Some(recorded) = recorded {
            return if recorded == result_fingerprint {
                Ok(action)
            } else {
                Err(AgentStoreError::Conflict)
            };
        }
        require_active_run(&transaction, &read_run(&transaction, action.run_id)?)?;
        if action.state != ActionState::Started {
            return Err(AgentStoreError::InvalidTransition);
        }
        transaction.execute(
            "UPDATE agent_actions SET state = ?1, result_fingerprint = ?2, updated_at = ?3 WHERE id = ?4",
            params![outcome.state(), result_fingerprint, now_millis(), action.id],
        ).map_err(store_error)?;
        insert_history(
            &transaction,
            action.conversation_id,
            action.run_id,
            Some(action.id),
            &HistoryContent::ToolResult {
                call_id: action.call_id.clone(),
                outcome,
                summary: redact_result(summary),
            },
        )?;
        let action = read_action(&transaction, action.id)?;
        transaction.commit().map_err(store_error)?;
        Ok(action)
    }
}

pub(super) fn claim_call_in_transaction(
    transaction: &Connection,
    reference: &ActionRecord,
    arguments: &Value,
) -> AgentStoreResult<ActionClaim> {
    let action = checked_arguments(transaction, reference, arguments)?;
    let existing: Option<String> = transaction
        .query_row(
            "SELECT claim_token FROM agent_actions WHERE id = ?1",
            [action.id],
            |row| row.get(0),
        )
        .map_err(store_error)?;
    if existing.is_some() {
        return Err(AgentStoreError::AlreadyClaimed);
    }
    require_active_run(transaction, &read_run(transaction, action.run_id)?)?;
    if action.state != ActionState::Approved {
        return Err(AgentStoreError::InvalidTransition);
    }
    let writer_pending: bool = transaction
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM agent_actions WHERE conversation_id = ?1 AND id != ?2
             AND (state = 'started' OR (state = 'interrupted' AND claim_token IS NOT NULL)))",
            params![action.conversation_id, action.id],
            |row| row.get(0),
        )
        .map_err(store_error)?;
    if writer_pending {
        return Err(AgentStoreError::InvalidTransition);
    }
    if super::concurrency::overlapping_writer(transaction, action.conversation_id)? {
        return Err(AgentStoreError::InvalidTransition);
    }
    let token: String = transaction
        .query_row("SELECT lower(hex(randomblob(16)))", [], |row| row.get(0))
        .map_err(store_error)?;
    let now = now_millis();
    transaction.execute("UPDATE agent_actions SET state = 'started', claim_token = ?1, updated_at = ?2 WHERE id = ?3 AND state = 'approved' AND claim_token IS NULL", params![token, now, action.id]).map_err(store_error)?;
    transaction
        .execute(
            "UPDATE agent_runs SET state = 'running', updated_at = ?1 WHERE id = ?2",
            params![now, action.run_id],
        )
        .map_err(store_error)?;
    insert_history(
        transaction,
        action.conversation_id,
        action.run_id,
        Some(action.id),
        &HistoryContent::StateEvent {
            event: AgentEventState::Started,
            action_id: Some(action.id),
            detail: None,
        },
    )?;
    Ok(ActionClaim {
        reference: action,
        token,
    })
}

pub(super) fn require_active_run(connection: &Connection, run: &RunRecord) -> AgentStoreResult<()> {
    let revision: i64 = connection
        .query_row(
            "SELECT context_revision FROM agent_contexts WHERE conversation_id = ?1",
            [run.conversation_id],
            |row| row.get(0),
        )
        .map_err(store_error)?;
    if revision != run.context_revision {
        return Err(AgentStoreError::StaleContext);
    }
    if !matches!(run.state, RunState::Prepared | RunState::Running) {
        return Err(AgentStoreError::InvalidTransition);
    }
    Ok(())
}

pub(super) fn checked_arguments(
    connection: &Connection,
    reference: &ActionRecord,
    arguments: &Value,
) -> AgentStoreResult<ActionRecord> {
    let action = checked_reference(connection, reference)?;
    if fingerprint(arguments)? != action.arguments_fingerprint {
        return Err(AgentStoreError::Conflict);
    }
    Ok(action)
}

pub(super) fn checked_reference(
    connection: &Connection,
    reference: &ActionRecord,
) -> AgentStoreResult<ActionRecord> {
    let action = read_action(connection, reference.id)?;
    if action.run_id != reference.run_id
        || action.conversation_id != reference.conversation_id
        || action.context_revision != reference.context_revision
        || action.call_id != reference.call_id
        || action.tool != reference.tool
        || action.arguments_fingerprint != reference.arguments_fingerprint
    {
        return Err(AgentStoreError::Conflict);
    }
    let revision: i64 = connection
        .query_row(
            "SELECT context_revision FROM agent_contexts WHERE conversation_id = ?1",
            [action.conversation_id],
            |row| row.get(0),
        )
        .map_err(store_error)?;
    if revision != action.context_revision {
        return Err(AgentStoreError::StaleContext);
    }
    Ok(action)
}
