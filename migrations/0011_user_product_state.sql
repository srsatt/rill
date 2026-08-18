CREATE TABLE user_product_state (
  user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
  default_streams_seeded INTEGER NOT NULL DEFAULT 0
    CHECK (default_streams_seeded IN (0, 1)),
  created_at INTEGER NOT NULL DEFAULT (unixepoch()),
  updated_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;
