use rusqlite::{OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};

use super::{AgentStore, AgentStoreError, AgentStoreResult, store_error};
use crate::foundry::jobs::now_millis;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgentMode {
    Review,
    Automatic,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationContext {
    pub conversation_id: i64,
    pub project_id: Option<i64>,
    pub revision: i64,
    pub mode: AgentMode,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectRecord {
    pub id: i64,
    pub name: String,
    pub root_identity_key: String,
    pub display_path: String,
    pub binding_revision: i64,
}

impl AgentStore<'_> {
    pub fn record_project(
        &self,
        name: &str,
        root_identity_key: &str,
        display_path: &str,
    ) -> AgentStoreResult<ProjectRecord> {
        if name.trim().is_empty()
            || name.len() > 128
            || root_identity_key.is_empty()
            || root_identity_key.len() > 256
            || display_path.is_empty()
            || display_path.len() > 4096
            || [name, root_identity_key, display_path]
                .iter()
                .any(|value| value.chars().any(char::is_control))
        {
            return Err(AgentStoreError::InvalidInput);
        }
        let mut connection = self.database.connection();
        let transaction = connection.transaction().map_err(store_error)?;
        let now = now_millis();
        transaction.execute(
            "INSERT INTO agent_projects (name, root_identity_key, display_path, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?4) ON CONFLICT(root_identity_key) DO NOTHING",
            params![name.trim(), root_identity_key, display_path, now],
        ).map_err(store_error)?;
        let project = transaction.query_row(
            "SELECT id, name, root_identity_key, display_path, binding_revision FROM agent_projects WHERE root_identity_key = ?1",
            [root_identity_key], project_from_row,
        ).map_err(store_error)?;
        transaction.commit().map_err(store_error)?;
        Ok(project)
    }

    pub fn project(&self, id: i64) -> AgentStoreResult<ProjectRecord> {
        self.database.connection().query_row(
            "SELECT id, name, root_identity_key, display_path, binding_revision FROM agent_projects WHERE id = ?1", [id], project_from_row,
        ).optional().map_err(store_error)?.ok_or(AgentStoreError::NotFound)
    }

    pub fn context(&self, conversation_id: i64) -> AgentStoreResult<ConversationContext> {
        let mut connection = self.database.connection();
        let transaction = connection.transaction().map_err(store_error)?;
        transaction.execute(
            "INSERT INTO agent_contexts (conversation_id) SELECT id FROM conversations WHERE id = ?1 ON CONFLICT(conversation_id) DO NOTHING",
            [conversation_id],
        ).map_err(store_error)?;
        let context = transaction.query_row(
            "SELECT conversation_id, project_id, context_revision, mode FROM agent_contexts WHERE conversation_id = ?1",
            [conversation_id], |row| {
                let mode: String = row.get(3)?;
                Ok(ConversationContext {
                    conversation_id: row.get(0)?, project_id: row.get(1)?, revision: row.get(2)?,
                    mode: serde_json::from_value(serde_json::Value::String(mode)).map_err(|_| rusqlite::Error::InvalidQuery)?,
                })
            },
        ).optional().map_err(store_error)?.ok_or(AgentStoreError::NotFound)?;
        transaction.commit().map_err(store_error)?;
        Ok(context)
    }
}

pub(super) fn project_from_row(row: &Row<'_>) -> rusqlite::Result<ProjectRecord> {
    Ok(ProjectRecord {
        id: row.get(0)?,
        name: row.get(1)?,
        root_identity_key: row.get(2)?,
        display_path: row.get(3)?,
        binding_revision: row.get(4)?,
    })
}
