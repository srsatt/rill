use std::sync::Arc;

use ammonia::clean;
use rill_collection_expansion::{
    CollectionPolicy, derived_identity, detect_collection_with_diagnostics, provider_request,
    validate_provider_response,
};
pub use rill_collection_expansion::{DetectionMode, ParentDisplayPolicy};
use rill_db::{DbError, DbPool};
use rill_dedup::{CuratorProvenance, DedupError, DedupService, canonicalize_url, content_checksum};
use rill_domain::{ExternalLink, ItemShape, LinkRelation, NormalizedDocument, RawSourceItem};
use rill_extraction::ExtractedArticle;
use rill_jobs::{EnqueueOptions, JobKind, JobQueue, QueueError};
use rill_model_api::{CollectionParserProvider, ModelError};
use rill_source_api::{ConnectorContext, ConnectorError, SourceBatch, SourceConnector};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use thiserror::Error;
use url::Url;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum IngestionError {
    #[error("database error: {0}")]
    Database(#[from] DbError),
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("connector error: {0}")]
    Connector(#[from] ConnectorError),
    #[error("job queue error: {0}")]
    Queue(#[from] QueueError),
    #[error("deduplication error: {0}")]
    Dedup(#[from] DedupError),
    #[error("invalid source data: {0}")]
    Invalid(String),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("model provider error: {0}")]
    Model(#[from] ModelError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SourceRegistration {
    pub id: String,
    pub visibility_scope: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IngestReport {
    pub raw_items: usize,
    pub documents_created: usize,
    pub collection_parents: usize,
    pub collection_children: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SearchHit {
    pub story_id: String,
    pub document_id: String,
    pub title: String,
    pub excerpt: String,
    pub canonical_url: Option<String>,
    pub publisher: Option<String>,
    pub published_at: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceView {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub visibility: String,
    pub audience: String,
    pub enabled: bool,
    pub last_success_at: Option<i64>,
    pub consecutive_failures: u32,
    pub last_error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RssFeedView {
    pub source_id: String,
    pub name: String,
    pub xml_url: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionEntryView {
    pub ordinal: u32,
    pub target_url: String,
    pub title_hint: Option<String>,
    pub commentary: Option<String>,
    pub extraction_method: String,
    pub confidence: f32,
    pub derived_raw_item_id: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionExpansionView {
    pub parser_kind: String,
    pub parser_version: String,
    pub confidence: f32,
    pub status: String,
    pub parent_display_policy: String,
    pub diagnostics: Value,
    pub entries: Vec<CollectionEntryView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct CollectionDebugView {
    pub raw_item_id: String,
    pub source_instance_id: String,
    pub source_name: String,
    pub parent_title: Option<String>,
    pub parent_url: Option<String>,
    pub override_mode: Option<String>,
    pub expansions: Vec<CollectionExpansionView>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PollSourcePayload {
    pub source_id: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExtractArticlePayload {
    pub raw_item_id: String,
    pub url: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ProcessDerivedPayload {
    pub source_instance_id: String,
    pub parent_raw_item_id: String,
    pub target_url: String,
    pub derived_identity: String,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RawItemJobPayload {
    pub raw_item_id: String,
}

#[derive(Clone)]
pub struct IngestionService {
    pool: DbPool,
    jobs: JobQueue,
    dedup: DedupService,
    collection_policy: CollectionPolicy,
    collection_parser: Option<Arc<dyn CollectionParserProvider>>,
}

include!("pipeline.rs");
include!("processing.rs");
include!("source_admin.rs");
include!("source_state.rs");
include!("telegram_subscriptions.rs");
include!("persistence.rs");

include!("helpers.rs");
#[cfg(test)]
include!("tests.rs");
