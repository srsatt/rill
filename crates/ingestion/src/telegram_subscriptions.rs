#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TelegramSubscriptionResult {
    pub source: SourceRegistration,
    pub channel_username: String,
    pub created_source: bool,
    pub created_subscription: bool,
}

impl IngestionService {
    pub fn ensure_telegram_subscription(
        &self,
        user_id: &str,
        username: &str,
        telegram_chat_id: Option<i64>,
        title: Option<&str>,
    ) -> Result<TelegramSubscriptionResult, IngestionError> {
        let username = normalize_telegram_username(username)?;
        let display_name = title
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .unwrap_or(username.as_str());
        let mut connection = self.pool.connection()?;
        let transaction = connection
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)?;
        transaction.execute(
            "INSERT INTO source_definitions(id, kind, display_name, built_in)
             VALUES ('builtin:telegram', 'telegram', 'Telegram public channel', 1)
             ON CONFLICT(kind) DO UPDATE SET display_name=excluded.display_name",
            [],
        )?;

        let by_username: Option<(String, Option<i64>)> = transaction
            .query_row(
                "SELECT source_instance_id, telegram_chat_id FROM public_telegram_channels
                 WHERE username=?1 COLLATE NOCASE",
                [&username],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        let by_chat_id: Option<(String, String)> = telegram_chat_id
            .map(|chat_id| {
                transaction
                    .query_row(
                        "SELECT source_instance_id, username FROM public_telegram_channels
                         WHERE telegram_chat_id=?1",
                        [chat_id],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .optional()
            })
            .transpose()?
            .flatten();

        if let (Some((username_source, _)), Some((chat_source, known_username))) =
            (&by_username, &by_chat_id)
            && username_source != chat_source
        {
            return Err(IngestionError::Invalid(format!(
                "Telegram username @{username} conflicts with channel @{known_username}"
            )));
        }
        if let (Some((_, Some(known_chat_id))), Some(chat_id)) = (&by_username, telegram_chat_id)
            && *known_chat_id != chat_id
        {
            return Err(IngestionError::Invalid(
                "Telegram username is already attached to another channel ID".into(),
            ));
        }

        let existing_source = by_username
            .as_ref()
            .map(|value| value.0.clone())
            .or_else(|| by_chat_id.as_ref().map(|value| value.0.clone()));
        let created_source = existing_source.is_none();
        let source_id = existing_source.unwrap_or_else(|| Uuid::new_v4().to_string());
        let config = json!({
            "username": username,
            "pollIntervalSeconds": 300,
            "enabled": true,
        });

        if created_source {
            transaction.execute(
                "INSERT INTO source_instances(
                   id, definition_id, owner_user_id, name, visibility, visibility_scope,
                   config_json, enabled, audience
                 ) VALUES (?1, (SELECT id FROM source_definitions WHERE kind='telegram'), NULL,
                   ?2, 'public', 'public', ?3, 1, 'subscribers')",
                params![source_id, display_name, serde_json::to_string(&config)?],
            )?;
            transaction.execute(
                "INSERT INTO public_telegram_channels(
                   source_instance_id, username, telegram_chat_id, title
                 ) VALUES (?1, ?2, ?3, ?4)",
                params![source_id, username, telegram_chat_id, title],
            )?;
        } else {
            transaction.execute(
                "UPDATE public_telegram_channels SET username=?2,
                   telegram_chat_id=coalesce(telegram_chat_id, ?3),
                   title=coalesce(?4, title), updated_at=unixepoch()
                 WHERE source_instance_id=?1",
                params![source_id, username, telegram_chat_id, title],
            )?;
            transaction.execute(
                "UPDATE source_instances SET name=?2, config_json=?3, enabled=1,
                   audience='subscribers', updated_at=unixepoch() WHERE id=?1",
                params![source_id, display_name, serde_json::to_string(&config)?],
            )?;
        }

        let created_subscription = transaction.execute(
            "INSERT INTO source_subscriptions(user_id, source_instance_id, enabled)
             VALUES (?1, ?2, 1) ON CONFLICT(user_id, source_instance_id) DO NOTHING",
            params![user_id, source_id],
        )? == 1;
        if !created_subscription {
            transaction.execute(
                "UPDATE source_subscriptions SET enabled=1
                 WHERE user_id=?1 AND source_instance_id=?2",
                params![user_id, source_id],
            )?;
        }
        transaction.commit()?;
        drop(connection);

        self.schedule_poll(&source_id, None)?;
        Ok(TelegramSubscriptionResult {
            source: SourceRegistration {
                id: source_id,
                visibility_scope: "public".into(),
            },
            channel_username: username,
            created_source,
            created_subscription,
        })
    }
}

pub fn normalize_telegram_username(value: &str) -> Result<String, IngestionError> {
    let value = value.trim().trim_start_matches('@').to_ascii_lowercase();
    if !(5..=32).contains(&value.len())
        || !value
            .bytes()
            .next()
            .is_some_and(|byte| byte.is_ascii_alphabetic())
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err(IngestionError::Invalid(
            "Telegram channel username must contain 5-32 ASCII letters, digits, or underscores"
                .into(),
        ));
    }
    Ok(value)
}

#[cfg(test)]
mod telegram_subscription_tests {
    use rill_db::DbPool;

    use super::*;

    fn service() -> IngestionService {
        IngestionService::new(DbPool::open_in_memory().unwrap(), 25)
    }

    fn add_user(service: &IngestionService, id: &str) {
        service
            .pool
            .with_connection(|connection| {
                connection.execute(
                    "INSERT INTO users(id, username, role) VALUES (?1, ?1, 'user')",
                    [id],
                )
            })
            .unwrap();
    }

    #[test]
    fn one_channel_source_serves_multiple_user_subscriptions() {
        let service = service();
        add_user(&service, "alice");
        add_user(&service, "bob");
        let alice = service
            .ensure_telegram_subscription("alice", "@Genau", Some(-10042), Some("GENAU"))
            .unwrap();
        let bob = service
            .ensure_telegram_subscription("bob", "genau", Some(-10042), Some("GENAU"))
            .unwrap();
        assert_eq!(alice.source.id, bob.source.id);
        assert!(alice.created_source);
        assert!(!bob.created_source);
        let (sources, subscriptions): (i64, i64) = service
            .pool
            .with_connection(|connection| {
                Ok((
                    connection.query_row(
                        "SELECT count(*) FROM public_telegram_channels",
                        [],
                        |row| row.get(0),
                    )?,
                    connection.query_row(
                        "SELECT count(*) FROM source_subscriptions",
                        [],
                        |row| row.get(0),
                    )?,
                ))
            })
            .unwrap();
        assert_eq!(sources, 1);
        assert_eq!(subscriptions, 2);
    }

    #[test]
    fn username_cannot_silently_change_channel_identity() {
        let service = service();
        add_user(&service, "alice");
        service
            .ensure_telegram_subscription("alice", "genau", Some(-10042), None)
            .unwrap();
        assert!(
            service
                .ensure_telegram_subscription("alice", "genau", Some(-10043), None)
                .is_err()
        );
    }

    #[test]
    fn subscriber_collection_access_and_polling_are_isolated_and_coalesced() {
        let service = service();
        add_user(&service, "alice");
        add_user(&service, "bob");
        let channel = service
            .ensure_telegram_subscription("alice", "genau", Some(-10042), None)
            .unwrap();
        service
            .ensure_telegram_subscription("bob", "genau", Some(-10042), None)
            .unwrap();
        let raw_item_id = "raw-channel-item";
        service
            .pool
            .with_connection(|connection| {
                connection.execute(
                    "INSERT INTO raw_items(
                       id, source_instance_id, external_id, visibility_scope,
                       item_kind, title, body_text, content_hash
                     ) VALUES (?1, ?2, 'telegram:genau:1', 'public',
                       'message', 'Roundup', 'One link', x'01')",
                    params![raw_item_id, channel.source.id],
                )
            })
            .unwrap();
        assert!(
            service
                .collection_debug(raw_item_id, "alice", false)
                .is_ok()
        );

        service
            .set_source_enabled(&channel.source.id, "bob", false, false)
            .unwrap();
        assert!(
            service
                .collection_debug(raw_item_id, "bob", false)
                .is_err()
        );
        let queued: i64 = service
            .pool
            .with_connection(|connection| {
                connection.query_row(
                    "SELECT count(*) FROM jobs WHERE kind='PollSource' AND status='queued'
                       AND json_extract(payload_json, '$.sourceId')=?1",
                    [&channel.source.id],
                    |row| row.get(0),
                )
            })
            .unwrap();
        assert_eq!(queued, 1, "two subscribers must share one queued poll");

        service
            .set_source_enabled(&channel.source.id, "alice", false, false)
            .unwrap();
        assert!(!service.should_poll_source(&channel.source.id).unwrap());
        let queued: i64 = service
            .pool
            .with_connection(|connection| {
                connection.query_row(
                    "SELECT count(*) FROM jobs WHERE kind='PollSource' AND status='queued'
                       AND json_extract(payload_json, '$.sourceId')=?1",
                    [&channel.source.id],
                    |row| row.get(0),
                )
            })
            .unwrap();
        assert_eq!(queued, 0, "last unsubscribe must stop the poll chain");
    }

    #[test]
    fn deleting_shared_channel_removes_orphan_content_instead_of_making_it_global() {
        let service = service();
        add_user(&service, "alice");
        let channel = service
            .ensure_telegram_subscription("alice", "genau", Some(-10042), None)
            .unwrap();
        service
            .ingest_batch(
                &channel.source.id,
                &SourceBatch {
                    items: vec![RawSourceItem {
                        external_id: "telegram:genau:1".into(),
                        item_kind: "message".into(),
                        title: Some("Shared channel item".into()),
                        body_text: Some("A useful public channel report with enough text.".into()),
                        body_html: None,
                        author: Some("@genau".into()),
                        source_url: Some("https://t.me/genau/1".into()),
                        published_at: Some(1_786_960_800),
                        edited_at: None,
                        deleted_at: None,
                        external_urls: Vec::new(),
                        media: Vec::new(),
                        metadata: json!({}),
                    }],
                    cursor: None,
                    not_modified: false,
                },
            )
            .unwrap();
        service
            .pool
            .with_connection(|connection| {
                connection.execute(
                    "INSERT INTO documents(
                       id, visibility_scope, exact_content_hash, title, body_text
                     ) VALUES ('keep-document', 'public', x'02', 'Keep me',
                       'A public document without a source curator.')",
                    [],
                )
            })
            .unwrap();
        let before: i64 = service
            .pool
            .with_connection(|connection| {
                connection.query_row("SELECT count(*) FROM documents", [], |row| row.get(0))
            })
            .unwrap();
        assert_eq!(before, 2);
        service
            .remove_source(&channel.source.id, "alice", true)
            .unwrap();
        let after: (i64, i64, i64) = service
            .pool
            .with_connection(|connection| {
                connection.query_row(
                    "SELECT
                       (SELECT count(*) FROM documents WHERE title='Shared channel item'),
                       (SELECT count(*) FROM documents WHERE title='Keep me'),
                       (SELECT count(*) FROM document_access
                        WHERE document_id='keep-document' AND user_id IS NULL)",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
            })
            .unwrap();
        assert_eq!(after, (0, 1, 1));
    }
}
