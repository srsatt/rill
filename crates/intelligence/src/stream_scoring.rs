fn validate_stream(name: &str, slug: &str) -> Result<(), IntelligenceError> {
    if name.trim().is_empty() || name.chars().count() > 80 {
        return Err(IntelligenceError::Invalid("stream name is invalid".into()));
    }
    if slug.is_empty()
        || slug.len() > 64
        || slug.starts_with('-')
        || slug.ends_with('-')
        || !slug
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
    {
        return Err(IntelligenceError::Invalid("stream slug is invalid".into()));
    }
    Ok(())
}

fn matches_filter(candidate: &Candidate, filter: &StreamFilter) -> bool {
    contains_any_or_empty(&candidate.sources, &filter.include_sources)
        && contains_none(&candidate.sources, &filter.exclude_sources)
        && contains_any_or_empty(&candidate.curators, &filter.include_curators)
        && contains_none(&candidate.curators, &filter.exclude_curators)
        && (filter.include_publishers.is_empty()
            || candidate
                .publisher
                .as_ref()
                .is_some_and(|value| filter.include_publishers.contains(value)))
        && candidate
            .publisher
            .as_ref()
            .is_none_or(|value| !filter.exclude_publishers.contains(value))
        && contains_any_case_insensitive_or_empty(&candidate.topics, &filter.include_topics)
        && contains_none_case_insensitive(&candidate.topics, &filter.exclude_topics)
        && (filter.languages.is_empty()
            || candidate
                .language
                .as_ref()
                .is_some_and(|value| filter.languages.contains(value)))
        && filter.text_query.as_ref().is_none_or(|query| {
            let query = query.to_lowercase();
            candidate.title.to_lowercase().contains(&query)
                || candidate.summary.to_lowercase().contains(&query)
        })
        && filter.maximum_age_hours.is_none_or(|hours| {
            candidate.published_at.is_none_or(|published| {
                unix_now().saturating_sub(published) <= i64::from(hours) * 3600
            })
        })
        && filter
            .minimum_coverage
            .is_none_or(|minimum| candidate.coverage >= minimum)
        && filter.read.is_none_or(|read| candidate.read == read)
        && filter
            .favorite
            .is_none_or(|favorite| candidate.favorite == favorite)
}

fn score_candidate(
    candidate: &mut Candidate,
    user_id: &str,
    pool: &rill_db::DbPool,
    positive: Option<&[f32]>,
    negative: Option<&[f32]>,
    stream: Option<&[f32]>,
) -> Result<(), IntelligenceError> {
    let age_hours = candidate.published_at.map_or(72.0, |published| {
        unix_now().saturating_sub(published).max(0) as f32 / 3600.0
    });
    let freshness = 1.0 / (1.0 + age_hours / 72.0);
    let coverage = (candidate.coverage.max(1) as f32).ln_1p() * 0.12;
    let affinity = affinity_score(
        pool,
        user_id,
        candidate.publisher.as_deref(),
        &candidate.sources,
        &candidate.curators,
    )?;
    let positive_similarity = candidate
        .vector
        .as_deref()
        .zip(positive)
        .and_then(|(candidate, centroid)| cosine(candidate, centroid))
        .unwrap_or(0.0);
    let negative_similarity = candidate
        .vector
        .as_deref()
        .zip(negative)
        .and_then(|(candidate, centroid)| cosine(candidate, centroid))
        .unwrap_or(0.0);
    let stream_similarity = candidate
        .vector
        .as_deref()
        .zip(stream)
        .and_then(|(candidate, stream)| cosine(candidate, stream))
        .unwrap_or(0.0);
    candidate.score = freshness * 0.45 + coverage + affinity * 0.25 + positive_similarity * 0.25
        - negative_similarity * 0.35
        + stream_similarity * 0.25;
    candidate.explanation = json!({
        "freshness": freshness,
        "coverage": coverage,
        "affinity": affinity,
        "positiveSimilarity": positive_similarity,
        "negativeSimilarity": negative_similarity,
        "streamSimilarity": stream_similarity,
        "fallback": true,
    });
    Ok(())
}

pub(crate) fn affinity_score(
    pool: &rill_db::DbPool,
    user_id: &str,
    publisher: Option<&str>,
    sources: &[String],
    curators: &[String],
) -> Result<f32, IntelligenceError> {
    let mut subjects = Vec::new();
    if let Some(publisher) = publisher {
        subjects.push(("publisher", publisher));
    }
    subjects.extend(sources.iter().map(|value| ("source", value.as_str())));
    subjects.extend(curators.iter().map(|value| ("curator", value.as_str())));
    if subjects.is_empty() {
        return Ok(0.0);
    }
    let connection = pool.connection()?;
    let mut total = 0.0_f64;
    for (kind, id) in &subjects {
        let mut statement = connection.prepare(
            "SELECT weight, created_at FROM source_affinity_events
             WHERE user_id=?1 AND subject_kind=?2 AND subject_id=?3",
        )?;
        let events = statement.query_map(params![user_id, kind, id], |row| {
            Ok((row.get::<_, f64>(0)?, row.get::<_, i64>(1)?))
        })?;
        let mut positive = 2.0;
        let mut negative = 2.0;
        for event in events {
            let (weight, created_at) = event?;
            let age_days = unix_now().saturating_sub(created_at).max(0) as f64 / 86_400.0;
            let decayed = weight.abs() * 0.5_f64.powf(age_days / 90.0);
            if weight >= 0.0 {
                positive += decayed;
            } else {
                negative += decayed;
            }
        }
        total += positive / (positive + negative) - 0.5;
    }
    Ok((total / subjects.len() as f64) as f32)
}

fn diversify(
    mut candidates: Vec<Candidate>,
    limit: usize,
    user_id: &str,
    slug: &str,
) -> Vec<Candidate> {
    let mut selected = Vec::new();
    let mut publisher_counts = HashMap::<String, usize>::new();
    while !candidates.is_empty() && selected.len() < limit {
        let (index, _) = candidates
            .iter()
            .enumerate()
            .max_by(|(_, left), (_, right)| {
                adjusted_score(left, &publisher_counts)
                    .total_cmp(&adjusted_score(right, &publisher_counts))
            })
            .expect("non-empty candidates");
        let mut candidate = candidates.swap_remove(index);
        let penalty = candidate
            .publisher
            .as_ref()
            .and_then(|publisher| publisher_counts.get(publisher))
            .copied()
            .unwrap_or(0) as f32
            * 0.10;
        candidate.score -= penalty;
        candidate.explanation["diversityPenalty"] = json!(penalty);
        if let Some(publisher) = &candidate.publisher {
            *publisher_counts.entry(publisher.clone()).or_default() += 1;
        }
        selected.push(candidate);
    }
    if selected.len() >= 10 && !candidates.is_empty() {
        let day = unix_now() / 86_400;
        let digest = Sha256::digest(format!("{user_id}:{slug}:{day}").as_bytes());
        let index = usize::from(u16::from_le_bytes([digest[0], digest[1]])) % candidates.len();
        let mut exploration = candidates.swap_remove(index);
        exploration.explanation["exploration"] = json!(true);
        if let Some(last) = selected.last_mut() {
            *last = exploration;
        }
    }
    selected
}

fn adjusted_score(candidate: &Candidate, counts: &HashMap<String, usize>) -> f32 {
    candidate.score
        - candidate
            .publisher
            .as_ref()
            .and_then(|publisher| counts.get(publisher))
            .copied()
            .unwrap_or(0) as f32
            * 0.10
}

fn centroid(vectors: &[Vec<f32>]) -> Option<Vec<f32>> {
    let dimension = vectors.first()?.len();
    if vectors.iter().any(|vector| vector.len() != dimension) {
        return None;
    }
    let mut output = vec![0.0; dimension];
    for vector in vectors {
        for (output, value) in output.iter_mut().zip(vector) {
            *output += value;
        }
    }
    for value in &mut output {
        *value /= vectors.len() as f32;
    }
    Some(output)
}

fn contains_any_or_empty(candidate: &[String], filter: &[String]) -> bool {
    filter.is_empty() || candidate.iter().any(|value| filter.contains(value))
}

fn contains_none(candidate: &[String], filter: &[String]) -> bool {
    candidate.iter().all(|value| !filter.contains(value))
}

fn contains_any_case_insensitive_or_empty(candidate: &[String], filter: &[String]) -> bool {
    filter.is_empty()
        || candidate
            .iter()
            .any(|value| filter.iter().any(|item| topic_matches(value, item)))
}

fn contains_none_case_insensitive(candidate: &[String], filter: &[String]) -> bool {
    candidate
        .iter()
        .all(|value| filter.iter().all(|item| !topic_matches(value, item)))
}

fn topic_matches(topic: &str, filter: &str) -> bool {
    let topic = topic.trim().to_lowercase();
    let filter = filter.trim().to_lowercase();
    topic == filter
        || topic
            .strip_prefix(&filter)
            .is_some_and(topic_suffix)
        || filter
            .strip_prefix(&topic)
            .is_some_and(topic_suffix)
}

fn topic_suffix(value: &str) -> bool {
    value
        .chars()
        .next()
        .is_some_and(|character| matches!(character, ' ' | '-' | '/'))
}

fn comma_list(value: String) -> Vec<String> {
    value
        .split(',')
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect()
}
