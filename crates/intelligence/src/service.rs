impl IntelligenceService {
    pub fn new(
        pool: DbPool,
        embedding: Arc<dyn EmbeddingProvider>,
        summary: Arc<dyn SummaryProvider>,
        _recommendation: Option<Arc<dyn RecommendationProvider>>,
    ) -> Self {
        Self {
            jobs: JobQueue::new(pool.clone()),
            pool,
            embedding,
            summary,
            cluster_window_seconds: 72 * 60 * 60,
            cluster_threshold: 0.82,
            preference_refit_batch_size: 5,
            preference_fit_window: 500,
        }
    }

    pub fn configure_preference_model(mut self, refit_batch_size: usize, fit_window: usize) -> Self {
        self.preference_refit_batch_size = refit_batch_size.max(1);
        self.preference_fit_window = fit_window.max(self.preference_refit_batch_size);
        self
    }

    pub async fn process_summary(&self, document_id: &str) -> Result<(), IntelligenceError> {
        let document = self.load_document(document_id)?;
        let source_instructions = self.source_processing_instructions(document_id)?;
        let has_unprompted_source = source_instructions.iter().any(|(_, prompt)| prompt.is_empty());
        let request = |custom_instruction: Option<String>| SummaryRequest {
            title: document.title.clone(),
            source: document.publisher.clone(),
            author: document.author.clone(),
            canonical_url: document.canonical_url.clone(),
            language: document.language.clone(),
            text: bounded_text(&document.body_text, 32_000),
            custom_instruction,
        };
        let mut selected = if source_instructions.is_empty() || has_unprompted_source {
            Some(self.summary.summarize(request(None)).await?)
        } else {
            None
        };
        let mut inclusion = Vec::new();
        for (source_id, prompt) in source_instructions
            .iter()
            .filter(|(_, prompt)| !prompt.is_empty())
        {
            let response = self
                .summary
                .summarize(request(Some(prompt.clone())))
                .await?;
            inclusion.push((source_id.clone(), response.include));
            // ponytail: one document stores one summary; add per-source presentations if
            // conflicting translation prompts become common.
            if response.include && selected.is_none() {
                selected = Some(response);
            }
        }
        let mut connection = self.pool.connection()?;
        let transaction = connection.transaction()?;
        for (source_id, included) in inclusion {
            transaction.execute(
                "UPDATE document_curators SET included=?3
                 WHERE document_id=?1 AND source_instance_id=?2",
                params![document_id, source_id, included],
            )?;
        }
        let Some(response) = selected else {
            transaction.execute(
                "DELETE FROM summaries WHERE entity_type='document' AND entity_id=?1",
                [document_id],
            )?;
            transaction.execute(
                "DELETE FROM document_topics WHERE document_id=?1",
                [document_id],
            )?;
            transaction.commit()?;
            drop(connection);
            self.invalidate_recommendations(None)?;
            return Ok(());
        };
        let text = response
            .text
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        if text.is_empty() {
            return Err(IntelligenceError::Invalid(
                "summary provider returned empty text".into(),
            ));
        }
        let identity = self.summary.identity();
        let topics = normalize_topics(response.tags);
        transaction.execute(
                "INSERT INTO summaries(id, entity_type, entity_id, provider, model, model_version,
                 input_checksum, summary_text) VALUES (?1, 'document', ?2, ?3, ?4, ?5, ?6, ?7)
                 ON CONFLICT(entity_type, entity_id, provider, model, model_version, input_checksum)
                 DO UPDATE SET summary_text=excluded.summary_text, created_at=unixepoch()",
                params![
                    Uuid::new_v4().to_string(),
                    &document.id,
                    &identity.provider,
                    &identity.model,
                    &identity.version,
                    &document.input_checksum,
                    &text
                ],
            )?;
        transaction.execute(
            "DELETE FROM document_topics WHERE document_id=?1 AND provider=?2 AND model=?3
             AND model_version=?4",
            params![
                &document.id,
                &identity.provider,
                &identity.model,
                &identity.version
            ],
        )?;
        for topic in topics {
            transaction.execute(
                "INSERT INTO document_topics(document_id, topic, confidence, provider, model,
                 model_version, input_checksum) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params![
                    &document.id,
                    topic.label,
                    topic.confidence,
                    &identity.provider,
                    &identity.model,
                    &identity.version,
                    &document.input_checksum,
                ],
            )?;
        }
        transaction.commit()?;
        drop(connection);
        self.invalidate_recommendations(None)?;
        Ok(())
    }

    fn source_processing_instructions(
        &self,
        document_id: &str,
    ) -> Result<Vec<(String, String)>, IntelligenceError> {
        let connection = self.pool.connection()?;
        let mut statement = connection.prepare(
            "SELECT DISTINCT coalesce(dc.source_instance_id, ''),
             coalesce(si.processing_prompt, '')
             FROM document_curators dc
             LEFT JOIN source_instances si ON si.id=dc.source_instance_id
             WHERE dc.document_id=?1
             ORDER BY dc.source_instance_id",
        )?;
        let rows = statement.query_map([document_id], |row| Ok((row.get(0)?, row.get(1)?)))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub async fn process_embedding(&self, document_id: &str) -> Result<(), IntelligenceError> {
        let document = self.load_document(document_id)?;
        let input = format!(
            "{}\n\n{}",
            document.title,
            bounded_text(&document.body_text, 48_000)
        );
        let mut output = self
            .embedding
            .embed(&[EmbeddingInput {
                id: document.id.clone(),
                text: input,
            }])
            .await?;
        let output = output.pop().ok_or_else(|| {
            IntelligenceError::Invalid("embedding provider returned no vector".into())
        })?;
        if output.id != document.id || output.vector.is_empty() || output.vector.len() > 4096 {
            return Err(IntelligenceError::Invalid(
                "embedding provider returned an invalid vector".into(),
            ));
        }
        if output.vector.iter().any(|value| !value.is_finite()) {
            return Err(IntelligenceError::Invalid(
                "embedding vector contains non-finite values".into(),
            ));
        }
        let identity = self.embedding.identity();
        let vector_bytes = encode_vector(&output.vector);
        self.pool.with_connection(|connection| {
            connection.execute(
                "INSERT INTO embedding_records(id, provider, model, model_version, dimension,
                 input_checksum, entity_type, entity_id, vector_f32le)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'document', ?7, ?8)
                 ON CONFLICT(provider, model, model_version, input_checksum, entity_type, entity_id)
                 DO UPDATE SET vector_f32le=excluded.vector_f32le, dimension=excluded.dimension,
                 created_at=unixepoch()",
                params![
                    Uuid::new_v4().to_string(),
                    identity.provider,
                    identity.model,
                    identity.version,
                    i64::try_from(output.vector.len()).unwrap_or(i64::MAX),
                    document.input_checksum,
                    document.id,
                    vector_bytes
                ],
            )
        })?;
        clustering::cluster_document(self, &document, &output.vector, &identity)?;
        self.invalidate_recommendations(None)?;
        self.enqueue_preference_refits_for_document(&document.id);
        Ok(())
    }

    pub fn record_feedback(
        &self,
        user_id: &str,
        story_id: &str,
        feedback: FeedbackKind,
        source: &str,
    ) -> Result<String, IntelligenceError> {
        let document_id = self.representative_document_id(user_id, story_id)?;

        let mut connection = self.pool.connection()?;
        let transaction = connection.transaction()?;
        let (mut favorite, mut explicit): (bool, Option<String>) = transaction
            .query_row(
                "SELECT favorite, explicit_feedback FROM user_story_state
                 WHERE user_id = ?1 AND story_id = ?2",
                params![user_id, story_id],
                |row| Ok((row.get::<_, i64>(0)? != 0, row.get(1)?)),
            )
            .optional()?
            .unwrap_or((false, None));
        match feedback {
            FeedbackKind::Like => explicit = Some("like".into()),
            FeedbackKind::Dislike => explicit = Some("dislike".into()),
            FeedbackKind::Favorite => favorite = true,
            FeedbackKind::None => {
                favorite = false;
                explicit = None;
            }
        }
        let event_id = Uuid::new_v4().to_string();
        transaction.execute(
            "INSERT INTO feedback_events(id, user_id, story_id, document_id, feedback, source)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                event_id,
                user_id,
                story_id,
                document_id,
                feedback.as_str(),
                source
            ],
        )?;
        transaction.execute(
            "INSERT INTO user_story_state(user_id, story_id, favorite, explicit_feedback)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(user_id, story_id) DO UPDATE SET favorite=excluded.favorite,
             explicit_feedback=excluded.explicit_feedback, updated_at=unixepoch()",
            params![user_id, story_id, favorite, explicit],
        )?;
        self.insert_affinity_events(&transaction, user_id, story_id, &document_id, feedback)?;
        transaction.execute(
            "DELETE FROM recommendation_runs WHERE user_id=?1",
            [user_id],
        )?;
        transaction.commit()?;
        drop(connection);

        if matches!(feedback, FeedbackKind::Like | FeedbackKind::Dislike)
            && let Err(error) = self.enqueue_preference_refit_if_due(user_id)
        {
            warn!(error = %error, %user_id, "preference refit could not be queued");
        }
        Ok(event_id)
    }

    pub fn manual_merge(
        &self,
        actor_user_id: &str,
        document_id: &str,
        target_story_id: &str,
    ) -> Result<(), IntelligenceError> {
        clustering::manual_merge(self, actor_user_id, document_id, target_story_id)
    }

    pub fn manual_split(
        &self,
        actor_user_id: &str,
        document_id: &str,
    ) -> Result<String, IntelligenceError> {
        clustering::manual_split(self, actor_user_id, document_id)
    }

    pub fn invalidate_recommendations(
        &self,
        user_id: Option<&str>,
    ) -> Result<usize, IntelligenceError> {
        let changed = self.pool.with_connection(|connection| match user_id {
            Some(user_id) => connection.execute(
                "DELETE FROM recommendation_runs WHERE user_id=?1",
                [user_id],
            ),
            None => connection.execute("DELETE FROM recommendation_runs", []),
        })?;
        Ok(changed)
    }

    pub fn enqueue_reembedding(
        &self,
        document_id: Option<&str>,
    ) -> Result<usize, IntelligenceError> {
        let identity = self.embedding.identity();
        let connection = self.pool.connection()?;
        let mut statement = connection.prepare(
            "SELECT d.id, d.exact_content_hash FROM documents d
             WHERE (?1 IS NULL OR d.id=?1)
               AND NOT EXISTS(
                 SELECT 1 FROM embedding_records er WHERE er.entity_type='document'
                   AND er.entity_id=d.id AND er.provider=?2 AND er.model=?3
                   AND er.model_version=?4 AND er.input_checksum=d.exact_content_hash
               )
             ORDER BY d.created_at LIMIT 1000",
        )?;
        let rows = statement.query_map(
            params![
                document_id,
                identity.provider,
                identity.model,
                identity.version
            ],
            |row| Ok((row.get::<_, String>(0)?, row.get::<_, Vec<u8>>(1)?)),
        )?;
        let documents = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        drop(statement);
        drop(connection);
        for (document_id, checksum) in &documents {
            let checksum = checksum
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>();
            self.jobs.enqueue(
                JobKind::GenerateEmbedding,
                &serde_json::to_value(DocumentJobPayload {
                    document_id: document_id.clone(),
                })?,
                EnqueueOptions {
                    idempotency_key: Some(format!(
                        "GenerateEmbedding:{document_id}:{}:{}:{}:{checksum}",
                        identity.provider, identity.model, identity.version
                    )),
                    ..Default::default()
                },
            )?;
        }
        Ok(documents.len())
    }

    fn insert_affinity_events(
        &self,
        transaction: &rusqlite::Transaction<'_>,
        user_id: &str,
        story_id: &str,
        document_id: &str,
        feedback: FeedbackKind,
    ) -> rusqlite::Result<()> {
        let base = match feedback {
            FeedbackKind::Like => 1.0,
            FeedbackKind::Dislike => -1.0,
            FeedbackKind::Favorite => 2.0,
            FeedbackKind::None => 0.0,
        };
        if base == 0.0 {
            return Ok(());
        }
        let (publisher, curated): (Option<String>, bool) = transaction.query_row(
            "SELECT d.publisher, EXISTS(SELECT 1 FROM document_curators dc
             WHERE dc.document_id = d.id AND dc.included=1 AND dc.collection_entry_id IS NOT NULL
               AND (dc.source_instance_id IS NULL OR EXISTS (
                 SELECT 1 FROM source_access sa WHERE sa.source_instance_id=dc.source_instance_id
                   AND (sa.user_id IS NULL OR sa.user_id=?2))))
             FROM documents d WHERE d.id = ?1",
            params![document_id, user_id],
            |row| Ok((row.get(0)?, row.get::<_, i64>(1)? != 0)),
        )?;
        let signal = feedback.as_str();
        if let Some(publisher) = publisher {
            insert_affinity(
                transaction,
                user_id,
                "publisher",
                &publisher,
                signal,
                if curated { base * 0.25 } else { base },
                story_id,
            )?;
        }
        let mut statement = transaction.prepare(
            "SELECT curator_id, source_instance_id FROM document_curators
             WHERE document_id = ?1 AND included=1 AND (source_instance_id IS NULL OR EXISTS (
               SELECT 1 FROM source_access sa WHERE sa.source_instance_id=document_curators.source_instance_id
                 AND (sa.user_id IS NULL OR sa.user_id=?2)))",
        )?;
        let rows = statement.query_map(params![document_id, user_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, Option<String>>(1)?))
        })?;
        for row in rows {
            let (curator, source_id) = row?;
            insert_affinity(
                transaction,
                user_id,
                "curator",
                &curator,
                signal,
                base,
                story_id,
            )?;
            if let Some(source_id) = source_id {
                insert_affinity(
                    transaction,
                    user_id,
                    "source",
                    &source_id,
                    signal,
                    base,
                    story_id,
                )?;
            }
        }
        Ok(())
    }

    pub(crate) fn load_document(
        &self,
        document_id: &str,
    ) -> Result<StoredDocument, IntelligenceError> {
        let document = self
            .pool
            .with_connection(|connection| {
                connection
                    .query_row(
                        "SELECT id, visibility_scope, title, body_text, author, publisher,
                         canonical_url, language, published_at, exact_content_hash
                         FROM documents WHERE id = ?1",
                        [document_id],
                        |row| {
                            Ok(StoredDocument {
                                id: row.get(0)?,
                                visibility_scope: row.get(1)?,
                                title: row.get(2)?,
                                body_text: row.get(3)?,
                                author: row.get(4)?,
                                publisher: row.get(5)?,
                                canonical_url: row.get(6)?,
                                language: row.get(7)?,
                                published_at: row.get(8)?,
                                input_checksum: row.get(9)?,
                            })
                        },
                    )
                    .optional()
            })?
            .ok_or(IntelligenceError::NotFound)?;
        Ok(document)
    }
}

fn normalize_topics(topics: Vec<rill_model_api::TopicTag>) -> Vec<rill_model_api::TopicTag> {
    let mut seen = std::collections::HashSet::new();
    topics
        .into_iter()
        .filter_map(|topic| {
            let label = topic
                .label
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase();
            (label.chars().count() >= 2
                && label.chars().count() <= 40
                && topic.confidence.is_finite()
                && (0.0..=1.0).contains(&topic.confidence)
                && seen.insert(label.clone()))
            .then_some(rill_model_api::TopicTag {
                label,
                confidence: topic.confidence,
            })
        })
        .take(8)
        .collect()
}
