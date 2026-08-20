mod admin;
mod global_settings;
mod maintenance;
mod metrics;
mod model_runtime;
mod rate_limit;
mod server;
mod telegram_integration;
mod worker;

use std::{
    env, fs,
    io::{self, Read},
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, Result, bail};
use clap::{Parser, Subcommand, ValueEnum};
use rill_auth::AuthService;
use rill_config::Settings;
use rill_db::DbPool;
use rill_domain::Role;
use rill_ingestion::IngestionService;
use rill_plugin_host::{PluginLimits, PluginService};
use rill_renderer_host::{RendererLimits, load_renderer};
use rill_source_api::{BoundedHttpClient, ConnectorContext, FetchPolicy, SourceConnector};
use rill_source_rss::RssConnector;
use rill_source_telegram::TelegramConnector;
use serde_json::json;
use tracing_subscriber::EnvFilter;
use zeroize::Zeroizing;

#[derive(Debug, Parser)]
#[command(
    name = "rill",
    version,
    about = "Resource-efficient personal news reader"
)]
struct Cli {
    #[arg(long, global = true, value_name = "FILE")]
    config: Option<PathBuf>,
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    Serve,
    Migrate,
    DevSeed {
        #[arg(long, default_value = "admin")]
        user: String,
    },
    Doctor,
    Backup {
        #[arg(value_name = "FILE")]
        output: PathBuf,
    },
    Plugins {
        #[command(subcommand)]
        command: PluginCommand,
    },
    Admin {
        #[command(subcommand)]
        command: AdminCommand,
    },
    Sessions {
        #[command(subcommand)]
        command: SessionCommand,
    },
    Sources {
        #[command(subcommand)]
        command: SourceCommand,
    },
    Search {
        #[arg(long, help = "User ID, username, or email")]
        user: String,
        #[arg(long, default_value_t = 20)]
        limit: usize,
        #[arg(required = true)]
        query: Vec<String>,
    },
}

#[derive(Debug, Subcommand)]
enum AdminCommand {
    Create {
        #[arg(long)]
        username: String,
        #[arg(long)]
        email: Option<String>,
        #[arg(long, value_enum, default_value_t = CliRole::Admin)]
        role: CliRole,
    },
}

#[derive(Debug, Subcommand)]
enum SessionCommand {
    Revoke {
        #[arg(long, help = "User ID, username, or email")]
        user: String,
        #[arg(long, help = "One browser session ID; omit to revoke all")]
        session: Option<String>,
    },
}

#[derive(Debug, Subcommand)]
enum SourceCommand {
    AddRss {
        #[arg(long, help = "Owner user ID, username, or email")]
        user: String,
        #[arg(long)]
        url: String,
        #[arg(long)]
        name: String,
        #[arg(long)]
        shared: bool,
        #[arg(long, default_value_t = 900)]
        poll_interval_seconds: u64,
    },
    Poll {
        #[arg(help = "Source instance ID")]
        source: String,
    },
}

#[derive(Debug, Subcommand)]
enum PluginCommand {
    Inspect {
        #[arg(value_name = "COMPONENT")]
        component: PathBuf,
    },
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum CliRole {
    Admin,
    User,
}

impl From<CliRole> for Role {
    fn from(role: CliRole) -> Self {
        match role {
            CliRole::Admin => Self::Admin,
            CliRole::User => Self::User,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let settings = Settings::load(cli.config.as_deref())?;
    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_new(&settings.logging.filter)
                .context("invalid logging.filter directive")?,
        )
        .init();

    match cli.command {
        Command::Serve => {
            let pool = open_pool(&settings)?;
            server::serve(settings, pool).await
        }
        Command::Migrate => {
            open_pool(&settings)?;
            println!("database migrations are current");
            Ok(())
        }
        Command::DevSeed { user } => dev_seed(&settings, &user),
        Command::Doctor => doctor(&settings),
        Command::Backup { output } => backup(&settings, &output),
        Command::Plugins { command } => match command {
            PluginCommand::Inspect { component } => {
                let bytes = fs::read(&component)
                    .with_context(|| format!("read plugin component {}", component.display()))?;
                let context = connector_context(&settings)?;
                let service = PluginService::new(
                    open_pool(&settings)?,
                    None,
                    context.http,
                    PluginLimits {
                        memory_bytes: settings.plugins.memory_bytes,
                        fuel: settings.plugins.fuel,
                        timeout: Duration::from_millis(settings.plugins.timeout_ms),
                        maximum_output_bytes: settings.plugins.maximum_output_bytes,
                        maximum_component_bytes: settings.plugins.maximum_component_bytes,
                        maximum_http_bytes: settings.plugins.maximum_http_bytes,
                    },
                )?;
                println!(
                    "{}",
                    serde_json::to_string_pretty(&service.inspect(&bytes).await?)?
                );
                Ok(())
            }
        },
        Command::Admin { command } => match command {
            AdminCommand::Create {
                username,
                email,
                role,
            } => {
                let password = Zeroizing::new(read_admin_password()?);
                let auth = auth_service(&settings, open_pool(&settings)?);
                let user = auth.create_user(
                    &username,
                    email.as_deref(),
                    password.trim_end(),
                    role.into(),
                )?;
                println!(
                    "created {} user {} ({})",
                    user.role.as_str(),
                    user.username,
                    user.id
                );
                Ok(())
            }
        },
        Command::Sessions { command } => match command {
            SessionCommand::Revoke { user, session } => {
                let auth = auth_service(&settings, open_pool(&settings)?);
                let user = auth.find_user(&user)?.context("user not found")?;
                match session {
                    Some(session_id) => {
                        if !auth.revoke_session(&user.id, &session_id)? {
                            bail!("active session not found");
                        }
                        println!("revoked session {session_id}");
                    }
                    None => println!(
                        "revoked {} browser session(s)",
                        auth.revoke_all_sessions(&user.id)?
                    ),
                }
                Ok(())
            }
        },
        Command::Sources { command } => {
            let pool = open_pool(&settings)?;
            let ingestion = ingestion_service(&settings, pool.clone());
            match command {
                SourceCommand::AddRss {
                    user,
                    url,
                    name,
                    shared,
                    poll_interval_seconds,
                } => {
                    if poll_interval_seconds < 60 {
                        bail!("poll interval must be at least 60 seconds");
                    }
                    let auth = auth_service(&settings, pool);
                    let owner = auth.find_user(&user)?.context("user not found")?;
                    if shared && owner.role != Role::Admin {
                        bail!("only an admin can create a shared source");
                    }
                    let config = json!({
                        "url": url,
                        "pollIntervalSeconds": poll_interval_seconds,
                        "enabled": true,
                        "shared": shared,
                    });
                    let validation = RssConnector
                        .validate(&connector_context(&settings)?, &config)
                        .await?;
                    if !validation.valid {
                        bail!("invalid RSS source: {}", validation.messages.join("; "));
                    }
                    let registration = ingestion.register_source(
                        "rss",
                        &name,
                        Some(&owner.id),
                        shared,
                        &config,
                    )?;
                    println!("created RSS source {}", registration.id);
                    Ok(())
                }
                SourceCommand::Poll { source } => {
                    let context = connector_context(&settings)?;
                    let report = match ingestion.source_kind(&source)?.as_str() {
                        "rss" => {
                            ingestion
                                .poll_source(
                                    &RssConnector,
                                    &context,
                                    &source,
                                    settings.ingestion.poll_item_limit,
                                )
                                .await?
                        }
                        "telegram" => {
                            ingestion
                                .poll_source(
                                    &TelegramConnector,
                                    &context,
                                    &source,
                                    settings.ingestion.poll_item_limit,
                                )
                                .await?
                        }
                        kind => bail!("CLI polling is unsupported for source kind {kind}"),
                    };
                    println!(
                        "polled {source}: {} raw item(s), {} document(s), {} collection child item(s)",
                        report.raw_items, report.documents_created, report.collection_children
                    );
                    Ok(())
                }
            }
        }
        Command::Search { user, limit, query } => {
            let pool = open_pool(&settings)?;
            let auth = auth_service(&settings, pool.clone());
            let user = auth.find_user(&user)?.context("user not found")?;
            let hits =
                ingestion_service(&settings, pool).search(&user.id, &query.join(" "), limit)?;
            println!("{}", serde_json::to_string_pretty(&hits)?);
            Ok(())
        }
    }
}

fn open_pool(settings: &Settings) -> Result<DbPool> {
    Ok(DbPool::open(
        &settings.database.path,
        settings.database.pool_size,
        settings.database.busy_timeout(),
    )?)
}

fn auth_service(settings: &Settings, pool: DbPool) -> AuthService {
    AuthService::new(
        pool,
        settings.auth.session_days,
        settings.auth.reader_session_days,
        settings.auth.pairing_minutes,
        settings.auth.pairing_max_attempts,
    )
}

fn ingestion_service(settings: &Settings, pool: DbPool) -> IngestionService {
    IngestionService::new(pool, settings.ingestion.maximum_collection_fan_out)
}

fn connector_context(settings: &Settings) -> Result<ConnectorContext> {
    let client = BoundedHttpClient::new(FetchPolicy {
        timeout: Duration::from_secs(settings.fetch.timeout_seconds),
        max_redirects: settings.fetch.max_redirects,
        max_response_bytes: settings.fetch.max_response_bytes,
        allow_private_networks: settings.fetch.allow_private_networks,
    })?;
    Ok(ConnectorContext {
        http: Arc::new(client),
    })
}

fn dev_seed(settings: &Settings, user: &str) -> Result<()> {
    let pool = open_pool(settings)?;
    let auth = auth_service(settings, pool.clone());
    let owner = auth.find_user(user)?.context("seed user not found")?;
    let ingestion = ingestion_service(settings, pool);
    let existing = ingestion.list_rss_feeds(&owner.id, owner.role == Role::Admin)?;
    for (name, url) in [
        ("Rill fixture", "http://127.0.0.1:3011/rss.xml"),
        ("Hacker News", "https://news.ycombinator.com/rss"),
    ] {
        if existing.iter().any(|feed| feed.xml_url == url) {
            continue;
        }
        let config = json!({
            "url": url,
            "pollIntervalSeconds": 900,
            "enabled": true,
            "shared": true,
        });
        ingestion.register_source("rss", name, Some(&owner.id), true, &config)?;
    }
    ingestion.ensure_telegram_subscription(&owner.id, "genau", None, Some("@genau"))?;
    ingestion.ensure_telegram_subscription(
        &owner.id,
        "cortex_pulse",
        None,
        Some("@cortex_pulse"),
    )?;
    println!("development sources ready for {}", owner.username);
    Ok(())
}

fn read_admin_password() -> Result<String> {
    if let Ok(password) = env::var("RILL_ADMIN_PASSWORD") {
        return Ok(password);
    }
    let mut password = String::new();
    io::stdin()
        .read_to_string(&mut password)
        .context("read password from standard input")?;
    if password.trim_end().is_empty() {
        bail!("provide password on standard input or in RILL_ADMIN_PASSWORD")
    }
    Ok(password)
}

fn doctor(settings: &Settings) -> Result<()> {
    require_file(&settings.assets.renderer_wasm, "renderer AOT module")?;
    require_directory(&settings.assets.static_dir, "static asset")?;
    let limits = RendererLimits {
        fuel: settings.renderer.fuel,
        memory_bytes: settings.renderer.memory_bytes,
        input_bytes: 512 * 1024,
        output_bytes: settings.renderer.max_response_bytes,
        timeout: std::time::Duration::from_millis(settings.renderer.timeout_ms),
    };
    load_renderer(&settings.assets.renderer_wasm, limits)?;
    open_pool(settings)?;
    println!("configuration, SQLite, renderer, and static assets are ready");
    Ok(())
}

fn backup(settings: &Settings, output: &Path) -> Result<()> {
    if output == settings.database.path {
        bail!("backup output must differ from the live database path");
    }
    if output.exists() {
        bail!("backup output already exists: {}", output.display());
    }
    let parent = output.parent().filter(|path| !path.as_os_str().is_empty());
    if parent.is_some_and(|path| !path.is_dir()) {
        bail!("backup output directory does not exist");
    }
    let output = output
        .to_str()
        .context("backup output path is not valid UTF-8")?;
    open_pool(settings)?.with_connection(|connection| {
        connection.execute("VACUUM INTO ?1", [output])?;
        Ok(())
    })?;
    println!("created consistent SQLite backup at {output}");
    Ok(())
}

fn require_file(path: &Path, label: &str) -> Result<()> {
    if path.is_file() {
        Ok(())
    } else {
        bail!("{label} not found: {}", path.display())
    }
}

fn require_directory(path: &Path, label: &str) -> Result<()> {
    if path.is_dir() {
        Ok(())
    } else {
        bail!("{label} directory not found: {}", path.display())
    }
}
