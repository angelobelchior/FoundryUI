CREATE TABLE IF NOT EXISTS conversation_snapshot_metadata (
  snapshot_id INTEGER PRIMARY KEY REFERENCES conversation_snapshots(id) ON DELETE CASCADE,
  metadata_json TEXT NOT NULL CHECK (json_valid(metadata_json) AND json_type(metadata_json) = 'object')
);
