use std::{
    collections::BTreeMap,
    fmt::Write as _,
    sync::{Arc, Mutex},
    time::Duration,
};

use rill_db::DbPool;

#[derive(Debug, Clone, Copy, Default)]
struct Series {
    count: u64,
    failures: u64,
    duration_micros: u128,
    items: u64,
}

#[derive(Debug, Default)]
struct Inner {
    operations: Mutex<BTreeMap<String, Series>>,
}

#[derive(Debug, Clone)]
pub struct Metrics {
    inner: Arc<Inner>,
    renderer_memory_limit: usize,
}

impl Metrics {
    pub fn new(renderer_memory_limit: usize) -> Self {
        Self {
            inner: Arc::new(Inner::default()),
            renderer_memory_limit,
        }
    }

    pub fn observe(&self, operation: &str, duration: Duration, success: bool, items: usize) {
        let Ok(mut series) = self.inner.operations.lock() else {
            return;
        };
        let entry = series.entry(operation.to_owned()).or_default();
        entry.count = entry.count.saturating_add(1);
        entry.failures = entry.failures.saturating_add(u64::from(!success));
        entry.duration_micros = entry.duration_micros.saturating_add(duration.as_micros());
        entry.items = entry
            .items
            .saturating_add(u64::try_from(items).unwrap_or(u64::MAX));
    }

    pub fn render(&self, pool: &DbPool) -> Result<String, rill_db::DbError> {
        let operations = self
            .inner
            .operations
            .lock()
            .map(|guard| guard.clone())
            .unwrap_or_default();
        let mut output = String::with_capacity(8_192);
        output.push_str("# HELP rill_operation_duration_seconds Operation wall time.\n");
        output.push_str("# TYPE rill_operation_duration_seconds summary\n");
        output.push_str("# HELP rill_operation_failures_total Failed operations.\n");
        output.push_str("# TYPE rill_operation_failures_total counter\n");
        output.push_str("# HELP rill_operation_items_total Items produced by operations.\n");
        output.push_str("# TYPE rill_operation_items_total counter\n");
        for (operation, series) in operations {
            let label = prometheus_label(&operation);
            let _ = writeln!(
                output,
                "rill_operation_duration_seconds_count{{operation=\"{label}\"}} {}",
                series.count
            );
            let _ = writeln!(
                output,
                "rill_operation_duration_seconds_sum{{operation=\"{label}\"}} {:.6}",
                series.duration_micros as f64 / 1_000_000.0
            );
            let _ = writeln!(
                output,
                "rill_operation_failures_total{{operation=\"{label}\"}} {}",
                series.failures
            );
            let _ = writeln!(
                output,
                "rill_operation_items_total{{operation=\"{label}\"}} {}",
                series.items
            );
        }
        let _ = writeln!(
            output,
            "# HELP rill_renderer_memory_limit_bytes Configured renderer memory limit.\n# TYPE rill_renderer_memory_limit_bytes gauge\nrill_renderer_memory_limit_bytes {}",
            self.renderer_memory_limit
        );

        pool.with_connection(|connection| append_database_metrics(connection, &mut output))?;
        Ok(output)
    }
}

fn append_database_metrics(
    connection: &rusqlite::Connection,
    output: &mut String,
) -> rusqlite::Result<()> {
    output.push_str("# HELP rill_job_queue_depth Jobs by current state.\n");
    output.push_str("# TYPE rill_job_queue_depth gauge\n");
    let mut statement = connection.prepare("SELECT status, count(*) FROM jobs GROUP BY status")?;
    let rows = statement.query_map([], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    for row in rows {
        let (status, count) = row?;
        let _ = writeln!(
            output,
            "rill_job_queue_depth{{status=\"{}\"}} {count}",
            prometheus_label(&status)
        );
    }
    let retries: i64 = connection.query_row(
        "SELECT count(*) FROM job_attempts WHERE outcome='retry'",
        [],
        |row| row.get(0),
    )?;
    let _ = writeln!(
        output,
        "# HELP rill_job_retries_total Retried jobs.\n# TYPE rill_job_retries_total counter\nrill_job_retries_total {retries}"
    );

    append_scalar(
        connection,
        output,
        "rill_source_errors",
        "Current consecutive source errors.",
        "SELECT coalesce(sum(consecutive_failures), 0) FROM source_health",
        "gauge",
    )?;
    append_scalar(
        connection,
        output,
        "rill_new_items_total",
        "Persisted source items.",
        "SELECT count(*) FROM raw_items",
        "counter",
    )?;
    append_scalar(
        connection,
        output,
        "rill_collection_detections_total",
        "Collection detection records.",
        "SELECT count(*) FROM collection_expansions",
        "counter",
    )?;
    append_scalar(
        connection,
        output,
        "rill_collection_expansions_total",
        "Expanded collections.",
        "SELECT count(*) FROM collection_expansions WHERE status='expanded'",
        "counter",
    )?;
    append_scalar(
        connection,
        output,
        "rill_collection_parser_model_usage_total",
        "Model-assisted collection parser executions.",
        "SELECT count(*) FROM collection_expansions WHERE parser_kind NOT IN ('deterministic', 'telegram', 'email')",
        "counter",
    )?;
    append_scalar(
        connection,
        output,
        "rill_collection_fanout_total",
        "Collection-derived entries.",
        "SELECT count(*) FROM collection_entries",
        "counter",
    )?;
    append_scalar(
        connection,
        output,
        "rill_collection_parse_failures_total",
        "Failed collection parses.",
        "SELECT count(*) FROM collection_expansions WHERE status='failed'",
        "counter",
    )?;
    append_scalar(
        connection,
        output,
        "rill_extraction_errors_total",
        "Failed extraction job attempts.",
        "SELECT count(*) FROM job_attempts ja JOIN jobs j ON j.id=ja.job_id WHERE ja.outcome IN ('retry','dead') AND j.kind IN ('ExtractArticle','ProcessDerivedItem')",
        "counter",
    )?;
    append_scalar(
        connection,
        output,
        "rill_action_failures_total",
        "Failed action executions.",
        "SELECT count(*) FROM action_executions WHERE status IN ('failed','dead')",
        "counter",
    )?;
    append_scalar(
        connection,
        output,
        "rill_cluster_count",
        "Story clusters.",
        "SELECT count(*) FROM stories",
        "gauge",
    )?;
    append_scalar(
        connection,
        output,
        "rill_cluster_size_max",
        "Largest current story cluster.",
        "SELECT coalesce(max(size), 0) FROM (SELECT count(*) AS size FROM story_memberships GROUP BY story_id)",
        "gauge",
    )?;
    Ok(())
}

fn append_scalar(
    connection: &rusqlite::Connection,
    output: &mut String,
    name: &str,
    help: &str,
    sql: &str,
    metric_type: &str,
) -> rusqlite::Result<()> {
    let value: i64 = connection.query_row(sql, [], |row| row.get(0))?;
    let _ = writeln!(
        output,
        "# HELP {name} {help}\n# TYPE {name} {metric_type}\n{name} {value}"
    );
    Ok(())
}

fn prometheus_label(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exposition_includes_runtime_and_database_metrics() {
        let pool = DbPool::open_in_memory().unwrap();
        let metrics = Metrics::new(32 * 1024 * 1024);
        metrics.observe("renderer", Duration::from_millis(25), true, 1);
        let text = metrics.render(&pool).unwrap();
        assert!(text.contains("rill_operation_duration_seconds_count{operation=\"renderer\"} 1"));
        assert!(text.contains("rill_job_queue_depth"));
        assert!(text.contains("rill_collection_fanout_total 0"));
        assert!(text.contains("rill_renderer_memory_limit_bytes 33554432"));
    }
}
