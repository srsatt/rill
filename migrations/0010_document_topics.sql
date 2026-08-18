CREATE TABLE document_topics (
  document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
  topic TEXT NOT NULL COLLATE NOCASE,
  confidence REAL NOT NULL CHECK (confidence >= 0 AND confidence <= 1),
  provider TEXT NOT NULL,
  model TEXT NOT NULL,
  model_version TEXT NOT NULL,
  input_checksum BLOB NOT NULL,
  created_at INTEGER NOT NULL DEFAULT (unixepoch()),
  PRIMARY KEY (
    document_id, topic, provider, model, model_version, input_checksum
  )
) STRICT;

CREATE INDEX document_topics_topic
ON document_topics(topic COLLATE NOCASE, confidence DESC);

CREATE INDEX document_topics_document
ON document_topics(document_id, created_at DESC);

CREATE VIEW story_topics AS
SELECT sm.story_id, dt.topic, max(dt.confidence) AS confidence
FROM story_memberships sm
JOIN document_topics dt ON dt.document_id=sm.document_id
JOIN documents d ON d.id=dt.document_id AND d.exact_content_hash=dt.input_checksum
GROUP BY sm.story_id, dt.topic;
