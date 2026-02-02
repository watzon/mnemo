use clap::Parser;
use mnemo_tui::{App, Tui};

#[derive(Parser, Debug)]
#[command(name = "mnemo-tui")]
#[command(about = "Real-time TUI dashboard for mnemo daemon")]
#[command(version)]
struct Args {
    /// URL of the mnemo daemon (e.g., http://localhost:8420)
    #[arg(short, long, default_value = "http://localhost:8420")]
    daemon: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    println!("Connecting to daemon at: {}", args.daemon);

    // TODO: Initialize and run TUI in later tasks
    Ok(())
}
