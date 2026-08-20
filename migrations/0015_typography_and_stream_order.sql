ALTER TABLE user_product_state ADD COLUMN font_family TEXT NOT NULL DEFAULT 'sans'
  CHECK (font_family IN ('sans', 'serif'));

UPDATE streams
SET sort_order = sort_order + 1
WHERE slug != 'all'
  AND EXISTS (
    SELECT 1 FROM streams all_stream
    WHERE all_stream.owner_user_id = streams.owner_user_id AND all_stream.slug = 'all'
  );

UPDATE streams SET sort_order = 0 WHERE slug = 'all';
