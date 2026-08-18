CREATE TABLE plugin_components (
    plugin_installation_id TEXT PRIMARY KEY REFERENCES plugin_installations(id) ON DELETE CASCADE,
    component_bytes BLOB NOT NULL CHECK (length(component_bytes) > 0)
) STRICT;

CREATE TABLE plugin_health (
    plugin_installation_id TEXT PRIMARY KEY REFERENCES plugin_installations(id) ON DELETE CASCADE,
    last_success_at INTEGER,
    consecutive_failures INTEGER NOT NULL DEFAULT 0,
    last_error_message TEXT,
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;
