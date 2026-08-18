DROP VIEW document_access;

CREATE VIEW document_access AS
SELECT DISTINCT dc.document_id, sa.user_id
FROM document_curators dc
JOIN source_access sa ON sa.source_instance_id = dc.source_instance_id
UNION
SELECT d.id, NULL
FROM documents d
WHERE d.visibility_scope='public'
  AND (
    NOT EXISTS (SELECT 1 FROM document_curators dc WHERE dc.document_id=d.id)
    OR EXISTS (
      SELECT 1 FROM document_curators dc
      WHERE dc.document_id=d.id AND dc.source_instance_id IS NULL
    )
  );

CREATE TRIGGER source_instances_delete_content_guard
BEFORE DELETE ON source_instances BEGIN
  DELETE FROM documents
  WHERE id IN (
    SELECT dc.document_id FROM document_curators dc
    WHERE dc.source_instance_id=old.id
      AND NOT EXISTS (
        SELECT 1 FROM document_curators remaining
        WHERE remaining.document_id=dc.document_id
          AND (remaining.source_instance_id IS NULL
            OR remaining.source_instance_id!=old.id)
      )
  );
  DELETE FROM document_curators WHERE source_instance_id=old.id;
  DELETE FROM stories
  WHERE NOT EXISTS (
    SELECT 1 FROM story_memberships sm WHERE sm.story_id=stories.id
  );
END;

CREATE INDEX telegram_bot_update_claims_created_at
ON telegram_bot_update_claims(created_at);

CREATE TABLE telegram_bot_rate_limits (
  scope_key TEXT PRIMARY KEY,
  window_started_at INTEGER NOT NULL,
  message_count INTEGER NOT NULL CHECK (message_count > 0),
  updated_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;
CREATE INDEX telegram_bot_rate_limits_updated_at
ON telegram_bot_rate_limits(updated_at);
