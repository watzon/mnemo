//! Mnemo Daemon - HTTP proxy for transparent LLM memory injection

use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::Mutex as TokioMutex;

use clap::{Parser, Subcommand};
use tracing_subscriber::{EnvFilter, layer::SubscriberExt, util::SubscriberInitExt};

use mnemo::config::Config;
use mnemo::embedding::EmbeddingModel;
use mnemo::error::Result;
use mnemo::proxy::ProxyServer;
use mnemo::router::MemoryRouter;
use mnemo::storage::LanceStore;

/// Mnemo - Transparent HTTP proxy that gives your LLM long-term memory
#[derive(Parser)]
#[command(name = "mnemo")]
#[command(about = "A transparent HTTP proxy that gives your LLM long-term memory")]
#[command(version)]
pub struct Cli {
    /// Path to config file
    #[arg(long, short = 'c', global = true)]
    pub config: Option<PathBuf>,

    #[command(subcommand)]
    pub command: Option<Command>,
}

#[derive(Subcommand)]
pub enum Command {
    /// Start the proxy server (default command)
    #[command(name = "serve")]
    Serve,
}

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

async fn run() -> Result<()> {
    init_logging();

    let cli = Cli::parse();

    match cli.command {
        None | Some(Command::Serve) => serve(cli.config).await,
    }
}

fn init_logging() {
    let filter =
        EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info,mnemo=debug"));

    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer())
        .init();
}

fn load_config(config_path: Option<PathBuf>) -> Result<Config> {
    if let Some(path) = config_path {
        tracing::info!("Loading config from: {}", path.display());
        let content = std::fs::read_to_string(&path).map_err(|e| {
            mnemo::MnemoError::Config(format!(
                "Failed to read config file {}: {}",
                path.display(),
                e
            ))
        })?;
        let config: Config = toml::from_str(&content)
            .map_err(|e| mnemo::MnemoError::Config(format!("Failed to parse config: {e}")))?;
        Ok(config)
    } else {
        let default_paths = [
            dirs::home_dir().map(|h| h.join(".mnemo").join("config.toml")),
            dirs::config_dir().map(|c| c.join("mnemo").join("config.toml")),
            Some(PathBuf::from("config.toml")),
        ];

        for path_opt in default_paths.iter().flatten() {
            if path_opt.exists() {
                tracing::info!("Loading config from: {}", path_opt.display());
                let content = std::fs::read_to_string(path_opt).map_err(|e| {
                    mnemo::MnemoError::Config(format!(
                        "Failed to read config file {}: {}",
                        path_opt.display(),
                        e
                    ))
                })?;
                let config: Config = toml::from_str(&content).map_err(|e| {
                    mnemo::MnemoError::Config(format!("Failed to parse config: {e}"))
                })?;
                return Ok(config);
            }
        }

        tracing::info!("No config file found, using defaults");
        Ok(Config::default())
    }
}

async fn serve(config_path: Option<PathBuf>) -> Result<()> {
    tracing::info!("Starting Mnemo daemon");

    let config = load_config(config_path)?;
    tracing::debug!("Config loaded: {:?}", config);

    let data_dir = &config.storage.data_dir;
    tracing::info!("Initializing storage at: {}", data_dir.display());

    std::fs::create_dir_all(data_dir).map_err(|e| {
        mnemo::MnemoError::Storage(format!(
            "Failed to create data directory {}: {}",
            data_dir.display(),
            e
        ))
    })?;

    let mut store = LanceStore::connect(data_dir).await?;

    if store.table_exists("memories").await? {
        tracing::debug!("Opening existing memories table");
        store.open_memories_table().await?;
    } else {
        tracing::info!("Creating memories table");
        store.create_memories_table().await?;
    }

    if store.table_exists("tombstones").await? {
        tracing::debug!("Opening existing tombstones table");
        store.open_tombstones_table().await?;
    } else {
        tracing::info!("Creating tombstones table");
        store.create_tombstones_table().await?;
    }

    tracing::info!("Initializing embedding model (this may take a moment on first run)...");
    let embedding_model = EmbeddingModel::new()?;
    tracing::info!("Embedding model initialized");

    tracing::info!("Initializing memory router...");
    let router = MemoryRouter::new()?;
    tracing::info!("Memory router initialized");

    // Wrap components for sharing across async handlers
    let store = Arc::new(TokioMutex::new(store));
    let embedding_model = Arc::new(embedding_model);
    let router = Arc::new(router);

    let proxy = ProxyServer::new(
        config.proxy.clone(),
        store,
        embedding_model,
        router,
        config.router.clone(),
    );
    tracing::info!("Starting proxy server on {}", config.proxy.listen_addr);

    proxy.serve().await?;

    tracing::info!("Mnemo daemon stopped");
    Ok(())
}
