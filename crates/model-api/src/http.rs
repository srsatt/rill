use std::{
    collections::HashSet,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use async_trait::async_trait;
use futures_util::StreamExt;
use reqwest::{Client, Method, StatusCode};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::{Value, json};
use tokio::time::sleep;
use url::Url;

use crate::{
    CollectionParseRequest, CollectionParseResponse, CollectionParserProvider, EmbeddingInput,
    EmbeddingOutput, EmbeddingProvider, ModelError, ModelHealth, ModelIdentity, RankRequest,
    RankResponse, RankedCandidate, RecommendationFeedbackEvent, RecommendationProvider,
    SummaryProvider, SummaryRequest, SummaryResponse, TopicTag, heuristic_topic_tags,
};

#[derive(Clone)]
pub struct HttpProviderConfig {
    pub identity: ModelIdentity,
    pub base_url: Url,
    pub api_key: Option<String>,
    pub timeout: Duration,
    pub maximum_request_bytes: usize,
    pub maximum_response_bytes: usize,
    pub maximum_batch_items: usize,
    pub retries: usize,
    pub circuit_failure_threshold: u32,
    pub circuit_cooldown: Duration,
}

impl HttpProviderConfig {
    pub fn new(identity: ModelIdentity, base_url: Url) -> Self {
        Self {
            identity,
            base_url,
            api_key: None,
            timeout: Duration::from_secs(30),
            maximum_request_bytes: 512 * 1024,
            maximum_response_bytes: 4 * 1024 * 1024,
            maximum_batch_items: 128,
            retries: 2,
            circuit_failure_threshold: 3,
            circuit_cooldown: Duration::from_secs(30),
        }
    }

    fn validate(&self) -> Result<(), ModelError> {
        if !matches!(self.base_url.scheme(), "http" | "https") || self.base_url.host_str().is_none()
        {
            return Err(ModelError::InvalidOutput(
                "model base URL must be absolute HTTP(S)".into(),
            ));
        }
        if self.timeout.is_zero()
            || self.maximum_request_bytes < 1024
            || self.maximum_response_bytes < 1024
            || self.maximum_batch_items == 0
            || self.circuit_failure_threshold == 0
            || self.circuit_cooldown.is_zero()
        {
            return Err(ModelError::InvalidOutput(
                "model provider limits must be positive".into(),
            ));
        }
        if self.identity.provider.trim().is_empty()
            || self.identity.model.trim().is_empty()
            || self.identity.version.trim().is_empty()
        {
            return Err(ModelError::InvalidOutput(
                "model provider identity is incomplete".into(),
            ));
        }
        Ok(())
    }
}

include!("client.rs");

#[derive(Clone)]
pub struct OpenAiCompatibleProvider {
    client: BoundedJsonClient,
}

impl OpenAiCompatibleProvider {
    pub fn new(config: HttpProviderConfig) -> Result<Self, ModelError> {
        Ok(Self {
            client: BoundedJsonClient::new(config)?,
        })
    }
}

#[derive(Deserialize)]
struct OpenAiEmbeddingResponse {
    data: Vec<OpenAiEmbedding>,
}

#[derive(Deserialize)]
struct OpenAiEmbedding {
    index: usize,
    embedding: Vec<f32>,
}

#[async_trait]
impl EmbeddingProvider for OpenAiCompatibleProvider {
    fn identity(&self) -> ModelIdentity {
        self.client.config.identity.clone()
    }

    async fn embed(&self, input: &[EmbeddingInput]) -> Result<Vec<EmbeddingOutput>, ModelError> {
        if input.is_empty() || input.len() > self.client.config.maximum_batch_items {
            return Err(ModelError::InvalidOutput(
                "embedding batch size is invalid".into(),
            ));
        }
        let response = self
            .client
            .post(
                "embeddings",
                &json!({
                    "model": self.client.config.identity.model,
                    "input": input.iter().map(|item| bounded_text(&item.text, 32_000)).collect::<Vec<_>>(),
                }),
            )
            .await?;
        let mut response: OpenAiEmbeddingResponse = serde_json::from_value(response)
            .map_err(|error| ModelError::InvalidOutput(error.to_string()))?;
        response.data.sort_by_key(|item| item.index);
        if response.data.len() != input.len() {
            return Err(ModelError::InvalidOutput(
                "embedding response count differs from request".into(),
            ));
        }
        input
            .iter()
            .zip(response.data)
            .map(|(input, output)| {
                if output.embedding.is_empty()
                    || output.embedding.iter().any(|value| !value.is_finite())
                {
                    return Err(ModelError::InvalidOutput(
                        "embedding contains invalid values".into(),
                    ));
                }
                Ok(EmbeddingOutput {
                    id: input.id.clone(),
                    vector: output.embedding,
                })
            })
            .collect()
    }

    async fn health(&self) -> Result<ModelHealth, ModelError> {
        self.client.health().await
    }
}

#[async_trait]
impl SummaryProvider for OpenAiCompatibleProvider {
    fn identity(&self) -> ModelIdentity {
        self.client.config.identity.clone()
    }

    async fn summarize(&self, request: SummaryRequest) -> Result<SummaryResponse, ModelError> {
        let fallback_tags = heuristic_topic_tags(&request.title, &request.text);
        let response = self
            .client
            .post("chat/completions", &json!({
                "model": self.client.config.identity.model,
                "response_format": {"type": "json_object"},
                "temperature": 0,
                "messages": [
                    {"role": "system", "content": "Return only JSON with this shape: {\"summary\":\"two concise factual sentences\",\"tags\":[{\"label\":\"lowercase topic\",\"confidence\":0.9}]}. Produce 3-6 tags. Exactly one tag must be the best high-level category from: technology, ai, world, science, business, culture, health, environment. Remaining tags must be specific. Avoid generic tags such as news, article, or update."},
                    {"role": "user", "content": format!(
                        "Summarize this article to help decide whether it is worth reading. Prefer concrete facts, numbers, new claims, and consequences.\nTitle: {}\nSource: {}\nAuthor: {}\nLanguage: {}\nURL: {}\nText:\n{}",
                        bounded_text(&request.title, 500),
                        request.source.as_deref().unwrap_or("unknown"),
                        request.author.as_deref().unwrap_or("unknown"),
                        request.language.as_deref().unwrap_or("unknown"),
                        request.canonical_url.as_deref().unwrap_or("unknown"),
                        bounded_text(&request.text, 24_000)
                    )}
                ]
            }))
            .await?;
        let content = chat_content(&response)?;
        let (text, tags) = match parse_json_object::<ChatEnrichment>(&content) {
            Ok(enrichment) => (
                enrichment.summary.trim().to_owned(),
                validated_topics(enrichment.tags, fallback_tags),
            ),
            Err(_) => (content.trim().to_owned(), fallback_tags),
        };
        if text.is_empty() || text.chars().count() > 2_000 {
            return Err(ModelError::InvalidOutput(
                "summary length is invalid".into(),
            ));
        }
        Ok(SummaryResponse { text, tags })
    }

    async fn health(&self) -> Result<ModelHealth, ModelError> {
        self.client.health().await
    }
}

#[async_trait]
impl CollectionParserProvider for OpenAiCompatibleProvider {
    fn identity(&self) -> ModelIdentity {
        self.client.config.identity.clone()
    }

    async fn parse_collection(
        &self,
        request: CollectionParseRequest,
    ) -> Result<CollectionParseResponse, ModelError> {
        let response = self.client.post("chat/completions", &json!({
            "model": self.client.config.identity.model,
            "response_format": {"type": "json_object"},
            "temperature": 0,
            "messages": [{"role":"user", "content": format!(
                "Classify a possible link collection. Return JSON with isCollection, confidence, and entries. Entries may use only allowed URLs.\nTitle: {}\nText: {}\nAllowed URLs: {}",
                request.title.as_deref().unwrap_or(""), bounded_text(&request.cleaned_text, 16_000),
                request.allowed_urls.join("\n"))}],
        })).await?;
        let content = chat_content(&response)?;
        let mut parsed: CollectionParseResponse = parse_json_object(&content)?;
        validate_collection_response(&request, &mut parsed)?;
        Ok(parsed)
    }

    async fn health(&self) -> Result<ModelHealth, ModelError> {
        self.client.health().await
    }
}

#[derive(Debug, Deserialize)]
struct ChatEnrichment {
    summary: String,
    #[serde(default)]
    tags: Vec<ChatTopic>,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
enum ChatTopic {
    Label(String),
    Scored {
        label: String,
        #[serde(default = "default_topic_confidence")]
        confidence: f32,
    },
}

fn validated_topics(input: Vec<ChatTopic>, fallback: Vec<TopicTag>) -> Vec<TopicTag> {
    let mut seen = HashSet::new();
    let tags = input
        .into_iter()
        .filter_map(|topic| {
            let (label, confidence) = match topic {
                ChatTopic::Label(label) => (label, default_topic_confidence()),
                ChatTopic::Scored { label, confidence } => (label, confidence),
            };
            let label = label
                .split_whitespace()
                .collect::<Vec<_>>()
                .join(" ")
                .to_lowercase();
            (label.chars().count() >= 2
                && label.chars().count() <= 40
                && confidence.is_finite()
                && (0.0..=1.0).contains(&confidence)
                && seen.insert(label.clone()))
            .then_some(TopicTag { label, confidence })
        })
        .take(8)
        .collect::<Vec<_>>();
    if tags.is_empty() { fallback } else { tags }
}

const fn default_topic_confidence() -> f32 {
    0.8
}

#[derive(Clone)]
pub struct HttpRecommendationProvider {
    client: BoundedJsonClient,
}

impl HttpRecommendationProvider {
    pub fn new(config: HttpProviderConfig) -> Result<Self, ModelError> {
        Ok(Self {
            client: BoundedJsonClient::new(config)?,
        })
    }
}

#[async_trait]
impl RecommendationProvider for HttpRecommendationProvider {
    fn identity(&self) -> ModelIdentity {
        self.client.config.identity.clone()
    }

    async fn rank(&self, request: RankRequest) -> Result<RankResponse, ModelError> {
        if request.candidates.len() > self.client.config.maximum_batch_items {
            return Err(ModelError::InvalidOutput(
                "ranking batch is too large".into(),
            ));
        }
        let allowed = request
            .candidates
            .iter()
            .map(|candidate| candidate.story_id.clone())
            .collect::<HashSet<_>>();
        let value = serde_json::to_value(&request)
            .map_err(|error| ModelError::InvalidOutput(error.to_string()))?;
        let response: RankResponse =
            serde_json::from_value(self.client.post("rank", &value).await?)
                .map_err(|error| ModelError::InvalidOutput(error.to_string()))?;
        let mut seen = HashSet::new();
        if response.ranked.iter().any(|item| {
            !allowed.contains(&item.story_id)
                || !seen.insert(&item.story_id)
                || !item.score.is_finite()
        }) {
            return Err(ModelError::InvalidOutput(
                "ranking response contains invalid candidates".into(),
            ));
        }
        Ok(response)
    }

    async fn submit_feedback(
        &self,
        events: &[RecommendationFeedbackEvent],
    ) -> Result<(), ModelError> {
        if events.len() > self.client.config.maximum_batch_items {
            return Err(ModelError::InvalidOutput(
                "feedback batch is too large".into(),
            ));
        }
        let body = json!({"events": events.iter().map(|event| json!({
            "eventId": event.event_id, "userKey": event.user_key,
            "storyId": event.story_id, "feedback": event.feedback,
        })).collect::<Vec<_>>()});
        self.client.post("feedback", &body).await?;
        Ok(())
    }

    async fn health(&self) -> Result<ModelHealth, ModelError> {
        self.client.get("health").await.map(|_| ModelHealth {
            ready: true,
            detail: "HTTP recommender ready".into(),
        })
    }
}

#[derive(Clone)]
pub struct OpenAiCompatibleRecommendationProvider {
    client: BoundedJsonClient,
}

impl OpenAiCompatibleRecommendationProvider {
    pub fn new(config: HttpProviderConfig) -> Result<Self, ModelError> {
        Ok(Self {
            client: BoundedJsonClient::new(config)?,
        })
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChatRankResponse {
    #[serde(default)]
    request_id: Option<String>,
    ranked: Vec<ChatRankedCandidate>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ChatRankedCandidate {
    story_id: String,
    score: f32,
}

#[async_trait]
impl RecommendationProvider for OpenAiCompatibleRecommendationProvider {
    fn identity(&self) -> ModelIdentity {
        self.client.config.identity.clone()
    }

    async fn rank(&self, request: RankRequest) -> Result<RankResponse, ModelError> {
        if request.candidates.is_empty()
            || request.candidates.len() > self.client.config.maximum_batch_items
        {
            return Err(ModelError::InvalidOutput(
                "ranking batch size is invalid".into(),
            ));
        }
        let candidates = serde_json::to_string(&request.candidates)
            .map_err(|error| ModelError::InvalidOutput(error.to_string()))?;
        let response = self
            .client
            .post(
                "chat/completions",
                &json!({
                    "model": self.client.config.identity.model,
                    "response_format": {"type": "json_object"},
                    "temperature": 0,
                    "messages": [
                        {"role": "system", "content": "Return only JSON shaped as {\"requestId\":\"...\",\"ranked\":[{\"storyId\":\"allowed id\",\"score\":0.0}]}. Rank only supplied candidates. Scores must be finite numbers from 0 to 1. Prefer specific, consequential, information-dense stories and follow the stream instruction."},
                        {"role": "user", "content": format!(
                            "Stream: {}\nUI: {}\nResult count: {}\nRanking instruction: {}\nCandidates JSON:\n{}",
                            request.stream_slug,
                            request.ui_mode,
                            request.result_count,
                            request.ranking_instruction.as_deref().unwrap_or("Use general relevance."),
                            bounded_text(&candidates, 120_000)
                        )}
                    ]
                }),
            )
            .await?;
        let content = chat_content(&response)?;
        let parsed: ChatRankResponse = parse_json_object(&content)?;
        let allowed = request
            .candidates
            .iter()
            .map(|candidate| candidate.story_id.as_str())
            .collect::<HashSet<_>>();
        let mut seen = HashSet::new();
        let ranked = parsed
            .ranked
            .into_iter()
            .filter_map(|candidate| {
                (allowed.contains(candidate.story_id.as_str())
                    && seen.insert(candidate.story_id.clone())
                    && candidate.score.is_finite()
                    && (0.0..=1.0).contains(&candidate.score))
                .then_some(RankedCandidate {
                    story_id: candidate.story_id,
                    score: candidate.score,
                    features: std::collections::BTreeMap::from([("llmRank".into(), 1.0)]),
                })
            })
            .take(request.result_count.max(1))
            .collect::<Vec<_>>();
        if ranked.is_empty() {
            return Err(ModelError::InvalidOutput(
                "chat ranker returned no valid candidates".into(),
            ));
        }
        Ok(RankResponse {
            request_id: parsed.request_id.unwrap_or_else(|| "chat-rank".into()),
            ranked,
        })
    }

    async fn submit_feedback(
        &self,
        _events: &[RecommendationFeedbackEvent],
    ) -> Result<(), ModelError> {
        Ok(())
    }

    async fn health(&self) -> Result<ModelHealth, ModelError> {
        self.client.health().await
    }
}

fn chat_content(value: &Value) -> Result<String, ModelError> {
    value
        .pointer("/choices/0/message/content")
        .and_then(Value::as_str)
        .map(str::trim)
        .map(str::to_owned)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| ModelError::InvalidOutput("chat response has no content".into()))
}

fn bounded_text(value: &str, maximum_chars: usize) -> String {
    value.chars().take(maximum_chars).collect()
}

fn parse_json_object<T: DeserializeOwned>(value: &str) -> Result<T, ModelError> {
    let end = value
        .rfind('}')
        .ok_or_else(|| ModelError::InvalidOutput("model response has no JSON object".into()))?;
    for (start, _) in value.match_indices('{') {
        if start > end {
            break;
        }
        if let Ok(parsed) = serde_json::from_str(&value[start..=end]) {
            return Ok(parsed);
        }
    }
    Err(ModelError::InvalidOutput(
        "model response contains invalid JSON object".into(),
    ))
}

fn validate_collection_response(
    request: &CollectionParseRequest,
    response: &mut CollectionParseResponse,
) -> Result<(), ModelError> {
    if !response.confidence.is_finite()
        || !(0.0..=1.0).contains(&response.confidence)
        || response.entries.len() > 200
    {
        return Err(ModelError::InvalidOutput(
            "collection confidence or size is invalid".into(),
        ));
    }
    let allowed = request.allowed_urls.iter().collect::<HashSet<_>>();
    let mut seen = HashSet::new();
    response.entries.retain(|entry| {
        allowed.contains(&entry.url)
            && seen.insert(entry.url.clone())
            && entry.confidence.is_finite()
            && (0.0..=1.0).contains(&entry.confidence)
    });
    Ok(())
}

#[cfg(test)]
include!("http_tests.rs");
