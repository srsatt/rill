fn device_model(device: ReaderDevice) -> ReaderDeviceModel {
    ReaderDeviceModel {
        id: device.id,
        label: device.label,
        created_at: device.created_at,
        last_used_at: device.last_used_at,
        expires_at: device.expires_at,
        user_agent: device.user_agent,
    }
}

async fn load_stream_feed(
    state: &AppState,
    user_id: &str,
    username: &str,
    slug: &str,
    ui_mode: &str,
    page: u32,
    page_size: usize,
) -> Result<FeedPageModel, Response> {
    let requested = usize::try_from(page)
        .unwrap_or(usize::MAX)
        .saturating_mul(page_size)
        .saturating_add(1)
        .min(101);
    let intelligence = state.intelligence.clone();
    let user_id_owned = user_id.to_owned();
    let slug_owned = slug.to_owned();
    let ui_mode_owned = ui_mode.to_owned();
    let loaded = tokio::task::spawn_blocking(move || {
        let streams = intelligence.list_streams(&user_id_owned)?;
        let ranked = intelligence.rank_stream_now(
            &user_id_owned,
            &slug_owned,
            requested,
            &ui_mode_owned,
        )?;
        let preferences = intelligence.user_preferences(&user_id_owned)?;
        Ok::<_, IntelligenceError>((streams, ranked, preferences))
    })
    .await;
    let (streams, ranked, preferences) = match loaded {
        Ok(Ok(loaded)) => loaded,
        Ok(Err(error)) => return Err(intelligence_error(error)),
        Err(error) => {
            error!(error = %error, "stream feed task failed");
            return Err(api_error(
                StatusCode::INTERNAL_SERVER_ERROR,
                "stream unavailable",
            ));
        }
    };
    let start = usize::try_from(page.saturating_sub(1))
        .unwrap_or(usize::MAX)
        .saturating_mul(page_size);
    let has_next = ranked.len() > start.saturating_add(page_size);
    let stories = ranked
        .into_iter()
        .skip(start)
        .take(page_size)
        .collect::<Vec<_>>();
    let title = streams
        .iter()
        .find(|stream| stream.slug == slug)
        .map_or_else(|| "Rill".to_owned(), |stream| stream.name.clone());
    Ok(FeedPageModel {
        title,
        active_stream: slug.to_owned(),
        streams: streams
            .into_iter()
            .map(|stream| StreamLink {
                name: stream.name,
                slug: stream.slug,
            })
            .collect(),
        stories: stories.into_iter().map(story_card).collect(),
        username: username.to_owned(),
        font_family: preferences.font_family,
        page,
        previous_page: (page > 1).then_some(page - 1),
        next_page: has_next.then_some(page + 1),
    })
}

fn story_page_model(
    detail: StoryDetailView,
    username: String,
    font_family: String,
    reader: bool,
) -> StoryPageModel {
    StoryPageModel {
        title: detail.representative.title.clone(),
        username,
        font_family,
        story_id: detail.story_id,
        representative: story_variant_model(detail.representative),
        variants: detail
            .variants
            .into_iter()
            .map(story_variant_model)
            .collect(),
        coverage_count: detail.coverage_count,
        read: detail.read,
        favorite: detail.favorite,
        explicit_feedback: detail.explicit_feedback,
        reader,
    }
}

fn story_variant_model(variant: StoryVariantView) -> StoryVariantModel {
    let body_text = presentable_body_text(
        &variant.body_text,
        &variant.title,
        variant.canonical_url.as_deref(),
    );
    let summary = presentable_body_text(
        &variant.summary,
        &variant.title,
        variant.canonical_url.as_deref(),
    );
    StoryVariantModel {
        document_id: variant.document_id,
        title: variant.title,
        summary,
        body_text,
        canonical_url: variant.canonical_url,
        links: variant
            .links
            .into_iter()
            .map(|link| rill_contracts::StoryLinkModel {
                url: link.url,
                relation: link.relation,
                title: link.title,
            })
            .collect(),
        author: variant.author,
        publisher: variant.publisher,
        language: variant.language,
        published_at: variant
            .published_at
            .and_then(|timestamp| DateTime::<Utc>::from_timestamp(timestamp, 0))
            .map(|date| date.to_rfc3339()),
        curators: variant
            .curators
            .into_iter()
            .map(|path| CuratorPathModel {
                kind: path.kind,
                curator_id: path.curator_id,
                source_name: path.source_name,
                curator_commentary: path.curator_commentary,
                parent_title: path.parent_title,
                parent_url: path.parent_url,
            })
            .collect(),
        selected: variant.selected,
    }
}

fn presentable_body_text(body_text: &str, title: &str, canonical_url: Option<&str>) -> String {
    let trimmed = body_text.trim();
    let json_payload = serde_json::from_str::<serde_json::Value>(trimmed)
        .ok()
        .is_some_and(|value| value.is_object() || value.is_array());
    let prefix = trimmed
        .chars()
        .take(256)
        .collect::<String>()
        .to_ascii_lowercase();
    let markup_payload = prefix.starts_with("<!doctype")
        || prefix.starts_with("<html")
        || prefix.starts_with("<body")
        || prefix.starts_with("<script")
        || prefix.starts_with("<?xml");
    if !json_payload && !markup_payload {
        return body_text.to_owned();
    }
    let title = bounded_card_text(title.trim(), 300);
    let is_youtube = canonical_url.is_some_and(|url| {
        let url = url.to_ascii_lowercase();
        url.contains("youtube.com/") || url.contains("youtu.be/")
    });
    if is_youtube {
        format!("Watch “{title}” on YouTube.")
    } else {
        format!("Open “{title}” at the original source.")
    }
}

fn story_card(hit: RankedStory) -> StoryCardModel {
    let summary = presentable_body_text(
        &hit.summary,
        &hit.title,
        hit.canonical_url.as_deref(),
    );
    let word_count = summary.split_whitespace().count();
    StoryCardModel {
        id: hit.story_id,
        title: hit.title,
        summary: if summary.trim().is_empty() {
            "No summary available.".to_owned()
        } else {
            summary
        },
        source: hit
            .publisher
            .map(|publisher| bounded_card_text(&publisher, 120))
            .unwrap_or_else(|| "Unknown publisher".to_owned()),
        source_ids: hit.source_ids,
        canonical_url: hit.canonical_url,
        curator: None,
        published_at: hit
            .published_at
            .and_then(|timestamp| DateTime::<Utc>::from_timestamp(timestamp, 0))
            .map(|date| date.to_rfc3339())
            .unwrap_or_default(),
        read: hit.read,
        coverage_count: hit.coverage,
        reading_minutes: word_count.div_ceil(200).max(1) as u32,
        tags: hit.topics,
    }
}

fn bounded_card_text(value: &str, maximum_chars: usize) -> String {
    let mut characters = value.chars();
    let mut bounded = characters.by_ref().take(maximum_chars).collect::<String>();
    if characters.next().is_some() {
        bounded.push('…');
    }
    bounded
}

#[cfg(test)]
mod view_model_tests {
    use super::{bounded_card_text, presentable_body_text, story_card};
    use rill_intelligence::RankedStory;
    use serde_json::json;

    #[test]
    fn story_card_preserves_full_title_and_summary() {
        let title = "title ".repeat(80);
        let summary = "summary ".repeat(250);
        let card = story_card(RankedStory {
            story_id: "story-1".into(),
            document_id: "document-1".into(),
            title: title.clone(),
            summary: summary.clone(),
            canonical_url: None,
            publisher: Some("Publisher".into()),
            source_ids: vec!["source-1".into()],
            published_at: None,
            read: true,
            coverage: 1,
            topics: Vec::new(),
            score: 0.0,
            explanation: json!({}),
        });

        assert_eq!(card.title, title);
        assert_eq!(card.summary, summary);
        assert_eq!(card.source_ids, ["source-1"]);
        assert!(card.read);
    }

    #[test]
    fn card_text_is_bounded_at_unicode_character_boundaries() {
        assert_eq!(bounded_card_text("café東京", 5), "café東…");
        assert_eq!(bounded_card_text("short", 5), "short");
    }

    #[test]
    fn machine_payload_body_uses_meaningful_placeholder() {
        assert_eq!(
            presentable_body_text(
                r#"{"streamingData":{"signatureCipher":"opaque-key"}}"#,
                "Интервью о SQLite",
                Some("https://youtu.be/fixture"),
            ),
            "Watch “Интервью о SQLite” on YouTube."
        );
        assert_eq!(
            presentable_body_text(
                "<html><body>machine output</body></html>",
                "Readable title",
                Some("https://example.com/item"),
            ),
            "Open “Readable title” at the original source."
        );
    }

    #[test]
    fn story_card_hides_stored_machine_summary() {
        let card = story_card(RankedStory {
            story_id: "story-1".into(),
            document_id: "document-1".into(),
            title: "Интервью о SQLite".into(),
            summary: r#"{"streamingData":{"signatureCipher":"opaque-key"}}"#.into(),
            canonical_url: Some("https://www.youtube.com/watch?v=fixture".into()),
            publisher: Some("YouTube".into()),
            source_ids: vec!["source-1".into()],
            published_at: None,
            read: false,
            coverage: 1,
            topics: Vec::new(),
            score: 0.0,
            explanation: json!({}),
        });

        assert_eq!(card.summary, "Watch “Интервью о SQLite” on YouTube.");
    }
}
