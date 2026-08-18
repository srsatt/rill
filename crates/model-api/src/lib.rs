use std::collections::BTreeMap;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;

mod http;

pub use http::{
    HttpProviderConfig, HttpRecommendationProvider, OpenAiCompatibleProvider,
    OpenAiCompatibleRecommendationProvider,
};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ModelIdentity {
    pub provider: String,
    pub model: String,
    pub version: String,
}

#[derive(Debug, Clone)]
pub struct EmbeddingInput {
    pub id: String,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct EmbeddingOutput {
    pub id: String,
    pub vector: Vec<f32>,
}

#[derive(Debug, Clone)]
pub struct SummaryRequest {
    pub title: String,
    pub source: Option<String>,
    pub author: Option<String>,
    pub canonical_url: Option<String>,
    pub language: Option<String>,
    pub text: String,
}

#[derive(Debug, Clone)]
pub struct SummaryResponse {
    pub text: String,
    pub tags: Vec<TopicTag>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TopicTag {
    pub label: String,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RankCandidate {
    pub story_id: String,
    pub title: String,
    pub summary: String,
    pub topics: Vec<String>,
    pub publisher: Option<String>,
    pub freshness: f32,
    pub coverage: u32,
    pub local_score: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RankRequest {
    pub user_key: String,
    pub stream_slug: String,
    pub ranking_instruction: Option<String>,
    pub candidates: Vec<RankCandidate>,
    pub result_count: usize,
    pub ui_mode: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RankedCandidate {
    pub story_id: String,
    pub score: f32,
    #[serde(default)]
    pub features: BTreeMap<String, f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RankResponse {
    pub request_id: String,
    pub ranked: Vec<RankedCandidate>,
}

#[derive(Debug, Clone)]
pub struct RecommendationFeedbackEvent {
    pub event_id: String,
    pub user_key: String,
    pub story_id: String,
    pub feedback: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionParseRequest {
    pub source_kind: String,
    pub title: Option<String>,
    pub cleaned_text: String,
    pub allowed_urls: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionParseEntry {
    pub url: String,
    pub title_hint: Option<String>,
    pub commentary: Option<String>,
    pub author_hint: Option<String>,
    pub confidence: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionParseResponse {
    pub is_collection: bool,
    pub confidence: f32,
    pub entries: Vec<CollectionParseEntry>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelHealth {
    pub ready: bool,
    pub detail: String,
}

#[derive(Debug, Error)]
pub enum ModelError {
    #[error("model request failed: {0}")]
    Request(String),
    #[error("model returned invalid output: {0}")]
    InvalidOutput(String),
    #[error("model is unavailable: {0}")]
    Unavailable(String),
}

#[async_trait]
pub trait EmbeddingProvider: Send + Sync {
    fn identity(&self) -> ModelIdentity;
    async fn embed(&self, input: &[EmbeddingInput]) -> Result<Vec<EmbeddingOutput>, ModelError>;
    async fn health(&self) -> Result<ModelHealth, ModelError>;
}

#[async_trait]
pub trait SummaryProvider: Send + Sync {
    fn identity(&self) -> ModelIdentity;
    async fn summarize(&self, request: SummaryRequest) -> Result<SummaryResponse, ModelError>;
    async fn health(&self) -> Result<ModelHealth, ModelError>;
}

#[async_trait]
pub trait RecommendationProvider: Send + Sync {
    fn identity(&self) -> ModelIdentity;
    async fn rank(&self, request: RankRequest) -> Result<RankResponse, ModelError>;
    async fn submit_feedback(
        &self,
        events: &[RecommendationFeedbackEvent],
    ) -> Result<(), ModelError>;
    async fn health(&self) -> Result<ModelHealth, ModelError>;
}

#[async_trait]
pub trait CollectionParserProvider: Send + Sync {
    fn identity(&self) -> ModelIdentity;
    async fn parse_collection(
        &self,
        request: CollectionParseRequest,
    ) -> Result<CollectionParseResponse, ModelError>;
    async fn health(&self) -> Result<ModelHealth, ModelError>;
}

/// Resource-bounded feature-hashing embeddings. Deterministic local provider;
/// useful offline and as a compatibility-safe fallback.
#[derive(Debug, Clone)]
pub struct FeatureHashEmbeddingProvider {
    dimension: usize,
}

impl FeatureHashEmbeddingProvider {
    pub fn new(dimension: usize) -> Result<Self, ModelError> {
        if !(16..=4096).contains(&dimension) {
            return Err(ModelError::InvalidOutput(
                "embedding dimension must be between 16 and 4096".into(),
            ));
        }
        Ok(Self { dimension })
    }
}

#[async_trait]
impl EmbeddingProvider for FeatureHashEmbeddingProvider {
    fn identity(&self) -> ModelIdentity {
        ModelIdentity {
            provider: "rill-local".into(),
            model: "feature-hash".into(),
            version: "1".into(),
        }
    }

    async fn embed(&self, input: &[EmbeddingInput]) -> Result<Vec<EmbeddingOutput>, ModelError> {
        Ok(input
            .iter()
            .map(|item| EmbeddingOutput {
                id: item.id.clone(),
                vector: feature_hash(&item.text, self.dimension),
            })
            .collect())
    }

    async fn health(&self) -> Result<ModelHealth, ModelError> {
        Ok(ModelHealth {
            ready: true,
            detail: "local provider ready".into(),
        })
    }
}

#[derive(Debug, Clone, Default)]
pub struct ExtractiveSummaryProvider;

#[async_trait]
impl SummaryProvider for ExtractiveSummaryProvider {
    fn identity(&self) -> ModelIdentity {
        ModelIdentity {
            provider: "rill-local".into(),
            model: "extractive-summary".into(),
            version: "1".into(),
        }
    }

    async fn summarize(&self, request: SummaryRequest) -> Result<SummaryResponse, ModelError> {
        let tags = heuristic_topic_tags(&request.title, &request.text);
        let mut sentences = split_sentences(&request.text);
        sentences.retain(|sentence| sentence.split_whitespace().count() >= 5);
        let text = sentences.into_iter().take(2).collect::<Vec<_>>().join(" ");
        Ok(SummaryResponse {
            text: if text.is_empty() { request.title } else { text },
            tags,
        })
    }

    async fn health(&self) -> Result<ModelHealth, ModelError> {
        Ok(ModelHealth {
            ready: true,
            detail: "local provider ready".into(),
        })
    }
}

pub(crate) fn heuristic_topic_tags(title: &str, text: &str) -> Vec<TopicTag> {
    let mut counts = BTreeMap::<String, u32>::new();
    for (value, weight) in [(title, 4_u32), (text, 1_u32)] {
        for token in value
            .split(|character: char| !character.is_alphanumeric())
            .map(str::to_lowercase)
            .filter(|token| (4..=32).contains(&token.chars().count()))
            .filter(|token| !topic_stopword(token))
            .take(8_192)
        {
            *counts.entry(token).or_default() += weight;
        }
    }
    let mut ranked = counts.into_iter().collect::<Vec<_>>();
    ranked.sort_by(|(left_label, left_count), (right_label, right_count)| {
        right_count
            .cmp(left_count)
            .then_with(|| left_label.cmp(right_label))
    });
    let maximum = ranked.first().map_or(1, |(_, count)| *count).max(1) as f32;
    ranked
        .into_iter()
        .take(6)
        .map(|(label, count)| TopicTag {
            label,
            confidence: (0.45 + 0.5 * count as f32 / maximum).min(0.95),
        })
        .collect()
}

fn topic_stopword(value: &str) -> bool {
    matches!(
        value,
        "about"
            | "after"
            | "also"
            | "been"
            | "before"
            | "being"
            | "between"
            | "could"
            | "from"
            | "have"
            | "into"
            | "more"
            | "most"
            | "other"
            | "over"
            | "such"
            | "than"
            | "that"
            | "their"
            | "there"
            | "these"
            | "they"
            | "this"
            | "through"
            | "under"
            | "using"
            | "were"
            | "what"
            | "when"
            | "where"
            | "which"
            | "while"
            | "with"
            | "would"
    )
}

fn feature_hash(text: &str, dimension: usize) -> Vec<f32> {
    let mut vector = vec![0.0_f32; dimension];
    for token in text
        .split(|character: char| !character.is_alphanumeric())
        .filter(|token| token.len() > 1)
        .take(8_192)
    {
        let lowercase = token.to_lowercase();
        let digest = Sha256::digest(lowercase.as_bytes());
        let index = usize::from(u16::from_le_bytes([digest[0], digest[1]])) % dimension;
        let sign = if digest[2] & 1 == 0 { 1.0 } else { -1.0 };
        vector[index] += sign;
    }
    let norm = vector.iter().map(|value| value * value).sum::<f32>().sqrt();
    if norm > 0.0 {
        for value in &mut vector {
            *value /= norm;
        }
    }
    vector
}

fn split_sentences(text: &str) -> Vec<String> {
    let mut sentences = Vec::new();
    let mut start = 0;
    for (index, character) in text.char_indices() {
        if matches!(character, '.' | '!' | '?') {
            let end = index + character.len_utf8();
            let sentence = text[start..end]
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ");
            if !sentence.is_empty() {
                sentences.push(sentence);
            }
            start = end;
        }
    }
    let tail = text[start..]
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    if !tail.is_empty() {
        sentences.push(tail);
    }
    sentences
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn local_embeddings_are_normalized_and_deterministic() {
        let provider = FeatureHashEmbeddingProvider::new(32).unwrap();
        let input = [EmbeddingInput {
            id: "one".into(),
            text: "Rust compiles safe systems software".into(),
        }];
        let first = provider.embed(&input).await.unwrap();
        let second = provider.embed(&input).await.unwrap();
        assert_eq!(first[0].vector, second[0].vector);
        let norm = first[0]
            .vector
            .iter()
            .map(|value| value * value)
            .sum::<f32>();
        assert!((norm - 1.0).abs() < 0.0001);
    }

    #[tokio::test]
    async fn local_summary_uses_concrete_opening_sentences() {
        let summary = ExtractiveSummaryProvider
            .summarize(SummaryRequest {
                title: "Title".into(),
                source: None,
                author: None,
                canonical_url: None,
                language: None,
                text: "Germany approved the procurement change on Tuesday. The first migrations begin in October. A third sentence is omitted.".into(),
            })
            .await
            .unwrap();
        assert!(summary.text.contains("Germany approved"));
        assert!(!summary.text.contains("third sentence"));
    }
}
