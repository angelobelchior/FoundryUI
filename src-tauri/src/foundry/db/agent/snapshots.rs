use rusqlite::{OptionalExtension, Row, params};
use serde::{Deserialize, Serialize};

use super::history::history_on;
use super::{AgentStore, AgentStoreError, AgentStoreResult, HistoryEntry, store_error};
use crate::foundry::jobs::now_millis;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationSnapshot {
    pub id: i64,
    pub destination_conversation_id: i64,
    pub source_conversation_id: i64,
    pub source_title: String,
    pub entries: Vec<HistoryEntry>,
    pub omitted_attachment_count: i64,
    pub captured_at: i64,
}

impl AgentStore<'_> {
    pub fn capture_snapshot(
        &self,
        source_id: i64,
        destination_id: i64,
    ) -> AgentStoreResult<ConversationSnapshot> {
        let mut connection = self.database.connection();
        let transaction = connection.transaction().map_err(store_error)?;
        let title: String = transaction
            .query_row(
                "SELECT title FROM conversations WHERE id = ?1",
                [source_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(store_error)?
            .ok_or(AgentStoreError::NotFound)?;
        let destination_exists: bool = transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM conversations WHERE id = ?1)",
                [destination_id],
                |row| row.get(0),
            )
            .map_err(store_error)?;
        if !destination_exists {
            return Err(AgentStoreError::NotFound);
        }
        let entries = history_on(&transaction, source_id)?;
        let encoded = serde_json::to_string(&entries).map_err(|_| AgentStoreError::Storage)?;
        if encoded.len() > 1_048_576 {
            return Err(AgentStoreError::InvalidInput);
        }
        let omitted: i64 = transaction.query_row(
            "SELECT COUNT(*) FROM attachments JOIN messages ON messages.id = attachments.message_id WHERE messages.conversation_id = ?1",
            [source_id], |row| row.get(0),
        ).map_err(store_error)?;
        let captured_at = now_millis();
        transaction.execute(
            "INSERT INTO conversation_snapshots (destination_conversation_id, source_conversation_id, source_title, entries_json, omitted_attachment_count, captured_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![destination_id, source_id, title, encoded, omitted, captured_at],
        ).map_err(store_error)?;
        let snapshot = ConversationSnapshot {
            id: transaction.last_insert_rowid(),
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
            "SELECT id, destination_conversation_id, source_conversation_id, source_title, entries_json, omitted_attachment_count, captured_at
             FROM conversation_snapshots WHERE id = ?1 AND destination_conversation_id = ?2",
            params![id, destination_id], snapshot_from_row,
        ).optional().map_err(store_error)?.ok_or(AgentStoreError::NotFound)
    }
}

fn snapshot_from_row(row: &Row<'_>) -> rusqlite::Result<ConversationSnapshot> {
    Ok(ConversationSnapshot {
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
