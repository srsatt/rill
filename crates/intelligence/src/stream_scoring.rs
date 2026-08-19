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
    affinities: &AffinityScores,
    positive: Option<&[f32]>,
    negative: Option<&[f32]>,
    stream: Option<&[f32]>,
    preference: Option<&PreferenceModel>,
) {
    let age_hours = candidate.published_at.map_or(72.0, |published| {
        unix_now().saturating_sub(published).max(0) as f32 / 3600.0
    });
    let freshness = 1.0 / (1.0 + age_hours / 72.0);
    let coverage = (candidate.coverage.max(1) as f32).ln_1p() * 0.12;
    let affinity = affinity_score_from(
        affinities,
        candidate.publisher.as_deref(),
        &candidate.sources,
        &candidate.curators,
    );
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
    if let Some(probability) = preference.and_then(|model| {
        model.predict(
            candidate.vector.as_deref()?,
            candidate.published_at,
            candidate.coverage,
            affinity,
        )
    }) {
        candidate.score = candidate.score * 0.75 + probability * 0.25;
        candidate.explanation["preferenceProbability"] = json!(probability);
        candidate.explanation["fallback"] = json!(false);
    }
}

pub(crate) fn affinity_score(
    pool: &rill_db::DbPool,
    user_id: &str,
    publisher: Option<&str>,
    sources: &[String],
    curators: &[String],
) -> Result<f32, IntelligenceError> {
    let scores = load_affinity_scores(pool, user_id)?;
    Ok(affinity_score_from(
        &scores, publisher, sources, curators,
    ))
}

fn load_affinity_scores(
    pool: &rill_db::DbPool,
    user_id: &str,
) -> Result<AffinityScores, IntelligenceError> {
    let connection = pool.connection()?;
    let mut statement = connection.prepare(
        "SELECT subject_kind, subject_id, weight, created_at
         FROM source_affinity_events WHERE user_id=?1",
    )?;
    let events = statement.query_map([user_id], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
            row.get::<_, f64>(2)?,
            row.get::<_, i64>(3)?,
        ))
    })?;
    let now = unix_now();
    let mut weights = HashMap::<String, HashMap<String, (f64, f64)>>::new();
    for event in events {
        let (kind, id, weight, created_at) = event?;
        let totals = weights
            .entry(kind)
            .or_default()
            .entry(id)
            .or_insert((2.0, 2.0));
        let age_days = now.saturating_sub(created_at).max(0) as f64 / 86_400.0;
        let decayed = weight.abs() * 0.5_f64.powf(age_days / 90.0);
        if weight >= 0.0 {
            totals.0 += decayed;
        } else {
            totals.1 += decayed;
        }
    }
    Ok(weights
        .into_iter()
        .map(|(kind, subjects)| {
            let subjects = subjects
                .into_iter()
                .map(|(id, (positive, negative))| {
                    (id, (positive / (positive + negative) - 0.5) as f32)
                })
                .collect();
            (kind, subjects)
        })
        .collect())
}

fn affinity_score_from(
    scores: &AffinityScores,
    publisher: Option<&str>,
    sources: &[String],
    curators: &[String],
) -> f32 {
    let mut total = 0.0;
    let mut count = 0_usize;
    if let Some(publisher) = publisher {
        total += scores
            .get("publisher")
            .and_then(|subjects| subjects.get(publisher))
            .copied()
            .unwrap_or(0.0);
        count += 1;
    }
    for (kind, ids) in [("source", sources), ("curator", curators)] {
        for id in ids {
            total += scores
                .get(kind)
                .and_then(|subjects| subjects.get(id))
                .copied()
                .unwrap_or(0.0);
            count += 1;
        }
    }
    if count == 0 { 0.0 } else { total / count as f32 }
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
                    .then_with(|| right.story_id.cmp(&left.story_id))
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

fn control_list(value: String) -> Vec<String> {
    value
        .split('\u{1f}')
        .filter(|value| !value.is_empty())
        .map(str::to_owned)
        .collect()
}

#[cfg(test)]
mod scoring_tests {
    use super::*;

    #[test]
    fn equal_scores_use_story_id_tie_break() {
        let candidate = |story_id: &str| Candidate {
            story_id: story_id.into(),
            document_id: story_id.into(),
            title: story_id.into(),
            summary: String::new(),
            canonical_url: None,
            publisher: None,
            language: None,
            published_at: None,
            coverage: 1,
            topics: Vec::new(),
            sources: Vec::new(),
            curators: Vec::new(),
            read: false,
            favorite: false,
            vector: None,
            score: 1.0,
            explanation: serde_json::json!({}),
        };
        let ranked = diversify(
            vec![candidate("story-b"), candidate("story-a")],
            2,
            "user",
            "home",
        );
        assert_eq!(ranked[0].story_id, "story-a");
    }
}
