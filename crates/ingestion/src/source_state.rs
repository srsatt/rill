impl IngestionService {
    pub fn source_poll_interval(&self, source_id: &str) -> Result<u64, IngestionError> {
        let config: String = self.pool.with_connection(|connection| {
            connection.query_row(
                "SELECT config_json FROM source_instances WHERE id = ?1",
                [source_id],
                |row| row.get(0),
            )
        })?;
        let value: Value = serde_json::from_str(&config)?;
        Ok(value
            .get("pollIntervalSeconds")
            .and_then(Value::as_u64)
            .unwrap_or(900)
            .max(60))
    }

    pub fn source_poll_state(
        &self,
        source_id: &str,
    ) -> Result<(Value, Option<Value>), IngestionError> {
        self.load_source_poll_state(source_id)
    }

    pub fn should_poll_source(&self, source_id: &str) -> Result<bool, IngestionError> {
        Ok(self.pool.with_connection(|connection| {
            connection.query_row(
                "SELECT EXISTS(
                   SELECT 1 FROM source_instances si
                   WHERE si.id=?1 AND si.enabled=1
                     AND (si.audience!='subscribers' OR EXISTS(
                       SELECT 1 FROM source_subscriptions ss
                       WHERE ss.source_instance_id=si.id AND ss.enabled=1
                     ))
                 )",
                [source_id],
                |row| row.get(0),
            )
        })?)
    }

    pub fn record_external_poll_success(
        &self,
        source_id: &str,
        cursor: Option<&Value>,
    ) -> Result<(), IngestionError> {
        self.record_poll_success(source_id, cursor)
    }

    pub fn record_external_poll_failure(
        &self,
        source_id: &str,
        message: &str,
    ) -> Result<(), IngestionError> {
        self.record_poll_failure(source_id, message)
    }

    pub fn schedule_poll(
        &self,
        source_id: &str,
        available_at: Option<i64>,
    ) -> Result<String, IngestionError> {
        let suffix = available_at.map_or_else(|| Uuid::new_v4().to_string(), |at| at.to_string());
        Ok(self.jobs.enqueue_coalesced_queued(
            JobKind::PollSource,
            &serde_json::to_value(PollSourcePayload {
                source_id: source_id.to_owned(),
            })?,
            EnqueueOptions {
                available_at,
                idempotency_key: Some(format!("PollSource:{source_id}:{suffix}")),
                ..Default::default()
            },
        )?)
    }

    fn replace_media(
        &self,
        document_id: &str,
        article: &ExtractedArticle,
    ) -> Result<(), IngestionError> {
        let mut connection = self.pool.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute("DELETE FROM media WHERE document_id = ?1", [document_id])?;
        for (ordinal, image) in article.images.iter().enumerate() {
            transaction.execute(
                "INSERT INTO media(id, document_id, media_kind, url, width, height, alt_text, ordinal)\n\
                 VALUES (?1, ?2, 'image', ?3, ?4, ?5, ?6, ?7)",
                params![Uuid::new_v4().to_string(), document_id, image.url, image.width,
                    image.height, image.alt, i64::try_from(ordinal).unwrap_or(i64::MAX)],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    fn enqueue_intelligence(
        &self,
        document_id: &str,
        document: &NormalizedDocument,
    ) -> Result<(), IngestionError> {
        let checksum = content_checksum(&document.title, &document.body_text)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        for kind in [JobKind::GenerateSummary, JobKind::GenerateEmbedding] {
            self.jobs.enqueue(
                kind,
                &json!({ "documentId": document_id }),
                EnqueueOptions {
                    idempotency_key: Some(format!("{}:{document_id}:{checksum}", kind.as_str())),
                    ..Default::default()
                },
            )?;
        }
        Ok(())
    }

    fn load_source_poll_state(
        &self,
        source_id: &str,
    ) -> Result<(Value, Option<Value>), IngestionError> {
        let connection = self.pool.connection()?;
        let config: String = connection.query_row(
            "SELECT config_json FROM source_instances WHERE id = ?1 AND enabled = 1",
            [source_id],
            |row| row.get(0),
        )?;
        let cursor: Option<String> = connection.query_row(
            "SELECT cursor_value FROM source_cursors WHERE source_instance_id = ?1 AND cursor_kind = 'poll'",
            [source_id], |row| row.get(0),
        ).optional()?;
        Ok((
            serde_json::from_str(&config)?,
            cursor
                .map(|value| serde_json::from_str(&value))
                .transpose()?,
        ))
    }

    fn source_identity(&self, source_id: &str) -> Result<(String, String), IngestionError> {
        Ok(self.pool.with_connection(|connection| {
            connection.query_row(
                "SELECT visibility_scope, id FROM source_instances WHERE id = ?1 AND enabled = 1",
                [source_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
        })?)
    }

    fn load_raw_item(
        &self,
        raw_item_id: &str,
    ) -> Result<(String, String, RawSourceItem), IngestionError> {
        let connection = self.pool.connection()?;
        let row = connection.query_row(
            "SELECT source_instance_id, visibility_scope, external_id, item_kind, title,
             body_text, body_html, author, source_url, published_at, edited_at, deleted_at,
             external_urls_json, media_json, metadata_json
             FROM raw_items WHERE id=?1",
            [raw_item_id],
            |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    RawSourceItem {
                        external_id: row.get(2)?,
                        item_kind: row.get(3)?,
                        title: row.get(4)?,
                        body_text: row.get(5)?,
                        body_html: row.get(6)?,
                        author: row.get(7)?,
                        source_url: row.get(8)?,
                        published_at: row.get(9)?,
                        edited_at: row.get(10)?,
                        deleted_at: row.get(11)?,
                        external_urls: serde_json::from_str(&row.get::<_, String>(12)?).map_err(
                            |error| {
                                rusqlite::Error::FromSqlConversionFailure(
                                    12,
                                    rusqlite::types::Type::Text,
                                    Box::new(error),
                                )
                            },
                        )?,
                        media: serde_json::from_str(&row.get::<_, String>(13)?).map_err(
                            |error| {
                                rusqlite::Error::FromSqlConversionFailure(
                                    13,
                                    rusqlite::types::Type::Text,
                                    Box::new(error),
                                )
                            },
                        )?,
                        metadata: serde_json::from_str(&row.get::<_, String>(14)?).map_err(
                            |error| {
                                rusqlite::Error::FromSqlConversionFailure(
                                    14,
                                    rusqlite::types::Type::Text,
                                    Box::new(error),
                                )
                            },
                        )?,
                    },
                ))
            },
        )?;
        Ok(row)
    }

    fn collection_settings_for_source(
        &self,
        source_id: &str,
    ) -> Result<(CollectionPolicy, DetectionMode), IngestionError> {
        let raw: String = self.pool.with_connection(|connection| {
            connection.query_row(
                "SELECT config_json FROM source_instances WHERE id=?1",
                [source_id],
                |row| row.get(0),
            )
        })?;
        let config: Value = serde_json::from_str(&raw)?;
        let mut policy = self.collection_policy.clone();
        if let Some(threshold) = config
            .get("collectionDetectionThreshold")
            .and_then(Value::as_f64)
            .filter(|value| value.is_finite() && (0.0..=1.0).contains(value))
        {
            policy.threshold = threshold as f32;
        }
        if let Some(value) = config
            .get("collectionParentDisplay")
            .and_then(Value::as_str)
        {
            policy.parent_display_policy = parse_parent_display(value)?;
        }
        extend_string_array(
            &mut policy.excluded_hosts,
            config.get("collectionExcludedHosts"),
        );
        extend_string_array(
            &mut policy.excluded_path_fragments,
            config.get("collectionExcludedPathFragments"),
        );
        let mode = match config
            .get("collectionDetectionMode")
            .and_then(Value::as_str)
        {
            Some("force_collection") => DetectionMode::ForceCollection,
            Some("force_single") => DetectionMode::ForceSingle,
            Some("auto") | None => DetectionMode::Auto,
            Some(_) => {
                return Err(IngestionError::Invalid(
                    "source collection detection mode is invalid".into(),
                ));
            }
        };
        Ok((policy, mode))
    }

    fn manual_detection_mode(
        &self,
        raw_item_id: &str,
    ) -> Result<Option<DetectionMode>, IngestionError> {
        let mode: Option<String> = self.pool.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT detection_mode FROM collection_overrides WHERE parent_raw_item_id=?1",
                    [raw_item_id],
                    |row| row.get(0),
                )
                .optional()
        })?;
        mode.map(|mode| match mode.as_str() {
            "force_collection" => Ok(DetectionMode::ForceCollection),
            "force_single" => Ok(DetectionMode::ForceSingle),
            _ => Err(IngestionError::Invalid(
                "persisted collection override is invalid".into(),
            )),
        })
        .transpose()
    }
}
