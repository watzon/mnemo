use std::path::PathBuf;

use clap::{Parser, Subcommand};
use mnemo_server::storage::LanceStore;
use mnemo_cli::commands::{CompactCommand, ConfigCommand, MemoryCommand, StatsCommand};
use mnemo_cli::error::CliResult;
use mnemo_cli::output::OutputFormat;

#[derive(Parser)]
#[command(name = "mnemo-cli")]
#[command(about = "Mnemo CLI - Management tool for the mnemo daemon")]
#[command(version)]
pub struct Cli {
    #[clap(long, short, global = true, help = "Output in JSON format")]
    pub json: bool,

    #[clap(long, short = 'd', global = true, help = "Path to data directory")]
    pub data_dir: Option<PathBuf>,

    #[clap(long, short = 'c', global = true, help = "Path to config file")]
    pub config: Option<PathBuf>,

    #[clap(subcommand)]
    pub command: Command,
}

#[derive(Subcommand)]
pub enum Command {
    #[clap(about = "Memory management commands")]
    Memory(MemoryCommand),

    #[clap(about = "Show storage statistics")]
    Stats(StatsCommand),

    #[clap(about = "Trigger memory compaction")]
    Compact(CompactCommand),

    #[clap(about = "Configuration commands")]
    Config(ConfigCommand),
}

#[tokio::main]
async fn main() {
    if let Err(e) = run().await {
        eprintln!("Error: {e}");
        std::process::exit(1);
    }
}

async fn run() -> CliResult<()> {
    let cli = Cli::parse();

    let format = if cli.json {
        OutputFormat::Json
    } else {
        OutputFormat::Table
    };

    let data_dir = cli.data_dir.clone().unwrap_or_else(|| {
        dirs::home_dir()
            .map(|h| h.join(".mnemo"))
            .unwrap_or_else(|| PathBuf::from(".mnemo"))
    });

    match &cli.command {
        Command::Config(cmd) => cmd.execute(cli.config.as_deref(), format).await,
        Command::Memory(_) | Command::Stats(_) | Command::Compact(_) => {
            let mut store = LanceStore::connect(&data_dir).await?;

            if store.table_exists("memories").await? {
                store.open_memories_table().await?;
            } else {
                store.create_memories_table().await?;
            }

            if store.table_exists("tombstones").await? {
                store.open_tombstones_table().await?;
            } else {
                store.create_tombstones_table().await?;
            }

            match &cli.command {
                Command::Memory(cmd) => cmd.execute(&store, format).await,
                Command::Stats(cmd) => cmd.execute(&store, format).await,
                Command::Compact(cmd) => cmd.execute(&store, format).await,
                Command::Config(_) => unreachable!(),
            }
        }
    }
}
