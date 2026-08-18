use std::{
    sync::{Arc, Mutex},
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result, bail};
use async_trait::async_trait;
use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};
use rill_db::DbPool;
use rill_ingestion::IngestionService;
use rill_secrets::SecretStore;
use rill_telegram_bot::{
    BindOutcome, BindRequest, SubscribeOutcome, SubscribeRequest, TelegramBotService,
    run_long_polling,
};
use rusqlite::{OptionalExtension, Transaction, params};
use serde::Serialize;
use sha2::{Digest, Sha256};
use teloxide::{Bot, prelude::Requester};
use tokio::{sync::oneshot, task::JoinHandle};
use uuid::Uuid;

#[derive(Clone)]
pub(crate) struct TelegramIntegration {
    pool: DbPool,
    secrets: Option<SecretStore>,
    supervisor: TelegramBotSupervisor,
}

#[derive(Clone)]
struct TelegramBotDomain {
    pool: DbPool,
    ingestion: IngestionService,
}

#[derive(Clone)]
struct TelegramBotSupervisor {
    domain: Arc<TelegramBotDomain>,
    state: Arc<Mutex<BotTaskState>>,
}

#[derive(Default)]
struct BotTaskState {
    username: Option<String>,
    shutdown: Option<oneshot::Sender<()>>,
    task: Option<JoinHandle<()>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TelegramBotView {
    pub configured: bool,
    pub active: bool,
    pub username: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TelegramBindingView {
    pub bound: bool,
    pub telegram_user_id: Option<i64>,
    pub bot_username: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct TelegramBindingChallengeView {
    pub deep_link: String,
    pub expires_at: i64,
    pub bot_username: String,
}

impl TelegramIntegration {
    pub fn new(pool: DbPool, secrets: Option<SecretStore>, ingestion: IngestionService) -> Self {
        let domain = Arc::new(TelegramBotDomain {
            pool: pool.clone(),
            ingestion,
        });
        Self {
            pool,
            secrets,
            supervisor: TelegramBotSupervisor {
                domain,
                state: Arc::new(Mutex::new(BotTaskState::default())),
            },
        }
    }

    pub async fn start_persisted(&self) -> Result<()> {
        let Some((secret_id, configured_username)) = self.load_bot_setting()? else {
            return Ok(());
        };
        let token = self.read_secret(&secret_id)?;
        let username = validate_token(&token).await?;
        if configured_username.as_deref() != Some(username.as_str()) {
            self.update_bot_username(&username)?;
        }
        self.supervisor.replace(token, username);
        Ok(())
    }

    pub fn bot_view(&self) -> Result<TelegramBotView> {
        let configured = self.load_bot_setting()?.is_some();
        let username = self.supervisor.username();
        Ok(TelegramBotView {
            configured,
            active: username.is_some(),
            username,
        })
    }

    pub async fn configure_bot(
        &self,
        actor_user_id: &str,
        actor_session_id: &str,
        token: String,
    ) -> Result<TelegramBotView> {
        if token.trim().is_empty() {
            bail!("Telegram bot token is empty");
        }
        let username = validate_token(&token).await?;
        let store = self
            .secrets
            .as_ref()
            .context("master key is required to save the Telegram bot token")?;
        let previous_secret = self.load_bot_setting()?.map(|value| value.0);
        let new_secret = store.put(None, "telegram-bot-token", token.as_bytes())?;
        let config = serde_json::json!({ "username": username });
        let config_json = serde_json::to_string(&config)?;
        let saved = self.pool.with_connection(|connection| {
            let transaction = connection.unchecked_transaction()?;
            transaction.execute(
                "INSERT INTO instance_settings(
                   setting_key, config_json, credential_secret_id, enabled, updated_by_user_id
                 ) VALUES ('telegram_bot', ?1, ?2, 1, ?3)
                 ON CONFLICT(setting_key) DO UPDATE SET config_json=excluded.config_json,
                   credential_secret_id=excluded.credential_secret_id, enabled=1,
                   revision=instance_settings.revision+1,
                   updated_by_user_id=excluded.updated_by_user_id, updated_at=unixepoch()",
                params![config_json, new_secret, actor_user_id],
            )?;
            transaction.execute(
                "INSERT INTO audit_events(
                   id, user_id, actor_session_id, event_type, target_type, target_id, detail_json
                 ) VALUES (?1, ?2, ?3, 'settings.telegram_bot_updated',
                   'instance_setting', 'telegram_bot', json_object('configured', 1))",
                params![Uuid::new_v4().to_string(), actor_user_id, actor_session_id],
            )?;
            transaction.commit()
        });
        if let Err(error) = saved {
            let _ = store.delete(&new_secret);
            return Err(error.into());
        }
        if let Some(previous_secret) = previous_secret {
            let _ = store.delete(&previous_secret);
        }
        self.supervisor.replace(token, username.clone());
        Ok(TelegramBotView {
            configured: true,
            active: true,
            username: Some(username),
        })
    }

    pub fn delete_bot(&self, actor_user_id: &str, actor_session_id: &str) -> Result<()> {
        let old_secret: Option<String> = self.pool.with_connection(|connection| {
            let transaction = connection.unchecked_transaction()?;
            let old_secret = transaction
                .query_row(
                    "SELECT credential_secret_id FROM instance_settings
                     WHERE setting_key='telegram_bot'",
                    [],
                    |row| row.get(0),
                )
                .optional()?
                .flatten();
            transaction.execute(
                "DELETE FROM instance_settings WHERE setting_key='telegram_bot'",
                [],
            )?;
            transaction.execute(
                "INSERT INTO audit_events(
                   id, user_id, actor_session_id, event_type, target_type, target_id, detail_json
                 ) VALUES (?1, ?2, ?3, 'settings.telegram_bot_reset',
                   'instance_setting', 'telegram_bot', json_object('configured', 0))",
                params![Uuid::new_v4().to_string(), actor_user_id, actor_session_id],
            )?;
            transaction.commit()?;
            Ok(old_secret)
        })?;
        self.supervisor.stop();
        if let (Some(store), Some(secret_id)) = (&self.secrets, old_secret) {
            let _ = store.delete(&secret_id);
        }
        Ok(())
    }

    pub fn binding(&self, user_id: &str) -> Result<TelegramBindingView> {
        let telegram_user_id: Option<i64> = self.pool.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT telegram_user_id FROM telegram_bot_bindings WHERE user_id=?1",
                    [user_id],
                    |row| row.get(0),
                )
                .optional()
        })?;
        Ok(TelegramBindingView {
            bound: telegram_user_id.is_some(),
            telegram_user_id,
            bot_username: self.supervisor.username(),
        })
    }

    pub fn create_binding_challenge(&self, user_id: &str) -> Result<TelegramBindingChallengeView> {
        let bot_username = self
            .supervisor
            .username()
            .context("Telegram bot is not configured")?;
        let mut raw_token = [0_u8; 32];
        getrandom::fill(&mut raw_token)?;
        let token = URL_SAFE_NO_PAD.encode(raw_token);
        let token_hash = Sha256::digest(token.as_bytes());
        let expires_at = unix_now().saturating_add(600);
        self.pool.with_connection(|connection| {
            let transaction = connection.unchecked_transaction()?;
            transaction.execute(
                "DELETE FROM telegram_binding_challenges
                 WHERE user_id=?1 AND consumed_at IS NULL",
                [user_id],
            )?;
            transaction.execute(
                "INSERT INTO telegram_binding_challenges(
                   id, user_id, token_hash, expires_at
                 ) VALUES (?1, ?2, ?3, ?4)",
                params![
                    Uuid::new_v4().to_string(),
                    user_id,
                    token_hash.as_slice(),
                    expires_at
                ],
            )?;
            transaction.commit()
        })?;
        Ok(TelegramBindingChallengeView {
            deep_link: format!("https://t.me/{bot_username}?start={token}"),
            expires_at,
            bot_username,
        })
    }

    pub fn unbind(&self, user_id: &str) -> Result<()> {
        self.pool.with_connection(|connection| {
            connection.execute(
                "DELETE FROM telegram_bot_bindings WHERE user_id=?1",
                [user_id],
            )
        })?;
        Ok(())
    }

    fn load_bot_setting(&self) -> Result<Option<(String, Option<String>)>> {
        self.pool
            .with_connection(|connection| {
                connection
                    .query_row(
                        "SELECT credential_secret_id, json_extract(config_json, '$.username')
                     FROM instance_settings
                     WHERE setting_key='telegram_bot' AND enabled=1
                       AND credential_secret_id IS NOT NULL",
                        [],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .optional()
            })
            .map_err(Into::into)
    }

    fn read_secret(&self, secret_id: &str) -> Result<String> {
        let store = self
            .secrets
            .as_ref()
            .context("master key is required to load the Telegram bot token")?;
        Ok(String::from_utf8(store.get(secret_id)?)?)
    }

    fn update_bot_username(&self, username: &str) -> Result<()> {
        self.pool.with_connection(|connection| {
            connection.execute(
                "UPDATE instance_settings SET config_json=json_object('username', ?1),
                   updated_at=unixepoch() WHERE setting_key='telegram_bot'",
                [username],
            )
        })?;
        Ok(())
    }
}

impl TelegramBotSupervisor {
    fn replace(&self, token: String, username: String) {
        self.stop();
        let bot = Bot::new(token);
        let domain = self.domain.clone();
        let (shutdown, stopped) = oneshot::channel();
        let task = tokio::spawn(async move {
            run_long_polling(bot, domain, async move {
                let _ = stopped.await;
            })
            .await;
        });
        let mut state = self.state.lock().unwrap_or_else(|value| value.into_inner());
        state.username = Some(username);
        state.shutdown = Some(shutdown);
        state.task = Some(task);
    }

    fn stop(&self) {
        let mut state = self.state.lock().unwrap_or_else(|value| value.into_inner());
        if let Some(shutdown) = state.shutdown.take() {
            let _ = shutdown.send(());
        }
        state.task.take();
        state.username = None;
    }

    fn username(&self) -> Option<String> {
        self.state
            .lock()
            .unwrap_or_else(|value| value.into_inner())
            .username
            .clone()
    }
}

#[async_trait]
impl TelegramBotService for TelegramBotDomain {
    type Error = anyhow::Error;

    async fn allow_message(&self, telegram_user_id: u64) -> Result<bool> {
        let telegram_user_id = i64::try_from(telegram_user_id)
            .context("Telegram user ID is outside the supported range")?;
        let now = unix_now();
        let mut connection = self.pool.connection()?;
        let transaction =
            connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        transaction.execute(
            "DELETE FROM telegram_bot_rate_limits WHERE updated_at < ?1 - 604800",
            [now],
        )?;
        let user_allowed =
            claim_rate_limit(&transaction, &format!("user:{telegram_user_id}"), 10, now)?;
        if !user_allowed {
            transaction.commit()?;
            return Ok(false);
        }
        let global_allowed = claim_rate_limit(&transaction, "global", 120, now)?;
        transaction.commit()?;
        Ok(global_allowed)
    }

    async fn consume_bind_token(&self, request: BindRequest) -> Result<BindOutcome> {
        let telegram_user_id = i64::try_from(request.telegram_user_id)
            .context("Telegram user ID is outside the supported range")?;
        let token_hash = Sha256::digest(request.token.as_bytes());
        let mut connection = self.pool.connection()?;
        let transaction =
            connection.transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        let user_id: Option<String> = transaction
            .query_row(
                "SELECT user_id FROM telegram_binding_challenges
                     WHERE token_hash=?1 AND consumed_at IS NULL AND expires_at>=unixepoch()",
                [token_hash.as_slice()],
                |row| row.get(0),
            )
            .optional()?;
        let Some(user_id) = user_id else {
            transaction.commit()?;
            return Ok(BindOutcome::InvalidOrExpired);
        };
        let existing: Option<(String, i64)> = transaction
            .query_row(
                "SELECT user_id, telegram_user_id FROM telegram_bot_bindings
                     WHERE user_id=?1 OR telegram_user_id=?2",
                params![user_id, telegram_user_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((existing_user, existing_telegram)) = existing {
            transaction.execute(
                    "UPDATE telegram_binding_challenges SET consumed_at=unixepoch() WHERE token_hash=?1",
                    [token_hash.as_slice()],
                )?;
            transaction.commit()?;
            return Ok(
                if existing_user == user_id && existing_telegram == telegram_user_id {
                    BindOutcome::AlreadyBound
                } else {
                    BindOutcome::InvalidOrExpired
                },
            );
        }
        transaction.execute(
            "INSERT INTO telegram_bot_bindings(
                   user_id, telegram_user_id, private_chat_id
                 ) VALUES (?1, ?2, ?3)",
            params![user_id, telegram_user_id, request.private_chat_id],
        )?;
        transaction.execute(
            "UPDATE telegram_binding_challenges SET consumed_at=unixepoch() WHERE token_hash=?1",
            [token_hash.as_slice()],
        )?;
        transaction.commit()?;
        Ok(BindOutcome::Bound)
    }

    async fn ensure_channel_subscription(
        &self,
        request: SubscribeRequest,
    ) -> Result<SubscribeOutcome> {
        let telegram_user_id = i64::try_from(request.telegram_user_id)
            .context("Telegram user ID is outside the supported range")?;
        let user_id: Option<String> = self.pool.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT user_id FROM telegram_bot_bindings WHERE telegram_user_id=?1",
                    [telegram_user_id],
                    |row| row.get(0),
                )
                .optional()
        })?;
        let user_id = user_id.context("Telegram account is not bound to Rill")?;
        let result = self.ingestion.ensure_telegram_subscription(
            &user_id,
            &request.channel.username,
            request.channel.telegram_chat_id,
            request.channel.title.as_deref(),
        )?;
        Ok(if result.created_subscription {
            SubscribeOutcome::Added
        } else {
            SubscribeOutcome::AlreadySubscribed
        })
    }

    async fn claim_reply(&self, idempotency_key: &str) -> Result<bool> {
        Ok(self.pool.with_connection(|connection| {
            let transaction = connection.unchecked_transaction()?;
            transaction.execute(
                "DELETE FROM telegram_bot_update_claims
                 WHERE created_at < unixepoch() - 604800",
                [],
            )?;
            let inserted = transaction.execute(
                "INSERT OR IGNORE INTO telegram_bot_update_claims(claim_key) VALUES (?1)",
                [idempotency_key],
            )? == 1;
            transaction.commit()?;
            Ok(inserted)
        })?)
    }
}

fn claim_rate_limit(
    transaction: &Transaction<'_>,
    scope_key: &str,
    limit: i64,
    now: i64,
) -> rusqlite::Result<bool> {
    let state: Option<(i64, i64)> = transaction
        .query_row(
            "SELECT window_started_at, message_count
             FROM telegram_bot_rate_limits WHERE scope_key=?1",
            [scope_key],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .optional()?;
    match state {
        None => {
            transaction.execute(
                "INSERT INTO telegram_bot_rate_limits(
                   scope_key, window_started_at, message_count, updated_at
                 ) VALUES (?1, ?2, 1, ?2)",
                params![scope_key, now],
            )?;
            Ok(true)
        }
        Some((window_started_at, _)) if window_started_at <= now.saturating_sub(60) => {
            transaction.execute(
                "UPDATE telegram_bot_rate_limits
                 SET window_started_at=?2, message_count=1, updated_at=?2
                 WHERE scope_key=?1",
                params![scope_key, now],
            )?;
            Ok(true)
        }
        Some((_, message_count)) if message_count >= limit => Ok(false),
        Some(_) => {
            transaction.execute(
                "UPDATE telegram_bot_rate_limits
                 SET message_count=message_count+1, updated_at=?2
                 WHERE scope_key=?1",
                params![scope_key, now],
            )?;
            Ok(true)
        }
    }
}

async fn validate_token(token: &str) -> Result<String> {
    let me = tokio::time::timeout(Duration::from_secs(15), Bot::new(token.to_owned()).get_me())
        .await
        .map_err(|_| anyhow::anyhow!("Telegram bot validation timed out"))?
        .map_err(|_| anyhow::anyhow!("Telegram rejected the bot token"))?;
    me.user
        .username
        .context("Telegram bot has no public username")
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_secs()).ok())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use rill_telegram_bot::ChannelReference;

    use super::*;

    #[tokio::test]
    async fn binding_and_forward_create_one_subscription_idempotently() {
        let pool = DbPool::open_in_memory().unwrap();
        pool.with_connection(|connection| {
            connection.execute(
                "INSERT INTO users(id, username, role) VALUES ('user', 'user', 'user')",
                [],
            )
        })
        .unwrap();
        let token = "one-time-token";
        let token_hash = Sha256::digest(token.as_bytes());
        pool.with_connection(|connection| {
            connection.execute(
                "INSERT INTO telegram_binding_challenges(
                   id, user_id, token_hash, expires_at
                 ) VALUES ('challenge', 'user', ?1, unixepoch()+600)",
                [token_hash.as_slice()],
            )
        })
        .unwrap();
        let domain = TelegramBotDomain {
            pool: pool.clone(),
            ingestion: IngestionService::new(pool.clone(), 25),
        };
        let bound = domain
            .consume_bind_token(BindRequest {
                token: token.into(),
                telegram_user_id: 42,
                private_chat_id: 84,
                idempotency_key: "bind".into(),
            })
            .await
            .unwrap();
        assert_eq!(bound, BindOutcome::Bound);
        let request = SubscribeRequest {
            telegram_user_id: 42,
            channel: ChannelReference {
                username: "genau".into(),
                telegram_chat_id: Some(-10042),
                title: Some("GENAU".into()),
                forwarded_message_id: Some(7),
            },
            idempotency_key: "subscribe".into(),
        };
        assert_eq!(
            domain
                .ensure_channel_subscription(request.clone())
                .await
                .unwrap(),
            SubscribeOutcome::Added
        );
        assert_eq!(
            domain.ensure_channel_subscription(request).await.unwrap(),
            SubscribeOutcome::AlreadySubscribed
        );
        assert!(domain.claim_reply("reply").await.unwrap());
        assert!(!domain.claim_reply("reply").await.unwrap());
    }

    #[tokio::test]
    async fn bot_messages_are_rate_limited_per_telegram_user() {
        let pool = DbPool::open_in_memory().unwrap();
        let domain = TelegramBotDomain {
            pool: pool.clone(),
            ingestion: IngestionService::new(pool, 25),
        };
        for _ in 0..10 {
            assert!(domain.allow_message(42).await.unwrap());
        }
        assert!(!domain.allow_message(42).await.unwrap());
        assert!(domain.allow_message(43).await.unwrap());
    }
}
