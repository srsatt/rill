#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::Value;

    fn item(text: &str, html: Option<&str>) -> RawSourceItem {
        RawSourceItem {
            external_id: "parent-1".into(),
            item_kind: "article".into(),
            title: Some("Roundup".into()),
            body_text: Some(text.into()),
            body_html: html.map(str::to_owned),
            author: None,
            source_url: None,
            published_at: None,
            edited_at: None,
            deleted_at: None,
            external_urls: Vec::new(),
            media: Vec::new(),
            metadata: Value::Null,
        }
    }

    #[test]
    fn formatted_three_link_roundup_expands_with_hints() {
        let text = "1. SQLite planner\nhttps://one.example/a\n\n2. Rust async\nhttps://two.example/b\n\n3. Solid SSR\nhttps://three.example/c";
        let shape = detect_collection(
            &item(text, None),
            None,
            DetectionMode::Auto,
            &CollectionPolicy::default(),
        );
        let ItemShape::Collection { entries, .. } = shape else {
            panic!("expected collection");
        };
        assert_eq!(entries.len(), 3);
        assert_eq!(entries[0].title_hint.as_deref(), Some("SQLite planner"));
    }

    #[test]
    fn repeated_cards_preserve_heading_and_commentary() {
        let html = r#"<section><h3>SQLite planner</h3><p>Deep change.</p><a href="https://one.example/a">Read article</a></section>
        <section><h3>Rust async</h3><p>Worth it.</p><a href="https://two.example/b">Read article</a></section>
        <section><h3>Solid SSR</h3><p>Fast.</p><a href="https://three.example/c">Read article</a></section>"#;
        let shape = detect_collection(
            &item("", Some(html)),
            None,
            DetectionMode::Auto,
            &CollectionPolicy::default(),
        );
        let ItemShape::Collection { entries, .. } = shape else {
            panic!("expected collection");
        };
        assert_eq!(entries[0].title_hint.as_deref(), Some("SQLite planner"));
        assert!(
            entries[0]
                .commentary
                .as_deref()
                .unwrap()
                .contains("Deep change")
        );
    }

    #[test]
    fn ordinary_single_link_and_incidental_links_do_not_expand() {
        assert_eq!(
            detect_collection(
                &item("Read https://example.com/a", None),
                None,
                DetectionMode::Auto,
                &CollectionPolicy::default()
            ),
            ItemShape::Single,
        );
        let html = r#"<p>Article <a href="https://example.com/reference">reference</a>.</p>
        <a href="https://x.com/person">Share</a><a href="https://example.com/privacy">Privacy</a>"#;
        assert_eq!(
            detect_collection(
                &item("", Some(html)),
                None,
                DetectionMode::Auto,
                &CollectionPolicy::default()
            ),
            ItemShape::Single,
        );
    }

    #[test]
    fn telegram_digest_post_links_expand_but_profiles_do_not() {
        let html = r#"<p>AI digest:
          <a href="https://t.me/channel_one/101">First source</a>
          <a href="https://t.me/channel_two/202">Second source</a>
          <a href="https://t.me/channel_three/303">Third source</a>
          <a href="https://t.me/channel_four/404">Fourth source</a>
          <a href="https://t.me/channel_five/505">Fifth source</a>
          <a href="https://t.me/channel_profile">Profile</a>
        </p>"#;
        let shape = detect_collection(
            &item("", Some(html)),
            None,
            DetectionMode::Auto,
            &CollectionPolicy::default(),
        );
        let ItemShape::Collection { entries, .. } = shape else {
            panic!("expected Telegram digest collection");
        };
        assert_eq!(entries.len(), 5);
        assert!(entries.iter().all(|entry| entry.url.matches('/').count() >= 4));
    }

    #[test]
    fn inline_telegram_digest_preserves_per_link_curator_commentary() {
        let html = r#"
          <br><i><b>🎵</b></i> Вышла нейросеть Pika Music для генерации песен (<a href="https://t.me/source_one/101">Метаверсище и ИИще</a>)
          <br>Новая версия Qwen (<a href="https://t.me/source_two/202">AI News</a>)
          <br>DeepSeek открыл новый API (<a href="https://t.me/source_three/303">ML Channel</a>)
          <br>OpenAI опубликовала отчет (<a href="https://t.me/source_four/404">Security</a>)
          <br>Релиз новостей про агентов (<a href="https://t.me/source_five/505">Agents</a>)
        "#;
        let shape = detect_collection(
            &item("", Some(html)),
            None,
            DetectionMode::Auto,
            &CollectionPolicy::default(),
        );
        let ItemShape::Collection { entries, .. } = shape else {
            panic!("expected Telegram digest collection");
        };
        assert_eq!(entries.len(), 5);
        assert!(entries[0].commentary.as_deref().unwrap().contains("Pika Music"));
        assert!(entries[0].title_hint.as_deref().unwrap().contains("Pika Music"));
        assert_ne!(entries[0].title_hint.as_deref(), Some("Метаверсище и ИИще"));
    }

    #[test]
    fn same_url_repeated_is_one_child_with_stable_identity() {
        let text = "1. One https://one.example/a?utm_source=x\n2. Again https://one.example/a\n3. Two https://two.example/b\n4. Three https://three.example/c";
        let shape = detect_collection(
            &item(text, None),
            None,
            DetectionMode::ForceCollection,
            &CollectionPolicy::default(),
        );
        let ItemShape::Collection { entries, .. } = shape else {
            panic!("expected collection");
        };
        assert_eq!(entries.len(), 3);
        assert_eq!(
            derived_identity("parent", "https://one.example/a?utm_source=x").unwrap(),
            derived_identity("parent", "https://one.example/a").unwrap(),
        );
    }

    #[test]
    fn diagnostics_explain_filtered_links_and_source_rules() {
        let mut policy = CollectionPolicy::default();
        policy.excluded_hosts.push("blocked.example".into());
        let detection = detect_collection_with_diagnostics(
            &item(
                "1. Story https://one.example/a\n2. Privacy https://one.example/privacy\n3. Blocked https://blocked.example/a",
                None,
            ),
            None,
            DetectionMode::ForceCollection,
            &policy,
        );
        assert!(
            detection
                .ignored_links
                .iter()
                .any(|link| { link.url.contains("privacy") && link.reason == "non-content link" })
        );
        assert!(detection.ignored_links.iter().any(|link| {
            link.url.contains("blocked.example") && link.reason == "excluded hostname"
        }));
    }

    #[test]
    fn model_parser_cannot_invent_urls() {
        let request = CollectionParseRequest {
            source_kind: "email".into(),
            title: Some("Digest".into()),
            cleaned_text: "bounded".into(),
            allowed_urls: vec!["https://allowed.example/a".into()],
        };
        let response = CollectionParseResponse {
            is_collection: true,
            confidence: 0.9,
            entries: vec![rill_model_api::CollectionParseEntry {
                url: "https://invented.example/x".into(),
                title_hint: None,
                commentary: None,
                author_hint: None,
                confidence: 0.9,
            }],
        };
        assert!(matches!(
            validate_provider_response(&request, response, 25),
            Err(ModelError::InvalidOutput(_))
        ));
    }
}
