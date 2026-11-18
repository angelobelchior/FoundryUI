use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde::Serialize;
use std::path::Path;

use super::context::project_from_row;
use super::{
    AgentMode, AgentStore, AgentStoreError, AgentStoreResult, ConversationContext, ProjectRecord,
    conversation_has_active_work, store_error,
};
use crate::foundry::db::Conversation;
use crate::foundry::jobs::now_millis;
use crate::foundry::resources::{ResourceErrorKind, RootHandle};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectAvailability {
    Available,
    Missing,
    Changed,
    Denied,
    Unsupported,
}

#[derive(Debug, Clone, Serialize)]
pub struct ProjectConversation {
    pub conversation: Conversation,
    pub context: ConversationContext,
}

impl ProjectRecord {
    pub fn availability(&self) -> ProjectAvailability {
        match open_stored_root(self) {
            Ok(_) => ProjectAvailability::Available,
            Err(availability) => availability,
        }
    }
}

impl AgentStore<'_> {
    pub fn list_project_conversations(
        &self,
        project_id: Option<i64>,
        archived: bool,
    ) -> AgentStoreResult<Vec<ProjectConversation>> {
        let ids = {
            let connection = self.database.connection();
            if let Some(id) = project_id {
                let exists: bool = connection
                    .query_row(
                        "SELECT EXISTS(SELECT 1 FROM agent_projects WHERE id = ?1)",
                        [id],
                        |row| row.get(0),
                    )
                    .map_err(store_error)?;
                if !exists {
                    return Err(AgentStoreError::NotFound);
                }
            }
            let mut statement = connection
                .prepare(
                    "SELECT c.id FROM conversations c
                 LEFT JOIN agent_contexts a ON a.conversation_id = c.id
                 WHERE a.project_id IS ?1 AND c.archived = ?2
                 ORDER BY c.favorite DESC, c.updated_at DESC, c.id DESC",
                )
                .map_err(store_error)?;
            statement
                .query_map(params![project_id, archived], |row| row.get::<_, i64>(0))
                .map_err(store_error)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(store_error)?
        };
        ids.into_iter()
            .map(|id| {
                Ok(ProjectConversation {
                    conversation: self
                        .database
                        .conversation(id)
                        .map_err(|_| AgentStoreError::Storage)?,
                    context: self.context(id)?,
                })
            })
            .collect()
    }

    pub fn relocate_project(&self, id: i64, root: &RootHandle) -> AgentStoreResult<ProjectRecord> {
        self.relocate_project_binding(id, None, root)
    }

    pub fn relocate_project_if_current(
        &self,
        expected: &ProjectRecord,
        root: &RootHandle,
    ) -> AgentStoreResult<ProjectRecord> {
        self.relocate_project_binding(expected.id, Some(expected), root)
    }

    fn relocate_project_binding(
        &self,
        id: i64,
        expected: Option<&ProjectRecord>,
        root: &RootHandle,
    ) -> AgentStoreResult<ProjectRecord> {
        let identity = root
            .identity()
            .map_err(|_| AgentStoreError::InvalidInput)?
            .storage_key();
        let path = root
            .display_path()
            .to_str()
            .ok_or(AgentStoreError::InvalidInput)?;
        if path.is_empty() || path.len() > 4096 || path.chars().any(char::is_control) {
            return Err(AgentStoreError::InvalidInput);
        }
        let mut connection = self.database.connection();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(store_error)?;
        let project = transaction.query_row("SELECT id, name, root_identity_key, display_path, binding_revision FROM agent_projects WHERE id = ?1", [id], project_from_row).optional().map_err(store_error)?.ok_or(AgentStoreError::NotFound)?;
        if expected.is_some_and(|expected| {
            expected.root_identity_key != project.root_identity_key
                || expected.display_path != project.display_path
                || expected.binding_revision != project.binding_revision
        }) {
            return Err(AgentStoreError::StaleContext);
        }
        if project.root_identity_key == identity && project.display_path == path {
            return Ok(project);
        }
        let next_binding_revision = project
            .binding_revision
            .checked_add(1)
            .ok_or(AgentStoreError::InvalidInput)?;
        let duplicate: bool = transaction.query_row("SELECT EXISTS(SELECT 1 FROM agent_projects WHERE root_identity_key = ?1 AND id != ?2)", params![identity, id], |row| row.get(0)).map_err(store_error)?;
        if duplicate {
            return Err(AgentStoreError::Conflict);
        }
        let streaming: bool = transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM messages m
             JOIN agent_contexts a ON a.conversation_id = m.conversation_id
             WHERE a.project_id = ?1 AND m.status = 'streaming')",
                [id],
                |row| row.get(0),
            )
            .map_err(store_error)?;
        if streaming {
            return Err(AgentStoreError::ConversationBusy);
        }
        let conversations = {
            let mut statement = transaction
                .prepare("SELECT conversation_id, context_revision FROM agent_contexts WHERE project_id = ?1")
                .map_err(store_error)?;
            statement
                .query_map([id], |row| {
                    Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?))
                })
                .map_err(store_error)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(store_error)?
        };
        for (conversation_id, revision) in conversations {
            if conversation_has_active_work(&transaction, conversation_id)? {
                return Err(AgentStoreError::ConversationBusy);
            }
            revision
                .checked_add(1)
                .ok_or(AgentStoreError::InvalidInput)?;
        }
        transaction.execute("UPDATE agent_contexts SET context_revision = context_revision + 1 WHERE project_id = ?1", [id]).map_err(store_error)?;
        transaction.execute("UPDATE agent_projects SET root_identity_key = ?1, display_path = ?2, updated_at = ?3, binding_revision = ?4 WHERE id = ?5", params![identity, path, now_millis(), next_binding_revision, id]).map_err(store_error)?;
        let updated = transaction.query_row("SELECT id, name, root_identity_key, display_path, binding_revision FROM agent_projects WHERE id = ?1", [id], project_from_row).map_err(store_error)?;
        transaction.commit().map_err(store_error)?;
        Ok(updated)
    }

    pub fn project_availability(&self, id: i64) -> AgentStoreResult<ProjectAvailability> {
        Ok(self.project(id)?.availability())
    }

    pub fn open_project_root(&self, id: i64) -> AgentStoreResult<RootHandle> {
        open_stored_root(&self.project(id)?).map_err(|availability| match availability {
            ProjectAvailability::Changed => AgentStoreError::StaleContext,
            _ => AgentStoreError::InvalidInput,
        })
    }

    pub fn create_project_conversation(
        &self,
        project_id: Option<i64>,
        model_alias: &str,
    ) -> AgentStoreResult<ProjectConversation> {
        if model_alias.trim().is_empty()
            || model_alias.len() > 4096
            || model_alias.chars().any(char::is_control)
        {
            return Err(AgentStoreError::InvalidInput);
        }
        let mut connection = self.database.connection();
        let transaction = connection.transaction().map_err(store_error)?;
        if let Some(id) = project_id {
            let exists: bool = transaction
                .query_row(
                    "SELECT EXISTS(SELECT 1 FROM agent_projects WHERE id = ?1)",
                    [id],
                    |row| row.get(0),
                )
                .map_err(store_error)?;
            if !exists {
                return Err(AgentStoreError::NotFound);
            }
        }
        transaction.execute("INSERT INTO conversations (title, model_alias, created_at, updated_at) VALUES ('', ?1, ?2, ?2)", params![model_alias, now_millis()]).map_err(store_error)?;
        let id = transaction.last_insert_rowid();
        transaction
            .execute(
                "INSERT INTO agent_contexts (conversation_id, project_id) VALUES (?1, ?2)",
                params![id, project_id],
            )
            .map_err(store_error)?;
        transaction.commit().map_err(store_error)?;
        drop(connection);
        Ok(ProjectConversation {
            conversation: self
                .database
                .conversation(id)
                .map_err(|_| AgentStoreError::Storage)?,
            context: ConversationContext {
                conversation_id: id,
                project_id,
                revision: 1,
                mode: AgentMode::Review,
            },
        })
    }

    pub fn create_project(
        &self,
        root: &RootHandle,
        name: Option<&str>,
    ) -> AgentStoreResult<ProjectRecord> {
        let identity = root.identity().map_err(|_| AgentStoreError::InvalidInput)?;
        let path = root
            .display_path()
            .to_str()
            .ok_or(AgentStoreError::InvalidInput)?;
        let default_name = root
            .display_path()
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(path);
        self.record_project(name.unwrap_or(default_name), &identity.storage_key(), path)
    }

    pub fn list_projects(&self) -> AgentStoreResult<Vec<ProjectRecord>> {
        let connection = self.database.connection();
        let mut statement = connection
            .prepare(
                "SELECT id, name, root_identity_key, display_path, binding_revision FROM agent_projects ORDER BY id",
            )
            .map_err(store_error)?;
        statement
            .query_map([], project_from_row)
            .map_err(store_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(store_error)
    }

    pub fn rename_project(&self, id: i64, name: &str) -> AgentStoreResult<ProjectRecord> {
        if name.trim().is_empty() || name.len() > 128 || name.chars().any(char::is_control) {
            return Err(AgentStoreError::InvalidInput);
        }
        let connection = self.database.connection();
        let changed = connection
            .execute(
                "UPDATE agent_projects SET name = ?1, updated_at = ?2 WHERE id = ?3",
                params![name.trim(), now_millis(), id],
            )
            .map_err(store_error)?;
        if changed == 0 {
            return Err(AgentStoreError::NotFound);
        }
        connection.query_row("SELECT id, name, root_identity_key, display_path, binding_revision FROM agent_projects WHERE id = ?1", [id], project_from_row).map_err(store_error)
    }
}

fn open_stored_root(project: &ProjectRecord) -> Result<RootHandle, ProjectAvailability> {
    let root = RootHandle::enroll(Path::new(&project.display_path))
        .map_err(|error| resource_availability(error.kind))?;
    let identity = root
        .identity()
        .map_err(|error| resource_availability(error.kind))?;
    if identity.storage_key() != project.root_identity_key {
        return Err(ProjectAvailability::Changed);
    }
    root.operation_io_available()
        .map_err(|error| resource_availability(error.kind))?;
    Ok(root)
}

fn resource_availability(kind: ResourceErrorKind) -> ProjectAvailability {
    match kind {
        ResourceErrorKind::Unavailable => ProjectAvailability::Missing,
        ResourceErrorKind::RootChanged
        | ResourceErrorKind::ResourceChanged
        | ResourceErrorKind::NotDirectory => ProjectAvailability::Changed,
        ResourceErrorKind::AccessDenied | ResourceErrorKind::Revoked => ProjectAvailability::Denied,
        _ => ProjectAvailability::Unsupported,
    }
}
