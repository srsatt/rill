ALTER TABLE streams ADD COLUMN icon TEXT;
ALTER TABLE streams ADD COLUMN enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1));
ALTER TABLE streams ADD COLUMN filter_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(filter_json));
ALTER TABLE streams ADD COLUMN ranking_instruction TEXT;
ALTER TABLE streams ADD COLUMN updated_at INTEGER NOT NULL DEFAULT (unixepoch());

CREATE TABLE cluster_evidence (
    id TEXT PRIMARY KEY,
    story_id TEXT NOT NULL REFERENCES stories(id) ON DELETE CASCADE,
    document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    anchor_document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    model_version TEXT NOT NULL,
    similarity REAL NOT NULL,
    decision TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;
CREATE INDEX cluster_evidence_document ON cluster_evidence(document_id, created_at DESC);

CREATE TABLE manual_cluster_overrides (
    id TEXT PRIMARY KEY,
    document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    target_story_id TEXT REFERENCES stories(id) ON DELETE CASCADE,
    operation TEXT NOT NULL CHECK (operation IN ('merge', 'split')),
    actor_user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;

