mod context;
mod history;
mod lifecycle;
mod projects;
mod records;
mod snapshots;

pub use context::{AgentMode, ConversationContext, ProjectRecord};
pub use history::{ActionRecord, ActionState, AgentEventState, HistoryContent, HistoryEntry};
pub use lifecycle::{ActionClaim, ActionOutcome, ApprovalDecision};
pub use projects::{ProjectAvailability, ProjectConversation};
pub use records::{ContextFact, FactRecord, NetworkEventState, SourceReference};
pub use snapshots::ConversationSnapshot;

use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};

use super::Database;
use crate::foundry::jobs::now_millis;

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AgentStoreError {
    #[error("agent storage operation failed")]
    Storage,
    #[error("agent database operation failed: {kind:?} (SQLite code: {sqlite_code:?})")]
    Database {
        kind: StorageFailureKind,
        sqlite_code: Option<i32>,
    },
    #[error("agent record not found")]
    NotFound,
    #[error("invalid agent storage input")]
    InvalidInput,
    #[error("agent record conflicts with an existing operation")]
    Conflict,
    #[error("agent context is no longer current")]
    StaleContext,
    #[error("invalid agent action transition")]
    InvalidTransition,
    #[error("agent action was already claimed")]
    AlreadyClaimed,
    #[error("agent conversation has unfinished or uncertain actions")]
    ConversationBusy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StorageFailureKind {
    Busy,
    Locked,
    Full,
    ReadOnly,
    Corrupt,
    Io,
    Constraint,
    Query,
    Decode,
    Other,
}

pub type AgentStoreResult<T> = Result<T, AgentStoreError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunState {
    Prepared,
    Running,
    Complete,
    Failed,
    Cancelled,
    Interrupted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunRecord {
    pub id: i64,
    pub conversation_id: i64,
    pub context_revision: i64,
    pub request_key: String,
    pub state: RunState,
}

pub struct AgentStore<'a> {
    database: &'a Database,
}

impl Database {
    pub fn agent_store(&self) -> AgentStore<'_> {
        AgentStore { database: self }
    }
}

impl AgentStore<'_> {
    pub fn start_run(
        &self,
        conversation_id: i64,
        request_key: &str,
    ) -> AgentStoreResult<RunRecord> {
        validate_key(request_key)?;
        let mut connection = self.database.connection();
        let transaction = connection.transaction().map_err(store_error)?;
        transaction
            .execute(
                "INSERT INTO agent_contexts (conversation_id)
             SELECT id FROM conversations WHERE id = ?1
             ON CONFLICT(conversation_id) DO NOTHING",
                [conversation_id],
            )
            .map_err(store_error)?;
        let revision: i64 = transaction
            .query_row(
                "SELECT context_revision FROM agent_contexts WHERE conversation_id = ?1",
                [conversation_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(store_error)?
            .ok_or(AgentStoreError::NotFound)?;
        let now = now_millis();
        let inserted = transaction.execute(
            "INSERT INTO agent_runs (conversation_id, context_revision, request_key, state, created_at, updated_at)
             VALUES (?1, ?2, ?3, 'prepared', ?4, ?4)
             ON CONFLICT(conversation_id, request_key) DO NOTHING",
            params![conversation_id, revision, request_key, now],
        ).map_err(store_error)?;
        let run = transaction
            .query_row(
                "SELECT id, conversation_id, context_revision, request_key, state
             FROM agent_runs WHERE conversation_id = ?1 AND request_key = ?2",
                params![conversation_id, request_key],
                run_from_row,
            )
            .map_err(store_error)?;
        if run.context_revision != revision {
            return Err(AgentStoreError::StaleContext);
        }
        if inserted == 1 {
            history::insert_history(
                &transaction,
                conversation_id,
                run.id,
                None,
                &HistoryContent::StateEvent {
                    event: AgentEventState::RunPrepared,
                    action_id: None,
                },
            )?;
        }
        transaction.commit().map_err(store_error)?;
        Ok(run)
    }

    pub fn run(&self, id: i64) -> AgentStoreResult<RunRecord> {
        read_run(&self.database.connection(), id)
    }

    pub fn finish_run(&self, id: i64) -> AgentStoreResult<RunRecord> {
        let mut connection = self.database.connection();
        let transaction = connection.transaction().map_err(store_error)?;
        let run = read_run(&transaction, id)?;
        if run.state == RunState::Complete {
            return Ok(run);
        }
        lifecycle::require_active_run(&transaction, &run)?;
        let pending: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM agent_actions WHERE run_id = ?1 AND state IN ('proposed', 'approved', 'started', 'interrupted'))",
            [id], |row| row.get(0),
        ).map_err(store_error)?;
        if pending {
            return Err(AgentStoreError::InvalidTransition);
        }
        transaction
            .execute(
                "UPDATE agent_runs SET state = 'complete', updated_at = ?1 WHERE id = ?2",
                params![now_millis(), id],
            )
            .map_err(store_error)?;
        history::insert_history(
            &transaction,
            run.conversation_id,
            run.id,
            None,
            &HistoryContent::StateEvent {
                event: AgentEventState::RunCompleted,
                action_id: None,
            },
        )?;
        let run = read_run(&transaction, id)?;
        transaction.commit().map_err(store_error)?;
        Ok(run)
    }

    pub fn advance_context(
        &self,
        conversation_id: i64,
        expected_revision: i64,
    ) -> AgentStoreResult<i64> {
        let mut connection = self.database.connection();
        let transaction = connection.transaction().map_err(store_error)?;
        let revision: i64 = transaction
            .query_row(
                "SELECT context_revision FROM agent_contexts WHERE conversation_id = ?1",
                [conversation_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(store_error)?
            .ok_or(AgentStoreError::NotFound)?;
        if revision != expected_revision {
            return Err(AgentStoreError::StaleContext);
        }
        if conversation_has_active_work(&transaction, conversation_id)? {
            return Err(AgentStoreError::InvalidTransition);
        }
        let next = revision
            .checked_add(1)
            .ok_or(AgentStoreError::InvalidInput)?;
        transaction
            .execute(
                "UPDATE agent_contexts SET context_revision = ?1 WHERE conversation_id = ?2",
                params![next, conversation_id],
            )
            .map_err(store_error)?;
        transaction.commit().map_err(store_error)?;
        Ok(next)
    }
}

fn validate_key(value: &str) -> AgentStoreResult<()> {
    if value.is_empty()
        || value.len() > 128
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.:-".contains(&byte))
    {
        return Err(AgentStoreError::InvalidInput);
    }
    Ok(())
}

fn read_run(connection: &Connection, id: i64) -> AgentStoreResult<RunRecord> {
    connection.query_row(
        "SELECT id, conversation_id, context_revision, request_key, state FROM agent_runs WHERE id = ?1",
        [id], run_from_row,
    ).optional().map_err(store_error)?.ok_or(AgentStoreError::NotFound)
}

fn run_from_row(row: &Row<'_>) -> rusqlite::Result<RunRecord> {
    let state: String = row.get(4)?;
    Ok(RunRecord {
        id: row.get(0)?,
        conversation_id: row.get(1)?,
        context_revision: row.get(2)?,
        request_key: row.get(3)?,
        state: serde_json::from_value(serde_json::Value::String(state))
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
    })
}

fn store_error(error: rusqlite::Error) -> AgentStoreError {
    let sqlite_code = error.sqlite_error().map(|failure| failure.extended_code);
    let kind = match sqlite_code.map(|code| code & 0xff) {
        Some(rusqlite::ffi::SQLITE_BUSY) => StorageFailureKind::Busy,
        Some(rusqlite::ffi::SQLITE_LOCKED) => StorageFailureKind::Locked,
        Some(rusqlite::ffi::SQLITE_FULL) => StorageFailureKind::Full,
        Some(rusqlite::ffi::SQLITE_READONLY) => StorageFailureKind::ReadOnly,
        Some(rusqlite::ffi::SQLITE_CORRUPT | rusqlite::ffi::SQLITE_NOTADB) => {
            StorageFailureKind::Corrupt
        }
        Some(rusqlite::ffi::SQLITE_IOERR | rusqlite::ffi::SQLITE_CANTOPEN) => {
            StorageFailureKind::Io
        }
        Some(rusqlite::ffi::SQLITE_CONSTRAINT) => StorageFailureKind::Constraint,
        Some(rusqlite::ffi::SQLITE_ERROR | rusqlite::ffi::SQLITE_SCHEMA) => {
            StorageFailureKind::Query
        }
        Some(_) => StorageFailureKind::Other,
        None => match error {
            rusqlite::Error::InvalidQuery
            | rusqlite::Error::InvalidColumnName(_)
            | rusqlite::Error::InvalidColumnIndex(_) => StorageFailureKind::Query,
            _ => StorageFailureKind::Decode,
        },
    };
    AgentStoreError::Database { kind, sqlite_code }
}

pub(super) fn next_sequence(
    connection: &Connection,
    conversation_id: i64,
) -> AgentStoreResult<i64> {
    connection
        .query_row(
            "SELECT MAX(
           COALESCE((SELECT MAX(seq) FROM messages WHERE conversation_id = ?1), 0),
           COALESCE((SELECT MAX(seq) FROM agent_history WHERE conversation_id = ?1), 0)
         ) + 1",
            [conversation_id],
            |row| row.get(0),
        )
        .map_err(store_error)
}

fn conversation_has_active_work(
    connection: &Connection,
    conversation_id: i64,
) -> AgentStoreResult<bool> {
    connection.query_row(
        "SELECT EXISTS(SELECT 1 FROM agent_runs WHERE conversation_id = ?1 AND state IN ('prepared', 'running'))
         OR EXISTS(SELECT 1 FROM agent_actions WHERE conversation_id = ?1 AND state = 'interrupted' AND claim_token IS NOT NULL)",
        [conversation_id], |row| row.get(0),
    ).map_err(store_error)
}

pub(super) fn ensure_removable(
    connection: &Connection,
    conversation_id: i64,
) -> AgentStoreResult<()> {
    if conversation_has_active_work(connection, conversation_id)? {
        return Err(AgentStoreError::ConversationBusy);
    }
    Ok(())
}

pub(super) fn recover_interrupted(connection: &Connection) -> AgentStoreResult<()> {
    let transaction = connection.unchecked_transaction().map_err(store_error)?;
    let now = now_millis();
    let actions = {
        let mut statement = transaction.prepare("SELECT id, conversation_id, run_id FROM agent_actions WHERE state IN ('proposed', 'approved', 'started')").map_err(store_error)?;
        statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, i64>(0)?,
                    row.get::<_, i64>(1)?,
                    row.get::<_, i64>(2)?,
                ))
            })
            .map_err(store_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(store_error)?
    };
    let runs = {
        let mut statement = transaction
            .prepare(
                "SELECT id, conversation_id FROM agent_runs WHERE state IN ('prepared', 'running')",
            )
            .map_err(store_error)?;
        statement
            .query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)))
            .map_err(store_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(store_error)?
    };
    transaction
        .execute(
            "UPDATE agent_actions SET state = 'interrupted', updated_at = ?1
         WHERE state IN ('proposed', 'approved', 'started')",
            [now],
        )
        .map_err(store_error)?;
    for (id, conversation_id, run_id) in actions {
        history::insert_history(
            &transaction,
            conversation_id,
            run_id,
            Some(id),
            &HistoryContent::StateEvent {
                event: AgentEventState::ActionInterrupted,
                action_id: Some(id),
            },
        )?;
    }
    for (id, conversation_id) in runs {
        history::insert_history(
            &transaction,
            conversation_id,
            id,
            None,
            &HistoryContent::StateEvent {
                event: AgentEventState::RunInterrupted,
                action_id: None,
            },
        )?;
    }
    transaction
        .execute(
            "UPDATE agent_runs SET state = 'interrupted', updated_at = ?1
         WHERE state IN ('prepared', 'running')",
            [now],
        )
        .map_err(store_error)?;
    transaction.commit().map_err(store_error)?;
    Ok(())
}

#[cfg(test)]
mod tests;

#[cfg(all(test, target_os = "macos"))]
mod project_tests;
