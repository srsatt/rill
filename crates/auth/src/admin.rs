use rill_domain::{Role, User};
use rusqlite::{OptionalExtension, params};
use serde::Serialize;
use serde_json::Value;

use super::{AuthError, AuthService, audit, now, user_from_row};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AdminUserView {
    #[serde(flatten)]
    pub user: User,
    pub created_at: i64,
    pub active_browser_sessions: u32,
    pub active_reader_devices: u32,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BrowserSessionView {
    pub id: String,
    pub user_id: String,
    pub created_at: i64,
    pub expires_at: i64,
    pub last_used_at: i64,
    pub user_agent: Option<String>,
    pub ip_summary: Option<String>,
    pub revoked_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AuditEventView {
    pub id: String,
    pub user_id: Option<String>,
    pub actor_session_id: Option<String>,
    pub event_type: String,
    pub target_type: Option<String>,
    pub target_id: Option<String>,
    pub detail: Value,
    pub created_at: i64,
}

impl AuthService {
    pub fn list_users(&self) -> Result<Vec<AdminUserView>, AuthError> {
        let connection = self.pool.connection()?;
        let mut statement = connection.prepare(
            "SELECT u.id, u.username, u.email, u.role, u.disabled, u.created_at,
             (SELECT count(*) FROM sessions s WHERE s.user_id=u.id AND s.revoked_at IS NULL
                AND s.expires_at > unixepoch()),
             (SELECT count(*) FROM device_sessions d WHERE d.user_id=u.id AND d.revoked_at IS NULL
                AND d.expires_at > unixepoch())
             FROM users u ORDER BY u.username COLLATE NOCASE",
        )?;
        let rows = statement.query_map([], |row| {
            Ok(AdminUserView {
                user: user_from_row(row)?,
                created_at: row.get(5)?,
                active_browser_sessions: row.get(6)?,
                active_reader_devices: row.get(7)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn set_user_disabled(
        &self,
        actor_user_id: &str,
        actor_session_id: &str,
        target_user_id: &str,
        disabled: bool,
    ) -> Result<bool, AuthError> {
        if disabled && actor_user_id == target_user_id {
            return Err(AuthError::InvalidInput);
        }
        let timestamp = now()?;
        let mut connection = self.pool.connection()?;
        let transaction = connection.transaction()?;
        if disabled && is_last_enabled_admin(&transaction, target_user_id)? {
            return Err(AuthError::InvalidInput);
        }
        let changed = transaction.execute(
            "UPDATE users SET disabled=?2, updated_at=?3 WHERE id=?1 AND disabled<>?2",
            params![target_user_id, disabled, timestamp],
        )? == 1;
        if changed {
            if disabled {
                transaction.execute(
                    "UPDATE sessions SET revoked_at=?2 WHERE user_id=?1 AND revoked_at IS NULL",
                    params![target_user_id, timestamp],
                )?;
                transaction.execute(
                    "UPDATE device_sessions SET revoked_at=?2 WHERE user_id=?1 AND revoked_at IS NULL",
                    params![target_user_id, timestamp],
                )?;
            }
            audit(
                &transaction,
                Some(actor_user_id),
                Some(actor_session_id),
                if disabled {
                    "user.disabled"
                } else {
                    "user.enabled"
                },
                "user",
                Some(target_user_id),
            )?;
        }
        transaction.commit()?;
        Ok(changed)
    }

    pub fn set_user_role(
        &self,
        actor_user_id: &str,
        actor_session_id: &str,
        target_user_id: &str,
        role: Role,
    ) -> Result<bool, AuthError> {
        let mut connection = self.pool.connection()?;
        let transaction = connection.transaction()?;
        if role != Role::Admin && is_last_enabled_admin(&transaction, target_user_id)? {
            return Err(AuthError::InvalidInput);
        }
        let changed = transaction.execute(
            "UPDATE users SET role=?2, updated_at=unixepoch() WHERE id=?1 AND role<>?2",
            params![target_user_id, role.as_str()],
        )? == 1;
        if changed {
            transaction.execute(
                "UPDATE sessions SET revoked_at=unixepoch() WHERE user_id=?1 AND id<>?2
                 AND revoked_at IS NULL",
                params![target_user_id, actor_session_id],
            )?;
            audit(
                &transaction,
                Some(actor_user_id),
                Some(actor_session_id),
                "user.role_changed",
                "user",
                Some(target_user_id),
            )?;
        }
        transaction.commit()?;
        Ok(changed)
    }

    pub fn list_browser_sessions(
        &self,
        user_id: Option<&str>,
        limit: usize,
    ) -> Result<Vec<BrowserSessionView>, AuthError> {
        let connection = self.pool.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, user_id, created_at, expires_at, last_used_at, user_agent, ip_summary,
             revoked_at FROM sessions WHERE (?1 IS NULL OR user_id=?1)
             ORDER BY created_at DESC LIMIT ?2",
        )?;
        let sql_limit = i64::try_from(limit.min(500)).unwrap_or(500);
        let rows = statement.query_map(params![user_id, sql_limit], |row| {
            Ok(BrowserSessionView {
                id: row.get(0)?,
                user_id: row.get(1)?,
                created_at: row.get(2)?,
                expires_at: row.get(3)?,
                last_used_at: row.get(4)?,
                user_agent: row.get(5)?,
                ip_summary: row.get(6)?,
                revoked_at: row.get(7)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn admin_revoke_session(
        &self,
        actor_user_id: &str,
        actor_session_id: &str,
        session_id: &str,
    ) -> Result<bool, AuthError> {
        if actor_session_id == session_id {
            return Err(AuthError::InvalidInput);
        }
        let mut connection = self.pool.connection()?;
        let transaction = connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE sessions SET revoked_at=unixepoch() WHERE id=?1 AND revoked_at IS NULL",
            [session_id],
        )? == 1;
        if changed {
            audit(
                &transaction,
                Some(actor_user_id),
                Some(actor_session_id),
                "session.admin_revoked",
                "session",
                Some(session_id),
            )?;
        }
        transaction.commit()?;
        Ok(changed)
    }

    pub fn list_audit_events(&self, limit: usize) -> Result<Vec<AuditEventView>, AuthError> {
        let connection = self.pool.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, user_id, actor_session_id, event_type, target_type, target_id,
             detail_json, created_at FROM audit_events ORDER BY created_at DESC LIMIT ?1",
        )?;
        let sql_limit = i64::try_from(limit.min(1_000)).unwrap_or(1_000);
        let rows = statement.query_map([sql_limit], |row| {
            let detail_text: String = row.get(6)?;
            let detail = serde_json::from_str(&detail_text).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    6,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?;
            Ok(AuditEventView {
                id: row.get(0)?,
                user_id: row.get(1)?,
                actor_session_id: row.get(2)?,
                event_type: row.get(3)?,
                target_type: row.get(4)?,
                target_id: row.get(5)?,
                detail,
                created_at: row.get(7)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }
}

fn is_last_enabled_admin(
    transaction: &rusqlite::Transaction<'_>,
    target_user_id: &str,
) -> rusqlite::Result<bool> {
    let target_is_admin = transaction
        .query_row(
            "SELECT role='admin' AND disabled=0 FROM users WHERE id=?1",
            [target_user_id],
            |row| row.get::<_, bool>(0),
        )
        .optional()?
        .unwrap_or(false);
    if !target_is_admin {
        return Ok(false);
    }
    let enabled_admins: i64 = transaction.query_row(
        "SELECT count(*) FROM users WHERE role='admin' AND disabled=0",
        [],
        |row| row.get(0),
    )?;
    Ok(enabled_admins <= 1)
}

#[cfg(test)]
mod tests {
    use rill_db::DbPool;

    use super::*;

    fn service() -> AuthService {
        AuthService::new(DbPool::open_in_memory().unwrap(), 30, 180, 10, 3)
    }

    #[test]
    fn last_enabled_admin_cannot_be_disabled_or_demoted() {
        let service = service();
        let admin = service
            .create_user("admin", None, "correct horse battery", Role::Admin)
            .unwrap();
        assert!(matches!(
            service.set_user_disabled(&admin.id, "session", &admin.id, true),
            Err(AuthError::InvalidInput)
        ));
        assert!(matches!(
            service.set_user_role(&admin.id, "session", &admin.id, Role::User),
            Err(AuthError::InvalidInput)
        ));
    }

    #[test]
    fn disabling_user_revokes_browser_and_reader_sessions() {
        let service = service();
        let admin = service
            .create_user("admin", None, "correct horse battery", Role::Admin)
            .unwrap();
        let user = service
            .create_user("reader", None, "another correct password", Role::User)
            .unwrap();
        let browser = service
            .authenticate("reader", "another correct password", None, None)
            .unwrap();
        let pairing = service.create_pairing_code(&user.id, "Kobo").unwrap();
        service
            .consume_pairing_code(pairing.code.expose(), "Kobo", None, None)
            .unwrap();

        assert!(
            service
                .set_user_disabled(&admin.id, "admin-session", &user.id, true)
                .unwrap()
        );
        assert!(matches!(
            service.browser_principal(browser.token.expose()),
            Err(AuthError::InvalidSession)
        ));
        let view = service
            .list_users()
            .unwrap()
            .into_iter()
            .find(|view| view.user.id == user.id)
            .unwrap();
        assert!(view.user.disabled);
        assert_eq!(view.active_browser_sessions, 0);
        assert_eq!(view.active_reader_devices, 0);
    }
}
