use anyhow::Result;
use clap::{Parser, Subcommand};
use seaport_harness::config::AppConfig;
use seaport_harness::eval::evaluate_replay;
use seaport_harness::events::{EventSink, HarnessEvent};
use seaport_harness::orchestrator::{Orchestrator, TaskSpec};
use seaport_harness::provider::{EchoProvider, HttpProviderStub, Provider};
use seaport_harness::replay::replay_file;
use seaport_harness::tools::ToolRegistry;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{info, warn};

#[derive(Parser, Debug)]
#[command(name = "seaport-harness")]
#[command(about = "Rust-first LLM harness scaffold for Seaport operations")]
struct Cli {
    /// Optional config file path (TOML)
    #[arg(long)]
    config: Option<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Run a single task through provider/tool orchestrator.
    Run {
        #[arg(long)]
        objective: String,
    },
    /// Loop mode: repeatedly run the orchestrator against the same objective.
    Loop {
        #[arg(long, default_value_t = 1800)]
        interval_seconds: u64,
        #[arg(long, default_value = "heartbeat time task")]
        objective: String,
    },
    /// Print harness config and mode details.
    Status,
    /// Replay event log and print event-kind summary.
    Replay {
        #[arg(long)]
        path: Option<String>,
    },
    /// Run basic eval checks against event log.
    Eval {
        #[arg(long)]
        path: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()))
        .init();

    let cli = Cli::parse();
    let cfg = AppConfig::load(cli.config.as_deref())?;

    match cli.command {
        Commands::Run { objective } => run_once(objective, &cfg).await?,
        Commands::Loop {
            interval_seconds,
            objective,
        } => loop_mode(interval_seconds, objective, &cfg).await?,
        Commands::Status => status(&cfg).await?,
        Commands::Replay { path } => replay(path.as_deref(), &cfg).await?,
        Commands::Eval { path } => eval(path.as_deref(), &cfg).await?,
    }

    Ok(())
}

async fn run_once(objective: String, cfg: &AppConfig) -> Result<()> {
    info!(%objective, "run_once start");
    let provider: Box<dyn Provider> = match cfg.provider.kind.as_str() {
        "http-stub" => Box::new(HttpProviderStub {
            endpoint: "http://localhost:11434/v1/chat/completions".to_string(),
            model: cfg.provider.model.clone(),
        }),
        _ => Box::new(EchoProvider),
    };

    let tools = ToolRegistry::with_defaults();
    let sink = EventSink::new(&cfg.event_log_path)?;

    let orchestrator = Orchestrator {
        provider: provider.as_ref(),
        tools: &tools,
        cfg: cfg.orchestrator.clone(),
        event_sink: &sink,
    };

    let task_id = format!("task-{}", seaport_harness::events::now_unix());
    let summary = orchestrator.run_task(TaskSpec { task_id: task_id.clone(), objective })?;
    sink.emit(&HarnessEvent::new("cli.run.summary").with_task_id(task_id).with_data(
        serde_json::json!({
            "steps": summary.steps,
            "tool_calls": summary.tool_calls,
            "reason": summary.stopped_reason,
        }),
    ))?;

    println!("{}", serde_json::to_string_pretty(&summary)?);
    info!("run_once complete");
    Ok(())
}

async fn loop_mode(interval_seconds: u64, objective: String, cfg: &AppConfig) -> Result<()> {
    warn!(interval_seconds, "loop mode active");
    loop {
        run_once(objective.clone(), cfg).await?;
        sleep(Duration::from_secs(interval_seconds)).await;
    }
}

async fn status(cfg: &AppConfig) -> Result<()> {
    println!("{}", serde_json::to_string_pretty(cfg)?);
    Ok(())
}

async fn replay(path: Option<&str>, cfg: &AppConfig) -> Result<()> {
    let p = path.unwrap_or(&cfg.event_log_path);
    let summary = replay_file(p)?;
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}

async fn eval(path: Option<&str>, cfg: &AppConfig) -> Result<()> {
    let p = path.unwrap_or(&cfg.event_log_path);
    let summary = replay_file(p)?;
    let report = evaluate_replay(&summary);
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}
