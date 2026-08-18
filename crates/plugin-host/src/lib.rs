use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    sync::{Arc, Mutex},
    thread,
    time::Duration,
};

use base64::{Engine as _, engine::general_purpose};
use rill_db::{DbError, DbPool};
use rill_secrets::{SecretError, SecretStore};
use rill_source_api::{BoundedHttpClient, ConditionalHeaders, SourceBatch};
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use thiserror::Error;
use url::Url;
use uuid::Uuid;
use wasmtime::component::{Component, HasSelf, Linker, bindgen};
use wasmtime::{Config, Engine, Store, StoreLimits, StoreLimitsBuilder};

bindgen!({
    path: "wit",
    world: "plugin",
    imports: { default: async },
    exports: { default: async },
});

const EPOCH_TICK: Duration = Duration::from_millis(10);

#[derive(Debug, Clone)]
pub struct PluginLimits {
    pub memory_bytes: usize,
    pub fuel: u64,
    pub timeout: Duration,
    pub maximum_output_bytes: usize,
    pub maximum_component_bytes: usize,
    pub maximum_http_bytes: usize,
}

#[derive(Debug, Error)]
pub enum PluginError {
    #[error("plugin was not found")]
    NotFound,
    #[error("plugin is disabled")]
    Disabled,
    #[error("plugin is still used by a source")]
    InUse,
    #[error("invalid plugin: {0}")]
    Invalid(String),
    #[error("plugin trapped: {0}")]
    Trap(String),
    #[error("plugin output exceeds {0} byte limit")]
    OutputTooLarge(usize),
    #[error("plugin capability is denied: {0}")]
    CapabilityDenied(String),
    #[error("database error: {0}")]
    Database(#[from] DbError),
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("secret error: {0}")]
    Secret(#[from] SecretError),
    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginMetadata {
    pub id: String,
    pub name: String,
    pub version: String,
    pub description: String,
    #[serde(default)]
    pub requested_permissions: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginView {
    pub installation_id: String,
    pub metadata: PluginMetadata,
    pub component_sha256: String,
    pub config_schema: Value,
    pub enabled: bool,
    pub granted_permissions: Vec<PluginPermission>,
    pub last_success_at: Option<i64>,
    pub consecutive_failures: u32,
    pub last_error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginInspection {
    pub metadata: PluginMetadata,
    pub component_sha256: String,
    pub config_schema: Value,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PluginPermission {
    pub capability: String,
    pub constraint: Value,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct PluginSourceConfig {
    pub plugin_installation_id: String,
    #[serde(default)]
    pub plugin_config: Value,
    #[serde(default = "default_poll_seconds")]
    pub poll_interval_seconds: u64,
    #[serde(default = "enabled")]
    pub enabled: bool,
}

#[derive(Clone)]
pub struct PluginService {
    pool: DbPool,
    secrets: Option<SecretStore>,
    http: Arc<BoundedHttpClient>,
    engine: Arc<Engine>,
    limits: PluginLimits,
    cache: Arc<Mutex<HashMap<String, Arc<Component>>>>,
}

struct HostState {
    limits: StoreLimits,
    secrets: Option<SecretStore>,
    named_secrets: BTreeMap<String, String>,
    allowed_hosts: BTreeSet<String>,
    secret_values: Vec<String>,
    http: Arc<BoundedHttpClient>,
    maximum_http_bytes: usize,
}

include!("service.rs");
include!("inspection.rs");
include!("host.rs");
include!("validation.rs");
include!("tests.rs");
