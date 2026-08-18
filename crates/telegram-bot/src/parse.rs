use teloxide::types::{Message, MessageOrigin};
use url::Url;

use crate::ChannelReference;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum IncomingAction {
    Bind { token: String },
    Subscribe { channel: ChannelReference },
    PrivateChannelNeedsUsername,
    Help,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedMessage {
    pub chat_id: i64,
    pub message_id: i32,
    pub telegram_user_id: u64,
    pub idempotency_key: String,
    pub action: IncomingAction,
}

pub fn parse_message(message: &Message) -> Option<ParsedMessage> {
    if !message.chat.is_private() {
        return None;
    }
    let user_id = message.from.as_ref()?.id.0;
    let idempotency_key = format!("telegram:{}:{}", message.chat.id.0, message.id.0);
    let text = message.text().or_else(|| message.caption()).unwrap_or("");
    let action = parse_start(text)
        .or_else(|| forwarded_channel(message))
        .or_else(|| explicit_channel(text).map(|channel| IncomingAction::Subscribe { channel }))
        .or_else(|| is_help(text).then_some(IncomingAction::Help))?;
    Some(ParsedMessage {
        chat_id: message.chat.id.0,
        message_id: message.id.0,
        telegram_user_id: user_id,
        idempotency_key,
        action,
    })
}

fn is_help(text: &str) -> bool {
    text.split_whitespace()
        .next()
        .and_then(|command| command.split('@').next())
        == Some("/help")
}

fn parse_start(text: &str) -> Option<IncomingAction> {
    let mut parts = text.split_whitespace();
    let command = parts.next()?;
    let command = command
        .split_once('@')
        .map_or(command, |(command, _)| command);
    if command != "/start" {
        return None;
    }
    match (parts.next(), parts.next()) {
        (Some(token), None) if !token.is_empty() && token.len() <= 256 => {
            Some(IncomingAction::Bind {
                token: token.to_owned(),
            })
        }
        _ => Some(IncomingAction::Help),
    }
}

fn forwarded_channel(message: &Message) -> Option<IncomingAction> {
    let MessageOrigin::Channel {
        chat, message_id, ..
    } = message.forward_origin()?
    else {
        return None;
    };
    let Some(username) = chat.username().and_then(normalize_username) else {
        return Some(IncomingAction::PrivateChannelNeedsUsername);
    };
    Some(IncomingAction::Subscribe {
        channel: ChannelReference {
            username,
            telegram_chat_id: Some(chat.id.0),
            title: chat.title().map(str::to_owned),
            forwarded_message_id: Some(message_id.0),
        },
    })
}

pub fn explicit_channel(text: &str) -> Option<ChannelReference> {
    text.split_whitespace().find_map(|token| {
        let token = token.trim_matches(|character: char| {
            matches!(
                character,
                '(' | ')' | '[' | ']' | '<' | '>' | ',' | '.' | ';' | ':' | '!' | '?' | '\'' | '"'
            )
        });
        let username = if let Some(username) = token.strip_prefix('@') {
            normalize_username(username)
        } else {
            username_from_tme_link(token)
        }?;
        Some(ChannelReference {
            username,
            telegram_chat_id: None,
            title: None,
            forwarded_message_id: None,
        })
    })
}

fn username_from_tme_link(value: &str) -> Option<String> {
    let candidate = if value.starts_with("t.me/") || value.starts_with("www.t.me/") {
        format!("https://{value}")
    } else {
        value.to_owned()
    };
    let url = Url::parse(&candidate).ok()?;
    if !matches!(url.scheme(), "http" | "https")
        || !matches!(
            url.host_str()
                .map(|host| host.to_ascii_lowercase())
                .as_deref(),
            Some("t.me" | "www.t.me")
        )
    {
        return None;
    }
    let mut segments = url.path_segments()?;
    let first = segments.next()?;
    let username = if first.eq_ignore_ascii_case("s") {
        segments.next()?
    } else {
        first
    };
    normalize_username(username)
}

fn normalize_username(value: &str) -> Option<String> {
    let value = value.trim().trim_start_matches('@').to_ascii_lowercase();
    let mut characters = value.chars();
    ((5..=32).contains(&value.len())
        && characters
            .next()
            .is_some_and(|character| character.is_ascii_alphabetic())
        && characters.all(|character| character.is_ascii_alphanumeric() || character == '_'))
    .then_some(value)
}
