ALTER TABLE collection_expansions ADD COLUMN parent_display_policy TEXT NOT NULL
    DEFAULT 'children_only'
    CHECK (parent_display_policy IN ('children_only', 'parent_and_children', 'parent_only'));

CREATE TABLE collection_overrides (
    parent_raw_item_id TEXT PRIMARY KEY REFERENCES raw_items(id) ON DELETE CASCADE,
    detection_mode TEXT NOT NULL CHECK (detection_mode IN ('force_collection', 'force_single')),
    actor_user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;
