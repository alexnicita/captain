use agent_harness::coding::{
    parse_duration_seconds, run_coding_loop, CodingRunArgs, ExecutorPreset,
};
use agent_harness::config::AppConfig;
use agent_harness::eval::evaluate_events;
use agent_harness::events::{kinds, now_unix, EventSink, HarnessEvent};
use agent_harness::model_profile::ModelProfile;
use agent_harness::orchestrator::{Orchestrator, TaskSpec};
use agent_harness::provider::build_provider;
use agent_harness::replay::{
    replay_events_file_with_filter, replay_file_with_filter, ReplayFilter,
};
use agent_harness::runtime_gate::{
    gate_start, gate_status, gate_stop, GateStartArgs, GateStatusArgs, GateStopArgs,
};
use agent_harness::scheduler::{QueuedTask, Scheduler, TaskQueue};
use agent_harness::tools::{ToolPolicy, ToolPolicyMode, ToolRegistry};
use anyhow::{anyhow, Result};
use clap::{Parser, Subcommand};
use serde_json::json;
use std::fs;
use std::path::Path;
use std::time::Duration;
use tokio::process::Command;
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

    /// Shortcut: run coding mode with this prompt (equivalent to `code --repo . --time 1h --executor openclaw --prompt ...`)
    #[arg(index = 1)]
    quick_prompt: Option<String>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand, Debug)]
#[allow(clippy::large_enum_variant)]
enum Commands {
    /// Simple default launcher for coding mode.
    Start {
        /// Optional objective prompt for this run.
        #[arg(long)]
        prompt: Option<String>,
        /// Total run time (default: 5m).
        #[arg(long, default_value = "5m")]
        time: String,
    },
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
    /// Run coding cycles against a repository for a fixed timebox.
    Code {
        #[arg(long)]
        repo: String,
        #[arg(long)]
        time: String,
        #[arg(long, default_value_t = 30)]
        heartbeat_sec: u64,
        #[arg(long, default_value_t = 2)]
        cycle_pause_sec: u64,
        #[arg(long, default_value_t = false)]
        supercycle: bool,
        #[arg(long, default_value_t = 15)]
        research_budget_sec: u64,
        #[arg(long, default_value_t = 20)]
        planning_budget_sec: u64,
        #[arg(long, default_value_t = false)]
        require_commit_each_cycle: bool,
        #[arg(long, default_value = "cargo")]
        executor: String,
        #[arg(long)]
        plan_cmd: Vec<String>,
        #[arg(long)]
        act_cmd: Vec<String>,
        #[arg(long)]
        verify_cmd: Vec<String>,
        #[arg(long)]
        allow_cmd: Vec<String>,
        #[arg(long, default_value_t = false)]
        commit_each_cycle: bool,
        #[arg(long, default_value_t = false)]
        push_each_cycle: bool,
        #[arg(long)]
        cycle_output_file: Option<String>,
        #[arg(long)]
        runtime_log_file: Option<String>,
        #[arg(long)]
        thought_log_file: Option<String>,
        #[arg(long, default_value_t = 3)]
        noop_streak_limit: u64,
        #[arg(long, default_value_t = 3)]
        conformance_interval_unchanged: u64,
        #[arg(long)]
        progress_file: Option<String>,
        #[arg(long)]
        run_lock_file: Option<String>,
        #[arg(long, conflicts_with = "prompt_file")]
        prompt: Option<String>,
        #[arg(long, conflicts_with = "prompt")]
        prompt_file: Option<String>,
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
        #[arg(long)]
        run_id: Option<String>,
        #[arg(long, default_value_t = false)]
        latest_run: bool,
    },
    /// Run eval checks against event log.
    Eval {
        #[arg(long)]
        path: Option<String>,
        #[arg(long)]
        run_id: Option<String>,
        #[arg(long, default_value_t = false)]
        latest_run: bool,
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
        Some(Commands::Start { prompt, time }) => {
            prepare_start_workspace(".").await?;
            let args = CodingModeArgs {
                repo: ".".to_string(),
                time,
                heartbeat_sec: 30,
                cycle_pause_sec: 2,
                executor: "openclaw".to_string(),
                supercycle: true,
                research_budget_sec: 20,
                planning_budget_sec: 40,
                require_commit_each_cycle: true,
                plan_cmd: Vec::new(),
                act_cmd: Vec::new(),
                verify_cmd: Vec::new(),
                allow_cmd: Vec::new(),
                commit_each_cycle: true,
                push_each_cycle: true,
                cycle_output_file: None,
                runtime_log_file: Some("./runs/runtime.log".to_string()),
                thought_log_file: Some("./runs/thoughts.md".to_string()),
                noop_streak_limit: 3,
                conformance_interval_unchanged: 3,
                progress_file: None,
                run_lock_file: None,
                prompt,
                prompt_file: None,
            };
            coding_mode(&cfg, args).await?
        }
        Some(Commands::Run { objective, task_id }) => {
            run_once(objective, task_id, &cfg, &policy).await?
        }
        Some(Commands::Loop {
            interval_seconds,
            objective,
            max_iterations,
        }) => loop_mode(interval_seconds, objective, max_iterations, &cfg, &policy).await?,
        Some(Commands::Batch { objectives_file }) => {
            batch_mode(&objectives_file, &cfg, &policy).await?
        }
        Some(Commands::Code {
            repo,
            time,
            heartbeat_sec,
            cycle_pause_sec,
            supercycle,
            research_budget_sec,
            planning_budget_sec,
            require_commit_each_cycle,
            executor,
            plan_cmd,
            act_cmd,
            verify_cmd,
            allow_cmd,
            commit_each_cycle,
            push_each_cycle,
            cycle_output_file,
            runtime_log_file,
            thought_log_file,
            noop_streak_limit,
            conformance_interval_unchanged,
            progress_file,
            run_lock_file,
            prompt,
            prompt_file,
        }) => {
            let args = CodingModeArgs {
                repo,
                time,
                heartbeat_sec,
                cycle_pause_sec,
                executor,
                supercycle,
                research_budget_sec,
                planning_budget_sec,
                require_commit_each_cycle,
                plan_cmd,
                act_cmd,
                verify_cmd,
                allow_cmd,
                commit_each_cycle,
                push_each_cycle,
                cycle_output_file,
                runtime_log_file,
                thought_log_file,
                noop_streak_limit,
                conformance_interval_unchanged,
                progress_file,
                run_lock_file,
                prompt,
                prompt_file,
            };
            coding_mode(&cfg, args).await?
        }
        Some(Commands::Gate { command }) => gate_command(command).await?,
        Some(Commands::Status) => status(&cfg).await?,
        Some(Commands::Replay {
            path,
            run_id,
            latest_run,
        }) => replay(path.as_deref(), run_id.as_deref(), latest_run, &cfg).await?,
        Some(Commands::Eval {
            path,
            run_id,
            latest_run,
        }) => eval(path.as_deref(), run_id.as_deref(), latest_run, &cfg).await?,
        None => {
            if let Some(prompt) = cli.quick_prompt {
                prepare_start_workspace(".").await?;
                let args = CodingModeArgs {
                    repo: ".".to_string(),
                    time: "1h".to_string(),
                    heartbeat_sec: 30,
                    cycle_pause_sec: 2,
                    executor: "openclaw".to_string(),
                    supercycle: true,
                    research_budget_sec: 20,
                    planning_budget_sec: 40,
                    require_commit_each_cycle: true,
                    plan_cmd: Vec::new(),
                    act_cmd: Vec::new(),
                    verify_cmd: Vec::new(),
                    allow_cmd: Vec::new(),
                    commit_each_cycle: true,
                    push_each_cycle: true,
                    cycle_output_file: None,
                    runtime_log_file: Some("./runs/runtime.log".to_string()),
                    thought_log_file: Some("./runs/thoughts.md".to_string()),
                    noop_streak_limit: 3,
                    conformance_interval_unchanged: 3,
                    progress_file: None,
                    run_lock_file: None,
                    prompt: Some(prompt),
                    prompt_file: None,
                };
                coding_mode(&cfg, args).await?
            } else {
                return Err(anyhow!(
                    "no command provided (use a subcommand or pass a prompt string)"
                ));
            }
        }
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
    let built_provider = build_provider(&cfg.provider);
    if let Some(reason) = built_provider.fallback_reason.as_deref() {
        warn!(
            requested = %built_provider.requested_kind,
            resolved = %built_provider.resolved_kind,
            %reason,
            "provider init fallback applied"
        );
    }
    let provider_requested = built_provider.requested_kind.clone();
    let provider_resolved = built_provider.resolved_kind.clone();
    let provider_fallback_reason = built_provider.fallback_reason.clone();
    let model_profile = ModelProfile::for_model(&cfg.provider.model);
    let provider = built_provider.provider;

    let tools = ToolRegistry::with_defaults();
    let sink = EventSink::new(&cfg.event_log_path)?;

    sink.emit(&HarnessEvent::new(kinds::RUN_STARTED).with_data(json!({
        "mode": "run",
        "provider_requested": provider_requested,
        "provider_resolved": provider_resolved,
        "provider_fallback_reason": provider_fallback_reason,
        "model": cfg.provider.model.clone(),
        "model_profile": model_profile,
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
    let built_provider = build_provider(&cfg.provider);
    if let Some(reason) = built_provider.fallback_reason.as_deref() {
        warn!(
            requested = %built_provider.requested_kind,
            resolved = %built_provider.resolved_kind,
            %reason,
            "provider init fallback applied"
        );
    }
    let provider_requested = built_provider.requested_kind.clone();
    let provider_resolved = built_provider.resolved_kind.clone();
    let provider_fallback_reason = built_provider.fallback_reason.clone();
    let model_profile = ModelProfile::for_model(&cfg.provider.model);
    let provider = built_provider.provider;

    let tools = ToolRegistry::with_defaults();
    let sink = EventSink::new(&cfg.event_log_path)?;

    sink.emit(&HarnessEvent::new(kinds::RUN_STARTED).with_data(json!({
        "mode": "batch",
        "provider_requested": provider_requested,
        "provider_resolved": provider_resolved,
        "provider_fallback_reason": provider_fallback_reason,
        "model": cfg.provider.model.clone(),
        "model_profile": model_profile,
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
    let task_prefix = format!("task-{}", now_unix());
    let mut task_index = 0usize;
    for line in lines.lines() {
        let Some((priority, objective)) = parse_queue_line(line) else {
            continue;
        };
        queue.enqueue(QueuedTask {
            task_id: format!("{task_prefix}-{task_index}"),
            objective,
            priority,
        });
        task_index += 1;
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

struct CodingModeArgs {
    repo: String,
    time: String,
    heartbeat_sec: u64,
    cycle_pause_sec: u64,
    executor: String,
    supercycle: bool,
    research_budget_sec: u64,
    planning_budget_sec: u64,
    require_commit_each_cycle: bool,
    plan_cmd: Vec<String>,
    act_cmd: Vec<String>,
    verify_cmd: Vec<String>,
    allow_cmd: Vec<String>,
    commit_each_cycle: bool,
    push_each_cycle: bool,
    cycle_output_file: Option<String>,
    runtime_log_file: Option<String>,
    thought_log_file: Option<String>,
    noop_streak_limit: u64,
    conformance_interval_unchanged: u64,
    progress_file: Option<String>,
    run_lock_file: Option<String>,
    prompt: Option<String>,
    prompt_file: Option<String>,
}

async fn prepare_start_workspace(repo: &str) -> Result<()> {
    let repo_path = Path::new(repo);

    // Stop stale/parallel harness runners so `cargo run -- start` is clean by default.
    let _ = Command::new("pkill")
        .args([
            "-f",
            "scripts/harness\\.sh|agent-harness( --config [^ ]+)? code --repo|target/debug/agent-harness( --config [^ ]+)? code --repo",
        ])
        .output()
        .await;

    let lock_path = repo_path.join(".git/.agent-harness-code.lock");
    let _ = fs::remove_file(lock_path);

    // Clear stale OpenClaw harness session locks created by prior run-scoped sessions.
    if let Ok(home) = std::env::var("HOME") {
        let sessions_dir = Path::new(&home).join(".openclaw/agents/main/sessions");
        if let Ok(entries) = fs::read_dir(&sessions_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
                if name.starts_with("harness-run-") && name.ends_with(".jsonl.lock") {
                    let _ = fs::remove_file(path);
                }
            }
        }
    }

    let supercycle_dir = repo_path.join(".harness/supercycle");
    if supercycle_dir.exists() {
        fs::remove_dir_all(&supercycle_dir)?;
    }

    let runs_dir = repo_path.join("runs");
    if runs_dir.exists() {
        fs::remove_dir_all(&runs_dir)?;
    }

    Ok(())
}

async fn coding_mode(cfg: &AppConfig, args: CodingModeArgs) -> Result<()> {
    let duration_sec = parse_duration_seconds(&args.time).map_err(|e| anyhow!(e))?;
    let preset = parse_executor_preset(&args.executor)?;

    let user_prompt = match (args.prompt, args.prompt_file) {
        (Some(prompt), None) => Some(prompt),
        (None, Some(path)) => {
            let loaded = fs::read_to_string(&path)?;
            let trimmed = loaded.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        }
        (None, None) => None,
        (Some(_), Some(_)) => unreachable!("clap enforces prompt conflict"),
    };

    let summary = run_coding_loop(CodingRunArgs {
        repo_path: args.repo,
        duration_sec,
        heartbeat_sec: args.heartbeat_sec,
        cycle_pause_sec: args.cycle_pause_sec,
        supercycle: args.supercycle,
        research_budget_sec: args.research_budget_sec,
        planning_budget_sec: args.planning_budget_sec,
        require_commit_each_cycle: args.require_commit_each_cycle,
        preset,
        plan_cmd: args.plan_cmd,
        act_cmd: args.act_cmd,
        verify_cmd: args.verify_cmd,
        allow_cmd: args.allow_cmd,
        user_prompt,
        commit_each_cycle: args.commit_each_cycle,
        push_each_cycle: args.push_each_cycle,
        cycle_output_file: args.cycle_output_file,
        runtime_log_file: args.runtime_log_file,
        thought_log_file: args.thought_log_file,
        noop_streak_limit: args.noop_streak_limit,
        conformance_interval_unchanged: args.conformance_interval_unchanged,
        progress_file: args.progress_file,
        run_lock_file: args.run_lock_file,
        provider_cfg: cfg.provider.clone(),
        event_log_path: cfg.event_log_path.clone(),
    })
    .await?;

    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}

fn parse_executor_preset(input: &str) -> Result<ExecutorPreset> {
    match input {
        "shell" => Ok(ExecutorPreset::Shell),
        "cargo" => Ok(ExecutorPreset::Cargo),
        "openclaw" => Ok(ExecutorPreset::OpenClaw),
        "hermes" => Ok(ExecutorPreset::Hermes),
        other => Err(anyhow!(
            "invalid --executor '{other}' (expected 'shell', 'cargo', 'openclaw', or 'hermes')"
        )),
    }
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
    let provider_resolution = build_provider(&cfg.provider);
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "config": cfg,
            "provider_resolution": {
                "requested_kind": provider_resolution.requested_kind,
                "resolved_kind": provider_resolution.resolved_kind,
                "fallback_reason": provider_resolution.fallback_reason,
                "provider_name": provider_resolution.provider.name(),
            },
            "tool_specs": tools.specs(),
        }))?
    );
    Ok(())
}

async fn replay(
    path: Option<&str>,
    run_id: Option<&str>,
    latest_run: bool,
    cfg: &AppConfig,
) -> Result<()> {
    let p = path.unwrap_or(&cfg.event_log_path);
    let summary = replay_file_with_filter(
        p,
        &ReplayFilter {
            run_id: run_id.map(|s| s.to_string()),
            latest_run,
        },
    )?;
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}

async fn eval(
    path: Option<&str>,
    run_id: Option<&str>,
    latest_run: bool,
    cfg: &AppConfig,
) -> Result<()> {
    let p = path.unwrap_or(&cfg.event_log_path);
    let filter = ReplayFilter {
        run_id: run_id.map(|s| s.to_string()),
        latest_run,
    };
    let events = replay_events_file_with_filter(p, &filter)?;
    let summary = replay_file_with_filter(p, &filter)?;
    let report = evaluate_events(&summary, &events);
    println!("{}", serde_json::to_string_pretty(&report)?);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cli_parses_code_with_prompt() {
        let cli = Cli::try_parse_from([
            "agent-harness",
            "code",
            "--repo",
            ".",
            "--time",
            "1h",
            "--prompt",
            "focus on portability",
        ])
        .unwrap();

        match cli.command {
            Some(Commands::Code {
                repo,
                time,
                prompt,
                prompt_file,
                ..
            }) => {
                assert_eq!(repo, ".");
                assert_eq!(time, "1h");
                assert_eq!(prompt.as_deref(), Some("focus on portability"));
                assert!(prompt_file.is_none());
            }
            _ => panic!("expected code command"),
        }
    }

    #[test]
    fn cli_rejects_prompt_and_prompt_file_together() {
        let result = Cli::try_parse_from([
            "agent-harness",
            "code",
            "--repo",
            ".",
            "--time",
            "1h",
            "--prompt",
            "x",
            "--prompt-file",
            "./prompt.txt",
        ]);

        assert!(result.is_err());
    }

    #[test]
    fn parse_executor_preset_rejects_invalid() {
        assert!(parse_executor_preset("python").is_err());
    }

    #[test]
    fn parse_executor_preset_accepts_hermes() {
        assert!(matches!(
            parse_executor_preset("hermes").unwrap(),
            ExecutorPreset::Hermes
        ));
    }

    #[test]
    fn cli_parses_quick_prompt_shortcut() {
        let cli = Cli::try_parse_from(["agent-harness", "implement auth retries"]).unwrap();
        assert_eq!(cli.quick_prompt.as_deref(), Some("implement auth retries"));
        assert!(cli.command.is_none());
    }
}
