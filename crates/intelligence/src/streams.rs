use std::collections::HashMap;

use rill_jobs::{EnqueueOptions, JobKind};
use rill_model_api::{EmbeddingInput, ModelIdentity};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tracing::warn;
use uuid::Uuid;

use crate::preference::PreferenceModel;
use crate::{
    IntelligenceError, IntelligenceService, StreamEmbeddingPayload, cosine, decode_vector,
    encode_vector, unix_now,
};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields, default)]
pub struct StreamFilter {
    pub include_sources: Vec<String>,
    pub exclude_sources: Vec<String>,
    pub include_curators: Vec<String>,
    pub exclude_curators: Vec<String>,
    pub include_publishers: Vec<String>,
    pub exclude_publishers: Vec<String>,
    pub include_topics: Vec<String>,
    pub exclude_topics: Vec<String>,
    pub languages: Vec<String>,
    pub text_query: Option<String>,
    pub maximum_age_hours: Option<u32>,
    pub minimum_coverage: Option<u32>,
    pub read: Option<bool>,
    pub favorite: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StreamView {
    pub id: String,
    pub name: String,
    pub slug: String,
    pub icon: Option<String>,
    pub position: i32,
    pub semantic_description: Option<String>,
    pub ranking_instruction: Option<String>,
    pub filter: StreamFilter,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct UserPreferences {
    pub ai_free_mode: bool,
    pub stream_membership_mode: String,
    pub font_family: String,
    #[serde(default)]
    pub processing_prompt: String,
}

#[derive(Debug, Clone)]
pub struct CreateStreamInput {
    pub name: String,
    pub slug: String,
    pub icon: Option<String>,
    pub filter: StreamFilter,
    pub semantic_description: Option<String>,
    pub ranking_instruction: Option<String>,
}

#[derive(Debug, Clone)]
pub struct UpdateStreamInput {
    pub name: String,
    pub icon: Option<String>,
    pub filter: StreamFilter,
    pub semantic_description: Option<String>,
    pub ranking_instruction: Option<String>,
}

type PreferenceCentroids = (Option<Vec<f32>>, Option<Vec<f32>>);

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RankedStory {
    pub story_id: String,
    pub document_id: String,
    pub title: String,
    pub summary: String,
    pub canonical_url: Option<String>,
    pub publisher: Option<String>,
    pub source_ids: Vec<String>,
    pub published_at: Option<i64>,
    pub read: bool,
    pub coverage: u32,
    pub topics: Vec<String>,
    pub score: f32,
    pub explanation: Value,
}

#[derive(Debug, Clone)]
struct Candidate {
    story_id: String,
    document_id: String,
    title: String,
    summary: String,
    raw_excerpt: String,
    canonical_url: Option<String>,
    publisher: Option<String>,
    language: Option<String>,
    published_at: Option<i64>,
    coverage: u32,
    topics: Vec<String>,
    sources: Vec<String>,
    curators: Vec<String>,
    read: bool,
    favorite: bool,
    vector: Option<Vec<f32>>,
    score: f32,
    explanation: Value,
}

#[derive(Debug)]
struct CandidateVariant {
    candidate: Candidate,
    preferred: bool,
    body_chars: usize,
    direct: bool,
    readable: bool,
}

type AffinityScores = HashMap<String, HashMap<String, f32>>;

include!("stream_queries.rs");
include!("stream_management.rs");
include!("stream_models.rs");

include!("stream_scoring.rs");
