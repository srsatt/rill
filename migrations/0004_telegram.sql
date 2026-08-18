CREATE TABLE telegram_accounts (
    id TEXT PRIMARY KEY,
    owner_user_id TEXT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    phone_hint TEXT NOT NULL,
    session_secret_id TEXT NOT NULL REFERENCES encrypted_secrets(id) ON DELETE CASCADE,
    status TEXT NOT NULL CHECK (status IN ('active', 'revoked', 'error')),
    created_at INTEGER NOT NULL DEFAULT (unixepoch()),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch())
) STRICT;
CREATE INDEX telegram_accounts_owner ON telegram_accounts(owner_user_id);

CREATE TABLE telegram_channels (
    account_id TEXT NOT NULL REFERENCES telegram_accounts(id) ON DELETE CASCADE,
    peer_id TEXT NOT NULL,
    title TEXT NOT NULL,
    username TEXT,
    selected INTEGER NOT NULL DEFAULT 0 CHECK (selected IN (0, 1)),
    updated_at INTEGER NOT NULL DEFAULT (unixepoch()),
    PRIMARY KEY (account_id, peer_id)
) STRICT;

