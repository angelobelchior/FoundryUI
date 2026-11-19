use rusqlite::{Connection, OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};

use super::history::fingerprint;
use super::{AgentStore, AgentStoreError, AgentStoreResult, read_run, store_error, validate_key};
use crate::foundry::jobs::now_millis;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SourceReference {
    pub identity_key: String,
    pub content_fingerprint: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkEventState {
    Requested,
    Blocked,
    Attempted,
    Redirected,
    Completed,
    Failed,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ContextFact {
    PermissionGrant {
        operation: String,
        constraints_fingerprint: String,
    },
    AuthorizedRoot {
        identity_key: String,
        display_path: String,
    },
    InstructionApproval {
        identity_key: String,
        content_fingerprint: String,
    },
    ContextManifest {
        sources: Vec<SourceReference>,
    },
    ContextBudget {
        manifest_json: String,
    },
    NetworkEvent {
        destination: String,
        state: NetworkEventState,
        http_status: Option<u16>,
    },
    ChangeSet {
        operation: String,
        path: String,
        base_fingerprint: Option<String>,
        proposed_fingerprint: Option<String>,
        proposal_fingerprint: String,
        additions: u32,
        deletions: u32,
        diff: String,
        diff_truncated: bool,
        resource_keys: Vec<String>,
    },
    RecoveryRecord {
        change_set_id: i64,
        git_reference: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FactRecord {
    pub id: i64,
    pub conversation_id: i64,
    pub context_revision: i64,
    pub run_id: Option<i64>,
    pub key: String,
    pub fact: ContextFact,
    pub created_at: i64,
}

impl AgentStore<'_> {
    pub fn record_fact(
        &self,
        conversation_id: i64,
        expected_revision: i64,
        run_id: Option<i64>,
        key: &str,
        fact: &ContextFact,
    ) -> AgentStoreResult<FactRecord> {
        validate_key(key)?;
        let payload_fingerprint =
            fingerprint(&serde_json::to_value(fact).map_err(|_| AgentStoreError::InvalidInput)?)?;
        let normalized = normalize(fact)?;
        let encoded =
            serde_json::to_string(&normalized).map_err(|_| AgentStoreError::InvalidInput)?;
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
        if let Some(run_id) = run_id {
            let run = read_run(&transaction, run_id)?;
            if run.conversation_id != conversation_id {
                return Err(AgentStoreError::Conflict);
            }
            if run.context_revision != revision {
                return Err(AgentStoreError::StaleContext);
            }
        }
        let related_id = if let ContextFact::RecoveryRecord { change_set_id, .. } = &normalized {
            let related = read_fact(&transaction, conversation_id, *change_set_id)?;
            if related.context_revision != revision
                || related.run_id != run_id
                || !matches!(related.fact, ContextFact::ChangeSet { .. })
            {
                return Err(AgentStoreError::Conflict);
            }
            Some(*change_set_id)
        } else {
            None
        };
        let existing: Option<(i64, String, Option<i64>)> = transaction.query_row(
            "SELECT id, payload_fingerprint, run_id FROM agent_context_facts WHERE conversation_id = ?1 AND context_revision = ?2 AND fact_key = ?3",
            params![conversation_id, revision, key], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        ).optional().map_err(store_error)?;
        if let Some((id, previous, previous_run)) = existing {
            if previous != payload_fingerprint || previous_run != run_id {
                return Err(AgentStoreError::Conflict);
            }
            return read_fact(&transaction, conversation_id, id);
        }
        transaction.execute(
            "INSERT INTO agent_context_facts (conversation_id, context_revision, run_id, fact_key, payload_json, payload_fingerprint, related_fact_id, created_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            params![conversation_id, revision, run_id, key, encoded, payload_fingerprint, related_id, now_millis()],
        ).map_err(store_error)?;
        let record = read_fact(
            &transaction,
            conversation_id,
            transaction.last_insert_rowid(),
        )?;
        transaction.commit().map_err(store_error)?;
        Ok(record)
    }

    pub fn record_context_budget(
        &self,
        conversation_id: i64,
        expected_revision: i64,
        run_id: Option<i64>,
        key: &str,
        manifest_json: &str,
    ) -> AgentStoreResult<FactRecord> {
        self.record_fact(
            conversation_id,
            expected_revision,
            run_id,
            key,
            &ContextFact::ContextBudget {
                manifest_json: manifest_json.to_string(),
            },
        )
    }

    pub fn context_budgets(&self, conversation_id: i64) -> AgentStoreResult<Vec<FactRecord>> {
        let facts = self.facts(conversation_id)?;
        let mut budgets = facts
            .into_iter()
            .filter(|fact| matches!(fact.fact, ContextFact::ContextBudget { .. }))
            .collect::<Vec<_>>();
        budgets.sort_by_key(|fact| std::cmp::Reverse(fact.id));
        budgets.truncate(200);
        Ok(budgets)
    }

    pub fn fact(&self, conversation_id: i64, id: i64) -> AgentStoreResult<FactRecord> {
        read_fact(&self.database.connection(), conversation_id, id)
    }

    pub fn fact_by_key(&self, conversation_id: i64, key: &str) -> AgentStoreResult<FactRecord> {
        let connection = self.database.connection();
        connection
            .query_row(
                "SELECT id, conversation_id, context_revision, run_id, fact_key, payload_json, created_at
                 FROM agent_context_facts WHERE conversation_id = ?1 AND fact_key = ?2
                 ORDER BY id DESC LIMIT 1",
                params![conversation_id, key],
                fact_from_row,
            )
            .optional()
            .map_err(store_error)?
            .ok_or(AgentStoreError::NotFound)
    }

    pub fn facts(&self, conversation_id: i64) -> AgentStoreResult<Vec<FactRecord>> {
        let connection = self.database.connection();
        let mut statement = connection.prepare(
            "SELECT id, conversation_id, context_revision, run_id, fact_key, payload_json, created_at
             FROM agent_context_facts WHERE conversation_id = ?1 ORDER BY id",
        ).map_err(store_error)?;
        statement
            .query_map([conversation_id], fact_from_row)
            .map_err(store_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(store_error)
    }

    pub fn active_facts(
        &self,
        conversation_id: i64,
        expected_revision: i64,
    ) -> AgentStoreResult<Vec<FactRecord>> {
        let connection = self.database.connection();
        let revision: i64 = connection
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
        let mut statement = connection.prepare(
            "SELECT id, conversation_id, context_revision, run_id, fact_key, payload_json, created_at
             FROM agent_context_facts WHERE conversation_id = ?1 AND context_revision = ?2 ORDER BY id",
        ).map_err(store_error)?;
        statement
            .query_map(params![conversation_id, revision], fact_from_row)
            .map_err(store_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(store_error)
    }
}

fn read_fact(
    connection: &Connection,
    conversation_id: i64,
    id: i64,
) -> AgentStoreResult<FactRecord> {
    connection.query_row(
        "SELECT id, conversation_id, context_revision, run_id, fact_key, payload_json, created_at
         FROM agent_context_facts WHERE id = ?1 AND conversation_id = ?2",
        params![id, conversation_id], fact_from_row,
    ).optional().map_err(store_error)?.ok_or(AgentStoreError::NotFound)
}

fn fact_from_row(row: &Row<'_>) -> rusqlite::Result<FactRecord> {
    Ok(FactRecord {
        id: row.get(0)?,
        conversation_id: row.get(1)?,
        context_revision: row.get(2)?,
        run_id: row.get(3)?,
        key: row.get(4)?,
        fact: serde_json::from_str(&row.get::<_, String>(5)?)
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        created_at: row.get(6)?,
    })
}

fn validate_identity(value: &str) -> AgentStoreResult<()> {
    if value.is_empty()
        || value.len() > 256
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"_.:/-".contains(&byte))
    {
        return Err(AgentStoreError::InvalidInput);
    }
    Ok(())
}

fn validate_fingerprint(value: &str) -> AgentStoreResult<()> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(AgentStoreError::InvalidInput);
    }
    Ok(())
}

fn normalize(fact: &ContextFact) -> AgentStoreResult<ContextFact> {
    match fact {
        ContextFact::PermissionGrant {
            operation,
            constraints_fingerprint,
        } => {
            validate_key(operation)?;
            validate_fingerprint(constraints_fingerprint)?;
        }
        ContextFact::AuthorizedRoot {
            identity_key,
            display_path,
        } => {
            validate_identity(identity_key)?;
            if display_path.is_empty()
                || display_path.len() > 4096
                || display_path.chars().any(char::is_control)
            {
                return Err(AgentStoreError::InvalidInput);
            }
        }
        ContextFact::InstructionApproval {
            identity_key,
            content_fingerprint,
        } => {
            validate_identity(identity_key)?;
            validate_fingerprint(content_fingerprint)?;
        }
        ContextFact::ContextManifest { sources } => {
            if sources.len() > 128 {
                return Err(AgentStoreError::InvalidInput);
            }
            for source in sources {
                validate_identity(&source.identity_key)?;
                validate_fingerprint(&source.content_fingerprint)?;
            }
        }
        ContextFact::ContextBudget { manifest_json } => {
            if manifest_json.len() > 131_072
                || !serde_json::from_str::<serde_json::Value>(manifest_json)
                    .map(|value| value.is_object())
                    .unwrap_or(false)
            {
                return Err(AgentStoreError::InvalidInput);
            }
        }
        ContextFact::NetworkEvent {
            destination,
            state,
            http_status,
        } => {
            if destination.len() > 8192 || destination.chars().any(char::is_control) {
                return Err(AgentStoreError::InvalidInput);
            }
            let mut url =
                reqwest::Url::parse(destination).map_err(|_| AgentStoreError::InvalidInput)?;
            if !matches!(url.scheme(), "http" | "https")
                || url.host_str().is_none()
                || http_status.is_some_and(|code| !(100..=599).contains(&code))
            {
                return Err(AgentStoreError::InvalidInput);
            }
            url.set_username("")
                .map_err(|_| AgentStoreError::InvalidInput)?;
            url.set_password(None)
                .map_err(|_| AgentStoreError::InvalidInput)?;
            url.set_path("/[redacted]");
            url.set_query(None);
            url.set_fragment(None);
            return Ok(ContextFact::NetworkEvent {
                destination: url.to_string(),
                state: *state,
                http_status: *http_status,
            });
        }
        ContextFact::ChangeSet {
            operation,
            path,
            base_fingerprint,
            proposed_fingerprint,
            proposal_fingerprint,
            additions,
            deletions,
            diff,
            diff_truncated,
            resource_keys,
        } => {
            if !matches!(operation.as_str(), "create" | "edit" | "delete")
                || path.is_empty()
                || path.len() > 4096
                || path.chars().any(char::is_control)
            {
                return Err(AgentStoreError::InvalidInput);
            }
            for fingerprint in [
                base_fingerprint,
                proposed_fingerprint,
                &Some(proposal_fingerprint.clone()),
            ]
            .into_iter()
            .flatten()
            {
                validate_fingerprint(fingerprint)?;
            }
            if diff.len() > 131_072
                || resource_keys.len() > 128
                || *additions > 1_000_000
                || *deletions > 1_000_000
            {
                return Err(AgentStoreError::InvalidInput);
            }
            for key in resource_keys {
                validate_identity(key)?;
            }
            let _ = diff_truncated;
        }
        ContextFact::RecoveryRecord {
            change_set_id,
            git_reference,
        } => {
            if *change_set_id <= 0
                || !git_reference.starts_with("refs/foundryui/")
                || git_reference.contains("..")
                || git_reference.ends_with(".lock")
                || git_reference.len() > 256
                || !git_reference
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"_./-".contains(&byte))
            {
                return Err(AgentStoreError::InvalidInput);
            }
        }
    }
    Ok(fact.clone())
}
