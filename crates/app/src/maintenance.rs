use rill_db::DbPool;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct RecommendationMaintenancePayload {
    pub user_id: Option<String>,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct ReembedContentPayload {
    pub document_id: Option<String>,
}

#[derive(Clone)]
pub struct MaintenanceService {
    pool: DbPool,
}

impl MaintenanceService {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }

    pub fn cleanup_sessions(&self) -> Result<usize, rill_db::DbError> {
        self.pool.with_connection(|connection| {
            let transaction = connection.unchecked_transaction()?;
            let mut changed = transaction.execute(
                "DELETE FROM sessions WHERE expires_at <= unixepoch()
                     OR revoked_at <= unixepoch() - 2592000",
                [],
            )?;
            changed += transaction.execute(
                "DELETE FROM device_sessions WHERE expires_at <= unixepoch()
                     OR revoked_at <= unixepoch() - 2592000",
                [],
            )?;
            changed += transaction.execute(
                "DELETE FROM security_rate_limits
                     WHERE window_started_at <= unixepoch() - 86400",
                [],
            )?;
            transaction.commit()?;
            Ok(changed)
        })
    }

    pub fn cleanup_pairing_codes(&self) -> Result<usize, rill_db::DbError> {
        self.pool.with_connection(|connection| {
            connection.execute(
                "DELETE FROM pairing_codes WHERE expires_at <= unixepoch()
                     OR consumed_at <= unixepoch() - 86400",
                [],
            )
        })
    }

    pub fn database_maintenance(&self) -> Result<(), rill_db::DbError> {
        self.pool.with_connection(|connection| {
            let transaction = connection.unchecked_transaction()?;
            transaction.execute(
                "DELETE FROM recommendation_runs WHERE created_at <= unixepoch() - 86400",
                [],
            )?;
            transaction.execute(
                "DELETE FROM jobs WHERE status='succeeded'
                     AND completed_at <= unixepoch() - 604800",
                [],
            )?;
            transaction.commit()?;
            connection.execute_batch("PRAGMA optimize;")?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use rill_db::DbPool;
    use uuid::Uuid;

    use super::MaintenanceService;

    #[test]
    fn cleanup_removes_expired_sessions_and_pairing_codes() {
        let pool = DbPool::open_in_memory().unwrap();
        let user_id = Uuid::new_v4().to_string();
        pool.with_connection(|connection| {
            connection.execute(
                "INSERT INTO users(id, username, role) VALUES (?1, 'maint', 'user')",
                [&user_id],
            )?;
            connection.execute(
                "INSERT INTO sessions(id, user_id, token_hash, csrf_hash, created_at, expires_at,
                 last_used_at) VALUES ('session', ?1, zeroblob(32), zeroblob(32), 1, 2, 1)",
                [&user_id],
            )?;
            connection.execute(
                "INSERT INTO pairing_codes(id, user_id, code_hash, label, created_at, expires_at,
                 attempts_remaining) VALUES ('pair', ?1, randomblob(32), 'reader', 1, 2, 1)",
                [&user_id],
            )?;
            Ok(())
        })
        .unwrap();
        let maintenance = MaintenanceService::new(pool.clone());
        assert_eq!(maintenance.cleanup_sessions().unwrap(), 1);
        assert_eq!(maintenance.cleanup_pairing_codes().unwrap(), 1);
    }
}
