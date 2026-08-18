struct RankingPersistence<'a> {
    user_id: &'a str,
    stream_id: &'a str,
    provider: &'a str,
    model: &'a str,
    version: &'a str,
    limit: usize,
    ui_mode: &'a str,
    stories: &'a [Candidate],
    replace_existing: bool,
}

impl IntelligenceService {
    pub async fn process_stream_embedding(
        &self,
        stream_id: &str,
        description: &str,
    ) -> Result<(), IntelligenceError> {
        let mut output = self
            .embedding
            .embed(&[EmbeddingInput {
                id: stream_id.to_owned(),
                text: description.to_owned(),
            }])
            .await?;
        let output = output.pop().ok_or_else(|| {
            IntelligenceError::Invalid("embedding provider returned no stream vector".into())
        })?;
        let identity = self.embedding.identity();
        let checksum = Sha256::digest(description.as_bytes());
        self.pool.with_connection(|connection| {
            connection.execute(
                "INSERT INTO embedding_records(id, provider, model, model_version, dimension,
                 input_checksum, entity_type, entity_id, vector_f32le)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'stream', ?7, ?8)
                 ON CONFLICT(provider, model, model_version, input_checksum, entity_type, entity_id)
                 DO UPDATE SET vector_f32le=excluded.vector_f32le, created_at=unixepoch()",
                params![
                    Uuid::new_v4().to_string(),
                    identity.provider,
                    identity.model,
                    identity.version,
                    i64::try_from(output.vector.len()).unwrap_or(i64::MAX),
                    checksum.as_slice(),
                    stream_id,
                    encode_vector(&output.vector)
                ],
            )
        })?;
        Ok(())
    }

    fn enqueue_stream_embedding(
        &self,
        stream_id: &str,
        description: &str,
    ) -> Result<(), IntelligenceError> {
        self.jobs.enqueue_coalesced_queued(
            JobKind::EmbedStream,
            &serde_json::to_value(StreamEmbeddingPayload {
                stream_id: stream_id.to_owned(),
                description: description.to_owned(),
            })?,
            EnqueueOptions {
                priority: -4,
                max_attempts: 3,
                ..Default::default()
            },
        )?;
        Ok(())
    }

    fn stream_vector(
        &self,
        stream_id: &str,
        identity: &ModelIdentity,
    ) -> Result<Option<Vec<f32>>, IntelligenceError> {
        let bytes: Option<Vec<u8>> = self.pool.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT vector_f32le FROM embedding_records WHERE entity_type='stream'
                     AND entity_id=?1 AND provider=?2 AND model=?3 AND model_version=?4
                     ORDER BY created_at DESC LIMIT 1",
                    params![
                        stream_id,
                        identity.provider,
                        identity.model,
                        identity.version
                    ],
                    |row| row.get(0),
                )
                .optional()
        })?;
        Ok(bytes.as_deref().and_then(decode_vector))
    }

    async fn apply_external_ranker(
        &self,
        user_id: &str,
        stream: &StreamView,
        candidates: &mut [Candidate],
        limit: usize,
        ui_mode: &str,
    ) -> Result<Option<ModelIdentity>, IntelligenceError> {
        let Some(provider) = &self.recommendation else {
            return Ok(None);
        };
        if candidates.is_empty() {
            return Ok(None);
        }
        let response = provider
            .rank(RankRequest {
                user_key: hashed_user_key(user_id),
                stream_slug: stream.slug.clone(),
                ranking_instruction: stream.ranking_instruction.clone(),
                candidates: candidates
                    .iter()
                    .take(64)
                    .map(|candidate| RankCandidate {
                        story_id: candidate.story_id.clone(),
                        title: candidate.title.clone(),
                        summary: candidate.summary.clone(),
                        topics: candidate.topics.clone(),
                        publisher: candidate.publisher.clone(),
                        freshness: candidate
                            .explanation
                            .get("freshness")
                            .and_then(Value::as_f64)
                            .unwrap_or_default() as f32,
                        coverage: candidate.coverage,
                        local_score: candidate.score,
                    })
                    .collect(),
                result_count: limit,
                ui_mode: ui_mode.to_owned(),
            })
            .await?;
        let scores = response
            .ranked
            .into_iter()
            .map(|ranked| (ranked.story_id, (ranked.score, ranked.features)))
            .collect::<HashMap<_, _>>();
        if scores.is_empty()
            || scores
                .keys()
                .any(|id| !candidates.iter().any(|c| &c.story_id == id))
        {
            return Err(IntelligenceError::Invalid(
                "recommender returned unknown or empty ranking".into(),
            ));
        }
        for candidate in candidates {
            if let Some((score, features)) = scores.get(&candidate.story_id) {
                candidate.score = *score;
                candidate.explanation["external"] = json!(features);
            }
        }
        Ok(Some(provider.identity()))
    }

    fn persist_ranking(&self, run: RankingPersistence<'_>) -> Result<(), IntelligenceError> {
        let mut connection = self.pool.connection()?;
        let transaction = connection.transaction()?;
        if run.replace_existing {
            transaction.execute(
                "DELETE FROM recommendation_runs WHERE user_id=?1 AND stream_id=?2
                 AND json_extract(config_json, '$.limit')=?3
                 AND json_extract(config_json, '$.uiMode')=?4",
                params![
                    run.user_id,
                    run.stream_id,
                    i64::try_from(run.limit).unwrap_or(i64::MAX),
                    run.ui_mode
                ],
            )?;
        }
        let run_id = Uuid::new_v4().to_string();
        transaction.execute(
            "INSERT INTO recommendation_runs(id, user_id, stream_id, provider, model, config_json)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![
                run_id,
                run.user_id,
                run.stream_id,
                run.provider,
                run.model,
                json!({"version": run.version, "limit": run.limit, "uiMode": run.ui_mode})
                    .to_string()
            ],
        )?;
        for (rank, story) in run.stories.iter().enumerate() {
            transaction.execute(
                "INSERT INTO recommendation_scores(run_id, story_id, document_id, score, rank,
                 explanation_json) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![
                    run_id,
                    story.story_id,
                    story.document_id,
                    story.score,
                    i64::try_from(rank).unwrap_or(i64::MAX),
                    story.explanation.to_string()
                ],
            )?;
        }
        transaction.commit()?;
        Ok(())
    }

    fn load_cached_ranking(
        &self,
        user_id: &str,
        stream_id: &str,
        limit: usize,
        ui_mode: &str,
        candidates: &[Candidate],
    ) -> Result<Option<Vec<RankedStory>>, IntelligenceError> {
        const CACHE_TTL_SECONDS: i64 = 60;

        let connection = self.pool.connection()?;
        let run_id = connection
            .query_row(
                "SELECT id FROM recommendation_runs
                 WHERE user_id=?1 AND stream_id=?2 AND created_at >= unixepoch() - ?3
                   AND json_extract(config_json, '$.limit')=?4
                   AND json_extract(config_json, '$.uiMode')=?5
                 ORDER BY created_at DESC LIMIT 1",
                params![
                    user_id,
                    stream_id,
                    CACHE_TTL_SECONDS,
                    i64::try_from(limit).unwrap_or(i64::MAX),
                    ui_mode
                ],
                |row| row.get::<_, String>(0),
            )
            .optional()?;
        let Some(run_id) = run_id else {
            return Ok(None);
        };
        let mut statement = connection.prepare(
            "SELECT story_id, document_id, score, explanation_json
             FROM recommendation_scores WHERE run_id=?1 ORDER BY rank",
        )?;
        let rows = statement.query_map([run_id], |row| {
            Ok((
                row.get::<_, String>(0)?,
                row.get::<_, Option<String>>(1)?,
                row.get::<_, f32>(2)?,
                row.get::<_, String>(3)?,
            ))
        })?;
        let mut ranked = Vec::new();
        for row in rows {
            let (story_id, document_id, score, explanation) = row?;
            let Some(candidate) = candidates
                .iter()
                .find(|candidate| candidate.story_id == story_id)
            else {
                return Ok(None);
            };
            if document_id.as_deref() != Some(candidate.document_id.as_str()) {
                return Ok(None);
            }
            ranked.push(RankedStory {
                story_id: candidate.story_id.clone(),
                document_id: candidate.document_id.clone(),
                title: candidate.title.clone(),
                summary: candidate.summary.clone(),
                canonical_url: candidate.canonical_url.clone(),
                publisher: candidate.publisher.clone(),
                published_at: candidate.published_at,
                coverage: candidate.coverage,
                topics: candidate.topics.clone(),
                score,
                explanation: serde_json::from_str(&explanation)?,
            });
        }
        if ranked.is_empty() && !candidates.is_empty() {
            return Ok(None);
        }
        Ok(Some(ranked))
    }
}
