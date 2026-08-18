use std::{collections::BTreeMap, sync::Arc, time::Duration};

use async_trait::async_trait;
use futures_util::TryStreamExt;
use mail_parser::MessageParser;
use rill_domain::RawSourceItem;
use rill_secrets::{SecretError, SecretStore};
use rill_source_api::SourceBatch;
use rustls::{ClientConfig, RootCertStore, pki_types::ServerName};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use thiserror::Error;
use tokio::net::TcpStream;
use tokio_rustls::TlsConnector;
use zeroize::Zeroizing;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EmailAccountConfig {
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    pub username: String,
    #[serde(default = "default_mailbox")]
    pub mailbox: String,
    #[serde(default)]
    pub folders: Vec<String>,
    #[serde(default)]
    pub mark_as_read: bool,
    pub password_secret_id: String,
    #[serde(default = "default_poll_seconds")]
    pub poll_interval_seconds: u64,
    #[serde(default = "enabled")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct EmailCursor {
    #[serde(default)]
    pub highest_uid: u32,
    #[serde(default)]
    pub folders: BTreeMap<String, u32>,
}

#[derive(Debug, Error)]
pub enum EmailError {
    #[error("invalid email source configuration: {0}")]
    InvalidConfig(String),
    #[error("email credential is unavailable: {0}")]
    Secret(#[from] SecretError),
    #[error("IMAP operation failed: {0}")]
    Imap(String),
    #[error("IMAP connection timed out")]
    Timeout,
    #[error("email message exceeds {0} byte limit")]
    TooLarge(usize),
}

#[async_trait]
pub trait EmailGateway: Send + Sync {
    async fn poll(
        &self,
        config: &EmailAccountConfig,
        cursor: &EmailCursor,
        limit: usize,
    ) -> Result<SourceBatch, EmailError>;
}

#[derive(Clone)]
pub struct ImapEmailGateway {
    secrets: SecretStore,
    timeout: Duration,
    maximum_message_bytes: usize,
}

impl ImapEmailGateway {
    pub fn new(secrets: SecretStore, timeout: Duration, maximum_message_bytes: usize) -> Self {
        Self {
            secrets,
            timeout,
            maximum_message_bytes: maximum_message_bytes.max(1024),
        }
    }

    async fn poll_inner(
        &self,
        config: &EmailAccountConfig,
        cursor: &EmailCursor,
        limit: usize,
    ) -> Result<SourceBatch, EmailError> {
        validate_config(config)?;
        let password = self.secrets.get(&config.password_secret_id)?;
        let password = Zeroizing::new(
            String::from_utf8(password)
                .map_err(|_| EmailError::InvalidConfig("credential is not UTF-8".into()))?,
        );
        let tcp = TcpStream::connect((config.host.as_str(), config.port))
            .await
            .map_err(|error| EmailError::Imap(error.to_string()))?;
        let roots = RootCertStore::from_iter(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
        let tls = TlsConnector::from(Arc::new(
            ClientConfig::builder()
                .with_root_certificates(roots)
                .with_no_client_auth(),
        ));
        let server_name = ServerName::try_from(config.host.clone())
            .map_err(|_| EmailError::InvalidConfig("invalid TLS server name".into()))?;
        let stream = tls
            .connect(server_name, tcp)
            .await
            .map_err(|error| EmailError::Imap(error.to_string()))?;
        let client = async_imap::Client::new(stream);
        let mut session = client
            .login(&config.username, &password)
            .await
            .map_err(|(error, _)| EmailError::Imap(error.to_string()))?;
        let folders = selected_folders(config)?;
        let mut items = Vec::with_capacity(limit.min(500));
        let mut next_cursor = cursor.clone();
        for folder in folders {
            if items.len() >= limit.min(500) {
                break;
            }
            session
                .select(&folder)
                .await
                .map_err(|error| EmailError::Imap(error.to_string()))?;
            let previous_uid = cursor
                .folders
                .get(&folder)
                .copied()
                .unwrap_or(cursor.highest_uid);
            let start = previous_uid.saturating_add(1).max(1);
            let mut uids = session
                .uid_search(format!("UID {start}:*"))
                .await
                .map_err(|error| EmailError::Imap(error.to_string()))?
                .into_iter()
                .collect::<Vec<_>>();
            uids.sort_unstable();
            uids.truncate(limit.min(500).saturating_sub(items.len()));
            let mut highest_uid = previous_uid;
            for uid in uids {
                let mut fetches = session
                    .uid_fetch(uid.to_string(), "(UID RFC822.SIZE BODY.PEEK[])")
                    .await
                    .map_err(|error| EmailError::Imap(error.to_string()))?;
                while let Some(fetch) = fetches
                    .try_next()
                    .await
                    .map_err(|error| EmailError::Imap(error.to_string()))?
                {
                    let actual_uid = fetch.uid.unwrap_or(uid);
                    if fetch
                        .size
                        .is_some_and(|size| size as usize > self.maximum_message_bytes)
                    {
                        return Err(EmailError::TooLarge(self.maximum_message_bytes));
                    }
                    let body = fetch.body().unwrap_or_default();
                    if body.len() > self.maximum_message_bytes {
                        return Err(EmailError::TooLarge(self.maximum_message_bytes));
                    }
                    if let Some(item) = parse_message(body, actual_uid, &folder) {
                        items.push(item);
                    }
                    highest_uid = highest_uid.max(actual_uid);
                }
                drop(fetches);
                if config.mark_as_read {
                    let mut stored = session
                        .uid_store(uid.to_string(), "+FLAGS.SILENT (\\Seen)")
                        .await
                        .map_err(|error| EmailError::Imap(error.to_string()))?;
                    while stored
                        .try_next()
                        .await
                        .map_err(|error| EmailError::Imap(error.to_string()))?
                        .is_some()
                    {}
                }
            }
            next_cursor.folders.insert(folder, highest_uid);
            next_cursor.highest_uid = next_cursor.highest_uid.max(highest_uid);
        }
        let _ = session.logout().await;
        let changed = next_cursor != *cursor;
        Ok(SourceBatch {
            items,
            cursor: Some(json!(next_cursor)),
            not_modified: !changed,
        })
    }
}

#[async_trait]
impl EmailGateway for ImapEmailGateway {
    async fn poll(
        &self,
        config: &EmailAccountConfig,
        cursor: &EmailCursor,
        limit: usize,
    ) -> Result<SourceBatch, EmailError> {
        if !config.enabled {
            return Ok(SourceBatch {
                items: Vec::new(),
                cursor: Some(json!(cursor)),
                not_modified: true,
            });
        }
        tokio::time::timeout(self.timeout, self.poll_inner(config, cursor, limit))
            .await
            .map_err(|_| EmailError::Timeout)?
    }
}

pub fn parse_message(body: &[u8], uid: u32, mailbox: &str) -> Option<RawSourceItem> {
    let message = MessageParser::default().parse(body)?;
    let author = message
        .from()
        .and_then(|address| address.first())
        .map(|address| {
            address
                .name
                .as_deref()
                .or(address.address.as_deref())
                .unwrap_or("Unknown sender")
                .to_owned()
        });
    let body_text = message.body_text(0).map(|value| value.into_owned());
    let body_html = message.body_html(0).map(|value| value.into_owned());
    let external_id = message
        .message_id()
        .filter(|value| !value.trim().is_empty())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("imap:{mailbox}:{uid}"));
    let list_unsubscribe = message.header_raw("List-Unsubscribe").map(str::to_owned);
    Some(RawSourceItem {
        external_id,
        item_kind: "email-newsletter".into(),
        title: message.subject().map(str::to_owned),
        body_text,
        body_html,
        author,
        source_url: None,
        published_at: message.date().map(|date| date.to_timestamp()),
        edited_at: None,
        deleted_at: None,
        external_urls: Vec::new(),
        media: Vec::new(),
        metadata: json!({
            "imapUid": uid,
            "mailbox": mailbox,
            "listUnsubscribe": list_unsubscribe,
        }),
    })
}

pub fn config_from_value(value: &Value) -> Result<EmailAccountConfig, EmailError> {
    let config: EmailAccountConfig = serde_json::from_value(value.clone())
        .map_err(|error| EmailError::InvalidConfig(error.to_string()))?;
    validate_config(&config)?;
    Ok(config)
}

fn validate_config(config: &EmailAccountConfig) -> Result<(), EmailError> {
    if config.host.trim().is_empty()
        || config.username.trim().is_empty()
        || config.password_secret_id.trim().is_empty()
    {
        return Err(EmailError::InvalidConfig(
            "host, username, and password secret are required".into(),
        ));
    }
    selected_folders(config)?;
    Ok(())
}

fn selected_folders(config: &EmailAccountConfig) -> Result<Vec<String>, EmailError> {
    let mut folders = if config.folders.is_empty() {
        vec![config.mailbox.trim().to_owned()]
    } else {
        config
            .folders
            .iter()
            .map(|folder| folder.trim().to_owned())
            .collect()
    };
    if folders.is_empty()
        || folders
            .iter()
            .any(|folder| folder.is_empty() || folder.chars().count() > 200)
    {
        return Err(EmailError::InvalidConfig(
            "at least one valid IMAP folder is required".into(),
        ));
    }
    folders.sort();
    folders.dedup();
    if folders.len() > 32 {
        return Err(EmailError::InvalidConfig(
            "at most 32 IMAP folders are supported".into(),
        ));
    }
    Ok(folders)
}

const fn default_port() -> u16 {
    993
}

fn default_mailbox() -> String {
    "INBOX".into()
}

const fn default_poll_seconds() -> u64 {
    900
}

const fn enabled() -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_newsletter_mime_and_stable_identity() {
        let raw = b"From: Curator <news@example.test>\r\nSubject: Weekly picks\r\nMessage-ID: <week-1@example.test>\r\nDate: Sun, 17 Aug 2026 10:00:00 +0000\r\nContent-Type: text/html; charset=utf-8\r\n\r\n<h1>Weekly picks</h1><p><a href=\"https://a.test/one\">One</a></p><p><a href=\"https://b.test/two\">Two</a></p><p><a href=\"https://c.test/three\">Three</a></p>";
        let item = parse_message(raw, 42, "INBOX").unwrap();
        assert_eq!(item.external_id, "week-1@example.test");
        assert_eq!(item.author.as_deref(), Some("Curator"));
        assert!(item.body_html.unwrap().contains("https://c.test/three"));
    }

    #[test]
    fn recorded_html_and_plain_newsletters_parse_without_credentials() {
        let html = parse_message(
            include_bytes!("../../../fixtures/email/newsletter-html.eml"),
            1,
            "INBOX",
        )
        .unwrap();
        let plain = parse_message(
            include_bytes!("../../../fixtures/email/newsletter-plain.eml"),
            2,
            "Newsletters",
        )
        .unwrap();
        assert_eq!(html.external_id, "systems-weekly-1@example.test");
        assert!(html.body_html.unwrap().contains("SQLite queue recovery"));
        assert_eq!(plain.external_id, "plain-dispatch-1@example.test");
        assert!(plain.body_text.unwrap().contains("WASI rendering limits"));
    }
}
