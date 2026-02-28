use anyhow::Result;
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tokio::time::sleep;
use tracing::{info, warn};

#[derive(Parser, Debug)]
#[command(name = "seaport-harness")]
#[command(about = "Rust-first LLM harness scaffold for Seaport operations")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Run a single task (one-shot)
    Run {
        #[arg(long)]
        objective: String,
    },
    /// Start loop mode with periodic ticks
    Loop {
        #[arg(long, default_value_t = 1800)]
        interval_seconds: u64,
    },
    /// Print harness health/config
    Status,
}

#[derive(Debug, Serialize, Deserialize)]
struct HarnessEvent {
    kind: String,
    detail: String,
    ts_unix: u64,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()))
        .init();

    let cli = Cli::parse();

    match cli.command {
        Commands::Run { objective } => run_once(objective).await?,
        Commands::Loop { interval_seconds } => loop_mode(interval_seconds).await?,
        Commands::Status => status().await?,
    }

    Ok(())
}

async fn run_once(objective: String) -> Result<()> {
    info!(%objective, "run_once start");
    // TODO: plug in model provider routing + tool execution adapters
    let evt = HarnessEvent {
        kind: "run_once".to_string(),
        detail: format!("Objective accepted: {objective}"),
        ts_unix: now_unix(),
    };

    println!("{}", serde_json::to_string_pretty(&evt)?);
    info!("run_once complete");
    Ok(())
}

async fn loop_mode(interval_seconds: u64) -> Result<()> {
    warn!(interval_seconds, "loop mode scaffold running");
    loop {
        let evt = HarnessEvent {
            kind: "tick".to_string(),
            detail: "heartbeat tick (TODO: schedule jobs + evaluate queue)".to_string(),
            ts_unix: now_unix(),
        };
        println!("{}", serde_json::to_string(&evt)?);
        sleep(Duration::from_secs(interval_seconds)).await;
    }
}

async fn status() -> Result<()> {
    let status = serde_json::json!({
        "name": "seaport-harness",
        "version": "0.1.0",
        "mode": "scaffold",
        "next": [
            "provider adapters",
            "tool registry",
            "task queue",
            "cost guardrails",
            "eval hooks"
        ]
    });
    println!("{}", serde_json::to_string_pretty(&status)?);
    Ok(())
}

fn now_unix() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
