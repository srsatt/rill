use std::{
    fs,
    ops::{Deref, DerefMut},
    path::Path,
    sync::{
        Arc, Mutex,
        mpsc::{self, Receiver, SyncSender},
    },
    time::Duration,
};

use rusqlite::{Connection, OptionalExtension, params};
use sha2::{Digest, Sha256};
use thiserror::Error;

const MIGRATIONS: &[(i64, &str)] = &[
    (1, include_str!("../../../migrations/0001_initial.sql")),
    (
        2,
        include_str!("../../../migrations/0002_collection_context.sql"),
    ),
    (3, include_str!("../../../migrations/0003_intelligence.sql")),
    (4, include_str!("../../../migrations/0004_telegram.sql")),
    (5, include_str!("../../../migrations/0005_plugins.sql")),
    (
        6,
        include_str!("../../../migrations/0006_collection_controls.sql"),
    ),
    (
        7,
        include_str!("../../../migrations/0007_source_item_state.sql"),
    ),
    (
        8,
        include_str!("../../../migrations/0008_telegram_web_and_settings.sql"),
    ),
    (
        9,
        include_str!("../../../migrations/0009_subscriber_source_isolation.sql"),
    ),
    (
        10,
        include_str!("../../../migrations/0010_document_topics.sql"),
    ),
    (
        11,
        include_str!("../../../migrations/0011_user_product_state.sql"),
    ),
    (
        12,
        include_str!("../../../migrations/0012_preference_models.sql"),
    ),
    (
        13,
        include_str!("../../../migrations/0013_document_links.sql"),
    ),
    (
        14,
        include_str!("../../../migrations/0014_user_preferences_and_cluster_repair.sql"),
    ),
    (
        15,
        include_str!("../../../migrations/0015_typography_and_stream_order.sql"),
    ),
    (
        16,
        include_str!("../../../migrations/0016_source_processing.sql"),
    ),
    (
        17,
        include_str!("../../../migrations/0017_remove_home_stream.sql"),
    ),
];

#[derive(Debug, Error)]
pub enum DbError {
    #[error("database pool size must be greater than zero")]
    EmptyPool,
    #[error("could not create database directory {path}: {source}")]
    CreateDirectory {
        path: String,
        source: std::io::Error,
    },
    #[error("SQLite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("database pool is unavailable")]
    PoolUnavailable,
    #[error("migration {version} checksum differs from the applied migration")]
    MigrationChanged { version: i64 },
}

struct PoolInner {
    sender: SyncSender<Connection>,
    receiver: Mutex<Receiver<Connection>>,
}

#[derive(Clone)]
pub struct DbPool {
    inner: Arc<PoolInner>,
}

pub struct PooledConnection {
    connection: Option<Connection>,
    pool: Arc<PoolInner>,
}

impl DbPool {
    pub fn open(path: &Path, size: usize, busy_timeout: Duration) -> Result<Self, DbError> {
        if size == 0 {
            return Err(DbError::EmptyPool);
        }
        if let Some(parent) = path.parent().filter(|path| !path.as_os_str().is_empty()) {
            fs::create_dir_all(parent).map_err(|source| DbError::CreateDirectory {
                path: parent.display().to_string(),
                source,
            })?;
        }
        let mut connections = Vec::with_capacity(size);
        for _ in 0..size {
            let connection = Connection::open(path)?;
            configure(&connection, busy_timeout)?;
            connections.push(connection);
        }
        migrate(&mut connections[0])?;
        Ok(Self::from_connections(connections))
    }

    pub fn open_in_memory() -> Result<Self, DbError> {
        let mut connection = Connection::open_in_memory()?;
        configure(&connection, Duration::from_secs(5))?;
        migrate(&mut connection)?;
        Ok(Self::from_connections(vec![connection]))
    }

    fn from_connections(connections: Vec<Connection>) -> Self {
        let (sender, receiver) = mpsc::sync_channel(connections.len());
        for connection in connections {
            sender.send(connection).expect("new pool receiver exists");
        }
        Self {
            inner: Arc::new(PoolInner {
                sender,
                receiver: Mutex::new(receiver),
            }),
        }
    }

    pub fn connection(&self) -> Result<PooledConnection, DbError> {
        let connection = self
            .inner
            .receiver
            .lock()
            .map_err(|_| DbError::PoolUnavailable)?
            .recv()
            .map_err(|_| DbError::PoolUnavailable)?;
        Ok(PooledConnection {
            connection: Some(connection),
            pool: self.inner.clone(),
        })
    }

    pub fn with_connection<T>(
        &self,
        operation: impl FnOnce(&Connection) -> rusqlite::Result<T>,
    ) -> Result<T, DbError> {
        let connection = self.connection()?;
        Ok(operation(&connection)?)
    }
}

impl Deref for PooledConnection {
    type Target = Connection;
    fn deref(&self) -> &Self::Target {
        self.connection.as_ref().expect("leased connection")
    }
}

impl DerefMut for PooledConnection {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.connection.as_mut().expect("leased connection")
    }
}

impl Drop for PooledConnection {
    fn drop(&mut self) {
        if let Some(connection) = self.connection.take() {
            let _ = self.pool.sender.send(connection);
        }
    }
}

fn configure(connection: &Connection, busy_timeout: Duration) -> rusqlite::Result<()> {
    connection.busy_timeout(busy_timeout)?;
    connection.execute_batch(
        "PRAGMA foreign_keys = ON;\nPRAGMA journal_mode = WAL;\nPRAGMA synchronous = NORMAL;",
    )?;
    Ok(())
}

fn migrate(connection: &mut Connection) -> Result<(), DbError> {
    connection.execute_batch(
        "CREATE TABLE IF NOT EXISTS _rill_migrations (\n\
         version INTEGER PRIMARY KEY, checksum TEXT NOT NULL, applied_at INTEGER NOT NULL\n\
         DEFAULT (unixepoch())\n\
         ) STRICT;",
    )?;
    for (version, sql) in MIGRATIONS {
        let checksum = format!("{:x}", Sha256::digest(sql.as_bytes()));
        let existing: Option<String> = connection
            .query_row(
                "SELECT checksum FROM _rill_migrations WHERE version = ?1",
                [version],
                |row| row.get(0),
            )
            .optional()?;
        match existing {
            Some(existing) if existing != checksum => {
                return Err(DbError::MigrationChanged { version: *version });
            }
            Some(_) => continue,
            None => {}
        }
        let transaction = connection.transaction()?;
        transaction.execute_batch(sql)?;
        transaction.execute(
            "INSERT INTO _rill_migrations(version, checksum) VALUES (?1, ?2)",
            params![version, checksum],
        )?;
        transaction.commit()?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migration_creates_every_required_table_and_fts() {
        let pool = DbPool::open_in_memory().unwrap();
        let connection = pool.connection().unwrap();
        for table in [
            "users",
            "password_credentials",
            "sessions",
            "device_sessions",
            "pairing_codes",
            "audit_events",
            "source_definitions",
            "source_instances",
            "source_subscriptions",
            "source_cursors",
            "source_health",
            "public_telegram_channels",
            "telegram_bot_bindings",
            "telegram_binding_challenges",
            "telegram_bot_update_claims",
            "instance_settings",
            "encrypted_secrets",
            "raw_items",
            "collection_expansions",
            "collection_entries",
            "collection_overrides",
            "documents",
            "document_curators",
            "stories",
            "story_memberships",
            "media",
            "canonical_urls",
            "document_links",
            "embedding_records",
            "summaries",
            "model_providers",
            "recommendation_runs",
            "recommendation_scores",
            "preference_models",
            "feedback_events",
            "user_story_state",
            "source_affinity_events",
            "streams",
            "stream_rules",
            "user_stream_state",
            "action_definitions",
            "action_triggers",
            "action_executions",
            "action_attempts",
            "jobs",
            "job_attempts",
            "plugin_installations",
            "plugin_permissions",
            "documents_fts",
        ] {
            let exists: i64 = connection
                .query_row(
                    "SELECT count(*) FROM sqlite_master WHERE name = ?1",
                    [table],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(exists, 1, "missing table {table}");
        }
    }

    #[test]
    fn every_connection_enforces_foreign_keys() {
        let pool = DbPool::open_in_memory().unwrap();
        let enabled: i64 = pool
            .with_connection(|connection| {
                connection.query_row("PRAGMA foreign_keys", [], |row| row.get(0))
            })
            .unwrap();
        assert_eq!(enabled, 1);
    }

    #[test]
    fn migration_removes_home_stream_and_its_loose_references() {
        let connection = Connection::open_in_memory().unwrap();
        connection
            .execute_batch("PRAGMA foreign_keys = ON;")
            .unwrap();
        for (_, migration) in &MIGRATIONS[..16] {
            connection.execute_batch(migration).unwrap();
        }
        connection
            .execute_batch(
                "INSERT INTO users(id, username, role) VALUES ('user', 'reader', 'user');
                 INSERT INTO streams(id, owner_user_id, name, slug)
                   VALUES ('all-stream', 'user', 'All', 'all'),
                          ('home-stream', 'user', 'Home', 'home');
                 INSERT INTO device_sessions(
                   id, user_id, token_hash, csrf_hash, label, selected_stream_id,
                   created_at, expires_at, last_used_at
                 ) VALUES (
                   'device', 'user', zeroblob(32), zeroblob(32), 'Reader', 'home-stream',
                   1, 2, 1
                 );
                 INSERT INTO embedding_records(
                   id, provider, model, model_version, dimension, input_checksum,
                   entity_type, entity_id, vector_f32le
                 ) VALUES (
                   'embedding', 'fixture', 'fixture', '1', 1, x'01',
                   'stream', 'home-stream', zeroblob(4)
                 );
                 INSERT INTO recommendation_runs(id, user_id, stream_id, provider, model)
                   VALUES ('run', 'user', 'home-stream', 'fixture', 'fixture');",
            )
            .unwrap();

        connection.execute_batch(MIGRATIONS[16].1).unwrap();

        let count = |table: &str| {
            connection
                .query_row(&format!("SELECT count(*) FROM {table}"), [], |row| {
                    row.get::<_, i64>(0)
                })
                .unwrap()
        };
        assert_eq!(count("streams"), 1);
        assert_eq!(count("embedding_records"), 0);
        assert_eq!(count("recommendation_runs"), 0);
        let selected: Option<String> = connection
            .query_row(
                "SELECT selected_stream_id FROM device_sessions WHERE id='device'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(selected, None);
    }

    #[test]
    fn subscriber_audience_is_visible_only_to_subscribed_users() {
        let pool = DbPool::open_in_memory().unwrap();
        pool.with_connection(|connection| {
            connection.execute_batch(
                "INSERT INTO users(id, username, role) VALUES
                   ('alice', 'alice', 'user'), ('bob', 'bob', 'user');
                 INSERT INTO source_instances(
                   id, definition_id, name, visibility, visibility_scope, audience
                 ) VALUES ('channel', 'builtin:telegram', '@channel', 'public', 'public', 'subscribers');
                 INSERT INTO source_subscriptions(user_id, source_instance_id)
                   VALUES ('alice', 'channel');
                 INSERT INTO documents(
                   id, visibility_scope, exact_content_hash, title, body_text
                 ) VALUES ('document', 'public', x'01', 'Title', 'Body');
                 INSERT INTO document_curators(
                   document_id, curator_kind, curator_id, source_instance_id
                 ) VALUES ('document', 'telegram', 'channel', 'channel');",
            )
        })
        .unwrap();
        let visible = |user: &str| {
            pool.with_connection(|connection| {
                connection.query_row(
                    "SELECT count(*) FROM document_access
                     WHERE document_id='document' AND user_id=?1",
                    [user],
                    |row| row.get::<_, i64>(0),
                )
            })
            .unwrap()
        };
        assert_eq!(visible("alice"), 1);
        assert_eq!(visible("bob"), 0);
        pool.with_connection(|connection| {
            connection.execute("DELETE FROM source_instances WHERE id='channel'", [])
        })
        .unwrap();
        let globally_visible: i64 = pool
            .with_connection(|connection| {
                connection.query_row(
                    "SELECT count(*) FROM document_access
                     WHERE document_id='document' AND user_id IS NULL",
                    [],
                    |row| row.get(0),
                )
            })
            .unwrap();
        assert_eq!(globally_visible, 0);
    }

    #[test]
    fn excluded_source_document_is_not_accessible() {
        let pool = DbPool::open_in_memory().unwrap();
        pool.with_connection(|connection| {
            connection.execute_batch(
                "INSERT INTO users(id, username, role) VALUES ('alice', 'alice', 'user');
                 INSERT INTO source_instances(
                   id, definition_id, owner_user_id, name, visibility, visibility_scope, audience
                 ) VALUES ('feed', 'builtin:telegram', 'alice', 'Feed', 'private', 'user:alice', 'owner');
                 INSERT INTO source_subscriptions(user_id, source_instance_id) VALUES ('alice', 'feed');
                 INSERT INTO documents(id, visibility_scope, exact_content_hash, title, body_text)
                   VALUES ('document', 'user:alice', x'01', 'Title', 'Body'),
                          ('direct', 'user:alice', x'02', 'Direct', 'Body');
                 INSERT INTO document_curators(
                   document_id, curator_kind, curator_id, source_instance_id, included
                 ) VALUES ('document', 'source', 'feed', 'feed', 0);",
            )
        })
        .unwrap();

        let visible: i64 = pool
            .with_connection(|connection| {
                connection.query_row(
                    "SELECT count(*) FROM document_access WHERE document_id='document'",
                    [],
                    |row| row.get(0),
                )
            })
            .unwrap();
        assert_eq!(visible, 0);
        let direct_visible: i64 = pool
            .with_connection(|connection| {
                connection.query_row(
                    "SELECT count(*) FROM document_access
                     WHERE document_id='direct' AND user_id='alice'",
                    [],
                    |row| row.get(0),
                )
            })
            .unwrap();
        assert_eq!(direct_visible, 1);
    }
}
