fn parse_parent_display(value: &str) -> Result<ParentDisplayPolicy, IngestionError> {
    match value {
        "children_only" => Ok(ParentDisplayPolicy::ChildrenOnly),
        "parent_and_children" => Ok(ParentDisplayPolicy::ParentAndChildren),
        "parent_only" => Ok(ParentDisplayPolicy::ParentOnly),
        _ => Err(IngestionError::Invalid(
            "collection parent display policy is invalid".into(),
        )),
    }
}

fn extend_string_array(target: &mut Vec<String>, value: Option<&Value>) {
    let Some(values) = value.and_then(Value::as_array) else {
        return;
    };
    target.extend(
        values
            .iter()
            .filter_map(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty() && value.len() <= 240)
            .map(str::to_owned),
    );
    target.sort();
    target.dedup();
}

fn normalize_feed_item(
    item: &RawSourceItem,
    visibility_scope: &str,
) -> Result<Option<NormalizedDocument>, IngestionError> {
    let title = item.title.as_deref().unwrap_or_default().trim();
    let body = item.body_text.as_deref().unwrap_or_default().trim();
    if title.is_empty() && body.is_empty() {
        return Ok(None);
    }
    let canonical_url = item
        .source_url
        .as_deref()
        .map(canonicalize_url)
        .transpose()?;
    let publisher = canonical_url
        .as_deref()
        .and_then(|url| Url::parse(url).ok())
        .and_then(|url| url.host_str().map(str::to_owned));
    Ok(Some(NormalizedDocument {
        visibility_scope: visibility_scope.to_owned(),
        title: if title.is_empty() {
            canonical_url
                .clone()
                .unwrap_or_else(|| item.external_id.clone())
        } else {
            title.to_owned()
        },
        body_text: body.to_owned(),
        sanitized_html: item.body_html.as_deref().map(clean),
        author: item.author.clone(),
        publisher,
        canonical_url,
        links: item.external_urls.clone(),
        language: item
            .metadata
            .get("language")
            .and_then(Value::as_str)
            .map(str::to_owned),
        published_at: item.published_at,
    }))
}

fn safe_fts_query(query: &str) -> Result<String, IngestionError> {
    let tokens = query
        .split_whitespace()
        .filter(|token| !token.is_empty())
        .take(12)
        .map(|token| token.replace('"', "\"\""))
        .collect::<Vec<_>>();
    if tokens.is_empty() {
        return Err(IngestionError::Invalid("search query is empty".into()));
    }
    Ok(tokens
        .into_iter()
        .map(|token| format!("\"{token}\"*"))
        .collect::<Vec<_>>()
        .join(" AND "))
}

fn truncate(value: &str, maximum: usize) -> String {
    value.chars().take(maximum).collect()
}
