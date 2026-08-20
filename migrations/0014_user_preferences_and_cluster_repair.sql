ALTER TABLE user_product_state ADD COLUMN ai_free_mode INTEGER NOT NULL DEFAULT 0
  CHECK (ai_free_mode IN (0, 1));
ALTER TABLE user_product_state ADD COLUMN stream_membership_mode TEXT NOT NULL DEFAULT 'multiple'
  CHECK (stream_membership_mode IN ('multiple', 'exclusive'));

UPDATE streams
SET sort_order = sort_order + 1
WHERE slug != 'home'
  AND NOT EXISTS (
    SELECT 1 FROM streams all_stream
    WHERE all_stream.owner_user_id = streams.owner_user_id AND all_stream.slug = 'all'
  );

INSERT INTO streams(
  id, owner_user_id, name, slug, definition_text, ranking_instruction, sort_order, filter_json
)
SELECT lower(hex(randomblob(16))), users.id, 'All', 'all',
  'Every visible story, without subject filtering.',
  'Show newest stories first.', 1, '{}'
FROM users
WHERE NOT EXISTS (
  SELECT 1 FROM streams WHERE streams.owner_user_id = users.id AND streams.slug = 'all'
);

-- One publisher is one coverage voice. Earlier clustering could merge recurring
-- editions from the same publisher because it only bounded one side of time.
CREATE TEMP TABLE rill_split_memberships(
  original_story_id TEXT NOT NULL,
  document_id TEXT PRIMARY KEY,
  replacement_story_id TEXT NOT NULL
);

INSERT INTO rill_split_memberships(original_story_id, document_id, replacement_story_id)
SELECT story_id, document_id, lower(hex(randomblob(16)))
FROM (
  SELECT sm.story_id, sm.document_id,
    row_number() OVER (
      PARTITION BY sm.story_id, lower(d.publisher)
      ORDER BY coalesce(d.published_at, d.created_at) DESC, d.id
    ) AS publisher_rank
  FROM story_memberships sm
  JOIN documents d ON d.id = sm.document_id
  WHERE d.publisher IS NOT NULL AND trim(d.publisher) != ''
    AND NOT EXISTS (
      SELECT 1 FROM manual_cluster_overrides manual WHERE manual.document_id = sm.document_id
    )
)
WHERE publisher_rank > 1;

INSERT INTO stories(id, visibility_scope, cluster_version, anchor_document_id, created_at, updated_at)
SELECT split.replacement_story_id, story.visibility_scope, 'publisher-repair-v1',
  split.document_id, story.created_at, unixepoch()
FROM rill_split_memberships split
JOIN stories story ON story.id = split.original_story_id;

UPDATE story_memberships
SET story_id = (
  SELECT replacement_story_id FROM rill_split_memberships split
  WHERE split.document_id = story_memberships.document_id
)
WHERE document_id IN (SELECT document_id FROM rill_split_memberships);

UPDATE stories
SET anchor_document_id = (
  SELECT membership.document_id
  FROM story_memberships membership
  JOIN documents document ON document.id = membership.document_id
  WHERE membership.story_id = stories.id
  ORDER BY coalesce(document.published_at, document.created_at) DESC, document.id
  LIMIT 1
)
WHERE anchor_document_id IS NULL OR NOT EXISTS (
  SELECT 1 FROM story_memberships membership
  WHERE membership.story_id = stories.id AND membership.document_id = stories.anchor_document_id
);

DROP TABLE rill_split_memberships;
