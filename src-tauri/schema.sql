CREATE TABLE IF NOT EXISTS conversations (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  title TEXT NOT NULL DEFAULT '',
  model_alias TEXT NOT NULL,
  params_json TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  favorite INTEGER NOT NULL DEFAULT 0,
  archived INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS conversations_updated ON conversations(archived, updated_at DESC);

CREATE INDEX IF NOT EXISTS conversations_favorite
  ON conversations(archived, favorite DESC, updated_at DESC);

CREATE TABLE IF NOT EXISTS messages (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  conversation_id INTEGER NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
  seq INTEGER NOT NULL,
  role TEXT NOT NULL CHECK (role IN ('user', 'assistant', 'divider')),
  content_md TEXT NOT NULL DEFAULT '',
  reasoning_md TEXT NOT NULL DEFAULT '',
  model_alias TEXT,
  params_json TEXT,
  usage_json TEXT,
  error_json TEXT,
  status TEXT NOT NULL DEFAULT 'complete'
    CHECK (status IN ('streaming', 'complete', 'cancelled', 'error', 'interrupted')),
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE UNIQUE INDEX IF NOT EXISTS messages_conversation_seq ON messages(conversation_id, seq);

CREATE TABLE IF NOT EXISTS attachments (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  message_id INTEGER NOT NULL REFERENCES messages(id) ON DELETE CASCADE,
  kind TEXT NOT NULL,
  mime TEXT,
  path TEXT NOT NULL,
  bytes INTEGER,
  width INTEGER,
  height INTEGER,
  created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS attachments_message ON attachments(message_id);

CREATE TABLE IF NOT EXISTS model_params (
  model_alias TEXT PRIMARY KEY,
  params_json TEXT NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS settings (
  key TEXT PRIMARY KEY,
  value_json TEXT NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE VIRTUAL TABLE IF NOT EXISTS messages_fts USING fts5(
  content_md,
  content='messages',
  content_rowid='id',
  tokenize='unicode61'
);

CREATE TRIGGER IF NOT EXISTS messages_fts_insert AFTER INSERT ON messages BEGIN
  INSERT INTO messages_fts(rowid, content_md) VALUES (new.id, new.content_md);
END;

CREATE TRIGGER IF NOT EXISTS messages_fts_delete AFTER DELETE ON messages BEGIN
  INSERT INTO messages_fts(messages_fts, rowid, content_md)
  VALUES ('delete', old.id, old.content_md);
END;

CREATE TRIGGER IF NOT EXISTS messages_fts_update AFTER UPDATE OF content_md ON messages BEGIN
  INSERT INTO messages_fts(messages_fts, rowid, content_md)
  VALUES ('delete', old.id, old.content_md);
  INSERT INTO messages_fts(rowid, content_md) VALUES (new.id, new.content_md);
END;

CREATE TABLE IF NOT EXISTS transcriptions (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  model_alias TEXT NOT NULL,
  source_path TEXT NOT NULL,
  language TEXT,
  params_json TEXT,
  text TEXT NOT NULL DEFAULT '',
  duration_ms INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  favorite INTEGER NOT NULL DEFAULT 0,
  archived INTEGER NOT NULL DEFAULT 0
);

CREATE INDEX IF NOT EXISTS transcriptions_list
  ON transcriptions(archived, favorite DESC, created_at DESC);

CREATE TABLE IF NOT EXISTS embedding_corpora (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL,
  model_alias TEXT NOT NULL,
  dimensions INTEGER NOT NULL DEFAULT 0,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  favorite INTEGER NOT NULL DEFAULT 0,
  archived INTEGER NOT NULL DEFAULT 0
);

CREATE TABLE IF NOT EXISTS corpus_documents (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  corpus_id INTEGER NOT NULL,
  position INTEGER NOT NULL,
  text TEXT NOT NULL,
  source_path TEXT,
  created_at INTEGER NOT NULL,
  FOREIGN KEY(corpus_id) REFERENCES embedding_corpora(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS corpus_vectors (
  corpus_id INTEGER NOT NULL,
  document_id INTEGER PRIMARY KEY,
  dimensions INTEGER NOT NULL,
  vector BLOB NOT NULL,
  created_at INTEGER NOT NULL,
  FOREIGN KEY(corpus_id) REFERENCES embedding_corpora(id) ON DELETE CASCADE,
  FOREIGN KEY(document_id) REFERENCES corpus_documents(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS embedding_corpora_list
  ON embedding_corpora(archived, favorite DESC, updated_at DESC);
CREATE INDEX IF NOT EXISTS corpus_documents_corpus ON corpus_documents(corpus_id, position);
CREATE INDEX IF NOT EXISTS corpus_vectors_corpus ON corpus_vectors(corpus_id);
