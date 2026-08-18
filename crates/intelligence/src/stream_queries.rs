impl IntelligenceService {
    pub fn ensure_home_stream(&self, user_id: &str) -> Result<String, IntelligenceError> {
        let defaults = [
            (
                "Home",
                "home",
                0,
                "Everything you follow, balanced across subjects and sources.",
                serde_json::json!({}),
                "Balance relevance, freshness, source affinity, and variety.",
            ),
            (
                "Technology",
                "technology",
                1,
                "Software, programming, security, infrastructure, devices, and the technology industry.",
                serde_json::json!({"includeTopics":["technology","software","programming","security","databases","rust","javascript","hardware"]}),
                "Prefer concrete engineering detail, useful tools, and measured results.",
            ),
            (
                "AI",
                "ai",
                2,
                "Artificial intelligence research, models, products, agents, and their social impact.",
                serde_json::json!({"includeTopics":["ai","artificial intelligence","machine learning","generative ai","llm","agents"]}),
                "Prefer substantive releases, research, evaluations, and practical implementation details over hype.",
            ),
            (
                "World",
                "world",
                3,
                "International affairs, politics, economics, cities, and major events around the world.",
                serde_json::json!({"includeTopics":["world","politics","international","economics","europe","germany","ukraine"]}),
                "Prefer consequential reporting and diverse geographic coverage.",
            ),
            (
                "Science",
                "science",
                4,
                "Science, health, climate, space, and peer-reviewed research.",
                serde_json::json!({"includeTopics":["science","research","health","climate","space","biology","physics"]}),
                "Prefer primary research, careful evidence, and clear explanations.",
            ),
        ];
        let mut pending_embeddings = Vec::new();
        let mut connection = self.pool.connection()?;
        let transaction = connection.transaction()?;
        let seeded: bool = transaction.query_row(
            "SELECT EXISTS(SELECT 1 FROM user_product_state
             WHERE user_id=?1 AND default_streams_seeded=1)",
            [user_id],
            |row| row.get(0),
        )?;
        if !seeded {
            for (name, slug, position, description, filter, ranking) in defaults {
                let id = Uuid::new_v4().to_string();
                let changed = transaction.execute(
                    "INSERT INTO streams(id, owner_user_id, name, slug, definition_text,
                     ranking_instruction, sort_order, filter_json)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
                     ON CONFLICT(owner_user_id, slug) DO NOTHING",
                    params![
                        id,
                        user_id,
                        name,
                        slug,
                        description,
                        ranking,
                        position,
                        filter.to_string()
                    ],
                )?;
                if changed == 1 {
                    pending_embeddings.push((id, description));
                }
            }
            transaction.execute(
                "INSERT INTO user_product_state(user_id, default_streams_seeded)
                 VALUES (?1, 1) ON CONFLICT(user_id) DO UPDATE SET
                 default_streams_seeded=1, updated_at=unixepoch()",
                [user_id],
            )?;
        }
        let home = transaction.query_row(
            "SELECT id FROM streams WHERE owner_user_id = ?1 AND slug = 'home'",
            [user_id],
            |row| row.get(0),
        )?;
        transaction.commit()?;
        drop(connection);
        for (stream_id, description) in pending_embeddings {
            if let Err(error) = self.enqueue_stream_embedding(&stream_id, description) {
                warn!(error = %error, %stream_id, "default stream embedding could not be queued");
            }
        }
        Ok(home)
    }

    pub async fn create_stream(
        &self,
        user_id: &str,
        input: &CreateStreamInput,
    ) -> Result<StreamView, IntelligenceError> {
        let CreateStreamInput {
            name,
            slug,
            icon,
            filter,
            semantic_description,
            ranking_instruction,
        } = input;
        validate_stream(name, slug)?;
        let id = Uuid::new_v4().to_string();
        let position: i32 = self.pool.with_connection(|connection| {
            connection.query_row(
                "SELECT coalesce(max(sort_order), -1) + 1 FROM streams WHERE owner_user_id = ?1",
                [user_id],
                |row| row.get(0),
            )
        })?;
        self.pool.with_connection(|connection| {
            connection.execute(
                "INSERT INTO streams(id, owner_user_id, name, slug, definition_text, sort_order,
                 icon, filter_json, ranking_instruction) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    id,
                    user_id,
                    name.trim(),
                    slug,
                    semantic_description.as_deref(),
                    position,
                    icon.as_deref(),
                    serde_json::to_string(filter).map_err(|error| {
                        rusqlite::Error::ToSqlConversionFailure(Box::new(error))
                    })?,
                    ranking_instruction.as_deref()
                ],
            )
        })?;
        if let Some(description) = semantic_description
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            && let Err(error) = self.enqueue_stream_embedding(&id, description)
        {
            warn!(error = %error, stream_id = %id, "semantic stream embedding could not be queued");
        }
        Ok(StreamView {
            id,
            name: name.trim().to_owned(),
            slug: slug.to_owned(),
            icon: icon.clone(),
            position,
            semantic_description: semantic_description.clone(),
            ranking_instruction: ranking_instruction.clone(),
            filter: filter.clone(),
        })
    }

    pub fn list_streams(&self, user_id: &str) -> Result<Vec<StreamView>, IntelligenceError> {
        self.ensure_home_stream(user_id)?;
        let connection = self.pool.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, name, slug, icon, sort_order, definition_text, ranking_instruction,
             filter_json FROM streams WHERE owner_user_id = ?1 AND enabled = 1
             ORDER BY sort_order, name COLLATE NOCASE",
        )?;
        let rows = statement.query_map([user_id], |row| {
            let filter_json: String = row.get(7)?;
            let filter = serde_json::from_str(&filter_json).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    filter_json.len(),
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?;
            Ok(StreamView {
                id: row.get(0)?,
                name: row.get(1)?,
                slug: row.get(2)?,
                icon: row.get(3)?,
                position: row.get(4)?,
                semantic_description: row.get(5)?,
                ranking_instruction: row.get(6)?,
                filter,
            })
        })?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub async fn rank_stream(
        &self,
        user_id: &str,
        slug: &str,
        limit: usize,
        ui_mode: &str,
    ) -> Result<Vec<RankedStory>, IntelligenceError> {
        self.rank_stream_now(user_id, slug, limit, ui_mode)
    }

    pub fn rank_stream_now(
        &self,
        user_id: &str,
        slug: &str,
        limit: usize,
        ui_mode: &str,
    ) -> Result<Vec<RankedStory>, IntelligenceError> {
        self.ensure_home_stream(user_id)?;
        let stream = self.load_stream(user_id, slug)?;
        let identity = self.embedding.identity();
        let mut candidates = self.load_candidates(user_id, &identity)?;
        candidates.retain(|candidate| matches_filter(candidate, &stream.filter));
        if let Some(cached) =
            self.load_cached_ranking(user_id, &stream.id, limit, ui_mode, &candidates)?
        {
            return Ok(cached);
        }
        let (positive, negative) = self.preference_centroids(user_id, &identity)?;
        let preference = self.preference_model(user_id, &identity)?;
        let stream_vector = self.stream_vector(&stream.id, &identity)?;
        for candidate in &mut candidates {
            score_candidate(
                candidate,
                user_id,
                &self.pool,
                positive.as_deref(),
                negative.as_deref(),
                stream_vector.as_deref(),
                preference.as_ref(),
            )?;
        }
        let selected = diversify(candidates, limit.min(100), user_id, slug);
        self.persist_ranking(RankingPersistence {
            user_id,
            stream_id: &stream.id,
            provider: "rill-local",
            model: if preference.is_some() {
                "preference-logistic"
            } else {
                "fallback-ranker"
            },
            version: "1",
            limit,
            ui_mode,
            stories: &selected,
            replace_existing: false,
        })?;
        Ok(into_ranked_stories(selected))
    }

    fn load_stream(&self, user_id: &str, slug: &str) -> Result<StreamView, IntelligenceError> {
        let connection = self.pool.connection()?;
        connection
            .query_row(
                "SELECT id, name, slug, icon, sort_order, definition_text, ranking_instruction,
                 filter_json FROM streams WHERE owner_user_id = ?1 AND slug = ?2 AND enabled = 1",
                params![user_id, slug],
                |row| {
                    let raw: String = row.get(7)?;
                    let filter = serde_json::from_str(&raw).map_err(|error| {
                        rusqlite::Error::FromSqlConversionFailure(
                            raw.len(),
                            rusqlite::types::Type::Text,
                            Box::new(error),
                        )
                    })?;
                    Ok(StreamView {
                        id: row.get(0)?,
                        name: row.get(1)?,
                        slug: row.get(2)?,
                        icon: row.get(3)?,
                        position: row.get(4)?,
                        semantic_description: row.get(5)?,
                        ranking_instruction: row.get(6)?,
                        filter,
                    })
                },
            )
            .optional()?
            .ok_or(IntelligenceError::NotFound)
    }

    fn load_candidates(
        &self,
        user_id: &str,
        identity: &ModelIdentity,
    ) -> Result<Vec<Candidate>, IntelligenceError> {
        let private_scope = format!("user:{user_id}");
        let connection = self.pool.connection()?;
        let mut statement = connection.prepare(
            "SELECT s.id, d.id, d.title,
             coalesce((SELECT su.summary_text FROM summaries su WHERE su.entity_type='document'
               AND su.entity_id=d.id AND su.input_checksum=d.exact_content_hash
               ORDER BY su.created_at DESC LIMIT 1), substr(d.body_text, 1, 600)),
             d.canonical_url, d.publisher, d.language, d.published_at,
             (SELECT count(*) FROM story_memberships scm WHERE scm.story_id=s.id),
             coalesce((SELECT group_concat(DISTINCT dc.source_instance_id) FROM document_curators dc
               WHERE dc.document_id=d.id), ''),
             coalesce((SELECT group_concat(DISTINCT dc.curator_id) FROM document_curators dc
               WHERE dc.document_id=d.id), ''),
             uss.read_at IS NOT NULL, coalesce(uss.favorite, 0), er.vector_f32le
             FROM stories s JOIN documents d ON d.id=s.anchor_document_id
             LEFT JOIN user_story_state uss ON uss.user_id=?1 AND uss.story_id=s.id
             LEFT JOIN embedding_records er ON er.id=(SELECT latest.id FROM embedding_records latest
               WHERE latest.entity_type='document' AND latest.entity_id=d.id
               AND latest.provider=?3 AND latest.model=?4 AND latest.model_version=?5
               ORDER BY latest.created_at DESC LIMIT 1)
             WHERE uss.hidden_at IS NULL AND EXISTS (
               SELECT 1 FROM story_memberships visible_sm
               JOIN documents visible_d ON visible_d.id=visible_sm.document_id
               WHERE visible_sm.story_id=s.id AND (visible_d.visibility_scope=?2 OR EXISTS (
                 SELECT 1 FROM document_access da WHERE da.document_id=visible_d.id
                   AND (da.user_id IS NULL OR da.user_id=?1)))
             )
             ORDER BY coalesce(d.published_at, d.created_at) DESC LIMIT 500",
        )?;
        let rows = statement.query_map(
            params![
                user_id,
                private_scope,
                identity.provider,
                identity.model,
                identity.version,
            ],
            |row| {
                let bytes: Option<Vec<u8>> = row.get(13)?;
                Ok(Candidate {
                    story_id: row.get(0)?,
                    document_id: row.get(1)?,
                    title: row.get(2)?,
                    summary: row.get(3)?,
                    canonical_url: row.get(4)?,
                    publisher: row.get(5)?,
                    language: row.get(6)?,
                    published_at: row.get(7)?,
                    coverage: row.get(8)?,
                    topics: Vec::new(),
                    sources: comma_list(row.get(9)?),
                    curators: comma_list(row.get(10)?),
                    read: row.get::<_, i64>(11)? != 0,
                    favorite: row.get::<_, i64>(12)? != 0,
                    vector: bytes.as_deref().and_then(decode_vector),
                    score: 0.0,
                    explanation: json!({}),
                })
            },
        )?;
        let mut candidates = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        drop(statement);
        drop(connection);
        for candidate in &mut candidates {
            let detail = self.story_detail(user_id, &candidate.story_id)?;
            let representative = detail.representative;
            candidate.document_id = representative.document_id;
            candidate.title = representative.title;
            candidate.summary = representative.summary;
            candidate.canonical_url = representative.canonical_url;
            candidate.publisher = representative.publisher;
            candidate.language = representative.language;
            candidate.published_at = representative.published_at;
            candidate.coverage = detail.coverage_count;
            candidate.topics = self.story_topics(&candidate.story_id)?;
            candidate.sources = representative
                .curators
                .iter()
                .filter_map(|path| path.source_instance_id.clone())
                .collect();
            candidate.curators = representative
                .curators
                .iter()
                .map(|path| path.curator_id.clone())
                .collect();
            candidate.vector = self.document_vector(&candidate.document_id, identity)?;
        }
        Ok(candidates)
    }

    pub(crate) fn story_topics(&self, story_id: &str) -> Result<Vec<String>, IntelligenceError> {
        let connection = self.pool.connection()?;
        let mut statement = connection.prepare(
            "SELECT topic FROM story_topics WHERE story_id=?1
             ORDER BY confidence DESC, topic COLLATE NOCASE LIMIT 8",
        )?;
        let rows = statement.query_map([story_id], |row| row.get(0))?;
        Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    fn document_vector(
        &self,
        document_id: &str,
        identity: &ModelIdentity,
    ) -> Result<Option<Vec<f32>>, IntelligenceError> {
        let bytes: Option<Vec<u8>> = self.pool.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT vector_f32le FROM embedding_records WHERE entity_type='document'
                     AND entity_id=?1 AND provider=?2 AND model=?3 AND model_version=?4
                     ORDER BY created_at DESC LIMIT 1",
                    params![
                        document_id,
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

    fn preference_centroids(
        &self,
        user_id: &str,
        identity: &ModelIdentity,
    ) -> Result<PreferenceCentroids, IntelligenceError> {
        let connection = self.pool.connection()?;
        let mut statement = connection.prepare(
            "SELECT uss.explicit_feedback, uss.favorite, er.vector_f32le
             FROM user_story_state uss JOIN stories s ON s.id=uss.story_id
             JOIN embedding_records er ON er.entity_type='document'
               AND er.entity_id=coalesce(uss.selected_document_id, s.anchor_document_id)
             WHERE uss.user_id=?1 AND (uss.explicit_feedback IS NOT NULL OR uss.favorite=1)
               AND er.provider=?2 AND er.model=?3 AND er.model_version=?4
             ORDER BY er.created_at DESC LIMIT 500",
        )?;
        let rows = statement.query_map(
            params![user_id, identity.provider, identity.model, identity.version],
            |row| {
                Ok((
                    row.get::<_, Option<String>>(0)?,
                    row.get::<_, i64>(1)? != 0,
                    row.get::<_, Vec<u8>>(2)?,
                ))
            },
        )?;
        let mut positive = Vec::<Vec<f32>>::new();
        let mut negative = Vec::<Vec<f32>>::new();
        for row in rows {
            let (feedback, favorite, bytes) = row?;
            let Some(vector) = decode_vector(&bytes) else {
                continue;
            };
            if favorite {
                positive.push(vector.clone());
                positive.push(vector.clone());
            }
            match feedback.as_deref() {
                Some("like") => positive.push(vector),
                Some("dislike") => negative.push(vector),
                _ => {}
            }
        }
        Ok((centroid(&positive), centroid(&negative)))
    }

}

fn into_ranked_stories(candidates: Vec<Candidate>) -> Vec<RankedStory> {
    candidates
        .into_iter()
        .map(|candidate| RankedStory {
            story_id: candidate.story_id,
            document_id: candidate.document_id,
            title: candidate.title,
            summary: candidate.summary,
            canonical_url: candidate.canonical_url,
            publisher: candidate.publisher,
            published_at: candidate.published_at,
            coverage: candidate.coverage,
            topics: candidate.topics,
            score: candidate.score,
            explanation: candidate.explanation,
        })
        .collect()
}
