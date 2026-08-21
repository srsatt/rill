UPDATE device_sessions
SET selected_stream_id = NULL
WHERE selected_stream_id IN (SELECT id FROM streams WHERE slug = 'home');

DELETE FROM recommendation_runs
WHERE stream_id IN (SELECT id FROM streams WHERE slug = 'home');

DELETE FROM embedding_records
WHERE entity_type = 'stream'
  AND entity_id IN (SELECT id FROM streams WHERE slug = 'home');

DELETE FROM streams WHERE slug = 'home';
