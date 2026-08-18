mod parser;

use std::collections::BTreeMap;

use async_trait::async_trait;
pub use parser::{ParsedPost, parse_channel_html};
use rill_domain::{RawSourceItem, SourceKind};
use rill_source_api::{
    ConditionalHeaders, ConnectorContext, ConnectorError, ConnectorMetadata, SourceBatch,
    SourceConnector, ValidationResult,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use url::Url;

const MAX_PAGES_PER_POLL: usize = 32;
const RECENT_EDIT_OVERLAP: usize = 10;

#[derive(Debug, Clone, Default)]
pub struct TelegramConnector;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TelegramConfig {
    pub username: String,
    #[serde(default = "default_poll_seconds")]
    pub poll_interval_seconds: u64,
    #[serde(default = "enabled")]
    pub enabled: bool,
    #[serde(default)]
    pub shared: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TelegramCursor {
    #[serde(default)]
    pub last_message_id: u64,
}

#[async_trait]
impl SourceConnector for TelegramConnector {
    fn kind(&self) -> SourceKind {
        SourceKind::Telegram
    }

    fn metadata(&self) -> ConnectorMetadata {
        ConnectorMetadata {
            display_name: "Telegram public channel".to_owned(),
            supports_backfill: true,
            supports_push: false,
        }
    }

    fn config_schema(&self) -> Value {
        json!({
            "type": "object",
            "additionalProperties": false,
            "required": ["username"],
            "properties": {
                "username": {
                    "type": "string",
                    "pattern": "^@?[A-Za-z][A-Za-z0-9_]{4,31}$"
                },
                "pollIntervalSeconds": { "type": "integer", "minimum": 60 },
                "enabled": { "type": "boolean" },
                "shared": { "type": "boolean" }
            }
        })
    }

    async fn validate(
        &self,
        _context: &ConnectorContext,
        config: &Value,
    ) -> Result<ValidationResult, ConnectorError> {
        let config = parse_config(config)?;
        let mut messages = Vec::new();
        if normalize_username(&config.username).is_err() {
            messages.push(
                "username must contain 5-32 ASCII letters, digits, or underscores and start with a letter"
                    .to_owned(),
            );
        }
        if config.poll_interval_seconds < 60 {
            messages.push("pollIntervalSeconds must be at least 60".to_owned());
        }
        Ok(ValidationResult {
            valid: messages.is_empty(),
            messages,
        })
    }

    async fn poll(
        &self,
        context: &ConnectorContext,
        config: &Value,
        cursor: Option<&Value>,
        limit: usize,
    ) -> Result<SourceBatch, ConnectorError> {
        let config = parse_config(config)?;
        let username = normalize_username(&config.username)?;
        let cursor = parse_cursor(cursor)?;
        if !config.enabled || limit == 0 {
            return batch(Vec::new(), cursor.last_message_id, true);
        }

        let mut posts = BTreeMap::new();
        let mut before = None;
        let mut overlapped_cursor = false;
        let mut exhausted = false;

        for _ in 0..MAX_PAGES_PER_POLL {
            let url = channel_url(&username, before)?;
            let response = context
                .http
                .get(&url, &ConditionalHeaders::default())
                .await?;
            let page = parse_channel_html(&response.body, &username)?;
            if page.is_empty() {
                validate_empty_preview(&response.body, &username)?;
                exhausted = true;
                break;
            }

            let oldest = page.iter().map(|post| post.message_id).min();
            if cursor.last_message_id > 0
                && page
                    .iter()
                    .any(|post| post.message_id <= cursor.last_message_id)
            {
                overlapped_cursor = true;
            }
            for post in page {
                posts.entry(post.message_id).or_insert(post.item);
            }

            if overlapped_cursor || (cursor.last_message_id == 0 && posts.len() >= limit) {
                break;
            }
            if oldest.is_none() {
                exhausted = true;
                break;
            }
            if oldest == before {
                break;
            }
            before = oldest;
        }

        if cursor.last_message_id > 0 && !overlapped_cursor && !exhausted {
            return Err(ConnectorError::Parse(format!(
                "Telegram pagination exceeded {MAX_PAGES_PER_POLL} pages before reaching cursor {}",
                cursor.last_message_id
            )));
        }

        let (items, next_id) = select_items(posts, cursor.last_message_id, limit);
        let not_modified = items.is_empty();
        batch(items, next_id, not_modified)
    }
}

fn validate_empty_preview(bytes: &[u8], username: &str) -> Result<(), ConnectorError> {
    let html = std::str::from_utf8(bytes)
        .map_err(|error| ConnectorError::Parse(format!("Telegram HTML is not UTF-8: {error}")))?;
    let lowercase = html.to_ascii_lowercase();
    let expected_post = format!("data-post=\"{username}/");
    if lowercase.contains(&expected_post) {
        return Err(ConnectorError::Parse(
            "Telegram preview contained channel posts but none were usable".into(),
        ));
    }
    if !lowercase.contains("tgme_channel_info") {
        return Err(ConnectorError::Parse(
            "Telegram did not return a public channel preview".into(),
        ));
    }
    Ok(())
}

pub fn normalize_username(value: &str) -> Result<String, ConnectorError> {
    let username = value.trim().trim_start_matches('@').to_ascii_lowercase();
    let mut chars = username.chars();
    let valid = (5..=32).contains(&username.len())
        && chars
            .next()
            .is_some_and(|value| value.is_ascii_alphabetic())
        && chars.all(|value| value.is_ascii_alphanumeric() || value == '_');
    if valid {
        Ok(username)
    } else {
        Err(ConnectorError::InvalidConfig(
            "invalid Telegram username".to_owned(),
        ))
    }
}

fn select_items(
    posts: BTreeMap<u64, RawSourceItem>,
    cursor: u64,
    limit: usize,
) -> (Vec<RawSourceItem>, u64) {
    if cursor == 0 {
        let start = posts.len().saturating_sub(limit);
        let selected = posts.into_iter().skip(start).collect::<Vec<_>>();
        let next = selected.last().map(|(id, _)| *id).unwrap_or_default();
        return (selected.into_iter().map(|(_, item)| item).collect(), next);
    }

    let mut newer = posts
        .iter()
        .filter(|(id, _)| **id > cursor)
        .map(|(id, item)| (*id, item.clone()))
        .take(limit)
        .collect::<Vec<_>>();
    let next = newer.last().map(|(id, _)| *id).unwrap_or(cursor);
    let overlap_capacity = limit.saturating_sub(newer.len()).min(RECENT_EDIT_OVERLAP);
    if overlap_capacity > 0 {
        let mut overlap = posts
            .into_iter()
            .filter(|(id, _)| *id <= cursor)
            .rev()
            .take(overlap_capacity)
            .collect::<Vec<_>>();
        overlap.reverse();
        newer.extend(overlap);
        newer.sort_by_key(|(id, _)| *id);
    }
    (newer.into_iter().map(|(_, item)| item).collect(), next)
}

fn channel_url(username: &str, before: Option<u64>) -> Result<Url, ConnectorError> {
    let mut url = Url::parse(&format!("https://t.me/s/{username}"))
        .map_err(|error| ConnectorError::InvalidConfig(error.to_string()))?;
    if let Some(before) = before {
        url.query_pairs_mut()
            .clear()
            .append_pair("before", &before.to_string());
    }
    Ok(url)
}

fn parse_config(config: &Value) -> Result<TelegramConfig, ConnectorError> {
    serde_json::from_value(config.clone())
        .map_err(|error| ConnectorError::InvalidConfig(error.to_string()))
}

fn parse_cursor(cursor: Option<&Value>) -> Result<TelegramCursor, ConnectorError> {
    cursor
        .cloned()
        .map(serde_json::from_value)
        .transpose()
        .map_err(|error| ConnectorError::InvalidConfig(format!("cursor: {error}")))
        .map(Option::unwrap_or_default)
}

fn batch(
    items: Vec<RawSourceItem>,
    last_message_id: u64,
    not_modified: bool,
) -> Result<SourceBatch, ConnectorError> {
    let cursor = serde_json::to_value(TelegramCursor { last_message_id })
        .map_err(|error| ConnectorError::Parse(error.to_string()))?;
    Ok(SourceBatch {
        items,
        cursor: Some(cursor),
        not_modified,
    })
}

const fn default_poll_seconds() -> u64 {
    300
}

const fn enabled() -> bool {
    true
}

#[cfg(test)]
mod tests;
