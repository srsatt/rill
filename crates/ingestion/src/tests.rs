#[cfg(test)]
mod tests {
    use super::*;
    use rill_domain::Role;

    fn setup() -> (DbPool, IngestionService, String, SourceRegistration) {
        let pool = DbPool::open_in_memory().unwrap();
        let user_id = Uuid::new_v4().to_string();
        pool.with_connection(|connection| {
            connection.execute(
                "INSERT INTO users(id, username, role) VALUES (?1, 'alice', ?2)",
                params![user_id, Role::User.as_str()],
            )
        })
        .unwrap();
        let service = IngestionService::new(pool.clone(), 10);
        let source = service
            .register_source(
                "rss",
                "Example feed",
                Some(&user_id),
                false,
                &json!({"url":"https://feed.example/rss","enabled":true}),
            )
            .unwrap();
        (pool, service, user_id, source)
    }

    fn article(external_id: &str, url: &str, title: &str) -> RawSourceItem {
        RawSourceItem {
            external_id: external_id.into(), item_kind: "article".into(), title: Some(title.into()),
            body_text: Some("A useful report about SQLite query planning and performance.".into()),
            body_html: Some("<p>A useful report about SQLite query planning and performance.</p><script>bad()</script>".into()),
            author: Some("Ada".into()), source_url: Some(url.into()), published_at: Some(1_755_432_000),
            edited_at: None, deleted_at: None, external_urls: vec![ExternalLink {
                url: url.into(), relation: LinkRelation::alternate(), title: None, ordinal: 0,
            }], media: Vec::new(),
            metadata: json!({"language":"en"}),
        }
    }

    #[test]
    fn rss_item_becomes_searchable_story_idempotently() {
        let (pool, service, user_id, source) = setup();
        let batch = SourceBatch {
            items: vec![article(
                "one",
                "https://example.com/a?utm_source=rss",
                "SQLite planner",
            )],
            cursor: None,
            not_modified: false,
        };
        let first = service.ingest_batch(&source.id, &batch).unwrap();
        let second = service.ingest_batch(&source.id, &batch).unwrap();
        assert_eq!(first.documents_created, 1);
        assert_eq!(second.documents_created, 0);
        let results = service.search(&user_id, "SQLite planning", 10).unwrap();
        assert_eq!(results.len(), 1);
        let script_count: i64 = pool
            .with_connection(|connection| {
                connection.query_row(
                    "SELECT count(*) FROM documents WHERE sanitized_html LIKE '%script%'",
                    [],
                    |row| row.get(0),
                )
            })
            .unwrap();
        assert_eq!(script_count, 0);
    }

    #[test]
    fn feed_item_drops_mastodon_javascript_fallback() {
        let (pool, service, _user_id, source) = setup();
        let mut item = article("mastodon", "https://social.example/@graphene/1", "GrapheneOS");
        item.body_text = Some(
            "GrapheneOS is projected for high-end Motorola phones in 2027.\n\n<img alt=\"Mastodon\" src=\"/logo.svg\" /> <div>To use the Mastodon web application, please enable JavaScript. Alternatively, try one of the native apps for Mastodon for your platform.</div>".into(),
        );
        item.body_html = Some(
            "<p>GrapheneOS is projected for high-end Motorola phones in 2027.</p><img alt=\"Mastodon\" src=\"/logo.svg\"><div>To use the Mastodon web application, please enable JavaScript.</div>".into(),
        );
        service
            .ingest_batch(
                &source.id,
                &SourceBatch { items: vec![item], cursor: None, not_modified: false },
            )
            .unwrap();

        let body: String = pool
            .with_connection(|connection| {
                connection.query_row("SELECT body_text FROM documents", [], |row| row.get(0))
            })
            .unwrap();
        assert!(body.contains("Motorola phones"), "{body}");
        assert!(!body.contains("enable JavaScript"));
        let html: String = pool
            .with_connection(|connection| {
                connection.query_row("SELECT sanitized_html FROM documents", [], |row| row.get(0))
            })
            .unwrap();
        assert!(!html.contains("enable JavaScript"));
    }

    #[test]
    fn source_processing_prompt_is_saved_and_requeues_documents() {
        let (pool, service, user_id, source) = setup();
        service
            .ingest_batch(
                &source.id,
                &SourceBatch {
                    items: vec![article("prompted", "https://example.com/prompted", "Prompted")],
                    cursor: None,
                    not_modified: false,
                },
            )
            .unwrap();

        service
            .set_source_processing_prompt(
                &source.id,
                &user_id,
                false,
                "Translate summaries to German; remove product launches.",
            )
            .unwrap();

        let sources = service.list_sources(&user_id, false).unwrap();
        assert_eq!(
            sources[0].processing_prompt,
            "Translate summaries to German; remove product launches."
        );
        let requeued: i64 = pool
            .with_connection(|connection| {
                connection.query_row(
                    "SELECT count(*) FROM jobs
                     WHERE idempotency_key LIKE 'GenerateSummary:%:source-prompt:%'",
                    [],
                    |row| row.get(0),
                )
            })
            .unwrap();
        assert_eq!(requeued, 1);
    }

    #[test]
    fn deleted_source_item_removes_its_orphaned_story_and_keeps_tombstone() {
        let (pool, service, user_id, source) = setup();
        let original = article("gone", "https://example.com/gone", "Temporary report");
        service
            .ingest_batch(
                &source.id,
                &SourceBatch {
                    items: vec![original.clone()],
                    cursor: None,
                    not_modified: false,
                },
            )
            .unwrap();
        let deleted = RawSourceItem {
            title: Some("Deleted report".into()),
            body_text: None,
            body_html: None,
            source_url: None,
            published_at: None,
            edited_at: None,
            deleted_at: Some(1_755_500_000),
            external_urls: Vec::new(),
            media: Vec::new(),
            metadata: json!({"deleted": true}),
            ..original
        };
        service
            .ingest_batch(
                &source.id,
                &SourceBatch {
                    items: vec![deleted],
                    cursor: None,
                    not_modified: false,
                },
            )
            .unwrap();

        assert!(
            service
                .search(&user_id, "Temporary", 10)
                .unwrap()
                .is_empty()
        );
        let state: (i64, i64, i64) = pool
            .with_connection(|connection| {
                connection.query_row(
                    "SELECT deleted_at IS NOT NULL,
                     (SELECT count(*) FROM documents), (SELECT count(*) FROM stories)
                     FROM raw_items WHERE external_id='gone'",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
            })
            .unwrap();
        assert_eq!(state, (1, 0, 0));
    }

    #[test]
    fn collection_parent_and_children_are_normalized_relationships() {
        let (pool, service, user_id, source) = setup();
        let mut roundup = article("roundup", "https://feed.example/roundup", "Weekly links");
        roundup.body_text = Some("1. One\nhttps://one.example/a\n2. Two\nhttps://two.example/b\n3. Three\nhttps://three.example/c".into());
        let report = service
            .ingest_batch(
                &source.id,
                &SourceBatch {
                    items: vec![roundup],
                    cursor: None,
                    not_modified: false,
                },
            )
            .unwrap();
        assert_eq!(report.collection_parents, 1);
        assert_eq!(report.collection_children, 3);
        let relationships: i64 = pool
            .with_connection(|connection| {
                connection.query_row(
                    "SELECT count(*) FROM collection_entries ce JOIN collection_expansions cx\n\
             ON cx.id = ce.expansion_id WHERE cx.parent_raw_item_id = ce.parent_raw_item_id",
                    [],
                    |row| row.get(0),
                )
            })
            .unwrap();
        assert_eq!(relationships, 3);
        let parent_documents: i64 = pool
            .with_connection(|connection| {
                connection.query_row(
                    "SELECT count(*) FROM documents WHERE title='Weekly links'",
                    [],
                    |row| row.get(0),
                )
            })
            .unwrap();
        assert_eq!(
            parent_documents, 0,
            "children_only hides the parent candidate"
        );
        let raw_item_id: String = pool
            .with_connection(|connection| {
                connection.query_row(
                    "SELECT id FROM raw_items WHERE external_id='roundup'",
                    [],
                    |row| row.get(0),
                )
            })
            .unwrap();
        let debug = service
            .collection_debug(&raw_item_id, &user_id, false)
            .unwrap();
        assert_eq!(debug.expansions[0].entries.len(), 3);
        service
            .set_collection_override(&raw_item_id, &user_id, false, DetectionMode::ForceSingle)
            .unwrap();
        service.detect_raw_collection(&raw_item_id).unwrap();
        let debug = service
            .collection_debug(&raw_item_id, &user_id, false)
            .unwrap();
        assert_eq!(debug.override_mode.as_deref(), Some("force_single"));
        assert_eq!(debug.expansions[0].status, "rejected");
    }

    #[test]
    fn derived_document_keeps_per_link_commentary_in_curator_provenance() {
        let (pool, service, _user_id, source) = setup();
        let mut roundup = article("commentary-roundup", "https://feed.example/roundup", "Links");
        roundup.body_text = Some(
            "1. Curator says this planner analysis is unusually clear\nhttps://one.example/a\n2. Two\nhttps://two.example/b\n3. Three\nhttps://three.example/c".into(),
        );
        service
            .ingest_batch(
                &source.id,
                &SourceBatch {
                    items: vec![roundup],
                    cursor: None,
                    not_modified: false,
                },
            )
            .unwrap();
        let parent_raw_item_id: String = pool
            .with_connection(|connection| {
                connection.query_row(
                    "SELECT id FROM raw_items WHERE external_id='commentary-roundup'",
                    [],
                    |row| row.get(0),
                )
            })
            .unwrap();
        let derived_identity =
            derived_identity("commentary-roundup", "https://one.example/a").unwrap();
        let document = NormalizedDocument {
            visibility_scope: source.visibility_scope.clone(),
            title: "SQLite planner analysis".into(),
            body_text: "The extracted publisher article body remains distinct from the curator note."
                .into(),
            sanitized_html: Some("<p>The extracted publisher article body.</p>".into()),
            author: Some("Publisher author".into()),
            publisher: Some("one.example".into()),
            canonical_url: Some("https://one.example/a".into()),
            links: Vec::new(),
            language: Some("en".into()),
            published_at: Some(1_755_432_000),
        };
        let article = ExtractedArticle {
            content_checksum: content_checksum(&document.title, &document.body_text),
            document,
            images: Vec::new(),
        };
        service
            .process_derived_article(
                &ProcessDerivedPayload {
                    source_instance_id: source.id,
                    parent_raw_item_id,
                    target_url: "https://one.example/a".into(),
                    derived_identity,
                },
                &article,
            )
            .unwrap();

        let provenance: (String, String, String) = pool
            .with_connection(|connection| {
                connection.query_row(
                    "SELECT ce.commentary, d.title, d.body_text
                     FROM document_curators dc
                     JOIN collection_entries ce ON ce.id=dc.collection_entry_id
                     JOIN documents d ON d.id=dc.document_id
                     WHERE d.canonical_url='https://one.example/a'",
                    [],
                    |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
                )
            })
            .unwrap();
        assert_eq!(
            provenance.0,
            "1. Curator says this planner analysis is unusually clear"
        );
        assert_eq!(provenance.1, "SQLite planner analysis");
        assert!(!provenance.2.contains("Curator says"));
    }

    #[test]
    fn late_collection_detection_removes_an_already_normalized_hidden_parent() {
        let (pool, service, _user_id, source) = setup();
        let mut roundup = article("late-roundup", "https://feed.example/late", "Late links");
        roundup.body_text = Some(
            "1. One\nhttps://one.example/a\n2. Two\nhttps://two.example/b\n3. Three\nhttps://three.example/c".into(),
        );
        let raw_id = service
            .upsert_raw_item(&source.id, "user:test", &roundup)
            .unwrap();
        assert!(service.normalize_raw_item(&raw_id).unwrap());
        assert_eq!(
            pool.with_connection(|connection| connection.query_row(
                "SELECT count(*) FROM documents WHERE title='Late links'",
                [],
                |row| row.get::<_, i64>(0),
            ))
            .unwrap(),
            1
        );

        assert_eq!(service.detect_raw_collection(&raw_id).unwrap(), 3);
        assert_eq!(
            pool.with_connection(|connection| connection.query_row(
                "SELECT count(*) FROM documents WHERE title='Late links'",
                [],
                |row| row.get::<_, i64>(0),
            ))
            .unwrap(),
            0
        );
    }

    #[test]
    fn private_search_never_leaks_to_another_user() {
        let (_pool, service, owner, source) = setup();
        service
            .ingest_batch(
                &source.id,
                &SourceBatch {
                    items: vec![article(
                        "private",
                        "https://example.com/private",
                        "Private telescope",
                    )],
                    cursor: None,
                    not_modified: false,
                },
            )
            .unwrap();
        assert_eq!(
            service
                .search(&owner, "Private telescope", 10)
                .unwrap()
                .len(),
            1
        );
        assert!(
            service
                .search("different-user", "Private telescope", 10)
                .unwrap()
                .is_empty()
        );
    }
}
