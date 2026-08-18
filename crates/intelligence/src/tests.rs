#[cfg(test)]
mod tests {
    use super::*;
    use rill_dedup::{CuratorProvenance, DedupService};
    use rill_model_api::{
        ExtractiveSummaryProvider, FeatureHashEmbeddingProvider, ModelError, ModelHealth,
        ModelIdentity, RankRequest, RankResponse, RecommendationFeedbackEvent,
        RecommendationProvider,
    };

    struct FailingRecommendation;

    struct SlowRecommendation;

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

    #[async_trait::async_trait]
    impl RecommendationProvider for SlowRecommendation {
        fn identity(&self) -> ModelIdentity {
            ModelIdentity {
                provider: "test".into(),
                model: "slow-ranker".into(),
                version: "1".into(),
            }
        }

        async fn rank(&self, _: RankRequest) -> Result<RankResponse, ModelError> {
            tokio::time::sleep(std::time::Duration::from_millis(250)).await;
            Err(ModelError::Unavailable("slow fixture".into()))
        }

        async fn submit_feedback(
            &self,
            _: &[RecommendationFeedbackEvent],
        ) -> Result<(), ModelError> {
            Ok(())
        }

        async fn health(&self) -> Result<ModelHealth, ModelError> {
            Ok(ModelHealth {
                ready: true,
                detail: "slow fixture".into(),
            })
        }
    }

    fn service() -> (DbPool, IntelligenceService, String, String) {
        let pool = DbPool::open_in_memory().unwrap();
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

    fn rill_domain_fixture() -> rill_domain::NormalizedDocument {
        rill_domain::NormalizedDocument {
            visibility_scope: "public".into(),
            title: "Germany changes public software procurement".into(),
            body_text: "Germany approved a new public software procurement rule on Tuesday. The first office-suite migrations begin in October and include open document formats.".into(),
            sanitized_html: None,
            author: Some("Reporter".into()),
            publisher: Some("example.test".into()),
            canonical_url: Some("https://example.test/article".into()),
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
    async fn external_recommendation_failure_keeps_local_feed() {
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
    async fn foreground_feed_does_not_wait_for_slow_recommendation_provider() {
        let (pool, _, user_id, _) = service();
        let intelligence = IntelligenceService::new(
            pool.clone(),
            Arc::new(FeatureHashEmbeddingProvider::new(32).unwrap()),
            Arc::new(ExtractiveSummaryProvider),
            Some(Arc::new(SlowRecommendation)),
        );

        let feed = tokio::time::timeout(
            std::time::Duration::from_millis(50),
            intelligence.rank_stream(&user_id, "home", 20, "test"),
        )
        .await
        .expect("foreground feed must not await external ranking")
        .unwrap();
        assert_eq!(feed.len(), 1);
        let queued: i64 = pool
            .with_connection(|connection| {
                connection.query_row(
                    "SELECT count(*) FROM jobs WHERE kind='EvaluateStreamCandidates'
                     AND status='queued'",
                    [],
                    |row| row.get(0),
                )
            })
            .unwrap();
        assert_eq!(queued, 1);
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
        slugs.insert(0, "local-ai".into());
        intelligence
            .reorder_streams(&user_id, &slugs)
            .unwrap();
        assert_eq!(
            intelligence.list_streams(&user_id).unwrap()[0].slug,
            "local-ai"
        );
        intelligence.delete_stream(&user_id, "local-ai").unwrap();
        assert_eq!(intelligence.list_streams(&user_id).unwrap().len(), 5);
        assert!(intelligence.delete_stream(&user_id, "home").is_err());
    }

    #[test]
    fn first_stream_listing_seeds_five_high_level_streams_once() {
        let (pool, intelligence, user_id, _) = service();
        let streams = intelligence.list_streams(&user_id).unwrap();
        assert_eq!(
            streams
                .iter()
                .map(|stream| stream.slug.as_str())
                .collect::<Vec<_>>(),
            ["home", "technology", "ai", "world", "science"]
        );
        assert_eq!(intelligence.list_streams(&user_id).unwrap().len(), 5);
        let queued: i64 = pool
            .with_connection(|connection| {
                connection.query_row(
                    "SELECT count(*) FROM jobs WHERE kind='EmbedStream' AND status='queued'",
                    [],
                    |row| row.get(0),
                )
            })
            .unwrap();
        assert_eq!(queued, 5);
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
