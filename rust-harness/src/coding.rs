use crate::events::{kinds, now_unix, EventSink, HarnessEvent};
use crate::runtime_gate::RuntimeGate;
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::Instant;
use tokio::process::Command;
use tokio::time::{sleep, Duration};

const OUTPUT_TAIL_LIMIT: usize = 4_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutorPreset {
    Shell,
    Cargo,
}

#[derive(Debug, Clone)]
pub struct CodingRunArgs {
    pub repo_path: String,
    pub duration_sec: u64,
    pub heartbeat_sec: u64,
    pub cycle_pause_sec: u64,
    pub preset: ExecutorPreset,
    pub plan_cmd: Vec<String>,
    pub act_cmd: Vec<String>,
    pub verify_cmd: Vec<String>,
    pub allow_cmd: Vec<String>,
    pub user_prompt: Option<String>,
    pub commit_each_cycle: bool,
    pub push_each_cycle: bool,
    pub commit_message_prefix: String,
    pub cycle_output_file: Option<String>,
    pub runtime_log_file: Option<String>,
    pub event_log_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodingRunSummary {
    pub run_id: String,
    pub repo_path: String,
    pub duration_sec: u64,
    pub elapsed_sec: u64,
    pub cycles_total: u64,
    pub cycles_succeeded: u64,
    pub cycles_failed: u64,
    pub executor: String,
    pub prompt_provided: bool,
    pub event_log: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandPolicy {
    pub allowlisted_commands: BTreeSet<String>,
}

impl CommandPolicy {
    pub fn default_safe() -> Self {
        Self {
            allowlisted_commands: ["cargo", "git"].into_iter().map(String::from).collect(),
        }
    }

    pub fn with_extra_commands(extra: &[String]) -> Self {
        let mut policy = Self::default_safe();
        for command in extra {
            policy.allowlisted_commands.insert(command.clone());
        }
        policy
    }

    pub fn allows(&self, command: &str) -> bool {
        self.allowlisted_commands.contains(command)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WorkStage {
    Plan,
    Act,
    Verify,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CyclePhase {
    Architecture,
    Feature,
    Conformance,
    Cleanup,
    Pause,
}

impl CyclePhase {
    fn label(self) -> &'static str {
        match self {
            CyclePhase::Architecture => "architecture",
            CyclePhase::Feature => "feature",
            CyclePhase::Conformance => "conformance",
            CyclePhase::Cleanup => "cleanup",
            CyclePhase::Pause => "pause",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeatureTask {
    pub title: String,
    pub source: String,
    pub selected_line: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhaseResult {
    pub phase: CyclePhase,
    pub reason: String,
    pub selected_task: Option<FeatureTask>,
    pub success: bool,
    pub result: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandExecution {
    pub stage: WorkStage,
    pub command: String,
    pub argv: Vec<String>,
    pub success: bool,
    pub exit_code: Option<i32>,
    pub duration_ms: u64,
    pub stdout_tail: String,
    pub stderr_tail: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StageResult {
    pub stage: WorkStage,
    pub success: bool,
    pub error: Option<String>,
    pub commands: Vec<CommandExecution>,
}

impl StageResult {
    fn skipped(stage: WorkStage, reason: impl Into<String>) -> Self {
        Self {
            stage,
            success: false,
            error: Some(reason.into()),
            commands: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HookResult {
    pub name: String,
    pub success: bool,
    pub skipped: bool,
    pub detail: String,
}

#[derive(Debug, Clone)]
pub struct CycleContext {
    pub cycle: u64,
    pub repo_path: PathBuf,
    pub user_prompt: Option<String>,
}

#[async_trait]
pub trait WorkExecutor: Send + Sync {
    fn name(&self) -> &'static str;
    fn policy(&self) -> &CommandPolicy;
    async fn plan(&self, ctx: &CycleContext) -> StageResult;
    async fn act(&self, ctx: &CycleContext) -> StageResult;
    async fn verify(&self, ctx: &CycleContext) -> StageResult;
}

#[derive(Debug, Clone)]
pub struct ShellWorkExecutor {
    pub policy: CommandPolicy,
    pub plan_cmd: Vec<String>,
    pub act_cmd: Vec<String>,
    pub verify_cmd: Vec<String>,
    pub label: &'static str,
}

#[async_trait]
impl WorkExecutor for ShellWorkExecutor {
    fn name(&self) -> &'static str {
        self.label
    }

    fn policy(&self) -> &CommandPolicy {
        &self.policy
    }

    async fn plan(&self, ctx: &CycleContext) -> StageResult {
        run_stage_commands(WorkStage::Plan, &self.plan_cmd, ctx, &self.policy).await
    }

    async fn act(&self, ctx: &CycleContext) -> StageResult {
        run_stage_commands(WorkStage::Act, &self.act_cmd, ctx, &self.policy).await
    }

    async fn verify(&self, ctx: &CycleContext) -> StageResult {
        run_stage_commands(WorkStage::Verify, &self.verify_cmd, ctx, &self.policy).await
    }
}

pub async fn run_coding_loop(args: CodingRunArgs) -> Result<CodingRunSummary> {
    if args.duration_sec == 0 {
        return Err(anyhow!("duration must be > 0"));
    }
    if args.heartbeat_sec == 0 {
        return Err(anyhow!("heartbeat interval must be > 0"));
    }

    let repo_path = PathBuf::from(&args.repo_path);
    if !repo_path.exists() || !repo_path.is_dir() {
        return Err(anyhow!("repo path does not exist or is not a directory"));
    }

    let user_prompt = args
        .user_prompt
        .as_deref()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned);

    let output_path = resolve_output_file(&repo_path, args.cycle_output_file.as_deref())?;
    let runtime_log_path = resolve_output_file(&repo_path, args.runtime_log_file.as_deref())?;
    let executor = build_executor(&args)?;
    if !executor.policy().allows("git") {
        return Err(anyhow!(
            "coding cycle requires git in allowlist for mandatory cleanup commit/push"
        ));
    }

    let sink = EventSink::new(&args.event_log_path)?;
    let start_epoch = now_unix();
    let gate = RuntimeGate::new(start_epoch, args.duration_sec);

    sink.emit(&HarnessEvent::new(kinds::RUN_STARTED).with_data(json!({
        "mode": "coding",
        "repo": repo_path.display().to_string(),
        "duration_sec": args.duration_sec,
        "executor": executor.name(),
        "allowlisted_commands": executor.policy().allowlisted_commands.clone(),
        "deadline_epoch": gate.deadline_epoch(),
        "prompt_provided": user_prompt.is_some(),
        "user_prompt": user_prompt.clone(),
    })))?;

    sink.emit(
        &HarnessEvent::new(kinds::CODING_RUN_STARTED).with_data(json!({
            "repo": repo_path.display().to_string(),
            "duration_sec": args.duration_sec,
            "executor": executor.name(),
            "deadline_epoch": gate.deadline_epoch(),
            "prompt_provided": user_prompt.is_some(),
            "user_prompt": user_prompt.clone(),
        })),
    )?;

    let mut cycles_total = 0u64;
    let mut cycles_succeeded = 0u64;
    let mut cycles_failed = 0u64;
    let mut next_heartbeat_epoch = start_epoch;

    while gate.is_active_at(now_unix()) {
        let now = now_unix();
        if now >= next_heartbeat_epoch {
            sink.emit(
                &HarnessEvent::new(kinds::CODING_HEARTBEAT).with_data(json!({
                    "elapsed_sec": gate.elapsed_sec_at(now),
                    "remaining_sec": gate.remaining_sec_at(now),
                    "deadline_epoch": gate.deadline_epoch(),
                    "cycles_total": cycles_total,
                    "prompt_provided": user_prompt.is_some(),
                })),
            )?;
            next_heartbeat_epoch = now.saturating_add(args.heartbeat_sec);
        }

        cycles_total += 1;
        let cycle_id = format!("cycle-{}", cycles_total);
        let cycle_start = Instant::now();

        let ctx = CycleContext {
            cycle: cycles_total,
            repo_path: repo_path.clone(),
            user_prompt: user_prompt.clone(),
        };

        sink.emit(
            &HarnessEvent::new(kinds::TASK_STARTED)
                .with_task_id(cycle_id.clone())
                .with_data(json!({
                    "mode": "coding",
                    "cycle": cycles_total,
                    "executor": executor.name(),
                    "remaining_sec": gate.remaining_sec_at(now_unix()),
                    "prompt_provided": user_prompt.is_some(),
                    "user_prompt": user_prompt.clone(),
                })),
        )?;

        sink.emit(
            &HarnessEvent::new(kinds::CODING_CYCLE_STARTED)
                .with_task_id(cycle_id.clone())
                .with_data(json!({
                    "cycle": cycles_total,
                    "deadline_epoch": gate.deadline_epoch(),
                    "remaining_sec": gate.remaining_sec_at(now_unix()),
                    "prompt_provided": user_prompt.is_some(),
                    "user_prompt": user_prompt.clone(),
                })),
        )?;

        let mut phase_results = Vec::new();

        let architecture_result = run_architecture_phase(&repo_path).await?;
        emit_phase_event(
            &sink,
            runtime_log_path.as_deref(),
            &cycle_id,
            cycles_total,
            architecture_result.clone(),
            "feature",
        )?;
        let selected_task = architecture_result.selected_task.clone();
        phase_results.push(architecture_result.clone());

        let plan_result = if architecture_result.success {
            executor.plan(&ctx).await
        } else {
            StageResult::skipped(WorkStage::Plan, "architecture phase failed")
        };
        sink.emit(
            &HarnessEvent::new(kinds::CODING_CYCLE_PLAN)
                .with_task_id(cycle_id.clone())
                .with_data(serde_json::to_value(&plan_result)?),
        )?;

        let feature_reason = selected_task
            .as_ref()
            .map(|task| format!("executing selected roadmap task '{}'", task.title))
            .unwrap_or_else(|| {
                "executing feature phase from current working tree state".to_string()
            });
        let act_result = if architecture_result.success && plan_result.success {
            executor.act(&ctx).await
        } else {
            StageResult::skipped(WorkStage::Act, "architecture/plan stage failed")
        };
        sink.emit(
            &HarnessEvent::new(kinds::CODING_CYCLE_ACT)
                .with_task_id(cycle_id.clone())
                .with_data(serde_json::to_value(&act_result)?),
        )?;
        let feature_result = PhaseResult {
            phase: CyclePhase::Feature,
            reason: feature_reason,
            selected_task: selected_task.clone(),
            success: act_result.success,
            result: if act_result.success {
                "feature phase completed".to_string()
            } else {
                act_result
                    .error
                    .clone()
                    .unwrap_or_else(|| "feature phase failed".to_string())
            },
        };
        emit_phase_event(
            &sink,
            runtime_log_path.as_deref(),
            &cycle_id,
            cycles_total,
            feature_result.clone(),
            "conformance",
        )?;
        phase_results.push(feature_result);

        let verify_result = if plan_result.success && act_result.success {
            executor.verify(&ctx).await
        } else {
            StageResult::skipped(WorkStage::Verify, "feature stage failed")
        };
        sink.emit(
            &HarnessEvent::new(kinds::CODING_CYCLE_VERIFY)
                .with_task_id(cycle_id.clone())
                .with_data(serde_json::to_value(&verify_result)?),
        )?;
        let conformance_result = PhaseResult {
            phase: CyclePhase::Conformance,
            reason: "run conformance checks for current cycle".to_string(),
            selected_task: selected_task.clone(),
            success: verify_result.success,
            result: if verify_result.success {
                "conformance checks passed".to_string()
            } else {
                verify_result
                    .error
                    .clone()
                    .unwrap_or_else(|| "conformance checks failed".to_string())
            },
        };
        emit_phase_event(
            &sink,
            runtime_log_path.as_deref(),
            &cycle_id,
            cycles_total,
            conformance_result.clone(),
            "cleanup",
        )?;
        phase_results.push(conformance_result.clone());

        let mut hook_results = Vec::new();
        if verify_result.success {
            hook_results = run_cycle_hooks(
                &repo_path,
                cycles_total,
                &args,
                output_path.as_deref(),
                user_prompt.as_deref(),
                executor.policy(),
                true,
            )
            .await?;
        } else if output_path.is_some() {
            hook_results.push(HookResult {
                name: "output".to_string(),
                success: false,
                skipped: true,
                detail: "verify stage failed".to_string(),
            });
        }

        let cleanup_success = hook_results.iter().all(|hook| hook.success || hook.skipped);
        let cleanup_result = PhaseResult {
            phase: CyclePhase::Cleanup,
            reason: "run cleanup hooks (output + commit/push when meaningful)".to_string(),
            selected_task: selected_task.clone(),
            success: cleanup_success,
            result: summarize_hooks(&hook_results),
        };
        emit_phase_event(
            &sink,
            runtime_log_path.as_deref(),
            &cycle_id,
            cycles_total,
            cleanup_result.clone(),
            "pause",
        )?;
        phase_results.push(cleanup_result);

        sink.emit(
            &HarnessEvent::new(kinds::CODING_CYCLE_HOOK)
                .with_task_id(cycle_id.clone())
                .with_data(json!({ "hooks": hook_results })),
        )?;

        let cycle_success = phase_results.iter().all(|phase| phase.success)
            && plan_result.success
            && act_result.success
            && verify_result.success
            && hook_results.iter().all(|hook| hook.success || hook.skipped);
        if cycle_success {
            cycles_succeeded += 1;
        } else {
            cycles_failed += 1;
        }

        let cycle_runtime_ms = cycle_start.elapsed().as_millis() as u64;
        sink.emit(
            &HarnessEvent::new(kinds::TASK_FINISHED)
                .with_task_id(cycle_id.clone())
                .with_data(json!({
                    "mode": "coding",
                    "cycle": cycles_total,
                    "reason": if cycle_success { "cycle_complete" } else { "cycle_failed" },
                    "runtime_ms": cycle_runtime_ms,
                })),
        )?;

        sink.emit(
            &HarnessEvent::new(kinds::CODING_CYCLE_FINISHED)
                .with_task_id(cycle_id.clone())
                .with_data(json!({
                    "cycle": cycles_total,
                    "success": cycle_success,
                    "runtime_ms": cycle_runtime_ms,
                    "remaining_sec": gate.remaining_sec_at(now_unix()),
                })),
        )?;

        if gate.is_active_at(now_unix()) {
            let pause_result = PhaseResult {
                phase: CyclePhase::Pause,
                reason: "mandatory cycle pause before next architecture phase".to_string(),
                selected_task,
                success: true,
                result: if args.cycle_pause_sec > 0 {
                    format!("slept {}s", args.cycle_pause_sec)
                } else {
                    "no sleep configured".to_string()
                },
            };
            emit_phase_event(
                &sink,
                runtime_log_path.as_deref(),
                &cycle_id,
                cycles_total,
                pause_result,
                "architecture",
            )?;

            if args.cycle_pause_sec > 0 {
                sleep(Duration::from_secs(args.cycle_pause_sec)).await;
            }
        }
    }

    let elapsed_sec = gate.elapsed_sec_at(now_unix());

    sink.emit(
        &HarnessEvent::new(kinds::CODING_RUN_FINISHED).with_data(json!({
            "cycles_total": cycles_total,
            "cycles_succeeded": cycles_succeeded,
            "cycles_failed": cycles_failed,
            "elapsed_sec": elapsed_sec,
            "duration_sec": args.duration_sec,
            "deadline_epoch": gate.deadline_epoch(),
        })),
    )?;

    sink.emit(&HarnessEvent::new(kinds::RUN_FINISHED).with_data(json!({
        "mode": "coding",
        "event_log": sink.path().display().to_string(),
        "run_id": sink.run_id(),
        "elapsed_sec": elapsed_sec,
    })))?;

    Ok(CodingRunSummary {
        run_id: sink.run_id().to_string(),
        repo_path: repo_path.display().to_string(),
        duration_sec: args.duration_sec,
        elapsed_sec,
        cycles_total,
        cycles_succeeded,
        cycles_failed,
        executor: executor.name().to_string(),
        prompt_provided: user_prompt.is_some(),
        event_log: sink.path().display().to_string(),
    })
}

fn build_executor(args: &CodingRunArgs) -> Result<Box<dyn WorkExecutor>> {
    let policy = CommandPolicy::with_extra_commands(&args.allow_cmd);

    let (plan_cmd, act_cmd, verify_cmd, label) = match args.preset {
        ExecutorPreset::Cargo => {
            let defaults = default_cargo_commands();
            (
                if args.plan_cmd.is_empty() {
                    defaults.plan
                } else {
                    args.plan_cmd.clone()
                },
                if args.act_cmd.is_empty() {
                    defaults.act
                } else {
                    args.act_cmd.clone()
                },
                if args.verify_cmd.is_empty() {
                    defaults.verify
                } else {
                    args.verify_cmd.clone()
                },
                "cargo",
            )
        }
        ExecutorPreset::Shell => (
            if args.plan_cmd.is_empty() {
                vec!["git status --short".to_string()]
            } else {
                args.plan_cmd.clone()
            },
            if args.act_cmd.is_empty() {
                vec!["git status --short".to_string()]
            } else {
                args.act_cmd.clone()
            },
            if args.verify_cmd.is_empty() {
                vec!["git diff --stat".to_string()]
            } else {
                args.verify_cmd.clone()
            },
            "shell",
        ),
    };

    let executor = ShellWorkExecutor {
        policy,
        plan_cmd,
        act_cmd,
        verify_cmd,
        label,
    };

    Ok(Box::new(executor))
}

#[derive(Debug)]
struct DefaultCommands {
    plan: Vec<String>,
    act: Vec<String>,
    verify: Vec<String>,
}

fn default_cargo_commands() -> DefaultCommands {
    DefaultCommands {
        plan: vec!["git status --short".to_string()],
        act: vec![
            "cargo fmt --all".to_string(),
            "cargo check --all-targets".to_string(),
        ],
        verify: vec!["cargo test --all-targets".to_string()],
    }
}

async fn run_cycle_hooks(
    repo_path: &Path,
    cycle: u64,
    args: &CodingRunArgs,
    output_path: Option<&Path>,
    user_prompt: Option<&str>,
    policy: &CommandPolicy,
    meaningful_cycle: bool,
) -> Result<Vec<HookResult>> {
    let mut hooks = Vec::new();

    if let Some(path) = output_path {
        let payload = json!({
            "cycle": cycle,
            "epoch": now_unix(),
            "repo": repo_path.display().to_string(),
            "user_prompt": user_prompt,
        });
        append_jsonl(path, &payload)?;
        hooks.push(HookResult {
            name: "output".to_string(),
            success: true,
            skipped: false,
            detail: path.display().to_string(),
        });
    }

    let should_commit_and_push = args.commit_each_cycle || meaningful_cycle;

    if should_commit_and_push {
        let hook_ctx = CycleContext {
            cycle,
            repo_path: repo_path.to_path_buf(),
            user_prompt: user_prompt.map(ToOwned::to_owned),
        };

        let fetch_result =
            execute_command_line(WorkStage::Act, "git fetch --all --prune", &hook_ctx, policy)
                .await;
        hooks.push(HookResult {
            name: "git_fetch".to_string(),
            success: fetch_result.success,
            skipped: false,
            detail: if fetch_result.success {
                "git fetch completed".to_string()
            } else {
                format!("git fetch failed: {}", summarize_error(&fetch_result))
            },
        });
        if !fetch_result.success {
            return Ok(hooks);
        }

        let pull_result =
            execute_command_line(WorkStage::Act, "git pull --ff-only", &hook_ctx, policy).await;
        let conflict_files = unresolved_conflicts(repo_path).await.unwrap_or_default();
        let pull_conflict = !pull_result.success && !conflict_files.is_empty();
        hooks.push(HookResult {
            name: "git_pull".to_string(),
            success: pull_result.success,
            skipped: false,
            detail: if pull_result.success {
                "git pull merged cleanly".to_string()
            } else if pull_conflict {
                format!(
                    "git pull conflict; unresolved files: {}",
                    conflict_files.join(", ")
                )
            } else {
                format!("git pull failed: {}", summarize_error(&pull_result))
            },
        });

        hooks.push(HookResult {
            name: "conflict_resolution".to_string(),
            success: conflict_files.is_empty(),
            skipped: false,
            detail: if conflict_files.is_empty() {
                "no unresolved conflicts".to_string()
            } else {
                format!("unresolved conflicts remain: {}", conflict_files.join(", "))
            },
        });

        if !pull_result.success || !conflict_files.is_empty() {
            return Ok(hooks);
        }

        let dirty = repo_dirty(repo_path)
            .await
            .with_context(|| "failed to inspect git status before commit hook")?;

        if !dirty {
            hooks.push(HookResult {
                name: "commit".to_string(),
                success: true,
                skipped: true,
                detail: "no tracked changes".to_string(),
            });
        } else {
            let add_result =
                execute_command_line(WorkStage::Act, "git add -A", &hook_ctx, policy).await;

            if !add_result.success {
                hooks.push(HookResult {
                    name: "commit".to_string(),
                    success: false,
                    skipped: false,
                    detail: format!("git add failed: {}", summarize_error(&add_result)),
                });
                return Ok(hooks);
            }

            let commit_kind = infer_commit_kind(repo_path, args.user_prompt.as_deref()).await;
            let message = format!(
                "{}(harness): {} [cycle {}]",
                commit_kind,
                summarize_commit_focus(args.user_prompt.as_deref()),
                cycle,
            );
            let commit_cmd = format!("git commit -m {}", shell_words::quote(&message));
            let commit_result =
                execute_command_line(WorkStage::Act, &commit_cmd, &hook_ctx, policy).await;
            if !commit_result.success {
                hooks.push(HookResult {
                    name: "commit".to_string(),
                    success: false,
                    skipped: false,
                    detail: format!("git commit failed: {}", summarize_error(&commit_result)),
                });
                return Ok(hooks);
            }

            hooks.push(HookResult {
                name: "commit".to_string(),
                success: true,
                skipped: false,
                detail: message,
            });

            let push_result =
                execute_command_line(WorkStage::Act, "git push", &hook_ctx, policy).await;
            hooks.push(HookResult {
                name: "push".to_string(),
                success: push_result.success,
                skipped: false,
                detail: if push_result.success {
                    "git push ok".to_string()
                } else {
                    format!("git push failed: {}", summarize_error(&push_result))
                },
            });
        }
    }

    Ok(hooks)
}

async fn run_architecture_phase(repo_path: &Path) -> Result<PhaseResult> {
    let dirty = repo_dirty(repo_path)
        .await
        .with_context(|| "failed to inspect git status during architecture phase")?;

    if dirty {
        return Ok(PhaseResult {
            phase: CyclePhase::Architecture,
            reason: "repo has pending changes; continue current feature thread".to_string(),
            selected_task: None,
            success: true,
            result: "working tree already dirty; reuse in-flight feature task".to_string(),
        });
    }

    let selected_task = select_next_feature_task_from_docs(repo_path).unwrap_or(FeatureTask {
        title: "Roadmap fallback: improve coding loop reliability".to_string(),
        source: "internal_default".to_string(),
        selected_line: "No roadmap bullet found; use reliability backlog".to_string(),
    });

    Ok(PhaseResult {
        phase: CyclePhase::Architecture,
        reason: "repo clean; select/build next feature task from internal roadmap/docs".to_string(),
        selected_task: Some(selected_task.clone()),
        success: true,
        result: format!(
            "selected task '{}' from {}",
            selected_task.title, selected_task.source
        ),
    })
}

fn select_next_feature_task_from_docs(repo_path: &Path) -> Option<FeatureTask> {
    let candidates = ["ARCHITECTURE.md", "README.md", "RUNBOOK.md", "MIGRATION.md"];
    for file in candidates {
        let path = repo_path.join(file);
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };

        for raw_line in content.lines() {
            let line = raw_line.trim();
            if line.is_empty() {
                continue;
            }

            let roadmap_hint = line.to_ascii_lowercase();
            let is_actionable = line.starts_with("- ")
                || line.starts_with("*")
                || line
                    .chars()
                    .next()
                    .map(|ch| ch.is_ascii_digit())
                    .unwrap_or(false);
            let looks_like_roadmap = roadmap_hint.contains("planned")
                || roadmap_hint.contains("next")
                || roadmap_hint.contains("todo")
                || roadmap_hint.contains("increment")
                || roadmap_hint.contains("feature")
                || line.starts_with("- [ ]");

            if is_actionable && looks_like_roadmap {
                return Some(FeatureTask {
                    title: line
                        .trim_start_matches('-')
                        .trim_start_matches('*')
                        .trim_start_matches("[ ]")
                        .trim()
                        .to_string(),
                    source: file.to_string(),
                    selected_line: line.to_string(),
                });
            }
        }
    }

    None
}

fn emit_phase_event(
    sink: &EventSink,
    runtime_log_path: Option<&Path>,
    cycle_id: &str,
    cycle: u64,
    result: PhaseResult,
    next_step: &str,
) -> Result<()> {
    let data = json!({
        "cycle": cycle,
        "phase": result.phase.label(),
        "reason": result.reason,
        "selected_task": result.selected_task,
        "success": result.success,
        "result": result.result,
        "next": next_step,
    });

    sink.emit(
        &HarnessEvent::new(kinds::CODING_PHASE)
            .with_task_id(cycle_id.to_string())
            .with_data(data.clone()),
    )?;

    if let Some(path) = runtime_log_path {
        append_runtime_log(path, cycle, result.phase, &data)?;
    }

    Ok(())
}

fn append_runtime_log(
    path: &Path,
    cycle: u64,
    phase: CyclePhase,
    data: &serde_json::Value,
) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    let ts = now_unix();
    let reason = data.get("reason").and_then(|v| v.as_str()).unwrap_or("n/a");
    let result = data.get("result").and_then(|v| v.as_str()).unwrap_or("n/a");
    let next = data.get("next").and_then(|v| v.as_str()).unwrap_or("n/a");
    let selected_task = data
        .get("selected_task")
        .and_then(|v| v.get("title"))
        .and_then(|v| v.as_str())
        .unwrap_or("none");

    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(
        file,
        "[{ts}] phase={} cycle={}\n- reason: {}\n- task: {}\n- result: {}\n- next: {}\n",
        phase.label(),
        cycle,
        reason,
        selected_task,
        result,
        next
    )?;

    Ok(())
}

fn summarize_hooks(hooks: &[HookResult]) -> String {
    if hooks.is_empty() {
        return "no hooks executed".to_string();
    }

    hooks
        .iter()
        .map(|hook| {
            if hook.skipped {
                format!("{}: skipped ({})", hook.name, hook.detail)
            } else if hook.success {
                format!("{}: ok ({})", hook.name, hook.detail)
            } else {
                format!("{}: failed ({})", hook.name, hook.detail)
            }
        })
        .collect::<Vec<_>>()
        .join("; ")
}

fn summarize_error(execution: &CommandExecution) -> String {
    execution
        .error
        .clone()
        .or_else(|| {
            if execution.stderr_tail.trim().is_empty() {
                None
            } else {
                Some(execution.stderr_tail.clone())
            }
        })
        .unwrap_or_else(|| "unknown error".to_string())
}

fn summarize_commit_focus(user_prompt: Option<&str>) -> String {
    let default = "automated harness update".to_string();
    let Some(prompt) = user_prompt else {
        return default;
    };

    let cleaned = prompt
        .split_whitespace()
        .take(12)
        .collect::<Vec<_>>()
        .join(" ")
        .trim()
        .trim_matches(|c: char| c == '"' || c == '\'' || c == '.' || c == '!')
        .to_lowercase();

    if cleaned.is_empty() {
        default
    } else {
        cleaned
    }
}

async fn infer_commit_kind(repo_path: &Path, user_prompt: Option<&str>) -> String {
    let prompt_lc = user_prompt.unwrap_or_default().to_lowercase();

    if ["fix", "bug", "error", "regression", "hotfix"]
        .iter()
        .any(|k| prompt_lc.contains(k))
    {
        return "fix".to_string();
    }
    if ["test", "tests", "coverage"]
        .iter()
        .any(|k| prompt_lc.contains(k))
    {
        return "test".to_string();
    }
    if ["refactor", "cleanup", "clean up"]
        .iter()
        .any(|k| prompt_lc.contains(k))
    {
        return "refactor".to_string();
    }
    if ["docs", "readme", "runbook", "documentation"]
        .iter()
        .any(|k| prompt_lc.contains(k))
    {
        return "docs".to_string();
    }

    let output = Command::new("git")
        .arg("diff")
        .arg("--cached")
        .arg("--name-only")
        .current_dir(repo_path)
        .output()
        .await;

    let files: Vec<String> = match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned)
            .collect(),
        _ => Vec::new(),
    };

    if !files.is_empty() && files.iter().all(|f| f.ends_with(".md")) {
        return "docs".to_string();
    }
    if files
        .iter()
        .any(|f| f.contains("test") || f.contains("fixtures/"))
    {
        return "test".to_string();
    }
    if files.iter().any(|f| f.starts_with("src/")) {
        return "feat".to_string();
    }

    "chore".to_string()
}

async fn unresolved_conflicts(repo_path: &Path) -> Result<Vec<String>> {
    let output = Command::new("git")
        .arg("diff")
        .arg("--name-only")
        .arg("--diff-filter=U")
        .current_dir(repo_path)
        .output()
        .await
        .context("git diff --name-only --diff-filter=U failed")?;

    if !output.status.success() {
        return Err(anyhow!(
            "git diff for conflict status failed: {}",
            truncate_tail(String::from_utf8_lossy(&output.stderr).as_ref())
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
        .collect())
}

async fn repo_dirty(repo_path: &Path) -> Result<bool> {
    let output = Command::new("git")
        .arg("status")
        .arg("--porcelain")
        .current_dir(repo_path)
        .output()
        .await
        .context("git status --porcelain failed")?;

    if !output.status.success() {
        return Err(anyhow!(
            "git status failed: {}",
            truncate_tail(String::from_utf8_lossy(&output.stderr).as_ref())
        ));
    }

    Ok(!String::from_utf8_lossy(&output.stdout).trim().is_empty())
}

async fn run_stage_commands(
    stage: WorkStage,
    commands: &[String],
    ctx: &CycleContext,
    policy: &CommandPolicy,
) -> StageResult {
    let mut runs = Vec::new();

    for command in commands {
        let execution = execute_command_line(stage, command, ctx, policy).await;
        let success = execution.success;
        let error = execution.error.clone();
        runs.push(execution);
        if !success {
            return StageResult {
                stage,
                success: false,
                error,
                commands: runs,
            };
        }
    }

    StageResult {
        stage,
        success: true,
        error: None,
        commands: runs,
    }
}

async fn execute_command_line(
    stage: WorkStage,
    command: &str,
    ctx: &CycleContext,
    policy: &CommandPolicy,
) -> CommandExecution {
    let stage_start = Instant::now();

    let argv = match shell_words::split(command) {
        Ok(parts) if !parts.is_empty() => parts,
        Ok(_) => {
            return CommandExecution {
                stage,
                command: command.to_string(),
                argv: Vec::new(),
                success: false,
                exit_code: None,
                duration_ms: stage_start.elapsed().as_millis() as u64,
                stdout_tail: String::new(),
                stderr_tail: String::new(),
                error: Some("empty command".to_string()),
            }
        }
        Err(err) => {
            return CommandExecution {
                stage,
                command: command.to_string(),
                argv: Vec::new(),
                success: false,
                exit_code: None,
                duration_ms: stage_start.elapsed().as_millis() as u64,
                stdout_tail: String::new(),
                stderr_tail: String::new(),
                error: Some(format!("command parse error: {err}")),
            }
        }
    };

    let executable = argv[0].clone();
    if !policy.allows(&executable) {
        return CommandExecution {
            stage,
            command: command.to_string(),
            argv,
            success: false,
            exit_code: None,
            duration_ms: stage_start.elapsed().as_millis() as u64,
            stdout_tail: String::new(),
            stderr_tail: String::new(),
            error: Some(format!(
                "command blocked by policy: {executable} (allowlist: {})",
                policy
                    .allowlisted_commands
                    .iter()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(",")
            )),
        };
    }

    let mut command_handle = Command::new(&executable);
    command_handle
        .args(&argv[1..])
        .current_dir(&ctx.repo_path)
        .env("OPENCLAW_CODING_CYCLE", ctx.cycle.to_string())
        .env("OPENCLAW_CODING_STAGE", stage_label(stage));

    if let Some(prompt) = ctx.user_prompt.as_deref() {
        command_handle.env("OPENCLAW_USER_PROMPT", prompt);
    }

    let output = command_handle.output().await;

    match output {
        Ok(output) => CommandExecution {
            stage,
            command: command.to_string(),
            argv,
            success: output.status.success(),
            exit_code: output.status.code(),
            duration_ms: stage_start.elapsed().as_millis() as u64,
            stdout_tail: truncate_tail(String::from_utf8_lossy(&output.stdout).as_ref()),
            stderr_tail: truncate_tail(String::from_utf8_lossy(&output.stderr).as_ref()),
            error: if output.status.success() {
                None
            } else {
                Some(format!("command exited with status {}", output.status))
            },
        },
        Err(err) => CommandExecution {
            stage,
            command: command.to_string(),
            argv,
            success: false,
            exit_code: None,
            duration_ms: stage_start.elapsed().as_millis() as u64,
            stdout_tail: String::new(),
            stderr_tail: String::new(),
            error: Some(format!("command execution error: {err}")),
        },
    }
}

fn stage_label(stage: WorkStage) -> &'static str {
    match stage {
        WorkStage::Plan => "plan",
        WorkStage::Act => "act",
        WorkStage::Verify => "verify",
    }
}

fn truncate_tail(content: &str) -> String {
    if content.chars().count() <= OUTPUT_TAIL_LIMIT {
        return content.to_string();
    }
    let mut chars = content
        .chars()
        .rev()
        .take(OUTPUT_TAIL_LIMIT)
        .collect::<Vec<_>>();
    chars.reverse();
    format!("...{}", chars.into_iter().collect::<String>())
}

fn append_jsonl(path: &Path, value: &serde_json::Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "{}", serde_json::to_string(value)?)?;
    Ok(())
}

fn resolve_output_file(repo_path: &Path, output_file: Option<&str>) -> Result<Option<PathBuf>> {
    let Some(path) = output_file else {
        return Ok(None);
    };

    let raw = PathBuf::from(path);
    let resolved = if raw.is_absolute() {
        raw
    } else {
        repo_path.join(raw)
    };

    if let Some(parent) = resolved.parent() {
        fs::create_dir_all(parent)?;
    }

    Ok(Some(resolved))
}

pub fn parse_duration_seconds(input: &str) -> std::result::Result<u64, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("duration cannot be empty".to_string());
    }

    if let Ok(seconds) = input.parse::<u64>() {
        if seconds == 0 {
            return Err("duration must be > 0".to_string());
        }
        return Ok(seconds);
    }

    let split_at = input
        .char_indices()
        .find(|(_, ch)| !ch.is_ascii_digit())
        .map(|(idx, _)| idx)
        .ok_or_else(|| format!("invalid duration: {input}"))?;

    let (num_part, unit_part) = input.split_at(split_at);
    if num_part.is_empty() {
        return Err(format!("invalid duration: {input}"));
    }

    let quantity = num_part
        .parse::<u64>()
        .map_err(|_| format!("invalid duration number: {num_part}"))?;

    if quantity == 0 {
        return Err("duration must be > 0".to_string());
    }

    let unit = unit_part.trim().to_ascii_lowercase();
    match unit.as_str() {
        "s" => Ok(quantity),
        "m" => Ok(quantity.saturating_mul(60)),
        "h" => Ok(quantity.saturating_mul(3600)),
        _ => Err(format!(
            "invalid duration unit '{unit_part}' (use seconds or suffix s/m/h)"
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_duration_supports_seconds_minutes_and_hours() {
        assert_eq!(parse_duration_seconds("3600").unwrap(), 3600);
        assert_eq!(parse_duration_seconds("45s").unwrap(), 45);
        assert_eq!(parse_duration_seconds("5m").unwrap(), 300);
        assert_eq!(parse_duration_seconds("2h").unwrap(), 7200);
    }

    #[test]
    fn parse_duration_rejects_invalid_values() {
        assert!(parse_duration_seconds("0").is_err());
        assert!(parse_duration_seconds("10d").is_err());
        assert!(parse_duration_seconds("abc").is_err());
    }

    #[test]
    fn command_policy_allows_defaults_and_rejects_unknown() {
        let policy = CommandPolicy::default_safe();
        assert!(policy.allows("cargo"));
        assert!(policy.allows("git"));
        assert!(!policy.allows("rm"));
    }

    #[tokio::test]
    async fn stage_fails_for_blocked_command() {
        let policy = CommandPolicy::default_safe();
        let dir = tempfile::tempdir().unwrap();
        let ctx = CycleContext {
            cycle: 1,
            repo_path: dir.path().to_path_buf(),
            user_prompt: None,
        };
        let stage =
            run_stage_commands(WorkStage::Act, &["echo hi".to_string()], &ctx, &policy).await;

        assert!(!stage.success);
        assert_eq!(stage.commands.len(), 1);
        assert!(stage.commands[0]
            .error
            .as_ref()
            .unwrap()
            .contains("blocked by policy"));
    }

    #[tokio::test]
    async fn output_hook_includes_user_prompt() {
        let dir = tempfile::tempdir().unwrap();
        let output = dir.path().join("out/events.jsonl");
        let policy = CommandPolicy::default_safe();
        let args = CodingRunArgs {
            repo_path: dir.path().display().to_string(),
            duration_sec: 1,
            heartbeat_sec: 1,
            cycle_pause_sec: 0,
            preset: ExecutorPreset::Shell,
            plan_cmd: vec![],
            act_cmd: vec![],
            verify_cmd: vec![],
            allow_cmd: vec![],
            user_prompt: Some("ship this".to_string()),
            commit_each_cycle: false,
            push_each_cycle: false,
            commit_message_prefix: "test".to_string(),
            cycle_output_file: Some(output.display().to_string()),
            runtime_log_file: None,
            event_log_path: dir.path().join("events.jsonl").display().to_string(),
        };

        let hooks = run_cycle_hooks(
            dir.path(),
            1,
            &args,
            Some(output.as_path()),
            args.user_prompt.as_deref(),
            &policy,
            false,
        )
        .await
        .unwrap();

        assert_eq!(hooks.len(), 1);
        let content = fs::read_to_string(output).unwrap();
        assert!(content.contains("ship this"));
    }
}
