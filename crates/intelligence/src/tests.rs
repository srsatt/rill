#[cfg(test)]
mod tests {
    use super::*;
    use rill_dedup::{CuratorProvenance, DedupService};
    use rill_model_api::{
        ExtractiveSummaryProvider, FeatureHashEmbeddingProvider, ModelError, ModelHealth,
        ModelIdentity, RankRequest, RankResponse, RecommendationFeedbackEvent,
        RecommendationProvider, SummaryProvider, SummaryRequest, SummaryResponse, TopicTag,
    };

    struct FailingRecommendation;

    struct SourceInstructionSummary;

    #[async_trait::async_trait]
    impl SummaryProvider for SourceInstructionSummary {
        fn identity(&self) -> ModelIdentity {
            ModelIdentity {
                provider: "test".into(),
                model: "source-instructions".into(),
                version: "1".into(),
            }
        }

        async fn summarize(&self, request: SummaryRequest) -> Result<SummaryResponse, ModelError> {
            let include = !request
                .custom_instruction
                .as_deref()
                .unwrap_or_default()
                .contains("Remove product launches");
            Ok(SummaryResponse {
                text: "Processed summary.".into(),
                tags: vec![TopicTag { label: "technology".into(), confidence: 0.9 }],
                include,
            })
        }

        async fn health(&self) -> Result<ModelHealth, ModelError> {
            Ok(ModelHealth { ready: true, detail: "ready".into() })
        }
    }

    #[async_trait::async_trait]
    impl RecommendationProvider for FailingRecommendation {
        fn identity(&self) -> ModelIdentity {
            ModelIdentity {
                provider: "test".into(),
                model: "always-fails".into(),
                version: "1".into(),
            }
        }

        async fn rank(&self, _: RankRequest) -> Result<RankResponse, ModelError> {
            Err(ModelError::Unavailable("fixture outage".into()))
        }

        async fn submit_feedback(
            &self,
            _: &[RecommendationFeedbackEvent],
        ) -> Result<(), ModelError> {
            Err(ModelError::Unavailable("fixture outage".into()))
        }

        async fn health(&self) -> Result<ModelHealth, ModelError> {
            Ok(ModelHealth {
                ready: false,
                detail: "fixture outage".into(),
            })
        }
    }

    fn service() -> (DbPool, IntelligenceService, String, String) {
        let pool = DbPool::open_in_memory().unwrap();
        service_with_pool(pool)
    }

    fn service_with_pool(pool: DbPool) -> (DbPool, IntelligenceService, String, String) {
        let connection = pool.connection().unwrap();
        let user_id = Uuid::new_v4().to_string();
        connection
            .execute(
                "INSERT INTO users(id, username, role) VALUES (?1, 'reader', 'user')",
                [&user_id],
            )
            .unwrap();
        drop(connection);
        let document = rill_domain_fixture();
        let result = DedupService::new(pool.clone())
            .upsert_document(
                &document,
                &CuratorProvenance {
                    curator_kind: "source".into(),
                    curator_id: "feed".into(),
                    source_instance_id: None,
                    raw_item_id: None,
                    collection_entry_id: None,
                },
            )
            .unwrap();
        let service = IntelligenceService::new(
            pool.clone(),
            Arc::new(FeatureHashEmbeddingProvider::new(32).unwrap()),
            Arc::new(ExtractiveSummaryProvider),
            None,
        );
        (pool, service, user_id, result.document_id)
    }

    fn story_id(pool: &DbPool, document_id: &str) -> String {
        pool.with_connection(|connection| {
            connection.query_row(
                "SELECT story_id FROM story_memberships WHERE document_id=?1",
                [document_id],
                |row| row.get(0),
            )
        })
        .unwrap()
    }

    fn rill_domain_fixture() -> rill_domain::NormalizedDocument {
        rill_domain::NormalizedDocument {
            visibility_scope: "public".into(),
            title: "Germany changes public software procurement".into(),
            body_text: "Germany approved a new public software procurement rule on Tuesday. The first office-suite migrations begin in October and include open document formats.".into(),
            sanitized_html: None,
            author: Some("Reporter".into()),
            publisher: Some("example.test".into()),
            canonical_url: Some("https://example.test/article".into()),
            links: Vec::new(),
            language: Some("en".into()),
            published_at: Some(unix_now()),
        }
    }

    fn variant_fixture(
        visibility_scope: &str,
        publisher: &str,
        title: &str,
        url: &str,
    ) -> rill_domain::NormalizedDocument {
        rill_domain::NormalizedDocument {
            visibility_scope: visibility_scope.into(),
            title: title.into(),
            body_text: format!(
                "{title}. This independently reported variant contains enough readable article text for representative selection."
            ),
            sanitized_html: Some(format!("<p>{title}</p>")),
            author: Some("Another reporter".into()),
            publisher: Some(publisher.into()),
            canonical_url: Some(url.into()),
            links: Vec::new(),
            language: Some("en".into()),
            published_at: Some(unix_now()),
        }
    }

    #[tokio::test]
    async fn persists_summary_and_embedding_with_model_identity() {
        let (pool, service, _, document_id) = service();
        service.process_summary(&document_id).await.unwrap();
        service.process_embedding(&document_id).await.unwrap();
        let connection = pool.connection().unwrap();
        let summaries: i64 = connection
            .query_row("SELECT count(*) FROM summaries", [], |row| row.get(0))
            .unwrap();
        let topics: i64 = connection
            .query_row("SELECT count(*) FROM document_topics", [], |row| row.get(0))
            .unwrap();
        let embeddings: i64 = connection
            .query_row("SELECT count(*) FROM embedding_records", [], |row| {
                row.get(0)
            })
            .unwrap();
        assert_eq!((summaries, topics, embeddings), (1, 6, 1));
    }

    #[tokio::test]
    async fn source_instruction_can_filter_document_from_feed() {
        let pool = DbPool::open_in_memory().unwrap();
        let user_id = Uuid::new_v4().to_string();
        pool.with_connection(|connection| {
            connection.execute(
                "INSERT INTO users(id, username, role) VALUES (?1, 'reader', 'user')",
                [&user_id],
            )?;
            connection.execute_batch(
                "INSERT INTO source_definitions(id, kind, display_name) VALUES
                   ('fixture:rss', 'fixture-rss', 'Fixture RSS');",
            )?;
            connection.execute(
                "INSERT INTO source_instances(
                   id, definition_id, owner_user_id, name, visibility, visibility_scope,
                   processing_prompt, audience
                 ) VALUES ('feed', 'fixture:rss', ?1, 'Feed', 'private', ?2,
                   'Remove product launches from the feed', 'owner')",
                params![user_id, format!("user:{user_id}")],
            )?;
            Ok::<_, rusqlite::Error>(())
        })
        .unwrap();
        let mut source_document = rill_domain_fixture();
        source_document.visibility_scope = format!("user:{user_id}");
        let document = DedupService::new(pool.clone())
            .upsert_document(
                &source_document,
                &CuratorProvenance {
                    curator_kind: "source".into(),
                    curator_id: "feed".into(),
                    source_instance_id: Some("feed".into()),
                    raw_item_id: None,
                    collection_entry_id: None,
                },
            )
            .unwrap();
        let service = IntelligenceService::new(
            pool.clone(),
            Arc::new(FeatureHashEmbeddingProvider::new(32).unwrap()),
            Arc::new(SourceInstructionSummary),
            None,
        );

        service.process_summary(&document.document_id).await.unwrap();

        let visible: i64 = pool
            .with_connection(|connection| {
                connection.query_row(
                    "SELECT count(*) FROM document_access WHERE document_id=?1",
                    [&document.document_id],
                    |row| row.get(0),
                )
            })
            .unwrap();
        assert_eq!(visible, 0);
        assert!(
            service
                .rank_stream_now(&user_id, "all", 20, "test")
                .unwrap()
                .is_empty()
        );
    }

    #[tokio::test]
    async fn recurring_posts_from_one_publisher_do_not_become_coverage() {
        let (pool, service, _, first_document_id) = service();
        service.process_embedding(&first_document_id).await.unwrap();
        let mut recurring = rill_domain_fixture();
        recurring.title.push_str(" — daily edition");
        recurring.body_text.push_str(" Daily edition.");
        recurring.canonical_url = Some("https://example.test/article-2".into());
        let second = DedupService::new(pool.clone())
            .upsert_document(
                &recurring,
                &CuratorProvenance {
                    curator_kind: "source".into(), curator_id: "feed".into(),
                    source_instance_id: None, raw_item_id: None, collection_entry_id: None,
                },
            )
            .unwrap();
        service.process_embedding(&second.document_id).await.unwrap();

        assert_ne!(story_id(&pool, &first_document_id), story_id(&pool, &second.document_id));
    }

    #[tokio::test]
    async fn reprocessing_old_documents_does_not_cluster_with_future_stories() {
        let (pool, service, _, first_document_id) = service();
        service.process_embedding(&first_document_id).await.unwrap();
        let mut old = rill_domain_fixture();
        old.title.push_str(" — archived report");
        old.body_text.push_str(" Archived report.");
        old.publisher = Some("archive.test".into());
        old.canonical_url = Some("https://archive.test/article".into());
        old.published_at = Some(unix_now() - 10 * 24 * 60 * 60);
        let second = DedupService::new(pool.clone())
            .upsert_document(
                &old,
                &CuratorProvenance {
                    curator_kind: "source".into(), curator_id: "archive".into(),
                    source_instance_id: None, raw_item_id: None, collection_entry_id: None,
                },
            )
            .unwrap();
        service.process_embedding(&second.document_id).await.unwrap();

        assert_ne!(story_id(&pool, &first_document_id), story_id(&pool, &second.document_id));
    }

    #[tokio::test]
    async fn stream_topic_filters_use_persisted_enrichment_tags() {
        let (_, intelligence, user_id, document_id) = service();
        intelligence.process_summary(&document_id).await.unwrap();
        intelligence
            .create_stream(
                &user_id,
                &CreateStreamInput {
                    name: "Germany".into(),
                    slug: "germany".into(),
                    icon: None,
                    filter: StreamFilter {
                        include_topics: vec!["GERMANY".into()],
                        ..Default::default()
                    },
                    semantic_description: None,
                    ranking_instruction: None,
                },
            )
            .await
            .unwrap();

        let feed = intelligence
            .rank_stream(&user_id, "germany", 20, "test")
            .await
            .unwrap();
        assert_eq!(feed.len(), 1);
        assert!(feed[0].topics.iter().any(|topic| topic == "germany"));
    }

    #[tokio::test]
    async fn ai_free_mode_uses_raw_text_without_topics_or_model_ranking() {
        let (_, intelligence, user_id, document_id) = service();
        intelligence.process_summary(&document_id).await.unwrap();
        intelligence
            .update_user_preferences(
                &user_id,
                &UserPreferences {
                    ai_free_mode: true,
                    stream_membership_mode: "multiple".into(),
                    font_family: "sans".into(),
                },
            )
            .unwrap();

        let feed = intelligence.rank_stream_now(&user_id, "all", 20, "test").unwrap();
        assert_eq!(feed.len(), 1);
        assert!(feed[0].topics.is_empty());
        assert_eq!(feed[0].explanation["aiFree"], true);
        assert!(feed[0].summary.starts_with("Germany approved"));
    }

    #[tokio::test]
    async fn exclusive_stream_mode_assigns_to_first_matching_subject_stream() {
        let (pool, intelligence, user_id, document_id) = service();
        pool.with_connection(|connection| {
            connection.execute(
                "INSERT INTO document_topics(document_id, topic, confidence, provider, model,
                 model_version, input_checksum)
                 SELECT id, 'aardvark', 0.9, 'fixture', 'topics', '1', exact_content_hash
                 FROM documents WHERE id=?1",
                [&document_id],
            )
        })
        .unwrap();
        for slug in ["first", "second"] {
            intelligence
                .create_stream(
                    &user_id,
                    &CreateStreamInput {
                        name: slug.into(), slug: slug.into(), icon: None,
                        filter: StreamFilter { include_topics: vec!["aardvark".into()], ..Default::default() },
                        semantic_description: None, ranking_instruction: None,
                    },
                )
                .await
                .unwrap();
        }
        intelligence
            .update_user_preferences(
                &user_id,
                &UserPreferences {
                    ai_free_mode: false,
                    stream_membership_mode: "exclusive".into(),
                    font_family: "sans".into(),
                },
            )
            .unwrap();

        assert_eq!(intelligence.rank_stream_now(&user_id, "first", 20, "test").unwrap().len(), 1);
        assert!(intelligence.rank_stream_now(&user_id, "second", 20, "test").unwrap().is_empty());
    }

    #[tokio::test]
    async fn topic_library_matches_persisted_tags_case_insensitively() {
        let (_, intelligence, user_id, document_id) = service();
        intelligence.process_summary(&document_id).await.unwrap();

        let stories = intelligence.topic_stories(&user_id, "GERMANY", 20).unwrap();

        assert_eq!(stories.len(), 1);
        assert!(stories[0].topics.iter().any(|topic| topic == "germany"));
        assert!(intelligence
            .topic_stories(&user_id, "missing-topic", 20)
            .unwrap()
            .is_empty());
    }

    #[tokio::test]
    async fn stream_topic_filters_include_tags_from_non_representative_variants() {
        let (pool, intelligence, user_id, first_document_id) = service();
        let story_id: String = pool
            .with_connection(|connection| {
                connection.query_row(
                    "SELECT story_id FROM story_memberships WHERE document_id=?1",
                    [&first_document_id],
                    |row| row.get(0),
                )
            })
            .unwrap();
        let tagged = DedupService::new(pool.clone())
            .upsert_document(
                &variant_fixture(
                    "public",
                    "javascript.example",
                    "JavaScript compiler implementation",
                    "https://javascript.example/compiler",
                ),
                &CuratorProvenance {
                    curator_kind: "source".into(),
                    curator_id: "javascript-feed".into(),
                    source_instance_id: None,
                    raw_item_id: None,
                    collection_entry_id: None,
                },
            )
            .unwrap();
        intelligence
            .manual_merge(&user_id, &tagged.document_id, &story_id)
            .unwrap();
        intelligence
            .select_story_variant(&user_id, &story_id, &first_document_id)
            .unwrap();
        pool.with_connection(|connection| {
            connection.execute(
                "INSERT INTO document_topics(document_id, topic, confidence, provider, model,
                 model_version, input_checksum)
                 SELECT id, 'javascript', 0.9, 'fixture', 'topics', '1', exact_content_hash
                 FROM documents WHERE id=?1",
                [&tagged.document_id],
            )
        })
        .unwrap();
        intelligence
            .create_stream(
                &user_id,
                &CreateStreamInput {
                    name: "JavaScript".into(),
                    slug: "javascript".into(),
                    icon: None,
                    filter: StreamFilter {
                        include_topics: vec!["javascript".into()],
                        ..Default::default()
                    },
                    semantic_description: None,
                    ranking_instruction: None,
                },
            )
            .await
            .unwrap();

        let feed = intelligence
            .rank_stream(&user_id, "javascript", 20, "test")
            .await
            .unwrap();
        assert_eq!(feed.len(), 1);
        assert_eq!(feed[0].document_id, first_document_id);
        assert!(feed[0].topics.iter().any(|topic| topic == "javascript"));
    }

    #[test]
    fn feedback_replacement_keeps_one_explicit_state() {
        let (pool, service, user_id, document_id) = service();
        let story_id: String = pool
            .with_connection(|connection| {
                connection.query_row(
                    "SELECT story_id FROM story_memberships WHERE document_id = ?1",
                    [&document_id],
                    |row| row.get(0),
                )
            })
            .unwrap();
        service
            .record_feedback(&user_id, &story_id, FeedbackKind::Like, "test")
            .unwrap();
        service
            .record_feedback(&user_id, &story_id, FeedbackKind::Dislike, "test")
            .unwrap();
        let connection = pool.connection().unwrap();
        let state: String = connection
            .query_row(
                "SELECT explicit_feedback FROM user_story_state WHERE user_id = ?1 AND story_id = ?2",
                params![user_id, story_id],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(state, "dislike");
        let raw_events: i64 = connection
            .query_row("SELECT count(*) FROM feedback_events", [], |row| row.get(0))
            .unwrap();
        assert_eq!(raw_events, 2);
    }

    #[tokio::test]
    async fn provider_recommendation_override_is_disabled() {
        let (pool, _, user_id, _) = service();
        let intelligence = IntelligenceService::new(
            pool.clone(),
            Arc::new(FeatureHashEmbeddingProvider::new(32).unwrap()),
            Arc::new(ExtractiveSummaryProvider),
            Some(Arc::new(FailingRecommendation)),
        );

        let feed = intelligence
            .rank_stream(&user_id, "home", 20, "test")
            .await
            .unwrap();
        assert_eq!(feed.len(), 1);
        let provider: String = pool
            .with_connection(|connection| {
                connection.query_row(
                    "SELECT provider FROM recommendation_runs ORDER BY created_at DESC LIMIT 1",
                    [],
                    |row| row.get(0),
                )
            })
            .unwrap();
        assert_eq!(provider, "rill-local");
    }

    #[tokio::test]
    async fn preference_model_cold_start_and_one_class_fall_back() {
        let (pool, intelligence, user_id, document_id) = service();
        let identity = intelligence.embedding.identity();
        assert!(intelligence.preference_model(&user_id, &identity).unwrap().is_none());
        intelligence.process_embedding(&document_id).await.unwrap();
        let story_id = story_id(&pool, &document_id);
        for _ in 0..5 {
            intelligence
                .record_feedback(&user_id, &story_id, FeedbackKind::Like, "test")
                .unwrap();
        }
        assert!(!intelligence.refit_preference_model(&user_id).unwrap());
        assert!(intelligence.preference_model(&user_id, &identity).unwrap().is_none());
    }

    #[test]
    fn preference_refit_is_queued_at_configured_threshold() {
        let (pool, intelligence, user_id, document_id) = service();
        let intelligence = intelligence.configure_preference_model(5, 500);
        let story_id = story_id(&pool, &document_id);
        for _ in 0..4 {
            intelligence
                .record_feedback(&user_id, &story_id, FeedbackKind::Like, "test")
                .unwrap();
        }
        let queued = || {
            pool.with_connection(|connection| {
                connection.query_row(
                    "SELECT count(*) FROM jobs WHERE kind='RefitPreferenceModel'",
                    [],
                    |row| row.get::<_, i64>(0),
                )
            })
            .unwrap()
        };
        assert_eq!(queued(), 0);
        intelligence
            .record_feedback(&user_id, &story_id, FeedbackKind::Dislike, "test")
            .unwrap();
        assert_eq!(queued(), 1);
    }

    #[tokio::test]
    async fn preference_model_refits_and_survives_restart() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("rill.db");
        let pool = DbPool::open(&path, 1, std::time::Duration::from_secs(1)).unwrap();
        let (pool, intelligence, user_id, document_id) = service_with_pool(pool);
        intelligence.process_embedding(&document_id).await.unwrap();
        let story_id = story_id(&pool, &document_id);
        for feedback in [
            FeedbackKind::Like,
            FeedbackKind::Like,
            FeedbackKind::Dislike,
            FeedbackKind::Like,
            FeedbackKind::Dislike,
        ] {
            intelligence
                .record_feedback(&user_id, &story_id, feedback, "test")
                .unwrap();
        }
        assert!(intelligence.refit_preference_model(&user_id).unwrap());
        let feed = intelligence
            .rank_stream(&user_id, "home", 20, "test")
            .await
            .unwrap();
        assert_eq!(feed[0].explanation["fallback"], false);
        drop(intelligence);
        drop(pool);

        let pool = DbPool::open(&path, 1, std::time::Duration::from_secs(1)).unwrap();
        let intelligence = IntelligenceService::new(
            pool.clone(),
            Arc::new(FeatureHashEmbeddingProvider::new(32).unwrap()),
            Arc::new(ExtractiveSummaryProvider),
            None,
        );
        assert!(
            intelligence
                .preference_model(&user_id, &intelligence.embedding.identity())
                .unwrap()
                .is_some()
        );
        let durable: (i64, i64) = pool
            .with_connection(|connection| {
                Ok::<_, rusqlite::Error>((
                    connection.query_row("SELECT count(*) FROM feedback_events", [], |row| {
                        row.get(0)
                    })?,
                    connection.query_row("SELECT count(*) FROM embedding_records", [], |row| {
                        row.get(0)
                    })?,
                ))
            })
            .unwrap();
        assert_eq!(durable.0, 5);
        assert!(durable.1 > 0);
    }

    #[tokio::test]
    async fn corrupt_preference_model_falls_back() {
        let (pool, intelligence, user_id, _) = service();
        let identity = intelligence.embedding.identity();
        pool.with_connection(|connection| {
            connection.execute(
                "INSERT INTO preference_models(user_id, model_version, feature_version,
                 embedding_provider, embedding_model, embedding_version, coefficients_json,
                 sample_count, positive_count, negative_count, trained_event_count)
                 VALUES (?1, 1, 1, ?2, ?3, ?4, '[]', 2, 1, 1, 2)",
                params![user_id, identity.provider, identity.model, identity.version],
            )
        })
        .unwrap();
        let feed = intelligence
            .rank_stream(&user_id, "home", 20, "test")
            .await
            .unwrap();
        assert_eq!(feed[0].explanation["fallback"], true);
    }

    #[tokio::test]
    async fn recommendation_cache_is_reused_and_feedback_invalidates_it() {
        let (pool, intelligence, user_id, document_id) = service();
        let story_id: String = pool
            .with_connection(|connection| {
                connection.query_row(
                    "SELECT story_id FROM story_memberships WHERE document_id=?1",
                    [&document_id],
                    |row| row.get(0),
                )
            })
            .unwrap();

        intelligence
            .rank_stream(&user_id, "home", 20, "test")
            .await
            .unwrap();
        intelligence
            .rank_stream(&user_id, "home", 20, "test")
            .await
            .unwrap();
        let runs: i64 = pool
            .with_connection(|connection| {
                connection.query_row("SELECT count(*) FROM recommendation_runs", [], |row| {
                    row.get(0)
                })
            })
            .unwrap();
        assert_eq!(runs, 1);

        intelligence
            .record_feedback(&user_id, &story_id, FeedbackKind::Like, "test")
            .unwrap();
        intelligence
            .rank_stream(&user_id, "home", 20, "test")
            .await
            .unwrap();
        let runs: i64 = pool
            .with_connection(|connection| {
                connection.query_row("SELECT count(*) FROM recommendation_runs", [], |row| {
                    row.get(0)
                })
            })
            .unwrap();
        assert_eq!(runs, 1);
    }

    #[tokio::test]
    async fn streams_can_be_updated_reordered_and_deleted() {
        let (_, intelligence, user_id, _) = service();
        intelligence.ensure_home_stream(&user_id).unwrap();
        intelligence
            .create_stream(
                &user_id,
                &CreateStreamInput {
                    name: "Local AI".into(),
                    slug: "local-ai".into(),
                    icon: None,
                    filter: StreamFilter::default(),
                    semantic_description: Some("Technical AI research".into()),
                    ranking_instruction: None,
                },
            )
            .await
            .unwrap();
        let updated = intelligence
            .update_stream(
                &user_id,
                "local-ai",
                &UpdateStreamInput {
                    name: "Applied AI".into(),
                    icon: Some("spark".into()),
                    filter: StreamFilter::default(),
                    semantic_description: None,
                    ranking_instruction: Some("Prefer implementation details".into()),
                },
        )
            .await
            .unwrap();
        assert_eq!(updated.name, "Applied AI");
        let mut slugs = intelligence
            .list_streams(&user_id)
            .unwrap()
            .into_iter()
            .map(|stream| stream.slug)
            .collect::<Vec<_>>();
        slugs.retain(|slug| slug != "local-ai");
        slugs.insert(1, "local-ai".into());
        intelligence
            .reorder_streams(&user_id, &slugs)
            .unwrap();
        assert_eq!(
            intelligence.list_streams(&user_id).unwrap()[1].slug,
            "local-ai"
        );
        let mut invalid = slugs.clone();
        invalid.swap(0, 1);
        assert!(intelligence.reorder_streams(&user_id, &invalid).is_err());
        intelligence.delete_stream(&user_id, "local-ai").unwrap();
        assert_eq!(intelligence.list_streams(&user_id).unwrap().len(), 6);
        assert!(intelligence.delete_stream(&user_id, "home").is_err());
        assert!(intelligence.delete_stream(&user_id, "all").is_err());
    }

    #[test]
    fn first_stream_listing_seeds_default_streams_once() {
        let (pool, intelligence, user_id, _) = service();
        let streams = intelligence.list_streams(&user_id).unwrap();
        assert_eq!(
            streams
                .iter()
                .map(|stream| stream.slug.as_str())
                .collect::<Vec<_>>(),
            ["all", "home", "technology", "ai", "world", "science"]
        );
        assert_eq!(intelligence.list_streams(&user_id).unwrap().len(), 6);
        let queued: i64 = pool
            .with_connection(|connection| {
                connection.query_row(
                    "SELECT count(*) FROM jobs WHERE kind='EmbedStream' AND status='queued'",
                    [],
                    |row| row.get(0),
                )
            })
            .unwrap();
        assert_eq!(queued, 6);
    }

    #[tokio::test]
    async fn high_level_stream_matches_specific_topic_family() {
        let (pool, intelligence, user_id, document_id) = service();
        pool.with_connection(|connection| {
            connection.execute(
                "INSERT INTO document_topics(document_id, topic, confidence, provider, model,
                 model_version, input_checksum)
                 SELECT id, 'ai hardware', 0.9, 'fixture', 'topics', '1', exact_content_hash
                 FROM documents WHERE id=?1",
                [&document_id],
            )
        })
        .unwrap();

        let feed = intelligence
            .rank_stream(&user_id, "ai", 20, "test")
            .await
            .unwrap();
        assert_eq!(feed.len(), 1);
        assert_eq!(feed[0].topics, ["ai hardware"]);
    }

    #[test]
    fn representative_uses_affinity_without_changing_story_anchor() {
        let (pool, service, user_id, first_document_id) = service();
        let story_id: String = pool
            .with_connection(|connection| {
                connection.query_row(
                    "SELECT story_id FROM story_memberships WHERE document_id=?1",
                    [&first_document_id],
                    |row| row.get(0),
                )
            })
            .unwrap();
        let second = DedupService::new(pool.clone())
            .upsert_document(
                &variant_fixture(
                    "public",
                    "trusted.test",
                    "Trusted outlet reports procurement change",
                    "https://trusted.test/report",
                ),
                &CuratorProvenance {
                    curator_kind: "source".into(),
                    curator_id: "trusted-feed".into(),
                    source_instance_id: None,
                    raw_item_id: None,
                    collection_entry_id: None,
                },
            )
            .unwrap();
        service
            .manual_merge(&user_id, &second.document_id, &story_id)
            .unwrap();
        let anchor_before: String = pool
            .with_connection(|connection| {
                connection.query_row(
                    "SELECT anchor_document_id FROM stories WHERE id=?1",
                    [&story_id],
                    |row| row.get(0),
                )
            })
            .unwrap();
        pool.with_connection(|connection| {
            connection.execute(
                "INSERT INTO source_affinity_events(id, user_id, subject_kind, subject_id,
                 signal, weight, story_id) VALUES (?1, ?2, 'publisher', 'trusted.test',
                 'like', 40, ?3)",
                params![Uuid::new_v4().to_string(), user_id, story_id],
            )
        })
        .unwrap();

        let detail = service.story_detail(&user_id, &story_id).unwrap();
        assert_eq!(detail.coverage_count, 2);
        assert_eq!(detail.representative.document_id, second.document_id);
        assert_eq!(
            service.rank_stream_now(&user_id, "home", 20, "test").unwrap()[0].document_id,
            second.document_id
        );
        assert!(
            detail
                .variants
                .iter()
                .all(|variant| !variant.title.is_empty())
        );
        let anchor_after: String = pool
            .with_connection(|connection| {
                connection.query_row(
                    "SELECT anchor_document_id FROM stories WHERE id=?1",
                    [&story_id],
                    |row| row.get(0),
                )
            })
            .unwrap();
        assert_eq!(anchor_after, anchor_before);

        service
            .select_story_variant(&user_id, &story_id, &first_document_id)
            .unwrap();
        assert_eq!(
            service
                .story_detail(&user_id, &story_id)
                .unwrap()
                .representative
                .document_id,
            first_document_id
        );
        assert_eq!(
            service.rank_stream_now(&user_id, "home", 20, "test").unwrap()[0].document_id,
            first_document_id
        );
    }

    #[test]
    fn private_story_is_not_visible_to_another_user() {
        let (pool, service, owner_id, _) = service();
        let other_id = Uuid::new_v4().to_string();
        pool.with_connection(|connection| {
            connection.execute(
                "INSERT INTO users(id, username, role) VALUES (?1, 'other', 'user')",
                [&other_id],
            )
        })
        .unwrap();
        let private = DedupService::new(pool.clone())
            .upsert_document(
                &variant_fixture(
                    &format!("user:{owner_id}"),
                    "private.test",
                    "Private newsletter item",
                    "https://private.test/item",
                ),
                &CuratorProvenance {
                    curator_kind: "newsletter".into(),
                    curator_id: "private-letter".into(),
                    source_instance_id: None,
                    raw_item_id: None,
                    collection_entry_id: None,
                },
            )
            .unwrap();
        let story_id: String = pool
            .with_connection(|connection| {
                connection.query_row(
                    "SELECT story_id FROM story_memberships WHERE document_id=?1",
                    [&private.document_id],
                    |row| row.get(0),
                )
            })
            .unwrap();
        assert!(service.story_detail(&owner_id, &story_id).is_ok());
        assert!(matches!(
            service.story_detail(&other_id, &story_id),
            Err(IntelligenceError::NotFound)
        ));
    }
}
