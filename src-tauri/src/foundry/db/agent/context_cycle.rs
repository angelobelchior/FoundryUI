use rusqlite::{Connection, OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};

use super::{
    AgentMode, AgentStore, AgentStoreError, AgentStoreResult, ConversationContext,
    ensure_removable, store_error, validate_key,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProjectRemovalMode {
    Keep,
    Delete,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ContextCycleOperation {
    MoveConversation {
        conversation_id: i64,
        destination_project_id: Option<i64>,
    },
    RemoveProject {
        project_id: i64,
        mode: ProjectRemovalMode,
    },
    DeleteConversation {
        conversation_id: i64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ProjectBinding {
    id: i64,
    revision: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct PlanData {
    operation: ContextCycleOperation,
    conversations: Vec<ConversationContext>,
    projects: Vec<ProjectBinding>,
}

#[derive(Debug, Clone)]
pub struct ContextCyclePlan {
    request_key: String,
    token: String,
    data: PlanData,
}

impl ContextCyclePlan {
    pub fn request_key(&self) -> &str {
        &self.request_key
    }
    pub fn operation(&self) -> &ContextCycleOperation {
        &self.data.operation
    }
    pub fn conversations(&self) -> &[ConversationContext] {
        &self.data.conversations
    }
    pub fn project_binding_revision(&self, project_id: i64) -> Option<i64> {
        self.data
            .projects
            .iter()
            .find(|binding| binding.id == project_id)
            .map(|binding| binding.revision)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextCycleResult {
    data: ResultData,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResultData {
    contexts: Vec<ConversationContext>,
    removed_project_id: Option<i64>,
    deleted_conversation_ids: Vec<i64>,
}

#[derive(Serialize, Deserialize)]
struct CompletionReceipt {
    contexts: Vec<ContextRevision>,
    removed_project_id: Option<i64>,
    deleted_conversation_ids: Vec<i64>,
}

#[derive(Serialize, Deserialize)]
struct ContextRevision {
    conversation_id: i64,
    project_id: Option<i64>,
    revision: i64,
}

impl ContextCycleResult {
    pub fn contexts(&self) -> &[ConversationContext] {
        &self.data.contexts
    }
    pub fn removed_project_id(&self) -> Option<i64> {
        self.data.removed_project_id
    }
    pub fn deleted_conversation_ids(&self) -> &[i64] {
        &self.data.deleted_conversation_ids
    }
}

impl AgentStore<'_> {
    pub fn prepare_conversation_deletion(
        &self,
        request_key: &str,
        conversation_id: i64,
    ) -> AgentStoreResult<ContextCyclePlan> {
        self.prepare_context_cycle(
            request_key,
            ContextCycleOperation::DeleteConversation { conversation_id },
        )
    }

    pub fn prepare_project_removal(
        &self,
        request_key: &str,
        project_id: i64,
        mode: ProjectRemovalMode,
    ) -> AgentStoreResult<ContextCyclePlan> {
        self.prepare_context_cycle(
            request_key,
            ContextCycleOperation::RemoveProject { project_id, mode },
        )
    }

    pub fn prepare_conversation_move(
        &self,
        request_key: &str,
        conversation_id: i64,
        destination_project_id: Option<i64>,
    ) -> AgentStoreResult<ContextCyclePlan> {
        self.prepare_context_cycle(
            request_key,
            ContextCycleOperation::MoveConversation {
                conversation_id,
                destination_project_id,
            },
        )
    }

    fn prepare_context_cycle(
        &self,
        request_key: &str,
        operation: ContextCycleOperation,
    ) -> AgentStoreResult<ContextCyclePlan> {
        validate_key(request_key)?;
        let mut connection = self.database.connection();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(store_error)?;
        if let Some((token, encoded, _)) = receipt(&transaction, request_key)? {
            let data: PlanData =
                serde_json::from_str(&encoded).map_err(|_| AgentStoreError::Storage)?;
            if data.operation != operation {
                return Err(AgentStoreError::Conflict);
            }
            return Ok(ContextCyclePlan {
                request_key: request_key.to_string(),
                token,
                data,
            });
        }
        let mut projects = Vec::new();
        let conversations = match &operation {
            ContextCycleOperation::DeleteConversation { conversation_id } => {
                let context = read_context(&transaction, *conversation_id)?;
                if let Some(id) = context.project_id {
                    projects.push(read_binding(&transaction, id)?);
                }
                ensure_removable(&transaction, *conversation_id)?;
                vec![context]
            }
            ContextCycleOperation::RemoveProject { project_id, .. } => {
                projects.push(read_binding(&transaction, *project_id)?);
                let contexts = project_contexts(&transaction, *project_id)?;
                for context in &contexts {
                    ensure_removable(&transaction, context.conversation_id)?;
                }
                contexts
            }
            ContextCycleOperation::MoveConversation {
                conversation_id,
                destination_project_id,
            } => {
                let context = read_context(&transaction, *conversation_id)?;
                if context.project_id == *destination_project_id {
                    return Err(AgentStoreError::InvalidInput);
                }
                for id in [context.project_id, *destination_project_id]
                    .into_iter()
                    .flatten()
                {
                    projects.push(read_binding(&transaction, id)?);
                }
                ensure_removable(&transaction, *conversation_id)?;
                vec![context]
            }
        };
        let data = PlanData {
            operation,
            conversations,
            projects,
        };
        let encoded = serde_json::to_string(&data).map_err(|_| AgentStoreError::Storage)?;
        let token: String = transaction
            .query_row("SELECT lower(hex(randomblob(16)))", [], |row| row.get(0))
            .map_err(store_error)?;
        transaction.execute("INSERT INTO agent_context_cycle_receipts (request_key, token, plan_json) VALUES (?1, ?2, ?3)", params![request_key, token, encoded]).map_err(store_error)?;
        transaction.commit().map_err(store_error)?;
        Ok(ContextCyclePlan {
            request_key: request_key.to_string(),
            token,
            data,
        })
    }

    pub fn commit_context_cycle(
        &self,
        plan: &ContextCyclePlan,
    ) -> AgentStoreResult<ContextCycleResult> {
        let mut connection = self.database.connection();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(store_error)?;
        let (token, encoded, completed) =
            receipt(&transaction, &plan.request_key)?.ok_or(AgentStoreError::NotFound)?;
        let stored: PlanData =
            serde_json::from_str(&encoded).map_err(|_| AgentStoreError::Storage)?;
        if token != plan.token || stored != plan.data {
            return Err(AgentStoreError::Conflict);
        }
        if let Some(completed) = completed {
            let completed: CompletionReceipt =
                serde_json::from_str(&completed).map_err(|_| AgentStoreError::Storage)?;
            let contexts = completed
                .contexts
                .into_iter()
                .map(|context| {
                    let expected = plan
                        .data
                        .conversations
                        .iter()
                        .find(|expected| expected.conversation_id == context.conversation_id)
                        .ok_or(AgentStoreError::Storage)?;
                    Ok(ConversationContext {
                        conversation_id: context.conversation_id,
                        project_id: context.project_id,
                        revision: context.revision,
                        mode: expected.mode,
                    })
                })
                .collect::<AgentStoreResult<Vec<_>>>()?;
            return Ok(ContextCycleResult {
                data: ResultData {
                    contexts,
                    removed_project_id: completed.removed_project_id,
                    deleted_conversation_ids: completed.deleted_conversation_ids,
                },
            });
        }
        for expected in &plan.data.projects {
            if read_binding(&transaction, expected.id).map_err(stale_if_missing)? != *expected {
                return Err(AgentStoreError::StaleContext);
            }
        }
        if let ContextCycleOperation::RemoveProject { project_id, .. } = &plan.data.operation
            && project_contexts(&transaction, *project_id)? != plan.data.conversations
        {
            return Err(AgentStoreError::StaleContext);
        }
        for expected in &plan.data.conversations {
            if read_context(&transaction, expected.conversation_id).map_err(stale_if_missing)?
                != *expected
            {
                return Err(AgentStoreError::StaleContext);
            }
            ensure_removable(&transaction, expected.conversation_id)?;
        }
        let mut contexts = Vec::new();
        let mut removed_project_id = None;
        let mut deleted_conversation_ids = Vec::new();
        match &plan.data.operation {
            ContextCycleOperation::DeleteConversation { conversation_id } => {
                delete_conversation(&transaction, *conversation_id)?;
                deleted_conversation_ids.push(*conversation_id);
            }
            ContextCycleOperation::RemoveProject { project_id, mode } => {
                for context in &plan.data.conversations {
                    match mode {
                        ProjectRemovalMode::Keep => {
                            let mut next = context.clone();
                            next.project_id = None;
                            next.revision = next
                                .revision
                                .checked_add(1)
                                .ok_or(AgentStoreError::InvalidInput)?;
                            write_context(&transaction, &next)?;
                            contexts.push(next);
                        }
                        ProjectRemovalMode::Delete => {
                            delete_conversation(&transaction, context.conversation_id)?;
                            deleted_conversation_ids.push(context.conversation_id);
                        }
                    }
                }
                transaction
                    .execute("DELETE FROM agent_projects WHERE id = ?1", [project_id])
                    .map_err(store_error)?;
                removed_project_id = Some(*project_id);
            }
            ContextCycleOperation::MoveConversation {
                destination_project_id,
                ..
            } => {
                let mut next = plan.data.conversations[0].clone();
                next.project_id = *destination_project_id;
                next.revision = next
                    .revision
                    .checked_add(1)
                    .ok_or(AgentStoreError::InvalidInput)?;
                write_context(&transaction, &next)?;
                contexts.push(next);
            }
        }
        let result = ContextCycleResult {
            data: ResultData {
                contexts,
                removed_project_id,
                deleted_conversation_ids,
            },
        };
        let completed = CompletionReceipt {
            contexts: result
                .data
                .contexts
                .iter()
                .map(|context| ContextRevision {
                    conversation_id: context.conversation_id,
                    project_id: context.project_id,
                    revision: context.revision,
                })
                .collect(),
            removed_project_id: result.data.removed_project_id,
            deleted_conversation_ids: result.data.deleted_conversation_ids.clone(),
        };
        let encoded = serde_json::to_string(&completed).map_err(|_| AgentStoreError::Storage)?;
        transaction
            .execute(
                "UPDATE agent_context_cycle_receipts SET result_json = ?1 WHERE request_key = ?2",
                params![encoded, plan.request_key],
            )
            .map_err(store_error)?;
        transaction.commit().map_err(store_error)?;
        Ok(result)
    }
}

fn receipt(
    connection: &Connection,
    request_key: &str,
) -> AgentStoreResult<Option<(String, String, Option<String>)>> {
    connection.query_row("SELECT token, plan_json, result_json FROM agent_context_cycle_receipts WHERE request_key = ?1", [request_key], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?))).optional().map_err(store_error)
}

fn delete_conversation(connection: &Connection, id: i64) -> AgentStoreResult<()> {
    connection
        .execute("DELETE FROM conversations WHERE id = ?1", [id])
        .map_err(store_error)?;
    Ok(())
}

fn read_binding(connection: &Connection, id: i64) -> AgentStoreResult<ProjectBinding> {
    connection
        .query_row(
            "SELECT id, binding_revision FROM agent_projects WHERE id = ?1",
            [id],
            |row| {
                Ok(ProjectBinding {
                    id: row.get(0)?,
                    revision: row.get(1)?,
                })
            },
        )
        .optional()
        .map_err(store_error)?
        .ok_or(AgentStoreError::NotFound)
}

fn read_context(connection: &Connection, id: i64) -> AgentStoreResult<ConversationContext> {
    connection.query_row("SELECT c.id, a.project_id, COALESCE(a.context_revision, 1), COALESCE(a.mode, 'review') FROM conversations c LEFT JOIN agent_contexts a ON a.conversation_id = c.id WHERE c.id = ?1", [id], |row| {
        let mode: String = row.get(3)?;
        Ok(ConversationContext { conversation_id: row.get(0)?, project_id: row.get(1)?, revision: row.get(2)?, mode: serde_json::from_value(serde_json::Value::String(mode)).map_err(|_| rusqlite::Error::InvalidQuery)? })
    }).optional().map_err(store_error)?.ok_or(AgentStoreError::NotFound)
}

fn project_contexts(
    connection: &Connection,
    id: i64,
) -> AgentStoreResult<Vec<ConversationContext>> {
    let mut statement = connection.prepare("SELECT conversation_id FROM agent_contexts WHERE project_id = ?1 ORDER BY conversation_id").map_err(store_error)?;
    let ids = statement
        .query_map([id], |row| row.get::<_, i64>(0))
        .map_err(store_error)?
        .collect::<rusqlite::Result<Vec<_>>>()
        .map_err(store_error)?;
    ids.into_iter()
        .map(|id| read_context(connection, id))
        .collect()
}

fn write_context(connection: &Connection, context: &ConversationContext) -> AgentStoreResult<()> {
    let mode = match context.mode {
        AgentMode::Review => "review",
        AgentMode::Automatic => "automatic",
    };
    connection.execute("INSERT INTO agent_contexts (conversation_id, project_id, context_revision, mode) VALUES (?1, ?2, ?3, ?4) ON CONFLICT(conversation_id) DO UPDATE SET project_id = excluded.project_id, context_revision = excluded.context_revision", params![context.conversation_id, context.project_id, context.revision, mode]).map_err(store_error)?;
    Ok(())
}

fn stale_if_missing(error: AgentStoreError) -> AgentStoreError {
    if error == AgentStoreError::NotFound {
        AgentStoreError::StaleContext
    } else {
        error
    }
}
