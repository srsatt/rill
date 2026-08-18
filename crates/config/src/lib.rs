use serde::Deserialize;
use std::{
    env, fs,
    net::SocketAddr,
    path::{Path, PathBuf},
    time::Duration,
};
use thiserror::Error;
use url::Url;

#[derive(Debug, Error)]
pub enum ConfigError {
    #[error("could not read configuration {path}: {source}")]
    Read {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("invalid TOML in {path}: {source}")]
    Parse {
        path: PathBuf,
        source: toml::de::Error,
    },
    #[error("invalid configuration: {0}")]
    Invalid(String),
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub http: HttpSettings,
    pub database: DatabaseSettings,
    pub assets: AssetSettings,
    pub auth: AuthSettings,
    pub renderer: RendererSettings,
    pub fetch: FetchSettings,
    pub ingestion: IngestionSettings,
    pub jobs: JobSettings,
    pub secrets: SecretSettings,
    pub email: EmailSettings,
    pub models: ModelSettings,
    pub plugins: PluginSettings,
    pub metrics: MetricsSettings,
    pub logging: LoggingSettings,
}

include!("settings.rs");

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HttpSettings {
    pub bind: String,
    pub public_base_url: String,
    pub secure_cookies: bool,
}

impl Default for HttpSettings {
    fn default() -> Self {
        Self {
            bind: "127.0.0.1:3000".to_owned(),
            public_base_url: "http://127.0.0.1:3000".to_owned(),
            secure_cookies: false,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DatabaseSettings {
    pub path: PathBuf,
    pub pool_size: usize,
    pub busy_timeout_ms: u64,
}

impl Default for DatabaseSettings {
    fn default() -> Self {
        Self {
            path: PathBuf::from("var/rill.db"),
            pool_size: 4,
            busy_timeout_ms: 5_000,
        }
    }
}

impl DatabaseSettings {
    pub fn busy_timeout(&self) -> Duration {
        Duration::from_millis(self.busy_timeout_ms)
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AssetSettings {
    pub static_dir: PathBuf,
    pub renderer_wasm: PathBuf,
    pub plugin_dir: PathBuf,
}

impl Default for AssetSettings {
    fn default() -> Self {
        Self {
            static_dir: PathBuf::from("ui/dist/client"),
            renderer_wasm: PathBuf::from("artifacts/ui-renderer.wasm"),
            plugin_dir: PathBuf::from("plugins"),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AuthSettings {
    pub session_days: u64,
    pub reader_session_days: u64,
    pub pairing_minutes: u64,
    pub pairing_max_attempts: u32,
}

impl Default for AuthSettings {
    fn default() -> Self {
        Self {
            session_days: 30,
            reader_session_days: 180,
            pairing_minutes: 10,
            pairing_max_attempts: 5,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RendererSettings {
    pub memory_bytes: usize,
    pub fuel: u64,
    pub timeout_ms: u64,
    pub max_response_bytes: usize,
}

impl Default for RendererSettings {
    fn default() -> Self {
        Self {
            memory_bytes: 64 * 1024 * 1024,
            fuel: 200_000_000,
            timeout_ms: 750,
            max_response_bytes: 2 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct FetchSettings {
    pub timeout_seconds: u64,
    pub max_redirects: usize,
    pub max_response_bytes: usize,
    pub allow_private_networks: bool,
}

impl Default for FetchSettings {
    fn default() -> Self {
        Self {
            timeout_seconds: 15,
            max_redirects: 5,
            max_response_bytes: 4 * 1024 * 1024,
            allow_private_networks: false,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct IngestionSettings {
    pub maximum_collection_fan_out: usize,
    pub poll_item_limit: usize,
    pub collection_detection_threshold: f32,
    pub collection_parent_display_default: String,
    pub collection_excluded_hosts: Vec<String>,
    pub collection_excluded_path_fragments: Vec<String>,
}

impl Default for IngestionSettings {
    fn default() -> Self {
        Self {
            maximum_collection_fan_out: 25,
            poll_item_limit: 200,
            collection_detection_threshold: 0.65,
            collection_parent_display_default: "children_only".to_owned(),
            collection_excluded_hosts: Vec::new(),
            collection_excluded_path_fragments: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct JobSettings {
    pub concurrency: usize,
    pub idle_poll_ms: u64,
}

impl Default for JobSettings {
    fn default() -> Self {
        Self {
            concurrency: 2,
            idle_poll_ms: 500,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SecretSettings {
    pub master_key_env: String,
    pub key_version: i64,
}

impl Default for SecretSettings {
    fn default() -> Self {
        Self {
            master_key_env: "RILL_MASTER_KEY".into(),
            key_version: 1,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct EmailSettings {
    pub timeout_seconds: u64,
    pub maximum_message_bytes: usize,
}

impl Default for EmailSettings {
    fn default() -> Self {
        Self {
            timeout_seconds: 30,
            maximum_message_bytes: 8 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ModelSettings {
    pub embedding: Option<HttpModelSettings>,
    pub summary: Option<HttpModelSettings>,
    pub recommendation: Option<HttpModelSettings>,
    pub collection_parser: Option<HttpModelSettings>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct HttpModelSettings {
    pub base_url: String,
    pub provider: String,
    pub model: String,
    pub version: String,
    pub api_key_env: Option<String>,
    pub timeout_seconds: u64,
    pub maximum_request_bytes: usize,
    pub maximum_response_bytes: usize,
    pub maximum_batch_items: usize,
    pub retries: usize,
    pub circuit_failure_threshold: u32,
    pub circuit_cooldown_seconds: u64,
}

impl Default for HttpModelSettings {
    fn default() -> Self {
        Self {
            base_url: "http://127.0.0.1:11434/v1/".into(),
            provider: "openai-compatible".into(),
            model: "model".into(),
            version: "configured".into(),
            api_key_env: None,
            timeout_seconds: 30,
            maximum_request_bytes: 512 * 1024,
            maximum_response_bytes: 4 * 1024 * 1024,
            maximum_batch_items: 128,
            retries: 2,
            circuit_failure_threshold: 3,
            circuit_cooldown_seconds: 30,
        }
    }
}

impl HttpModelSettings {
    fn validate(&self, name: &str) -> Result<(), ConfigError> {
        let url = Url::parse(&self.base_url).map_err(|error| {
            ConfigError::Invalid(format!("models.{name}.base_url is invalid: {error}"))
        })?;
        if !matches!(url.scheme(), "http" | "https") || url.host_str().is_none() {
            return Err(ConfigError::Invalid(format!(
                "models.{name}.base_url must be absolute HTTP(S)"
            )));
        }
        if self.provider.trim().is_empty()
            || self.model.trim().is_empty()
            || self.version.trim().is_empty()
            || self.timeout_seconds == 0
            || self.maximum_request_bytes < 1024
            || self.maximum_response_bytes < 1024
            || self.maximum_batch_items == 0
            || self.circuit_failure_threshold == 0
            || self.circuit_cooldown_seconds == 0
            || self
                .api_key_env
                .as_deref()
                .is_some_and(|value| value.trim().is_empty())
        {
            return Err(ConfigError::Invalid(format!(
                "models.{name} identity or limits are invalid"
            )));
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct PluginSettings {
    pub memory_bytes: usize,
    pub fuel: u64,
    pub timeout_ms: u64,
    pub maximum_output_bytes: usize,
    pub maximum_component_bytes: usize,
    pub maximum_http_bytes: usize,
}

impl Default for PluginSettings {
    fn default() -> Self {
        Self {
            memory_bytes: 32 * 1024 * 1024,
            fuel: 10_000_000,
            timeout_ms: 2_000,
            maximum_output_bytes: 4 * 1024 * 1024,
            maximum_component_bytes: 16 * 1024 * 1024,
            maximum_http_bytes: 4 * 1024 * 1024,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MetricsSettings {
    pub enabled: bool,
    pub path: String,
}

impl Default for MetricsSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            path: "/metrics".to_owned(),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LoggingSettings {
    pub filter: String,
}

impl Default for LoggingSettings {
    fn default() -> Self {
        Self {
            filter: "rill=info,tower_http=info".to_owned(),
        }
    }
}

include!("tests.rs");
