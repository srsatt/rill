#[derive(Debug, Clone)]
pub(crate) struct StoredDocument {
    id: String,
    visibility_scope: String,
    title: String,
    body_text: String,
    author: Option<String>,
    publisher: Option<String>,
    canonical_url: Option<String>,
    language: Option<String>,
    published_at: Option<i64>,
    input_checksum: Vec<u8>,
}

fn insert_affinity(
    transaction: &rusqlite::Transaction<'_>,
    user_id: &str,
    subject_kind: &str,
    subject_id: &str,
    signal: &str,
    weight: f64,
    story_id: &str,
) -> rusqlite::Result<()> {
    transaction.execute(
        "INSERT INTO source_affinity_events(id, user_id, subject_kind, subject_id, signal,
         weight, story_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            Uuid::new_v4().to_string(),
            user_id,
            subject_kind,
            subject_id,
            signal,
            weight,
            story_id
        ],
    )?;
    Ok(())
}

pub(crate) fn encode_vector(vector: &[f32]) -> Vec<u8> {
    vector
        .iter()
        .flat_map(|value| value.to_le_bytes())
        .collect()
}

pub(crate) fn decode_vector(bytes: &[u8]) -> Option<Vec<f32>> {
    if !bytes.len().is_multiple_of(4) {
        return None;
    }
    let mut vector = Vec::with_capacity(bytes.len() / 4);
    for chunk in bytes.chunks_exact(4) {
        vector.push(f32::from_le_bytes(chunk.try_into().ok()?));
    }
    Some(vector)
}

pub(crate) fn cosine(left: &[f32], right: &[f32]) -> Option<f32> {
    if left.len() != right.len() || left.is_empty() {
        return None;
    }
    let dot = left.iter().zip(right).map(|(a, b)| a * b).sum::<f32>();
    let left_norm = left.iter().map(|value| value * value).sum::<f32>().sqrt();
    let right_norm = right.iter().map(|value| value * value).sum::<f32>().sqrt();
    (left_norm > 0.0 && right_norm > 0.0).then_some(dot / (left_norm * right_norm))
}

fn bounded_text(text: &str, maximum_chars: usize) -> String {
    text.chars().take(maximum_chars).collect()
}

pub(crate) fn hashed_user_key(user_id: &str) -> String {
    format!("{:x}", Sha256::digest(user_id.as_bytes()))
}

pub(crate) fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_secs()).ok())
        .unwrap_or(0)
}

