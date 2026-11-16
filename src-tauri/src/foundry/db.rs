use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

use rusqlite::{Connection, OptionalExtension, Row, params, params_from_iter};
use serde::{Deserialize, Serialize};

use super::attachments::StagedFile;
use super::chat::ChatParams;
use super::error::{FoundryError, FoundryResult};
use super::jobs::now_millis;
use super::params::ResolvedParams;

const SCHEMA: &str = include_str!("../../schema.sql");
const SCHEMA_STAMP_SUFFIX: &str = ".schema";
const STREAM_FLUSH_INTERVAL: Duration = Duration::from_millis(250);
const DEFAULT_PAGE: u32 = 80;
const MAX_PAGE: u32 = 500;
const TITLE_LIMIT: usize = 40;
const DEFAULT_SEARCH: u32 = 40;
const MAX_SEARCH: u32 = 200;
const MAX_SEARCH_TERMS: usize = 8;
#[cfg(test)]
pub const SNIPPET_START: &str = "\u{2}";
#[cfg(test)]
pub const SNIPPET_END: &str = "\u{3}";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Conversation {
    pub id: i64,
    pub title: String,
    pub model_alias: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub favorite: bool,
    pub archived: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageRole {
    User,
    Assistant,
    Divider,
}

impl MessageRole {
    fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Assistant => "assistant",
            Self::Divider => "divider",
        }
    }

    fn parse(value: &str) -> FoundryResult<Self> {
        match value {
            "user" => Ok(Self::User),
            "assistant" => Ok(Self::Assistant),
            "divider" => Ok(Self::Divider),
            other => Err(storage(format!("unknown message role {other}"))),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MessageStatus {
    Streaming,
    Complete,
    Cancelled,
    Error,
    Interrupted,
}

impl MessageStatus {
    fn as_str(self) -> &'static str {
        match self {
            Self::Streaming => "streaming",
            Self::Complete => "complete",
            Self::Cancelled => "cancelled",
            Self::Error => "error",
            Self::Interrupted => "interrupted",
        }
    }

    fn parse(value: &str) -> FoundryResult<Self> {
        match value {
            "streaming" => Ok(Self::Streaming),
            "complete" => Ok(Self::Complete),
            "cancelled" => Ok(Self::Cancelled),
            "error" => Ok(Self::Error),
            "interrupted" => Ok(Self::Interrupted),
            other => Err(storage(format!("unknown message status {other}"))),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TokenUsage {
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
}

impl TokenUsage {
    fn from_json(value: &str) -> Option<Self> {
        serde_json::from_str(value).ok()
    }

    fn to_json(self) -> String {
        serde_json::to_string(&self).unwrap_or_else(|_| "null".to_string())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MessageError {
    pub code: String,
    pub message_key: String,
    pub message: String,
    pub detail: Option<String>,
}

impl From<&FoundryError> for MessageError {
    fn from(error: &FoundryError) -> Self {
        Self {
            code: error.code().to_string(),
            message_key: error.message_key(),
            message: error.to_string(),
            detail: error.detail(),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Attachment {
    pub id: i64,
    pub message_id: i64,
    pub kind: String,
    pub mime: String,
    pub path: String,
    pub bytes: u64,
    pub width: Option<u32>,
    pub height: Option<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatMessage {
    pub id: i64,
    pub conversation_id: i64,
    pub seq: i64,
    pub role: MessageRole,
    pub content: String,
    pub reasoning: String,
    pub model_alias: Option<String>,
    pub params: Option<ResolvedParams>,
    pub usage: Option<TokenUsage>,
    pub error: Option<MessageError>,
    pub status: MessageStatus,
    pub attachments: Vec<Attachment>,
    pub created_at: i64,
    pub updated_at: i64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct MessagePage {
    pub messages: Vec<ChatMessage>,
    pub has_more: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelChange {
    pub conversation: Conversation,
    pub divider: Option<ChatMessage>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub message_id: i64,
    pub conversation_id: i64,
    pub conversation_title: String,
    pub role: MessageRole,
    pub model_alias: Option<String>,
    pub seq: i64,
    pub snippet: String,
    pub created_at: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TranscriptionParams {
    pub requested_language: Option<String>,
    pub temperature: f32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Transcription {
    pub id: i64,
    pub model_alias: String,
    pub source_path: String,
    pub language: Option<String>,
    pub params: Option<TranscriptionParams>,
    pub text: String,
    pub duration_ms: i64,
    pub audio_exists: bool,
    pub created_at: i64,
    pub favorite: bool,
    pub archived: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EmbeddingCorpus {
    pub id: i64,
    pub name: String,
    pub model_alias: String,
    pub dimensions: u32,
    pub document_count: u32,
    pub vector_count: u32,
    pub created_at: i64,
    pub updated_at: i64,
    pub favorite: bool,
    pub archived: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CorpusDocument {
    pub id: i64,
    pub corpus_id: i64,
    pub position: u32,
    pub text: String,
    pub source_path: Option<String>,
    pub created_at: i64,
}

#[derive(Debug, Clone)]
pub struct CorpusDocumentInput {
    pub text: String,
    pub source_path: Option<String>,
}

#[derive(Debug, Clone)]
pub struct CorpusVectorRecord {
    pub document: CorpusDocument,
    pub dimensions: u32,
    pub vector: Vec<u8>,
}

pub struct StreamBuffer {
    content: String,
    reasoning: String,
    last_flush: Instant,
    dirty: bool,
}

impl StreamBuffer {
    pub fn new(content: String, reasoning: String) -> Self {
        Self {
            content,
            reasoning,
            last_flush: Instant::now(),
            dirty: false,
        }
    }

    pub fn append(&mut self, content: &str, reasoning: &str) {
        self.content.push_str(content);
        self.reasoning.push_str(reasoning);

        if !content.is_empty() || !reasoning.is_empty() {
            self.dirty = true;
        }
    }

    pub fn due(&self) -> bool {
        self.dirty && self.last_flush.elapsed() >= STREAM_FLUSH_INTERVAL
    }

    pub fn mark_flushed(&mut self) {
        self.last_flush = Instant::now();
        self.dirty = false;
    }

    pub fn content(&self) -> &str {
        &self.content
    }

    pub fn reasoning(&self) -> &str {
        &self.reasoning
    }
}

pub struct Database {
    connection: Mutex<Connection>,
}

impl Database {
    pub fn open(path: &Path) -> FoundryResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(storage)?;
        }

        if is_stale_schema(path) {
            discard_database_files(path)?;
        }

        let connection = Connection::open(path).map_err(storage)?;
        let database = Self::prepare(connection)?;
        write_schema_stamp(path)?;

        Ok(database)
    }

    pub fn open_in_memory() -> FoundryResult<Self> {
        let connection = Connection::open_in_memory().map_err(storage)?;

        Self::prepare(connection)
    }

    fn prepare(connection: Connection) -> FoundryResult<Self> {
        connection
            .execute_batch(
                "PRAGMA journal_mode = WAL;
                 PRAGMA synchronous = NORMAL;
                 PRAGMA foreign_keys = ON;
                 PRAGMA busy_timeout = 5000;",
            )
            .map_err(storage)?;

        apply_schema(&connection)?;
        recover_interrupted(&connection)?;

        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    pub fn list_conversations(&self, archived: bool) -> FoundryResult<Vec<Conversation>> {
        let connection = self.connection();
        let mut statement = connection
            .prepare(
                "SELECT id, title, model_alias, created_at, updated_at, favorite, archived
                 FROM conversations
                 WHERE archived = ?1
                 ORDER BY favorite DESC, updated_at DESC, id DESC",
            )
            .map_err(storage)?;
        let rows = statement
            .query_map([archived], conversation_from_row)
            .map_err(storage)?;

        collect(rows)
    }

    pub fn conversation(&self, id: i64) -> FoundryResult<Conversation> {
        let connection = self.connection();
        let conversation = connection
            .query_row(
                "SELECT id, title, model_alias, created_at, updated_at, favorite, archived
                 FROM conversations
                 WHERE id = ?1",
                [id],
                conversation_from_row,
            )
            .optional()
            .map_err(storage)?;

        conversation.ok_or_else(|| storage(format!("conversation {id} does not exist")))
    }

    pub fn set_conversation_archived(
        &self,
        id: i64,
        archived: bool,
    ) -> FoundryResult<Conversation> {
        let connection = self.connection();

        connection
            .execute(
                "UPDATE conversations SET archived = ?1 WHERE id = ?2",
                params![archived, id],
            )
            .map_err(storage)?;

        drop(connection);

        self.conversation(id)
    }

    pub fn set_conversation_favorite(
        &self,
        id: i64,
        favorite: bool,
    ) -> FoundryResult<Conversation> {
        let connection = self.connection();

        connection
            .execute(
                "UPDATE conversations SET favorite = ?1 WHERE id = ?2",
                params![favorite, id],
            )
            .map_err(storage)?;

        drop(connection);

        self.conversation(id)
    }

    pub fn create_conversation(&self, model_alias: &str) -> FoundryResult<Conversation> {
        let now = now_millis();
        let connection = self.connection();

        connection
            .execute(
                "INSERT INTO conversations (title, model_alias, created_at, updated_at)
                 VALUES ('', ?1, ?2, ?2)",
                params![model_alias, now],
            )
            .map_err(storage)?;
        let id = connection.last_insert_rowid();

        drop(connection);

        self.conversation(id)
    }

    pub fn rename_conversation(&self, id: i64, title: &str) -> FoundryResult<Conversation> {
        let trimmed = title.trim();

        if trimmed.is_empty() {
            return Err(FoundryError::CommandFailed {
                command: "chat_rename_conversation".to_string(),
                message: "the title is empty".to_string(),
                hint: None,
            });
        }

        let now = now_millis();
        let connection = self.connection();

        connection
            .execute(
                "UPDATE conversations SET title = ?1, updated_at = ?2 WHERE id = ?3",
                params![trimmed, now, id],
            )
            .map_err(storage)?;

        drop(connection);

        self.conversation(id)
    }

    pub fn delete_conversation(&self, id: i64) -> FoundryResult<()> {
        let connection = self.connection();

        connection
            .execute("DELETE FROM conversations WHERE id = ?1", [id])
            .map_err(storage)?;

        Ok(())
    }

    pub fn set_conversation_model(&self, id: i64, model_alias: &str) -> FoundryResult<ModelChange> {
        let current = self.conversation(id)?;

        if current.model_alias == model_alias {
            return Ok(ModelChange {
                conversation: current,
                divider: None,
            });
        }

        let now = now_millis();
        let divider = {
            let mut connection = self.connection();
            let transaction = connection.transaction().map_err(storage)?;
            let seq = next_seq(&transaction, id)?;

            transaction
                .execute(
                    "UPDATE conversations SET model_alias = ?1, updated_at = ?2 WHERE id = ?3",
                    params![model_alias, now, id],
                )
                .map_err(storage)?;
            transaction
                .execute(
                    "INSERT INTO messages
                       (conversation_id, seq, role, content_md, reasoning_md, model_alias,
                        status, created_at, updated_at)
                     VALUES (?1, ?2, 'divider', '', '', ?3, 'complete', ?4, ?4)",
                    params![id, seq, model_alias, now],
                )
                .map_err(storage)?;
            let divider_id = transaction.last_insert_rowid();

            transaction.commit().map_err(storage)?;

            drop(connection);

            self.message(divider_id)?
        };

        Ok(ModelChange {
            conversation: self.conversation(id)?,
            divider: Some(divider),
        })
    }

    pub fn append_message(
        &self,
        conversation_id: i64,
        role: MessageRole,
        content: &str,
        reasoning: &str,
        model_alias: Option<&str>,
        status: MessageStatus,
    ) -> FoundryResult<ChatMessage> {
        let now = now_millis();
        let mut connection = self.connection();
        let transaction = connection.transaction().map_err(storage)?;
        let seq = next_seq(&transaction, conversation_id)?;

        transaction
            .execute(
                "INSERT INTO messages
                   (conversation_id, seq, role, content_md, reasoning_md, model_alias,
                    status, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8)",
                params![
                    conversation_id,
                    seq,
                    role.as_str(),
                    content,
                    reasoning,
                    model_alias,
                    status.as_str(),
                    now
                ],
            )
            .map_err(storage)?;
        let message_id = transaction.last_insert_rowid();

        if role == MessageRole::User {
            transaction
                .execute(
                    "UPDATE conversations
                     SET title = CASE WHEN title = '' THEN ?1 ELSE title END,
                         updated_at = ?2
                     WHERE id = ?3",
                    params![heuristic_title(content), now, conversation_id],
                )
                .map_err(storage)?;
        } else {
            transaction
                .execute(
                    "UPDATE conversations SET updated_at = ?1 WHERE id = ?2",
                    params![now, conversation_id],
                )
                .map_err(storage)?;
        }

        transaction.commit().map_err(storage)?;

        drop(connection);

        self.message(message_id)
    }

    pub fn update_streaming(
        &self,
        message_id: i64,
        content: &str,
        reasoning: &str,
    ) -> FoundryResult<()> {
        let connection = self.connection();

        connection
            .execute(
                "UPDATE messages
                 SET content_md = ?1, reasoning_md = ?2, updated_at = ?3
                 WHERE id = ?4",
                params![content, reasoning, now_millis(), message_id],
            )
            .map_err(storage)?;

        Ok(())
    }

    pub fn finish_message(
        &self,
        message_id: i64,
        status: MessageStatus,
        content: &str,
        reasoning: &str,
        usage: Option<TokenUsage>,
        error: Option<&FoundryError>,
    ) -> FoundryResult<ChatMessage> {
        let error_json = error.map(|error| {
            serde_json::to_string(&MessageError::from(error)).unwrap_or_else(|_| "null".to_string())
        });
        let now = now_millis();
        let connection = self.connection();

        connection
            .execute(
                "UPDATE messages
                 SET content_md = ?1, reasoning_md = ?2, status = ?3, usage_json = ?4,
                     error_json = ?5, updated_at = ?6
                 WHERE id = ?7",
                params![
                    content,
                    reasoning,
                    status.as_str(),
                    usage.map(TokenUsage::to_json),
                    error_json,
                    now,
                    message_id
                ],
            )
            .map_err(storage)?;

        drop(connection);

        self.message(message_id)
    }

    pub fn touch_conversation(&self, conversation_id: i64) -> FoundryResult<Conversation> {
        let now = now_millis();
        let connection = self.connection();

        connection
            .execute(
                "UPDATE conversations SET updated_at = ?1 WHERE id = ?2",
                params![now, conversation_id],
            )
            .map_err(storage)?;

        drop(connection);

        self.conversation(conversation_id)
    }

    pub fn list_messages(
        &self,
        conversation_id: i64,
        before_seq: Option<i64>,
        limit: Option<u32>,
    ) -> FoundryResult<MessagePage> {
        let limit = limit.unwrap_or(DEFAULT_PAGE).clamp(1, MAX_PAGE) as i64;
        let before = before_seq.unwrap_or(i64::MAX);
        let connection = self.connection();
        let mut statement = connection
            .prepare(
                "SELECT id, conversation_id, seq, role, content_md, reasoning_md, model_alias,
                        usage_json, error_json, status, created_at, updated_at, params_json
                 FROM messages
                 WHERE conversation_id = ?1 AND seq < ?2
                 ORDER BY seq DESC
                 LIMIT ?3",
            )
            .map_err(storage)?;
        let rows = statement
            .query_map(
                params![conversation_id, before, limit + 1],
                message_from_row,
            )
            .map_err(storage)?;
        let mut messages = collect(rows)?;
        let has_more = messages.len() as i64 > limit;

        messages.truncate(limit as usize);
        messages.reverse();

        drop(statement);
        drop(connection);

        self.hydrate_attachments(&mut messages)?;

        Ok(MessagePage { messages, has_more })
    }

    pub fn conversation_usage(&self, conversation_id: i64) -> FoundryResult<TokenUsage> {
        let connection = self.connection();
        let mut statement = connection
            .prepare(
                "SELECT usage_json
                 FROM messages
                 WHERE conversation_id = ?1 AND usage_json IS NOT NULL",
            )
            .map_err(storage)?;
        let rows = statement
            .query_map([conversation_id], |row| row.get::<_, String>(0))
            .map_err(storage)?;
        let mut totals = TokenUsage::default();

        for usage in collect(rows)? {
            if let Some(usage) = TokenUsage::from_json(&usage) {
                totals.prompt_tokens += usage.prompt_tokens;
                totals.completion_tokens += usage.completion_tokens;
                totals.total_tokens += usage.total_tokens;
            }
        }

        Ok(totals)
    }

    pub fn messages_for_prompt(
        &self,
        conversation_id: i64,
        exclude_id: i64,
    ) -> FoundryResult<Vec<ChatMessage>> {
        let messages = {
            let connection = self.connection();
            let mut statement = connection
                .prepare(
                    "SELECT id, conversation_id, seq, role, content_md, reasoning_md, model_alias,
                            usage_json, error_json, status, created_at, updated_at, params_json
                     FROM messages
                     WHERE conversation_id = ?1
                       AND role IN ('user', 'assistant')
                       AND (
                         content_md <> ''
                         OR EXISTS (
                           SELECT 1 FROM attachments WHERE attachments.message_id = messages.id
                         )
                       )
                       AND id <> ?2
                     ORDER BY seq ASC",
                )
                .map_err(storage)?;
            let rows = statement
                .query_map(params![conversation_id, exclude_id], message_from_row)
                .map_err(storage)?;

            collect(rows)?
        };

        let mut messages = messages;

        self.hydrate_attachments(&mut messages)?;

        Ok(messages)
    }

    pub fn search_messages(
        &self,
        query: &str,
        conversation_id: Option<i64>,
        limit: Option<u32>,
    ) -> FoundryResult<Vec<SearchHit>> {
        let Some(match_query) = fts_query(query) else {
            return Ok(Vec::new());
        };
        let limit = limit.unwrap_or(DEFAULT_SEARCH).clamp(1, MAX_SEARCH) as i64;
        let connection = self.connection();
        let mut statement = connection
            .prepare(
                "SELECT m.id, m.conversation_id, c.title, m.role, m.model_alias, m.seq,
                        snippet(messages_fts, 0, char(2), char(3), '…', 12), m.created_at
                 FROM messages_fts
                 JOIN messages m ON m.id = messages_fts.rowid
                 JOIN conversations c ON c.id = m.conversation_id
                 WHERE messages_fts MATCH ?1
                   AND m.role IN ('user', 'assistant')
                   AND (?2 IS NULL OR m.conversation_id = ?2)
                   AND c.archived = 0
                 ORDER BY bm25(messages_fts) ASC, m.id DESC
                 LIMIT ?3",
            )
            .map_err(storage)?;
        let rows = statement
            .query_map(params![match_query, conversation_id, limit], |row| {
                let role: String = row.get(3)?;

                Ok(SearchHit {
                    message_id: row.get(0)?,
                    conversation_id: row.get(1)?,
                    conversation_title: row.get(2)?,
                    role: MessageRole::parse(&role).map_err(|_| rusqlite::Error::InvalidQuery)?,
                    model_alias: row.get(4)?,
                    seq: row.get(5)?,
                    snippet: row.get(6)?,
                    created_at: row.get(7)?,
                })
            })
            .map_err(storage)?;

        collect(rows)
    }

    pub fn all_messages(&self, conversation_id: i64) -> FoundryResult<Vec<ChatMessage>> {
        let messages = {
            let connection = self.connection();
            let mut statement = connection
                .prepare(
                    "SELECT id, conversation_id, seq, role, content_md, reasoning_md, model_alias,
                            usage_json, error_json, status, created_at, updated_at, params_json
                     FROM messages
                     WHERE conversation_id = ?1
                     ORDER BY seq ASC",
                )
                .map_err(storage)?;
            let rows = statement
                .query_map([conversation_id], message_from_row)
                .map_err(storage)?;

            collect(rows)?
        };

        let mut messages = messages;

        self.hydrate_attachments(&mut messages)?;

        Ok(messages)
    }

    pub fn latest_user_message(&self, conversation_id: i64) -> FoundryResult<ChatMessage> {
        let message = {
            let connection = self.connection();
            let message = connection
                .query_row(
                    "SELECT id, conversation_id, seq, role, content_md, reasoning_md, model_alias,
                            usage_json, error_json, status, created_at, updated_at, params_json
                     FROM messages
                     WHERE conversation_id = ?1 AND role = 'user'
                     ORDER BY seq DESC
                     LIMIT 1",
                    [conversation_id],
                    message_from_row,
                )
                .optional()
                .map_err(storage)?;

            message.ok_or_else(|| FoundryError::CommandFailed {
                command: "chat_regenerate".to_string(),
                message: "there is no prompt to regenerate".to_string(),
                hint: None,
            })?
        };
        let mut message = message;

        self.hydrate_attachments(std::slice::from_mut(&mut message))?;

        Ok(message)
    }

    pub fn truncate_from(&self, conversation_id: i64, seq: i64) -> FoundryResult<Vec<i64>> {
        self.truncate(conversation_id, seq, true)
    }

    pub fn truncate_after(&self, conversation_id: i64, seq: i64) -> FoundryResult<Vec<i64>> {
        self.truncate(conversation_id, seq, false)
    }

    fn truncate(&self, conversation_id: i64, seq: i64, inclusive: bool) -> FoundryResult<Vec<i64>> {
        let operator = if inclusive { ">=" } else { ">" };
        let mut connection = self.connection();
        let transaction = connection.transaction().map_err(storage)?;
        let ids = {
            let mut statement = transaction
                .prepare(&format!(
                    "SELECT id FROM messages
                     WHERE conversation_id = ?1 AND seq {operator} ?2
                     ORDER BY seq ASC"
                ))
                .map_err(storage)?;

            collect(
                statement
                    .query_map(params![conversation_id, seq], |row| row.get(0))
                    .map_err(storage)?,
            )?
        };

        transaction
            .execute(
                &format!("DELETE FROM messages WHERE conversation_id = ?1 AND seq {operator} ?2"),
                params![conversation_id, seq],
            )
            .map_err(storage)?;
        transaction
            .execute(
                "UPDATE conversations SET updated_at = ?1 WHERE id = ?2",
                params![now_millis(), conversation_id],
            )
            .map_err(storage)?;
        transaction.commit().map_err(storage)?;

        Ok(ids)
    }

    pub fn replace_from(
        &self,
        conversation_id: i64,
        seq: i64,
        content: &str,
        model_alias: &str,
        attachments: &[Attachment],
    ) -> FoundryResult<(Vec<i64>, ChatMessage)> {
        let mut connection = self.connection();
        let transaction = connection.transaction().map_err(storage)?;
        let removed = {
            let mut statement = transaction
                .prepare(
                    "SELECT id FROM messages
                     WHERE conversation_id = ?1 AND seq >= ?2
                     ORDER BY seq ASC",
                )
                .map_err(storage)?;

            collect(
                statement
                    .query_map(params![conversation_id, seq], |row| row.get(0))
                    .map_err(storage)?,
            )?
        };

        transaction
            .execute(
                "DELETE FROM messages WHERE conversation_id = ?1 AND seq >= ?2",
                params![conversation_id, seq],
            )
            .map_err(storage)?;
        let now = now_millis();
        transaction
            .execute(
                "INSERT INTO messages
                   (conversation_id, seq, role, content_md, reasoning_md, model_alias, status,
                    created_at, updated_at)
                 VALUES (?1, ?2, 'user', ?3, '', ?4, 'complete', ?5, ?5)",
                params![conversation_id, seq, content, model_alias, now],
            )
            .map_err(storage)?;
        let message_id = transaction.last_insert_rowid();

        for attachment in attachments {
            transaction
                .execute(
                    "INSERT INTO attachments
                       (message_id, kind, mime, path, bytes, width, height, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    params![
                        message_id,
                        attachment.kind,
                        attachment.mime,
                        attachment.path,
                        attachment.bytes as i64,
                        attachment.width,
                        attachment.height,
                        now
                    ],
                )
                .map_err(storage)?;
        }

        transaction
            .execute(
                "UPDATE conversations SET updated_at = ?1 WHERE id = ?2",
                params![now, conversation_id],
            )
            .map_err(storage)?;
        transaction.commit().map_err(storage)?;

        drop(connection);

        Ok((removed, self.message(message_id)?))
    }

    pub fn message(&self, id: i64) -> FoundryResult<ChatMessage> {
        let message = {
            let connection = self.connection();

            connection
                .query_row(
                    "SELECT id, conversation_id, seq, role, content_md, reasoning_md, model_alias,
                            usage_json, error_json, status, created_at, updated_at, params_json
                     FROM messages
                     WHERE id = ?1",
                    [id],
                    message_from_row,
                )
                .optional()
                .map_err(storage)?
        };
        let mut message = message.ok_or_else(|| storage(format!("message {id} does not exist")))?;

        self.hydrate_attachments(std::slice::from_mut(&mut message))?;

        Ok(message)
    }

    fn hydrate_attachments(&self, messages: &mut [ChatMessage]) -> FoundryResult<()> {
        if messages.is_empty() {
            return Ok(());
        }

        let ids: Vec<i64> = messages.iter().map(|message| message.id).collect();
        let placeholders = vec!["?"; ids.len()].join(", ");
        let sql = format!(
            "SELECT id, message_id, kind, mime, path, bytes, width, height
             FROM attachments
             WHERE message_id IN ({placeholders})
             ORDER BY id ASC"
        );
        let connection = self.connection();
        let mut statement = connection.prepare(&sql).map_err(storage)?;
        let rows = statement
            .query_map(params_from_iter(ids.iter()), attachment_from_row)
            .map_err(storage)?;
        let mut grouped: HashMap<i64, Vec<Attachment>> = HashMap::new();

        for attachment in collect(rows)? {
            grouped
                .entry(attachment.message_id)
                .or_default()
                .push(attachment);
        }

        for message in messages.iter_mut() {
            message.attachments = grouped.remove(&message.id).unwrap_or_default();
        }

        Ok(())
    }

    pub fn add_attachment(
        &self,
        message_id: i64,
        kind: &str,
        staged: &StagedFile,
    ) -> FoundryResult<Attachment> {
        let path = staged.path.to_string_lossy().to_string();
        let connection = self.connection();

        connection
            .execute(
                "INSERT INTO attachments
                   (message_id, kind, mime, path, bytes, width, height, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                params![
                    message_id,
                    kind,
                    staged.meta.mime,
                    path,
                    staged.meta.bytes as i64,
                    staged.meta.width,
                    staged.meta.height,
                    now_millis()
                ],
            )
            .map_err(storage)?;
        let id = connection.last_insert_rowid();

        drop(connection);

        self.attachment(id)
    }

    pub fn attachment(&self, id: i64) -> FoundryResult<Attachment> {
        let connection = self.connection();
        let attachment = connection
            .query_row(
                "SELECT id, message_id, kind, mime, path, bytes, width, height
                 FROM attachments
                 WHERE id = ?1",
                [id],
                attachment_from_row,
            )
            .optional()
            .map_err(storage)?;

        attachment.ok_or_else(|| storage(format!("attachment {id} does not exist")))
    }

    pub fn attachment_paths(&self) -> FoundryResult<HashSet<String>> {
        let connection = self.connection();
        let mut statement = connection
            .prepare("SELECT path FROM attachments")
            .map_err(storage)?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(storage)?;

        Ok(collect(rows)?.into_iter().collect())
    }

    pub fn conversation_attachment_paths(
        &self,
        conversation_id: i64,
    ) -> FoundryResult<Vec<String>> {
        let connection = self.connection();
        let mut statement = connection
            .prepare(
                "SELECT a.path
                 FROM attachments a
                 JOIN messages m ON m.id = a.message_id
                 WHERE m.conversation_id = ?1",
            )
            .map_err(storage)?;
        let rows = statement
            .query_map([conversation_id], |row| row.get::<_, String>(0))
            .map_err(storage)?;

        collect(rows)
    }

    pub fn conversation_params(&self, conversation_id: i64) -> FoundryResult<ChatParams> {
        let connection = self.connection();
        let params: Option<String> = connection
            .query_row(
                "SELECT params_json FROM conversations WHERE id = ?1",
                [conversation_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage)?
            .flatten();

        Ok(parse_params(params.as_deref()))
    }

    pub fn set_conversation_params(
        &self,
        conversation_id: i64,
        params: &ChatParams,
    ) -> FoundryResult<Conversation> {
        let connection = self.connection();

        connection
            .execute(
                "UPDATE conversations SET params_json = ?1 WHERE id = ?2",
                params![params_json(params), conversation_id],
            )
            .map_err(storage)?;

        drop(connection);

        self.conversation(conversation_id)
    }

    pub fn model_params(&self, model_alias: &str) -> FoundryResult<ChatParams> {
        let connection = self.connection();
        let params: Option<String> = connection
            .query_row(
                "SELECT params_json FROM model_params WHERE model_alias = ?1",
                [model_alias],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage)?;

        Ok(parse_params(params.as_deref()))
    }

    pub fn set_model_params(&self, model_alias: &str, params: &ChatParams) -> FoundryResult<()> {
        let connection = self.connection();

        if params.is_empty() {
            connection
                .execute(
                    "DELETE FROM model_params WHERE model_alias = ?1",
                    [model_alias],
                )
                .map_err(storage)?;
        } else {
            connection
                .execute(
                    "INSERT INTO model_params (model_alias, params_json, updated_at)
                     VALUES (?1, ?2, ?3)
                     ON CONFLICT(model_alias) DO UPDATE
                       SET params_json = excluded.params_json, updated_at = excluded.updated_at",
                    params![model_alias, params_json(params), now_millis()],
                )
                .map_err(storage)?;
        }

        Ok(())
    }

    pub fn generation_defaults(&self) -> FoundryResult<ChatParams> {
        let connection = self.connection();
        let value: Option<String> = connection
            .query_row(
                "SELECT value_json FROM settings WHERE key = 'generation_defaults'",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage)?;

        Ok(parse_params(value.as_deref()))
    }

    pub fn set_generation_defaults(&self, params: &ChatParams) -> FoundryResult<()> {
        let connection = self.connection();

        connection
            .execute(
                "INSERT INTO settings (key, value_json, updated_at)
                 VALUES ('generation_defaults', ?1, ?2)
                 ON CONFLICT(key) DO UPDATE
                   SET value_json = excluded.value_json, updated_at = excluded.updated_at",
                params![params_json(params), now_millis()],
            )
            .map_err(storage)?;

        Ok(())
    }

    pub fn speech_storage_directory(&self) -> FoundryResult<Option<String>> {
        let connection = self.connection();
        let value: Option<String> = connection
            .query_row(
                "SELECT value_json FROM settings WHERE key = 'speech_audio_storage_directory'",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage)?;

        Ok(value
            .and_then(|value| serde_json::from_str::<String>(&value).ok())
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty()))
    }

    pub fn set_speech_storage_directory(&self, directory: Option<&str>) -> FoundryResult<()> {
        let connection = self.connection();

        match directory.map(str::trim).filter(|value| !value.is_empty()) {
            Some(path) => {
                let value = serde_json::to_string(path).map_err(storage)?;

                connection
                    .execute(
                        "INSERT INTO settings (key, value_json, updated_at)
                         VALUES ('speech_audio_storage_directory', ?1, ?2)
                         ON CONFLICT(key) DO UPDATE
                           SET value_json = excluded.value_json, updated_at = excluded.updated_at",
                        params![value, now_millis()],
                    )
                    .map_err(storage)?;
            }
            None => {
                connection
                    .execute(
                        "DELETE FROM settings WHERE key = 'speech_audio_storage_directory'",
                        [],
                    )
                    .map_err(storage)?;
            }
        }

        Ok(())
    }

    pub fn set_message_params(
        &self,
        message_id: i64,
        params: &ResolvedParams,
    ) -> FoundryResult<()> {
        let value = serde_json::to_string(params).map_err(storage)?;
        let connection = self.connection();

        connection
            .execute(
                "UPDATE messages SET params_json = ?1 WHERE id = ?2",
                params![value, message_id],
            )
            .map_err(storage)?;

        Ok(())
    }

    pub fn insert_transcription(
        &self,
        model_alias: &str,
        source_path: &str,
        language: Option<&str>,
        params: Option<&TranscriptionParams>,
        text: &str,
        duration_ms: i64,
    ) -> FoundryResult<Transcription> {
        let now = now_millis();
        let connection = self.connection();

        connection
            .execute(
                "INSERT INTO transcriptions
                   (model_alias, source_path, language, params_json, text, duration_ms, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    model_alias,
                    source_path,
                    language,
                    params.and_then(transcription_params_json),
                    text,
                    duration_ms.max(0),
                    now
                ],
            )
            .map_err(storage)?;
        let id = connection.last_insert_rowid();

        drop(connection);

        self.transcription(id)
    }

    pub fn list_transcriptions(&self, archived: bool) -> FoundryResult<Vec<Transcription>> {
        let connection = self.connection();
        let mut statement = connection
            .prepare(
                "SELECT id, model_alias, source_path, language, params_json, text, duration_ms, created_at,
                        favorite, archived
                 FROM transcriptions
                 WHERE archived = ?1
                 ORDER BY favorite DESC, created_at DESC, id DESC",
            )
            .map_err(storage)?;
        let rows = statement
            .query_map([archived], transcription_from_row)
            .map_err(storage)?;

        collect(rows)
    }

    pub fn transcription(&self, id: i64) -> FoundryResult<Transcription> {
        let connection = self.connection();
        let transcription = connection
            .query_row(
                "SELECT id, model_alias, source_path, language, params_json, text, duration_ms, created_at,
                        favorite, archived
                 FROM transcriptions
                 WHERE id = ?1",
                [id],
                transcription_from_row,
            )
            .optional()
            .map_err(storage)?;

        transcription.ok_or_else(|| storage(format!("transcription {id} does not exist")))
    }

    pub fn update_transcription_text(&self, id: i64, text: &str) -> FoundryResult<Transcription> {
        let connection = self.connection();

        connection
            .execute(
                "UPDATE transcriptions SET text = ?1 WHERE id = ?2",
                params![text, id],
            )
            .map_err(storage)?;

        drop(connection);

        self.transcription(id)
    }

    pub fn delete_transcription(&self, id: i64) -> FoundryResult<Transcription> {
        let transcription = self.transcription(id)?;
        let connection = self.connection();

        connection
            .execute("DELETE FROM transcriptions WHERE id = ?1", [id])
            .map_err(storage)?;

        Ok(transcription)
    }

    pub fn set_transcription_favorite(
        &self,
        id: i64,
        favorite: bool,
    ) -> FoundryResult<Transcription> {
        let connection = self.connection();

        connection
            .execute(
                "UPDATE transcriptions SET favorite = ?1 WHERE id = ?2",
                params![favorite, id],
            )
            .map_err(storage)?;

        drop(connection);

        self.transcription(id)
    }

    pub fn set_transcription_archived(
        &self,
        id: i64,
        archived: bool,
    ) -> FoundryResult<Transcription> {
        let connection = self.connection();

        connection
            .execute(
                "UPDATE transcriptions SET archived = ?1 WHERE id = ?2",
                params![archived, id],
            )
            .map_err(storage)?;

        drop(connection);

        self.transcription(id)
    }

    pub fn transcription_paths(&self) -> FoundryResult<HashSet<String>> {
        let connection = self.connection();
        let mut statement = connection
            .prepare("SELECT source_path FROM transcriptions")
            .map_err(storage)?;
        let rows = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(storage)?;

        Ok(collect(rows)?.into_iter().collect())
    }

    pub fn list_embedding_corpora(&self, archived: bool) -> FoundryResult<Vec<EmbeddingCorpus>> {
        let connection = self.connection();
        let mut statement = connection
            .prepare(
                "SELECT c.id, c.name, c.model_alias, c.dimensions, c.created_at, c.updated_at,
                        COALESCE(d.docs, 0), COALESCE(v.vectors, 0), c.favorite, c.archived
                 FROM embedding_corpora c
                 LEFT JOIN (
                   SELECT corpus_id, COUNT(*) AS docs
                   FROM corpus_documents
                   GROUP BY corpus_id
                 ) d ON d.corpus_id = c.id
                 LEFT JOIN (
                   SELECT corpus_id, COUNT(*) AS vectors
                   FROM corpus_vectors
                   GROUP BY corpus_id
                 ) v ON v.corpus_id = c.id
                 WHERE c.archived = ?1
                 ORDER BY c.favorite DESC, c.updated_at DESC, c.id DESC",
            )
            .map_err(storage)?;
        let rows = statement
            .query_map([archived], embedding_corpus_from_row)
            .map_err(storage)?;

        collect(rows)
    }

    pub fn embedding_corpus(&self, id: i64) -> FoundryResult<EmbeddingCorpus> {
        let connection = self.connection();
        let corpus = connection
            .query_row(
                "SELECT c.id, c.name, c.model_alias, c.dimensions, c.created_at, c.updated_at,
                        COALESCE(d.docs, 0), COALESCE(v.vectors, 0), c.favorite, c.archived
                 FROM embedding_corpora c
                 LEFT JOIN (
                   SELECT corpus_id, COUNT(*) AS docs
                   FROM corpus_documents
                   GROUP BY corpus_id
                 ) d ON d.corpus_id = c.id
                 LEFT JOIN (
                   SELECT corpus_id, COUNT(*) AS vectors
                   FROM corpus_vectors
                   GROUP BY corpus_id
                 ) v ON v.corpus_id = c.id
                 WHERE c.id = ?1",
                [id],
                embedding_corpus_from_row,
            )
            .optional()
            .map_err(storage)?;

        corpus.ok_or_else(|| storage(format!("embedding corpus {id} does not exist")))
    }

    pub fn create_embedding_corpus(
        &self,
        name: &str,
        model_alias: &str,
        dimensions: u32,
        documents: &[String],
    ) -> FoundryResult<EmbeddingCorpus> {
        let inputs: Vec<CorpusDocumentInput> = documents
            .iter()
            .map(|text| CorpusDocumentInput {
                text: text.clone(),
                source_path: None,
            })
            .collect();

        self.create_embedding_corpus_with_sources(name, model_alias, dimensions, &inputs)
    }

    pub fn create_embedding_corpus_with_sources(
        &self,
        name: &str,
        model_alias: &str,
        dimensions: u32,
        documents: &[CorpusDocumentInput],
    ) -> FoundryResult<EmbeddingCorpus> {
        let now = now_millis();
        let clean_name = name.trim();
        let clean_model = model_alias.trim();

        if clean_name.is_empty() {
            return Err(FoundryError::CommandFailed {
                command: "embeddings_create_corpus".to_string(),
                message: "the corpus name is empty".to_string(),
                hint: None,
            });
        }

        if clean_model.is_empty() {
            return Err(FoundryError::CommandFailed {
                command: "embeddings_create_corpus".to_string(),
                message: "the embedding model is empty".to_string(),
                hint: None,
            });
        }

        let docs = normalized_document_inputs(documents);
        let mut connection = self.connection();
        let transaction = connection.transaction().map_err(storage)?;

        transaction
            .execute(
                "INSERT INTO embedding_corpora
                   (name, model_alias, dimensions, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?4)",
                params![clean_name, clean_model, dimensions as i64, now],
            )
            .map_err(storage)?;
        let corpus_id = transaction.last_insert_rowid();

        for (index, document) in docs.iter().enumerate() {
            transaction
                .execute(
                    "INSERT INTO corpus_documents (corpus_id, position, text, source_path, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![corpus_id, index as i64, document.text, document.source_path, now],
                )
                .map_err(storage)?;
        }

        transaction.commit().map_err(storage)?;
        drop(connection);

        self.embedding_corpus(corpus_id)
    }

    pub fn update_embedding_corpus_name(
        &self,
        id: i64,
        name: &str,
    ) -> FoundryResult<EmbeddingCorpus> {
        let clean_name = name.trim();

        if clean_name.is_empty() {
            return Err(FoundryError::CommandFailed {
                command: "embeddings_rename_corpus".to_string(),
                message: "the corpus name is empty".to_string(),
                hint: None,
            });
        }

        let connection = self.connection();

        connection
            .execute(
                "UPDATE embedding_corpora SET name = ?1, updated_at = ?2 WHERE id = ?3",
                params![clean_name, now_millis(), id],
            )
            .map_err(storage)?;

        drop(connection);

        self.embedding_corpus(id)
    }

    pub fn replace_embedding_corpus_documents(
        &self,
        id: i64,
        documents: &[String],
    ) -> FoundryResult<EmbeddingCorpus> {
        let inputs: Vec<CorpusDocumentInput> = documents
            .iter()
            .map(|text| CorpusDocumentInput {
                text: text.clone(),
                source_path: None,
            })
            .collect();

        self.replace_embedding_corpus_documents_with_sources(id, &inputs)
    }

    pub fn replace_embedding_corpus_documents_with_sources(
        &self,
        id: i64,
        documents: &[CorpusDocumentInput],
    ) -> FoundryResult<EmbeddingCorpus> {
        let docs = normalized_document_inputs(documents);
        let now = now_millis();
        let mut connection = self.connection();
        let transaction = connection.transaction().map_err(storage)?;

        let exists: bool = transaction
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM embedding_corpora WHERE id = ?1)",
                [id],
                |row| row.get(0),
            )
            .map_err(storage)?;

        if !exists {
            return Err(storage(format!("embedding corpus {id} does not exist")));
        }

        transaction
            .execute("DELETE FROM corpus_vectors WHERE corpus_id = ?1", [id])
            .map_err(storage)?;
        transaction
            .execute("DELETE FROM corpus_documents WHERE corpus_id = ?1", [id])
            .map_err(storage)?;

        for (index, document) in docs.iter().enumerate() {
            transaction
                .execute(
                    "INSERT INTO corpus_documents (corpus_id, position, text, source_path, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    params![id, index as i64, document.text, document.source_path, now],
                )
                .map_err(storage)?;
        }

        transaction
            .execute(
                "UPDATE embedding_corpora SET updated_at = ?1 WHERE id = ?2",
                params![now, id],
            )
            .map_err(storage)?;

        transaction.commit().map_err(storage)?;
        drop(connection);

        self.embedding_corpus(id)
    }

    pub fn set_embedding_corpus_model(
        &self,
        id: i64,
        model_alias: &str,
        dimensions: u32,
    ) -> FoundryResult<EmbeddingCorpus> {
        let alias = model_alias.trim();

        if alias.is_empty() {
            return Err(FoundryError::CommandFailed {
                command: "embeddings_set_model".to_string(),
                message: "the embedding model is empty".to_string(),
                hint: None,
            });
        }

        let current = self.embedding_corpus(id)?;

        if current.model_alias == alias && current.dimensions == dimensions {
            return Ok(current);
        }

        let now = now_millis();
        let mut connection = self.connection();
        let transaction = connection.transaction().map_err(storage)?;

        transaction
            .execute(
                "UPDATE embedding_corpora
                 SET model_alias = ?1, dimensions = ?2, updated_at = ?3
                 WHERE id = ?4",
                params![alias, dimensions as i64, now, id],
            )
            .map_err(storage)?;
        transaction
            .execute("DELETE FROM corpus_vectors WHERE corpus_id = ?1", [id])
            .map_err(storage)?;
        transaction.commit().map_err(storage)?;
        drop(connection);

        self.embedding_corpus(id)
    }

    pub fn update_embedding_dimensions(&self, id: i64, dimensions: u32) -> FoundryResult<()> {
        let connection = self.connection();

        connection
            .execute(
                "UPDATE embedding_corpora SET dimensions = ?1, updated_at = ?2 WHERE id = ?3",
                params![dimensions as i64, now_millis(), id],
            )
            .map_err(storage)?;

        Ok(())
    }

    pub fn delete_embedding_corpus(&self, id: i64) -> FoundryResult<()> {
        let connection = self.connection();

        connection
            .execute("DELETE FROM embedding_corpora WHERE id = ?1", [id])
            .map_err(storage)?;

        Ok(())
    }

    pub fn set_embedding_corpus_favorite(
        &self,
        id: i64,
        favorite: bool,
    ) -> FoundryResult<EmbeddingCorpus> {
        let connection = self.connection();

        connection
            .execute(
                "UPDATE embedding_corpora SET favorite = ?1 WHERE id = ?2",
                params![favorite, id],
            )
            .map_err(storage)?;

        drop(connection);

        self.embedding_corpus(id)
    }

    pub fn set_embedding_corpus_archived(
        &self,
        id: i64,
        archived: bool,
    ) -> FoundryResult<EmbeddingCorpus> {
        let connection = self.connection();

        connection
            .execute(
                "UPDATE embedding_corpora SET archived = ?1 WHERE id = ?2",
                params![archived, id],
            )
            .map_err(storage)?;

        drop(connection);

        self.embedding_corpus(id)
    }

    pub fn embedding_documents(&self, corpus_id: i64) -> FoundryResult<Vec<CorpusDocument>> {
        let connection = self.connection();
        let mut statement = connection
            .prepare(
                "SELECT id, corpus_id, position, text, source_path, created_at
                 FROM corpus_documents
                 WHERE corpus_id = ?1
                 ORDER BY position ASC, id ASC",
            )
            .map_err(storage)?;
        let rows = statement
            .query_map([corpus_id], corpus_document_from_row)
            .map_err(storage)?;

        collect(rows)
    }

    pub fn upsert_corpus_vector(
        &self,
        corpus_id: i64,
        document_id: i64,
        dimensions: u32,
        vector: &[u8],
    ) -> FoundryResult<()> {
        let connection = self.connection();

        connection
            .execute(
                "INSERT INTO corpus_vectors (corpus_id, document_id, dimensions, vector, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)
                 ON CONFLICT(document_id) DO UPDATE
                   SET corpus_id = excluded.corpus_id,
                       dimensions = excluded.dimensions,
                       vector = excluded.vector,
                       created_at = excluded.created_at",
                params![corpus_id, document_id, dimensions as i64, vector, now_millis()],
            )
            .map_err(storage)?;

        Ok(())
    }

    pub fn corpus_vector_records(&self, corpus_id: i64) -> FoundryResult<Vec<CorpusVectorRecord>> {
        let connection = self.connection();
        let mut statement = connection
            .prepare(
                "SELECT d.id, d.corpus_id, d.position, d.text, d.source_path, d.created_at,
                        v.dimensions, v.vector
                 FROM corpus_documents d
                 JOIN corpus_vectors v ON v.document_id = d.id
                 WHERE d.corpus_id = ?1 AND v.corpus_id = ?1
                 ORDER BY d.position ASC, d.id ASC",
            )
            .map_err(storage)?;
        let rows = statement
            .query_map([corpus_id], |row| {
                Ok(CorpusVectorRecord {
                    document: CorpusDocument {
                        id: row.get(0)?,
                        corpus_id: row.get(1)?,
                        position: row.get::<_, i64>(2)?.max(0) as u32,
                        text: row.get(3)?,
                        source_path: row.get(4)?,
                        created_at: row.get(5)?,
                    },
                    dimensions: row.get::<_, i64>(6)?.max(0) as u32,
                    vector: row.get(7)?,
                })
            })
            .map_err(storage)?;

        collect(rows)
    }

    fn connection(&self) -> MutexGuard<'_, Connection> {
        self.connection
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

fn params_json(params: &ChatParams) -> String {
    serde_json::to_string(params).unwrap_or_else(|_| "{}".to_string())
}

fn transcription_params_json(params: &TranscriptionParams) -> Option<String> {
    serde_json::to_string(params).ok()
}

fn parse_params(value: Option<&str>) -> ChatParams {
    value
        .and_then(|value| serde_json::from_str(value).ok())
        .unwrap_or_default()
}

fn fts_query(query: &str) -> Option<String> {
    let tokens: Vec<String> = query
        .split(|character: char| !character.is_alphanumeric())
        .filter(|token| !token.is_empty())
        .take(MAX_SEARCH_TERMS)
        .map(|token| format!("\"{}\"*", token.to_lowercase()))
        .collect();

    if tokens.is_empty() {
        None
    } else {
        Some(tokens.join(" "))
    }
}

fn heuristic_title(content: &str) -> String {
    let collapsed = content.split_whitespace().collect::<Vec<_>>().join(" ");

    if collapsed.chars().count() <= TITLE_LIMIT {
        return collapsed;
    }

    let mut title: String = collapsed.chars().take(TITLE_LIMIT).collect();
    title.push('…');
    title
}

fn next_seq(connection: &Connection, conversation_id: i64) -> FoundryResult<i64> {
    connection
        .query_row(
            "SELECT COALESCE(MAX(seq), 0) + 1 FROM messages WHERE conversation_id = ?1",
            [conversation_id],
            |row| row.get(0),
        )
        .map_err(storage)
}

fn conversation_from_row(row: &Row<'_>) -> rusqlite::Result<Conversation> {
    Ok(Conversation {
        id: row.get(0)?,
        title: row.get(1)?,
        model_alias: row.get(2)?,
        created_at: row.get(3)?,
        updated_at: row.get(4)?,
        favorite: row.get(5)?,
        archived: row.get(6)?,
    })
}

fn message_from_row(row: &Row<'_>) -> rusqlite::Result<ChatMessage> {
    let role: String = row.get(3)?;
    let status: String = row.get(9)?;
    let usage: Option<String> = row.get(7)?;
    let error: Option<String> = row.get(8)?;
    let params: Option<String> = row.get(12)?;

    Ok(ChatMessage {
        id: row.get(0)?,
        conversation_id: row.get(1)?,
        seq: row.get(2)?,
        role: MessageRole::parse(&role).map_err(|_| rusqlite::Error::InvalidQuery)?,
        content: row.get(4)?,
        reasoning: row.get(5)?,
        model_alias: row.get(6)?,
        params: params
            .as_deref()
            .and_then(|value| serde_json::from_str(value).ok()),
        usage: usage.as_deref().and_then(TokenUsage::from_json),
        error: error
            .as_deref()
            .and_then(|value| serde_json::from_str(value).ok()),
        status: MessageStatus::parse(&status).map_err(|_| rusqlite::Error::InvalidQuery)?,
        attachments: Vec::new(),
        created_at: row.get(10)?,
        updated_at: row.get(11)?,
    })
}

fn transcription_from_row(row: &Row<'_>) -> rusqlite::Result<Transcription> {
    let params: Option<String> = row.get(4)?;

    Ok(Transcription {
        id: row.get(0)?,
        model_alias: row.get(1)?,
        source_path: row.get(2)?,
        language: row.get(3)?,
        params: params
            .as_deref()
            .and_then(|value| serde_json::from_str(value).ok()),
        text: row.get(5)?,
        duration_ms: row.get::<_, i64>(6)?.max(0),
        audio_exists: true,
        created_at: row.get(7)?,
        favorite: row.get(8)?,
        archived: row.get(9)?,
    })
}

fn embedding_corpus_from_row(row: &Row<'_>) -> rusqlite::Result<EmbeddingCorpus> {
    Ok(EmbeddingCorpus {
        id: row.get(0)?,
        name: row.get(1)?,
        model_alias: row.get(2)?,
        dimensions: row.get::<_, i64>(3)?.max(0) as u32,
        created_at: row.get(4)?,
        updated_at: row.get(5)?,
        document_count: row.get::<_, i64>(6)?.max(0) as u32,
        vector_count: row.get::<_, i64>(7)?.max(0) as u32,
        favorite: row.get(8)?,
        archived: row.get(9)?,
    })
}

fn corpus_document_from_row(row: &Row<'_>) -> rusqlite::Result<CorpusDocument> {
    Ok(CorpusDocument {
        id: row.get(0)?,
        corpus_id: row.get(1)?,
        position: row.get::<_, i64>(2)?.max(0) as u32,
        text: row.get(3)?,
        source_path: row.get(4)?,
        created_at: row.get(5)?,
    })
}

fn normalized_document_inputs(documents: &[CorpusDocumentInput]) -> Vec<CorpusDocumentInput> {
    documents
        .iter()
        .map(|document| CorpusDocumentInput {
            text: document.text.trim().to_string(),
            source_path: document
                .source_path
                .as_deref()
                .map(str::trim)
                .filter(|value| !value.is_empty())
                .map(str::to_string),
        })
        .filter(|document| !document.text.is_empty())
        .collect()
}

fn attachment_from_row(row: &Row<'_>) -> rusqlite::Result<Attachment> {
    Ok(Attachment {
        id: row.get(0)?,
        message_id: row.get(1)?,
        kind: row.get(2)?,
        mime: row.get(3)?,
        path: row.get(4)?,
        bytes: row.get::<_, Option<i64>>(5)?.unwrap_or_default().max(0) as u64,
        width: row
            .get::<_, Option<i64>>(6)?
            .map(|value| value.max(0) as u32),
        height: row
            .get::<_, Option<i64>>(7)?
            .map(|value| value.max(0) as u32),
    })
}

fn apply_schema(connection: &Connection) -> FoundryResult<()> {
    connection.execute_batch(SCHEMA).map_err(storage)?;

    Ok(())
}

fn schema_stamp() -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;

    for byte in SCHEMA.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }

    format!("{hash:016x}")
}

fn is_stale_schema(path: &Path) -> bool {
    if !path.exists() {
        return false;
    }

    let stamp_matches = std::fs::read_to_string(sidecar_path(path, SCHEMA_STAMP_SUFFIX))
        .map(|stamp| stamp.trim() == schema_stamp())
        .unwrap_or(false);

    if !stamp_matches {
        return true;
    }

    Connection::open(path)
        .and_then(|connection| {
            connection.pragma_query_value(None, "page_count", |row| row.get::<_, i64>(0))
        })
        .is_err()
}

fn write_schema_stamp(path: &Path) -> FoundryResult<()> {
    std::fs::write(sidecar_path(path, SCHEMA_STAMP_SUFFIX), schema_stamp()).map_err(storage)
}

fn discard_database_files(path: &Path) -> FoundryResult<()> {
    for candidate in [
        path.to_path_buf(),
        sidecar_path(path, "-wal"),
        sidecar_path(path, "-shm"),
        sidecar_path(path, "-journal"),
        sidecar_path(path, SCHEMA_STAMP_SUFFIX),
    ] {
        match std::fs::remove_file(&candidate) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(storage(error)),
        }
    }

    Ok(())
}

fn sidecar_path(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);

    PathBuf::from(name)
}

fn recover_interrupted(connection: &Connection) -> FoundryResult<()> {
    connection
        .execute(
            "UPDATE messages
             SET status = 'interrupted', updated_at = ?1
             WHERE status = 'streaming'",
            [now_millis()],
        )
        .map_err(storage)?;

    Ok(())
}

fn collect<T>(
    rows: rusqlite::MappedRows<'_, impl FnMut(&Row<'_>) -> rusqlite::Result<T>>,
) -> FoundryResult<Vec<T>> {
    rows.collect::<rusqlite::Result<Vec<T>>>().map_err(storage)
}

fn storage(error: impl std::fmt::Display) -> FoundryError {
    FoundryError::Storage {
        detail: error.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use super::*;

    fn database() -> Database {
        Database::open_in_memory().expect("an in-memory database opens")
    }

    fn count(connection: &Connection, sql: &str) -> i64 {
        connection
            .query_row(sql, [], |row| row.get(0))
            .expect("count")
    }

    #[test]
    fn transcriptions_keep_model_language_duration_and_text() {
        let database = database();
        let transcription = database
            .insert_transcription(
                "whisper-tiny",
                "/tmp/audio.wav",
                Some("pt"),
                Some(&TranscriptionParams {
                    requested_language: Some("pt".to_string()),
                    temperature: 0.2,
                }),
                "olá mundo",
                30_500,
            )
            .expect("transcription");

        assert_eq!(transcription.model_alias, "whisper-tiny");
        assert_eq!(transcription.language.as_deref(), Some("pt"));
        assert_eq!(
            transcription
                .params
                .as_ref()
                .and_then(|value| value.requested_language.as_deref()),
            Some("pt")
        );
        assert_eq!(
            transcription.params.as_ref().map(|value| value.temperature),
            Some(0.2)
        );
        assert_eq!(transcription.text, "olá mundo");
        assert_eq!(transcription.duration_ms, 30_500);

        let listed = database.list_transcriptions(false).expect("list");

        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].id, transcription.id);
        assert!(!listed[0].favorite);
        assert!(!listed[0].archived);

        let updated = database
            .update_transcription_text(transcription.id, "texto revisado")
            .expect("update");

        assert_eq!(updated.text, "texto revisado");

        let removed = database
            .delete_transcription(transcription.id)
            .expect("delete");

        assert_eq!(removed.source_path, "/tmp/audio.wav");
        assert!(
            database
                .list_transcriptions(false)
                .expect("list")
                .is_empty()
        );
    }

    #[test]
    fn favorite_transcriptions_come_first_and_archived_are_filtered() {
        let database = database();
        let first = database
            .insert_transcription("whisper-tiny", "/tmp/a.wav", None, None, "one", 1_000)
            .expect("first");
        std::thread::sleep(std::time::Duration::from_millis(3));
        let second = database
            .insert_transcription("whisper-base", "/tmp/b.wav", None, None, "two", 2_000)
            .expect("second");

        let favorite = database
            .set_transcription_favorite(first.id, true)
            .expect("favorite");

        assert!(favorite.favorite);

        let listed = database.list_transcriptions(false).expect("list");

        assert_eq!(listed[0].id, first.id);
        assert_eq!(listed[1].id, second.id);

        let archived = database
            .set_transcription_archived(second.id, true)
            .expect("archive");

        assert!(archived.archived);

        let active = database.list_transcriptions(false).expect("active");
        let archived = database.list_transcriptions(true).expect("archived");

        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, first.id);
        assert_eq!(archived.len(), 1);
        assert_eq!(archived[0].id, second.id);

        let restored = database
            .set_transcription_archived(second.id, false)
            .expect("restore");

        assert!(!restored.archived);
        assert_eq!(
            database.list_transcriptions(true).expect("archived").len(),
            0
        );
    }

    #[test]
    fn transcriptions_are_ordered_newest_first() {
        let database = database();
        let first = database
            .insert_transcription("whisper-tiny", "/tmp/a.wav", None, None, "one", 1_000)
            .expect("first");
        std::thread::sleep(std::time::Duration::from_millis(3));
        let second = database
            .insert_transcription("whisper-base", "/tmp/b.wav", None, None, "two", 2_000)
            .expect("second");

        let listed = database.list_transcriptions(false).expect("list");

        assert_eq!(listed[0].id, second.id);
        assert_eq!(listed[1].id, first.id);
    }

    #[test]
    fn inserting_a_transcription_clamps_negative_duration() {
        let database = database();
        let transcription = database
            .insert_transcription("whisper-tiny", "/tmp/a.wav", None, None, "", -30)
            .expect("transcription");

        assert_eq!(transcription.duration_ms, 0);
    }

    #[test]
    fn embedding_corpora_keep_documents_and_vector_counts() {
        let database = database();
        let corpus = database
            .create_embedding_corpus(
                "Support FAQs",
                "qwen3-embedding-0.6b",
                1024,
                &[
                    "How to reset my password".to_string(),
                    "How to cancel my subscription".to_string(),
                ],
            )
            .expect("corpus");

        assert_eq!(corpus.document_count, 2);
        assert_eq!(corpus.vector_count, 0);

        let documents = database.embedding_documents(corpus.id).expect("documents");
        let first = &documents[0];

        database
            .upsert_corpus_vector(corpus.id, first.id, 1024, &[0, 0, 128, 63])
            .expect("vector");

        let reloaded = database.embedding_corpus(corpus.id).expect("corpus");
        assert_eq!(reloaded.document_count, 2);
        assert_eq!(reloaded.vector_count, 1);

        let records = database
            .corpus_vector_records(corpus.id)
            .expect("vector records");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].document.id, first.id);
        assert_eq!(records[0].dimensions, 1024);
    }

    #[test]
    fn changing_the_embedding_model_clears_existing_vectors() {
        let database = database();
        let corpus = database
            .create_embedding_corpus(
                "Billing",
                "qwen3-embedding-0.6b",
                1024,
                &["Cancel subscription".to_string()],
            )
            .expect("corpus");
        let document = database.embedding_documents(corpus.id).expect("documents")[0].clone();

        database
            .upsert_corpus_vector(corpus.id, document.id, 1024, &[0, 0, 128, 63])
            .expect("vector");

        let changed = database
            .set_embedding_corpus_model(corpus.id, "qwen3-embedding-8b", 4096)
            .expect("model change");

        assert_eq!(changed.model_alias, "qwen3-embedding-8b");
        assert_eq!(changed.dimensions, 4096);
        assert_eq!(changed.vector_count, 0);
    }

    #[test]
    fn favorite_corpora_come_first_and_archived_are_filtered() {
        let database = database();
        let first = database
            .create_embedding_corpus("Support", "qwen3-embedding-0.6b", 1024, &[])
            .expect("first");
        std::thread::sleep(std::time::Duration::from_millis(3));
        let second = database
            .create_embedding_corpus("Billing", "qwen3-embedding-0.6b", 1024, &[])
            .expect("second");

        let favorite = database
            .set_embedding_corpus_favorite(first.id, true)
            .expect("favorite");

        assert!(favorite.favorite);

        let listed = database.list_embedding_corpora(false).expect("list");

        assert_eq!(listed[0].id, first.id);
        assert_eq!(listed[1].id, second.id);

        database
            .set_embedding_corpus_archived(second.id, true)
            .expect("archive");

        let active = database.list_embedding_corpora(false).expect("active");
        let archived = database.list_embedding_corpora(true).expect("archived");

        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, first.id);
        assert_eq!(archived.len(), 1);
        assert_eq!(archived[0].id, second.id);
        assert!(archived[0].archived);
    }

    #[test]
    fn clearing_parameter_layers_leaves_nothing_behind() {
        let database = database();
        let model = ChatParams {
            temperature: Some(0.2),
            ..ChatParams::default()
        };

        database
            .set_generation_defaults(&model)
            .expect("global set");
        database
            .set_model_params("qwen3-0.6b", &model)
            .expect("model set");

        assert_eq!(
            database.generation_defaults().expect("global").temperature,
            Some(0.2)
        );
        assert_eq!(
            database
                .model_params("qwen3-0.6b")
                .expect("model")
                .temperature,
            Some(0.2)
        );

        database
            .set_generation_defaults(&ChatParams::default())
            .expect("global cleared");
        database
            .set_model_params("qwen3-0.6b", &ChatParams::default())
            .expect("model cleared");

        assert!(database.generation_defaults().expect("global").is_empty());
        assert!(
            database
                .model_params("qwen3-0.6b")
                .expect("model")
                .is_empty()
        );
    }

    #[test]
    fn schema_applies_from_scratch_and_is_idempotent() {
        let database = database();
        let connection = database.connection();

        assert_eq!(
            connection
                .pragma_query_value(None, "user_version", |row| row.get::<_, i64>(0))
                .expect("user version"),
            0
        );
        assert_eq!(
            count(
                &connection,
                "SELECT COUNT(*) FROM sqlite_master WHERE name IN
                   ('conversations', 'messages', 'attachments', 'model_params', 'settings', 'messages_fts', 'transcriptions',
                    'embedding_corpora', 'corpus_documents', 'corpus_vectors')"
            ),
            10
        );
        assert_eq!(
            count(
                &connection,
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'trigger' AND name LIKE 'messages_fts_%'"
            ),
            3
        );
        assert_eq!(
            count(
                &connection,
                "SELECT COUNT(*) FROM sqlite_master WHERE name = 'schema_migrations'"
            ),
            0
        );

        apply_schema(&connection).expect("a second run is a no-op");

        assert_eq!(
            count(
                &connection,
                "SELECT COUNT(*) FROM sqlite_master WHERE type = 'trigger' AND name LIKE 'messages_fts_%'"
            ),
            3
        );
    }

    #[test]
    fn an_outdated_database_file_is_discarded_and_rebuilt() {
        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().join("foundryui.sqlite3");

        let database = Database::open(&path).expect("open");
        database
            .create_conversation("qwen3-0.6b")
            .expect("conversation");
        drop(database);

        std::fs::write(sidecar_path(&path, SCHEMA_STAMP_SUFFIX), "stale").expect("stale stamp");

        let rebuilt = Database::open(&path).expect("reopen");
        let conversations = rebuilt.list_conversations(false).expect("conversations");

        assert!(conversations.is_empty());
        assert_eq!(
            std::fs::read_to_string(sidecar_path(&path, SCHEMA_STAMP_SUFFIX)).expect("stamp"),
            schema_stamp()
        );
    }

    #[test]
    fn a_database_file_matching_the_schema_stamp_is_kept() {
        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().join("foundryui.sqlite3");

        let database = Database::open(&path).expect("open");
        database
            .create_conversation("qwen3-0.6b")
            .expect("conversation");
        drop(database);

        let reopened = Database::open(&path).expect("reopen");
        let conversations = reopened.list_conversations(false).expect("conversations");

        assert_eq!(conversations.len(), 1);
    }

    #[test]
    fn interrupted_streams_are_recovered_on_open() {
        let database = database();
        let conversation = database
            .create_conversation("qwen3-0.6b")
            .expect("conversation");
        let message = database
            .append_message(
                conversation.id,
                MessageRole::Assistant,
                "partial",
                "",
                Some("qwen3-0.6b"),
                MessageStatus::Streaming,
            )
            .expect("message");

        recover_interrupted(&database.connection()).expect("recovery");

        let recovered = database.message(message.id).expect("message is kept");
        assert_eq!(recovered.status, MessageStatus::Interrupted);
        assert_eq!(recovered.content, "partial");
    }

    #[test]
    fn titles_take_the_first_forty_characters() {
        assert_eq!(heuristic_title("  hello \n world  "), "hello world");
        assert_eq!(heuristic_title(""), "");
        assert_eq!(
            heuristic_title("a message that is clearly longer than forty characters"),
            "a message that is clearly longer than fo…"
        );
    }

    #[test]
    fn conversations_are_ordered_by_recent_activity() {
        let database = database();
        let first = database.create_conversation("qwen3-0.6b").expect("first");
        std::thread::sleep(std::time::Duration::from_millis(3));
        let second = database.create_conversation("qwen3-1.7b").expect("second");

        let conversations = database.list_conversations(false).expect("list");
        assert_eq!(conversations[0].id, second.id);
        assert_eq!(conversations[1].id, first.id);

        std::thread::sleep(std::time::Duration::from_millis(3));
        database
            .append_message(
                first.id,
                MessageRole::User,
                "bring the first one back",
                "",
                Some("qwen3-0.6b"),
                MessageStatus::Complete,
            )
            .expect("message");

        let conversations = database.list_conversations(false).expect("list");
        assert_eq!(conversations[0].id, first.id);
        assert_eq!(conversations[0].title, "bring the first one back");
    }

    #[test]
    fn favorites_come_first_and_archived_conversations_are_filtered() {
        let database = database();
        let first = database.create_conversation("qwen3-0.6b").expect("first");
        std::thread::sleep(std::time::Duration::from_millis(3));
        let second = database.create_conversation("qwen3-1.7b").expect("second");

        database
            .set_conversation_favorite(first.id, true)
            .expect("favorite");

        let conversations = database.list_conversations(false).expect("list");
        assert_eq!(conversations[0].id, first.id);
        assert!(conversations[0].favorite);
        assert_eq!(conversations[1].id, second.id);

        database
            .set_conversation_archived(second.id, true)
            .expect("archive");

        let active = database.list_conversations(false).expect("active");
        assert_eq!(active.len(), 1);
        assert_eq!(active[0].id, first.id);

        let archived = database.list_conversations(true).expect("archived");
        assert_eq!(archived.len(), 1);
        assert_eq!(archived[0].id, second.id);
        assert!(archived[0].archived);

        let restored = database
            .set_conversation_archived(second.id, false)
            .expect("restore");
        assert!(!restored.archived);
        assert_eq!(database.list_conversations(false).expect("active").len(), 2);
    }

    #[test]
    fn switching_models_records_a_divider_once() {
        let database = database();
        let conversation = database
            .create_conversation("qwen3-0.6b")
            .expect("conversation");
        let change = database
            .set_conversation_model(conversation.id, "qwen3-1.7b")
            .expect("switch");

        assert_eq!(change.conversation.model_alias, "qwen3-1.7b");
        let divider = change.divider.expect("divider");
        assert_eq!(divider.role, MessageRole::Divider);
        assert_eq!(divider.model_alias.as_deref(), Some("qwen3-1.7b"));

        let again = database
            .set_conversation_model(conversation.id, "qwen3-1.7b")
            .expect("same model");
        assert!(again.divider.is_none());
        assert_eq!(
            database
                .list_messages(conversation.id, None, None)
                .expect("page")
                .messages
                .len(),
            1
        );
    }

    #[test]
    fn prompt_history_skips_dividers_and_placeholders() {
        let database = database();
        let conversation = database
            .create_conversation("qwen3-0.6b")
            .expect("conversation");
        database
            .append_message(
                conversation.id,
                MessageRole::User,
                "hello",
                "",
                None,
                MessageStatus::Complete,
            )
            .expect("user");
        let assistant = database
            .append_message(
                conversation.id,
                MessageRole::Assistant,
                "",
                "",
                None,
                MessageStatus::Streaming,
            )
            .expect("assistant");
        database
            .set_conversation_model(conversation.id, "qwen3-1.7b")
            .expect("divider");

        let history = database
            .messages_for_prompt(conversation.id, assistant.id)
            .expect("history");

        assert_eq!(history.len(), 1);
        assert_eq!(history[0].content, "hello");
    }

    #[test]
    fn conversation_usage_sums_finished_messages() {
        let database = database();
        let conversation = database
            .create_conversation("qwen3-0.6b")
            .expect("conversation");

        assert_eq!(
            database.conversation_usage(conversation.id).expect("usage"),
            TokenUsage::default()
        );

        let first = database
            .append_message(
                conversation.id,
                MessageRole::Assistant,
                "one",
                "",
                None,
                MessageStatus::Streaming,
            )
            .expect("assistant");
        let second = database
            .append_message(
                conversation.id,
                MessageRole::Assistant,
                "two",
                "",
                None,
                MessageStatus::Streaming,
            )
            .expect("assistant");

        database
            .finish_message(
                first.id,
                MessageStatus::Complete,
                "one",
                "",
                Some(TokenUsage {
                    prompt_tokens: 10,
                    completion_tokens: 4,
                    total_tokens: 14,
                }),
                None,
            )
            .expect("finish");
        database
            .finish_message(
                second.id,
                MessageStatus::Complete,
                "two",
                "",
                Some(TokenUsage {
                    prompt_tokens: 5,
                    completion_tokens: 3,
                    total_tokens: 8,
                }),
                None,
            )
            .expect("finish");

        assert_eq!(
            database.conversation_usage(conversation.id).expect("usage"),
            TokenUsage {
                prompt_tokens: 15,
                completion_tokens: 7,
                total_tokens: 22,
            }
        );
    }

    #[test]
    fn finishing_a_message_stores_usage_and_error() {
        let database = database();
        let conversation = database
            .create_conversation("qwen3-0.6b")
            .expect("conversation");
        let message = database
            .append_message(
                conversation.id,
                MessageRole::Assistant,
                "part",
                "",
                None,
                MessageStatus::Streaming,
            )
            .expect("assistant");
        let usage = TokenUsage {
            prompt_tokens: 12,
            completion_tokens: 34,
            total_tokens: 46,
        };
        let error = FoundryError::Timeout {
            stage: "token stream".to_string(),
            seconds: 60,
        };

        let finished = database
            .finish_message(
                message.id,
                MessageStatus::Error,
                "part",
                "thinking",
                Some(usage),
                Some(&error),
            )
            .expect("finish");

        assert_eq!(finished.status, MessageStatus::Error);
        assert_eq!(finished.usage, Some(usage));
        assert_eq!(finished.error.expect("error").code, "Timeout");
        assert_eq!(finished.reasoning, "thinking");
    }

    #[test]
    fn a_file_database_survives_a_restart() {
        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().join("foundryui.sqlite3");
        let database = Database::open(&path).expect("open");
        let conversation = database
            .create_conversation("qwen3-0.6b")
            .expect("conversation");
        let user = database
            .append_message(
                conversation.id,
                MessageRole::User,
                "stay with me",
                "",
                Some("qwen3-0.6b"),
                MessageStatus::Complete,
            )
            .expect("user");
        let assistant = database
            .append_message(
                conversation.id,
                MessageRole::Assistant,
                "",
                "",
                Some("qwen3-0.6b"),
                MessageStatus::Streaming,
            )
            .expect("assistant");
        database
            .finish_message(
                assistant.id,
                MessageStatus::Complete,
                "still here",
                "",
                None,
                None,
            )
            .expect("finish");

        drop(database);

        let reopened = Database::open(&path).expect("reopen");
        let conversations = reopened.list_conversations(false).expect("conversations");
        let page = reopened
            .list_messages(conversation.id, None, None)
            .expect("messages");

        assert_eq!(conversations.len(), 1);
        assert_eq!(conversations[0].title, "stay with me");
        assert_eq!(page.messages.len(), 2);
        assert_eq!(page.messages[0].id, user.id);
        assert_eq!(page.messages[1].content, "still here");
    }

    #[test]
    fn an_edited_conversation_survives_a_restart() {
        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().join("foundryui.sqlite3");
        let database = Database::open(&path).expect("open");
        let conversation = database
            .create_conversation("qwen3-0.6b")
            .expect("conversation");
        let first = database
            .append_message(
                conversation.id,
                MessageRole::User,
                "first prompt",
                "",
                Some("qwen3-0.6b"),
                MessageStatus::Complete,
            )
            .expect("first");
        let second = database
            .append_message(
                conversation.id,
                MessageRole::Assistant,
                "first reply",
                "",
                Some("qwen3-0.6b"),
                MessageStatus::Complete,
            )
            .expect("second");
        let third = database
            .append_message(
                conversation.id,
                MessageRole::User,
                "second prompt",
                "",
                Some("qwen3-0.6b"),
                MessageStatus::Complete,
            )
            .expect("third");

        let (removed, edited) = database
            .replace_from(
                conversation.id,
                first.seq,
                "edited prompt",
                "qwen3-0.6b",
                &[],
            )
            .expect("replace");
        let reply = database
            .append_message(
                conversation.id,
                MessageRole::Assistant,
                "edited reply",
                "",
                Some("qwen3-0.6b"),
                MessageStatus::Complete,
            )
            .expect("reply");

        assert_eq!(removed, vec![first.id, second.id, third.id]);
        assert_eq!(edited.seq, first.seq);

        drop(database);

        let reopened = Database::open(&path).expect("reopen");
        let page = reopened
            .list_messages(conversation.id, None, None)
            .expect("messages");

        assert_eq!(page.messages.len(), 2);
        assert_eq!(page.messages[0].id, edited.id);
        assert_eq!(page.messages[0].content, "edited prompt");
        assert_eq!(page.messages[1].id, reply.id);
        assert_eq!(page.messages[1].content, "edited reply");
        assert_eq!(
            reopened.list_conversations(false).expect("conversations")[0].title,
            "first prompt"
        );
    }

    #[test]
    fn attachments_and_resolved_params_survive_a_restart() {
        use super::super::attachments::{StagedAttachment, StagedFile};
        use super::super::params::{ParamOrigin, Resolved};

        let directory = tempfile::tempdir().expect("temp dir");
        let path = directory.path().join("foundryui.sqlite3");
        let image = directory.path().join("image.png");

        std::fs::write(&image, b"\x89PNG\r\n\x1a\n").expect("image");

        let database = Database::open(&path).expect("open");
        let conversation = database
            .create_conversation("qwen3.5-0.8b")
            .expect("conversation");
        let user = database
            .append_message(
                conversation.id,
                MessageRole::User,
                "describe it",
                "",
                Some("qwen3.5-0.8b"),
                MessageStatus::Complete,
            )
            .expect("user");

        database
            .add_attachment(
                user.id,
                "image",
                &StagedFile {
                    meta: StagedAttachment {
                        token: "att-1".to_string(),
                        mime: "image/png".to_string(),
                        bytes: 8,
                        width: Some(1),
                        height: Some(1),
                    },
                    path: image,
                },
            )
            .expect("attachment");

        let assistant = database
            .append_message(
                conversation.id,
                MessageRole::Assistant,
                "a pixel",
                "",
                Some("qwen3.5-0.8b"),
                MessageStatus::Complete,
            )
            .expect("assistant");

        database
            .set_message_params(
                assistant.id,
                &ResolvedParams {
                    temperature: Resolved {
                        value: Some(0.3),
                        origin: ParamOrigin::Conversation,
                    },
                    ..ResolvedParams::default()
                },
            )
            .expect("params");

        drop(database);

        let reopened = Database::open(&path).expect("reopen");
        let page = reopened
            .list_messages(conversation.id, None, None)
            .expect("messages");
        let params = page.messages[1].params.as_ref().expect("stored params");

        assert_eq!(page.messages[0].attachments.len(), 1);
        assert_eq!(page.messages[0].attachments[0].mime, "image/png");
        assert_eq!(params.temperature.value, Some(0.3));
        assert_eq!(params.temperature.origin, ParamOrigin::Conversation);
    }

    #[test]
    fn search_finds_terms_with_highlight_and_scope() {
        let database = database();
        let first = database
            .create_conversation("qwen3-0.6b")
            .expect("first conversation");
        let second = database
            .create_conversation("qwen3-1.7b")
            .expect("second conversation");
        let message = database
            .append_message(
                first.id,
                MessageRole::User,
                "The needle is hidden in this haystack",
                "",
                None,
                MessageStatus::Complete,
            )
            .expect("first message");
        database
            .append_message(
                second.id,
                MessageRole::User,
                "another needle elsewhere",
                "",
                None,
                MessageStatus::Complete,
            )
            .expect("second message");

        let scoped = database
            .search_messages("needle", Some(first.id), None)
            .expect("scoped search");
        let global = database
            .search_messages("needle", None, None)
            .expect("global search");

        assert_eq!(scoped.len(), 1);
        assert_eq!(scoped[0].message_id, message.id);
        assert_eq!(scoped[0].conversation_id, first.id);
        assert_eq!(scoped[0].role, MessageRole::User);
        assert_eq!(scoped[0].seq, message.seq);
        assert!(
            scoped[0]
                .snippet
                .contains(&format!("{SNIPPET_START}needle{SNIPPET_END}"))
        );
        assert_eq!(global.len(), 2);
        assert!(global.iter().any(|hit| hit.conversation_id == second.id));
    }

    #[test]
    fn search_matches_prefixes_and_requires_every_term() {
        let database = database();
        let conversation = database
            .create_conversation("qwen3-0.6b")
            .expect("conversation");

        database
            .append_message(
                conversation.id,
                MessageRole::User,
                "Regenerating the answer keeps the prompt",
                "",
                None,
                MessageStatus::Complete,
            )
            .expect("message");

        assert_eq!(
            database
                .search_messages("regene", None, None)
                .expect("prefix")
                .len(),
            1
        );
        assert_eq!(
            database
                .search_messages("regenerating prompt", None, None)
                .expect("two terms")
                .len(),
            1
        );
        assert!(
            database
                .search_messages("regenerating exported", None, None)
                .expect("missing term")
                .is_empty()
        );
        assert!(
            database
                .search_messages("???", None, None)
                .expect("punctuation")
                .is_empty()
        );
        assert!(
            database
                .search_messages("   ", None, None)
                .expect("blank")
                .is_empty()
        );
    }

    #[test]
    fn truncation_reports_removed_messages_and_finds_the_last_prompt() {
        let database = database();
        let conversation = database
            .create_conversation("qwen3-0.6b")
            .expect("conversation");
        let first = database
            .append_message(
                conversation.id,
                MessageRole::User,
                "first prompt",
                "",
                None,
                MessageStatus::Complete,
            )
            .expect("first prompt");
        let first_reply = database
            .append_message(
                conversation.id,
                MessageRole::Assistant,
                "first reply",
                "",
                None,
                MessageStatus::Complete,
            )
            .expect("first reply");
        let second = database
            .append_message(
                conversation.id,
                MessageRole::User,
                "second prompt",
                "",
                None,
                MessageStatus::Complete,
            )
            .expect("second prompt");
        let second_reply = database
            .append_message(
                conversation.id,
                MessageRole::Assistant,
                "second reply",
                "",
                None,
                MessageStatus::Complete,
            )
            .expect("second reply");

        let removed = database
            .truncate_after(conversation.id, second.seq)
            .expect("truncate reply");

        assert_eq!(removed, vec![second_reply.id]);
        assert_eq!(
            database
                .latest_user_message(conversation.id)
                .expect("last prompt")
                .id,
            second.id
        );
        assert_eq!(
            database
                .list_messages(conversation.id, None, None)
                .expect("page")
                .messages
                .len(),
            3
        );

        let removed = database
            .truncate_from(conversation.id, second.seq)
            .expect("truncate prompt");

        assert_eq!(removed, vec![second.id]);
        assert_eq!(
            database
                .latest_user_message(conversation.id)
                .expect("last prompt")
                .id,
            first.id
        );
        assert_eq!(
            database
                .list_messages(conversation.id, None, None)
                .expect("page")
                .messages
                .last()
                .expect("last message")
                .id,
            first_reply.id
        );
    }

    #[test]
    fn search_answers_in_under_one_hundred_milliseconds_with_five_thousand_messages() {
        let database = database();
        let conversation = database
            .create_conversation("qwen3-0.6b")
            .expect("conversation");
        let mut connection = database.connection();
        let transaction = connection.transaction().expect("transaction");
        let now = now_millis();

        for seq in 1..=5_000 {
            transaction
                .execute(
                    "INSERT INTO messages
                       (conversation_id, seq, role, content_md, created_at, updated_at)
                     VALUES (?1, ?2, 'user', ?3, ?4, ?4)",
                    params![
                        conversation.id,
                        seq,
                        format!("message {seq} with filler text"),
                        now
                    ],
                )
                .expect("insert");
        }

        transaction.commit().expect("commit");
        drop(connection);

        let started = Instant::now();
        let hits = database
            .search_messages("message 4321", Some(conversation.id), None)
            .expect("search");
        let elapsed = started.elapsed();

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].seq, 4_321);
        assert!(elapsed < Duration::from_millis(100), "{elapsed:?}");
    }

    #[test]
    fn the_latest_page_of_ten_thousand_messages_is_fast() {
        let database = database();
        let conversation = database
            .create_conversation("qwen3-0.6b")
            .expect("conversation");
        let mut connection = database.connection();
        let transaction = connection.transaction().expect("transaction");
        let now = now_millis();

        for seq in 1..=10_000 {
            transaction
                .execute(
                    "INSERT INTO messages
                       (conversation_id, seq, role, content_md, created_at, updated_at)
                     VALUES (?1, ?2, 'user', ?3, ?4, ?4)",
                    params![conversation.id, seq, format!("message {seq}"), now],
                )
                .expect("insert");
        }

        transaction.commit().expect("commit");
        drop(connection);

        let started = Instant::now();
        let page = database
            .list_messages(conversation.id, None, Some(80))
            .expect("page");
        let elapsed = started.elapsed();

        assert_eq!(page.messages.len(), 80);
        assert!(page.has_more);
        assert_eq!(page.messages.first().expect("first").seq, 9_921);
        assert_eq!(page.messages.last().expect("last").seq, 10_000);
        assert!(elapsed < Duration::from_millis(300), "{elapsed:?}");
    }
}
