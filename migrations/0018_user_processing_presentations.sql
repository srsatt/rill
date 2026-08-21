ALTER TABLE user_product_state
ADD COLUMN processing_prompt TEXT NOT NULL DEFAULT '';

CREATE TABLE user_document_presentations (
  user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
  document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
  input_checksum BLOB NOT NULL,
  included INTEGER NOT NULL CHECK (included IN (0, 1)),
  summary_text TEXT NOT NULL,
  provider TEXT NOT NULL,
  model TEXT NOT NULL,
  model_version TEXT NOT NULL,
  updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
  PRIMARY KEY (user_id, document_id)
) STRICT;

CREATE INDEX user_document_presentations_document
ON user_document_presentations(document_id, user_id);
