ALTER TABLE source_instances
ADD COLUMN audience TEXT NOT NULL DEFAULT 'owner'
CHECK (audience IN ('global', 'subscribers', 'owner'));

UPDATE source_instances
SET audience = CASE visibility WHEN 'public' THEN 'global' ELSE 'owner' END;

CREATE INDEX source_instances_audience ON source_instances(audience, enabled);
CREATE INDEX source_subscriptions_source_enabled
ON source_subscriptions(source_instance_id, enabled, user_id);

CREATE TABLE public_telegram_channels (
    source_instance_id TEXT PRIMARY KEY REFERENCES source_instances(id) ON DELETE CASCADE,
    username TEXT NOT NULL COLLATE NOCASE UNIQUE,
    telegram_chat_id INTEGER UNIQUE,
    title TEXT,
    parser_version INTEGER NOT NULL DEFAULT 1,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;

CREATE TABLE telegram_bot_bindings (
    user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    telegram_user_id INTEGER NOT NULL UNIQUE,
    private_chat_id INTEGER NOT NULL UNIQUE,
    telegram_username TEXT,
    display_name TEXT,
    bound_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;

CREATE TABLE telegram_binding_challenges (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash BLOB NOT NULL UNIQUE,
    expires_at INTEGER NOT NULL,
    consumed_at INTEGER,
    attempts_remaining INTEGER NOT NULL DEFAULT 5 CHECK (attempts_remaining >= 0),
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;
CREATE INDEX telegram_binding_challenges_active
ON telegram_binding_challenges(expires_at) WHERE consumed_at IS NULL;

CREATE TABLE telegram_bot_update_claims (
    claim_key TEXT PRIMARY KEY,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;

CREATE TABLE instance_settings (
    setting_key TEXT PRIMARY KEY,
    config_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(config_json)),
    credential_secret_id TEXT REFERENCES encrypted_secrets(id) ON DELETE SET NULL,
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    revision INTEGER NOT NULL DEFAULT 1 CHECK (revision > 0),
    updated_by_user_id TEXT REFERENCES users(id) ON DELETE SET NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;

ALTER TABLE model_providers ADD COLUMN slot TEXT;
ALTER TABLE model_providers ADD COLUMN updated_at INTEGER NOT NULL DEFAULT 0;
UPDATE model_providers SET updated_at = created_at WHERE updated_at = 0;
CREATE UNIQUE INDEX model_providers_global_slot
ON model_providers(slot) WHERE owner_user_id IS NULL AND slot IS NOT NULL;

CREATE VIEW source_access AS
SELECT id AS source_instance_id, NULL AS user_id
FROM source_instances
WHERE audience = 'global' AND enabled = 1
UNION
SELECT id, owner_user_id
FROM source_instances
WHERE audience = 'owner' AND enabled = 1 AND owner_user_id IS NOT NULL
UNION
SELECT si.id, ss.user_id
FROM source_instances si
JOIN source_subscriptions ss ON ss.source_instance_id = si.id
WHERE si.audience = 'subscribers' AND si.enabled = 1 AND ss.enabled = 1;

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

INSERT INTO source_definitions(id, kind, display_name, built_in)
VALUES ('builtin:telegram', 'telegram', 'Telegram public channel', 1)
ON CONFLICT(kind) DO UPDATE SET display_name=excluded.display_name;

INSERT INTO source_instances(
    id, definition_id, owner_user_id, name, visibility, visibility_scope,
    config_json, enabled, audience
)
SELECT
    'telegram:' || lower(trim(tc.username, '@')),
    (SELECT id FROM source_definitions WHERE kind='telegram'),
    NULL,
    coalesce(nullif(tc.title, ''), '@' || lower(trim(tc.username, '@'))),
    'public',
    'public',
    json_object(
        'username', lower(trim(tc.username, '@')),
        'pollIntervalSeconds', 300,
        'enabled', json('true')
    ),
    1,
    'subscribers'
FROM telegram_channels tc
WHERE tc.selected = 1 AND tc.username IS NOT NULL AND trim(tc.username, '@') != ''
GROUP BY lower(trim(tc.username, '@'))
ON CONFLICT(id) DO NOTHING;

INSERT INTO public_telegram_channels(source_instance_id, username, title)
SELECT
    'telegram:' || lower(trim(tc.username, '@')),
    lower(trim(tc.username, '@')),
    max(tc.title)
FROM telegram_channels tc
WHERE tc.selected = 1 AND tc.username IS NOT NULL AND trim(tc.username, '@') != ''
GROUP BY lower(trim(tc.username, '@'))
ON CONFLICT(username) DO NOTHING;

INSERT INTO source_subscriptions(user_id, source_instance_id, enabled)
SELECT DISTINCT
    ta.owner_user_id,
    'telegram:' || lower(trim(tc.username, '@')),
    1
FROM telegram_channels tc
JOIN telegram_accounts ta ON ta.id = tc.account_id
WHERE tc.selected = 1 AND tc.username IS NOT NULL AND trim(tc.username, '@') != ''
ON CONFLICT(user_id, source_instance_id) DO UPDATE SET enabled=1;

DELETE FROM document_curators
WHERE source_instance_id IN (
    SELECT si.id FROM source_instances si
    JOIN source_definitions sd ON sd.id=si.definition_id
    WHERE sd.kind='telegram' AND si.audience='owner'
);
DELETE FROM documents
WHERE NOT EXISTS (SELECT 1 FROM document_curators dc WHERE dc.document_id=documents.id);
DELETE FROM stories
WHERE NOT EXISTS (SELECT 1 FROM story_memberships sm WHERE sm.story_id=stories.id);
DELETE FROM source_instances
WHERE id IN (
    SELECT si.id FROM source_instances si
    JOIN source_definitions sd ON sd.id=si.definition_id
    WHERE sd.kind='telegram' AND si.audience='owner'
);

DELETE FROM jobs WHERE kind='ProcessTelegramUpdate';
DELETE FROM jobs
WHERE kind='PollSource'
  AND json_extract(payload_json, '$.sourceId') NOT IN (SELECT id FROM source_instances);

DELETE FROM encrypted_secrets
WHERE id IN (SELECT session_secret_id FROM telegram_accounts);

DROP TABLE telegram_channels;
DROP TABLE telegram_accounts;
