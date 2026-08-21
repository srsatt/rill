use anyhow::{Context, Result, bail};
use rill_config::{HttpModelSettings, ModelSettings};
use rill_db::DbPool;
use rill_model_api::{EmbeddingProvider, RecommendationProvider, SummaryProvider};
use rill_secrets::SecretStore;
use rusqlite::{OptionalExtension, params};
use serde::{Deserialize, Serialize};
use url::Url;
use uuid::Uuid;

use crate::model_runtime::RuntimeModelRegistry;

const MODEL_SLOTS: [&str; 3] = ["embedding", "ranking", "text_parse"];

#[derive(Clone)]
pub(crate) struct GlobalSettingsService {
    pool: DbPool,
    secrets: Option<SecretStore>,
    models: RuntimeModelRegistry,
    configured_models: ModelSettings,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub(crate) struct ModelSettingInput {
    #[serde(default)]
    pub base_url: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    pub model: String,
    #[serde(default = "default_version")]
    pub version: String,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default)]
    pub clear_api_key: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct ModelSettingView {
    pub slot: String,
    pub mode: String,
    pub provider: String,
    pub model: String,
    pub version: String,
    pub base_url: Option<String>,
    pub api_key_configured: bool,
    pub connection_managed: bool,
    pub overridden: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum StoredModelConfig {
    ModelOverride {
        model: String,
        version: String,
    },
    ProviderOverride {
        #[serde(flatten)]
        provider: StoredProviderConfig,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct StoredProviderConfig {
    base_url: String,
    provider: String,
    model: String,
    version: String,
    timeout_seconds: u64,
    maximum_request_bytes: usize,
    maximum_response_bytes: usize,
    maximum_batch_items: usize,
    retries: usize,
    circuit_failure_threshold: u32,
    circuit_cooldown_seconds: u64,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct LegacyStoredModelConfig {
    base_url: String,
    provider: String,
    model: String,
    version: String,
    timeout_seconds: u64,
    maximum_request_bytes: usize,
    maximum_response_bytes: usize,
    maximum_batch_items: usize,
    retries: usize,
    circuit_failure_threshold: u32,
    circuit_cooldown_seconds: u64,
}

impl GlobalSettingsService {
    pub fn new(
        pool: DbPool,
        secrets: Option<SecretStore>,
        models: RuntimeModelRegistry,
        configured_models: ModelSettings,
    ) -> Self {
        Self {
            pool,
            secrets,
            models,
            configured_models,
        }
    }

    pub fn apply_persisted_models(&self) -> Result<()> {
        for slot in MODEL_SLOTS {
            if let Some((config, secret_id)) = self.load_model(slot)? {
                let configured = self.configured_model(slot);
                let settings = config.as_http_settings(configured)?;
                let api_key = if configured.is_some() {
                    None
                } else {
                    self.read_secret(secret_id.as_deref())?
                };
                self.apply_model(slot, Some(&settings), api_key)?;
            }
        }
        Ok(())
    }

    pub fn list_models(&self) -> Result<Vec<ModelSettingView>> {
        MODEL_SLOTS
            .into_iter()
            .map(|slot| {
                let stored = self.load_model(slot)?;
                if let Some((config, secret_id)) = stored {
                    let configured = self.configured_model(slot);
                    let settings = config.as_http_settings(configured)?;
                    let connection_managed = configured.is_some();
                    Ok(ModelSettingView {
                        slot: slot.into(),
                        mode: if connection_managed {
                            "override"
                        } else {
                            "http"
                        }
                        .into(),
                        provider: settings.provider,
                        model: settings.model,
                        version: settings.version,
                        base_url: Some(settings.base_url),
                        api_key_configured: if connection_managed {
                            configured.is_some_and(|value| value.api_key_env.is_some())
                        } else {
                            secret_id.is_some()
                        },
                        connection_managed,
                        overridden: true,
                    })
                } else if let Some(configured) = self.configured_model(slot) {
                    Ok(ModelSettingView {
                        slot: slot.into(),
                        mode: "configured".into(),
                        provider: configured.provider.clone(),
                        model: configured.model.clone(),
                        version: configured.version.clone(),
                        base_url: Some(configured.base_url.clone()),
                        api_key_configured: configured.api_key_env.is_some(),
                        connection_managed: true,
                        overridden: false,
                    })
                } else {
                    let identity = match slot {
                        "embedding" => self.models.embedding.identity(),
                        "ranking" => self.models.ranking.identity(),
                        "text_parse" => self.models.summary.identity(),
                        _ => unreachable!(),
                    };
                    Ok(ModelSettingView {
                        slot: slot.into(),
                        mode: "local".into(),
                        provider: identity.provider,
                        model: identity.model,
                        version: identity.version,
                        base_url: None,
                        api_key_configured: false,
                        connection_managed: false,
                        overridden: false,
                    })
                }
            })
            .collect()
    }

    pub fn put_model(
        &self,
        actor_user_id: &str,
        actor_session_id: &str,
        slot: &str,
        input: &ModelSettingInput,
        api_key: Option<&str>,
    ) -> Result<ModelSettingView> {
        validate_slot(slot)?;
        let configured = self.configured_model(slot);
        let connection_managed = configured.is_some();
        if connection_managed
            && (api_key.is_some_and(|value| !value.is_empty()) || input.clear_api_key)
        {
            bail!("deployment-managed model connection cannot change API keys");
        }
        let stored = StoredModelConfig::from_input(input, configured)?;
        let settings = stored.as_http_settings(configured)?;
        let previous_secret: Option<String> = self.pool.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT credential_secret_id FROM model_providers
                     WHERE owner_user_id IS NULL AND slot=?1",
                    [slot],
                    |row| row.get(0),
                )
                .optional()
                .map(|value| value.flatten())
        })?;
        let retained_key = if connection_managed {
            None
        } else if api_key.is_none() && !input.clear_api_key {
            self.read_secret(previous_secret.as_deref())?
        } else {
            api_key.map(str::to_owned)
        };
        self.validate_model(slot, &settings, retained_key.clone())?;

        let new_secret = if connection_managed {
            None
        } else if let Some(value) = api_key.filter(|value| !value.is_empty()) {
            let store = self
                .secrets
                .as_ref()
                .context("master key is required to save model API keys")?;
            Some(store.put(None, &format!("model-{slot}-api-key"), value.as_bytes())?)
        } else if input.clear_api_key {
            None
        } else {
            previous_secret.clone()
        };
        let id = format!("global:{slot}");
        let config_json = serde_json::to_string(&stored)?;
        let transaction_result = self.pool.with_connection(|connection| {
            let transaction = connection.unchecked_transaction()?;
            transaction.execute(
                "INSERT INTO model_providers(
                   id, owner_user_id, provider_kind, name, config_json,
                   credential_secret_id, enabled, slot, updated_at
                 ) VALUES (?1, NULL, 'http', ?2, ?3, ?4, 1, ?2, unixepoch())
                 ON CONFLICT(id) DO UPDATE SET provider_kind='http', name=excluded.name,
                   config_json=excluded.config_json,
                   credential_secret_id=excluded.credential_secret_id,
                   enabled=1, slot=excluded.slot, updated_at=unixepoch()",
                params![id, slot, config_json, new_secret],
            )?;
            transaction.execute(
                "INSERT INTO audit_events(
                   id, user_id, actor_session_id, event_type, target_type, target_id, detail_json
                 ) VALUES (?1, ?2, ?3, 'settings.model_updated', 'model_provider', ?4,
                   json_object('slot', ?4, 'configured', 1))",
                params![
                    Uuid::new_v4().to_string(),
                    actor_user_id,
                    actor_session_id,
                    slot
                ],
            )?;
            transaction.commit()
        });
        if let Err(error) = transaction_result {
            if new_secret != previous_secret
                && let (Some(store), Some(secret_id)) = (&self.secrets, &new_secret)
            {
                let _ = store.delete(secret_id);
            }
            return Err(error.into());
        }
        if new_secret != previous_secret
            && let (Some(store), Some(secret_id)) = (&self.secrets, &previous_secret)
        {
            let _ = store.delete(secret_id);
        }
        self.apply_model(slot, Some(&settings), retained_key)?;
        if slot == "ranking" {
            self.invalidate_rankings()?;
        }
        self.list_models()?
            .into_iter()
            .find(|view| view.slot == slot)
            .context("saved model setting is unavailable")
    }

    pub fn delete_model(
        &self,
        actor_user_id: &str,
        actor_session_id: &str,
        slot: &str,
    ) -> Result<()> {
        validate_slot(slot)?;
        let secret_id: Option<String> = self.pool.with_connection(|connection| {
            let transaction = connection.unchecked_transaction()?;
            let secret_id = transaction
                .query_row(
                    "SELECT credential_secret_id FROM model_providers
                     WHERE owner_user_id IS NULL AND slot=?1",
                    [slot],
                    |row| row.get(0),
                )
                .optional()?
                .flatten();
            transaction.execute(
                "DELETE FROM model_providers WHERE owner_user_id IS NULL AND slot=?1",
                [slot],
            )?;
            transaction.execute(
                "INSERT INTO audit_events(
                   id, user_id, actor_session_id, event_type, target_type, target_id, detail_json
                 ) VALUES (?1, ?2, ?3, 'settings.model_reset', 'model_provider', ?4,
                   json_object('slot', ?4, 'configured', 0))",
                params![
                    Uuid::new_v4().to_string(),
                    actor_user_id,
                    actor_session_id,
                    slot
                ],
            )?;
            transaction.commit()?;
            Ok(secret_id)
        })?;
        if let (Some(store), Some(secret_id)) = (&self.secrets, secret_id) {
            let _ = store.delete(&secret_id);
        }
        self.apply_model(slot, self.configured_model(slot), None)?;
        if slot == "ranking" {
            self.invalidate_rankings()?;
        }
        Ok(())
    }

    pub fn test_model(
        &self,
        slot: &str,
        input: &ModelSettingInput,
        api_key: Option<&str>,
    ) -> Result<RuntimeModelRegistry> {
        validate_slot(slot)?;
        let configured = self.configured_model(slot);
        if configured.is_some()
            && (api_key.is_some_and(|value| !value.is_empty()) || input.clear_api_key)
        {
            bail!("deployment-managed model connection cannot change API keys");
        }
        let settings =
            StoredModelConfig::from_input(input, configured)?.as_http_settings(configured)?;
        let retained_key = if configured.is_some() || input.clear_api_key {
            None
        } else if api_key.is_some_and(|value| !value.is_empty()) {
            api_key.map(str::to_owned)
        } else {
            match self.load_model(slot)? {
                Some((_, secret_id)) => self.read_secret(secret_id.as_deref())?,
                None => None,
            }
        };
        let scratch = RuntimeModelRegistry::from_settings(&rill_config::Settings::default())?;
        match slot {
            "embedding" => scratch.set_embedding(Some(&settings), retained_key)?,
            "ranking" => scratch.set_ranking(Some(&settings), retained_key)?,
            "text_parse" => scratch.set_text_parse(Some(&settings), retained_key)?,
            _ => unreachable!(),
        }
        Ok(scratch)
    }

    fn load_model(&self, slot: &str) -> Result<Option<(StoredModelConfig, Option<String>)>> {
        let raw: Option<(String, Option<String>)> = self.pool.with_connection(|connection| {
            connection
                .query_row(
                    "SELECT config_json, credential_secret_id FROM model_providers
                     WHERE owner_user_id IS NULL AND slot=?1 AND enabled=1",
                    [slot],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .optional()
        })?;
        raw.map(|(config, secret)| Ok((StoredModelConfig::from_json(&config)?, secret)))
            .transpose()
    }

    fn configured_model(&self, slot: &str) -> Option<&HttpModelSettings> {
        match slot {
            "embedding" => self.configured_models.embedding.as_ref(),
            "ranking" => self.configured_models.recommendation.as_ref(),
            "text_parse" => self
                .configured_models
                .collection_parser
                .as_ref()
                .or(self.configured_models.summary.as_ref()),
            _ => None,
        }
    }

    fn read_secret(&self, secret_id: Option<&str>) -> Result<Option<String>> {
        let Some(secret_id) = secret_id else {
            return Ok(None);
        };
        let store = self
            .secrets
            .as_ref()
            .context("master key is required to load model API key")?;
        Ok(Some(String::from_utf8(store.get(secret_id)?)?))
    }

    fn validate_model(
        &self,
        slot: &str,
        settings: &HttpModelSettings,
        api_key: Option<String>,
    ) -> Result<()> {
        let scratch = RuntimeModelRegistry::from_settings(&rill_config::Settings::default())?;
        match slot {
            "embedding" => scratch.set_embedding(Some(settings), api_key),
            "ranking" => scratch.set_ranking(Some(settings), api_key),
            "text_parse" => scratch.set_text_parse(Some(settings), api_key),
            _ => unreachable!(),
        }
    }

    fn apply_model(
        &self,
        slot: &str,
        settings: Option<&HttpModelSettings>,
        api_key: Option<String>,
    ) -> Result<()> {
        match slot {
            "embedding" => self.models.set_embedding(settings, api_key),
            "ranking" => self.models.set_ranking(settings, api_key),
            "text_parse" => self.models.set_text_parse(settings, api_key),
            _ => unreachable!(),
        }
    }

    fn invalidate_rankings(&self) -> Result<()> {
        self.pool.with_connection(|connection| {
            connection.execute("DELETE FROM recommendation_runs", [])
        })?;
        Ok(())
    }
}

impl StoredModelConfig {
    fn from_input(
        input: &ModelSettingInput,
        configured: Option<&HttpModelSettings>,
    ) -> Result<Self> {
        validate_identity(&input.model, &input.version)?;
        if configured.is_some() {
            return Ok(Self::ModelOverride {
                model: input.model.trim().into(),
                version: input.version.trim().into(),
            });
        }
        Ok(Self::ProviderOverride {
            provider: StoredProviderConfig::from_input(input)?,
        })
    }

    fn from_json(value: &str) -> Result<Self> {
        match serde_json::from_str(value) {
            Ok(config) => Ok(config),
            Err(current_error) => match serde_json::from_str::<LegacyStoredModelConfig>(value) {
                Ok(legacy) => Ok(Self::ProviderOverride {
                    provider: legacy.into(),
                }),
                Err(_) => Err(current_error.into()),
            },
        }
    }

    fn as_http_settings(
        &self,
        configured: Option<&HttpModelSettings>,
    ) -> Result<HttpModelSettings> {
        let (model, version) = match self {
            Self::ModelOverride { model, version } => (model, version),
            Self::ProviderOverride { provider } => (&provider.model, &provider.version),
        };
        if let Some(configured) = configured {
            let mut effective = configured.clone();
            effective.model.clone_from(model);
            effective.version.clone_from(version);
            return Ok(effective);
        }
        match self {
            Self::ProviderOverride { provider } => Ok(provider.as_http_settings()),
            Self::ModelOverride { .. } => {
                bail!("model override requires a deployment-managed provider connection")
            }
        }
    }
}

impl StoredProviderConfig {
    fn from_input(input: &ModelSettingInput) -> Result<Self> {
        let base_url = input.base_url.as_deref().context("model URL is required")?;
        let provider = input
            .provider
            .as_deref()
            .context("model provider is required")?;
        let url = Url::parse(base_url.trim()).context("model URL is invalid")?;
        if !matches!(url.scheme(), "http" | "https")
            || url.host_str().is_none()
            || !url.username().is_empty()
            || url.password().is_some()
            || url.query().is_some()
            || url.fragment().is_some()
        {
            bail!("model URL must be absolute HTTP(S) without credentials, query, or fragment");
        }
        if provider.trim().is_empty() || provider.trim().len() > 160 {
            bail!("model provider is invalid");
        }
        Ok(Self {
            base_url: url.to_string(),
            provider: provider.trim().into(),
            model: input.model.trim().into(),
            version: input.version.trim().into(),
            timeout_seconds: 30,
            maximum_request_bytes: 512 * 1024,
            maximum_response_bytes: 4 * 1024 * 1024,
            maximum_batch_items: 128,
            retries: 2,
            circuit_failure_threshold: 3,
            circuit_cooldown_seconds: 30,
        })
    }

    fn as_http_settings(&self) -> HttpModelSettings {
        HttpModelSettings {
            base_url: self.base_url.clone(),
            provider: self.provider.clone(),
            model: self.model.clone(),
            version: self.version.clone(),
            api_key_env: None,
            ca_certificate_path: None,
            timeout_seconds: self.timeout_seconds,
            maximum_request_bytes: self.maximum_request_bytes,
            maximum_response_bytes: self.maximum_response_bytes,
            maximum_batch_items: self.maximum_batch_items,
            retries: self.retries,
            circuit_failure_threshold: self.circuit_failure_threshold,
            circuit_cooldown_seconds: self.circuit_cooldown_seconds,
        }
    }
}

impl From<LegacyStoredModelConfig> for StoredProviderConfig {
    fn from(value: LegacyStoredModelConfig) -> Self {
        Self {
            base_url: value.base_url,
            provider: value.provider,
            model: value.model,
            version: value.version,
            timeout_seconds: value.timeout_seconds,
            maximum_request_bytes: value.maximum_request_bytes,
            maximum_response_bytes: value.maximum_response_bytes,
            maximum_batch_items: value.maximum_batch_items,
            retries: value.retries,
            circuit_failure_threshold: value.circuit_failure_threshold,
            circuit_cooldown_seconds: value.circuit_cooldown_seconds,
        }
    }
}

fn validate_identity(model: &str, version: &str) -> Result<()> {
    for (name, value) in [("model", model.trim()), ("version", version.trim())] {
        if value.is_empty() || value.len() > 160 {
            bail!("model {name} is invalid");
        }
    }
    Ok(())
}

fn validate_slot(slot: &str) -> Result<()> {
    if MODEL_SLOTS.contains(&slot) {
        Ok(())
    } else {
        bail!("unknown model slot")
    }
}

fn default_version() -> String {
    "configured".into()
}

#[cfg(test)]
mod tests {
    use base64::{Engine as _, engine::general_purpose::URL_SAFE_NO_PAD};

    use super::*;

    #[test]
    fn ranking_setting_is_encrypted_redacted_live_and_invalidates_cache() {
        let pool = DbPool::open_in_memory().unwrap();
        pool.with_connection(|connection| {
            connection.execute(
                "INSERT INTO users(id, username, role) VALUES ('admin', 'admin', 'admin')",
                [],
            )?;
            connection.execute(
                "INSERT INTO recommendation_runs(id, user_id, provider, model)
                 VALUES ('run', 'admin', 'local', 'local')",
                [],
            )
        })
        .unwrap();
        let key = URL_SAFE_NO_PAD.encode([7_u8; 32]);
        let secrets = SecretStore::from_base64(pool.clone(), &key, 1).unwrap();
        let models =
            RuntimeModelRegistry::from_settings(&rill_config::Settings::default()).unwrap();
        let service = GlobalSettingsService::new(
            pool.clone(),
            Some(secrets),
            models.clone(),
            ModelSettings::default(),
        );
        let input = ModelSettingInput {
            base_url: Some("https://models.example/rill/".into()),
            provider: Some("example".into()),
            model: "rank-v1".into(),
            version: "2026-08".into(),
            api_key: None,
            clear_api_key: false,
        };
        let view = service
            .put_model("admin", "session", "ranking", &input, Some("super-secret"))
            .unwrap();
        assert!(view.api_key_configured);
        assert_eq!(models.ranking.identity().provider, "example");
        let (ciphertext, run_count): (Vec<u8>, i64) = pool
            .with_connection(|connection| {
                Ok((
                    connection.query_row(
                        "SELECT ciphertext FROM encrypted_secrets
                         WHERE purpose='model-ranking-api-key'",
                        [],
                        |row| row.get(0),
                    )?,
                    connection.query_row(
                        "SELECT count(*) FROM recommendation_runs",
                        [],
                        |row| row.get(0),
                    )?,
                ))
            })
            .unwrap();
        assert!(
            !ciphertext
                .windows(12)
                .any(|window| window == b"super-secret")
        );
        assert_eq!(run_count, 0);
        let json = serde_json::to_string(&service.list_models().unwrap()).unwrap();
        assert!(!json.contains("super-secret"));

        service.delete_model("admin", "session", "ranking").unwrap();
        assert_eq!(models.ranking.identity().provider, "rill-local");
    }

    #[test]
    fn managed_model_override_preserves_connection_and_reset_restores_configured_model() {
        let pool = DbPool::open_in_memory().unwrap();
        pool.with_connection(|connection| {
            connection.execute(
                "INSERT INTO users(id, username, role) VALUES ('admin', 'admin', 'admin')",
                [],
            )
        })
        .unwrap();
        let configured = HttpModelSettings {
            base_url: "https://models.example/v1/".into(),
            provider: "openai-compatible".into(),
            model: "fleet-model".into(),
            version: "fleet-pinned".into(),
            ca_certificate_path: Some("/etc/rill/private-model-ca.crt".into()),
            timeout_seconds: 47,
            ..HttpModelSettings::default()
        };
        let input = ModelSettingInput {
            base_url: None,
            provider: None,
            model: "ui-model".into(),
            version: "ui-selected".into(),
            api_key: None,
            clear_api_key: false,
        };
        let stored = StoredModelConfig::from_input(&input, Some(&configured)).unwrap();
        let effective = stored.as_http_settings(Some(&configured)).unwrap();
        assert_eq!(effective.base_url, configured.base_url);
        assert_eq!(effective.provider, configured.provider);
        assert_eq!(
            effective.ca_certificate_path,
            configured.ca_certificate_path
        );
        assert_eq!(effective.timeout_seconds, 47);
        assert_eq!(effective.model, "ui-model");
        let json = serde_json::to_string(&stored).unwrap();
        assert!(!json.contains("baseUrl"));
        assert!(!json.contains("private-model-ca"));

        let legacy = serde_json::json!({
            "baseUrl": "https://stale.example/v1/",
            "provider": "stale-provider",
            "model": "legacy-ui-model",
            "version": "legacy-ui-selected",
            "timeoutSeconds": 30,
            "maximumRequestBytes": 524288,
            "maximumResponseBytes": 4194304,
            "maximumBatchItems": 128,
            "retries": 2,
            "circuitFailureThreshold": 3,
            "circuitCooldownSeconds": 30
        })
        .to_string();
        let legacy_effective = StoredModelConfig::from_json(&legacy)
            .unwrap()
            .as_http_settings(Some(&configured))
            .unwrap();
        assert_eq!(legacy_effective.base_url, configured.base_url);
        assert_eq!(legacy_effective.provider, configured.provider);
        assert_eq!(
            legacy_effective.ca_certificate_path,
            configured.ca_certificate_path
        );
        assert_eq!(legacy_effective.model, "legacy-ui-model");

        let mut runtime_settings = rill_config::Settings::default();
        runtime_settings.models.recommendation = Some(HttpModelSettings {
            ca_certificate_path: None,
            ..configured.clone()
        });
        let models = RuntimeModelRegistry::from_settings(&runtime_settings).unwrap();
        let service =
            GlobalSettingsService::new(pool, None, models.clone(), runtime_settings.models.clone());
        let view = service
            .put_model("admin", "session", "ranking", &input, None)
            .unwrap();
        assert!(view.connection_managed);
        assert!(view.overridden);
        assert_eq!(models.ranking.identity().model, "ui-model");

        service.delete_model("admin", "session", "ranking").unwrap();
        assert_eq!(models.ranking.identity().model, "fleet-model");
    }
}
