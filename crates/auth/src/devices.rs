impl AuthService {
    pub fn change_password(
        &self,
        user_id: &str,
        current_password: &str,
        new_password: &str,
    ) -> Result<(), AuthError> {
        validate_password(new_password)?;
        let password_hash = hash_password(new_password)?;
        let now = now()?;
        let mut connection = self.pool.connection()?;
        let old: Option<String> = connection
            .query_row(
                "SELECT password_hash FROM password_credentials WHERE user_id = ?1",
                [user_id],
                |row| row.get(0),
            )
            .optional()?;
        verify_password(
            current_password,
            old.as_deref().ok_or(AuthError::InvalidCredentials)?,
        )?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "UPDATE password_credentials SET password_hash = ?2, changed_at = ?3 WHERE user_id = ?1",
            params![user_id, password_hash, now],
        )?;
        transaction.execute(
            "UPDATE sessions SET revoked_at = ?2 WHERE user_id = ?1 AND revoked_at IS NULL",
            params![user_id, now],
        )?;
        audit(
            &transaction,
            Some(user_id),
            None,
            "password.changed",
            "user",
            Some(user_id),
        )?;
        transaction.commit()?;
        Ok(())
    }

    pub fn create_pairing_code(
        &self,
        user_id: &str,
        label: &str,
    ) -> Result<PairingCode, AuthError> {
        self.create_pairing_code_at(user_id, label, now()?)
    }

    fn create_pairing_code_at(
        &self,
        user_id: &str,
        label: &str,
        now: i64,
    ) -> Result<PairingCode, AuthError> {
        let label = normalize_label(label)?;
        let code = generate_pairing_code()?;
        let code_hash = hash_secret(normalize_pairing_code(&code)?.as_bytes());
        let id = Uuid::new_v4().to_string();
        let expires_at = now.saturating_add(self.pairing_seconds);
        let mut connection = self.pool.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "INSERT INTO pairing_codes(id, user_id, code_hash, label, created_at, expires_at, attempts_remaining)\n\
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![id, user_id, code_hash.as_slice(), label, now, expires_at, self.pairing_max_attempts],
        )?;
        audit(
            &transaction,
            Some(user_id),
            None,
            "reader.pairing_created",
            "pairing_code",
            Some(&id),
        )?;
        transaction.commit()?;
        Ok(PairingCode {
            id,
            code: Secret(code),
            expires_at,
        })
    }

    pub fn consume_pairing_code(
        &self,
        code: &str,
        attempt_key: &str,
        user_agent: Option<&str>,
        ip_summary: Option<&str>,
    ) -> Result<ReaderSession, AuthError> {
        self.consume_pairing_code_at(code, attempt_key, user_agent, ip_summary, now()?)
    }

    fn consume_pairing_code_at(
        &self,
        code: &str,
        attempt_key: &str,
        user_agent: Option<&str>,
        ip_summary: Option<&str>,
        now: i64,
    ) -> Result<ReaderSession, AuthError> {
        let normalized = normalize_pairing_code(code)?;
        let code_hash = hash_secret(normalized.as_bytes());
        let attempt_hash = hash_secret(attempt_key.as_bytes());
        let material = SessionMaterial::generate()?;
        let session_id = Uuid::new_v4().to_string();
        let expires_at = now.saturating_add(self.reader_session_seconds);
        let mut connection = self.pool.connection()?;
        let transaction = connection.transaction()?;
        enforce_pairing_attempt_limit(&transaction, &attempt_hash, now, self.pairing_max_attempts)?;
        let pairing = transaction.query_row(
            "SELECT id, user_id, label, expires_at, consumed_at FROM pairing_codes WHERE code_hash = ?1",
            [code_hash.as_slice()],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?,
                row.get::<_, i64>(3)?, row.get::<_, Option<i64>>(4)?)),
        ).optional()?;
        let Some((pairing_id, user_id, label, pairing_expires, consumed_at)) = pairing else {
            record_pairing_failure(&transaction, &attempt_hash, now)?;
            transaction.commit()?;
            return Err(AuthError::InvalidPairingCode);
        };
        if consumed_at.is_some() {
            record_pairing_failure(&transaction, &attempt_hash, now)?;
            transaction.commit()?;
            return Err(AuthError::PairingReplay);
        }
        if pairing_expires <= now {
            record_pairing_failure(&transaction, &attempt_hash, now)?;
            transaction.commit()?;
            return Err(AuthError::PairingExpired);
        }
        let consumed = transaction.execute(
            "UPDATE pairing_codes SET consumed_at = ?2 WHERE id = ?1 AND consumed_at IS NULL AND expires_at > ?2",
            params![pairing_id, now],
        )?;
        if consumed != 1 {
            return Err(AuthError::PairingReplay);
        }
        transaction.execute(
            "INSERT INTO device_sessions(id, user_id, token_hash, csrf_hash, label, created_at,\n\
             expires_at, last_used_at, user_agent, ip_summary)\n\
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?6, ?8, ?9)",
            params![
                session_id,
                user_id,
                material.token_hash.as_slice(),
                material.csrf_hash.as_slice(),
                label,
                now,
                expires_at,
                summarize(user_agent, 160),
                summarize(ip_summary, 80)
            ],
        )?;
        transaction.execute(
            "DELETE FROM security_rate_limits WHERE key_hash = ?1",
            [attempt_hash.as_slice()],
        )?;
        audit(
            &transaction,
            Some(&user_id),
            Some(&session_id),
            "reader.paired",
            "device_session",
            Some(&session_id),
        )?;
        transaction.commit()?;
        Ok(ReaderSession {
            id: session_id,
            user_id,
            token: material.token,
            csrf_token: material.csrf_token,
            expires_at,
        })
    }

    pub fn list_reader_devices(&self, user_id: &str) -> Result<Vec<ReaderDevice>, AuthError> {
        let connection = self.pool.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, label, created_at, last_used_at, expires_at, user_agent, ip_summary\n\
             FROM device_sessions WHERE user_id = ?1 AND revoked_at IS NULL ORDER BY created_at DESC",
        )?;
        let rows = statement.query_map([user_id], |row| {
            Ok(ReaderDevice {
                id: row.get(0)?,
                label: row.get(1)?,
                created_at: row.get(2)?,
                last_used_at: row.get(3)?,
                expires_at: row.get(4)?,
                user_agent: row.get(5)?,
                ip_summary: row.get(6)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn revoke_reader_device(&self, user_id: &str, device_id: &str) -> Result<bool, AuthError> {
        let now = now()?;
        let mut connection = self.pool.connection()?;
        let transaction = connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE device_sessions SET revoked_at = ?3 WHERE id = ?1 AND user_id = ?2 AND revoked_at IS NULL",
            params![device_id, user_id, now],
        )? == 1;
        if changed {
            audit(
                &transaction,
                Some(user_id),
                None,
                "reader.revoked",
                "device_session",
                Some(device_id),
            )?;
        }
        transaction.commit()?;
        Ok(changed)
    }

    pub fn reader_selected_stream(
        &self,
        user_id: &str,
        device_id: &str,
    ) -> Result<Option<String>, AuthError> {
        let connection = self.pool.connection()?;
        Ok(connection
            .query_row(
                "SELECT st.slug FROM device_sessions ds
                 JOIN streams st ON st.id=ds.selected_stream_id
                 WHERE ds.id=?1 AND ds.user_id=?2 AND ds.revoked_at IS NULL
                   AND ds.expires_at > unixepoch() AND st.owner_user_id=?2 AND st.enabled=1",
                params![device_id, user_id],
                |row| row.get(0),
            )
            .optional()?)
    }

    pub fn set_reader_selected_stream(
        &self,
        user_id: &str,
        device_id: &str,
        stream_slug: &str,
    ) -> Result<(), AuthError> {
        let connection = self.pool.connection()?;
        let changed = connection.execute(
            "UPDATE device_sessions SET selected_stream_id=(SELECT id FROM streams
               WHERE owner_user_id=?1 AND slug=?3 AND enabled=1)
             WHERE id=?2 AND user_id=?1 AND revoked_at IS NULL
               AND EXISTS(SELECT 1 FROM streams WHERE owner_user_id=?1 AND slug=?3 AND enabled=1)",
            params![user_id, device_id, stream_slug],
        )?;
        if changed == 1 {
            Ok(())
        } else {
            Err(AuthError::InvalidInput)
        }
    }
}
