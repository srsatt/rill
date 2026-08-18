use std::{
    fmt,
    time::{SystemTime, UNIX_EPOCH},
};

use argon2::{
    Algorithm, Argon2, Params, PasswordHash, PasswordHasher, PasswordVerifier, Version,
    password_hash::SaltString,
};
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rill_db::{DbError, DbPool};
use rill_domain::{Role, User};
use rusqlite::{OptionalExtension, Transaction, params};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;
use thiserror::Error;
use uuid::Uuid;

mod admin;

pub use admin::{AdminUserView, AuditEventView, BrowserSessionView};

const TOKEN_BYTES: usize = 32;
const PAIR_ALPHABET: &[u8; 32] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

#[derive(Debug, Error)]
pub enum AuthError {
    #[error("invalid username, email, or password")]
    InvalidInput,
    #[error("invalid credentials")]
    InvalidCredentials,
    #[error("account is disabled")]
    Disabled,
    #[error("an account with that username or email already exists")]
    Conflict,
    #[error("session is invalid or expired")]
    InvalidSession,
    #[error("pairing code is invalid")]
    InvalidPairingCode,
    #[error("pairing code expired")]
    PairingExpired,
    #[error("pairing code was already used")]
    PairingReplay,
    #[error("too many pairing attempts; try again later")]
    RateLimited,
    #[error("permission denied")]
    Forbidden,
    #[error("database error: {0}")]
    Database(#[from] DbError),
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("password hashing failed")]
    PasswordHash,
    #[error("secure randomness is unavailable")]
    Random,
    #[error("system clock is before the Unix epoch")]
    Clock,
}

pub struct Secret(String);

impl Secret {
    pub fn expose(&self) -> &str {
        &self.0
    }
}

pub fn new_secret() -> Result<Secret, AuthError> {
    Ok(Secret(random_token()?))
}

impl fmt::Debug for Secret {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("Secret([redacted])")
    }
}

#[derive(Debug)]
pub struct BrowserSession {
    pub id: String,
    pub user: User,
    pub token: Secret,
    pub csrf_token: Secret,
    pub expires_at: i64,
}

#[derive(Debug)]
pub struct PairingCode {
    pub id: String,
    pub code: Secret,
    pub expires_at: i64,
}

#[derive(Debug)]
pub struct ReaderSession {
    pub id: String,
    pub user_id: String,
    pub token: Secret,
    pub csrf_token: Secret,
    pub expires_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Principal {
    pub user: User,
    pub session_id: String,
    pub kind: SessionKind,
}

impl Principal {
    pub fn require_admin(&self) -> Result<(), AuthError> {
        if self.kind == SessionKind::Browser && self.user.role == Role::Admin {
            Ok(())
        } else {
            Err(AuthError::Forbidden)
        }
    }

    pub fn require_browser(&self) -> Result<(), AuthError> {
        if self.kind == SessionKind::Browser {
            Ok(())
        } else {
            Err(AuthError::Forbidden)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionKind {
    Browser,
    Reader,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReaderDevice {
    pub id: String,
    pub label: String,
    pub created_at: i64,
    pub last_used_at: i64,
    pub expires_at: i64,
    pub user_agent: Option<String>,
    pub ip_summary: Option<String>,
}

#[derive(Clone)]
pub struct AuthService {
    pool: DbPool,
    session_seconds: i64,
    reader_session_seconds: i64,
    pairing_seconds: i64,
    pairing_max_attempts: u32,
}

include!("sessions.rs");
include!("devices.rs");

include!("helpers.rs");
include!("tests.rs");
