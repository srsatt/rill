impl IngestionService {
    pub fn normalize_raw_item(&self, raw_item_id: &str) -> Result<bool, IngestionError> {
        if self.collection_parent_hidden(raw_item_id)? {
            return Ok(false);
        }
        let (source_id, visibility_scope, item) = self.load_raw_item(raw_item_id)?;
        if item.deleted_at.is_some() {
            return Ok(false);
        }
        let Some(document) = normalize_feed_item(&item, &visibility_scope)? else {
            return Ok(false);
        };
        let result = self.dedup.upsert_document(
            &document,
            &CuratorProvenance {
                curator_kind: "source".into(),
                curator_id: source_id.clone(),
                source_instance_id: Some(source_id),
                raw_item_id: Some(raw_item_id.to_owned()),
                collection_entry_id: None,
            },
        )?;
        self.enqueue_intelligence(&result.document_id, &document)?;
        Ok(result.created)
    }

    pub fn detect_raw_collection(&self, raw_item_id: &str) -> Result<usize, IngestionError> {
        let (source_id, _, item) = self.load_raw_item(raw_item_id)?;
        if item.deleted_at.is_some() {
            return Ok(0);
        }
        let (policy, source_mode) = self.collection_settings_for_source(&source_id)?;
        let mode = self
            .manual_detection_mode(raw_item_id)?
            .unwrap_or(source_mode);
        let base_url = item
            .source_url
            .as_deref()
            .and_then(|url| Url::parse(url).ok());
        let detection = detect_collection_with_diagnostics(&item, base_url.as_ref(), mode, &policy);
        let diagnostics = json!({
            "mode": detection.mode,
            "ignoredLinks": detection.ignored_links,
        });
        match detection.shape {
            ItemShape::Collection {
                confidence,
                entries,
            } => self.store_collection(CollectionStorage {
                source_id: &source_id,
                raw_id: raw_item_id,
                parent: &item,
                confidence,
                entries: &entries,
                display_policy: policy.parent_display_policy,
                diagnostics: &diagnostics,
                parser_kind: "deterministic",
                parser_version: "1",
                extraction_method: "deterministic",
            }),
            ItemShape::Single => {
                self.record_collection_rejection(
                    raw_item_id,
                    policy.parent_display_policy,
                    &diagnostics,
                )?;
                Ok(0)
            }
        }
    }

    pub async fn detect_raw_collection_with_provider(
        &self,
        raw_item_id: &str,
    ) -> Result<usize, IngestionError> {
        let deterministic_count = self.detect_raw_collection(raw_item_id)?;
        let Some(provider) = self.collection_parser.as_ref() else {
            return Ok(deterministic_count);
        };
        if deterministic_count > 0 {
            return Ok(deterministic_count);
        }
        let (source_id, _, item) = self.load_raw_item(raw_item_id)?;
        if item.deleted_at.is_some() {
            return Ok(0);
        }
        let (policy, source_mode) = self.collection_settings_for_source(&source_id)?;
        let mode = self
            .manual_detection_mode(raw_item_id)?
            .unwrap_or(source_mode);
        if mode != DetectionMode::Auto {
            return Ok(0);
        }
        let base_url = item
            .source_url
            .as_deref()
            .and_then(|url| Url::parse(url).ok());
        let request = provider_request(&item, base_url.as_ref(), &policy);
        if request.allowed_urls.len() < 2 {
            return Ok(0);
        }
        let response = provider.parse_collection(request.clone()).await?;
        let response = validate_provider_response(&request, response, policy.maximum_fan_out)?;
        if !response.is_collection
            || response.confidence < policy.threshold
            || response.entries.len() < 2
        {
            return Ok(0);
        }
        let entries = response
            .entries
            .into_iter()
            .enumerate()
            .map(|(ordinal, entry)| rill_domain::CollectionEntryCandidate {
                url: entry.url,
                title_hint: entry.title_hint,
                commentary: entry.commentary,
                author_hint: entry.author_hint,
                published_at_hint: item.published_at,
                ordinal,
                confidence: entry.confidence,
            })
            .collect::<Vec<_>>();
        let identity = provider.identity();
        let diagnostics = json!({
            "mode": "model-assisted",
            "provider": identity.provider,
            "model": identity.model,
            "modelVersion": identity.version,
            "allowedUrlCount": request.allowed_urls.len(),
        });
        let parser_version = format!("{}:{}", identity.model, identity.version);
        self.store_collection_as(CollectionStorage {
            source_id: &source_id,
            raw_id: raw_item_id,
            parent: &item,
            confidence: response.confidence,
            entries: &entries,
            display_policy: policy.parent_display_policy,
            diagnostics: &diagnostics,
            parser_kind: "model",
            parser_version: &parser_version,
            extraction_method: "model",
        })
    }

    pub fn search(
        &self,
        user_id: &str,
        query: &str,
        limit: usize,
    ) -> Result<Vec<SearchHit>, IngestionError> {
        let fts_query = safe_fts_query(query)?;
        let private_scope = format!("user:{user_id}");
        let sql_limit = i64::try_from(limit.min(100)).unwrap_or(100);
        let connection = self.pool.connection()?;
        let mut statement = connection.prepare(
            "SELECT sm.story_id, d.id, d.title,\n\
             snippet(documents_fts, 2, '<mark>', '</mark>', ' … ', 24),\n\
             d.canonical_url, d.publisher, d.published_at\n\
             FROM documents_fts\n\
             JOIN documents d ON d.id = documents_fts.document_id\n\
             JOIN story_memberships sm ON sm.document_id = d.id\n\
             WHERE documents_fts MATCH ?1 AND (d.visibility_scope = ?2 OR EXISTS (\n\
               SELECT 1 FROM document_access da WHERE da.document_id=d.id\n\
                 AND (da.user_id IS NULL OR da.user_id=?3)))\n\
             ORDER BY bm25(documents_fts), coalesce(d.published_at, d.created_at) DESC LIMIT ?4",
        )?;
        let rows = statement.query_map(params![fts_query, private_scope, user_id, sql_limit], |row| {
            Ok(SearchHit {
                story_id: row.get(0)?,
                document_id: row.get(1)?,
                title: row.get(2)?,
                excerpt: row.get(3)?,
                canonical_url: row.get(4)?,
                publisher: row.get(5)?,
                published_at: row.get(6)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn latest_feed(
        &self,
        user_id: &str,
        limit: usize,
    ) -> Result<Vec<SearchHit>, IngestionError> {
        let private_scope = format!("user:{user_id}");
        let sql_limit = i64::try_from(limit.min(100)).unwrap_or(100);
        let connection = self.pool.connection()?;
        let mut statement = connection.prepare(
            "SELECT sm.story_id, d.id, d.title, substr(d.body_text, 1, 500), d.canonical_url,\n\
             d.publisher, d.published_at FROM documents d\n\
             JOIN story_memberships sm ON sm.document_id = d.id\n\
             WHERE d.visibility_scope = ?1 OR EXISTS (\n\
               SELECT 1 FROM document_access da WHERE da.document_id=d.id\n\
                 AND (da.user_id IS NULL OR da.user_id=?2))\n\
             ORDER BY coalesce(d.published_at, d.created_at) DESC, d.id LIMIT ?3",
        )?;
        let rows = statement.query_map(params![private_scope, user_id, sql_limit], |row| {
            Ok(SearchHit {
                story_id: row.get(0)?,
                document_id: row.get(1)?,
                title: row.get(2)?,
                excerpt: row.get(3)?,
                canonical_url: row.get(4)?,
                publisher: row.get(5)?,
                published_at: row.get(6)?,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn raw_visibility_scope(&self, raw_item_id: &str) -> Result<String, IngestionError> {
        Ok(self.pool.with_connection(|connection| {
            connection.query_row(
                "SELECT visibility_scope FROM raw_items WHERE id = ?1",
                [raw_item_id],
                |row| row.get(0),
            )
        })?)
    }

    pub fn process_extracted_article(
        &self,
        raw_item_id: &str,
        article: &ExtractedArticle,
    ) -> Result<(), IngestionError> {
        if self.collection_parent_hidden(raw_item_id)? {
            return Ok(());
        }
        let connection = self.pool.connection()?;
        let (source_id, visibility_scope): (String, String) = connection.query_row(
            "SELECT source_instance_id, visibility_scope FROM raw_items WHERE id = ?1",
            [raw_item_id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )?;
        drop(connection);
        if article.document.visibility_scope != visibility_scope {
            return Err(IngestionError::Invalid(
                "extracted article visibility does not match raw item".into(),
            ));
        }
        let result = self.dedup.upsert_document(
            &article.document,
            &CuratorProvenance {
                curator_kind: "source".into(),
                curator_id: source_id.clone(),
                source_instance_id: Some(source_id),
                raw_item_id: Some(raw_item_id.to_owned()),
                collection_entry_id: None,
            },
        )?;
        self.replace_media(&result.document_id, article)?;
        self.enqueue_intelligence(&result.document_id, &article.document)?;
        Ok(())
    }

    fn collection_parent_hidden(&self, raw_item_id: &str) -> Result<bool, IngestionError> {
        Ok(self.pool.with_connection(|connection| {
            connection.query_row(
                "SELECT EXISTS(SELECT 1 FROM collection_expansions
                 WHERE parent_raw_item_id=?1 AND status='expanded'
                   AND parent_display_policy='children_only')",
                [raw_item_id],
                |row| row.get(0),
            )
        })?)
    }

    pub fn process_derived_article(
        &self,
        payload: &ProcessDerivedPayload,
        article: &ExtractedArticle,
    ) -> Result<(), IngestionError> {
        let (visibility_scope, curator_id) = self.source_identity(&payload.source_instance_id)?;
        if article.document.visibility_scope != visibility_scope {
            return Err(IngestionError::Invalid(
                "derived article visibility does not match source".into(),
            ));
        }
        let raw = RawSourceItem {
            external_id: payload.derived_identity.clone(),
            item_kind: "derived-article".into(),
            title: Some(article.document.title.clone()),
            body_text: Some(article.document.body_text.clone()),
            body_html: article.document.sanitized_html.clone(),
            author: article.document.author.clone(),
            source_url: Some(payload.target_url.clone()),
            published_at: article.document.published_at,
            edited_at: None,
            deleted_at: None,
            external_urls: vec![payload.target_url.clone()],
            media: Vec::new(),
            metadata: json!({ "parentRawItemId": payload.parent_raw_item_id }),
        };
        let raw_id = self.upsert_raw_item(&payload.source_instance_id, &visibility_scope, &raw)?;
        let connection = self.pool.connection()?;
        let entry_id: String = connection.query_row(
            "SELECT id FROM collection_entries WHERE parent_raw_item_id = ?1 AND deterministic_key = ?2",
            params![payload.parent_raw_item_id, payload.derived_identity],
            |row| row.get(0),
        )?;
        connection.execute(
            "UPDATE collection_entries SET derived_raw_item_id = ?2 WHERE id = ?1",
            params![entry_id, raw_id],
        )?;
        drop(connection);
        let result = self.dedup.upsert_document(
            &article.document,
            &CuratorProvenance {
                curator_kind: "source".into(),
                curator_id,
                source_instance_id: Some(payload.source_instance_id.clone()),
                raw_item_id: Some(raw_id),
                collection_entry_id: Some(entry_id),
            },
        )?;
        self.replace_media(&result.document_id, article)?;
        self.enqueue_intelligence(&result.document_id, &article.document)?;
        Ok(())
    }
}
