use std::{error::Error, future::Future, io, sync::Arc};

use teloxide::{
    Bot,
    dispatching::{Dispatcher, UpdateFilterExt},
    dptree,
    prelude::Requester,
    types::{ChatId, Message, Update},
};

use crate::{
    BindOutcome, BindRequest, IncomingAction, SubscribeOutcome, SubscribeRequest,
    TelegramBotService, parse_message,
};

type BoxError = Box<dyn Error + Send + Sync>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BotReply {
    pub chat_id: i64,
    pub text: String,
}

pub async fn process_message<D>(domain: &D, message: &Message) -> Result<Option<BotReply>, D::Error>
where
    D: TelegramBotService,
{
    let Some(parsed) = parse_message(message) else {
        return Ok(None);
    };
    if !domain.allow_message(parsed.telegram_user_id).await? {
        return Ok(None);
    }
    let text = match parsed.action {
        IncomingAction::Bind { token } => {
            match domain
                .consume_bind_token(BindRequest {
                    token,
                    telegram_user_id: parsed.telegram_user_id,
                    private_chat_id: parsed.chat_id,
                    idempotency_key: format!("{}:bind", parsed.idempotency_key),
                })
                .await?
            {
                BindOutcome::Bound => {
                    "Account connected. Forward a public channel post or send its @username."
                }
                BindOutcome::AlreadyBound => {
                    "This Telegram account is already connected. Forward a public channel post or send its @username."
                }
                BindOutcome::InvalidOrExpired => {
                    "That one-time bind link is invalid or expired. Create a new link in Rill and try again."
                }
            }
        }
        IncomingAction::Subscribe { channel } => {
            match domain
                .ensure_channel_subscription(SubscribeRequest {
                    telegram_user_id: parsed.telegram_user_id,
                    channel,
                    idempotency_key: format!("{}:subscribe", parsed.idempotency_key),
                })
                .await?
            {
                SubscribeOutcome::Added => "Channel added to Rill.",
                SubscribeOutcome::AlreadySubscribed => "That channel is already in Rill.",
            }
        }
        IncomingAction::PrivateChannelNeedsUsername => {
            "That forwarded channel has no public username. Rill currently supports public channels; send an @username or t.me link."
        }
        IncomingAction::Help => {
            "Open your one-time Rill bind link first. Then forward a public channel post or send an @username or t.me link."
        }
    };

    if !domain
        .claim_reply(&format!("{}:reply", parsed.idempotency_key))
        .await?
    {
        return Ok(None);
    }
    Ok(Some(BotReply {
        chat_id: parsed.chat_id,
        text: text.to_owned(),
    }))
}

/// Runs exactly one long-poll dispatcher and requests a graceful stop when
/// `shutdown` resolves.
pub async fn run_long_polling<D, F>(bot: Bot, domain: Arc<D>, shutdown: F)
where
    D: TelegramBotService,
    F: Future<Output = ()> + Send + 'static,
{
    let handler = Update::filter_message().endpoint(dispatch_message::<D>);
    let mut dispatcher = Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![domain])
        .build();
    let shutdown_token = dispatcher.shutdown_token();
    let shutdown_task = tokio::spawn(async move {
        shutdown.await;
        if let Ok(stopped) = shutdown_token.shutdown() {
            stopped.await;
        }
    });
    dispatcher.dispatch().await;
    shutdown_task.abort();
}

async fn dispatch_message<D>(bot: Bot, message: Message, domain: Arc<D>) -> Result<(), BoxError>
where
    D: TelegramBotService,
{
    if let Some(reply) = process_message(domain.as_ref(), &message)
        .await
        .map_err(|error| Box::new(io::Error::other(error.to_string())) as BoxError)?
    {
        bot.send_message(ChatId(reply.chat_id), reply.text).await?;
    }
    Ok(())
}
