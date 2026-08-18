use rusqlite::params;
use serde::Serialize;
use serde_json::Value;

use super::{JobQueue, QueueError, unix_now};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobView {
    pub id: String,
    pub kind: String,
    pub payload: Value,
    pub visibility_scope: Option<String>,
    pub priority: i32,
    pub status: String,
    pub available_at: i64,
    pub attempt_count: u32,
    pub max_attempts: u32,
    pub last_error_class: Option<String>,
    pub last_error_message: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub completed_at: Option<i64>,
    pub attempts: Vec<JobAttemptView>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct JobAttemptView {
    pub attempt_number: u32,
    pub worker_id: String,
    pub started_at: i64,
    pub finished_at: Option<i64>,
    pub outcome: Option<String>,
    pub error_class: Option<String>,
    pub error_message: Option<String>,
}

impl JobQueue {
    pub fn list(&self, status: Option<&str>, limit: usize) -> Result<Vec<JobView>, QueueError> {
        let connection = self.pool.connection()?;
        let mut statement = connection.prepare(
            "SELECT id, kind, payload_json, visibility_scope, priority, status, available_at,
             attempt_count, max_attempts, last_error_class, last_error_message, created_at,
             updated_at, completed_at FROM jobs WHERE (?1 IS NULL OR status=?1)
             ORDER BY created_at DESC LIMIT ?2",
        )?;
        let sql_limit = i64::try_from(limit.min(500)).unwrap_or(500);
        let rows = statement.query_map(params![status, sql_limit], |row| {
            let payload_text: String = row.get(2)?;
            let payload = serde_json::from_str(&payload_text).map_err(|error| {
                rusqlite::Error::FromSqlConversionFailure(
                    2,
                    rusqlite::types::Type::Text,
                    Box::new(error),
                )
            })?;
            Ok(JobView {
                id: row.get(0)?,
                kind: row.get(1)?,
                payload,
                visibility_scope: row.get(3)?,
                priority: row.get(4)?,
                status: row.get(5)?,
                available_at: row.get(6)?,
                attempt_count: row.get(7)?,
                max_attempts: row.get(8)?,
                last_error_class: row.get(9)?,
                last_error_message: row.get(10)?,
                created_at: row.get(11)?,
                updated_at: row.get(12)?,
                completed_at: row.get(13)?,
                attempts: Vec::new(),
            })
        })?;
        let mut jobs = rows.collect::<rusqlite::Result<Vec<_>>>()?;
        for job in &mut jobs {
            job.attempts = list_attempts(&connection, &job.id)?;
        }
        Ok(jobs)
    }

    pub fn retry_dead(&self, job_id: &str) -> Result<bool, QueueError> {
        let timestamp = unix_now()?;
        Ok(self.pool.with_connection(|connection| {
            connection.execute(
                "UPDATE jobs SET status='queued', available_at=?2, attempt_count=0,
                 lease_owner=NULL, lease_expires_at=NULL, last_error_class=NULL,
                 last_error_message=NULL, completed_at=NULL, updated_at=?2
                 WHERE id=?1 AND status='dead'",
                params![job_id, timestamp],
            )
        })? == 1)
    }

    pub fn cancel_queued(&self, job_id: &str) -> Result<bool, QueueError> {
        let timestamp = unix_now()?;
        Ok(self.pool.with_connection(|connection| {
            connection.execute(
                "UPDATE jobs SET status='dead', last_error_class='admin_cancelled',
                 last_error_message='Cancelled by administrator', completed_at=?2, updated_at=?2
                 WHERE id=?1 AND status='queued'",
                params![job_id, timestamp],
            )
        })? == 1)
    }
}

fn list_attempts(
    connection: &rusqlite::Connection,
    job_id: &str,
) -> rusqlite::Result<Vec<JobAttemptView>> {
    let mut statement = connection.prepare(
        "SELECT attempt_number, worker_id, started_at, finished_at, outcome, error_class,
         error_message FROM job_attempts WHERE job_id=?1 ORDER BY attempt_number DESC",
    )?;
    let rows = statement.query_map([job_id], |row| {
        Ok(JobAttemptView {
            attempt_number: row.get(0)?,
            worker_id: row.get(1)?,
            started_at: row.get(2)?,
            finished_at: row.get(3)?,
            outcome: row.get(4)?,
            error_class: row.get(5)?,
            error_message: row.get(6)?,
        })
    })?;
    rows.collect()
}

#[cfg(test)]
mod tests {
    use rill_db::DbPool;
    use serde_json::json;

    use super::*;
    use crate::{EnqueueOptions, JobKind};

    #[test]
    fn administrator_can_cancel_and_retry_without_mutating_leased_jobs() {
        let queue = JobQueue::new(DbPool::open_in_memory().unwrap());
        let job_id = queue
            .enqueue(
                JobKind::DatabaseMaintenance,
                &json!({}),
                EnqueueOptions::default(),
            )
            .unwrap();
        assert!(queue.cancel_queued(&job_id).unwrap());
        assert_eq!(queue.list(Some("dead"), 10).unwrap()[0].id, job_id);
        assert!(queue.retry_dead(&job_id).unwrap());
        assert_eq!(queue.list(Some("queued"), 10).unwrap()[0].attempt_count, 0);
        let leased = queue
            .lease("worker", std::time::Duration::from_secs(30), i64::MAX / 2)
            .unwrap()
            .unwrap();
        assert!(!queue.cancel_queued(&leased.id).unwrap());
    }
}
