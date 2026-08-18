use base64::{Engine as _, engine::general_purpose};
use chacha20poly1305::{
    XChaCha20Poly1305, XNonce,
    aead::{Aead, KeyInit, Payload},
};
use rill_db::{DbError, DbPool};
use rusqlite::{OptionalExtension, params};
use thiserror::Error;
use uuid::Uuid;

const KEY_BYTES: usize = 32;
const NONCE_BYTES: usize = 24;
type EncryptedRecord = (Option<String>, String, Vec<u8>, Vec<u8>);

#[derive(Debug, Error)]
pub enum SecretError {
    #[error("master key must be base64-encoded 32 bytes")]
    InvalidMasterKey,
    #[error("secure randomness is unavailable")]
    Random,
    #[error("secret encryption failed")]
    Encrypt,
    #[error("secret decryption failed")]
    Decrypt,
    #[error("secret not found")]
    NotFound,
    #[error("database error: {0}")]
    Database(#[from] DbError),
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
}

#[derive(Clone)]
pub struct SecretStore {
    pool: DbPool,
    key: [u8; KEY_BYTES],
    key_version: i64,
}

impl SecretStore {
    pub fn from_base64(
        pool: DbPool,
        encoded_key: &str,
        key_version: i64,
    ) -> Result<Self, SecretError> {
        let decoded = general_purpose::URL_SAFE_NO_PAD
            .decode(encoded_key)
            .or_else(|_| general_purpose::STANDARD.decode(encoded_key))
            .map_err(|_| SecretError::InvalidMasterKey)?;
        let key: [u8; KEY_BYTES] = decoded
            .try_into()
            .map_err(|_| SecretError::InvalidMasterKey)?;
        Ok(Self {
            pool,
            key,
            key_version,
        })
    }

    pub fn put(
        &self,
        owner_user_id: Option<&str>,
        purpose: &str,
        plaintext: &[u8],
    ) -> Result<String, SecretError> {
        let id = Uuid::new_v4().to_string();
        let (nonce, ciphertext) = self.encrypt(&id, owner_user_id, purpose, plaintext)?;
        self.pool.with_connection(|connection| {
            connection.execute(
                "INSERT INTO encrypted_secrets(id, owner_user_id, key_version, nonce, ciphertext,
                 purpose) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    id,
                    owner_user_id,
                    self.key_version,
                    nonce,
                    ciphertext,
                    purpose
                ],
            )
        })?;
        Ok(id)
    }

    pub fn update(&self, id: &str, plaintext: &[u8]) -> Result<(), SecretError> {
        let connection = self.pool.connection()?;
        let metadata: Option<(Option<String>, String)> = connection
            .query_row(
                "SELECT owner_user_id, purpose FROM encrypted_secrets WHERE id = ?1",
                [id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        drop(connection);
        let (owner, purpose) = metadata.ok_or(SecretError::NotFound)?;
        let (nonce, ciphertext) = self.encrypt(id, owner.as_deref(), &purpose, plaintext)?;
        let changed = self.pool.with_connection(|connection| {
            connection.execute(
                "UPDATE encrypted_secrets SET key_version=?2, nonce=?3, ciphertext=?4,
                 updated_at=unixepoch() WHERE id=?1",
                params![id, self.key_version, nonce, ciphertext],
            )
        })?;
        if changed == 1 {
            Ok(())
        } else {
            Err(SecretError::NotFound)
        }
    }

    pub fn get(&self, id: &str) -> Result<Vec<u8>, SecretError> {
        let record: Option<EncryptedRecord> = self.pool.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT owner_user_id, purpose, nonce, ciphertext FROM encrypted_secrets
                         WHERE id = ?1",
                    [id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?)),
                )
                .optional()
        })?;
        let (owner, purpose, nonce, ciphertext) = record.ok_or(SecretError::NotFound)?;
        let nonce: [u8; NONCE_BYTES] = nonce.try_into().map_err(|_| SecretError::Decrypt)?;
        XChaCha20Poly1305::new((&self.key).into())
            .decrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: &ciphertext,
                    aad: &associated_data(id, owner.as_deref(), &purpose),
                },
            )
            .map_err(|_| SecretError::Decrypt)
    }

    pub fn delete(&self, id: &str) -> Result<bool, SecretError> {
        Ok(self.pool.with_connection(|connection| {
            connection.execute("DELETE FROM encrypted_secrets WHERE id = ?1", [id])
        })? == 1)
    }

    fn encrypt(
        &self,
        id: &str,
        owner_user_id: Option<&str>,
        purpose: &str,
        plaintext: &[u8],
    ) -> Result<([u8; NONCE_BYTES], Vec<u8>), SecretError> {
        let mut nonce = [0_u8; NONCE_BYTES];
        getrandom::fill(&mut nonce).map_err(|_| SecretError::Random)?;
        let ciphertext = XChaCha20Poly1305::new((&self.key).into())
            .encrypt(
                XNonce::from_slice(&nonce),
                Payload {
                    msg: plaintext,
                    aad: &associated_data(id, owner_user_id, purpose),
                },
            )
            .map_err(|_| SecretError::Encrypt)?;
        Ok((nonce, ciphertext))
    }
}

fn associated_data(id: &str, owner_user_id: Option<&str>, purpose: &str) -> Vec<u8> {
    format!(
        "rill-secret-v1\0{id}\0{}\0{purpose}",
        owner_user_id.unwrap_or("")
    )
    .into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encrypts_updates_and_authenticates_metadata() {
        let pool = DbPool::open_in_memory().unwrap();
        let key = general_purpose::URL_SAFE_NO_PAD.encode([7_u8; 32]);
        let store = SecretStore::from_base64(pool.clone(), &key, 1).unwrap();
        let id = store.put(None, "test", b"first").unwrap();
        assert_eq!(store.get(&id).unwrap(), b"first");
        store.update(&id, b"second").unwrap();
        assert_eq!(store.get(&id).unwrap(), b"second");
        let connection = pool.connection().unwrap();
        let raw: Vec<u8> = connection
            .query_row(
                "SELECT ciphertext FROM encrypted_secrets WHERE id = ?1",
                [&id],
                |row| row.get(0),
            )
            .unwrap();
        assert!(!raw.windows(6).any(|window| window == b"second"));
    }
}
