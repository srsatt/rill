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
        Ok::<_, IntelligenceError>((streams, ranked))
    })
    .await;
    let (streams, ranked) = match loaded {
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
        page,
        previous_page: (page > 1).then_some(page - 1),
        next_page: has_next.then_some(page + 1),
    })
}

fn story_page_model(detail: StoryDetailView, reader: bool) -> StoryPageModel {
    StoryPageModel {
        title: detail.representative.title.clone(),
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
    StoryVariantModel {
        document_id: variant.document_id,
        title: variant.title,
        summary: variant.summary,
        body_text: variant.body_text,
        canonical_url: variant.canonical_url,
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

fn story_card(hit: RankedStory) -> StoryCardModel {
    let word_count = hit.summary.split_whitespace().count();
    StoryCardModel {
        id: hit.story_id,
        title: bounded_card_text(&hit.title, 240),
        summary: if hit.summary.trim().is_empty() {
            "No summary available.".to_owned()
        } else {
            bounded_card_text(&hit.summary, 800)
        },
        source: hit
            .publisher
            .map(|publisher| bounded_card_text(&publisher, 120))
            .unwrap_or_else(|| "Unknown publisher".to_owned()),
        curator: None,
        published_at: hit
            .published_at
            .and_then(|timestamp| DateTime::<Utc>::from_timestamp(timestamp, 0))
            .map(|date| date.to_rfc3339())
            .unwrap_or_default(),
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
    use super::bounded_card_text;

    #[test]
    fn card_text_is_bounded_at_unicode_character_boundaries() {
        assert_eq!(bounded_card_text("café東京", 5), "café東…");
        assert_eq!(bounded_card_text("short", 5), "short");
    }
}
