use rusqlite::{Connection, OptionalExtension, Row, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use std::sync::Arc;

use super::history::history_on;
use super::{
    AgentStore, AgentStoreError, AgentStoreResult, HistoryEntry, conversation_has_active_work,
    ensure_removable, store_error,
};
use crate::foundry::jobs::now_millis;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotContentAuthority {
    HistoricalEvidence,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SnapshotOrigin {
    pub conversation_id: i64,
    pub title: String,
    pub project_id: Option<i64>,
    pub project_name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SnapshotCandidate {
    pub origin: SnapshotOrigin,
    pub archived: bool,
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum SnapshotOmission {
    UnsupportedAttachment {
        source_sequence: i64,
        media_kind: String,
        mime: String,
    },
    NestedConversationCopies {
        count: i64,
    },
    PrivateContextFacts {
        count: i64,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct CaptureBinding {
    origin: SnapshotOrigin,
    context_revision: i64,
    project_binding_revision: Option<i64>,
}

impl CaptureBinding {
    fn same_context(&self, other: &Self) -> bool {
        self.origin.conversation_id == other.origin.conversation_id
            && self.origin.project_id == other.origin.project_id
            && self.origin.project_name == other.origin.project_name
            && self.context_revision == other.context_revision
            && self.project_binding_revision == other.project_binding_revision
    }
}

#[derive(Debug, Clone)]
pub struct SnapshotCapturePlan {
    database_identity: Arc<()>,
    source: CaptureBinding,
    destination: CaptureBinding,
}

impl SnapshotCapturePlan {
    pub fn source_conversation_id(&self) -> i64 {
        self.source.origin.conversation_id
    }

    pub fn destination_conversation_id(&self) -> i64 {
        self.destination.origin.conversation_id
    }

    pub fn source_origin(&self) -> &SnapshotOrigin {
        &self.source.origin
    }

    pub fn destination_origin(&self) -> &SnapshotOrigin {
        &self.destination.origin
    }

    pub fn crossed_project_boundary(&self) -> bool {
        self.source.origin.project_id != self.destination.origin.project_id
    }

    pub fn expected_source_context_revision(&self) -> i64 {
        self.source.context_revision
    }

    pub fn expected_destination_context_revision(&self) -> i64 {
        self.destination.context_revision
    }
}

#[derive(Debug, Clone)]
pub struct SnapshotRemovalPlan {
    database_identity: Arc<()>,
    destination_conversation_id: i64,
    destination_context_revision: i64,
    snapshot_id: i64,
    source_origin: SnapshotOrigin,
}

impl SnapshotRemovalPlan {
    pub fn destination_conversation_id(&self) -> i64 {
        self.destination_conversation_id
    }

    pub fn snapshot_id(&self) -> i64 {
        self.snapshot_id
    }

    pub fn source_origin(&self) -> &SnapshotOrigin {
        &self.source_origin
    }

    pub fn expected_destination_context_revision(&self) -> i64 {
        self.destination_context_revision
    }
}

#[derive(Serialize, Deserialize)]
struct SnapshotMetadata {
    origin: SnapshotOrigin,
    crossed_project_boundary: bool,
    source_may_be_partial: bool,
    omissions: Vec<SnapshotOmission>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationSnapshot {
    pub read_only: bool,
    pub reference_only: bool,
    pub content_authority: SnapshotContentAuthority,
    pub origin: SnapshotOrigin,
    pub crossed_project_boundary: bool,
    pub source_may_be_partial: bool,
    pub omissions: Vec<SnapshotOmission>,
    pub id: i64,
    pub destination_conversation_id: i64,
    pub source_conversation_id: i64,
    pub source_title: String,
    pub entries: Vec<HistoryEntry>,
    pub omitted_attachment_count: i64,
    pub captured_at: i64,
}

impl AgentStore<'_> {
    pub fn snapshot_candidates(
        &self,
        project_id: Option<i64>,
        query: Option<&str>,
    ) -> AgentStoreResult<Vec<SnapshotCandidate>> {
        let query = query.unwrap_or("").trim();
        if query.len() > 4096 {
            return Err(AgentStoreError::InvalidInput);
        }
        let connection = self.database.connection();
        let mut statement = connection.prepare(
            "SELECT c.id, c.title, a.project_id, p.name, c.archived, c.updated_at
             FROM conversations c LEFT JOIN agent_contexts a ON a.conversation_id = c.id
             LEFT JOIN agent_projects p ON p.id = a.project_id
             WHERE (?1 IS NULL OR a.project_id = ?1)
               AND (?2 = '' OR instr(lower(c.title), lower(?2)) > 0 OR EXISTS(
                 SELECT 1 FROM messages m WHERE m.conversation_id = c.id AND instr(lower(m.content_md), lower(?2)) > 0))
             ORDER BY c.updated_at DESC, c.id DESC",
        ).map_err(store_error)?;
        statement
            .query_map(params![project_id, query], |row| {
                Ok(SnapshotCandidate {
                    origin: SnapshotOrigin {
                        conversation_id: row.get(0)?,
                        title: row.get(1)?,
                        project_id: row.get(2)?,
                        project_name: row.get(3)?,
                    },
                    archived: row.get(4)?,
                    updated_at: row.get(5)?,
                })
            })
            .map_err(store_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(store_error)
    }

    pub fn capture_snapshot(
        &self,
        source_id: i64,
        destination_id: i64,
    ) -> AgentStoreResult<ConversationSnapshot> {
        let plan = self.prepare_snapshot_capture(source_id, destination_id)?;
        self.commit_snapshot_capture(&plan)
    }

    pub fn prepare_snapshot_capture(
        &self,
        source_id: i64,
        destination_id: i64,
    ) -> AgentStoreResult<SnapshotCapturePlan> {
        let mut connection = self.database.connection();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(store_error)?;
        let source = capture_binding(&transaction, source_id)?;
        let destination = capture_binding(&transaction, destination_id)?;
        ensure_removable(&transaction, destination_id)?;
        transaction.commit().map_err(store_error)?;
        Ok(SnapshotCapturePlan {
            database_identity: Arc::clone(&self.database.authority_instance),
            source,
            destination,
        })
    }

    pub fn commit_snapshot_capture(
        &self,
        plan: &SnapshotCapturePlan,
    ) -> AgentStoreResult<ConversationSnapshot> {
        if !Arc::ptr_eq(&plan.database_identity, &self.database.authority_instance) {
            return Err(AgentStoreError::StaleContext);
        }
        let mut connection = self.database.connection();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(store_error)?;
        for expected in [&plan.source, &plan.destination] {
            let current = capture_binding(&transaction, expected.origin.conversation_id).map_err(
                |error| {
                    if error == AgentStoreError::NotFound {
                        AgentStoreError::StaleContext
                    } else {
                        error
                    }
                },
            )?;
            if !current.same_context(expected) {
                return Err(AgentStoreError::StaleContext);
            }
        }
        let source_id = plan.source_conversation_id();
        let destination_id = plan.destination_conversation_id();
        ensure_removable(&transaction, destination_id)?;
        let origin = capture_binding(&transaction, source_id)?.origin;
        let title = origin.title.clone();
        let entries = history_on(&transaction, source_id)?;
        let encoded = serde_json::to_string(&entries).map_err(|_| AgentStoreError::Storage)?;
        let mut omissions = {
            let mut statement = transaction.prepare("SELECT m.seq, a.kind, COALESCE(a.mime, 'application/octet-stream') FROM attachments a JOIN messages m ON m.id = a.message_id WHERE m.conversation_id = ?1 ORDER BY m.seq, a.id").map_err(store_error)?;
            statement
                .query_map([source_id], |row| {
                    Ok(SnapshotOmission::UnsupportedAttachment {
                        source_sequence: row.get(0)?,
                        media_kind: row.get(1)?,
                        mime: row.get(2)?,
                    })
                })
                .map_err(store_error)?
                .collect::<rusqlite::Result<Vec<_>>>()
                .map_err(store_error)?
        };
        let omitted = omissions.len() as i64;
        let nested_count: i64 = transaction.query_row("SELECT COUNT(*) FROM conversation_snapshots WHERE destination_conversation_id = ?1", [source_id], |row| row.get(0)).map_err(store_error)?;
        if nested_count > 0 {
            omissions.push(SnapshotOmission::NestedConversationCopies {
                count: nested_count,
            });
        }
        let private_fact_count: i64 = transaction
            .query_row(
                "SELECT COUNT(*) FROM agent_context_facts WHERE conversation_id = ?1",
                [source_id],
                |row| row.get(0),
            )
            .map_err(store_error)?;
        if private_fact_count > 0 {
            omissions.push(SnapshotOmission::PrivateContextFacts {
                count: private_fact_count,
            });
        }
        let source_may_be_partial = conversation_has_active_work(&transaction, source_id)?;
        let captured_at = now_millis();
        transaction.execute(
            "INSERT INTO conversation_snapshots (destination_conversation_id, source_conversation_id, source_title, entries_json, omitted_attachment_count, captured_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![destination_id, source_id, title, encoded, omitted, captured_at],
        ).map_err(store_error)?;
        let id = transaction.last_insert_rowid();
        let metadata = SnapshotMetadata {
            origin,
            crossed_project_boundary: plan.crossed_project_boundary(),
            source_may_be_partial,
            omissions,
        };
        let metadata_json =
            serde_json::to_string(&metadata).map_err(|_| AgentStoreError::Storage)?;
        transaction.execute("INSERT INTO conversation_snapshot_metadata (snapshot_id, metadata_json) VALUES (?1, ?2)", params![id, metadata_json]).map_err(store_error)?;
        let snapshot = ConversationSnapshot {
            read_only: true,
            reference_only: true,
            content_authority: SnapshotContentAuthority::HistoricalEvidence,
            origin: metadata.origin,
            crossed_project_boundary: metadata.crossed_project_boundary,
            source_may_be_partial: metadata.source_may_be_partial,
            omissions: metadata.omissions,
            id,
            destination_conversation_id: destination_id,
            source_conversation_id: source_id,
            source_title: title,
            entries,
            omitted_attachment_count: omitted,
            captured_at,
        };
        transaction.commit().map_err(store_error)?;
        Ok(snapshot)
    }

    pub fn snapshot(&self, destination_id: i64, id: i64) -> AgentStoreResult<ConversationSnapshot> {
        self.database.connection().query_row(
            "SELECT s.id, destination_conversation_id, source_conversation_id, source_title, entries_json, omitted_attachment_count, captured_at, metadata_json
             FROM conversation_snapshots s JOIN conversation_snapshot_metadata m ON m.snapshot_id = s.id WHERE s.id = ?1 AND destination_conversation_id = ?2",
            params![id, destination_id], snapshot_from_row,
        ).optional().map_err(store_error)?.ok_or(AgentStoreError::NotFound)
    }

    pub fn list_snapshots(
        &self,
        destination_id: i64,
    ) -> AgentStoreResult<Vec<ConversationSnapshot>> {
        let connection = self.database.connection();
        let mut statement = connection.prepare(
            "SELECT s.id, destination_conversation_id, source_conversation_id, source_title, entries_json, omitted_attachment_count, captured_at, metadata_json FROM conversation_snapshots s JOIN conversation_snapshot_metadata m ON m.snapshot_id = s.id WHERE destination_conversation_id = ?1 ORDER BY captured_at DESC, s.id DESC",
        ).map_err(store_error)?;
        statement
            .query_map([destination_id], snapshot_from_row)
            .map_err(store_error)?
            .collect::<rusqlite::Result<Vec<_>>>()
            .map_err(store_error)
    }

    pub fn remove_snapshot(&self, destination_id: i64, id: i64) -> AgentStoreResult<()> {
        let plan = self.prepare_snapshot_removal(destination_id, id)?;
        self.commit_snapshot_removal(&plan)
    }

    pub fn prepare_snapshot_removal(
        &self,
        destination_id: i64,
        id: i64,
    ) -> AgentStoreResult<SnapshotRemovalPlan> {
        let mut connection = self.database.connection();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(store_error)?;
        let encoded: String = transaction.query_row(
            "SELECT metadata_json FROM conversation_snapshots s JOIN conversation_snapshot_metadata m ON m.snapshot_id = s.id WHERE s.id = ?1 AND destination_conversation_id = ?2",
            params![id, destination_id], |row| row.get(0),
        ).optional().map_err(store_error)?.ok_or(AgentStoreError::NotFound)?;
        let metadata: SnapshotMetadata =
            serde_json::from_str(&encoded).map_err(|_| AgentStoreError::Storage)?;
        let destination = capture_binding(&transaction, destination_id)?;
        ensure_removable(&transaction, destination_id)?;
        transaction.commit().map_err(store_error)?;
        Ok(SnapshotRemovalPlan {
            database_identity: Arc::clone(&self.database.authority_instance),
            destination_conversation_id: destination_id,
            destination_context_revision: destination.context_revision,
            snapshot_id: id,
            source_origin: metadata.origin,
        })
    }

    pub fn commit_snapshot_removal(&self, plan: &SnapshotRemovalPlan) -> AgentStoreResult<()> {
        if !Arc::ptr_eq(&plan.database_identity, &self.database.authority_instance) {
            return Err(AgentStoreError::StaleContext);
        }
        let mut connection = self.database.connection();
        let transaction = connection
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(store_error)?;
        let destination_id = plan.destination_conversation_id;
        let id = plan.snapshot_id;
        let destination = capture_binding(&transaction, destination_id).map_err(|error| {
            if error == AgentStoreError::NotFound {
                AgentStoreError::StaleContext
            } else {
                error
            }
        })?;
        if destination.context_revision != plan.destination_context_revision {
            return Err(AgentStoreError::StaleContext);
        }
        let exists: bool = transaction.query_row("SELECT EXISTS(SELECT 1 FROM conversation_snapshots WHERE id = ?1 AND destination_conversation_id = ?2)", params![id, destination_id], |row| row.get(0)).map_err(store_error)?;
        if !exists {
            return Err(AgentStoreError::NotFound);
        }
        ensure_removable(&transaction, destination_id)?;
        transaction.execute("DELETE FROM conversation_snapshots WHERE id = ?1 AND destination_conversation_id = ?2", params![id, destination_id]).map_err(store_error)?;
        transaction.commit().map_err(store_error)?;
        Ok(())
    }
}

fn snapshot_from_row(row: &Row<'_>) -> rusqlite::Result<ConversationSnapshot> {
    let metadata: SnapshotMetadata = serde_json::from_str(&row.get::<_, String>(7)?)
        .map_err(|_| rusqlite::Error::InvalidQuery)?;
    Ok(ConversationSnapshot {
        read_only: true,
        reference_only: true,
        content_authority: SnapshotContentAuthority::HistoricalEvidence,
        origin: metadata.origin,
        crossed_project_boundary: metadata.crossed_project_boundary,
        source_may_be_partial: metadata.source_may_be_partial,
        omissions: metadata.omissions,
        id: row.get(0)?,
        destination_conversation_id: row.get(1)?,
        source_conversation_id: row.get(2)?,
        source_title: row.get(3)?,
        entries: serde_json::from_str(&row.get::<_, String>(4)?)
            .map_err(|_| rusqlite::Error::InvalidQuery)?,
        omitted_attachment_count: row.get(5)?,
        captured_at: row.get(6)?,
    })
}

fn capture_binding(connection: &Connection, id: i64) -> AgentStoreResult<CaptureBinding> {
    connection.query_row(
        "SELECT c.id, c.title, a.project_id, p.name, COALESCE(a.context_revision, 1), p.binding_revision
         FROM conversations c LEFT JOIN agent_contexts a ON a.conversation_id = c.id
         LEFT JOIN agent_projects p ON p.id = a.project_id WHERE c.id = ?1",
        [id], |row| Ok(CaptureBinding {
            origin: SnapshotOrigin { conversation_id: row.get(0)?, title: row.get(1)?, project_id: row.get(2)?, project_name: row.get(3)? },
            context_revision: row.get(4)?, project_binding_revision: row.get(5)?,
        }),
    ).optional().map_err(store_error)?.ok_or(AgentStoreError::NotFound)
}
