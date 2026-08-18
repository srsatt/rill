use std::fmt;

use async_trait::async_trait;

#[derive(Clone, PartialEq, Eq)]
pub struct BindRequest {
    pub token: String,
    pub telegram_user_id: u64,
    pub private_chat_id: i64,
    pub idempotency_key: String,
}

impl fmt::Debug for BindRequest {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BindRequest")
            .field("token", &"[redacted]")
            .field("telegram_user_id", &self.telegram_user_id)
            .field("private_chat_id", &self.private_chat_id)
            .field("idempotency_key", &self.idempotency_key)
            .finish()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelReference {
    pub username: String,
    pub telegram_chat_id: Option<i64>,
    pub title: Option<String>,
    pub forwarded_message_id: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubscribeRequest {
    pub telegram_user_id: u64,
    pub channel: ChannelReference,
    pub idempotency_key: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindOutcome {
    Bound,
    AlreadyBound,
    InvalidOrExpired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubscribeOutcome {
    Added,
    AlreadySubscribed,
}

/// Persistence and application lifecycle boundary for the Telegram bot.
///
/// Implementations must atomically consume bind tokens, make subscription
/// creation idempotent by `idempotency_key`, and persist reply claims so a
/// repeated Telegram update does not result in another reply.
#[async_trait]
pub trait TelegramBotService: Send + Sync + 'static {
    type Error: fmt::Display + Send + Sync + 'static;

    async fn allow_message(&self, telegram_user_id: u64) -> Result<bool, Self::Error>;

    async fn consume_bind_token(&self, request: BindRequest) -> Result<BindOutcome, Self::Error>;

    async fn ensure_channel_subscription(
        &self,
        request: SubscribeRequest,
    ) -> Result<SubscribeOutcome, Self::Error>;

    async fn claim_reply(&self, idempotency_key: &str) -> Result<bool, Self::Error>;
}
