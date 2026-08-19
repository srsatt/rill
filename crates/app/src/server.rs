use std::{
    collections::BTreeMap,
    env, fs,
    net::{IpAddr, SocketAddr},
    path::Path as FilePath,
    sync::Arc,
    time::Duration,
    time::Instant,
};

use anyhow::{Context, Result};
use axum::{
    Json, Router,
    body::{Body, Bytes},
    extract::{ConnectInfo, DefaultBodyLimit, Form, Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    middleware,
    response::{IntoResponse, Redirect, Response},
    routing::{delete, get, post, put},
};
use chrono::{DateTime, Utc};
use rill_actions::{ActionError, ActionService, CreateHttpAction};
use rill_auth::{AuthError, AuthService, Principal, ReaderDevice, SessionKind, new_secret};
use rill_config::Settings;
use rill_contracts::{
    AdminPageModel, CuratorPathModel, FeedPageModel, LibraryPageModel, LoginPageModel,
    RENDER_PROTOCOL_VERSION, ReaderDeviceModel, ReaderPairPageModel, ReaderPreferencesPageModel,
    ReaderSettingsPageModel, RenderMode, RenderRequest, RenderResponse, SourcesPageModel,
    StoryCardModel, StoryPageModel, StoryVariantModel, StreamLink,
};
use rill_db::DbPool;
use rill_domain::Role;
use rill_extraction::ArticleExtractor;
use rill_ingestion::{DetectionMode, IngestionError, IngestionService, ParentDisplayPolicy};
use rill_intelligence::{
    FeedbackKind, IntelligenceError, IntelligenceService, RankedStory, StoryDetailView,
    StoryVariantView, StreamFilter,
};
use rill_jobs::JobQueue;
use rill_model_api::{CollectionParserProvider, EmbeddingProvider, SummaryProvider};
use rill_plugin_host::{
    PluginError, PluginLimits, PluginPermission, PluginService, PluginSourceConfig,
};
use rill_renderer_host::{Renderer, RendererLimits, load_renderer};
use rill_secrets::SecretStore;
use rill_source_api::{ConditionalHeaders, ConnectorContext, SourceConnector};
use rill_source_email::{EmailGateway, ImapEmailGateway};
use rill_source_rss::{
    OpmlFeed, RssConnector, discover_feed, export_opml, import_opml, parse_feed,
};
use serde::{Deserialize, Serialize};
use subtle::ConstantTimeEq;
use tower_http::{compression::CompressionLayer, services::ServeDir};
use tracing::{Instrument as _, error, info, warn};
use url::Url;
use uuid::Uuid;
use zeroize::Zeroizing;

use crate::{
    global_settings::GlobalSettingsService,
    metrics::Metrics,
    model_runtime::RuntimeModelRegistry,
    rate_limit::AttemptLimiter,
    telegram_integration::TelegramIntegration,
    worker::{IngestionJobHandler, build_workers, run_workers},
};

const MAX_FORM_BYTES: usize = 64 * 1024;

#[derive(Clone)]
pub(crate) struct AppState {
    pub(crate) pool: DbPool,
    renderer: Arc<dyn Renderer>,
    assets: BrowserAssets,
    pub(crate) auth: AuthService,
    ingestion: IngestionService,
    intelligence: IntelligenceService,
    actions: ActionService,
    plugins: PluginService,
    connector_context: ConnectorContext,
    secrets: Option<SecretStore>,
    pub(crate) telegram: TelegramIntegration,
    public_origin: String,
    secure_cookies: bool,
    login_limiter: AttemptLimiter,
    pairing_generation_limiter: AttemptLimiter,
    pub(crate) admin_limiter: AttemptLimiter,
    pub(crate) global_settings: GlobalSettingsService,
    metrics: Metrics,
    instance_id: String,
    dev_reload: bool,
}

#[derive(Clone)]
struct BrowserAssets {
    modern_entry: String,
    css: Vec<String>,
}

#[derive(Debug, Deserialize)]
struct ManifestEntry {
    file: String,
    #[serde(default)]
    css: Vec<String>,
}

include!("server/http_types.rs");
pub async fn serve(settings: Settings, pool: DbPool) -> Result<()> {
    let metrics = Metrics::new(settings.renderer.memory_bytes);
    let metrics_enabled = settings.metrics.enabled;
    let metrics_path = settings.metrics.path.clone();
    let renderer = load_renderer(
        &settings.assets.renderer_wasm,
        RendererLimits {
            fuel: settings.renderer.fuel,
            memory_bytes: settings.renderer.memory_bytes,
            input_bytes: 512 * 1024,
            output_bytes: settings.renderer.max_response_bytes,
            timeout: Duration::from_millis(settings.renderer.timeout_ms),
        },
    )?;
    let assets = load_assets(&settings.assets.static_dir)?;
    let public_origin = origin(&Url::parse(&settings.http.public_base_url)?)?;
    let auth = AuthService::new(
        pool.clone(),
        settings.auth.session_days,
        settings.auth.reader_session_days,
        settings.auth.pairing_minutes,
        settings.auth.pairing_max_attempts,
    );
    let parent_display_policy = match settings
        .ingestion
        .collection_parent_display_default
        .as_str()
    {
        "children_only" => ParentDisplayPolicy::ChildrenOnly,
        "parent_and_children" => ParentDisplayPolicy::ParentAndChildren,
        "parent_only" => ParentDisplayPolicy::ParentOnly,
        _ => unreachable!("configuration validation rejects unknown display policies"),
    };
    let models = RuntimeModelRegistry::from_settings(&settings)?;
    let collection_parser: Option<Arc<dyn CollectionParserProvider>> =
        Some(models.collection_parser.clone());
    let ingestion =
        IngestionService::new(pool.clone(), settings.ingestion.maximum_collection_fan_out)
            .configure_collection_policy(
                settings.ingestion.collection_detection_threshold,
                parent_display_policy,
                settings.ingestion.collection_excluded_hosts.clone(),
                settings
                    .ingestion
                    .collection_excluded_path_fragments
                    .clone(),
            )
            .configure_collection_parser(collection_parser);
    let embedding: Arc<dyn EmbeddingProvider> = models.embedding.clone();
    let summary: Arc<dyn SummaryProvider> = models.summary.clone();
    let intelligence = IntelligenceService::new(pool.clone(), embedding, summary, None)
        .configure_preference_model(
            settings.recommendations.refit_batch_size,
            settings.recommendations.fit_window,
        );
    let secrets = env::var(&settings.secrets.master_key_env)
        .ok()
        .map(|key| SecretStore::from_base64(pool.clone(), &key, settings.secrets.key_version))
        .transpose()?;
    let global_settings = GlobalSettingsService::new(pool.clone(), secrets.clone(), models.clone());
    global_settings.apply_persisted_models()?;
    let email_gateway: Option<Arc<dyn EmailGateway>> = secrets.clone().map(|secrets| {
        Arc::new(ImapEmailGateway::new(
            secrets,
            Duration::from_secs(settings.email.timeout_seconds),
            settings.email.maximum_message_bytes,
        )) as Arc<dyn EmailGateway>
    });
    let telegram = TelegramIntegration::new(pool.clone(), secrets.clone(), ingestion.clone());
    if let Err(error) = telegram.start_persisted().await {
        warn!(error = %error, "configured Telegram bot could not start");
    }
    let connector_context = crate::connector_context(&settings)?;
    let plugins = PluginService::new(
        pool.clone(),
        secrets.clone(),
        connector_context.http.clone(),
        PluginLimits {
            memory_bytes: settings.plugins.memory_bytes,
            fuel: settings.plugins.fuel,
            timeout: Duration::from_millis(settings.plugins.timeout_ms),
            maximum_output_bytes: settings.plugins.maximum_output_bytes,
            maximum_component_bytes: settings.plugins.maximum_component_bytes,
            maximum_http_bytes: settings.plugins.maximum_http_bytes,
        },
    )?;
    let actions = ActionService::new(
        pool.clone(),
        secrets.clone(),
        settings.fetch.allow_private_networks,
    );
    let extractor = ArticleExtractor::new(connector_context.http.as_ref().clone());
    let job_queue = JobQueue::new(pool.clone());
    crate::worker::schedule_initial_maintenance(&job_queue)?;
    let workers = build_workers(
        job_queue.clone(),
        IngestionJobHandler::new(crate::worker::IngestionJobHandlerDependencies {
            ingestion: ingestion.clone(),
            extractor,
            connector_context: connector_context.clone(),
            poll_item_limit: settings.ingestion.poll_item_limit,
            intelligence: intelligence.clone(),
            email: email_gateway,
            actions: actions.clone(),
            plugins: plugins.clone(),
            metrics: metrics.clone(),
            maintenance: crate::maintenance::MaintenanceService::new(pool.clone()),
            jobs: job_queue,
        }),
        settings.jobs.concurrency,
    );
    let worker_task = tokio::spawn(run_workers(
        workers,
        Duration::from_millis(settings.jobs.idle_poll_ms),
    ));
    let state = AppState {
        pool,
        renderer,
        assets,
        auth,
        ingestion,
        intelligence,
        actions,
        plugins,
        connector_context,
        secrets,
        telegram,
        public_origin,
        secure_cookies: settings.http.secure_cookies,
        login_limiter: AttemptLimiter::default(),
        pairing_generation_limiter: AttemptLimiter::default(),
        admin_limiter: AttemptLimiter::default(),
        global_settings,
        metrics,
        instance_id: Uuid::new_v4().to_string(),
        dev_reload: env::var_os("RILL_DEV_RELOAD").is_some_and(|value| value == "1"),
    };

    let mut app = Router::new()
        .route("/", get(modern_feed))
        .route("/stream/home", get(modern_feed))
        .route("/stream/{slug}", get(modern_stream))
        .route("/story/{story_id}", get(modern_story))
        .route("/search", get(modern_search))
        .route("/favorites", get(modern_favorites))
        .route("/history", get(modern_history))
        .route("/sources", get(modern_sources))
        .route("/login", get(login_page).post(login_form))
        .route("/settings/readers", get(reader_settings))
        .route("/settings/password", post(settings_change_password))
        .route("/admin", get(modern_admin))
        .route("/settings/readers/pair", post(settings_pair_reader))
        .route(
            "/settings/readers/{device_id}/revoke",
            post(settings_revoke_reader),
        )
        .route("/reader", get(reader_feed))
        .route("/reader/stream/{slug}", get(reader_stream))
        .route("/reader/page/{page}", get(reader_page))
        .route("/reader/story/{story_id}", get(reader_story))
        .route(
            "/reader/story/{story_id}/feedback",
            post(reader_story_feedback),
        )
        .route("/reader/story/{story_id}/read", post(reader_story_read))
        .route(
            "/reader/story/{story_id}/variant",
            post(reader_story_variant),
        )
        .route("/reader/settings", get(reader_preferences))
        .route("/reader/logout", post(reader_logout))
        .route("/reader/pair", get(reader_pair_page).post(reader_pair_form))
        .route("/api/v1/auth/login", post(api_login))
        .route("/api/v1/auth/logout", post(api_logout))
        .route("/api/v1/auth/me", get(api_me))
        .route(
            "/api/v1/auth/password",
            post(crate::admin::api_change_password),
        )
        .route(
            "/api/v1/admin/users",
            get(crate::admin::api_users).post(crate::admin::api_create_user),
        )
        .route(
            "/api/v1/admin/users/{user_id}/disabled",
            post(crate::admin::api_user_disabled),
        )
        .route(
            "/api/v1/admin/users/{user_id}/role",
            post(crate::admin::api_user_role),
        )
        .route("/api/v1/admin/sessions", get(crate::admin::api_sessions))
        .route(
            "/api/v1/admin/sessions/{session_id}",
            delete(crate::admin::api_revoke_session),
        )
        .route("/api/v1/admin/audit", get(crate::admin::api_audit))
        .route("/api/v1/admin/jobs", get(crate::admin::api_jobs))
        .route("/api/v1/admin/models", get(crate::admin::api_models))
        .route(
            "/api/v1/admin/settings/models/{slot}",
            put(crate::admin::api_put_model).delete(crate::admin::api_delete_model),
        )
        .route(
            "/api/v1/admin/settings/telegram-bot",
            get(crate::admin::api_telegram_bot)
                .put(crate::admin::api_put_telegram_bot)
                .delete(crate::admin::api_delete_telegram_bot),
        )
        .route(
            "/api/v1/admin/jobs/{job_id}/retry",
            post(crate::admin::api_retry_job),
        )
        .route(
            "/api/v1/admin/jobs/{job_id}/cancel",
            post(crate::admin::api_cancel_job),
        )
        .route("/api/v1/feed", get(api_feed))
        .route("/api/v1/search", get(api_search))
        .route("/api/v1/sources", get(api_sources))
        .route("/api/v1/sources/quick-add", post(api_quick_add_source))
        .route("/api/v1/sources/rss", post(api_create_rss_source))
        .route(
            "/api/v1/sources/rss/opml",
            get(api_export_opml).post(api_import_opml),
        )
        .route("/api/v1/sources/email", post(api_create_email_source))
        .route("/api/v1/sources/telegram", post(api_create_telegram_source))
        .route(
            "/api/v1/telegram/binding",
            get(api_telegram_binding)
                .post(api_telegram_binding_challenge)
                .delete(api_telegram_unbind),
        )
        .route("/api/v1/sources/{source_id}/poll", post(api_poll_source))
        .route("/api/v1/sources/{source_id}", delete(api_remove_source))
        .route(
            "/api/v1/sources/{source_id}/enabled",
            post(api_source_enabled),
        )
        .route(
            "/api/v1/collections/{raw_item_id}",
            get(api_collection_debug),
        )
        .route(
            "/api/v1/collections/{raw_item_id}/detect",
            post(api_collection_control),
        )
        .route("/api/v1/streams", get(api_streams).post(api_create_stream))
        .route("/api/v1/streams/reorder", post(api_reorder_streams))
        .route(
            "/api/v1/streams/{slug}",
            post(api_update_stream).delete(api_delete_stream),
        )
        .route("/api/v1/streams/{slug}/feed", get(api_stream_feed))
        .route("/api/v1/actions", get(api_actions).post(api_create_action))
        .route("/api/v1/actions/{action_id}", delete(api_remove_action))
        .route(
            "/api/v1/actions/{action_id}/enabled",
            post(api_set_action_enabled),
        )
        .route("/api/v1/plugins", get(api_plugins))
        .route(
            "/api/v1/plugins/install",
            post(api_install_plugin).layer(DefaultBodyLimit::max(
                settings.plugins.maximum_component_bytes,
            )),
        )
        .route(
            "/api/v1/plugins/{plugin_id}",
            get(api_plugin).delete(api_remove_plugin),
        )
        .route(
            "/api/v1/plugins/{plugin_id}/enabled",
            post(api_set_plugin_enabled),
        )
        .route(
            "/api/v1/plugins/{plugin_id}/permissions",
            post(api_grant_plugin_permission),
        )
        .route(
            "/api/v1/plugins/{plugin_id}/sources",
            post(api_create_plugin_source),
        )
        .route(
            "/api/v1/stories/{story_id}/feedback",
            post(api_story_feedback),
        )
        .route("/api/v1/stories/{story_id}", get(api_story))
        .route(
            "/api/v1/stories/{story_id}/read-state",
            post(api_story_read_state),
        )
        .route(
            "/api/v1/stories/{story_id}/representative",
            post(api_story_representative),
        )
        .route(
            "/api/v1/reader/pairing-codes",
            post(api_create_pairing_code),
        )
        .route("/api/v1/reader/devices", get(api_reader_devices))
        .route(
            "/api/v1/reader/devices/{device_id}",
            delete(api_revoke_reader_device),
        )
        .route("/health/live", get(health_live))
        .route("/health/ready", get(health_ready));
    if metrics_enabled {
        app = app.route(&metrics_path, get(metrics_endpoint));
    }
    let app = app
        .nest_service("/static", ServeDir::new(settings.assets.static_dir))
        .layer(DefaultBodyLimit::max(MAX_FORM_BYTES))
        .layer(CompressionLayer::new())
        .layer(middleware::map_response(security_headers))
        .layer(middleware::from_fn(request_correlation))
        .with_state(state);

    let address: SocketAddr = settings.http.bind.parse()?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    info!(%address, "Rill listening");
    let result = axum::serve(
        listener,
        app.into_make_service_with_connect_info::<SocketAddr>(),
    )
    .with_graceful_shutdown(shutdown())
    .await;
    worker_task.abort();
    result?;
    Ok(())
}

include!("server/modern_pages.rs");
include!("server/reader_pages.rs");
include!("server/reader_settings.rs");
include!("server/core_api.rs");
include!("server/sources_api.rs");
include!("server/telegram_api.rs");
include!("server/intelligence_api.rs");
include!("server/extensions_api.rs");
include!("server/reader_api.rs");
include!("server/session_http.rs");
include!("server/view_models.rs");
include!("server/runtime.rs");
#[cfg(test)]
include!("server/tests.rs");
