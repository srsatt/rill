CREATE TABLE users (
    id TEXT PRIMARY KEY,
    username TEXT NOT NULL COLLATE NOCASE UNIQUE,
    email TEXT COLLATE NOCASE UNIQUE,
    role TEXT NOT NULL CHECK (role IN ('admin', 'user')),
    disabled INTEGER NOT NULL DEFAULT 0 CHECK (disabled IN (0, 1)),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;

CREATE TABLE password_credentials (
    user_id TEXT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    password_hash TEXT NOT NULL,
    changed_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;

CREATE TABLE sessions (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash BLOB NOT NULL UNIQUE CHECK (length(token_hash) = 32),
    csrf_hash BLOB NOT NULL CHECK (length(csrf_hash) = 32),
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    last_used_at INTEGER NOT NULL,
    user_agent TEXT,
    ip_summary TEXT,
    revoked_at INTEGER,
    CHECK (expires_at > created_at)
) STRICT;
CREATE INDEX sessions_user_active ON sessions(user_id, expires_at) WHERE revoked_at IS NULL;

CREATE TABLE device_sessions (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    token_hash BLOB NOT NULL UNIQUE CHECK (length(token_hash) = 32),
    csrf_hash BLOB NOT NULL CHECK (length(csrf_hash) = 32),
    label TEXT NOT NULL,
    scope TEXT NOT NULL DEFAULT 'reader' CHECK (scope = 'reader'),
    selected_stream_id TEXT,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    last_used_at INTEGER NOT NULL,
    user_agent TEXT,
    ip_summary TEXT,
    revoked_at INTEGER,
    CHECK (expires_at > created_at)
) STRICT;
CREATE INDEX device_sessions_user_active ON device_sessions(user_id, expires_at) WHERE revoked_at IS NULL;

CREATE TABLE pairing_codes (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    code_hash BLOB NOT NULL UNIQUE CHECK (length(code_hash) = 32),
    scope TEXT NOT NULL DEFAULT 'reader' CHECK (scope = 'reader'),
    label TEXT NOT NULL,
    created_at INTEGER NOT NULL,
    expires_at INTEGER NOT NULL,
    attempts_remaining INTEGER NOT NULL CHECK (attempts_remaining >= 0),
    consumed_at INTEGER,
    CHECK (expires_at > created_at)
) STRICT;
CREATE INDEX pairing_codes_active ON pairing_codes(expires_at) WHERE consumed_at IS NULL;

CREATE TABLE security_rate_limits (
    key_hash BLOB NOT NULL CHECK (length(key_hash) = 32),
    purpose TEXT NOT NULL,
    window_started_at INTEGER NOT NULL,
    attempts INTEGER NOT NULL CHECK (attempts >= 0),
    PRIMARY KEY (key_hash, purpose)
) STRICT;

CREATE TABLE audit_events (
    id TEXT PRIMARY KEY,
    user_id TEXT REFERENCES users(id) ON DELETE SET NULL,
    actor_session_id TEXT,
    event_type TEXT NOT NULL,
    target_type TEXT,
    target_id TEXT,
    detail_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(detail_json)),
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;
CREATE INDEX audit_events_user_time ON audit_events(user_id, created_at DESC);

CREATE TABLE encrypted_secrets (
    id TEXT PRIMARY KEY,
    owner_user_id TEXT REFERENCES users(id) ON DELETE CASCADE,
    key_version INTEGER NOT NULL,
    nonce BLOB NOT NULL,
    ciphertext BLOB NOT NULL,
    purpose TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;

CREATE TABLE source_definitions (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL UNIQUE,
    display_name TEXT NOT NULL,
    built_in INTEGER NOT NULL DEFAULT 1 CHECK (built_in IN (0, 1)),
    config_schema_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(config_schema_json)),
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;

CREATE TABLE source_instances (
    id TEXT PRIMARY KEY,
    definition_id TEXT NOT NULL REFERENCES source_definitions(id),
    owner_user_id TEXT REFERENCES users(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    visibility TEXT NOT NULL CHECK (visibility IN ('public', 'private')),
    visibility_scope TEXT NOT NULL,
    config_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(config_json)),
    credential_secret_id TEXT REFERENCES encrypted_secrets(id) ON DELETE SET NULL,
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;
CREATE INDEX source_instances_owner ON source_instances(owner_user_id);

CREATE TABLE source_subscriptions (
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    source_instance_id TEXT NOT NULL REFERENCES source_instances(id) ON DELETE CASCADE,
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (user_id, source_instance_id)
) STRICT;

CREATE TABLE source_cursors (
    source_instance_id TEXT NOT NULL REFERENCES source_instances(id) ON DELETE CASCADE,
    cursor_kind TEXT NOT NULL,
    cursor_value TEXT NOT NULL,
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (source_instance_id, cursor_kind)
) STRICT;

CREATE TABLE source_health (
    source_instance_id TEXT PRIMARY KEY REFERENCES source_instances(id) ON DELETE CASCADE,
    last_attempt_at INTEGER,
    last_success_at INTEGER,
    consecutive_failures INTEGER NOT NULL DEFAULT 0,
    last_error_class TEXT,
    last_error_message TEXT,
    next_poll_at INTEGER
) STRICT;

CREATE TABLE raw_items (
    id TEXT PRIMARY KEY,
    source_instance_id TEXT NOT NULL REFERENCES source_instances(id) ON DELETE CASCADE,
    external_id TEXT NOT NULL,
    visibility_scope TEXT NOT NULL,
    item_kind TEXT NOT NULL,
    title TEXT,
    body_text TEXT,
    body_html TEXT,
    author TEXT,
    source_url TEXT,
    content_hash BLOB NOT NULL,
    published_at INTEGER,
    fetched_at INTEGER NOT NULL DEFAULT (unixepoch()),
    metadata_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(metadata_json)),
    UNIQUE (source_instance_id, external_id)
) STRICT;
CREATE INDEX raw_items_visibility_time ON raw_items(visibility_scope, published_at DESC);

CREATE TABLE collection_expansions (
    id TEXT PRIMARY KEY,
    parent_raw_item_id TEXT NOT NULL REFERENCES raw_items(id) ON DELETE CASCADE,
    parser_kind TEXT NOT NULL,
    parser_version TEXT NOT NULL,
    confidence REAL NOT NULL CHECK (confidence >= 0 AND confidence <= 1),
    status TEXT NOT NULL CHECK (status IN ('detected', 'expanded', 'rejected', 'failed')),
    diagnostics_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(diagnostics_json)),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE (parent_raw_item_id, parser_kind, parser_version)
) STRICT;

CREATE TABLE collection_entries (
    id TEXT PRIMARY KEY,
    expansion_id TEXT NOT NULL REFERENCES collection_expansions(id) ON DELETE CASCADE,
    parent_raw_item_id TEXT NOT NULL REFERENCES raw_items(id) ON DELETE CASCADE,
    ordinal INTEGER NOT NULL CHECK (ordinal >= 0),
    target_url TEXT NOT NULL,
    title_hint TEXT,
    summary_hint TEXT,
    author_hint TEXT,
    deterministic_key TEXT NOT NULL,
    derived_raw_item_id TEXT REFERENCES raw_items(id) ON DELETE SET NULL,
    UNIQUE (expansion_id, deterministic_key),
    UNIQUE (expansion_id, ordinal)
) STRICT;

CREATE TABLE documents (
    id TEXT PRIMARY KEY,
    visibility_scope TEXT NOT NULL,
    exact_content_hash BLOB NOT NULL,
    title TEXT NOT NULL,
    body_text TEXT NOT NULL,
    sanitized_html TEXT,
    author TEXT,
    publisher TEXT,
    canonical_url TEXT,
    language TEXT,
    published_at INTEGER,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE (visibility_scope, exact_content_hash)
) STRICT;
CREATE INDEX documents_visibility_time ON documents(visibility_scope, published_at DESC);

CREATE VIRTUAL TABLE documents_fts USING fts5(
    document_id UNINDEXED, title, body_text, author,
    tokenize = 'unicode61 remove_diacritics 2'
);
CREATE TRIGGER documents_fts_insert AFTER INSERT ON documents BEGIN
    INSERT INTO documents_fts(document_id, title, body_text, author)
    VALUES (new.id, new.title, new.body_text, coalesce(new.author, ''));
END;
CREATE TRIGGER documents_fts_update AFTER UPDATE OF title, body_text, author ON documents BEGIN
    DELETE FROM documents_fts WHERE document_id = old.id;
    INSERT INTO documents_fts(document_id, title, body_text, author)
    VALUES (new.id, new.title, new.body_text, coalesce(new.author, ''));
END;
CREATE TRIGGER documents_fts_delete AFTER DELETE ON documents BEGIN
    DELETE FROM documents_fts WHERE document_id = old.id;
END;

CREATE TABLE document_curators (
    document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    curator_kind TEXT NOT NULL,
    curator_id TEXT NOT NULL,
    source_instance_id TEXT REFERENCES source_instances(id) ON DELETE SET NULL,
    raw_item_id TEXT REFERENCES raw_items(id) ON DELETE SET NULL,
    collection_entry_id TEXT REFERENCES collection_entries(id) ON DELETE SET NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (document_id, curator_kind, curator_id)
) STRICT;
CREATE INDEX document_curators_curator ON document_curators(curator_kind, curator_id);

CREATE TABLE stories (
    id TEXT PRIMARY KEY,
    visibility_scope TEXT NOT NULL,
    cluster_version TEXT NOT NULL,
    anchor_document_id TEXT REFERENCES documents(id) ON DELETE SET NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;

CREATE TABLE story_memberships (
    story_id TEXT NOT NULL REFERENCES stories(id) ON DELETE CASCADE,
    document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    similarity REAL,
    added_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (story_id, document_id)
) STRICT;

CREATE TABLE media (
    id TEXT PRIMARY KEY,
    document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    media_kind TEXT NOT NULL,
    url TEXT NOT NULL,
    mime_type TEXT,
    width INTEGER,
    height INTEGER,
    alt_text TEXT,
    ordinal INTEGER NOT NULL DEFAULT 0
) STRICT;

CREATE TABLE canonical_urls (
    normalized_url TEXT NOT NULL,
    visibility_scope TEXT NOT NULL,
    document_id TEXT NOT NULL REFERENCES documents(id) ON DELETE CASCADE,
    original_url TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (normalized_url, visibility_scope)
) STRICT;

CREATE TABLE model_providers (
    id TEXT PRIMARY KEY,
    owner_user_id TEXT REFERENCES users(id) ON DELETE CASCADE,
    provider_kind TEXT NOT NULL,
    name TEXT NOT NULL,
    config_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(config_json)),
    credential_secret_id TEXT REFERENCES encrypted_secrets(id) ON DELETE SET NULL,
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;

CREATE TABLE embedding_records (
    id TEXT PRIMARY KEY,
    provider_id TEXT REFERENCES model_providers(id) ON DELETE SET NULL,
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    model_version TEXT NOT NULL,
    dimension INTEGER NOT NULL CHECK (dimension > 0),
    input_checksum BLOB NOT NULL,
    entity_type TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    vector_f32le BLOB NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    CHECK (length(vector_f32le) = dimension * 4),
    UNIQUE (provider, model, model_version, input_checksum, entity_type, entity_id)
) STRICT;

CREATE TABLE summaries (
    id TEXT PRIMARY KEY,
    entity_type TEXT NOT NULL,
    entity_id TEXT NOT NULL,
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    model_version TEXT NOT NULL,
    input_checksum BLOB NOT NULL,
    summary_text TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE (entity_type, entity_id, provider, model, model_version, input_checksum)
) STRICT;

CREATE TABLE recommendation_runs (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    stream_id TEXT,
    provider TEXT NOT NULL,
    model TEXT NOT NULL,
    config_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(config_json)),
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;

CREATE TABLE recommendation_scores (
    run_id TEXT NOT NULL REFERENCES recommendation_runs(id) ON DELETE CASCADE,
    story_id TEXT NOT NULL REFERENCES stories(id) ON DELETE CASCADE,
    document_id TEXT REFERENCES documents(id) ON DELETE SET NULL,
    score REAL NOT NULL,
    rank INTEGER NOT NULL,
    explanation_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(explanation_json)),
    PRIMARY KEY (run_id, story_id)
) STRICT;

CREATE TABLE feedback_events (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    story_id TEXT NOT NULL REFERENCES stories(id) ON DELETE CASCADE,
    document_id TEXT REFERENCES documents(id) ON DELETE SET NULL,
    feedback TEXT NOT NULL CHECK (feedback IN ('like', 'dislike', 'favorite', 'none')),
    source TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;
CREATE INDEX feedback_events_user_story ON feedback_events(user_id, story_id, created_at DESC);

CREATE TABLE user_story_state (
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    story_id TEXT NOT NULL REFERENCES stories(id) ON DELETE CASCADE,
    selected_document_id TEXT REFERENCES documents(id) ON DELETE SET NULL,
    read_at INTEGER,
    hidden_at INTEGER,
    favorite INTEGER NOT NULL DEFAULT 0 CHECK (favorite IN (0, 1)),
    explicit_feedback TEXT CHECK (explicit_feedback IN ('like', 'dislike')),
    last_impression_at INTEGER,
    reader_progress REAL CHECK (reader_progress BETWEEN 0 AND 1),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (user_id, story_id)
) STRICT;

CREATE TABLE source_affinity_events (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    subject_kind TEXT NOT NULL CHECK (subject_kind IN ('source', 'curator', 'publisher')),
    subject_id TEXT NOT NULL,
    signal TEXT NOT NULL,
    weight REAL NOT NULL,
    story_id TEXT REFERENCES stories(id) ON DELETE SET NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;
CREATE INDEX source_affinity_user_subject ON source_affinity_events(user_id, subject_kind, subject_id);

CREATE TABLE streams (
    id TEXT PRIMARY KEY,
    owner_user_id TEXT REFERENCES users(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    slug TEXT NOT NULL,
    definition_text TEXT,
    ranking_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(ranking_json)),
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE (owner_user_id, slug)
) STRICT;

CREATE TABLE stream_rules (
    id TEXT PRIMARY KEY,
    stream_id TEXT NOT NULL REFERENCES streams(id) ON DELETE CASCADE,
    rule_kind TEXT NOT NULL,
    rule_json TEXT NOT NULL CHECK (json_valid(rule_json)),
    ordinal INTEGER NOT NULL,
    UNIQUE (stream_id, ordinal)
) STRICT;

CREATE TABLE user_stream_state (
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    stream_id TEXT NOT NULL REFERENCES streams(id) ON DELETE CASCADE,
    last_opened_at INTEGER,
    cursor_json TEXT CHECK (cursor_json IS NULL OR json_valid(cursor_json)),
    PRIMARY KEY (user_id, stream_id)
) STRICT;

CREATE TABLE action_definitions (
    id TEXT PRIMARY KEY,
    owner_user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    action_kind TEXT NOT NULL,
    config_json TEXT NOT NULL CHECK (json_valid(config_json)),
    credential_secret_id TEXT REFERENCES encrypted_secrets(id) ON DELETE SET NULL,
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    created_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;

CREATE TABLE action_triggers (
    id TEXT PRIMARY KEY,
    action_definition_id TEXT NOT NULL REFERENCES action_definitions(id) ON DELETE CASCADE,
    event_kind TEXT NOT NULL,
    filter_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(filter_json)),
    enabled INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1))
) STRICT;

CREATE TABLE action_executions (
    id TEXT PRIMARY KEY,
    action_definition_id TEXT NOT NULL REFERENCES action_definitions(id) ON DELETE CASCADE,
    trigger_id TEXT REFERENCES action_triggers(id) ON DELETE SET NULL,
    user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    event_id TEXT NOT NULL,
    idempotency_key TEXT NOT NULL UNIQUE,
    status TEXT NOT NULL,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    completed_at INTEGER
) STRICT;

CREATE TABLE action_attempts (
    id TEXT PRIMARY KEY,
    execution_id TEXT NOT NULL REFERENCES action_executions(id) ON DELETE CASCADE,
    attempt_number INTEGER NOT NULL,
    status TEXT NOT NULL,
    response_class TEXT,
    error_message TEXT,
    started_at INTEGER NOT NULL,
    finished_at INTEGER,
    UNIQUE (execution_id, attempt_number)
) STRICT;

CREATE TABLE jobs (
    id TEXT PRIMARY KEY,
    kind TEXT NOT NULL,
    payload_json TEXT NOT NULL CHECK (json_valid(payload_json)),
    visibility_scope TEXT,
    priority INTEGER NOT NULL DEFAULT 0,
    status TEXT NOT NULL CHECK (status IN ('queued', 'leased', 'succeeded', 'dead')),
    available_at INTEGER NOT NULL,
    lease_owner TEXT,
    lease_expires_at INTEGER,
    attempt_count INTEGER NOT NULL DEFAULT 0,
    max_attempts INTEGER NOT NULL DEFAULT 5 CHECK (max_attempts > 0),
    idempotency_key TEXT UNIQUE,
    last_error_class TEXT,
    last_error_message TEXT,
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    completed_at INTEGER,
    CHECK ((status = 'leased') = (lease_owner IS NOT NULL AND lease_expires_at IS NOT NULL))
) STRICT;
CREATE INDEX jobs_claim ON jobs(status, available_at, priority DESC, created_at);

CREATE TABLE job_attempts (
    id TEXT PRIMARY KEY,
    job_id TEXT NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
    attempt_number INTEGER NOT NULL,
    worker_id TEXT NOT NULL,
    started_at INTEGER NOT NULL,
    finished_at INTEGER,
    outcome TEXT,
    error_class TEXT,
    error_message TEXT,
    UNIQUE (job_id, attempt_number)
) STRICT;

CREATE TABLE plugin_installations (
    id TEXT PRIMARY KEY,
    name TEXT NOT NULL,
    version TEXT NOT NULL,
    wasm_sha256 TEXT NOT NULL,
    manifest_json TEXT NOT NULL CHECK (json_valid(manifest_json)),
    enabled INTEGER NOT NULL DEFAULT 0 CHECK (enabled IN (0, 1)),
    installed_at INTEGER NOT NULL DEFAULT (unixepoch()),
    UNIQUE (name, version)
) STRICT;

CREATE TABLE plugin_permissions (
    plugin_installation_id TEXT NOT NULL REFERENCES plugin_installations(id) ON DELETE CASCADE,
    capability TEXT NOT NULL,
    constraint_json TEXT NOT NULL DEFAULT '{}' CHECK (json_valid(constraint_json)),
    granted_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (plugin_installation_id, capability)
) STRICT;
