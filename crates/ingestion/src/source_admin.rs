impl IngestionService {
    pub fn source_kind(&self, source_id: &str) -> Result<String, IngestionError> {
        Ok(self.pool.with_connection(|connection| {
            connection.query_row(
                "SELECT sd.kind FROM source_instances si\n\
                 JOIN source_definitions sd ON sd.id = si.definition_id WHERE si.id = ?1",
                [source_id],
                |row| row.get(0),
            )
        })?)
    }

    pub fn can_manage_source(
        &self,
        source_id: &str,
        user_id: &str,
        is_admin: bool,
    ) -> Result<bool, IngestionError> {
        if is_admin {
            return Ok(true);
        }
        let access: Option<(Option<String>, String, bool)> = self.pool.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT si.owner_user_id, si.audience, EXISTS(
                       SELECT 1 FROM source_subscriptions ss
                       WHERE ss.source_instance_id=si.id AND ss.user_id=?2)
                     FROM source_instances si WHERE si.id=?1",
                    params![source_id, user_id],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
                .optional()
        })?;
        Ok(access.is_some_and(|(owner, audience, subscribed)| {
            owner.as_deref() == Some(user_id) || (audience == "subscribers" && subscribed)
        }))
    }

    pub fn list_sources(
        &self,
        user_id: &str,
        is_admin: bool,
    ) -> Result<Vec<SourceView>, IngestionError> {
        let connection = self.pool.connection()?;
        let mut statement = connection.prepare(
            "SELECT si.id, sd.kind, si.name, si.visibility, si.audience,
             CASE WHEN si.audience='subscribers' THEN coalesce(ss.enabled, 0) ELSE si.enabled END,
             (?2 = 1 OR si.owner_user_id = ?1),
             si.processing_prompt,
             sh.last_success_at,\n\
             coalesce(sh.consecutive_failures, 0), sh.last_error_message\n\
             FROM source_instances si JOIN source_definitions sd ON sd.id = si.definition_id\n\
             LEFT JOIN source_subscriptions ss ON ss.source_instance_id=si.id AND ss.user_id=?1
             LEFT JOIN source_health sh ON sh.source_instance_id = si.id\n\
             WHERE ?2 = 1 OR si.owner_user_id = ?1 OR si.audience='global' OR ss.user_id IS NOT NULL\n\
             ORDER BY si.name COLLATE NOCASE",
        )?;
        let rows = statement.query_map(params![user_id, is_admin], |row| {
            Ok(SourceView {
                id: row.get(0)?,
                kind: row.get(1)?,
                name: row.get(2)?,
                visibility: row.get(3)?,
                audience: row.get(4)?,
                enabled: row.get::<_, i64>(5)? != 0,
                editable: row.get::<_, i64>(6)? != 0,
                processing_prompt: row.get(7)?,
                last_success_at: row.get(8)?,
                consecutive_failures: row.get(9)?,
                last_error_message: row.get(10)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn set_source_processing_prompt(
        &self,
        source_id: &str,
        user_id: &str,
        is_admin: bool,
        prompt: &str,
    ) -> Result<(), IngestionError> {
        let prompt = prompt.trim();
        if prompt.chars().count() > 4_000 {
            return Err(IngestionError::Invalid(
                "source processing instructions are too long".into(),
            ));
        }
        let mut connection = self.pool.connection()?;
        let transaction = connection.transaction()?;
        let editable: Option<i64> = transaction
            .query_row(
                "SELECT 1 FROM source_instances
                 WHERE id=?1 AND (?3 OR owner_user_id=?2)",
                params![source_id, user_id, is_admin],
                |row| row.get(0),
            )
            .optional()?;
        if editable.is_none() {
            return Err(IngestionError::Invalid("source is not editable".into()));
        }
        transaction.execute(
            "UPDATE source_instances SET processing_prompt=?2, updated_at=unixepoch() WHERE id=?1",
            params![source_id, prompt],
        )?;
        if prompt.is_empty() {
            transaction.execute(
                "UPDATE document_curators SET included=1 WHERE source_instance_id=?1",
                [source_id],
            )?;
        }
        let documents = {
            let mut statement = transaction.prepare(
                "SELECT DISTINCT document_id FROM document_curators WHERE source_instance_id=?1",
            )?;
            statement
                .query_map([source_id], |row| row.get::<_, String>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?
        };
        transaction.commit()?;
        drop(connection);
        let update_id = Uuid::new_v4();
        for document_id in documents {
            self.jobs.enqueue(
                JobKind::GenerateSummary,
                &json!({ "documentId": document_id }),
                EnqueueOptions {
                    idempotency_key: Some(format!(
                        "GenerateSummary:{document_id}:source-prompt:{update_id}"
                    )),
                    ..Default::default()
                },
            )?;
        }
        Ok(())
    }

    pub fn list_rss_feeds(
        &self,
        user_id: &str,
        is_admin: bool,
    ) -> Result<Vec<RssFeedView>, IngestionError> {
        let connection = self.pool.connection()?;
        let mut statement = connection.prepare(
            "SELECT si.id, si.name, si.config_json FROM source_instances si
             JOIN source_definitions sd ON sd.id=si.definition_id
             LEFT JOIN source_subscriptions ss ON ss.source_instance_id=si.id AND ss.user_id=?1
             WHERE sd.kind='rss' AND (?2 OR si.audience='global' OR si.owner_user_id=?1
               OR (si.audience='subscribers' AND ss.enabled=1))
             ORDER BY si.name COLLATE NOCASE",
        )?;
        let rows = statement.query_map(params![user_id, is_admin], |row| {
            let config: String = row.get(2)?;
            let config: Value = serde_json::from_str(&config).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    2,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?;
            let url = config.get("url").and_then(Value::as_str).ok_or_else(|| {
                rusqlite::Error::FromSqlConversionFailure(
                    2,
                    rusqlite::types::Type::Text,
                    "RSS source lacks URL".into(),
                )
            })?;
            Ok(RssFeedView {
                source_id: row.get(0)?,
                name: row.get(1)?,
                xml_url: url.to_owned(),
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn set_source_enabled(
        &self,
        source_id: &str,
        user_id: &str,
        is_admin: bool,
        enabled: bool,
    ) -> Result<(), IngestionError> {
        if !self.can_manage_source(source_id, user_id, is_admin)? {
            return Err(IngestionError::Invalid("source is not manageable".into()));
        }
        let audience: String = self.pool.with_connection(|connection| {
            connection.query_row(
                "SELECT audience FROM source_instances WHERE id=?1",
                [source_id],
                |row| row.get(0),
            )
        })?;
        if !is_admin && audience == "subscribers" {
            self.pool.with_connection(|connection| {
                connection.execute(
                    "UPDATE source_subscriptions SET enabled=?3
                     WHERE source_instance_id=?1 AND user_id=?2",
                    params![source_id, user_id, enabled],
                )
            })?;
            if enabled {
                self.schedule_poll(source_id, None)?;
            } else {
                self.cancel_queued_polls_if_inactive(source_id)?;
            }
            return Ok(());
        }
        let mut connection = self.pool.connection()?;
        let transaction = connection.transaction()?;
        let raw: String = transaction.query_row(
            "SELECT config_json FROM source_instances WHERE id=?1",
            [source_id],
            |row| row.get(0),
        )?;
        let mut config: Value = serde_json::from_str(&raw)?;
        if let Some(object) = config.as_object_mut() {
            object.insert("enabled".into(), Value::Bool(enabled));
        }
        transaction.execute(
            "UPDATE source_instances SET enabled=?2, config_json=?3, updated_at=unixepoch()
             WHERE id=?1",
            params![source_id, enabled, serde_json::to_string(&config)?],
        )?;
        transaction.commit()?;
        drop(connection);
        if enabled {
            self.schedule_poll(source_id, None)?;
        } else {
            self.cancel_queued_polls_if_inactive(source_id)?;
        }
        Ok(())
    }

    pub fn remove_source(
        &self,
        source_id: &str,
        user_id: &str,
        is_admin: bool,
    ) -> Result<Option<String>, IngestionError> {
        if !self.can_manage_source(source_id, user_id, is_admin)? {
            return Err(IngestionError::Invalid("source is not manageable".into()));
        }
        let audience: String = self.pool.with_connection(|connection| {
            connection.query_row(
                "SELECT audience FROM source_instances WHERE id=?1",
                [source_id],
                |row| row.get(0),
            )
        })?;
        if !is_admin && audience == "subscribers" {
            self.pool.with_connection(|connection| {
                connection.execute(
                    "DELETE FROM source_subscriptions WHERE source_instance_id=?1 AND user_id=?2",
                    params![source_id, user_id],
                )
            })?;
            self.cancel_queued_polls_if_inactive(source_id)?;
            return Ok(None);
        }
        let mut connection = self.pool.connection()?;
        let transaction = connection.transaction()?;
        let credential: Option<String> = transaction
            .query_row(
                "SELECT credential_secret_id FROM source_instances WHERE id=?1",
                [source_id],
                |row| row.get(0),
            )
            .optional()?
            .flatten();
        transaction.execute(
            "DELETE FROM documents
             WHERE id IN (
               SELECT dc.document_id FROM document_curators dc
               WHERE dc.source_instance_id=?1
                 AND NOT EXISTS (
                   SELECT 1 FROM document_curators remaining
                   WHERE remaining.document_id=dc.document_id
                     AND (remaining.source_instance_id IS NULL
                       OR remaining.source_instance_id!=?1)
                 )
             )",
            [source_id],
        )?;
        transaction.execute(
            "DELETE FROM document_curators WHERE source_instance_id=?1",
            [source_id],
        )?;
        transaction.execute(
            "DELETE FROM stories
             WHERE NOT EXISTS (
               SELECT 1 FROM story_memberships sm WHERE sm.story_id=stories.id
             )",
            [],
        )?;
        transaction.execute(
            "DELETE FROM jobs WHERE kind='PollSource' AND status='queued'
               AND json_extract(payload_json, '$.sourceId')=?1",
            [source_id],
        )?;
        transaction.execute("DELETE FROM source_instances WHERE id=?1", [source_id])?;
        transaction.commit()?;
        Ok(credential)
    }

    pub fn collection_debug(
        &self,
        raw_item_id: &str,
        user_id: &str,
        is_admin: bool,
    ) -> Result<CollectionDebugView, IngestionError> {
        let connection = self.pool.connection()?;
        let parent = connection
            .query_row(
                "SELECT ri.source_instance_id, si.name, ri.title, ri.source_url,
                 (SELECT detection_mode FROM collection_overrides co
                   WHERE co.parent_raw_item_id=ri.id)
                 FROM raw_items ri JOIN source_instances si ON si.id=ri.source_instance_id
                 WHERE ri.id=?1 AND (?3 OR EXISTS(
                   SELECT 1 FROM source_access sa
                   WHERE sa.source_instance_id=si.id
                     AND (sa.user_id IS NULL OR sa.user_id=?2)
                 ))",
                params![raw_item_id, user_id, is_admin],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, Option<String>>(2)?,
                        row.get::<_, Option<String>>(3)?,
                        row.get::<_, Option<String>>(4)?,
                    ))
                },
            )
            .optional()?
            .ok_or_else(|| IngestionError::Invalid("collection item is not accessible".into()))?;
        let mut statement = connection.prepare(
            "SELECT id, parser_kind, parser_version, confidence, status,
             parent_display_policy, diagnostics_json FROM collection_expansions
             WHERE parent_raw_item_id=?1 ORDER BY created_at DESC, parser_kind",
        )?;
        let rows = statement.query_map([raw_item_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, String>(1)?,
                row.get::<_, String>(2)?,
                row.get::<_, f32>(3)?,
                row.get::<_, String>(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, String>(6)?,
            ))
        })?;
        let mut expansions = Vec::new();
        for row in rows {
            let (id, parser_kind, parser_version, confidence, status, policy, diagnostics) = row?;
            let mut entry_statement = connection.prepare(
                "SELECT ordinal, target_url, title_hint, commentary, extraction_method,
                 confidence, derived_raw_item_id FROM collection_entries
                 WHERE expansion_id=?1 ORDER BY ordinal",
            )?;
            let entries = entry_statement
                .query_map([id], |row| {
                    Ok(CollectionEntryView {
                        ordinal: row.get(0)?,
                        target_url: row.get(1)?,
                        title_hint: row.get(2)?,
                        commentary: row.get(3)?,
                        extraction_method: row.get(4)?,
                        confidence: row.get(5)?,
                        derived_raw_item_id: row.get(6)?,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            expansions.push(CollectionExpansionView {
                parser_kind,
                parser_version,
                confidence,
                status,
                parent_display_policy: policy,
                diagnostics: serde_json::from_str(&diagnostics)?,
                entries,
            });
        }
        Ok(CollectionDebugView {
            raw_item_id: raw_item_id.to_owned(),
            source_instance_id: parent.0,
            source_name: parent.1,
            parent_title: parent.2,
            parent_url: parent.3,
            override_mode: parent.4,
            expansions,
        })
    }

    pub fn set_collection_override(
        &self,
        raw_item_id: &str,
        user_id: &str,
        is_admin: bool,
        mode: DetectionMode,
    ) -> Result<String, IngestionError> {
        if mode == DetectionMode::Auto {
            return Err(IngestionError::Invalid(
                "automatic mode clears an override instead".into(),
            ));
        }
        self.require_collection_access(raw_item_id, user_id, is_admin)?;
        self.pool.with_connection(|connection| {
            connection.execute(
                "INSERT INTO collection_overrides(parent_raw_item_id, detection_mode, actor_user_id)
                 VALUES (?1, ?2, ?3) ON CONFLICT(parent_raw_item_id) DO UPDATE SET
                 detection_mode=excluded.detection_mode, actor_user_id=excluded.actor_user_id,
                 updated_at=unixepoch()",
                params![raw_item_id, mode.as_str(), user_id],
            )
        })?;
        self.enqueue_collection_detection(raw_item_id, "override")
    }

    pub fn clear_collection_override(
        &self,
        raw_item_id: &str,
        user_id: &str,
        is_admin: bool,
    ) -> Result<String, IngestionError> {
        self.require_collection_access(raw_item_id, user_id, is_admin)?;
        self.pool.with_connection(|connection| {
            connection.execute(
                "DELETE FROM collection_overrides WHERE parent_raw_item_id=?1",
                [raw_item_id],
            )
        })?;
        self.enqueue_collection_detection(raw_item_id, "override-clear")
    }

    pub fn rerun_collection_detection(
        &self,
        raw_item_id: &str,
        user_id: &str,
        is_admin: bool,
    ) -> Result<String, IngestionError> {
        self.require_collection_access(raw_item_id, user_id, is_admin)?;
        self.enqueue_collection_detection(raw_item_id, "rerun")
    }

    fn require_collection_access(
        &self,
        raw_item_id: &str,
        user_id: &str,
        is_admin: bool,
    ) -> Result<(), IngestionError> {
        let allowed: bool = self.pool.with_connection(|connection| {
            connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM raw_items ri JOIN source_instances si
                 ON si.id=ri.source_instance_id WHERE ri.id=?1
                 AND (?3 OR EXISTS(
                   SELECT 1 FROM source_access sa
                   WHERE sa.source_instance_id=si.id
                     AND (sa.user_id IS NULL OR sa.user_id=?2)
                 )))",
                params![raw_item_id, user_id, is_admin],
                |row| row.get(0),
            )
        })?;
        if allowed {
            Ok(())
        } else {
            Err(IngestionError::Invalid(
                "collection item is not accessible".into(),
            ))
        }
    }

    fn enqueue_collection_detection(
        &self,
        raw_item_id: &str,
        reason: &str,
    ) -> Result<String, IngestionError> {
        Ok(self.jobs.enqueue(
            JobKind::DetectCollection,
            &json!({"rawItemId": raw_item_id}),
            EnqueueOptions {
                idempotency_key: Some(format!(
                    "DetectCollection:{raw_item_id}:{reason}:{}",
                    Uuid::new_v4()
                )),
                ..Default::default()
            },
        )?)
    }

    fn cancel_queued_polls_if_inactive(&self, source_id: &str) -> Result<(), IngestionError> {
        if self.should_poll_source(source_id)? {
            return Ok(());
        }
        self.pool.with_connection(|connection| {
            connection.execute(
                "DELETE FROM jobs WHERE kind='PollSource' AND status='queued'
                   AND json_extract(payload_json, '$.sourceId')=?1",
                [source_id],
            )
        })?;
        Ok(())
    }
}
