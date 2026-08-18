pub fn provider_request(
    item: &RawSourceItem,
    base_url: Option<&Url>,
    policy: &CollectionPolicy,
) -> CollectionParseRequest {
    let forced =
        detect_collection_with_diagnostics(item, base_url, DetectionMode::ForceCollection, policy);
    let allowed_urls = match forced.shape {
        ItemShape::Collection { entries, .. } => {
            entries.into_iter().map(|entry| entry.url).collect()
        }
        ItemShape::Single => Vec::new(),
    };
    CollectionParseRequest {
        source_kind: item.item_kind.clone(),
        title: item.title.clone(),
        cleaned_text: item
            .body_text
            .as_deref()
            .unwrap_or_default()
            .chars()
            .take(16_000)
            .collect(),
        allowed_urls,
    }
}

fn parse_telegram_entity_candidates(
    item: &RawSourceItem,
    policy: &CollectionPolicy,
) -> (Vec<CollectionEntryCandidate>, bool) {
    let Some(entities) = item
        .metadata
        .get("entities")
        .and_then(|value| value.as_array())
    else {
        return (Vec::new(), false);
    };
    let mut candidates = Vec::new();
    for entity in entities {
        let Some(raw_url) = entity.get("url").and_then(|value| value.as_str()) else {
            continue;
        };
        let Some(url) = resolve_url(raw_url, None) else {
            continue;
        };
        let label = entity
            .get("label")
            .and_then(|value| value.as_str())
            .map(normalize)
            .filter(|value| anchor_is_title(value) && !value.starts_with("http"));
        if !meaningful_link(&url, label.as_deref().unwrap_or(""), policy) {
            continue;
        }
        candidates.push(CollectionEntryCandidate {
            url: url.to_string(),
            title_hint: label.clone(),
            commentary: label,
            author_hint: item.author.clone(),
            published_at_hint: item.published_at,
            ordinal: candidates.len(),
            confidence: 0.0,
        });
    }
    let structured = candidates.len() >= 3;
    (candidates, structured)
}

pub fn derived_identity(parent_identity: &str, target_url: &str) -> Result<String, String> {
    let normalized = canonicalize_url(target_url).map_err(|error| error.to_string())?;
    let mut hash = Sha256::new();
    hash.update(parent_identity.as_bytes());
    hash.update(b"\0");
    hash.update(normalized.as_bytes());
    Ok(format!("derived:{:x}", hash.finalize()))
}

