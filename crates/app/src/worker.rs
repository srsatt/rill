use std::{
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

use async_trait::async_trait;
use rill_actions::{ActionService, ExecuteActionPayload};
use rill_extraction::ArticleExtractor;
use rill_ingestion::{
    ExtractArticlePayload, IngestionService, PollSourcePayload, ProcessDerivedPayload,
    RawItemJobPayload,
};
use rill_intelligence::{
    DocumentJobPayload, IntelligenceService, PreferenceRefitPayload, StreamEmbeddingPayload,
};
use rill_jobs::{EnqueueOptions, Job, JobFailure, JobHandler, JobKind, JobQueue, Worker};
use rill_plugin_host::{PluginService, PluginSourceConfig};
use rill_source_api::ConnectorContext;
use rill_source_email::{EmailCursor, EmailGateway, config_from_value};
use rill_source_rss::RssConnector;
use rill_source_telegram::TelegramConnector;
use tracing::error;
use url::Url;

use crate::maintenance::{
    MaintenanceService, RecommendationMaintenancePayload, ReembedContentPayload,
};
use crate::metrics::Metrics;

pub struct IngestionJobHandler {
    ingestion: IngestionService,
    extractor: ArticleExtractor,
    connector_context: ConnectorContext,
    rss: RssConnector,
    poll_item_limit: usize,
    intelligence: IntelligenceService,
    email: Option<Arc<dyn EmailGateway>>,
    telegram: TelegramConnector,
    actions: ActionService,
    plugins: PluginService,
    metrics: Metrics,
    maintenance: MaintenanceService,
    jobs: JobQueue,
}

pub struct IngestionJobHandlerDependencies {
    pub ingestion: IngestionService,
    pub extractor: ArticleExtractor,
    pub connector_context: ConnectorContext,
    pub poll_item_limit: usize,
    pub intelligence: IntelligenceService,
    pub email: Option<Arc<dyn EmailGateway>>,
    pub actions: ActionService,
    pub plugins: PluginService,
    pub metrics: Metrics,
    pub maintenance: MaintenanceService,
    pub jobs: JobQueue,
}

impl IngestionJobHandler {
    pub fn new(dependencies: IngestionJobHandlerDependencies) -> Self {
        Self {
            ingestion: dependencies.ingestion,
            extractor: dependencies.extractor,
            connector_context: dependencies.connector_context,
            rss: RssConnector,
            poll_item_limit: dependencies.poll_item_limit,
            intelligence: dependencies.intelligence,
            email: dependencies.email,
            telegram: TelegramConnector,
            actions: dependencies.actions,
            plugins: dependencies.plugins,
            metrics: dependencies.metrics,
            maintenance: dependencies.maintenance,
            jobs: dependencies.jobs,
        }
    }
}

impl IngestionJobHandler {
    async fn handle_job(&self, job: &Job) -> Result<(), JobFailure> {
        match job.kind {
            JobKind::PollSource | JobKind::ParseEmail => {
                let payload: PollSourcePayload = decode(job)?;
                if !self
                    .ingestion
                    .should_poll_source(&payload.source_id)
                    .map_err(failure)?
                {
                    return Ok(());
                }
                let kind = self
                    .ingestion
                    .source_kind(&payload.source_id)
                    .map_err(failure)?;
                match kind.as_str() {
                    "rss" => {
                        self.ingestion
                            .poll_source(
                                &self.rss,
                                &self.connector_context,
                                &payload.source_id,
                                self.poll_item_limit,
                            )
                            .await
                            .map_err(failure)?;
                    }
                    "email" => self.poll_email(&payload.source_id).await?,
                    "telegram" => {
                        self.ingestion
                            .poll_source(
                                &self.telegram,
                                &self.connector_context,
                                &payload.source_id,
                                self.poll_item_limit,
                            )
                            .await
                            .map_err(failure)?;
                    }
                    "plugin" => self.poll_plugin(&payload.source_id).await?,
                    _ => {
                        return Err(JobFailure {
                            class: "unsupported_source".into(),
                            message: format!("no built-in polling worker for source kind {kind}"),
                        });
                    }
                }
                if self
                    .ingestion
                    .should_poll_source(&payload.source_id)
                    .map_err(failure)?
                {
                    let interval = self
                        .ingestion
                        .source_poll_interval(&payload.source_id)
                        .map_err(failure)?;
                    self.ingestion
                        .schedule_poll(
                            &payload.source_id,
                            Some(
                                unix_now()
                                    .saturating_add(i64::try_from(interval).unwrap_or(i64::MAX)),
                            ),
                        )
                        .map_err(failure)?;
                }
                Ok(())
            }
            JobKind::ExtractArticle => {
                let payload: ExtractArticlePayload = decode(job)?;
                let url = Url::parse(&payload.url).map_err(|error| JobFailure {
                    class: "invalid_url".into(),
                    message: error.to_string(),
                })?;
                let visibility = self
                    .ingestion
                    .raw_visibility_scope(&payload.raw_item_id)
                    .map_err(failure)?;
                let article = self
                    .extractor
                    .extract_url(&url, &visibility)
                    .await
                    .map_err(failure)?;
                self.ingestion
                    .process_extracted_article(&payload.raw_item_id, &article)
                    .map_err(failure)
            }
            JobKind::ProcessDerivedItem => {
                let payload: ProcessDerivedPayload = decode(job)?;
                let url = Url::parse(&payload.target_url).map_err(|error| JobFailure {
                    class: "invalid_url".into(),
                    message: error.to_string(),
                })?;
                let visibility = self
                    .ingestion
                    .raw_visibility_scope(&payload.parent_raw_item_id)
                    .map_err(failure)?;
                let article = self
                    .extractor
                    .extract_url(&url, &visibility)
                    .await
                    .map_err(failure)?;
                self.ingestion
                    .process_derived_article(&payload, &article)
                    .map_err(failure)
            }
            JobKind::NormalizeRawItem | JobKind::ResolveCanonicalUrl => {
                let payload: RawItemJobPayload = decode(job)?;
                self.ingestion
                    .normalize_raw_item(&payload.raw_item_id)
                    .map(|_| ())
                    .map_err(failure)
            }
            JobKind::DetectCollection
            | JobKind::ExpandCollection
            | JobKind::ParseCollectionWithProvider => {
                let payload: RawItemJobPayload = decode(job)?;
                self.ingestion
                    .detect_raw_collection_with_provider(&payload.raw_item_id)
                    .await
                    .map(|_| ())
                    .map_err(failure)
            }
            JobKind::GenerateSummary => {
                let payload: DocumentJobPayload = decode(job)?;
                self.intelligence
                    .process_summary(&payload.document_id)
                    .await
                    .map_err(failure)
            }
            JobKind::GenerateEmbedding | JobKind::ClusterStory => {
                let payload: DocumentJobPayload = decode(job)?;
                self.intelligence
                    .process_embedding(&payload.document_id)
                    .await
                    .map_err(failure)
            }
            JobKind::EmbedStream => {
                let payload: StreamEmbeddingPayload = decode(job)?;
                self.intelligence
                    .process_stream_embedding(&payload.stream_id, &payload.description)
                    .await
                    .map_err(failure)
            }
            JobKind::SubmitRecommendationFeedback => Ok(()),
            JobKind::InvalidateRecommendations | JobKind::RecomputeAffinity => {
                let payload: RecommendationMaintenancePayload = decode(job)?;
                self.intelligence
                    .invalidate_recommendations(payload.user_id.as_deref())
                    .map(|_| ())
                    .map_err(failure)
            }
            JobKind::EvaluateStreamCandidates => Ok(()),
            JobKind::RefitPreferenceModel => {
                let payload: PreferenceRefitPayload = decode(job)?;
                self.intelligence
                    .refit_preference_model(&payload.user_id)
                    .map(|_| ())
                    .map_err(failure)
            }
            JobKind::ExecuteAction | JobKind::RetryAction => {
                let payload: ExecuteActionPayload = decode(job)?;
                self.actions
                    .execute(&payload, job.attempt_count, job.max_attempts)
                    .await
                    .map_err(failure)
            }
            JobKind::ReembedContent => {
                let payload: ReembedContentPayload = decode(job)?;
                self.intelligence
                    .enqueue_reembedding(payload.document_id.as_deref())
                    .map(|_| ())
                    .map_err(failure)
            }
            JobKind::CleanupSessions => {
                self.maintenance.cleanup_sessions().map_err(failure)?;
                self.schedule_maintenance(JobKind::CleanupSessions)
            }
            JobKind::CleanupPairingCodes => {
                self.maintenance.cleanup_pairing_codes().map_err(failure)?;
                self.schedule_maintenance(JobKind::CleanupPairingCodes)
            }
            JobKind::DatabaseMaintenance => {
                self.maintenance.database_maintenance().map_err(failure)?;
                self.schedule_maintenance(JobKind::DatabaseMaintenance)
            }
        }
    }

    fn schedule_maintenance(&self, kind: JobKind) -> Result<(), JobFailure> {
        schedule_maintenance_job(&self.jobs, kind, unix_now().saturating_add(86_400))
            .map(|_| ())
            .map_err(failure)
    }
}

#[async_trait]
impl JobHandler for IngestionJobHandler {
    #[tracing::instrument(
        skip_all,
        fields(job_id = %job.id, job_kind = job.kind.as_str(), attempt = job.attempt_count)
    )]
    async fn handle(&self, job: Job) -> Result<(), JobFailure> {
        let started = Instant::now();
        let result = self.handle_job(&job).await;
        self.metrics.observe(
            job_operation(job.kind),
            started.elapsed(),
            result.is_ok(),
            0,
        );
        result
    }
}

const fn job_operation(kind: JobKind) -> &'static str {
    match kind {
        JobKind::PollSource => "source_poll",
        JobKind::DetectCollection
        | JobKind::ExpandCollection
        | JobKind::ParseCollectionWithProvider => "collection",
        JobKind::ExtractArticle | JobKind::ProcessDerivedItem => "extraction",
        JobKind::GenerateEmbedding | JobKind::EmbedStream => "embedding",
        JobKind::GenerateSummary => "summary",
        JobKind::EvaluateStreamCandidates
        | JobKind::RefitPreferenceModel
        | JobKind::SubmitRecommendationFeedback => "recommendation",
        JobKind::ExecuteAction | JobKind::RetryAction => "action",
        JobKind::CleanupSessions | JobKind::CleanupPairingCodes | JobKind::DatabaseMaintenance => {
            "maintenance"
        }
        _ => "job",
    }
}

impl IngestionJobHandler {
    async fn poll_email(&self, source_id: &str) -> Result<(), JobFailure> {
        let gateway = self.email.as_ref().ok_or_else(|| JobFailure {
            class: "email_unconfigured".into(),
            message: "email gateway requires a configured master key".into(),
        })?;
        let (config, cursor) = self
            .ingestion
            .source_poll_state(source_id)
            .map_err(failure)?;
        let config = config_from_value(&config).map_err(failure)?;
        let cursor = cursor
            .map(serde_json::from_value::<EmailCursor>)
            .transpose()
            .map_err(failure)?
            .unwrap_or_default();
        match gateway.poll(&config, &cursor, self.poll_item_limit).await {
            Ok(batch) => {
                self.ingestion
                    .ingest_batch(source_id, &batch)
                    .map_err(failure)?;
                self.ingestion
                    .record_external_poll_success(source_id, batch.cursor.as_ref())
                    .map_err(failure)
            }
            Err(error) => {
                self.ingestion
                    .record_external_poll_failure(source_id, &error.to_string())
                    .map_err(failure)?;
                Err(failure(error))
            }
        }
    }

    async fn poll_plugin(&self, source_id: &str) -> Result<(), JobFailure> {
        let (config, cursor) = self
            .ingestion
            .source_poll_state(source_id)
            .map_err(failure)?;
        let config: PluginSourceConfig = serde_json::from_value(config).map_err(failure)?;
        match self
            .plugins
            .poll(source_id, &config, cursor.as_ref(), self.poll_item_limit)
            .await
        {
            Ok(batch) => {
                self.ingestion
                    .ingest_batch(source_id, &batch)
                    .map_err(failure)?;
                self.ingestion
                    .record_external_poll_success(source_id, batch.cursor.as_ref())
                    .map_err(failure)
            }
            Err(error) => {
                self.ingestion
                    .record_external_poll_failure(source_id, &error.to_string())
                    .map_err(failure)?;
                Err(failure(error))
            }
        }
    }
}

pub async fn run_worker(worker: Worker<IngestionJobHandler>, idle_poll: Duration) {
    loop {
        match worker.run_once().await {
            Ok(0) => tokio::time::sleep(idle_poll).await,
            Ok(_) => {}
            Err(failure) => {
                error!(error = %failure, "job worker batch failed");
                tokio::time::sleep(idle_poll).await;
            }
        }
    }
}

pub fn build_worker(
    queue: rill_jobs::JobQueue,
    handler: IngestionJobHandler,
    concurrency: usize,
) -> Worker<IngestionJobHandler> {
    Worker::new(queue, Arc::new(handler), concurrency)
}

pub fn schedule_initial_maintenance(queue: &JobQueue) -> Result<(), rill_jobs::QueueError> {
    let now = unix_now();
    for available_at in [now, now.saturating_add(86_400)] {
        for kind in [
            JobKind::CleanupSessions,
            JobKind::CleanupPairingCodes,
            JobKind::DatabaseMaintenance,
        ] {
            schedule_maintenance_job(queue, kind, available_at)?;
        }
    }
    Ok(())
}

fn schedule_maintenance_job(
    queue: &JobQueue,
    kind: JobKind,
    available_at: i64,
) -> Result<String, rill_jobs::QueueError> {
    queue.enqueue(
        kind,
        &serde_json::json!({}),
        EnqueueOptions {
            available_at: Some(available_at),
            idempotency_key: Some(format!(
                "{}:{}",
                kind.as_str(),
                available_at.div_euclid(86_400)
            )),
            ..Default::default()
        },
    )
}

fn decode<T: serde::de::DeserializeOwned>(job: &Job) -> Result<T, JobFailure> {
    serde_json::from_value(job.payload.clone()).map_err(|error| JobFailure {
        class: "invalid_payload".into(),
        message: error.to_string(),
    })
}

fn failure(error: impl std::fmt::Display) -> JobFailure {
    JobFailure {
        class: "processing".into(),
        message: error.to_string(),
    }
}

fn unix_now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|duration| i64::try_from(duration.as_secs()).ok())
        .unwrap_or(0)
}
