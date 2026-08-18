struct CollectionStorage<'a> {
    source_id: &'a str,
    raw_id: &'a str,
    parent: &'a RawSourceItem,
    confidence: f32,
    entries: &'a [rill_domain::CollectionEntryCandidate],
    display_policy: ParentDisplayPolicy,
    diagnostics: &'a Value,
    parser_kind: &'a str,
    parser_version: &'a str,
    extraction_method: &'a str,
}

impl IngestionService {
    fn upsert_raw_item(
        &self,
        source_id: &str,
        visibility_scope: &str,
        item: &RawSourceItem,
    ) -> Result<String, IngestionError> {
        if item.external_id.trim().is_empty() {
            return Err(IngestionError::Invalid(
                "raw item external ID is required".into(),
            ));
        }
        let serialized = serde_json::to_vec(item)?;
        let hash: [u8; 32] = Sha256::digest(&serialized).into();
        let id = Uuid::new_v4().to_string();
        let connection = self.pool.connection()?;
        connection.execute(
            "INSERT INTO raw_items(id, source_instance_id, external_id, visibility_scope, item_kind,\n\
             title, body_text, body_html, author, source_url, content_hash, published_at, edited_at,\n\
             deleted_at, external_urls_json, media_json, metadata_json)\n\
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17)\n\
             ON CONFLICT(source_instance_id, external_id) DO UPDATE SET\n\
             title=excluded.title, body_text=excluded.body_text, body_html=excluded.body_html,\n\
             author=excluded.author, source_url=excluded.source_url, content_hash=excluded.content_hash,\n\
             published_at=excluded.published_at, edited_at=excluded.edited_at,\n\
             deleted_at=excluded.deleted_at, external_urls_json=excluded.external_urls_json,\n\
             media_json=excluded.media_json, metadata_json=excluded.metadata_json",
            params![id, source_id, item.external_id, visibility_scope, item.item_kind, item.title,
                item.body_text, item.body_html, item.author, item.source_url, hash.as_slice(),
                item.published_at, item.edited_at, item.deleted_at,
                serde_json::to_string(&item.external_urls)?, serde_json::to_string(&item.media)?,
                serde_json::to_string(&item.metadata)?],
        )?;
        Ok(connection.query_row(
            "SELECT id FROM raw_items WHERE source_instance_id = ?1 AND external_id = ?2",
            params![source_id, item.external_id],
            |row| row.get(0),
        )?)
    }

    fn remove_deleted_item_content(&self, raw_item_id: &str) -> Result<(), IngestionError> {
        let mut connection = self.pool.connection()?;
        let transaction = connection.transaction()?;
        transaction.execute(
            "DELETE FROM collection_expansions WHERE parent_raw_item_id=?1",
            [raw_item_id],
        )?;
        let document_ids = {
            let mut statement = transaction.prepare(
                "SELECT DISTINCT document_id FROM document_curators WHERE raw_item_id=?1",
            )?;
            let rows = statement.query_map([raw_item_id], |row| row.get::<_, String>(0))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()?
        };
        transaction.execute(
            "DELETE FROM document_curators WHERE raw_item_id=?1",
            [raw_item_id],
        )?;
        for document_id in document_ids {
            transaction.execute(
                "DELETE FROM documents WHERE id=?1
                 AND NOT EXISTS (SELECT 1 FROM document_curators WHERE document_id=?1)",
                [document_id],
            )?;
        }
        transaction.execute(
            "DELETE FROM stories WHERE NOT EXISTS
             (SELECT 1 FROM story_memberships WHERE story_id=stories.id)",
            [],
        )?;
        transaction.commit()?;
        Ok(())
    }

    fn enqueue_raw_jobs(&self, raw_id: &str, item: &RawSourceItem) -> Result<(), IngestionError> {
        let checksum = Sha256::digest(serde_json::to_vec(item)?);
        let checksum = checksum
            .iter()
            .take(8)
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        for kind in [JobKind::DetectCollection, JobKind::NormalizeRawItem] {
            self.jobs.enqueue(
                kind,
                &json!({ "rawItemId": raw_id }),
                EnqueueOptions {
                    idempotency_key: Some(format!("{}:{raw_id}:{checksum}", kind.as_str())),
                    ..Default::default()
                },
            )?;
        }
        if let Some(url) = &item.source_url {
            self.jobs.enqueue(
                JobKind::ExtractArticle,
                &json!({ "rawItemId": raw_id, "url": url }),
                EnqueueOptions {
                    idempotency_key: Some(format!("ExtractArticle:{raw_id}:{checksum}")),
                    ..Default::default()
                },
            )?;
        }
        Ok(())
    }

    fn store_collection(&self, collection: CollectionStorage<'_>) -> Result<usize, IngestionError> {
        self.store_collection_as(collection)
    }

    fn store_collection_as(
        &self,
        collection: CollectionStorage<'_>,
    ) -> Result<usize, IngestionError> {
        let mut connection = self.pool.connection()?;
        let transaction = connection.transaction()?;
        let expansion_id = Uuid::new_v4().to_string();
        transaction.execute(
            "INSERT INTO collection_expansions(id, parent_raw_item_id, parser_kind, parser_version,\n\
             confidence, status, diagnostics_json, parent_display_policy)\n\
             VALUES (?1, ?2, ?3, ?4, ?5, 'expanded', ?6, ?7)\n\
             ON CONFLICT(parent_raw_item_id, parser_kind, parser_version) DO UPDATE SET\n\
             confidence=excluded.confidence, status='expanded',\n\
             diagnostics_json=excluded.diagnostics_json,\n\
             parent_display_policy=excluded.parent_display_policy",
            params![
                expansion_id,
                collection.raw_id,
                collection.parser_kind,
                collection.parser_version,
                collection.confidence,
                serde_json::to_string(collection.diagnostics)?,
                collection.display_policy.as_str()
            ],
        )?;
        let persisted_id: String = transaction.query_row(
            "SELECT id FROM collection_expansions WHERE parent_raw_item_id = ?1\n\
             AND parser_kind = ?2 AND parser_version = ?3",
            params![
                collection.raw_id,
                collection.parser_kind,
                collection.parser_version
            ],
            |row| row.get(0),
        )?;
        let mut inserted = 0;
        let mut pending_jobs = Vec::new();
        for entry in collection.entries {
            let ordinal = i64::try_from(entry.ordinal)
                .map_err(|_| IngestionError::Invalid("collection ordinal is too large".into()))?;
            let identity = derived_identity(&collection.parent.external_id, &entry.url)
                .map_err(IngestionError::Invalid)?;
            let changed = transaction.execute(
                "INSERT INTO collection_entries(id, expansion_id, parent_raw_item_id, ordinal, target_url,\n\
                 title_hint, summary_hint, author_hint, deterministic_key, commentary, extraction_method, confidence)\n\
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, NULL, ?7, ?8, ?9, ?10, ?11)\n\
                 ON CONFLICT(expansion_id, deterministic_key) DO UPDATE SET\n\
                 title_hint=excluded.title_hint, author_hint=excluded.author_hint,\n\
                 commentary=excluded.commentary, confidence=excluded.confidence",
                params![Uuid::new_v4().to_string(), persisted_id, collection.raw_id, ordinal, entry.url,
                    entry.title_hint, entry.author_hint, identity, entry.commentary,
                    collection.extraction_method, entry.confidence],
            )?;
            inserted += usize::from(changed == 1);
            pending_jobs.push((identity, entry.url.clone()));
        }
        if collection.display_policy == ParentDisplayPolicy::ChildrenOnly {
            let parent_document_ids = {
                let mut statement = transaction.prepare(
                    "SELECT DISTINCT document_id FROM document_curators
                     WHERE raw_item_id=?1 AND collection_entry_id IS NULL",
                )?;
                let rows = statement.query_map([collection.raw_id], |row| row.get::<_, String>(0))?;
                rows.collect::<rusqlite::Result<Vec<_>>>()?
            };
            transaction.execute(
                "DELETE FROM document_curators WHERE raw_item_id=?1 AND collection_entry_id IS NULL",
                [collection.raw_id],
            )?;
            for document_id in parent_document_ids {
                let orphaned: bool = transaction.query_row(
                    "SELECT NOT EXISTS(SELECT 1 FROM document_curators WHERE document_id=?1)",
                    [&document_id],
                    |row| row.get(0),
                )?;
                if orphaned {
                    transaction.execute(
                        "DELETE FROM jobs WHERE status='queued'
                         AND kind IN ('GenerateSummary', 'GenerateEmbedding', 'ClusterStory')
                         AND json_extract(payload_json, '$.documentId')=?1",
                        [&document_id],
                    )?;
                    transaction.execute("DELETE FROM documents WHERE id=?1", [&document_id])?;
                }
            }
            transaction.execute(
                "DELETE FROM stories WHERE NOT EXISTS
                 (SELECT 1 FROM story_memberships WHERE story_id=stories.id)",
                [],
            )?;
        }
        transaction.commit()?;
        drop(connection);
        if collection.display_policy == ParentDisplayPolicy::ParentOnly {
            return Ok(inserted);
        }
        for (identity, target_url) in pending_jobs {
            self.jobs.enqueue(
                JobKind::ProcessDerivedItem,
                &json!({
                    "sourceInstanceId": collection.source_id,
                    "parentRawItemId": collection.raw_id,
                    "targetUrl": target_url,
                    "derivedIdentity": identity
                }),
                EnqueueOptions {
                    idempotency_key: Some(format!("ProcessDerivedItem:{identity}")),
                    ..Default::default()
                },
            )?;
        }
        Ok(inserted)
    }

    fn record_collection_rejection(
        &self,
        raw_item_id: &str,
        display_policy: ParentDisplayPolicy,
        diagnostics: &Value,
    ) -> Result<(), IngestionError> {
        let diagnostics = serde_json::to_string(diagnostics)?;
        self.pool.with_connection(|connection| {
            connection.execute(
                "INSERT INTO collection_expansions(id, parent_raw_item_id, parser_kind,\n\
                 parser_version, confidence, status, diagnostics_json, parent_display_policy)\n\
                 VALUES (?1, ?2, 'deterministic', '1', 0, 'rejected', ?3, ?4)\n\
                 ON CONFLICT(parent_raw_item_id, parser_kind, parser_version) DO UPDATE SET\n\
                 confidence=0, status='rejected', diagnostics_json=excluded.diagnostics_json,\n\
                 parent_display_policy=excluded.parent_display_policy",
                params![
                    Uuid::new_v4().to_string(),
                    raw_item_id,
                    diagnostics,
                    display_policy.as_str()
                ],
            )
        })?;
        Ok(())
    }

    fn record_poll_success(
        &self,
        source_id: &str,
        cursor: Option<&Value>,
    ) -> Result<(), IngestionError> {
        let mut connection = self.pool.connection()?;
        let transaction = connection.transaction()?;
        if let Some(cursor) = cursor {
            transaction.execute(
                "INSERT INTO source_cursors(source_instance_id, cursor_kind, cursor_value)\n\
                 VALUES (?1, 'poll', ?2) ON CONFLICT(source_instance_id, cursor_kind) DO UPDATE SET\n\
                 cursor_value=excluded.cursor_value, updated_at=unixepoch()",
                params![source_id, serde_json::to_string(cursor)?],
            )?;
        }
        transaction.execute(
            "INSERT INTO source_health(source_instance_id, last_attempt_at, last_success_at, consecutive_failures)\n\
             VALUES (?1, unixepoch(), unixepoch(), 0) ON CONFLICT(source_instance_id) DO UPDATE SET\n\
             last_attempt_at=unixepoch(), last_success_at=unixepoch(), consecutive_failures=0,\n\
             last_error_class=NULL, last_error_message=NULL",
            [source_id],
        )?;
        transaction.commit()?;
        Ok(())
    }

    fn record_poll_failure(&self, source_id: &str, message: &str) -> Result<(), IngestionError> {
        self.pool.with_connection(|connection| {
            connection.execute(
            "INSERT INTO source_health(source_instance_id, last_attempt_at, consecutive_failures,\n\
             last_error_class, last_error_message) VALUES (?1, unixepoch(), 1, 'connector', ?2)\n\
             ON CONFLICT(source_instance_id) DO UPDATE SET last_attempt_at=unixepoch(),\n\
             consecutive_failures=consecutive_failures+1, last_error_class='connector',\n\
             last_error_message=excluded.last_error_message",
            params![source_id, truncate(message, 1_000)],
        )
        })?;
        Ok(())
    }
}
