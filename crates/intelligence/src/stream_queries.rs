impl IntelligenceService {
    pub fn ensure_home_stream(&self, user_id: &str) -> Result<String, IntelligenceError> {
        let defaults = [
            (
                "All",
                "all",
                0,
                "Every visible story, without subject filtering.",
                serde_json::json!({}),
                "Show newest stories first.",
            ),
            (
                "Home",
                "home",
                1,
                "Everything you follow, balanced across subjects and sources.",
                serde_json::json!({}),
                "Balance relevance, freshness, source affinity, and variety.",
            ),
            (
                "Technology",
                "technology",
                2,
                "Software, programming, security, infrastructure, devices, and the technology industry.",
                serde_json::json!({"includeTopics":["technology","software","programming","security","databases","rust","javascript","hardware"]}),
                "Prefer concrete engineering detail, useful tools, and measured results.",
            ),
            (
                "AI",
                "ai",
                3,
                "Artificial intelligence research, models, products, agents, and their social impact.",
                serde_json::json!({"includeTopics":["ai","artificial intelligence","machine learning","generative ai","llm","agents"]}),
                "Prefer substantive releases, research, evaluations, and practical implementation details over hype.",
            ),
            (
                "World",
                "world",
                4,
                "International affairs, politics, economics, cities, and major events around the world.",
                serde_json::json!({"includeTopics":["world","politics","international","economics","europe","germany","ukraine"]}),
                "Prefer consequential reporting and diverse geographic coverage.",
            ),
            (
                "Science",
                "science",
                5,
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
             ORDER BY CASE WHEN slug='all' THEN 0 ELSE 1 END, sort_order, name COLLATE NOCASE",
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

    pub fn user_preferences(
        &self,
        user_id: &str,
    ) -> Result<UserPreferences, IntelligenceError> {
        self.ensure_home_stream(user_id)?;
        self.pool.with_connection(|connection| {
            connection.query_row(
                "SELECT ai_free_mode, stream_membership_mode, font_family FROM user_product_state
                 WHERE user_id=?1",
                [user_id],
                |row| {
                    Ok(UserPreferences {
                        ai_free_mode: row.get::<_, i64>(0)? != 0,
                        stream_membership_mode: row.get(1)?,
                        font_family: row.get(2)?,
                    })
                },
            )
        }).map_err(Into::into)
    }

    pub fn update_user_preferences(
        &self,
        user_id: &str,
        preferences: &UserPreferences,
    ) -> Result<UserPreferences, IntelligenceError> {
        if !matches!(preferences.stream_membership_mode.as_str(), "multiple" | "exclusive") {
            return Err(IntelligenceError::Invalid(
                "stream membership mode must be multiple or exclusive".into(),
            ));
        }
        if !matches!(preferences.font_family.as_str(), "sans" | "serif") {
            return Err(IntelligenceError::Invalid(
                "font family must be sans or serif".into(),
            ));
        }
        self.ensure_home_stream(user_id)?;
        self.pool.with_connection(|connection| {
            let transaction = connection.unchecked_transaction()?;
            transaction.execute(
                "UPDATE user_product_state SET ai_free_mode=?2, stream_membership_mode=?3,
                 font_family=?4,
                 updated_at=unixepoch() WHERE user_id=?1",
                params![
                    user_id,
                    preferences.ai_free_mode,
                    preferences.stream_membership_mode,
                    preferences.font_family
                ],
            )?;
            transaction.execute("DELETE FROM recommendation_runs WHERE user_id=?1", [user_id])?;
            transaction.commit()
        })?;
        self.user_preferences(user_id)
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
        let preferences = self.user_preferences(user_id)?;
        let stream = self.load_stream(user_id, slug)?;
        let identity = self.embedding.identity();
        let affinities = load_affinity_scores(&self.pool, user_id)?;
        let mut candidates = self.load_candidates(user_id, &identity, &affinities)?;
        if preferences.stream_membership_mode == "exclusive" && !matches!(slug, "home" | "all") {
            let earlier = self
                .list_streams(user_id)?
                .into_iter()
                .filter(|candidate_stream| {
                    !matches!(candidate_stream.slug.as_str(), "home" | "all")
                        && candidate_stream.position < stream.position
                })
                .collect::<Vec<_>>();
            candidates.retain(|candidate| {
                matches_filter(candidate, &stream.filter)
                    && earlier
                        .iter()
                        .all(|candidate_stream| !matches_filter(candidate, &candidate_stream.filter))
            });
        } else {
            candidates.retain(|candidate| matches_filter(candidate, &stream.filter));
        }
        if preferences.ai_free_mode {
            for candidate in &mut candidates {
                candidate.summary.clone_from(&candidate.raw_excerpt);
                candidate.topics.clear();
                candidate.vector = None;
                let age_hours = candidate.published_at.map_or(72.0, |published| {
                    unix_now().saturating_sub(published).max(0) as f32 / 3600.0
                });
                candidate.score = 1.0 / (1.0 + age_hours / 72.0);
                candidate.explanation = json!({"aiFree": true, "freshness": candidate.score});
            }
            let selected = diversify(candidates, limit.min(100), user_id, slug);
            return Ok(into_ranked_stories(selected));
        }
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
                &affinities,
                positive.as_deref(),
                negative.as_deref(),
                stream_vector.as_deref(),
                preference.as_ref(),
            );
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
        affinities: &AffinityScores,
    ) -> Result<Vec<Candidate>, IntelligenceError> {
        let connection = self.pool.connection()?;
        let mut statement = connection.prepare(
            "WITH candidate_stories AS (
               SELECT s.id, coalesce(anchor.published_at, anchor.created_at) candidate_order,
                 uss.selected_document_id, uss.read_at IS NOT NULL is_read,
                 coalesce(uss.favorite, 0) is_favorite
               FROM stories s JOIN documents anchor ON anchor.id=s.anchor_document_id
               LEFT JOIN user_story_state uss ON uss.user_id=?1 AND uss.story_id=s.id
               WHERE uss.hidden_at IS NULL AND EXISTS (
                 SELECT 1 FROM story_memberships visible_sm
                 JOIN documents visible_d ON visible_d.id=visible_sm.document_id
                 WHERE visible_sm.story_id=s.id AND EXISTS (
                   SELECT 1 FROM document_access da WHERE da.document_id=visible_d.id
                     AND (da.user_id IS NULL OR da.user_id=?1))
               )
               ORDER BY candidate_order DESC LIMIT 500
             ), ranked_topics AS (
               SELECT story_id, topic, row_number() OVER (
                 PARTITION BY story_id ORDER BY confidence DESC, topic COLLATE NOCASE
               ) topic_rank
               FROM story_topics
             ), topics AS (
               SELECT story_id, group_concat(topic, char(31)) topics FROM (
                 SELECT story_id, topic FROM ranked_topics WHERE topic_rank <= 8
                 ORDER BY story_id, topic_rank
               ) GROUP BY story_id
             ), visible_curators AS (
               SELECT dc.document_id, dc.source_instance_id, dc.curator_id,
                 parent.title IS NULL is_direct
               FROM document_curators dc
               LEFT JOIN collection_entries ce ON ce.id=dc.collection_entry_id
               LEFT JOIN raw_items parent ON parent.id=ce.parent_raw_item_id
               WHERE dc.included=1 AND (dc.source_instance_id IS NULL OR EXISTS (
                 SELECT 1 FROM source_access sa WHERE sa.source_instance_id=dc.source_instance_id
                   AND (sa.user_id IS NULL OR sa.user_id=?1)
               ))
             ), curators AS (
               SELECT document_id, coalesce(group_concat(source_instance_id), '') sources,
                 coalesce(group_concat(curator_id), '') curator_ids,
                 coalesce(max(is_direct), 0) is_direct
               FROM visible_curators GROUP BY document_id
             )
             SELECT cs.id, d.id, d.title,
             coalesce((SELECT su.summary_text FROM summaries su WHERE su.entity_type='document'
               AND su.entity_id=d.id AND su.input_checksum=d.exact_content_hash
               ORDER BY su.created_at DESC LIMIT 1), substr(d.body_text, 1, 600)),
             substr(d.body_text, 1, 600), d.canonical_url, d.publisher, d.language, d.published_at,
             coalesce(cs.selected_document_id=d.id, 0), cs.is_read, cs.is_favorite,
             coalesce(topics.topics, ''), coalesce(curators.sources, ''),
             coalesce(curators.curator_ids, ''), coalesce(curators.is_direct, 0),
             d.sanitized_html IS NOT NULL, coalesce(length(d.body_text), 0), er.vector_f32le
             FROM candidate_stories cs
             JOIN story_memberships sm ON sm.story_id=cs.id
             JOIN documents d ON d.id=sm.document_id
             LEFT JOIN topics ON topics.story_id=cs.id
             LEFT JOIN curators ON curators.document_id=d.id
             LEFT JOIN embedding_records er ON er.id=(SELECT latest.id FROM embedding_records latest
               WHERE latest.entity_type='document' AND latest.entity_id=d.id
               AND latest.provider=?2 AND latest.model=?3 AND latest.model_version=?4
               ORDER BY latest.created_at DESC LIMIT 1)
             WHERE EXISTS (
               SELECT 1 FROM document_access da WHERE da.document_id=d.id
                 AND (da.user_id IS NULL OR da.user_id=?1)
             )
             ORDER BY cs.candidate_order DESC, cs.id,
               coalesce(d.published_at, d.created_at), d.id",
        )?;
        let rows = statement.query_map(
            params![
                user_id,
                identity.provider,
                identity.model,
                identity.version,
            ],
            |row| {
                let bytes: Option<Vec<u8>> = row.get(18)?;
                Ok(CandidateVariant {
                    candidate: Candidate {
                        story_id: row.get(0)?,
                        document_id: row.get(1)?,
                        title: row.get(2)?,
                        summary: row.get(3)?,
                        raw_excerpt: row.get(4)?,
                        canonical_url: row.get(5)?,
                        publisher: row.get(6)?,
                        language: row.get(7)?,
                        published_at: row.get(8)?,
                        coverage: 0,
                        topics: control_list(row.get(12)?),
                        sources: comma_list(row.get(13)?),
                        curators: comma_list(row.get(14)?),
                        read: row.get::<_, i64>(10)? != 0,
                        favorite: row.get::<_, i64>(11)? != 0,
                        vector: bytes.as_deref().and_then(decode_vector),
                        score: 0.0,
                        explanation: json!({}),
                    },
                    preferred: row.get::<_, i64>(9)? != 0,
                    direct: row.get::<_, i64>(15)? != 0,
                    readable: row.get::<_, i64>(16)? != 0,
                    body_chars: usize::try_from(row.get::<_, i64>(17)?).unwrap_or(usize::MAX),
                })
            },
        )?;
        let variants = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        let mut grouped = Vec::<Vec<CandidateVariant>>::new();
        for variant in variants {
            if grouped.last().is_none_or(|group| {
                group[0].candidate.story_id != variant.candidate.story_id
            }) {
                grouped.push(Vec::new());
            }
            grouped.last_mut().expect("group was inserted").push(variant);
        }
        Ok(grouped
            .into_iter()
            .map(|variants| candidate_from_variants(variants, affinities))
            .collect())
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

fn candidate_from_variants(
    mut variants: Vec<CandidateVariant>,
    affinities: &AffinityScores,
) -> Candidate {
    let coverage = u32::try_from(variants.len()).unwrap_or(u32::MAX);
    let selected = variants
        .iter()
        .position(|variant| variant.preferred)
        .unwrap_or_else(|| {
            let latest = variants
                .iter()
                .filter_map(|variant| variant.candidate.published_at)
                .max()
                .unwrap_or_else(unix_now);
            let mut best = (0_usize, f32::NEG_INFINITY);
            for (index, variant) in variants.iter().enumerate() {
                let candidate = &variant.candidate;
                let affinity = affinity_score_from(
                    affinities,
                    candidate.publisher.as_deref(),
                    &candidate.sources,
                    &candidate.curators,
                );
                let completeness = variant.body_chars.min(8_000) as f32 / 8_000.0;
                let freshness = candidate.published_at.map_or(0.0, |published| {
                    1.0 - (latest.saturating_sub(published).max(0) as f32
                        / (30.0 * 86_400.0))
                        .min(1.0)
                });
                let score = affinity * 0.55
                    + completeness * 0.15
                    + freshness * 0.10
                    + if variant.direct { 0.12 } else { 0.0 }
                    + if variant.readable { 0.10 } else { 0.0 }
                    + if candidate
                        .canonical_url
                        .as_deref()
                        .is_some_and(|url| url.starts_with("https://"))
                    {
                        0.08
                    } else {
                        0.0
                    };
                if score > best.1 {
                    best = (index, score);
                }
            }
            best.0
        });
    let mut candidate = variants.swap_remove(selected).candidate;
    candidate.coverage = coverage;
    candidate
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
            source_ids: candidate.sources,
            published_at: candidate.published_at,
            read: candidate.read,
            coverage: candidate.coverage,
            topics: candidate.topics,
            score: candidate.score,
            explanation: candidate.explanation,
        })
        .collect()
}
