struct SessionMaterial {
    token: Secret,
    token_hash: [u8; 32],
    csrf_token: Secret,
    csrf_hash: [u8; 32],
}

impl SessionMaterial {
    fn generate() -> Result<Self, AuthError> {
        let token = random_token()?;
        let csrf_token = random_token()?;
        Ok(Self {
            token_hash: hash_secret(token.as_bytes()),
            csrf_hash: hash_secret(csrf_token.as_bytes()),
            token: Secret(token),
            csrf_token: Secret(csrf_token),
        })
    }
}

fn password_hasher() -> Result<Argon2<'static>, AuthError> {
    let parameters = Params::new(19 * 1024, 2, 1, None).map_err(|_| AuthError::PasswordHash)?;
    Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, parameters))
}

fn hash_password(password: &str) -> Result<String, AuthError> {
    let mut salt = [0_u8; 16];
    getrandom::fill(&mut salt).map_err(|_| AuthError::Random)?;
    let salt = SaltString::encode_b64(&salt).map_err(|_| AuthError::PasswordHash)?;
    password_hasher()?
        .hash_password(password.as_bytes(), &salt)
        .map(|hash| hash.to_string())
        .map_err(|_| AuthError::PasswordHash)
}

fn verify_password(password: &str, encoded: &str) -> Result<(), AuthError> {
    let parsed = PasswordHash::new(encoded).map_err(|_| AuthError::InvalidCredentials)?;
    password_hasher()?
        .verify_password(password.as_bytes(), &parsed)
        .map_err(|_| AuthError::InvalidCredentials)
}

fn random_token() -> Result<String, AuthError> {
    let mut bytes = [0_u8; TOKEN_BYTES];
    getrandom::fill(&mut bytes).map_err(|_| AuthError::Random)?;
    Ok(URL_SAFE_NO_PAD.encode(bytes))
}

fn generate_pairing_code() -> Result<String, AuthError> {
    let mut bytes = [0_u8; 5];
    getrandom::fill(&mut bytes).map_err(|_| AuthError::Random)?;
    let value = u64::from_be_bytes([0, 0, 0, bytes[0], bytes[1], bytes[2], bytes[3], bytes[4]]);
    let mut code = String::with_capacity(9);
    for index in (0..8).rev() {
        if index == 3 {
            code.push('-');
        }
        code.push(PAIR_ALPHABET[((value >> (index * 5)) & 31) as usize] as char);
    }
    Ok(code)
}

fn normalize_pairing_code(code: &str) -> Result<String, AuthError> {
    let normalized: String = code
        .chars()
        .filter(|character| *character != '-' && !character.is_whitespace())
        .flat_map(char::to_uppercase)
        .collect();
    if normalized.len() != 8 || !normalized.bytes().all(|byte| PAIR_ALPHABET.contains(&byte)) {
        return Err(AuthError::InvalidPairingCode);
    }
    Ok(normalized)
}

fn enforce_pairing_attempt_limit(
    transaction: &Transaction<'_>,
    key_hash: &[u8; 32],
    now: i64,
    maximum: u32,
) -> Result<(), AuthError> {
    let current: Option<(i64, i64)> = transaction.query_row(
        "SELECT window_started_at, attempts FROM security_rate_limits WHERE key_hash = ?1 AND purpose = 'reader_pair'",
        [key_hash.as_slice()], |row| Ok((row.get(0)?, row.get(1)?)),
    ).optional()?;
    if let Some((window_started, attempts)) = current {
        if now - window_started < 600 && attempts >= i64::from(maximum) {
            return Err(AuthError::RateLimited);
        }
        if now - window_started >= 600 {
            transaction.execute(
                "DELETE FROM security_rate_limits WHERE key_hash = ?1",
                [key_hash.as_slice()],
            )?;
        }
    }
    Ok(())
}

fn record_pairing_failure(
    transaction: &Transaction<'_>,
    key_hash: &[u8; 32],
    now: i64,
) -> rusqlite::Result<()> {
    transaction.execute(
        "INSERT INTO security_rate_limits(key_hash, purpose, window_started_at, attempts)\n\
         VALUES (?1, 'reader_pair', ?2, 1)\n\
         ON CONFLICT(key_hash, purpose) DO UPDATE SET attempts = attempts + 1",
        params![key_hash.as_slice(), now],
    )?;
    Ok(())
}

fn audit(
    transaction: &Transaction<'_>,
    user_id: Option<&str>,
    session_id: Option<&str>,
    event_type: &str,
    target_type: &str,
    target_id: Option<&str>,
) -> rusqlite::Result<()> {
    transaction.execute(
        "INSERT INTO audit_events(id, user_id, actor_session_id, event_type, target_type, target_id)\n\
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![Uuid::new_v4().to_string(), user_id, session_id, event_type, target_type, target_id],
    )?;
    Ok(())
}

fn user_from_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<User> {
    user_from_offset(row, 0)
}

fn user_from_offset(row: &rusqlite::Row<'_>, offset: usize) -> rusqlite::Result<User> {
    let role_text: String = row.get(offset + 3)?;
    let role = Role::from_db(&role_text).ok_or_else(|| {
        rusqlite::Error::InvalidColumnType(
            offset + 3,
            "role".to_owned(),
            rusqlite::types::Type::Text,
        )
    })?;
    Ok(User {
        id: row.get(offset)?,
        username: row.get(offset + 1)?,
        email: row.get(offset + 2)?,
        role,
        disabled: row.get::<_, i64>(offset + 4)? != 0,
    })
}

fn normalize_username(value: &str) -> Result<String, AuthError> {
    let value = value.trim();
    if !(3..=64).contains(&value.len())
        || !value.chars().all(|character| {
            character.is_ascii_alphanumeric() || matches!(character, '_' | '-' | '.')
        })
    {
        return Err(AuthError::InvalidInput);
    }
    Ok(value.to_owned())
}

fn normalize_email(value: Option<&str>) -> Result<Option<String>, AuthError> {
    let Some(value) = value.map(str::trim).filter(|value| !value.is_empty()) else {
        return Ok(None);
    };
    if value.len() > 254
        || value.starts_with('@')
        || value.ends_with('@')
        || value.matches('@').count() != 1
    {
        return Err(AuthError::InvalidInput);
    }
    Ok(Some(value.to_ascii_lowercase()))
}

fn validate_password(password: &str) -> Result<(), AuthError> {
    if password.len() < 12 || password.len() > 1024 {
        Err(AuthError::InvalidInput)
    } else {
        Ok(())
    }
}

fn normalize_label(label: &str) -> Result<String, AuthError> {
    let label = label.trim();
    if label.is_empty() || label.len() > 80 || label.chars().any(char::is_control) {
        Err(AuthError::InvalidInput)
    } else {
        Ok(label.to_owned())
    }
}

fn summarize(value: Option<&str>, maximum: usize) -> Option<String> {
    value
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(|value| value.chars().take(maximum).collect())
}

fn hash_secret(value: &[u8]) -> [u8; 32] {
    Sha256::digest(value).into()
}

fn now() -> Result<i64, AuthError> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| AuthError::Clock)?
        .as_secs();
    Ok(i64::try_from(seconds).unwrap_or(i64::MAX))
}

fn days_to_seconds(days: u64) -> i64 {
    i64::try_from(days.saturating_mul(86_400)).unwrap_or(i64::MAX)
}

fn minutes_to_seconds(minutes: u64) -> i64 {
    i64::try_from(minutes.saturating_mul(60)).unwrap_or(i64::MAX)
}

