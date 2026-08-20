ALTER TABLE source_instances
ADD COLUMN processing_prompt TEXT NOT NULL DEFAULT '';

ALTER TABLE document_curators
ADD COLUMN included INTEGER NOT NULL DEFAULT 1 CHECK (included IN (0, 1));

DROP VIEW document_access;

CREATE VIEW document_access AS
SELECT DISTINCT dc.document_id, sa.user_id
FROM document_curators dc
JOIN source_access sa ON sa.source_instance_id = dc.source_instance_id
WHERE dc.included = 1
UNION
SELECT d.id, NULL
FROM documents d
WHERE d.visibility_scope='public'
  AND (
    NOT EXISTS (SELECT 1 FROM document_curators dc WHERE dc.document_id=d.id)
    OR EXISTS (
      SELECT 1 FROM document_curators dc
      WHERE dc.document_id=d.id AND dc.source_instance_id IS NULL AND dc.included=1
    )
  )
UNION
SELECT d.id, substr(d.visibility_scope, 6)
FROM documents d
WHERE d.visibility_scope LIKE 'user:%'
  AND (
    NOT EXISTS (SELECT 1 FROM document_curators dc WHERE dc.document_id=d.id)
    OR EXISTS (
      SELECT 1 FROM document_curators dc
      WHERE dc.document_id=d.id AND dc.source_instance_id IS NULL AND dc.included=1
    )
  );
