CREATE TABLE IF NOT EXISTS agent_authorization_revisions (
  conversation_id INTEGER PRIMARY KEY REFERENCES conversations(id) ON DELETE CASCADE,
  authorization_revision INTEGER NOT NULL DEFAULT 0,
  mode_revision INTEGER NOT NULL DEFAULT 0
);

CREATE TRIGGER IF NOT EXISTS agent_authorization_mode_changed
AFTER UPDATE OF mode ON agent_contexts WHEN OLD.mode != NEW.mode
BEGIN
  INSERT INTO agent_authorization_revisions(conversation_id, mode_revision)
    VALUES (NEW.conversation_id, 1)
    ON CONFLICT(conversation_id) DO UPDATE SET mode_revision = mode_revision + 1;
END;

CREATE TABLE IF NOT EXISTS agent_authorization_settings (
  id INTEGER PRIMARY KEY CHECK (id = 1),
  read_allowed INTEGER NOT NULL DEFAULT 1 CHECK (read_allowed IN (0, 1))
);
INSERT OR IGNORE INTO agent_authorization_settings(id) VALUES (1);

CREATE TABLE IF NOT EXISTS agent_authorization_roots (
  conversation_id INTEGER NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
  context_revision INTEGER NOT NULL,
  root_id TEXT NOT NULL,
  action_scope INTEGER NOT NULL DEFAULT 0,
  display_path TEXT NOT NULL,
  PRIMARY KEY(conversation_id, context_revision, root_id, action_scope)
);

CREATE TABLE IF NOT EXISTS agent_authorization_proposals (
  action_id INTEGER PRIMARY KEY REFERENCES agent_actions(id) ON DELETE CASCADE,
  conversation_id INTEGER NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
  context_revision INTEGER NOT NULL,
  authorization_revision INTEGER NOT NULL,
  mode_revision INTEGER NOT NULL,
  binding_fingerprint TEXT NOT NULL,
  semantic_fingerprint TEXT NOT NULL CHECK(length(semantic_fingerprint) = 64),
  project_fingerprint TEXT NOT NULL,
  proposal_mode TEXT NOT NULL CHECK (proposal_mode IN ('review', 'automatic')),
  status TEXT NOT NULL CHECK (status IN ('ready', 'pending', 'denied', 'claimed')),
  denial TEXT,
  created_at INTEGER NOT NULL
);

CREATE TABLE IF NOT EXISTS agent_authorization_requests (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  action_id INTEGER NOT NULL REFERENCES agent_authorization_proposals(action_id) ON DELETE CASCADE,
  requirement_fingerprint TEXT NOT NULL,
  match_fingerprint TEXT NOT NULL,
  display_json TEXT NOT NULL CHECK(json_valid(display_json)),
  remember_eligible INTEGER NOT NULL CHECK(remember_eligible IN (0, 1)),
  response TEXT NOT NULL DEFAULT 'pending' CHECK(response IN ('pending', 'approved', 'denied')),
  remembered INTEGER NOT NULL DEFAULT 0 CHECK(remembered IN (0, 1)),
  decided_at INTEGER,
  UNIQUE(action_id, requirement_fingerprint)
);

CREATE TABLE IF NOT EXISTS agent_authorization_proposal_roots (
  action_id INTEGER NOT NULL REFERENCES agent_authorization_proposals(action_id) ON DELETE CASCADE,
  root_id TEXT NOT NULL,
  PRIMARY KEY(action_id, root_id)
);

CREATE TABLE IF NOT EXISTS agent_authorization_denial_latches (
  run_id INTEGER NOT NULL,
  conversation_id INTEGER NOT NULL,
  context_revision INTEGER NOT NULL,
  semantic_fingerprint TEXT NOT NULL CHECK(length(semantic_fingerprint) = 64),
  source_action_id INTEGER NOT NULL REFERENCES agent_actions(id) ON DELETE CASCADE,
  created_at INTEGER NOT NULL,
  PRIMARY KEY(run_id, conversation_id, context_revision, semantic_fingerprint),
  FOREIGN KEY(run_id, conversation_id) REFERENCES agent_runs(id, conversation_id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS agent_authorization_grants (
  id INTEGER PRIMARY KEY AUTOINCREMENT,
  conversation_id INTEGER NOT NULL REFERENCES conversations(id) ON DELETE CASCADE,
  context_revision INTEGER NOT NULL,
  authorization_revision INTEGER NOT NULL,
  mode_revision INTEGER NOT NULL,
  match_fingerprint TEXT NOT NULL,
  display_json TEXT NOT NULL CHECK(json_valid(display_json)),
  revoked INTEGER NOT NULL DEFAULT 0 CHECK(revoked IN (0, 1)),
  created_at INTEGER NOT NULL
);

CREATE INDEX IF NOT EXISTS agent_authorization_grants_context
  ON agent_authorization_grants(conversation_id, context_revision, authorization_revision, mode_revision);
CREATE INDEX IF NOT EXISTS agent_authorization_proposals_context
  ON agent_authorization_proposals(conversation_id, action_id);
