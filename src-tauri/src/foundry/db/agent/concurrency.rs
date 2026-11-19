use std::path::Path;

use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::Serialize;

use super::history::{insert_history, read_action};
use super::{
    ActionRecord, ActionState, AgentEventState, AgentStore, AgentStoreError, AgentStoreResult,
    HistoryContent, RunRecord, RunState, read_run, store_error,
};
use crate::foundry::jobs::now_millis;
use crate::foundry::resources::{RootHandle, RootRelation};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CancelReceipt {
    pub run_id: i64,
    pub state: RunState,
    pub cancelled_actions: usize,
    pub interrupted_actions: usize,
    pub writer_remaining: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ActiveRun {
    pub run_id: i64,
    pub conversation_id: i64,
    pub state: RunState,
}

#[derive(Clone)]
struct RootRef {
    root_id: String,
    display_path: String,
}

impl AgentStore<'_> {
    pub fn cancel_run(&self, id: i64) -> AgentStoreResult<CancelReceipt> {
        let mut connection = self.database.connection();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(store_error)?;
        let run = read_run(&transaction, id)?;
        if run.state == RunState::Cancelled {
            let receipt = cancel_receipt(&transaction, &run)?;
            transaction.commit().map_err(store_error)?;
            return Ok(receipt);
        }
        if !matches!(run.state, RunState::Prepared | RunState::Running) {
            return Err(AgentStoreError::InvalidTransition);
        }
        let now = now_millis();
        let actions: Vec<(i64, String)> = {
            let mut statement = transaction
                .prepare(
                    "SELECT id, state FROM agent_actions WHERE run_id = ?1 AND state IN ('proposed', 'approved', 'started')",
                )
                .map_err(store_error)?;
            statement
                .query_map([id], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
                })
                .map_err(store_error)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(store_error)?
        };
        for (action_id, state) in actions {
            match state.as_str() {
                "proposed" | "approved" => {
                    transaction
                        .execute(
                            "UPDATE agent_actions SET state = 'cancelled', updated_at = ?2 WHERE id = ?1",
                            params![action_id, now],
                        )
                        .map_err(store_error)?;
                    insert_history(
                        &transaction,
                        run.conversation_id,
                        id,
                        Some(action_id),
                        &HistoryContent::StateEvent {
                            event: AgentEventState::ActionCancelled,
                            action_id: Some(action_id),
                            detail: None,
                        },
                    )?;
                }
                "started" => {
                    transaction
                        .execute(
                            "UPDATE agent_actions SET state = 'interrupted', updated_at = ?2 WHERE id = ?1 AND state = 'started'",
                            params![action_id, now],
                        )
                        .map_err(store_error)?;
                    insert_history(
                        &transaction,
                        run.conversation_id,
                        id,
                        Some(action_id),
                        &HistoryContent::StateEvent {
                            event: AgentEventState::ActionInterrupted,
                            action_id: Some(action_id),
                            detail: None,
                        },
                    )?;
                }
                _ => {}
            }
        }
        transaction
            .execute(
                "UPDATE agent_runs SET state = 'cancelled', updated_at = ?2 WHERE id = ?1",
                params![id, now],
            )
            .map_err(store_error)?;
        insert_history(
            &transaction,
            run.conversation_id,
            id,
            None,
            &HistoryContent::StateEvent {
                event: AgentEventState::RunCancelled,
                action_id: None,
                detail: None,
            },
        )?;
        let receipt = cancel_receipt(&transaction, &run)?;
        transaction.commit().map_err(store_error)?;
        Ok(receipt)
    }

    pub fn confirm_action_terminated(&self, action_id: i64) -> AgentStoreResult<ActionRecord> {
        let mut connection = self.database.connection();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(store_error)?;
        let action = read_action(&transaction, action_id)?;
        let token: Option<String> = transaction
            .query_row(
                "SELECT claim_token FROM agent_actions WHERE id = ?1",
                [action_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(store_error)?;
        if action.state != ActionState::Interrupted || token.is_none() {
            return Err(AgentStoreError::InvalidTransition);
        }
        transaction
            .execute(
                "UPDATE agent_actions SET claim_token = NULL, updated_at = ?2 WHERE id = ?1 AND state = 'interrupted'",
                params![action_id, now_millis()],
            )
            .map_err(store_error)?;
        let action = read_action(&transaction, action_id)?;
        transaction.commit().map_err(store_error)?;
        Ok(action)
    }

    pub fn active_run_for_conversation(
        &self,
        conversation_id: i64,
    ) -> AgentStoreResult<Option<ActiveRun>> {
        let connection = self.database.connection();
        let roots = conversation_roots(&connection, conversation_id)?;
        if roots.is_empty() {
            return Ok(None);
        }
        let mut statement = connection
            .prepare(
                "SELECT DISTINCT r.id, r.conversation_id, r.state
                 FROM agent_runs r
                 WHERE r.conversation_id != ?1 AND r.state IN ('prepared', 'running')
                 UNION
                 SELECT DISTINCT r.id, r.conversation_id, r.state
                 FROM agent_actions a
                 JOIN agent_runs r ON r.id = a.run_id AND r.conversation_id = a.conversation_id
                 WHERE a.conversation_id != ?1 AND a.claim_token IS NOT NULL AND a.state IN ('started', 'interrupted')
                 ORDER BY 1",
            )
            .map_err(store_error)?;
        let rows = statement
            .query_map([conversation_id], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })
            .map_err(store_error)?;
        for row in rows {
            let (run_id, other, state) = row.map_err(store_error)?;
            let other_roots = conversation_roots(&connection, other)?;
            if roots_overlap(&roots, &other_roots) {
                return Ok(Some(ActiveRun {
                    run_id,
                    conversation_id: other,
                    state: parse_run_state(&state)?,
                }));
            }
        }
        Ok(None)
    }
}

pub(super) fn overlapping_writer(
    connection: &Connection,
    conversation_id: i64,
) -> AgentStoreResult<bool> {
    let roots = conversation_roots(connection, conversation_id)?;
    if roots.is_empty() {
        return Ok(false);
    }
    let mut statement = connection
        .prepare(
            "SELECT conversation_id FROM agent_actions WHERE conversation_id != ?1 AND (state = 'started' OR (state = 'interrupted' AND claim_token IS NOT NULL))",
        )
        .map_err(store_error)?;
    let rows = statement
        .query_map([conversation_id], |row| row.get::<_, i64>(0))
        .map_err(store_error)?;
    for row in rows {
        let other = row.map_err(store_error)?;
        let other_roots = conversation_roots(connection, other)?;
        if roots_overlap(&roots, &other_roots) {
            return Ok(true);
        }
    }
    Ok(false)
}

fn cancel_receipt(connection: &Connection, run: &RunRecord) -> AgentStoreResult<CancelReceipt> {
    let (cancelled, interrupted): (i64, i64) = connection
        .query_row(
            "SELECT (SELECT COUNT(*) FROM agent_actions WHERE run_id = ?1 AND state = 'cancelled'),
                    (SELECT COUNT(*) FROM agent_actions WHERE run_id = ?1 AND state = 'interrupted')",
            [run.id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(store_error)?;
    let writer_remaining: bool = connection
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM agent_actions WHERE run_id = ?1 AND claim_token IS NOT NULL AND state IN ('started', 'interrupted'))",
            [run.id],
            |row| row.get(0),
        )
        .map_err(store_error)?;
    Ok(CancelReceipt {
        run_id: run.id,
        state: RunState::Cancelled,
        cancelled_actions: cancelled as usize,
        interrupted_actions: interrupted as usize,
        writer_remaining,
    })
}

fn conversation_roots(
    connection: &Connection,
    conversation_id: i64,
) -> AgentStoreResult<Vec<RootRef>> {
    let context: Option<(Option<i64>, i64)> = connection
        .query_row(
            "SELECT project_id, context_revision FROM agent_contexts WHERE conversation_id = ?1",
            [conversation_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()
        .map_err(store_error)?;
    let Some((project_id, revision)) = context else {
        return Ok(Vec::new());
    };
    let mut roots = Vec::new();
    if let Some(project_id) = project_id
        && let Some((root_id, display_path)) = connection
            .query_row(
                "SELECT root_identity_key, display_path FROM agent_projects WHERE id = ?1",
                [project_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?)),
            )
            .optional()
            .map_err(store_error)?
    {
        roots.push(RootRef {
            root_id,
            display_path,
        });
    }
    let mut statement = connection
        .prepare(
            "SELECT root_id, display_path FROM agent_authorization_roots WHERE conversation_id = ?1 AND context_revision = ?2",
        )
        .map_err(store_error)?;
    let rows = statement
        .query_map(params![conversation_id, revision], |row| {
            Ok(RootRef {
                root_id: row.get(0)?,
                display_path: row.get(1)?,
            })
        })
        .map_err(store_error)?;
    for row in rows {
        roots.push(row.map_err(store_error)?);
    }
    Ok(roots)
}

fn roots_overlap(a: &[RootRef], b: &[RootRef]) -> bool {
    a.iter()
        .any(|left| b.iter().any(|right| root_refs_overlap(left, right)))
}

fn root_refs_overlap(left: &RootRef, right: &RootRef) -> bool {
    if left.root_id == right.root_id {
        return true;
    }
    let left_handle = match RootHandle::enroll(Path::new(&left.display_path)) {
        Ok(handle) => handle,
        Err(_) => return true,
    };
    let right_handle = match RootHandle::enroll(Path::new(&right.display_path)) {
        Ok(handle) => handle,
        Err(_) => return true,
    };
    let left_identity = left_handle
        .identity()
        .map(|identity| identity.storage_key());
    let right_identity = right_handle
        .identity()
        .map(|identity| identity.storage_key());
    if left_identity.ok().as_deref() != Some(left.root_id.as_str())
        || right_identity.ok().as_deref() != Some(right.root_id.as_str())
    {
        return true;
    }
    !matches!(
        left_handle.relation(&right_handle),
        Ok(RootRelation::Disjoint)
    )
}

fn parse_run_state(state: &str) -> AgentStoreResult<RunState> {
    serde_json::from_value(serde_json::Value::String(state.to_string()))
        .map_err(|_| store_error(rusqlite::Error::InvalidQuery))
}
