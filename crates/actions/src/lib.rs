use reqwest::{
    Method,
    header::{HeaderMap, HeaderName, HeaderValue},
};
use rill_db::{DbError, DbPool};
use rill_jobs::{EnqueueOptions, JobKind, JobQueue, QueueError};
use rill_secrets::{SecretError, SecretStore};
use rill_source_api::{BoundedHttpClient, FetchError, FetchPolicy, validate_outbound_url};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{collections::BTreeMap, env, time::Duration};
use thiserror::Error;
use url::Url;
use uuid::Uuid;

#[derive(Debug, Error)]
pub enum ActionError {
    #[error("action was not found")]
    NotFound,
    #[error("invalid action configuration: {0}")]
    InvalidConfig(String),
    #[error("encrypted secret storage is required for action headers")]
    SecretsUnavailable,
    #[error("database error: {0}")]
    Database(#[from] DbError),
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("job queue error: {0}")]
    Queue(#[from] QueueError),
    #[error("secret error: {0}")]
    Secret(#[from] SecretError),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("HTTP action failed: {0}")]
    Http(String),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HttpActionConfig {
    pub url: String,
    #[serde(default = "default_method")]
    pub method: String,
    #[serde(default = "default_timeout")]
    pub timeout_seconds: u64,
    #[serde(default = "default_response_limit")]
    pub maximum_response_bytes: usize,
    #[serde(default = "default_attempts")]
    pub maximum_attempts: u32,
    #[serde(default)]
    pub body_template: Option<Value>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HeaderEnvValue {
    pub env: String,
    #[serde(default)]
    pub prefix: String,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct CreateHttpAction {
    pub name: String,
    #[serde(flatten)]
    pub config: HttpActionConfig,
    #[serde(default)]
    pub headers: BTreeMap<String, String>,
    #[serde(default)]
    pub header_env: BTreeMap<String, HeaderEnvValue>,
    #[serde(default = "enabled")]
    pub enabled: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ActionView {
    pub id: String,
    pub name: String,
    pub kind: String,
    pub enabled: bool,
    pub event: String,
    pub config: HttpActionConfig,
    pub has_headers: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExecuteActionPayload {
    pub execution_id: String,
}

#[derive(Clone)]
pub struct ActionService {
    pool: DbPool,
    jobs: JobQueue,
    secrets: Option<SecretStore>,
    allow_private_networks: bool,
}

impl ActionService {
    pub fn new(pool: DbPool, secrets: Option<SecretStore>, allow_private_networks: bool) -> Self {
        Self {
            jobs: JobQueue::new(pool.clone()),
            pool,
            secrets,
            allow_private_networks,
        }
    }

    pub fn create_http(
        &self,
        user_id: &str,
        request: CreateHttpAction,
    ) -> Result<ActionView, ActionError> {
        if request.name.trim().is_empty() {
            return Err(ActionError::InvalidConfig("name is required".into()));
        }
        validate_config(&request.config, self.allow_private_networks)?;
        validate_header_env(&request.header_env)?;
        let mut headers = request.headers.clone();
        headers.extend(resolve_header_env(&request.header_env, |name| {
            env::var(name)
        })?);
        validate_headers(&headers)?;
        let secret_id = if headers.is_empty() {
            None
        } else {
            Some(
                self.secrets
                    .as_ref()
                    .ok_or(ActionError::SecretsUnavailable)?
                    .put(
                        Some(user_id),
                        "http-action-headers",
                        &serde_json::to_vec(&headers)?,
                    )?,
            )
        };
        let id = Uuid::new_v4().to_string();
        let trigger_id = Uuid::new_v4().to_string();
        let mut connection = self.pool.connection()?;
        let transaction = connection.transaction()?;
        if let Err(error) = transaction.execute(
            "INSERT INTO action_definitions(id, owner_user_id, name, action_kind, config_json,
             credential_secret_id, enabled) VALUES (?1, ?2, ?3, 'http', ?4, ?5, ?6)",
            params![
                id,
                user_id,
                request.name.trim(),
                serde_json::to_string(&request.config)?,
                secret_id,
                request.enabled
            ],
        ) {
            if let (Some(store), Some(secret_id)) = (&self.secrets, &secret_id) {
                let _ = store.delete(secret_id);
            }
            return Err(error.into());
        }
        transaction.execute(
            "INSERT INTO action_triggers(id, action_definition_id, event_kind, enabled)
             VALUES (?1, ?2, 'story.favorite', 1)",
            params![trigger_id, id],
        )?;
        transaction.commit()?;
        Ok(ActionView {
            id,
            name: request.name.trim().into(),
            kind: "http".into(),
            enabled: request.enabled,
            event: "story.favorite".into(),
            config: request.config,
            has_headers: secret_id.is_some(),
        })
    }

    pub fn list(&self, user_id: &str) -> Result<Vec<ActionView>, ActionError> {
        let connection = self.pool.connection()?;
        let mut statement = connection.prepare(
            "SELECT ad.id, ad.name, ad.action_kind, ad.enabled, at.event_kind, ad.config_json,
             ad.credential_secret_id IS NOT NULL FROM action_definitions ad
             JOIN action_triggers at ON at.action_definition_id=ad.id
             WHERE ad.owner_user_id=?1 ORDER BY ad.created_at, ad.id",
        )?;
        let rows = statement.query_map([user_id], |row| {
            Ok((
                row.get(0)?,
                row.get(1)?,
                row.get(2)?,
                row.get::<_, i64>(3)? != 0,
                row.get(4)?,
                row.get::<_, String>(5)?,
                row.get::<_, i64>(6)? != 0,
            ))
        })?;
        rows.map(|row| {
            let (id, name, kind, enabled, event, raw, has_headers) = row?;
            Ok(ActionView {
                id,
                name,
                kind,
                enabled,
                event,
                config: serde_json::from_str(&raw)?,
                has_headers,
            })
        })
        .collect()
    }

    pub fn set_enabled(&self, user_id: &str, id: &str, enabled: bool) -> Result<(), ActionError> {
        let changed = self.pool.with_connection(|connection| {
            connection.execute(
                "UPDATE action_definitions SET enabled=?3 WHERE id=?1 AND owner_user_id=?2",
                params![id, user_id, enabled],
            )
        })?;
        if changed == 1 {
            Ok(())
        } else {
            Err(ActionError::NotFound)
        }
    }

    pub fn remove(&self, user_id: &str, id: &str) -> Result<(), ActionError> {
        let mut connection = self.pool.connection()?;
        let transaction = connection.transaction()?;
        let secret_id: Option<Option<String>> = transaction
            .query_row(
                "SELECT credential_secret_id FROM action_definitions
                 WHERE id=?1 AND owner_user_id=?2",
                params![id, user_id],
                |row| row.get(0),
            )
            .optional()?;
        let Some(secret_id) = secret_id else {
            return Err(ActionError::NotFound);
        };
        transaction.execute("DELETE FROM action_definitions WHERE id=?1", [id])?;
        transaction.commit()?;
        drop(connection);
        if let (Some(store), Some(secret_id)) = (&self.secrets, secret_id) {
            let _ = store.delete(&secret_id)?;
        }
        Ok(())
    }

    pub fn enqueue_favorite(&self, user_id: &str, event_id: &str) -> Result<usize, ActionError> {
        let connection = self.pool.connection()?;
        let mut statement = connection.prepare(
            "SELECT ad.id, at.id, ad.config_json FROM action_definitions ad
             JOIN action_triggers at ON at.action_definition_id=ad.id
             WHERE ad.owner_user_id=?1 AND ad.enabled=1 AND at.enabled=1
             AND at.event_kind='story.favorite'",
        )?;
        let matches = statement
            .query_map([user_id], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, String>(2)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        drop(statement);
        drop(connection);
        let mut queued = 0;
        for (action_id, trigger_id, raw_config) in &matches {
            let config: HttpActionConfig = serde_json::from_str(raw_config)?;
            let execution_id = Uuid::new_v4().to_string();
            let key = format!("action:{action_id}:{event_id}");
            let changed = self.pool.with_connection(|connection| {
                connection.execute(
                    "INSERT INTO action_executions(id, action_definition_id, trigger_id, user_id,
                     event_id, idempotency_key, status) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'queued')
                     ON CONFLICT(idempotency_key) DO NOTHING",
                    params![execution_id, action_id, trigger_id, user_id, event_id, key],
                )
            })?;
            if changed == 1 {
                let enqueue_result = self.jobs.enqueue(
                    JobKind::ExecuteAction,
                    &serde_json::to_value(ExecuteActionPayload { execution_id })?,
                    EnqueueOptions {
                        visibility_scope: Some(format!("user:{user_id}")),
                        max_attempts: config.maximum_attempts,
                        idempotency_key: Some(key.clone()),
                        ..Default::default()
                    },
                );
                if let Err(error) = enqueue_result {
                    self.pool.with_connection(|connection| {
                        connection.execute(
                            "DELETE FROM action_executions WHERE idempotency_key=?1",
                            [&key],
                        )
                    })?;
                    return Err(error.into());
                }
                queued += 1;
            }
        }
        Ok(queued)
    }

    pub async fn execute(
        &self,
        payload: &ExecuteActionPayload,
        attempt: u32,
        max_attempts: u32,
    ) -> Result<(), ActionError> {
        let result = self.execute_inner(payload).await;
        let (status, response_class, error_message) = match &result {
            Ok(class) => ("succeeded", Some(class.as_str()), None),
            Err(error) if attempt >= max_attempts => ("failed", None, Some(error.to_string())),
            Err(error) => ("retrying", None, Some(error.to_string())),
        };
        self.pool.with_connection(|connection| {
            let transaction = connection.unchecked_transaction()?;
            transaction.execute(
                "INSERT INTO action_attempts(id, execution_id, attempt_number, status,
                 response_class, error_message, started_at, finished_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, unixepoch(), unixepoch())",
                params![
                    Uuid::new_v4().to_string(),
                    payload.execution_id,
                    attempt,
                    status,
                    response_class,
                    error_message.as_deref().map(redact_error)
                ],
            )?;
            transaction.execute(
                "UPDATE action_executions SET status=?2,
                 completed_at=CASE WHEN ?2 IN ('succeeded','failed') THEN unixepoch() ELSE NULL END
                 WHERE id=?1",
                params![payload.execution_id, status],
            )?;
            transaction.commit()
        })?;
        result.map(|_| ())
    }

    async fn execute_inner(&self, payload: &ExecuteActionPayload) -> Result<String, ActionError> {
        type Execution = (String, String, Option<String>, String, String, String, bool);
        let record: Option<Execution> = self.pool.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT ad.config_json, ad.action_kind, ad.credential_secret_id,
                         ae.idempotency_key, ae.event_id, ae.user_id, ad.enabled
                         FROM action_executions ae
                         JOIN action_definitions ad ON ad.id=ae.action_definition_id
                         WHERE ae.id=?1",
                    [&payload.execution_id],
                    |row| {
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            row.get(2)?,
                            row.get(3)?,
                            row.get(4)?,
                            row.get(5)?,
                            row.get::<_, i64>(6)? != 0,
                        ))
                    },
                )
                .optional()
        })?;
        let (raw_config, kind, secret_id, key, event_id, user_id, enabled) =
            record.ok_or(ActionError::NotFound)?;
        if !enabled {
            return Ok("disabled".into());
        }
        if kind != "http" {
            return Err(ActionError::InvalidConfig("unsupported action kind".into()));
        }
        let config: HttpActionConfig = serde_json::from_str(&raw_config)?;
        validate_config(&config, self.allow_private_networks)?;
        let event = self.event_payload(&user_id, &event_id)?;
        let body = render_body(&config, &event)?;
        let headers = self.load_headers(secret_id.as_deref())?;
        send_http(&config, &headers, &key, &body, self.allow_private_networks).await
    }

    fn load_headers(
        &self,
        secret_id: Option<&str>,
    ) -> Result<BTreeMap<String, String>, ActionError> {
        let Some(secret_id) = secret_id else {
            return Ok(BTreeMap::new());
        };
        Ok(serde_json::from_slice(
            &self
                .secrets
                .as_ref()
                .ok_or(ActionError::SecretsUnavailable)?
                .get(secret_id)?,
        )?)
    }

    fn event_payload(&self, user_id: &str, event_id: &str) -> Result<Value, ActionError> {
        type Story = (
            String,
            String,
            String,
            Option<String>,
            Option<String>,
            Option<i64>,
            Option<String>,
            Option<String>,
        );
        let result: Option<Story> = self.pool.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT s.id, d.id, d.title, d.canonical_url, d.publisher, d.published_at,
                     (SELECT summary_text FROM summaries su WHERE su.entity_type='document'
                      AND su.entity_id=d.id AND su.input_checksum=d.exact_content_hash
                      ORDER BY su.created_at DESC LIMIT 1),
                     (SELECT dc.curator_id FROM document_curators dc WHERE dc.document_id=d.id
                      ORDER BY dc.created_at LIMIT 1)
                     FROM feedback_events fe JOIN stories s ON s.id=fe.story_id
                     JOIN documents d ON d.id=fe.document_id
                     WHERE fe.id=?1 AND fe.user_id=?2 AND fe.feedback='favorite'",
                    params![event_id, user_id],
                    |row| {
                        Ok((
                            row.get(0)?,
                            row.get(1)?,
                            row.get(2)?,
                            row.get(3)?,
                            row.get(4)?,
                            row.get(5)?,
                            row.get(6)?,
                            row.get(7)?,
                        ))
                    },
                )
                .optional()
        })?;
        let (id, document_id, title, url, source, published_at, summary, curator) =
            result.ok_or(ActionError::NotFound)?;
        let related_links = self.pool.with_connection(|connection| {
            let mut statement = connection.prepare(
                "SELECT normalized_url FROM document_links WHERE document_id=?1
                 AND relation <> 'alternate' ORDER BY ordinal, normalized_url LIMIT 15",
            )?;
            let rows = statement.query_map([document_id], |row| row.get::<_, String>(0))?;
            rows.collect::<rusqlite::Result<Vec<_>>>()
        })?;
        Ok(json!({
            "event": "story.favorite",
            "eventId": event_id,
            "story": { "id": id, "title": title, "summary": summary, "url": url,
                "source": source, "curator": curator, "publishedAt": published_at,
                "relatedLinks": related_links }
        }))
    }
}

async fn send_http(
    config: &HttpActionConfig,
    headers: &BTreeMap<String, String>,
    idempotency_key: &str,
    body: &Value,
    allow_private_networks: bool,
) -> Result<String, ActionError> {
    let url =
        Url::parse(&config.url).map_err(|error| ActionError::InvalidConfig(error.to_string()))?;
    validate_outbound_url(&url, allow_private_networks).map_err(fetch_error)?;
    let method = Method::from_bytes(config.method.as_bytes())
        .map_err(|_| ActionError::InvalidConfig("invalid method".into()))?;
    let mut request_headers = HeaderMap::new();
    for (name, value) in headers {
        request_headers.insert(
            HeaderName::from_bytes(name.as_bytes())
                .map_err(|_| ActionError::InvalidConfig("invalid header name".into()))?,
            HeaderValue::from_str(value)
                .map_err(|_| ActionError::InvalidConfig("invalid header value".into()))?,
        );
    }
    request_headers.insert(
        HeaderName::from_static("idempotency-key"),
        HeaderValue::from_str(idempotency_key)
            .map_err(|_| ActionError::InvalidConfig("invalid idempotency key".into()))?,
    );
    let client = BoundedHttpClient::new(FetchPolicy {
        timeout: Duration::from_secs(config.timeout_seconds),
        max_redirects: 5,
        max_response_bytes: config.maximum_response_bytes,
        allow_private_networks,
    })
    .map_err(|error| ActionError::Http(error.to_string()))?;
    client
        .send_json(method, &url, request_headers, serde_json::to_vec(body)?)
        .await
        .map_err(|error| ActionError::Http(error.to_string()))?;
    Ok("2xx".into())
}

include!("validation.rs");

include!("tests.rs");
