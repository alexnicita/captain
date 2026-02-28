use agent_harness::config::AppConfig;
use agent_harness::eval::evaluate_replay;
use agent_harness::events::{kinds, now_unix, EventSink, HarnessEvent};
use agent_harness::orchestrator::{Orchestrator, TaskSpec};
use agent_harness::provider::build_provider;
use agent_harness::replay::replay_file;
use agent_harness::runtime_gate::{
    gate_start, gate_status, gate_stop, GateStartArgs, GateStatusArgs, GateStopArgs,
};
use agent_harness::scheduler::{QueuedTask, Scheduler, TaskQueue};
use agent_harness::tools::{ToolPolicy, ToolPolicyMode, ToolRegistry};
use anyhow::Result;
use clap::{Parser, Subcommand};
use serde_json::json;
use std::fs;
use std::time::Duration;
use tokio::time::sleep;
use tracing::{info, warn};

#[derive(Parser, Debug)]
#[command(name = "agent-harness")]
#[command(about = "General-purpose Rust harness scaffold for provider/tool orchestration")]
struct Cli {
    /// Optional config file path (TOML)
    #[arg(long)]
    config: Option<String>,

    /// Restrict execution to this set of tools (repeatable).
    #[arg(long)]
    allow_tool: Vec<String>,

    /// Block execution for this set of tools (repeatable).
    #[arg(long)]
    deny_tool: Vec<String>,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Run a single task through provider/tool orchestrator.
    Run {
        #[arg(long)]
        objective: String,
        #[arg(long)]
        task_id: Option<String>,
    },
    /// Loop mode: repeatedly run the orchestrator against the same objective.
    Loop {
        #[arg(long, default_value_t = 1800)]
        interval_seconds: u64,
        #[arg(long, default_value = "heartbeat time task")]
        objective: String,
        /// 0 = unbounded.
        #[arg(long, default_value_t = 0)]
        max_iterations: u32,
    },
    /// Run a queue of objectives from a text file (one per line).
    Batch {
        #[arg(long)]
        objectives_file: String,
    },
    /// Enforce runtime + checklist completion gates (Rust port of prior Python helper).
    Gate {
        #[command(subcommand)]
        command: GateCommands,
    },
    /// Print harness config and registered tools.
    Status,
    /// Replay event log and print event-kind summary.
    Replay {
        #[arg(long)]
        path: Option<String>,
    },
    /// Run eval checks against event log.
    Eval {
        #[arg(long)]
        path: Option<String>,
    },
}

#[derive(Subcommand, Debug)]
enum GateCommands {
    /// Start a runtime-gated checklist loop.
    Start {
        #[arg(long)]
        checklist: String,
        #[arg(long)]
        run_id: Option<String>,
        #[arg(long, default_value_t = 60.0)]
        min_runtime_minutes: f64,
        #[arg(long, default_value_t = 10.0)]
        heartbeat_minutes: f64,
        #[arg(long, default_value_t = 15)]
        poll_seconds: u64,
        #[arg(long, default_value_t = false)]
        dry_run: bool,
        #[arg(long, default_value_t = 75)]
        dry_runtime_sec: u64,
        #[arg(long, default_value_t = 12)]
        dry_heartbeat_sec: u64,
        #[arg(long)]
        base_dir: Option<String>,
    },
    /// Show status for current/latest runtime-gate run.
    Status {
        #[arg(long)]
        run_dir: Option<String>,
        #[arg(long)]
        base_dir: Option<String>,
    },
    /// Request stop for current/latest runtime-gate run.
    Stop {
        #[arg(long)]
        run_dir: Option<String>,
        #[arg(long)]
        base_dir: Option<String>,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(std::env::var("RUST_LOG").unwrap_or_else(|_| "info".into()))
        .init();

    let cli = Cli::parse();
    let cfg = AppConfig::load(cli.config.as_deref())?;
    let policy = make_policy(&cli);

    match cli.command {
        Commands::Run { objective, task_id } => run_once(objective, task_id, &cfg, &policy).await?,
        Commands::Loop {
            interval_seconds,
            objective,
            max_iterations,
        } => loop_mode(interval_seconds, objective, max_iterations, &cfg, &policy).await?,
        Commands::Batch { objectives_file } => batch_mode(&objectives_file, &cfg, &policy).await?,
        Commands::Gate { command } => gate_command(command).await?,
        Commands::Status => status(&cfg).await?,
        Commands::Replay { path } => replay(path.as_deref(), &cfg).await?,
        Commands::Eval { path } => eval(path.as_deref(), &cfg).await?,
    }

    Ok(())
}

fn make_policy(cli: &Cli) -> ToolPolicy {
    let mut policy = if cli.allow_tool.is_empty() {
        ToolPolicy::default()
    } else {
        ToolPolicy {
            mode: ToolPolicyMode::AllowList,
            allowed_tools: cli.allow_tool.iter().cloned().collect(),
            denied_tools: Default::default(),
        }
    };

    for denied in &cli.deny_tool {
        policy.denied_tools.insert(denied.clone());
    }
    policy
}

fn parse_queue_line(line: &str) -> Option<(u8, String)> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }

    if let Some(rest) = trimmed.strip_prefix("[p1]") {
        return Some((1, rest.trim().to_string()));
    }
    if let Some(rest) = trimmed.strip_prefix("[p0]") {
        return Some((0, rest.trim().to_string()));
    }

    Some((0, trimmed.to_string()))
}

async fn run_once(
    objective: String,
    task_id: Option<String>,
    cfg: &AppConfig,
    policy: &ToolPolicy,
) -> Result<()> {
    info!(%objective, "run_once start");
    let provider = build_provider(&cfg.provider);
    let tools = ToolRegistry::with_defaults();
    let sink = EventSink::new(&cfg.event_log_path)?;

    sink.emit(&HarnessEvent::new(kinds::RUN_STARTED).with_data(json!({
        "mode": "run",
        "provider": cfg.provider.kind.clone(),
        "model": cfg.provider.model.clone(),
    })))?;

    let orchestrator = Orchestrator {
        provider: provider.as_ref(),
        provider_cfg: cfg.provider.clone(),
        tools: &tools,
        tool_policy: policy.clone(),
        cfg: cfg.orchestrator.clone(),
        event_sink: &sink,
    };

    let task_id = task_id.unwrap_or_else(|| format!("task-{}", now_unix()));
    let summary = orchestrator
        .run_task(TaskSpec {
            task_id: task_id.clone(),
            objective,
        })
        .await?;

    sink.emit(
        &HarnessEvent::new(kinds::CLI_RUN_SUMMARY)
            .with_task_id(task_id)
            .with_data(json!({
                "steps": summary.steps,
                "tool_calls": summary.tool_calls,
                "reason": summary.stopped_reason.clone(),
                "runtime_ms": summary.runtime_ms,
            })),
    )?;

    sink.emit(&HarnessEvent::new(kinds::RUN_FINISHED).with_data(json!({
        "mode": "run",
        "event_log": sink.path().display().to_string(),
        "run_id": sink.run_id(),
    })))?;

    println!("{}", serde_json::to_string_pretty(&summary)?);
    info!(run_id = %sink.run_id(), "run_once complete");
    Ok(())
}

async fn loop_mode(
    interval_seconds: u64,
    objective: String,
    max_iterations: u32,
    cfg: &AppConfig,
    policy: &ToolPolicy,
) -> Result<()> {
    warn!(interval_seconds, max_iterations, "loop mode active");
    let mut count = 0u32;
    loop {
        run_once(objective.clone(), None, cfg, policy).await?;
        count += 1;
        if max_iterations > 0 && count >= max_iterations {
            break;
        }
        sleep(Duration::from_secs(interval_seconds)).await;
    }
    Ok(())
}

async fn batch_mode(objectives_file: &str, cfg: &AppConfig, policy: &ToolPolicy) -> Result<()> {
    let provider = build_provider(&cfg.provider);
    let tools = ToolRegistry::with_defaults();
    let sink = EventSink::new(&cfg.event_log_path)?;

    sink.emit(&HarnessEvent::new(kinds::RUN_STARTED).with_data(json!({
        "mode": "batch",
        "provider": cfg.provider.kind.clone(),
        "model": cfg.provider.model.clone(),
        "objectives_file": objectives_file,
    })))?;

    let orchestrator = Orchestrator {
        provider: provider.as_ref(),
        provider_cfg: cfg.provider.clone(),
        tools: &tools,
        tool_policy: policy.clone(),
        cfg: cfg.orchestrator.clone(),
        event_sink: &sink,
    };

    let mut queue = TaskQueue::new();
    let lines = fs::read_to_string(objectives_file)?;
    for (idx, line) in lines.lines().enumerate() {
        let Some((priority, objective)) = parse_queue_line(line) else {
            continue;
        };
        queue.enqueue(QueuedTask {
            task_id: format!("task-{}-{}", now_unix(), idx),
            objective,
            priority,
        });
    }

    let scheduler = Scheduler {
        orchestrator: &orchestrator,
        event_sink: &sink,
        cfg: cfg.scheduler.clone(),
    };
    let summary = scheduler.run_queue(queue).await?;

    sink.emit(
        &HarnessEvent::new(kinds::CLI_BATCH_SUMMARY).with_data(json!({
            "total": summary.total,
            "completed": summary.completed,
            "failed": summary.failed,
            "max_concurrent_tasks": cfg.scheduler.max_concurrent_tasks,
        })),
    )?;

    sink.emit(&HarnessEvent::new(kinds::RUN_FINISHED).with_data(json!({
        "mode": "batch",
        "event_log": sink.path().display().to_string(),
        "run_id": sink.run_id(),
    })))?;

    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}

async fn gate_command(command: GateCommands) -> Result<()> {
    match command {
        GateCommands::Start {
            checklist,
            run_id,
            min_runtime_minutes,
            heartbeat_minutes,
            poll_seconds,
            dry_run,
            dry_runtime_sec,
            dry_heartbeat_sec,
            base_dir,
        } => {
            gate_start(GateStartArgs {
                checklist,
                run_id,
                min_runtime_minutes,
                heartbeat_minutes,
                poll_seconds,
                dry_run,
                dry_runtime_sec,
                dry_heartbeat_sec,
                base_dir,
            })
            .await?
        }
        GateCommands::Status { run_dir, base_dir } => {
            let status = gate_status(GateStatusArgs { run_dir, base_dir })?;
            println!("{}", serde_json::to_string_pretty(&status)?);
        }
        GateCommands::Stop { run_dir, base_dir } => {
            let response = gate_stop(GateStopArgs { run_dir, base_dir })?;
            println!("{}", serde_json::to_string_pretty(&response)?);
        }
    }

    Ok(())
}

async fn status(cfg: &AppConfig) -> Result<()> {
    let tools = ToolRegistry::with_defaults();
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "config": cfg,
            "tool_specs": tools.specs(),
        }))?
    );
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
