use std::{collections::HashSet, convert::Infallible, sync::Mutex};

use async_trait::async_trait;
use teloxide::types::Message;

use super::*;
use crate::parse::explicit_channel;

fn fixture(value: &str) -> Message {
    serde_json::from_str(value).unwrap()
}

#[test]
fn parses_private_start_binding_fixture() {
    let message = fixture(include_str!(
        "../../../fixtures/telegram/bot-start-update.json"
    ));
    let parsed = parse_message(&message).unwrap();
    assert_eq!(parsed.telegram_user_id, 12345);
    assert_eq!(
        parsed.action,
        IncomingAction::Bind {
            token: "one-time-token".to_owned()
        }
    );
}

#[test]
fn ignores_arbitrary_private_messages() {
    let mut value: serde_json::Value = serde_json::from_str(include_str!(
        "../../../fixtures/telegram/bot-start-update.json"
    ))
    .unwrap();
    value["text"] = serde_json::json!("hello bot");
    let message: Message = serde_json::from_value(value).unwrap();
    assert!(parse_message(&message).is_none());
}

#[test]
fn parses_forwarded_channel_origin_fixture() {
    let message = fixture(include_str!(
        "../../../fixtures/telegram/bot-forwarded-update.json"
    ));
    let parsed = parse_message(&message).unwrap();
    assert_eq!(
        parsed.action,
        IncomingAction::Subscribe {
            channel: ChannelReference {
                username: "publicchannel".to_owned(),
                telegram_chat_id: Some(-1001122334455),
                title: Some("Public Channel".to_owned()),
                forwarded_message_id: Some(77),
            }
        }
    );
}

#[test]
fn parses_explicit_username_and_tme_fallbacks() {
    for (input, expected) in [
        ("please add @Mixed_Case", "mixed_case"),
        ("https://t.me/PublicChannel/42", "publicchannel"),
        ("t.me/s/Another_Channel", "another_channel"),
    ] {
        assert_eq!(explicit_channel(input).unwrap().username, expected);
    }
    assert!(explicit_channel("https://example.com/PublicChannel").is_none());
    assert!(explicit_channel("t.me/+privateInvite").is_none());
}

#[derive(Default)]
struct FakeDomain {
    operations: Mutex<HashSet<String>>,
    replies: Mutex<HashSet<String>>,
}

#[async_trait]
impl TelegramBotService for FakeDomain {
    type Error = Infallible;

    async fn allow_message(&self, _telegram_user_id: u64) -> Result<bool, Self::Error> {
        Ok(true)
    }

    async fn consume_bind_token(&self, request: BindRequest) -> Result<BindOutcome, Self::Error> {
        let fresh = self
            .operations
            .lock()
            .unwrap()
            .insert(request.idempotency_key);
        Ok(if fresh {
            BindOutcome::Bound
        } else {
            BindOutcome::AlreadyBound
        })
    }

    async fn ensure_channel_subscription(
        &self,
        request: SubscribeRequest,
    ) -> Result<SubscribeOutcome, Self::Error> {
        let fresh = self
            .operations
            .lock()
            .unwrap()
            .insert(request.idempotency_key);
        Ok(if fresh {
            SubscribeOutcome::Added
        } else {
            SubscribeOutcome::AlreadySubscribed
        })
    }

    async fn claim_reply(&self, idempotency_key: &str) -> Result<bool, Self::Error> {
        Ok(self
            .replies
            .lock()
            .unwrap()
            .insert(idempotency_key.to_owned()))
    }
}

#[tokio::test]
async fn duplicate_update_emits_only_one_reply() {
    let domain = FakeDomain::default();
    let message = fixture(include_str!(
        "../../../fixtures/telegram/bot-explicit-update.json"
    ));
    let first = process_message(&domain, &message).await.unwrap();
    let duplicate = process_message(&domain, &message).await.unwrap();
    assert_eq!(first.unwrap().text, "Channel added to Rill.");
    assert!(duplicate.is_none());
}
