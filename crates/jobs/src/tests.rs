#[cfg(test)]
mod tests {
    use super::*;

    fn queue() -> JobQueue {
        JobQueue::new(DbPool::open_in_memory().unwrap())
    }

    #[test]
    fn leases_by_priority_and_completes() {
        let queue = queue();
        queue
            .enqueue(
                JobKind::PollSource,
                &serde_json::json!({"sourceId":"low"}),
                EnqueueOptions {
                    available_at: Some(0),
                    ..Default::default()
                },
            )
            .unwrap();
        queue
            .enqueue(
                JobKind::PollSource,
                &serde_json::json!({"sourceId":"high"}),
                EnqueueOptions {
                    priority: 10,
                    available_at: Some(0),
                    ..Default::default()
                },
            )
            .unwrap();
        let job = queue
            .lease("worker", Duration::from_secs(30), 100)
            .unwrap()
            .unwrap();
        assert_eq!(job.payload["sourceId"], "high");
        queue.complete(&job, 101).unwrap();
        let next = queue
            .lease("worker", Duration::from_secs(30), 101)
            .unwrap()
            .unwrap();
        assert_eq!(next.payload["sourceId"], "low");
    }

    #[test]
    fn expired_lease_is_recovered_after_crash() {
        let queue = queue();
        queue
            .enqueue(
                JobKind::NormalizeRawItem,
                &Value::Null,
                EnqueueOptions {
                    available_at: Some(0),
                    ..Default::default()
                },
            )
            .unwrap();
        let first = queue
            .lease("dead-worker", Duration::from_secs(5), 100)
            .unwrap()
            .unwrap();
        assert!(
            queue
                .lease("new-worker", Duration::from_secs(5), 104)
                .unwrap()
                .is_none()
        );
        let recovered = queue
            .lease("new-worker", Duration::from_secs(5), 105)
            .unwrap()
            .unwrap();
        assert_eq!(first.id, recovered.id);
        assert_eq!(recovered.attempt_count, 2);
    }

    #[test]
    fn idempotency_key_returns_existing_job() {
        let queue = queue();
        let options = || EnqueueOptions {
            idempotency_key: Some("poll:source-1:cursor-1".into()),
            ..Default::default()
        };
        let first = queue
            .enqueue(JobKind::PollSource, &Value::Null, options())
            .unwrap();
        let second = queue
            .enqueue(JobKind::PollSource, &Value::Null, options())
            .unwrap();
        assert_eq!(first, second);
    }

    #[test]
    fn coalesced_queue_keeps_one_successor_and_pulls_it_forward() {
        let queue = queue();
        let payload = serde_json::json!({"sourceId":"channel"});
        let first = queue
            .enqueue_coalesced_queued(
                JobKind::PollSource,
                &payload,
                EnqueueOptions {
                    available_at: Some(500),
                    ..Default::default()
                },
            )
            .unwrap();
        let second = queue
            .enqueue_coalesced_queued(
                JobKind::PollSource,
                &payload,
                EnqueueOptions {
                    available_at: Some(100),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(first, second);
        let leased = queue
            .lease("worker", Duration::from_secs(30), 100)
            .unwrap()
            .unwrap();
        assert_eq!(leased.id, first);
    }

    #[test]
    fn retry_reaches_dead_letter_state() {
        let queue = queue();
        queue
            .enqueue(
                JobKind::ExtractArticle,
                &Value::Null,
                EnqueueOptions {
                    max_attempts: 1,
                    available_at: Some(0),
                    ..Default::default()
                },
            )
            .unwrap();
        let job = queue
            .lease("worker", Duration::from_secs(5), 100)
            .unwrap()
            .unwrap();
        assert!(
            queue
                .fail(&job, 101, "fetch", "timeout", Duration::from_secs(10))
                .unwrap()
        );
        assert!(
            queue
                .lease("worker", Duration::from_secs(5), 1_000)
                .unwrap()
                .is_none()
        );
    }
}
