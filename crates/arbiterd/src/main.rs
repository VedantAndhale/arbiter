use anyhow::Result;
use clap::Parser;
use std::path::PathBuf;
use tracing_subscriber::EnvFilter;

#[derive(Parser)]
#[command(name = "arbiterd", version, about = "Arbiter daemon")]
struct Args {
    /// Data directory (database, worktrees, discovery file).
    #[arg(long, env = "ARBITER_HOME")]
    home: Option<PathBuf>,
    /// Port on 127.0.0.1 to listen on.
    #[arg(long, env = "ARBITER_PORT", default_value_t = 7433)]
    port: u16,
    /// Restore history, tasks and settings from a memory folder (a clone of
    /// your memory repository) into an empty home, then exit.
    #[arg(long)]
    restore: Option<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info")))
        .init();
    let args = Args::parse();
    let home = args.home.unwrap_or_else(arbiterd::arbiter_home);
    if let Some(memory) = args.restore {
        let n = arbiterd::restore(&home, &memory)?;
        println!("Restored {n} events into {}", home.display());
        return Ok(());
    }
    let running = arbiterd::start(arbiterd::Config::new(home, args.port)).await?;
    tokio::select! {
        r = running.handle => r??,
        _ = tokio::signal::ctrl_c() => tracing::info!("shutting down"),
    }
    Ok(())
}
