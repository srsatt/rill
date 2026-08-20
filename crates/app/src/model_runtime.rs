use std::{
    collections::BTreeMap,
    env,
    sync::{Arc, RwLock},
    time::Duration,
};

use anyhow::{Context, Result};
use async_trait::async_trait;
use rill_config::{HttpModelSettings, Settings};
use rill_model_api::{
    CollectionParseRequest, CollectionParseResponse, CollectionParserProvider, EmbeddingInput,
    EmbeddingOutput, EmbeddingProvider, ExtractiveSummaryProvider, FeatureHashEmbeddingProvider,
    HttpProviderConfig, HttpRecommendationProvider, ModelError, ModelHealth, ModelIdentity,
    OpenAiCompatibleProvider, OpenAiCompatibleRecommendationProvider, RankRequest, RankResponse,
    RankedCandidate, RecommendationFeedbackEvent, RecommendationProvider, SummaryProvider,
    SummaryRequest, SummaryResponse,
};
use url::Url;

#[derive(Clone)]
pub(crate) struct RuntimeModelRegistry {
    pub embedding: Arc<SwitchableEmbedding>,
    pub summary: Arc<SwitchableSummary>,
    pub collection_parser: Arc<SwitchableCollectionParser>,
    pub ranking: Arc<SwitchableRecommendation>,
}

impl RuntimeModelRegistry {
    pub fn from_settings(settings: &Settings) -> Result<Self> {
        let embedding: Arc<dyn EmbeddingProvider> = match settings.models.embedding.as_ref() {
            Some(config) => Arc::new(OpenAiCompatibleProvider::new(http_model_config(
                config, None,
            )?)?),
            None => Arc::new(FeatureHashEmbeddingProvider::new(128)?),
        };
        let text_config = settings
            .models
            .collection_parser
            .as_ref()
            .or(settings.models.summary.as_ref());
        let (summary, collection_parser): (
            Arc<dyn SummaryProvider>,
            Arc<dyn CollectionParserProvider>,
        ) = match text_config {
            Some(config) => {
                let provider = Arc::new(OpenAiCompatibleProvider::new(http_model_config(
                    config, None,
                )?)?);
                (provider.clone(), provider)
            }
            None => (
                Arc::new(ExtractiveSummaryProvider),
                Arc::new(DeterministicCollectionFallback),
            ),
        };
        let ranking: Arc<dyn RecommendationProvider> = match settings.models.recommendation.as_ref()
        {
            Some(config) => recommendation_provider(config, None)?,
            None => Arc::new(LocalRankingFallback),
        };
        Ok(Self {
            embedding: Arc::new(SwitchableEmbedding::new(embedding)),
            summary: Arc::new(SwitchableSummary::new(summary)),
            collection_parser: Arc::new(SwitchableCollectionParser::new(collection_parser)),
            ranking: Arc::new(SwitchableRecommendation::new(ranking)),
        })
    }

    pub fn set_embedding(
        &self,
        settings: Option<&HttpModelSettings>,
        api_key: Option<String>,
    ) -> Result<()> {
        let provider: Arc<dyn EmbeddingProvider> = match settings {
            Some(settings) => Arc::new(OpenAiCompatibleProvider::new(http_model_config(
                settings, api_key,
            )?)?),
            None => Arc::new(FeatureHashEmbeddingProvider::new(128)?),
        };
        self.embedding.set(provider);
        Ok(())
    }

    pub fn set_ranking(
        &self,
        settings: Option<&HttpModelSettings>,
        api_key: Option<String>,
    ) -> Result<()> {
        let provider: Arc<dyn RecommendationProvider> = match settings {
            Some(settings) => recommendation_provider(settings, api_key)?,
            None => Arc::new(LocalRankingFallback),
        };
        self.ranking.set(provider);
        Ok(())
    }

    pub fn set_text_parse(
        &self,
        settings: Option<&HttpModelSettings>,
        api_key: Option<String>,
    ) -> Result<()> {
        match settings {
            Some(settings) => {
                let provider = Arc::new(OpenAiCompatibleProvider::new(http_model_config(
                    settings, api_key,
                )?)?);
                self.summary.set(provider.clone());
                self.collection_parser.set(provider);
            }
            None => {
                self.summary.set(Arc::new(ExtractiveSummaryProvider));
                self.collection_parser
                    .set(Arc::new(DeterministicCollectionFallback));
            }
        }
        Ok(())
    }

    pub async fn health(&self, slot: &str) -> Result<ModelHealth, ModelError> {
        match slot {
            "embedding" => self.embedding.health().await,
            "ranking" => self.ranking.health().await,
            "text_parse" => self.summary.health().await,
            _ => Err(ModelError::Unavailable("unknown model slot".into())),
        }
    }
}

fn recommendation_provider(
    settings: &HttpModelSettings,
    api_key: Option<String>,
) -> Result<Arc<dyn RecommendationProvider>> {
    let config = http_model_config(settings, api_key)?;
    if matches!(
        settings.provider.to_ascii_lowercase().as_str(),
        "ollama" | "openai" | "openai-compatible" | "claude" | "gemini"
    ) {
        Ok(Arc::new(OpenAiCompatibleRecommendationProvider::new(
            config,
        )?))
    } else {
        Ok(Arc::new(HttpRecommendationProvider::new(config)?))
    }
}

pub(crate) fn http_model_config(
    settings: &HttpModelSettings,
    api_key: Option<String>,
) -> Result<HttpProviderConfig> {
    let mut config = HttpProviderConfig::new(
        ModelIdentity {
            provider: settings.provider.clone(),
            model: settings.model.clone(),
            version: settings.version.clone(),
        },
        Url::parse(&settings.base_url)?,
    );
    config.api_key = match api_key {
        Some(value) => Some(value),
        None => settings
            .api_key_env
            .as_deref()
            .map(|name| env::var(name).with_context(|| format!("model API key {name} is missing")))
            .transpose()?,
    };
    config.timeout = Duration::from_secs(settings.timeout_seconds);
    config.maximum_request_bytes = settings.maximum_request_bytes;
    config.maximum_response_bytes = settings.maximum_response_bytes;
    config.maximum_batch_items = settings.maximum_batch_items;
    config.retries = settings.retries;
    config.circuit_failure_threshold = settings.circuit_failure_threshold;
    config.circuit_cooldown = Duration::from_secs(settings.circuit_cooldown_seconds);
    Ok(config)
}

macro_rules! switchable_provider {
    ($name:ident, $trait_name:ident) => {
        pub(crate) struct $name {
            inner: RwLock<Arc<dyn $trait_name>>,
        }

        impl $name {
            fn new(provider: Arc<dyn $trait_name>) -> Self {
                Self {
                    inner: RwLock::new(provider),
                }
            }

            fn current(&self) -> Arc<dyn $trait_name> {
                self.inner
                    .read()
                    .unwrap_or_else(|poisoned| poisoned.into_inner())
                    .clone()
            }

            fn set(&self, provider: Arc<dyn $trait_name>) {
                *self
                    .inner
                    .write()
                    .unwrap_or_else(|poisoned| poisoned.into_inner()) = provider;
            }
        }
    };
}

switchable_provider!(SwitchableEmbedding, EmbeddingProvider);
switchable_provider!(SwitchableSummary, SummaryProvider);
switchable_provider!(SwitchableCollectionParser, CollectionParserProvider);
switchable_provider!(SwitchableRecommendation, RecommendationProvider);

#[async_trait]
impl EmbeddingProvider for SwitchableEmbedding {
    fn identity(&self) -> ModelIdentity {
        self.current().identity()
    }

    async fn embed(&self, input: &[EmbeddingInput]) -> Result<Vec<EmbeddingOutput>, ModelError> {
        self.current().embed(input).await
    }

    async fn health(&self) -> Result<ModelHealth, ModelError> {
        self.current().health().await
    }
}

#[async_trait]
impl SummaryProvider for SwitchableSummary {
    fn identity(&self) -> ModelIdentity {
        self.current().identity()
    }

    async fn summarize(&self, request: SummaryRequest) -> Result<SummaryResponse, ModelError> {
        self.current().summarize(request).await
    }

    async fn health(&self) -> Result<ModelHealth, ModelError> {
        self.current().health().await
    }
}

#[async_trait]
impl CollectionParserProvider for SwitchableCollectionParser {
    fn identity(&self) -> ModelIdentity {
        self.current().identity()
    }

    async fn parse_collection(
        &self,
        request: CollectionParseRequest,
    ) -> Result<CollectionParseResponse, ModelError> {
        self.current().parse_collection(request).await
    }

    async fn health(&self) -> Result<ModelHealth, ModelError> {
        self.current().health().await
    }
}

#[async_trait]
impl RecommendationProvider for SwitchableRecommendation {
    fn identity(&self) -> ModelIdentity {
        self.current().identity()
    }

    async fn rank(&self, request: RankRequest) -> Result<RankResponse, ModelError> {
        self.current().rank(request).await
    }

    async fn submit_feedback(
        &self,
        events: &[RecommendationFeedbackEvent],
    ) -> Result<(), ModelError> {
        self.current().submit_feedback(events).await
    }

    async fn health(&self) -> Result<ModelHealth, ModelError> {
        self.current().health().await
    }
}

struct LocalRankingFallback;

#[async_trait]
impl RecommendationProvider for LocalRankingFallback {
    fn identity(&self) -> ModelIdentity {
        ModelIdentity {
            provider: "rill-local".into(),
            model: "fallback-ranker".into(),
            version: "1".into(),
        }
    }

    async fn rank(&self, request: RankRequest) -> Result<RankResponse, ModelError> {
        Ok(RankResponse {
            request_id: "local".into(),
            ranked: request
                .candidates
                .into_iter()
                .map(|candidate| RankedCandidate {
                    story_id: candidate.story_id,
                    score: candidate.local_score,
                    features: BTreeMap::from([("localFallback".into(), 1.0)]),
                })
                .collect(),
        })
    }

    async fn submit_feedback(
        &self,
        _events: &[RecommendationFeedbackEvent],
    ) -> Result<(), ModelError> {
        Ok(())
    }

    async fn health(&self) -> Result<ModelHealth, ModelError> {
        Ok(ModelHealth {
            ready: true,
            detail: "deterministic local ranking ready".into(),
        })
    }
}

struct DeterministicCollectionFallback;

#[async_trait]
impl CollectionParserProvider for DeterministicCollectionFallback {
    fn identity(&self) -> ModelIdentity {
        ModelIdentity {
            provider: "rill-local".into(),
            model: "deterministic-collection-parser".into(),
            version: "1".into(),
        }
    }

    async fn parse_collection(
        &self,
        _request: CollectionParseRequest,
    ) -> Result<CollectionParseResponse, ModelError> {
        Ok(CollectionParseResponse {
            is_collection: false,
            confidence: 0.0,
            entries: Vec::new(),
        })
    }

    async fn health(&self) -> Result<ModelHealth, ModelError> {
        Ok(ModelHealth {
            ready: true,
            detail: "deterministic parser ready".into(),
        })
    }
}
