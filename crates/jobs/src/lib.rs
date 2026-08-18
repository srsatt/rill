use async_trait::async_trait;
use rill_db::{DbError, DbPool};
use rusqlite::{OptionalExtension, TransactionBehavior, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use thiserror::Error;
use tokio::task::JoinSet;
use uuid::Uuid;

mod admin;

pub use admin::{JobAttemptView, JobView};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "PascalCase")]
pub enum JobKind {
    PollSource,
    ParseEmail,
    NormalizeRawItem,
    DetectCollection,
    ExpandCollection,
    ParseCollectionWithProvider,
    ProcessDerivedItem,
    ExtractArticle,
    ResolveCanonicalUrl,
    GenerateEmbedding,
    EmbedStream,
    GenerateSummary,
    ClusterStory,
    InvalidateRecommendations,
    EvaluateStreamCandidates,
    RefitPreferenceModel,
    SubmitRecommendationFeedback,
    ExecuteAction,
    RetryAction,
    RecomputeAffinity,
    ReembedContent,
    CleanupSessions,
    CleanupPairingCodes,
    DatabaseMaintenance,
}

impl JobKind {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::PollSource => "PollSource",
            Self::ParseEmail => "ParseEmail",
            Self::NormalizeRawItem => "NormalizeRawItem",
            Self::DetectCollection => "DetectCollection",
            Self::ExpandCollection => "ExpandCollection",
            Self::ParseCollectionWithProvider => "ParseCollectionWithProvider",
            Self::ProcessDerivedItem => "ProcessDerivedItem",
            Self::ExtractArticle => "ExtractArticle",
            Self::ResolveCanonicalUrl => "ResolveCanonicalUrl",
            Self::GenerateEmbedding => "GenerateEmbedding",
            Self::EmbedStream => "EmbedStream",
            Self::GenerateSummary => "GenerateSummary",
            Self::ClusterStory => "ClusterStory",
            Self::InvalidateRecommendations => "InvalidateRecommendations",
            Self::EvaluateStreamCandidates => "EvaluateStreamCandidates",
            Self::RefitPreferenceModel => "RefitPreferenceModel",
            Self::SubmitRecommendationFeedback => "SubmitRecommendationFeedback",
            Self::ExecuteAction => "ExecuteAction",
            Self::RetryAction => "RetryAction",
            Self::RecomputeAffinity => "RecomputeAffinity",
            Self::ReembedContent => "ReembedContent",
            Self::CleanupSessions => "CleanupSessions",
            Self::CleanupPairingCodes => "CleanupPairingCodes",
            Self::DatabaseMaintenance => "DatabaseMaintenance",
        }
    }

    fn from_db(value: &str) -> Option<Self> {
        Some(match value {
            "PollSource" => Self::PollSource,
            "ParseEmail" => Self::ParseEmail,
            "NormalizeRawItem" => Self::NormalizeRawItem,
            "DetectCollection" => Self::DetectCollection,
            "ExpandCollection" => Self::ExpandCollection,
            "ParseCollectionWithProvider" => Self::ParseCollectionWithProvider,
            "ProcessDerivedItem" => Self::ProcessDerivedItem,
            "ExtractArticle" => Self::ExtractArticle,
            "ResolveCanonicalUrl" => Self::ResolveCanonicalUrl,
            "GenerateEmbedding" => Self::GenerateEmbedding,
            "EmbedStream" => Self::EmbedStream,
            "GenerateSummary" => Self::GenerateSummary,
            "ClusterStory" => Self::ClusterStory,
            "InvalidateRecommendations" => Self::InvalidateRecommendations,
            "EvaluateStreamCandidates" => Self::EvaluateStreamCandidates,
            "RefitPreferenceModel" => Self::RefitPreferenceModel,
            "SubmitRecommendationFeedback" => Self::SubmitRecommendationFeedback,
            "ExecuteAction" => Self::ExecuteAction,
            "RetryAction" => Self::RetryAction,
            "RecomputeAffinity" => Self::RecomputeAffinity,
            "ReembedContent" => Self::ReembedContent,
            "CleanupSessions" => Self::CleanupSessions,
            "CleanupPairingCodes" => Self::CleanupPairingCodes,
            "DatabaseMaintenance" => Self::DatabaseMaintenance,
            _ => return None,
        })
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Job {
    pub id: String,
    pub kind: JobKind,
    pub payload: Value,
    pub visibility_scope: Option<String>,
    pub priority: i32,
    pub attempt_count: u32,
    pub max_attempts: u32,
    pub lease_owner: String,
    pub lease_expires_at: i64,
}

#[derive(Debug, Clone)]
pub struct EnqueueOptions {
    pub visibility_scope: Option<String>,
    pub priority: i32,
    pub available_at: Option<i64>,
    pub max_attempts: u32,
    pub idempotency_key: Option<String>,
}

impl Default for EnqueueOptions {
    fn default() -> Self {
        Self {
            visibility_scope: None,
            priority: 0,
            available_at: None,
            max_attempts: 5,
            idempotency_key: None,
        }
    }
}

#[derive(Debug, Error)]
pub enum QueueError {
    #[error("database error: {0}")]
    Database(#[from] DbError),
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("job payload is invalid: {0}")]
    Json(#[from] serde_json::Error),
    #[error("unknown persisted job kind: {0}")]
    UnknownKind(String),
    #[error("job is not leased by this worker")]
    LeaseLost,
    #[error("max_attempts must be greater than zero")]
    InvalidRetryPolicy,
    #[error("system clock is before the Unix epoch")]
    Clock,
    #[error("job worker task failed")]
    WorkerTask,
}

#[derive(Clone)]
pub struct JobQueue {
    pool: DbPool,
}

impl JobQueue {
    pub fn new(pool: DbPool) -> Self {
        Self { pool }
    }

    pub fn enqueue(
        &self,
        kind: JobKind,
        payload: &Value,
        options: EnqueueOptions,
    ) -> Result<String, QueueError> {
        if options.max_attempts == 0 {
            return Err(QueueError::InvalidRetryPolicy);
        }
        let now = unix_now()?;
        let available_at = options.available_at.unwrap_or(now);
        let id = Uuid::new_v4().to_string();
        let payload = serde_json::to_string(payload)?;
        let connection = self.pool.connection()?;
        let changed = connection.execute(
            "INSERT INTO jobs(id, kind, payload_json, visibility_scope, priority, status,\n\
             available_at, max_attempts, idempotency_key, created_at, updated_at)\n\
             VALUES (?1, ?2, ?3, ?4, ?5, 'queued', ?6, ?7, ?8, ?9, ?9)\n\
             ON CONFLICT(idempotency_key) DO NOTHING",
            params![
                id,
                kind.as_str(),
                payload,
                options.visibility_scope,
                options.priority,
                available_at,
                options.max_attempts,
                options.idempotency_key,
                now
            ],
        )?;
        if changed == 1 {
            return Ok(id);
        }
        let key = options.idempotency_key.ok_or(QueueError::LeaseLost)?;
        Ok(connection.query_row(
            "SELECT id FROM jobs WHERE idempotency_key = ?1",
            [key],
            |row| row.get(0),
        )?)
    }

    /// Enqueues one queued job for an exact kind/payload pair. If one already
    /// exists, an earlier request pulls it forward instead of creating a
    /// second recurring chain. A currently leased job does not block its one
    /// queued successor.
    pub fn enqueue_coalesced_queued(
        &self,
        kind: JobKind,
        payload: &Value,
        options: EnqueueOptions,
    ) -> Result<String, QueueError> {
        if options.max_attempts == 0 {
            return Err(QueueError::InvalidRetryPolicy);
        }
        let now = unix_now()?;
        let available_at = options.available_at.unwrap_or(now);
        let payload = serde_json::to_string(payload)?;
        let mut connection = self.pool.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let queued: Option<String> = transaction
            .query_row(
                "SELECT id FROM jobs
                 WHERE kind=?1 AND payload_json=?2 AND status='queued'
                 ORDER BY available_at, created_at LIMIT 1",
                params![kind.as_str(), payload],
                |row| row.get(0),
            )
            .optional()?;
        if let Some(id) = queued {
            transaction.execute(
                "UPDATE jobs SET available_at=min(available_at, ?2),
                   priority=max(priority, ?3), updated_at=?4 WHERE id=?1",
                params![id, available_at, options.priority, now],
            )?;
            transaction.commit()?;
            return Ok(id);
        }

        let id = Uuid::new_v4().to_string();
        transaction.execute(
            "INSERT INTO jobs(id, kind, payload_json, visibility_scope, priority, status,
             available_at, max_attempts, idempotency_key, created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, 'queued', ?6, ?7, ?8, ?9, ?9)",
            params![
                id,
                kind.as_str(),
                payload,
                options.visibility_scope,
                options.priority,
                available_at,
                options.max_attempts,
                options.idempotency_key,
                now
            ],
        )?;
        transaction.commit()?;
        Ok(id)
    }

    pub fn lease(
        &self,
        worker_id: &str,
        lease_for: Duration,
        at: i64,
    ) -> Result<Option<Job>, QueueError> {
        let lease_seconds = i64::try_from(lease_for.as_secs())
            .unwrap_or(i64::MAX)
            .max(1);
        let lease_expires = at.saturating_add(lease_seconds);
        let mut connection = self.pool.connection()?;
        let transaction = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let id: Option<String> = transaction
            .query_row(
                "SELECT id FROM jobs\n\
             WHERE (status = 'queued' AND available_at <= ?1)\n\
                OR (status = 'leased' AND lease_expires_at <= ?1)\n\
             ORDER BY priority DESC, available_at, created_at LIMIT 1",
                [at],
                |row| row.get(0),
            )
            .optional()?;
        let Some(id) = id else {
            transaction.commit()?;
            return Ok(None);
        };
        let changed = transaction.execute(
            "UPDATE jobs SET status = 'leased', lease_owner = ?2, lease_expires_at = ?3,\n\
             attempt_count = attempt_count + 1, updated_at = ?4 WHERE id = ?1",
            params![id, worker_id, lease_expires, at],
        )?;
        if changed != 1 {
            return Err(QueueError::LeaseLost);
        }
        let job = transaction.query_row(
            "SELECT kind, payload_json, visibility_scope, priority, attempt_count, max_attempts\n\
             FROM jobs WHERE id = ?1",
            [&id],
            |row| {
                let kind: String = row.get(0)?;
                Ok((
                    kind,
                    row.get::<_, String>(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get::<_, u32>(4)?,
                    row.get::<_, u32>(5)?,
                ))
            },
        )?;
        transaction.execute(
            "INSERT INTO job_attempts(id, job_id, attempt_number, worker_id, started_at)\n\
             VALUES (?1, ?2, ?3, ?4, ?5)",
            params![Uuid::new_v4().to_string(), id, job.4, worker_id, at],
        )?;
        transaction.commit()?;
        let kind = JobKind::from_db(&job.0).ok_or_else(|| QueueError::UnknownKind(job.0))?;
        Ok(Some(Job {
            id,
            kind,
            payload: serde_json::from_str(&job.1)?,
            visibility_scope: job.2,
            priority: job.3,
            attempt_count: job.4,
            max_attempts: job.5,
            lease_owner: worker_id.to_owned(),
            lease_expires_at: lease_expires,
        }))
    }

    pub fn complete(&self, job: &Job, at: i64) -> Result<(), QueueError> {
        let mut connection = self.pool.connection()?;
        let transaction = connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE jobs SET status = 'succeeded', lease_owner = NULL, lease_expires_at = NULL,\n\
             completed_at = ?3, updated_at = ?3\n\
             WHERE id = ?1 AND status = 'leased' AND lease_owner = ?2",
            params![job.id, job.lease_owner, at],
        )?;
        if changed != 1 {
            return Err(QueueError::LeaseLost);
        }
        finish_attempt(&transaction, job, at, "succeeded", None, None)?;
        transaction.commit()?;
        Ok(())
    }

    pub fn fail(
        &self,
        job: &Job,
        at: i64,
        error_class: &str,
        error_message: &str,
        base_delay: Duration,
    ) -> Result<bool, QueueError> {
        let dead = job.attempt_count >= job.max_attempts;
        let exponent = job.attempt_count.saturating_sub(1).min(20);
        let delay = base_delay
            .as_secs()
            .saturating_mul(1_u64 << exponent)
            .min(86_400);
        let available_at = at.saturating_add(i64::try_from(delay).unwrap_or(i64::MAX));
        let status = if dead { "dead" } else { "queued" };
        let mut connection = self.pool.connection()?;
        let transaction = connection.transaction()?;
        let changed = transaction.execute(
            "UPDATE jobs SET status = ?3, lease_owner = NULL, lease_expires_at = NULL,\n\
             available_at = ?4, last_error_class = ?5, last_error_message = ?6,\n\
             completed_at = CASE WHEN ?3 = 'dead' THEN ?7 ELSE NULL END, updated_at = ?7\n\
             WHERE id = ?1 AND status = 'leased' AND lease_owner = ?2",
            params![
                job.id,
                job.lease_owner,
                status,
                available_at,
                truncate(error_class, 80),
                truncate(error_message, 1_000),
                at
            ],
        )?;
        if changed != 1 {
            return Err(QueueError::LeaseLost);
        }
        finish_attempt(
            &transaction,
            job,
            at,
            if dead { "dead" } else { "retry" },
            Some(error_class),
            Some(error_message),
        )?;
        transaction.commit()?;
        Ok(dead)
    }
}

fn finish_attempt(
    transaction: &rusqlite::Transaction<'_>,
    job: &Job,
    at: i64,
    outcome: &str,
    error_class: Option<&str>,
    error_message: Option<&str>,
) -> rusqlite::Result<()> {
    transaction.execute(
        "UPDATE job_attempts SET finished_at = ?4, outcome = ?5, error_class = ?6,\n\
         error_message = ?7 WHERE job_id = ?1 AND attempt_number = ?2 AND worker_id = ?3",
        params![
            job.id,
            job.attempt_count,
            job.lease_owner,
            at,
            outcome,
            error_class.map(|value| truncate(value, 80)),
            error_message.map(|value| truncate(value, 1_000))
        ],
    )?;
    Ok(())
}

#[derive(Debug, Clone)]
pub struct JobFailure {
    pub class: String,
    pub message: String,
}

#[async_trait]
pub trait JobHandler: Send + Sync + 'static {
    async fn handle(&self, job: Job) -> Result<(), JobFailure>;
}

pub struct Worker<H> {
    queue: JobQueue,
    handler: Arc<H>,
    worker_id: String,
    concurrency: usize,
    lease_for: Duration,
    retry_delay: Duration,
}

impl<H: JobHandler> Worker<H> {
    pub fn new(queue: JobQueue, handler: Arc<H>, concurrency: usize) -> Self {
        Self {
            queue,
            handler,
            worker_id: Uuid::new_v4().to_string(),
            concurrency: concurrency.max(1),
            lease_for: Duration::from_secs(60),
            retry_delay: Duration::from_secs(10),
        }
    }

    pub async fn run_once(&self) -> Result<usize, QueueError> {
        let mut jobs = Vec::new();
        for _ in 0..self.concurrency {
            let queue = self.queue.clone();
            let worker_id = self.worker_id.clone();
            let lease_for = self.lease_for;
            let leased = tokio::task::spawn_blocking(move || {
                queue.lease(&worker_id, lease_for, unix_now()?)
            })
            .await
            .map_err(|_| QueueError::WorkerTask)??;
            let Some(job) = leased else {
                break;
            };
            jobs.push(job);
        }
        let count = jobs.len();
        let mut tasks = JoinSet::new();
        for job in jobs {
            let handler = self.handler.clone();
            let queue = self.queue.clone();
            let retry_delay = self.retry_delay;
            tasks.spawn(async move {
                let outcome = handler.handle(job.clone()).await;
                tokio::task::spawn_blocking(move || {
                    let at = unix_now()?;
                    match outcome {
                        Ok(()) => queue.complete(&job, at),
                        Err(failure) => queue
                            .fail(&job, at, &failure.class, &failure.message, retry_delay)
                            .map(|_| ()),
                    }
                })
                .await
                .map_err(|_| QueueError::WorkerTask)?
            });
        }
        while let Some(result) = tasks.join_next().await {
            result.map_err(|_| QueueError::WorkerTask)??;
        }
        Ok(count)
    }
}

fn truncate(value: &str, maximum: usize) -> String {
    value.chars().take(maximum).collect()
}

fn unix_now() -> Result<i64, QueueError> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| QueueError::Clock)?
        .as_secs();
    Ok(i64::try_from(seconds).unwrap_or(i64::MAX))
}

include!("tests.rs");
