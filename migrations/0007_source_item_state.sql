ALTER TABLE raw_items ADD COLUMN edited_at INTEGER;
ALTER TABLE raw_items ADD COLUMN deleted_at INTEGER;
ALTER TABLE raw_items ADD COLUMN external_urls_json TEXT NOT NULL DEFAULT '[]'
    CHECK (json_valid(external_urls_json));
ALTER TABLE raw_items ADD COLUMN media_json TEXT NOT NULL DEFAULT '[]'
    CHECK (json_valid(media_json));

ALTER TABLE telegram_accounts ADD COLUMN last_attempt_at INTEGER;
ALTER TABLE telegram_accounts ADD COLUMN last_success_at INTEGER;
ALTER TABLE telegram_accounts ADD COLUMN consecutive_failures INTEGER NOT NULL DEFAULT 0;
ALTER TABLE telegram_accounts ADD COLUMN last_error_message TEXT;
ALTER TABLE telegram_accounts ADD COLUMN reconnect_count INTEGER NOT NULL DEFAULT 0;
