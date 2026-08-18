impl AuthService {
    pub fn new(
        pool: DbPool,
        session_days: u64,
        reader_session_days: u64,
        pairing_minutes: u64,
        pairing_max_attempts: u32,
    ) -> Self {
        Self {
            pool,
            session_seconds: days_to_seconds(session_days),
            reader_session_seconds: days_to_seconds(reader_session_days),
            pairing_seconds: minutes_to_seconds(pairing_minutes),
            pairing_max_attempts,
        }
    }

    pub fn create_user(
        &self,
        username: &str,
        email: Option<&str>,
        password: &str,
        role: Role,
    ) -> Result<User, AuthError> {
        let username = normalize_username(username)?;
        let email = normalize_email(email)?;
        validate_password(password)?;
        let password_hash = hash_password(password)?;
        let user = User {
            id: Uuid::new_v4().to_string(),
            username,
            email,
            role,
            disabled: false,
        };
        let mut connection = self.pool.connection()?;
        let transaction = connection.transaction()?;
        let inserted = transaction.execute(
            "INSERT INTO users(id, username, email, role) VALUES (?1, ?2, ?3, ?4)",
            params![user.id, user.username, user.email, role.as_str()],
        );
        if let Err(error) = inserted {
            if error.sqlite_error_code() == Some(rusqlite::ErrorCode::ConstraintViolation) {
                return Err(AuthError::Conflict);
            }
            return Err(error.into());
        }
        transaction.execute(
            "INSERT INTO password_credentials(user_id, password_hash) VALUES (?1, ?2)",
            params![user.id, password_hash],
        )?;
        audit(
            &transaction,
            Some(&user.id),
            None,
            "user.created",
            "user",
            Some(&user.id),
        )?;
        transaction.commit()?;
        Ok(user)
    }

    pub fn find_user(&self, identity: &str) -> Result<Option<User>, AuthError> {
        let connection = self.pool.connection()?;
        Ok(connection
            .query_row(
                "SELECT id, username, email, role, disabled FROM users\n\
             WHERE id = ?1 OR username = ?1 COLLATE NOCASE OR email = ?1 COLLATE NOCASE",
                [identity.trim()],
                user_from_row,
            )
            .optional()?)
    }

    pub fn authenticate(
        &self,
        login: &str,
        password: &str,
        user_agent: Option<&str>,
        ip_summary: Option<&str>,
    ) -> Result<BrowserSession, AuthError> {
        self.authenticate_at(login, password, user_agent, ip_summary, now()?)
    }

    fn authenticate_at(
        &self,
        login: &str,
        password: &str,
        user_agent: Option<&str>,
        ip_summary: Option<&str>,
        now: i64,
    ) -> Result<BrowserSession, AuthError> {
        let login = login.trim();
        let mut connection = self.pool.connection()?;
        let row = connection
            .query_row(
                "SELECT u.id, u.username, u.email, u.role, u.disabled, p.password_hash\n\
             FROM users u JOIN password_credentials p ON p.user_id = u.id\n\
             WHERE u.username = ?1 COLLATE NOCASE OR u.email = ?1 COLLATE NOCASE",
                [login],
                |row| Ok((user_from_row(row)?, row.get::<_, String>(5)?)),
            )
            .optional()?;
        let Some((user, password_hash)) = row else {
            return Err(AuthError::InvalidCredentials);
        };
        verify_password(password, &password_hash)?;
        if user.disabled {
            return Err(AuthError::Disabled);
        }
        let material = SessionMaterial::generate()?;
        let id = Uuid::new_v4().to_string();
        let expires_at = now.saturating_add(self.session_seconds);
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO sessions(id, user_id, token_hash, csrf_hash, created_at, expires_at,\n\
             last_used_at, user_agent, ip_summary) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?5, ?7, ?8)",
            params![
                id,
                user.id,
                material.token_hash.as_slice(),
                material.csrf_hash.as_slice(),
                now,
                expires_at,
                summarize(user_agent, 160),
                summarize(ip_summary, 80)
            ],
        )?;
        audit(
            &transaction,
            Some(&user.id),
            Some(&id),
            "session.created",
            "session",
            Some(&id),
        )?;
        transaction.commit()?;
        Ok(BrowserSession {
            id,
            user,
            token: material.token,
            csrf_token: material.csrf_token,
            expires_at,
        })
    }

    pub fn browser_principal(&self, token: &str) -> Result<Principal, AuthError> {
        self.principal(token, SessionKind::Browser, now()?)
    }

    pub fn reader_principal(&self, token: &str) -> Result<Principal, AuthError> {
        self.principal(token, SessionKind::Reader, now()?)
    }

    fn principal(&self, token: &str, kind: SessionKind, now: i64) -> Result<Principal, AuthError> {
        if token.len() > 128 || token.is_empty() {
            return Err(AuthError::InvalidSession);
        }
        let token_hash = hash_secret(token.as_bytes());
        let table = match kind {
            SessionKind::Browser => "sessions",
            SessionKind::Reader => "device_sessions",
        };
        let sql = format!(
            "SELECT s.id, u.id, u.username, u.email, u.role, u.disabled\n\
             FROM {table} s JOIN users u ON u.id = s.user_id\n\
             WHERE s.token_hash = ?1 AND s.revoked_at IS NULL AND s.expires_at > ?2 AND u.disabled = 0"
        );
        let connection = self.pool.connection()?;
        let principal = connection
            .query_row(&sql, params![token_hash.as_slice(), now], |row| {
                Ok(Principal {
                    session_id: row.get(0)?,
                    user: user_from_offset(row, 1)?,
                    kind,
                })
            })
            .optional()?
            .ok_or(AuthError::InvalidSession)?;
        connection.execute(
            &format!("UPDATE {table} SET last_used_at = ?2 WHERE id = ?1"),
            params![principal.session_id, now],
        )?;
        Ok(principal)
    }

    pub fn validate_csrf(&self, principal: &Principal, token: &str) -> Result<(), AuthError> {
        let table = match principal.kind {
            SessionKind::Browser => "sessions",
            SessionKind::Reader => "device_sessions",
        };
        let connection = self.pool.connection()?;
        let stored: Option<Vec<u8>> = connection
            .query_row(
                &format!(
                    "SELECT csrf_hash FROM {table} WHERE id = ?1 AND revoked_at IS NULL\n\
                     AND expires_at > unixepoch()"
                ),
                [&principal.session_id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(stored) = stored else {
            return Err(AuthError::InvalidSession);
        };
        let actual = hash_secret(token.as_bytes());
        if stored.as_slice().ct_eq(actual.as_slice()).into() {
            Ok(())
        } else {
            Err(AuthError::Forbidden)
        }
    }

    pub fn revoke_session(&self, user_id: &str, session_id: &str) -> Result<bool, AuthError> {
        let now = now()?;
        let mut connection = self.pool.connection()?;
        let transaction = connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE sessions SET revoked_at = ?3 WHERE id = ?1 AND user_id = ?2 AND revoked_at IS NULL",
            params![session_id, user_id, now],
        )? == 1;
        if changed {
            audit(
                &transaction,
                Some(user_id),
                None,
                "session.revoked",
                "session",
                Some(session_id),
            )?;
        }
        transaction.commit()?;
        Ok(changed)
    }

    pub fn revoke_all_sessions(&self, user_id: &str) -> Result<usize, AuthError> {
        let now = now()?;
        let mut connection = self.pool.connection()?;
        let transaction = connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE sessions SET revoked_at = ?2 WHERE user_id = ?1 AND revoked_at IS NULL",
            params![user_id, now],
        )?;
        audit(
            &transaction,
            Some(user_id),
            None,
            "sessions.revoked_all",
            "user",
            Some(user_id),
        )?;
        transaction.commit()?;
        Ok(changed)
    }

}
