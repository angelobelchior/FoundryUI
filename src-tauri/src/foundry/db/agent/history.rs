use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};

use super::{
    ActionOutcome, AgentStore, AgentStoreError, AgentStoreResult, read_run, store_error,
    validate_key,
};
use crate::foundry::db::MessageRole;
use crate::foundry::jobs::now_millis;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ActionState {
    Proposed,
    Approved,
    Denied,
    Started,
    Complete,
    Failed,
    Cancelled,
    Interrupted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActionRecord {
    pub id: i64,
    pub run_id: i64,
    pub conversation_id: i64,
    pub context_revision: i64,
    pub call_id: String,
    pub tool: String,
    pub arguments: Value,
    pub arguments_fingerprint: String,
    pub state: ActionState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HistoryContent {
    Text {
        role: MessageRole,
        content: String,
        reasoning: String,
    },
    ToolCall {
        call_id: String,
        tool: String,
        arguments: Value,
    },
    ToolResult {
        call_id: String,
        outcome: ActionOutcome,
        summary: Value,
    },
    StateEvent {
        event: AgentEventState,
        action_id: Option<i64>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        detail: Option<String>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentEventState {
    RunPrepared,
    TurnStarted,
    Approved,
    Denied,
    AuthorizationPending,
    Started,
    ToolRejected,
    ToolCompleted,
    ToolFailed,
    LimitReached,
    RunCompleted,
    RunFailed,
    RunCancelled,
    ActionCancelled,
    RunInterrupted,
    ActionInterrupted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub sequence: i64,
    pub run_id: Option<i64>,
    pub content: HistoryContent,
}

impl AgentStore<'_> {
    pub fn propose_call(
        &self,
        run_id: i64,
        call_id: &str,
        tool: &str,
        arguments: &Value,
    ) -> AgentStoreResult<ActionRecord> {
        validate_key(call_id)?;
        validate_key(tool)?;
        if !arguments.is_object() {
            return Err(AgentStoreError::InvalidInput);
        }
        let fingerprint = fingerprint(arguments)?;
        let sanitized = redact(arguments);
        let encoded =
            serde_json::to_string(&sanitized).map_err(|_| AgentStoreError::InvalidInput)?;
        let mut connection = self.database.connection();
        let transaction = connection.transaction().map_err(store_error)?;
        let run = read_run(&transaction, run_id)?;
        let existing: Option<i64> = transaction
            .query_row(
                "SELECT id FROM agent_actions WHERE run_id = ?1 AND call_id = ?2",
                params![run_id, call_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(store_error)?;
        if let Some(id) = existing {
            let action = read_action(&transaction, id)?;
            if action.tool != tool || action.arguments_fingerprint != fingerprint {
                return Err(AgentStoreError::Conflict);
            }
            return Ok(action);
        }
        super::lifecycle::require_active_run(&transaction, &run)?;
        let now = now_millis();
        transaction.execute(
            "INSERT INTO agent_actions (run_id, conversation_id, context_revision, call_id, tool, arguments_json, arguments_fingerprint, state, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 'proposed', ?8, ?8)",
            params![run.id, run.conversation_id, run.context_revision, call_id, tool, encoded, fingerprint, now],
        ).map_err(store_error)?;
        let id = transaction.last_insert_rowid();
        let content = HistoryContent::ToolCall {
            call_id: call_id.to_string(),
            tool: tool.to_string(),
            arguments: sanitized,
        };
        insert_history(
            &transaction,
            run.conversation_id,
            run.id,
            Some(id),
            &content,
        )?;
        let action = read_action(&transaction, id)?;
        transaction.commit().map_err(store_error)?;
        Ok(action)
    }

    pub fn history(&self, conversation_id: i64) -> AgentStoreResult<Vec<HistoryEntry>> {
        history_on(&self.database.connection(), conversation_id)
    }

    pub fn events(&self, conversation_id: i64) -> AgentStoreResult<Vec<HistoryEntry>> {
        events_on(&self.database.connection(), conversation_id)
    }

    pub fn action(&self, id: i64) -> AgentStoreResult<ActionRecord> {
        read_action(&self.database.connection(), id)
    }
}

pub(super) fn insert_history(
    connection: &Connection,
    conversation_id: i64,
    run_id: i64,
    action_id: Option<i64>,
    content: &HistoryContent,
) -> AgentStoreResult<()> {
    let sequence = super::next_sequence(connection, conversation_id)?;
    let encoded = serde_json::to_string(content).map_err(|_| AgentStoreError::InvalidInput)?;
    connection.execute(
        "INSERT INTO agent_history (conversation_id, run_id, action_id, seq, content_json, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![conversation_id, run_id, action_id, sequence, encoded, now_millis()],
    ).map_err(store_error)?;
    Ok(())
}

pub(super) fn history_on(
    connection: &Connection,
    conversation_id: i64,
) -> AgentStoreResult<Vec<HistoryEntry>> {
    let mut entries = events_on(connection, conversation_id)?;
    entries.retain(|entry| !matches!(entry.content, HistoryContent::StateEvent { .. }));
    Ok(entries)
}

fn events_on(connection: &Connection, conversation_id: i64) -> AgentStoreResult<Vec<HistoryEntry>> {
    let mut statement = connection.prepare(
        "SELECT seq, NULL, role, content_md, reasoning_md, NULL FROM messages WHERE conversation_id = ?1
         UNION ALL
         SELECT seq, run_id, NULL, NULL, NULL, content_json FROM agent_history WHERE conversation_id = ?1
         ORDER BY 1",
    ).map_err(store_error)?;
    let entries = statement
        .query_map([conversation_id], |row| {
            let encoded: Option<String> = row.get(5)?;
            let content = if let Some(encoded) = encoded {
                serde_json::from_str(&encoded).map_err(|_| rusqlite::Error::InvalidQuery)?
            } else {
                HistoryContent::Text {
                    role: MessageRole::parse(&row.get::<_, String>(2)?)
                        .map_err(|_| rusqlite::Error::InvalidQuery)?,
                    content: row.get(3)?,
                    reasoning: row.get(4)?,
                }
            };
            Ok(HistoryEntry {
                sequence: row.get(0)?,
                run_id: row.get(1)?,
                content,
            })
        })
        .map_err(store_error)?;
    entries
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(store_error)
}

pub(super) fn read_action(connection: &Connection, id: i64) -> AgentStoreResult<ActionRecord> {
    connection.query_row(
        "SELECT id, run_id, conversation_id, context_revision, call_id, tool, arguments_json, arguments_fingerprint, state FROM agent_actions WHERE id = ?1",
        [id], action_from_row,
    ).optional().map_err(store_error)?.ok_or(AgentStoreError::NotFound)
}

fn action_from_row(row: &Row<'_>) -> rusqlite::Result<ActionRecord> {
    let state: String = row.get(8)?;
    Ok(ActionRecord {
        id: row.get(0)?,
        run_id: row.get(1)?,
        conversation_id: row.get(2)?,
        context_revision: row.get(3)?,
        call_id: row.get(4)?,
        tool: row.get(5)?,
        arguments: serde_json::from_str(&row.get::<_, String>(6)?)
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        arguments_fingerprint: row.get(7)?,
        state: serde_json::from_value(Value::String(state))
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
    })
}

pub(super) fn redact_result(value: &Value) -> Value {
    const PRESERVED: [&str; 3] = ["status", "code", "outcome"];
    const STRUCTURED: [&str; 2] = ["output", "result"];
    match value {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .filter_map(|(key, value)| {
                    if PRESERVED.contains(&key.as_str()) {
                        return value.is_string().then(|| (key.clone(), value.clone()));
                    }
                    if STRUCTURED.contains(&key.as_str()) {
                        return Some((key.clone(), redact_result(value)));
                    }
                    match key.as_str() {
                        "truncated" if value.is_boolean() => Some((key.clone(), value.clone())),
                        "bytes" if value.is_number() => Some((key.clone(), value.clone())),
                        _ => None,
                    }
                })
                .collect(),
        ),
        Value::Array(values) => Value::Array(
            values
                .iter()
                .filter_map(|value| match value {
                    Value::String(_) | Value::Number(_) | Value::Bool(_) => None,
                    _ => Some(redact_result(value)),
                })
                .collect(),
        ),
        Value::Null => Value::Null,
        _ => Value::String("[redacted]".to_string()),
    }
}

pub(super) fn redact(value: &Value) -> Value {
    match value {
        Value::Object(object) => Value::Object(
            object
                .iter()
                .enumerate()
                .map(|(index, (key, value))| {
                    let label = if [
                        "path",
                        "token",
                        "limit",
                        "offset",
                        "files",
                        "text",
                        "summary",
                        "code",
                        "result",
                        "status",
                        "name",
                        "operation",
                        "arguments",
                        "method",
                        "url",
                        "query",
                        "headers",
                        "body",
                        "command",
                        "args",
                        "error",
                        "message",
                        "metadata",
                        "value",
                        "key",
                        "content",
                        "bytes",
                        "count",
                        "start",
                        "end",
                        "line",
                        "column",
                        "cwd",
                        "working_directory",
                        "timeout",
                    ]
                    .contains(&key.as_str())
                    {
                        key.clone()
                    } else {
                        format!("$field_{index}")
                    };
                    (label, redact(value))
                })
                .collect(),
        ),
        Value::Array(values) => Value::Array(values.iter().map(redact).collect()),
        Value::Null => Value::Null,
        _ => Value::String("[redacted]".to_string()),
    }
}

pub(super) fn fingerprint(value: &Value) -> AgentStoreResult<String> {
    fn canonical(value: &Value) -> Value {
        match value {
            Value::Object(object) => {
                let mut keys: Vec<_> = object.keys().collect();
                keys.sort();
                Value::Object(
                    keys.into_iter()
                        .map(|key| (key.clone(), canonical(&object[key])))
                        .collect(),
                )
            }
            Value::Array(values) => Value::Array(values.iter().map(canonical).collect()),
            _ => value.clone(),
        }
    }
    let encoded =
        serde_json::to_vec(&canonical(value)).map_err(|_| AgentStoreError::InvalidInput)?;
    if encoded.len() > 65_536 {
        return Err(AgentStoreError::InvalidInput);
    }
    Ok(format!("{:x}", Sha256::digest(encoded)))
}
