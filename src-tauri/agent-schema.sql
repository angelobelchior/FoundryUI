CREATE TABLE IF NOT EXISTS agent_projects (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  name TEXT NOT NULL,
  root_identity_key TEXT NOT NULL UNIQUE,
  display_path TEXT NOT NULL,
  binding_revision INTEGER NOT NULL DEFAULT 1
    CHECK (typeof(binding_revision) = 'integer' AND binding_revision > 0),
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS agent_contexts (
  conversation_id INTEGER PRIMARY KEY REFERENCES conversations(id) ON DELETE CASCADE,
  project_id INTEGER REFERENCES agent_projects(id) ON DELETE RESTRICT,
  mode TEXT NOT NULL DEFAULT 'review' CHECK (mode IN ('review', 'automatic')),
  context_revision INTEGER NOT NULL DEFAULT 1 CHECK (context_revision > 0)
);

CREATE TABLE IF NOT EXISTS agent_runs (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  conversation_id INTEGER NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
  context_revision INTEGER NOT NULL CHECK (context_revision > 0),
  request_key TEXT NOT NULL,
  state TEXT NOT NULL CHECK (state IN ('prepared', 'running', 'complete', 'failed', 'cancelled', 'interrupted')),
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  UNIQUE (conversation_id, request_key),
  UNIQUE (id, conversation_id)
);

CREATE TABLE IF NOT EXISTS agent_actions (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  run_id INTEGER NOT NULL,
  conversation_id INTEGER NOT NULL,
  context_revision INTEGER NOT NULL,
  call_id TEXT NOT NULL,
  tool TEXT NOT NULL,
  arguments_json TEXT NOT NULL CHECK (json_valid(arguments_json) AND json_type(arguments_json) = 'object'),
  arguments_fingerprint TEXT NOT NULL,
  state TEXT NOT NULL CHECK (state IN ('proposed', 'approved', 'denied', 'started', 'complete', 'failed', 'cancelled', 'interrupted')),
  claim_token TEXT,
  result_fingerprint TEXT,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  FOREIGN KEY (run_id, conversation_id) REFERENCES agent_runs(id, conversation_id) ON DELETE CASCADE,
  UNIQUE (run_id, call_id),
  UNIQUE (id, run_id, conversation_id)
);

CREATE TABLE IF NOT EXISTS agent_approvals (
  action_id INTEGER PRIMARY KEY,
  run_id INTEGER NOT NULL,
  conversation_id INTEGER NOT NULL,
  context_revision INTEGER NOT NULL,
  arguments_fingerprint TEXT NOT NULL,
  decision TEXT NOT NULL CHECK (decision IN ('approved', 'denied')),
  created_at INTEGER NOT NULL,
  FOREIGN KEY (action_id, run_id, conversation_id) REFERENCES agent_actions(id, run_id, conversation_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS agent_history (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  conversation_id INTEGER NOT NULL,
  run_id INTEGER NOT NULL,
  action_id INTEGER,
  seq INTEGER NOT NULL,
  content_json TEXT NOT NULL CHECK (
    json_valid(content_json) AND json_type(content_json) = 'object' AND
    COALESCE(json_extract(content_json, '$.kind') IN ('text', 'tool_call', 'tool_result', 'state_event'), 0)
  ),
  created_at INTEGER NOT NULL,
  FOREIGN KEY (run_id, conversation_id) REFERENCES agent_runs(id, conversation_id) ON DELETE CASCADE,
  FOREIGN KEY (action_id, run_id, conversation_id) REFERENCES agent_actions(id, run_id, conversation_id) ON DELETE CASCADE,
  UNIQUE (conversation_id, seq)
);

CREATE TABLE IF NOT EXISTS conversation_snapshots (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  destination_conversation_id INTEGER NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
  source_conversation_id INTEGER NOT NULL,
  source_title TEXT NOT NULL,
  entries_json TEXT NOT NULL CHECK (json_valid(entries_json) AND json_type(entries_json) = 'array'),
  omitted_attachment_count INTEGER NOT NULL DEFAULT 0,
  captured_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS conversation_snapshots_destination
  ON conversation_snapshots(destination_conversation_id, captured_at, id);

CREATE TABLE IF NOT EXISTS agent_context_facts (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  conversation_id INTEGER NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
  context_revision INTEGER NOT NULL CHECK (context_revision > 0),
  run_id INTEGER,
  fact_key TEXT NOT NULL,
  payload_json TEXT NOT NULL CHECK (
    json_valid(payload_json) AND COALESCE(json_extract(payload_json, '$.kind') IN (
      'permission_grant', 'authorized_root', 'instruction_approval', 'context_manifest',
      'network_event', 'change_set', 'recovery_record'
    ), 0)
  ),
  payload_fingerprint TEXT NOT NULL,
  related_fact_id INTEGER,
  created_at INTEGER NOT NULL,
  FOREIGN KEY (run_id, conversation_id) REFERENCES agent_runs(id, conversation_id) ON DELETE CASCADE,
  FOREIGN KEY (related_fact_id, conversation_id) REFERENCES agent_context_facts(id, conversation_id),
  UNIQUE (conversation_id, context_revision, fact_key),
  UNIQUE (id, conversation_id)
);

CREATE INDEX IF NOT EXISTS agent_context_facts_owner
  ON agent_context_facts(conversation_id, context_revision, id);
