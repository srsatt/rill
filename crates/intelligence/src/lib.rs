mod clustering;
mod stories;
mod streams;

use std::{sync::Arc, time::SystemTime};

use rill_db::{DbError, DbPool};
use rill_jobs::{EnqueueOptions, JobKind, JobQueue, QueueError};
use rill_model_api::{
    EmbeddingInput, EmbeddingProvider, ModelError, RecommendationFeedbackEvent,
    RecommendationProvider, SummaryProvider, SummaryRequest,
};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use thiserror::Error;
use uuid::Uuid;

pub use stories::{CuratorPathView, StoryDetailView, StoryVariantView};
pub use streams::{CreateStreamInput, RankedStory, StreamFilter, StreamView, UpdateStreamInput};

#[derive(Debug, Error)]
pub enum IntelligenceError {
    #[error("database error: {0}")]
    Database(#[from] DbError),
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("model error: {0}")]
    Model(#[from] ModelError),
    #[error("job queue error: {0}")]
    Queue(#[from] QueueError),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid intelligence request: {0}")]
    Invalid(String),
    #[error("resource not found")]
    NotFound,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FeedbackKind {
    Like,
    Dislike,
    Favorite,
    None,
}

impl FeedbackKind {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Like => "like",
            Self::Dislike => "dislike",
            Self::Favorite => "favorite",
            Self::None => "none",
        }
    }
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocumentJobPayload {
    pub document_id: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FeedbackDeliveryPayload {
    pub event_id: String,
    pub user_id: String,
    pub story_id: String,
    pub feedback: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct EvaluateStreamPayload {
    pub user_id: String,
    pub slug: String,
    pub limit: usize,
    pub ui_mode: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct StreamEmbeddingPayload {
    pub stream_id: String,
    pub description: String,
}

#[derive(Clone)]
pub struct IntelligenceService {
    pub(crate) pool: DbPool,
    jobs: JobQueue,
    pub(crate) embedding: Arc<dyn EmbeddingProvider>,
    summary: Arc<dyn SummaryProvider>,
    pub(crate) recommendation: Option<Arc<dyn RecommendationProvider>>,
    pub(crate) cluster_window_seconds: i64,
    pub(crate) cluster_threshold: f32,
}

include!("service.rs");

include!("helpers.rs");
include!("tests.rs");
