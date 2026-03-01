use crate::code::{
    CodeCycleEngine, CodeTask, GitApplyDiffApplier, ProviderCodePlanner, ProviderDiffGenerator,
};
use crate::config::ProviderConfig;
use crate::events::{kinds, now_unix, EventSink, HarnessEvent};
use crate::provider::{build_provider, Provider};
use crate::runtime_gate::RuntimeGate;
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use tokio::process::Command;
use tokio::time::{sleep, Duration};

const OUTPUT_TAIL_LIMIT: usize = 4_000;
const TASK_SELECTION_COOLDOWN_CYCLES: u64 = 2;
const TASK_NO_DIFF_ESCALATION_THRESHOLD: u64 = 2;

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
    pub cycle_output_file: Option<String>,
    pub runtime_log_file: Option<String>,
    pub thought_log_file: Option<String>,
    pub noop_streak_limit: u64,
    pub conformance_interval_unchanged: u64,
    pub progress_file: Option<String>,
    pub run_lock_file: Option<String>,
    pub provider_cfg: ProviderConfig,
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
    pub id: String,
    pub title: String,
    pub source: String,
    pub selected_line: String,
}

#[derive(Debug, Clone)]
struct RankedTaskCandidate {
    task: FeatureTask,
    score: i64,
    impact: i64,
    novelty: i64,
    cooldown_remaining: u64,
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

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
struct TaskProgressMemory {
    completed_roadmap_lines: BTreeSet<String>,
    attempted_task_ids: BTreeSet<String>,
    completed_task_ids: BTreeSet<String>,
    repeated_no_diff_task_id: Option<String>,
    repeated_no_diff_cycles: u64,
    source_escalation_count: u64,
    task_history: BTreeMap<String, TaskHistory>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
struct TaskHistory {
    selected_count: u64,
    last_selected_cycle: u64,
    last_outcome: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct EventCounters {
    noop_streak: u64,
    forced_mutation: u64,
    task_advanced: u64,
    source_escalation: u64,
}

struct RepoRunLock {
    path: PathBuf,
}

impl Drop for RepoRunLock {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.path);
    }
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
    let thought_log_path = resolve_output_file(&repo_path, args.thought_log_file.as_deref())?;
    let progress_path = resolve_output_file(&repo_path, args.progress_file.as_deref())?
        .unwrap_or_else(|| repo_path.join(".harness/coding-progress.json"));
    let lock_path = resolve_output_file(&repo_path, args.run_lock_file.as_deref())?
        .unwrap_or_else(|| repo_path.join(".git/.agent-harness-code.lock"));
    let noop_streak_limit = args.noop_streak_limit.max(1);
    let conformance_interval_unchanged = args.conformance_interval_unchanged.max(1);

    let executor = build_executor(&args)?;
    if !executor.policy().allows("git") {
        return Err(anyhow!(
            "coding cycle requires git in allowlist for mandatory cleanup commit/push"
        ));
    }

    let built_provider = build_provider(&args.provider_cfg);
    let provider_requested_kind = built_provider.requested_kind.clone();
    let provider_resolved_kind = built_provider.resolved_kind.clone();
    let provider_fallback_reason = built_provider.fallback_reason.clone();
    let provider: Arc<dyn Provider> = Arc::from(built_provider.provider);
    let code_engine = CodeCycleEngine::new(
        Arc::new(ProviderCodePlanner::new(provider.clone())),
        Arc::new(ProviderDiffGenerator::new(provider.clone())),
        Arc::new(GitApplyDiffApplier),
    );

    let sink = EventSink::new(&args.event_log_path)?;
    let _repo_lock = match acquire_repo_run_lock(&lock_path) {
        Ok(lock) => {
            sink.emit(
                &HarnessEvent::new(kinds::CODING_LOCK_ACQUIRED).with_data(json!({
                    "repo": repo_path.display().to_string(),
                    "lock_file": lock_path.display().to_string(),
                })),
            )?;
            lock
        }
        Err(err) => {
            let refusal = format!(
                "coding run refused (fail-fast lock): another coding run appears active for repo {} (lock: {})",
                repo_path.display(),
                lock_path.display()
            );
            sink.emit(
                &HarnessEvent::new(kinds::CODING_LOCK_EXISTS).with_data(json!({
                    "repo": repo_path.display().to_string(),
                    "lock_file": lock_path.display().to_string(),
                    "error": err.to_string(),
                    "refusal": refusal,
                    "fail_fast": true,
                    "exit_code": 1,
                })),
            )?;
            return Err(anyhow!(refusal));
        }
    };

    let mut progress_memory = load_progress_memory(&progress_path)?;
    let start_epoch = now_unix();
    let gate = RuntimeGate::new(start_epoch, args.duration_sec);

    sink.emit(&HarnessEvent::new(kinds::RUN_STARTED).with_data(json!({
        "mode": "coding",
        "repo": repo_path.display().to_string(),
        "duration_sec": args.duration_sec,
        "executor": executor.name(),
        "allowlisted_commands": executor.policy().allowlisted_commands.clone(),
        "provider_requested": provider_requested_kind.clone(),
        "provider_resolved": provider_resolved_kind.clone(),
        "provider_fallback_reason": provider_fallback_reason.clone(),
        "deadline_epoch": gate.deadline_epoch(),
        "prompt_provided": user_prompt.is_some(),
        "user_prompt": user_prompt.clone(),
        "noop_streak_limit": noop_streak_limit,
        "conformance_interval_unchanged": conformance_interval_unchanged,
        "progress_file": progress_path.display().to_string(),
        "run_lock_file": lock_path.display().to_string(),
    })))?;

    sink.emit(
        &HarnessEvent::new(kinds::CODING_RUN_STARTED).with_data(json!({
            "repo": repo_path.display().to_string(),
            "duration_sec": args.duration_sec,
            "executor": executor.name(),
            "provider_requested": provider_requested_kind.clone(),
            "provider_resolved": provider_resolved_kind.clone(),
            "provider_fallback_reason": provider_fallback_reason.clone(),
            "deadline_epoch": gate.deadline_epoch(),
            "prompt_provided": user_prompt.is_some(),
            "user_prompt": user_prompt.clone(),
            "noop_streak_limit": noop_streak_limit,
            "conformance_interval_unchanged": conformance_interval_unchanged,
        })),
    )?;

    let mut cycles_total = 0u64;
    let mut cycles_succeeded = 0u64;
    let mut cycles_failed = 0u64;
    let mut next_heartbeat_epoch = start_epoch;
    let mut noop_streak = 0u64;
    let mut unchanged_since_conformance = 0u64;
    let mut counters = EventCounters::default();

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

        let forced_mutation_cycle = noop_streak >= noop_streak_limit;
        let escalate_source =
            progress_memory.repeated_no_diff_cycles >= TASK_NO_DIFF_ESCALATION_THRESHOLD;
        let architecture_result = run_architecture_phase(
            &repo_path,
            &progress_memory,
            forced_mutation_cycle,
            cycles_total,
            escalate_source,
        )
        .await?;
        emit_phase_event(
            &sink,
            runtime_log_path.as_deref(),
            thought_log_path.as_deref(),
            &cycle_id,
            cycles_total,
            architecture_result.clone(),
            "feature",
        )?;
        let selected_task = architecture_result.selected_task.clone();
        if let Some(task) = selected_task.as_ref() {
            record_task_selection(&mut progress_memory, task, cycles_total);
            save_progress_memory(&progress_path, &progress_memory)?;
        }
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

        let codegen_result = if architecture_result.success && plan_result.success {
            run_codegen_stage(
                &code_engine,
                &ctx,
                selected_task.as_ref(),
                user_prompt.as_deref(),
            )
            .await
        } else {
            StageResult::skipped(WorkStage::Act, "architecture/plan stage failed")
        };

        let command_act_result =
            if architecture_result.success && plan_result.success && codegen_result.success {
                executor.act(&ctx).await
            } else if !codegen_result.success {
                StageResult::skipped(WorkStage::Act, "code-diff engine stage failed")
            } else {
                StageResult::skipped(WorkStage::Act, "architecture/plan stage failed")
            };

        let act_result = merge_stage_results(WorkStage::Act, codegen_result, command_act_result);

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
            thought_log_path.as_deref(),
            &cycle_id,
            cycles_total,
            feature_result.clone(),
            "conformance",
        )?;
        phase_results.push(feature_result);

        let mutated_this_cycle = repo_dirty(&repo_path).await.unwrap_or(false);
        let should_run_conformance = mutated_this_cycle
            || unchanged_since_conformance >= conformance_interval_unchanged.saturating_sub(1);

        let verify_result = if !(plan_result.success && act_result.success) {
            StageResult::skipped(WorkStage::Verify, "feature stage failed")
        } else if should_run_conformance {
            executor.verify(&ctx).await
        } else {
            sink.emit(
                &HarnessEvent::new(kinds::CODING_CONFORMANCE_SKIPPED)
                    .with_task_id(cycle_id.clone())
                    .with_data(json!({
                        "cycle": cycles_total,
                        "reason": "unchanged_window",
                        "interval": conformance_interval_unchanged,
                    })),
            )?;
            StageResult {
                stage: WorkStage::Verify,
                success: true,
                error: None,
                commands: Vec::new(),
            }
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
            thought_log_path.as_deref(),
            &cycle_id,
            cycles_total,
            conformance_result.clone(),
            "cleanup",
        )?;
        phase_results.push(conformance_result.clone());

        let pending_before_hooks = pending_file_names(&repo_path).await;
        let meaningful_diff_this_cycle =
            commit_has_meaningful_scope(&pending_before_hooks, selected_task.as_ref());

        let hook_results = run_cycle_hooks(
            &sink,
            &cycle_id,
            &repo_path,
            cycles_total,
            &args,
            output_path.as_deref(),
            user_prompt.as_deref(),
            executor.policy(),
            meaningful_diff_this_cycle,
            verify_result.success,
            selected_task.as_ref(),
        )
        .await?;

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
            thought_log_path.as_deref(),
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

        if meaningful_diff_this_cycle {
            noop_streak = 0;
        } else {
            noop_streak = noop_streak.saturating_add(1);
        }
        counters.noop_streak = noop_streak;

        if forced_mutation_cycle {
            counters.forced_mutation = counters.forced_mutation.saturating_add(1);
            if !meaningful_diff_this_cycle {
                sink.emit(
                    &HarnessEvent::new(kinds::TASK_FINISHED)
                        .with_task_id(cycle_id.clone())
                        .with_data(json!({
                            "mode": "coding",
                            "cycle": cycles_total,
                            "reason": "forced_scoped_task_no_meaningful_diff",
                            "runtime_ms": cycle_start.elapsed().as_millis() as u64,
                        })),
                )?;
                return Err(anyhow!(
                    "forced scoped code-change task produced no meaningful diff; aborting run"
                ));
            }
        }

        if meaningful_diff_this_cycle {
            unchanged_since_conformance = 0;
            if let Some(task) = selected_task.clone() {
                let task_key = format!("{}::{}", task.source, task.selected_line);
                let mut advanced = false;
                if progress_memory.completed_roadmap_lines.insert(task_key) {
                    advanced = true;
                }
                if progress_memory.completed_task_ids.insert(task.id.clone()) {
                    advanced = true;
                }
                progress_memory.repeated_no_diff_task_id = None;
                progress_memory.repeated_no_diff_cycles = 0;
                record_task_outcome(&mut progress_memory, &task.id, "meaningful_diff");
                save_progress_memory(&progress_path, &progress_memory)?;
                if advanced {
                    counters.task_advanced = counters.task_advanced.saturating_add(1);
                }
            }
        } else {
            if let Some(task) = selected_task.as_ref() {
                if progress_memory.repeated_no_diff_task_id.as_deref() == Some(task.id.as_str()) {
                    progress_memory.repeated_no_diff_cycles =
                        progress_memory.repeated_no_diff_cycles.saturating_add(1);
                } else {
                    progress_memory.repeated_no_diff_task_id = Some(task.id.clone());
                    progress_memory.repeated_no_diff_cycles = 1;
                }
                if progress_memory.repeated_no_diff_cycles > TASK_NO_DIFF_ESCALATION_THRESHOLD {
                    progress_memory.source_escalation_count =
                        progress_memory.source_escalation_count.saturating_add(1);
                }
                record_task_outcome(&mut progress_memory, &task.id, "no_diff");
                save_progress_memory(&progress_path, &progress_memory)?;
            }

            if should_run_conformance {
                unchanged_since_conformance = 0;
            } else {
                unchanged_since_conformance = unchanged_since_conformance.saturating_add(1);
            }
        }

        counters.source_escalation = progress_memory.source_escalation_count;
        sink.emit(
            &HarnessEvent::new(kinds::CODING_COUNTER)
                .with_task_id(cycle_id.clone())
                .with_data(serde_json::to_value(&counters)?),
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
                thought_log_path.as_deref(),
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

fn merge_stage_results(stage: WorkStage, first: StageResult, second: StageResult) -> StageResult {
    let mut commands = first.commands;
    commands.extend(second.commands);

    let success = first.success && second.success;
    let error = if first.success {
        second.error.or(first.error)
    } else {
        first.error.or(second.error)
    };

    StageResult {
        stage,
        success,
        error,
        commands,
    }
}

async fn run_codegen_stage(
    engine: &CodeCycleEngine,
    ctx: &CycleContext,
    selected_task: Option<&FeatureTask>,
    user_prompt: Option<&str>,
) -> StageResult {
    let stage_start = Instant::now();

    let Some(selected_task) = selected_task else {
        return StageResult {
            stage: WorkStage::Act,
            success: true,
            error: None,
            commands: vec![CommandExecution {
                stage: WorkStage::Act,
                command: "code-engine bypass (no selected task)".to_string(),
                argv: vec!["code-engine".to_string(), "skip".to_string()],
                success: true,
                exit_code: Some(0),
                duration_ms: 0,
                stdout_tail: "no selected architecture task; skipping code engine".to_string(),
                stderr_tail: String::new(),
                error: None,
            }],
        };
    };

    let repo_snapshot = match build_repo_snapshot(&ctx.repo_path, selected_task, user_prompt).await
    {
        Ok(snapshot) => snapshot,
        Err(err) => {
            let message = format!("failed to build repo snapshot for code engine: {err}");
            return StageResult {
                stage: WorkStage::Act,
                success: false,
                error: Some(message.clone()),
                commands: vec![CommandExecution {
                    stage: WorkStage::Act,
                    command: "code-engine plan->diff->apply".to_string(),
                    argv: vec!["code-engine".to_string(), "snapshot".to_string()],
                    success: false,
                    exit_code: Some(1),
                    duration_ms: stage_start.elapsed().as_millis() as u64,
                    stdout_tail: String::new(),
                    stderr_tail: truncate_tail(&message),
                    error: Some(message),
                }],
            };
        }
    };

    let task = build_code_task(selected_task, ctx, user_prompt);

    match engine
        .run_cycle(&ctx.repo_path, &task, &repo_snapshot)
        .await
    {
        Ok(report) => {
            let changed_files = report.diff_applied.changed_files.clone();
            let success = report.diff_applied.applied && !changed_files.is_empty();
            let detail = if success {
                format!(
                    "code engine applied patch touching {} files",
                    changed_files.len()
                )
            } else {
                format!(
                    "code-diff engine produced no applied file changes: {}",
                    report.diff_applied.detail
                )
            };

            let payload = json!({
                "task_id": report.task_id,
                "plan_summary": report.planned.summary,
                "diff_summary": report.diff_generated.summary,
                "touched_files": report.diff_generated.touched_files,
                "changed_files": changed_files,
                "apply_detail": report.diff_applied.detail,
            });

            StageResult {
                stage: WorkStage::Act,
                success,
                error: if success { None } else { Some(detail.clone()) },
                commands: vec![CommandExecution {
                    stage: WorkStage::Act,
                    command: "code-engine plan->diff->apply".to_string(),
                    argv: vec!["code-engine".to_string(), task.id.clone()],
                    success,
                    exit_code: Some(if success { 0 } else { 1 }),
                    duration_ms: stage_start.elapsed().as_millis() as u64,
                    stdout_tail: truncate_tail(&payload.to_string()),
                    stderr_tail: if success {
                        String::new()
                    } else {
                        truncate_tail(&detail)
                    },
                    error: if success { None } else { Some(detail) },
                }],
            }
        }
        Err(err) => {
            let message = format!("code-diff engine failed: {err}");
            StageResult {
                stage: WorkStage::Act,
                success: false,
                error: Some(message.clone()),
                commands: vec![CommandExecution {
                    stage: WorkStage::Act,
                    command: "code-engine plan->diff->apply".to_string(),
                    argv: vec!["code-engine".to_string(), task.id],
                    success: false,
                    exit_code: Some(1),
                    duration_ms: stage_start.elapsed().as_millis() as u64,
                    stdout_tail: String::new(),
                    stderr_tail: truncate_tail(&message),
                    error: Some(message),
                }],
            }
        }
    }
}

fn build_code_task(
    selected_task: &FeatureTask,
    ctx: &CycleContext,
    user_prompt: Option<&str>,
) -> CodeTask {
    let target_files = infer_target_files(selected_task);
    let mut objective = selected_task.title.clone();
    if let Some(prompt) = user_prompt.map(str::trim).filter(|p| !p.is_empty()) {
        objective = format!("{} | user prompt: {}", objective, prompt);
    }

    let mut constraints = vec![
        "Produce a valid unified diff patch.".to_string(),
        "Keep edits minimal and aligned to the selected task.".to_string(),
        "Avoid unrelated formatting churn.".to_string(),
    ];

    if selected_task.source.ends_with(".md") {
        constraints.push(
            "Prefer docs-scoped changes unless task text clearly requires code edits.".to_string(),
        );
    } else {
        constraints
            .push("Prioritize src/ implementation updates before docs-only edits.".to_string());
    }

    CodeTask {
        id: format!("{}::cycle-{}", selected_task.id, ctx.cycle),
        objective,
        architecture_goal: selected_task.selected_line.clone(),
        constraints,
        target_files,
        acceptance_criteria: vec![
            format!("Advance selected task: {}", selected_task.title),
            "Produce a non-empty meaningful git diff.".to_string(),
            "Keep project checks green after apply.".to_string(),
        ],
    }
}

fn infer_target_files(task: &FeatureTask) -> Vec<String> {
    let mut files = BTreeSet::new();

    if looks_like_repo_path(&task.source) {
        files.insert(task.source.clone());
    }

    for text in [&task.title, &task.selected_line] {
        for token in text.split_whitespace() {
            let cleaned = token.trim_matches(|c: char| {
                c == '`'
                    || c == ','
                    || c == ';'
                    || c == '.'
                    || c == ':'
                    || c == '"'
                    || c == '\''
                    || c == '('
                    || c == ')'
                    || c == '['
                    || c == ']'
            });
            if looks_like_repo_path(cleaned) {
                files.insert(cleaned.to_string());
            }
        }
    }

    if files.is_empty() {
        if task.source.ends_with(".md") {
            files.insert(task.source.clone());
        } else {
            files.insert("src/coding.rs".to_string());
        }
    }

    files.into_iter().collect()
}

fn looks_like_repo_path(candidate: &str) -> bool {
    if candidate.is_empty() || candidate.starts_with('-') {
        return false;
    }

    let has_supported_extension = [".rs", ".md", ".toml", ".sh", ".json", ".yaml", ".yml"]
        .iter()
        .any(|suffix| candidate.ends_with(suffix));

    has_supported_extension && (candidate.contains('/') || candidate.starts_with("Cargo"))
}

async fn build_repo_snapshot(
    repo_path: &Path,
    selected_task: &FeatureTask,
    user_prompt: Option<&str>,
) -> Result<String> {
    let branch = capture_git_output(repo_path, &["rev-parse", "--abbrev-ref", "HEAD"]).await?;
    let head = capture_git_output(repo_path, &["log", "-1", "--oneline"]).await?;
    let status = capture_git_output(repo_path, &["status", "--short"]).await?;
    let diff_stat = capture_git_output(repo_path, &["diff", "--stat"]).await?;
    let tracked_files = capture_git_output(repo_path, &["ls-files"]).await?;

    let tracked_preview = tracked_files
        .lines()
        .take(80)
        .collect::<Vec<_>>()
        .join("\n");

    Ok(format!(
        "branch={branch}\nhead={head}\nselected_task={}\nselected_source={}\nuser_prompt={}\nstatus:\n{}\ndiff_stat:\n{}\ntracked_files:\n{}",
        selected_task.title,
        selected_task.source,
        user_prompt.unwrap_or_default(),
        if status.is_empty() { "(clean)" } else { status.as_str() },
        if diff_stat.is_empty() {
            "(no unstaged diff)"
        } else {
            diff_stat.as_str()
        },
        if tracked_preview.is_empty() {
            "(no tracked files)"
        } else {
            tracked_preview.as_str()
        }
    ))
}

async fn capture_git_output(repo_path: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(repo_path)
        .output()
        .await
        .with_context(|| format!("git {} failed to execute", args.join(" ")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(anyhow!(
            "git {} failed: {}",
            args.join(" "),
            if stderr.is_empty() {
                format!("exit status {}", output.status)
            } else {
                stderr
            }
        ));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
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

#[allow(clippy::too_many_arguments)]
async fn run_cycle_hooks(
    sink: &EventSink,
    cycle_id: &str,
    repo_path: &Path,
    cycle: u64,
    args: &CodingRunArgs,
    output_path: Option<&Path>,
    user_prompt: Option<&str>,
    policy: &CommandPolicy,
    meaningful_cycle: bool,
    verify_success: bool,
    selected_task: Option<&FeatureTask>,
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

    if !verify_success {
        let detail = "verify stage failed".to_string();
        hooks.push(HookResult {
            name: "commit".to_string(),
            success: false,
            skipped: true,
            detail: detail.clone(),
        });
        emit_git_commit_event(
            sink, cycle_id, cycle, false, true, None, None, "skipped", &detail,
        )?;
        emit_git_push_event(
            sink,
            cycle_id,
            cycle,
            false,
            true,
            None,
            None,
            "skipped",
            "push skipped because commit did not run",
        )?;
        return Ok(hooks);
    }

    let should_try_vcs = args.commit_each_cycle || meaningful_cycle || args.push_each_cycle;
    if !should_try_vcs {
        let detail = "no meaningful cycle and commit_each_cycle disabled".to_string();
        hooks.push(HookResult {
            name: "commit".to_string(),
            success: true,
            skipped: true,
            detail: detail.clone(),
        });
        emit_git_commit_event(
            sink, cycle_id, cycle, true, true, None, None, "skipped", &detail,
        )?;
        emit_git_push_event(
            sink,
            cycle_id,
            cycle,
            true,
            true,
            None,
            None,
            "skipped",
            "push_each_cycle disabled and cycle not marked meaningful",
        )?;
        return Ok(hooks);
    }

    let hook_ctx = CycleContext {
        cycle,
        repo_path: repo_path.to_path_buf(),
        user_prompt: user_prompt.map(ToOwned::to_owned),
    };

    let mut dirty = repo_dirty(repo_path)
        .await
        .with_context(|| "failed to inspect git status before commit hook")?;

    if dirty {
        hooks.push(HookResult {
            name: "git_fetch".to_string(),
            success: true,
            skipped: true,
            detail: "skipped git fetch because local changes are pending commit".to_string(),
        });
        hooks.push(HookResult {
            name: "git_pull".to_string(),
            success: true,
            skipped: true,
            detail: "skipped git pull because local changes are pending commit".to_string(),
        });
        hooks.push(HookResult {
            name: "conflict_resolution".to_string(),
            success: true,
            skipped: true,
            detail: "skipped conflict check because pull was skipped".to_string(),
        });
    } else {
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
            let detail = "git fetch failed before commit".to_string();
            hooks.push(HookResult {
                name: "commit".to_string(),
                success: false,
                skipped: true,
                detail: detail.clone(),
            });
            emit_git_commit_event(
                sink, cycle_id, cycle, false, true, None, None, "blocked", &detail,
            )?;
            emit_git_push_event(
                sink,
                cycle_id,
                cycle,
                false,
                true,
                None,
                None,
                "blocked",
                "push skipped because git fetch failed",
            )?;
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
            let detail = if !pull_result.success {
                "git pull failed before commit".to_string()
            } else {
                "unresolved merge conflicts remain".to_string()
            };
            hooks.push(HookResult {
                name: "commit".to_string(),
                success: false,
                skipped: true,
                detail: detail.clone(),
            });
            emit_git_commit_event(
                sink, cycle_id, cycle, false, true, None, None, "blocked", &detail,
            )?;
            emit_git_push_event(
                sink,
                cycle_id,
                cycle,
                false,
                true,
                None,
                None,
                "blocked",
                "push skipped because pull/conflict gate did not pass",
            )?;
            return Ok(hooks);
        }

        dirty = repo_dirty(repo_path)
            .await
            .with_context(|| "failed to inspect git status before commit hook")?;
    }

    if !dirty {
        let detail = "no tracked changes".to_string();
        hooks.push(HookResult {
            name: "commit".to_string(),
            success: true,
            skipped: true,
            detail: detail.clone(),
        });
        emit_git_commit_event(
            sink, cycle_id, cycle, true, true, None, None, "skipped", &detail,
        )?;
        emit_git_push_event(
            sink,
            cycle_id,
            cycle,
            true,
            true,
            None,
            None,
            "skipped",
            "push skipped because no commit was created",
        )?;
        return Ok(hooks);
    }

    let pending_names = pending_file_names(repo_path).await;
    if !commit_has_meaningful_scope(&pending_names, selected_task) {
        let detail =
            "commit quality gate: non-meaningful fallback/materialization-only diff".to_string();
        hooks.push(HookResult {
            name: "commit".to_string(),
            success: true,
            skipped: true,
            detail: detail.clone(),
        });
        emit_git_commit_event(
            sink, cycle_id, cycle, true, true, None, None, "skipped", &detail,
        )?;
        emit_git_push_event(
            sink,
            cycle_id,
            cycle,
            true,
            true,
            None,
            None,
            "skipped",
            "push skipped because commit quality gate rejected pending diff",
        )?;
        return Ok(hooks);
    }

    let add_result = execute_command_line(WorkStage::Act, "git add -A", &hook_ctx, policy).await;
    if !add_result.success {
        let detail = format!("git add failed: {}", summarize_error(&add_result));
        hooks.push(HookResult {
            name: "commit".to_string(),
            success: false,
            skipped: false,
            detail: detail.clone(),
        });
        emit_git_commit_event(
            sink, cycle_id, cycle, false, false, None, None, "failed", &detail,
        )?;
        emit_git_push_event(
            sink,
            cycle_id,
            cycle,
            false,
            true,
            None,
            None,
            "blocked",
            "push skipped because git add failed",
        )?;
        return Ok(hooks);
    }

    let staged_names = staged_file_names(repo_path).await;
    if staged_names.is_empty() {
        let detail = "git add produced empty staged set".to_string();
        hooks.push(HookResult {
            name: "commit".to_string(),
            success: false,
            skipped: true,
            detail: detail.clone(),
        });
        emit_git_commit_event(
            sink, cycle_id, cycle, false, true, None, None, "rejected", &detail,
        )?;
        emit_git_push_event(
            sink,
            cycle_id,
            cycle,
            false,
            true,
            None,
            None,
            "blocked",
            "push skipped because commit had no staged files",
        )?;
        return Ok(hooks);
    }

    let commit_kind = infer_commit_kind(repo_path, args.user_prompt.as_deref()).await;
    let mut subject = summarize_commit_focus(repo_path, args.user_prompt.as_deref()).await;
    if commit_subject_is_generic(&subject)
        || !subject_mentions_changed_scope(&subject, &staged_names)
    {
        subject = deterministic_subject_from_files(&staged_names);
    }
    if commit_subject_is_generic(&subject)
        || !subject_mentions_changed_scope(&subject, &staged_names)
    {
        let detail = format!(
            "commit subject rejected by quality gate: '{}'",
            subject.trim()
        );
        hooks.push(HookResult {
            name: "commit".to_string(),
            success: false,
            skipped: true,
            detail: detail.clone(),
        });
        emit_git_commit_event(
            sink,
            cycle_id,
            cycle,
            false,
            true,
            Some(subject.as_str()),
            None,
            "rejected",
            &detail,
        )?;
        emit_git_push_event(
            sink,
            cycle_id,
            cycle,
            false,
            true,
            Some(subject.as_str()),
            None,
            "blocked",
            "push skipped because commit subject failed quality gate",
        )?;
        return Ok(hooks);
    }

    let deduped_subject = dedupe_subject(repo_path, &subject).await;
    if commit_subject_is_generic(&deduped_subject)
        || !subject_mentions_changed_scope(&deduped_subject, &staged_names)
    {
        let detail = format!(
            "de-duplicated commit subject rejected by quality gate: '{}'",
            deduped_subject.trim()
        );
        hooks.push(HookResult {
            name: "commit".to_string(),
            success: false,
            skipped: true,
            detail: detail.clone(),
        });
        emit_git_commit_event(
            sink,
            cycle_id,
            cycle,
            false,
            true,
            Some(deduped_subject.as_str()),
            None,
            "rejected",
            &detail,
        )?;
        emit_git_push_event(
            sink,
            cycle_id,
            cycle,
            false,
            true,
            Some(deduped_subject.as_str()),
            None,
            "blocked",
            "push skipped because de-duplicated subject failed quality gate",
        )?;
        return Ok(hooks);
    }

    let commit_subject = deduped_subject;
    let commit_message = format!("{}(harness): {}", commit_kind, commit_subject);

    let commit_cmd = format!("git commit -m {}", shell_words::quote(&commit_message));
    let commit_result = execute_command_line(WorkStage::Act, &commit_cmd, &hook_ctx, policy).await;
    let commit_detail = if commit_result.success {
        "git commit ok".to_string()
    } else {
        summarize_error(&commit_result)
    };
    emit_git_commit_event(
        sink,
        cycle_id,
        cycle,
        commit_result.success,
        false,
        Some(commit_subject.as_str()),
        Some(commit_message.as_str()),
        if commit_result.success {
            "ok"
        } else {
            "failed"
        },
        &commit_detail,
    )?;
    if !commit_result.success {
        hooks.push(HookResult {
            name: "commit".to_string(),
            success: false,
            skipped: false,
            detail: format!("git commit failed: {}", summarize_error(&commit_result)),
        });
        emit_git_push_event(
            sink,
            cycle_id,
            cycle,
            false,
            true,
            Some(commit_subject.as_str()),
            Some(commit_message.as_str()),
            "blocked",
            "push skipped because commit failed",
        )?;
        return Ok(hooks);
    }

    hooks.push(HookResult {
        name: "commit".to_string(),
        success: true,
        skipped: false,
        detail: commit_message.clone(),
    });

    if args.push_each_cycle || meaningful_cycle {
        let push_result = execute_command_line(WorkStage::Act, "git push", &hook_ctx, policy).await;
        let push_detail = if push_result.success {
            "git push ok".to_string()
        } else {
            summarize_error(&push_result)
        };
        emit_git_push_event(
            sink,
            cycle_id,
            cycle,
            push_result.success,
            false,
            Some(commit_subject.as_str()),
            Some(commit_message.as_str()),
            if push_result.success { "ok" } else { "failed" },
            &push_detail,
        )?;
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
    } else {
        hooks.push(HookResult {
            name: "push".to_string(),
            success: true,
            skipped: true,
            detail: "push_each_cycle disabled and cycle not marked meaningful".to_string(),
        });
        emit_git_push_event(
            sink,
            cycle_id,
            cycle,
            true,
            true,
            Some(commit_subject.as_str()),
            Some(commit_message.as_str()),
            "skipped",
            "push_each_cycle disabled and cycle not marked meaningful",
        )?;
    }

    Ok(hooks)
}

#[allow(clippy::too_many_arguments)]
fn emit_git_commit_event(
    sink: &EventSink,
    cycle_id: &str,
    cycle: u64,
    success: bool,
    skipped: bool,
    subject: Option<&str>,
    message: Option<&str>,
    result: &str,
    detail: &str,
) -> Result<()> {
    sink.emit(
        &HarnessEvent::new(kinds::GIT_COMMIT)
            .with_task_id(cycle_id.to_string())
            .with_data(json!({
                "cycle": cycle,
                "success": success,
                "skipped": skipped,
                "result": result,
                "subject": subject,
                "message": message,
                "detail": detail,
            })),
    )
}

#[allow(clippy::too_many_arguments)]
fn emit_git_push_event(
    sink: &EventSink,
    cycle_id: &str,
    cycle: u64,
    success: bool,
    skipped: bool,
    subject: Option<&str>,
    message: Option<&str>,
    result: &str,
    detail: &str,
) -> Result<()> {
    sink.emit(
        &HarnessEvent::new(kinds::GIT_PUSH)
            .with_task_id(cycle_id.to_string())
            .with_data(json!({
                "cycle": cycle,
                "success": success,
                "skipped": skipped,
                "result": result,
                "subject": subject,
                "message": message,
                "detail": detail,
            })),
    )
}

async fn run_architecture_phase(
    repo_path: &Path,
    progress: &TaskProgressMemory,
    force_mutation: bool,
    cycle: u64,
    escalate_source: bool,
) -> Result<PhaseResult> {
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

    let selected_task = if force_mutation {
        select_forced_code_change_task(progress, cycle)
    } else {
        select_next_feature_task_from_docs(repo_path, progress, cycle, escalate_source)
            .unwrap_or_else(|| select_forced_code_change_task(progress, cycle))
    };

    Ok(PhaseResult {
        phase: CyclePhase::Architecture,
        reason: "repo clean; rank/select next feature task from internal roadmap/docs".to_string(),
        selected_task: Some(selected_task.clone()),
        success: true,
        result: if force_mutation {
            format!(
                "forced concrete scoped code-change task '{}' ({}) after no-diff streak",
                selected_task.title, selected_task.source
            )
        } else {
            format!(
                "selected ranked task '{}' from {}",
                selected_task.title, selected_task.source
            )
        },
    })
}

fn select_next_feature_task_from_docs(
    repo_path: &Path,
    progress: &TaskProgressMemory,
    cycle: u64,
    escalate_source: bool,
) -> Option<FeatureTask> {
    let mut ranked = rank_task_candidates(repo_path, progress, cycle, escalate_source);
    ranked.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| b.impact.cmp(&a.impact))
            .then_with(|| b.novelty.cmp(&a.novelty))
            .then_with(|| a.task.id.cmp(&b.task.id))
    });

    ranked
        .iter()
        .find(|candidate| candidate.cooldown_remaining == 0 || escalate_source)
        .map(|candidate| candidate.task.clone())
        .or_else(|| ranked.first().map(|candidate| candidate.task.clone()))
}

fn rank_task_candidates(
    repo_path: &Path,
    progress: &TaskProgressMemory,
    cycle: u64,
    escalate_source: bool,
) -> Vec<RankedTaskCandidate> {
    let doc_tasks = collect_doc_tasks(repo_path, escalate_source);
    let has_doc_tasks = !doc_tasks.is_empty();

    let mut tasks = doc_tasks;
    tasks.extend(internal_fallback_tasks());

    let mut ranked = Vec::new();
    for task in tasks {
        if progress.completed_task_ids.contains(&task.id) {
            continue;
        }

        let history = progress
            .task_history
            .get(&task.id)
            .cloned()
            .unwrap_or_default();
        let cooldown_remaining = cooldown_remaining(cycle, history.last_selected_cycle);

        if progress.attempted_task_ids.contains(&task.id)
            && cooldown_remaining > 0
            && !escalate_source
        {
            continue;
        }

        let impact = task_impact_score(&task);
        let novelty = task_novelty_score(&task, progress, &history);
        let cooldown_penalty = (cooldown_remaining as i64) * 120;
        let repeat_penalty = (history.selected_count as i64) * 18;
        let fallback_penalty =
            if has_doc_tasks && task.id.starts_with("fallback::") && !escalate_source {
                260
            } else {
                0
            };
        let score = impact * 100 + novelty - cooldown_penalty - repeat_penalty - fallback_penalty;

        ranked.push(RankedTaskCandidate {
            task,
            score,
            impact,
            novelty,
            cooldown_remaining,
        });
    }

    ranked
}

fn collect_doc_tasks(repo_path: &Path, escalate_source: bool) -> Vec<FeatureTask> {
    let primary = ["ARCHITECTURE.md", "README.md", "RUNBOOK.md", "MIGRATION.md"];
    let fallback_only = ["CONTRIBUTING.md"];

    let mut files = primary.to_vec();
    if escalate_source {
        files.extend(fallback_only);
    }

    let mut tasks = Vec::new();
    for file in files {
        let path = repo_path.join(file);
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };

        for raw_line in content.lines() {
            let line = raw_line.trim();
            if !looks_like_roadmap_task(line) {
                continue;
            }

            let task_id = format!("{}::{}", file, slugify_task_line(line));
            tasks.push(FeatureTask {
                id: task_id,
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

    tasks
}

fn looks_like_roadmap_task(line: &str) -> bool {
    if line.is_empty() {
        return false;
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
        || roadmap_hint.contains("improve")
        || roadmap_hint.contains("harden")
        || roadmap_hint.contains("fix")
        || roadmap_hint.contains("refactor")
        || line.starts_with("- [ ]");

    is_actionable && looks_like_roadmap
}

fn internal_fallback_tasks() -> Vec<FeatureTask> {
    vec![
        FeatureTask {
            id: "fallback::src/coding.rs::tighten-commit-subject-gate".to_string(),
            title: "Tighten commit-subject quality gate in src/coding.rs".to_string(),
            source: "src/coding.rs".to_string(),
            selected_line: "Implement deterministic informative commit subjects for staged files"
                .to_string(),
        },
        FeatureTask {
            id: "fallback::src/coding.rs::improve-task-ranking".to_string(),
            title: "Improve task ranking/cooldown logic in src/coding.rs".to_string(),
            source: "src/coding.rs".to_string(),
            selected_line: "Rank architecture tasks by impact and novelty; avoid repeats"
                .to_string(),
        },
        FeatureTask {
            id: "fallback::src/main.rs::strengthen-lock-observability".to_string(),
            title: "Strengthen lock refusal observability in src/main.rs".to_string(),
            source: "src/main.rs".to_string(),
            selected_line: "Fail fast on concurrent runs with clear process exit reason"
                .to_string(),
        },
    ]
}

fn select_forced_code_change_task(progress: &TaskProgressMemory, cycle: u64) -> FeatureTask {
    let mut forced = internal_fallback_tasks()
        .into_iter()
        .filter(|task| task.source.starts_with("src/"))
        .collect::<Vec<_>>();

    forced.sort_by(|a, b| {
        let ah = progress
            .task_history
            .get(&a.id)
            .cloned()
            .unwrap_or_default();
        let bh = progress
            .task_history
            .get(&b.id)
            .cloned()
            .unwrap_or_default();

        ah.selected_count
            .cmp(&bh.selected_count)
            .then_with(|| ah.last_selected_cycle.cmp(&bh.last_selected_cycle))
            .then_with(|| a.id.cmp(&b.id))
    });

    let mut selected = forced.into_iter().next().unwrap_or(FeatureTask {
        id: "fallback::src/coding.rs::recover-no-diff-streak".to_string(),
        title: "Recover no-diff streak with scoped coding.rs change".to_string(),
        source: "src/coding.rs".to_string(),
        selected_line: "Apply a concrete code change to break no-diff streak".to_string(),
    });

    selected.id = format!("{}::forced-cycle-{}", selected.id, cycle);
    selected
}

fn cooldown_remaining(cycle: u64, last_selected_cycle: u64) -> u64 {
    if last_selected_cycle == 0 {
        return 0;
    }
    let age = cycle.saturating_sub(last_selected_cycle);
    TASK_SELECTION_COOLDOWN_CYCLES.saturating_sub(age)
}

fn task_novelty_score(
    task: &FeatureTask,
    progress: &TaskProgressMemory,
    history: &TaskHistory,
) -> i64 {
    let mut score = 30i64;

    if progress.attempted_task_ids.contains(&task.id) {
        score -= 20;
    }
    if let Some(outcome) = history.last_outcome.as_deref() {
        if outcome == "no_diff" {
            score -= 15;
        }
    }

    score - (history.selected_count as i64 * 4)
}

fn task_impact_score(task: &FeatureTask) -> i64 {
    let mut score = if task.source.starts_with("src/") {
        7
    } else {
        4
    };
    let text = format!("{} {}", task.title, task.selected_line).to_ascii_lowercase();

    for keyword in [
        "security",
        "harden",
        "fail",
        "abort",
        "lock",
        "concurrency",
        "correctness",
    ] {
        if text.contains(keyword) {
            score += 3;
        }
    }
    for keyword in [
        "test",
        "coverage",
        "regression",
        "observe",
        "event",
        "commit",
        "push",
    ] {
        if text.contains(keyword) {
            score += 2;
        }
    }
    for keyword in ["docs", "readme", "runbook"] {
        if text.contains(keyword) {
            score += 1;
        }
    }

    score
}

fn record_task_selection(progress: &mut TaskProgressMemory, task: &FeatureTask, cycle: u64) {
    progress.attempted_task_ids.insert(task.id.clone());
    let history = progress.task_history.entry(task.id.clone()).or_default();
    history.selected_count = history.selected_count.saturating_add(1);
    history.last_selected_cycle = cycle;
}

fn record_task_outcome(progress: &mut TaskProgressMemory, task_id: &str, outcome: &str) {
    let history = progress
        .task_history
        .entry(task_id.to_string())
        .or_default();
    history.last_outcome = Some(outcome.to_string());
}

fn slugify_task_line(line: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in line.chars() {
        let mapped = if ch.is_ascii_alphanumeric() {
            ch.to_ascii_lowercase()
        } else {
            '-'
        };
        if mapped == '-' {
            if !prev_dash {
                out.push(mapped);
                prev_dash = true;
            }
        } else {
            out.push(mapped);
            prev_dash = false;
        }
    }
    out.trim_matches('-').to_string()
}

fn emit_phase_event(
    sink: &EventSink,
    runtime_log_path: Option<&Path>,
    thought_log_path: Option<&Path>,
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
    if let Some(path) = thought_log_path {
        append_thought_log(path, cycle, result.phase, &data)?;
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

fn append_thought_log(
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
    writeln!(file, "## cycle {cycle} — {} ({ts})", phase.label())?;
    writeln!(file, "- reason: {reason}")?;
    writeln!(file, "- selected task: {selected_task}")?;
    writeln!(file, "- result: {result}")?;
    writeln!(file, "- next: {next}\n")?;

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

async fn summarize_commit_focus(repo_path: &Path, _user_prompt: Option<&str>) -> String {
    let files = staged_file_names(repo_path).await;
    deterministic_subject_from_files(&files)
}

async fn staged_file_names(repo_path: &Path) -> Vec<String> {
    let output = Command::new("git")
        .arg("diff")
        .arg("--cached")
        .arg("--name-only")
        .current_dir(repo_path)
        .output()
        .await;

    match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned)
            .collect(),
        _ => Vec::new(),
    }
}

async fn pending_file_names(repo_path: &Path) -> Vec<String> {
    let output = Command::new("git")
        .arg("status")
        .arg("--porcelain")
        .current_dir(repo_path)
        .output()
        .await;

    match output {
        Ok(out) if out.status.success() => String::from_utf8_lossy(&out.stdout)
            .lines()
            .filter_map(|line| {
                if line.len() > 3 {
                    Some(line[3..].trim().to_string())
                } else {
                    None
                }
            })
            .filter(|s| !s.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

fn deterministic_subject_from_files(files: &[String]) -> String {
    let mut names = files.to_vec();
    names.sort();
    names.dedup();

    let top = names
        .iter()
        .take(2)
        .map(|f| f.as_str())
        .collect::<Vec<_>>()
        .join(", ");

    let intent = if names.iter().any(|f| f.starts_with("src/")) {
        "implement scoped code updates"
    } else if names
        .iter()
        .any(|f| f.ends_with("README.md") || f.ends_with("RUNBOOK.md"))
    {
        "document operator workflow changes"
    } else if names
        .iter()
        .any(|f| f.contains("test") || f.contains("fixtures/"))
    {
        "add regression coverage"
    } else {
        "update harness workflow"
    };

    if top.is_empty() {
        format!("{} in tracked files", intent)
    } else {
        format!("{} in {}", intent, top)
    }
}

fn commit_subject_is_generic(subject: &str) -> bool {
    let normalized = normalize_subject_text(subject);
    if normalized.is_empty() {
        return true;
    }

    let blocked_patterns = [
        "generalizable",
        "build a generalizable",
        "harness coding cycle",
        "coding cycle",
        "advance harness workflow",
        "update code",
        "misc updates",
        "minor fixes",
    ];

    blocked_patterns
        .iter()
        .any(|pattern| normalized.contains(pattern))
}

fn normalize_subject_text(subject: &str) -> String {
    subject
        .to_ascii_lowercase()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { ' ' })
        .collect::<String>()
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

fn subject_mentions_changed_scope(subject: &str, files: &[String]) -> bool {
    let normalized_subject = normalize_subject_text(subject);
    if normalized_subject.is_empty() || files.is_empty() {
        return false;
    }

    files
        .iter()
        .flat_map(|file| scope_tokens_from_file(file))
        .any(|token| normalized_subject.contains(&token))
}

fn scope_tokens_from_file(file: &str) -> Vec<String> {
    let normalized = file
        .to_ascii_lowercase()
        .chars()
        .map(|ch| if ch.is_ascii_alphanumeric() { ch } else { ' ' })
        .collect::<String>();

    normalized
        .split_whitespace()
        .filter(|token| token.len() >= 3)
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>()
}

async fn dedupe_subject(repo_path: &Path, subject: &str) -> String {
    let out = Command::new("git")
        .arg("log")
        .arg("--pretty=%s")
        .arg("-n")
        .arg("12")
        .current_dir(repo_path)
        .output()
        .await;

    let mut candidate = subject.trim().to_string();
    let history = match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .lines()
            .map(str::trim)
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>(),
        _ => Vec::new(),
    };

    if history.iter().any(|h| h.ends_with(&candidate)) {
        candidate = format!("{} [cycle-refresh]", candidate);
    }
    candidate
}

fn commit_has_meaningful_scope(files: &[String], selected_task: Option<&FeatureTask>) -> bool {
    if files.is_empty() {
        return false;
    }

    let only_internal_state = files
        .iter()
        .all(|f| f.starts_with(".harness/") || f.starts_with("runs/"));
    if only_internal_state {
        return false;
    }

    let has_src = files.iter().any(|f| f.starts_with("src/"));
    let has_docs = files.iter().any(|f| {
        f.ends_with("README.md")
            || f.ends_with("RUNBOOK.md")
            || f.ends_with("ARCHITECTURE.md")
            || f.ends_with("MIGRATION.md")
    });

    if has_src {
        return true;
    }

    if has_docs {
        if let Some(task) = selected_task {
            return task.source.ends_with("README.md")
                || task.source.ends_with("RUNBOOK.md")
                || task.source.ends_with("ARCHITECTURE.md")
                || task.source.ends_with("MIGRATION.md");
        }
    }

    false
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
        .arg("--untracked-files=no")
        .current_dir(repo_path)
        .output()
        .await
        .context("git status --porcelain --untracked-files=no failed")?;

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

fn acquire_repo_run_lock(path: &Path) -> Result<RepoRunLock> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }

    OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(path)
        .with_context(|| format!("run lock exists at {}", path.display()))?;

    Ok(RepoRunLock {
        path: path.to_path_buf(),
    })
}

fn load_progress_memory(path: &Path) -> Result<TaskProgressMemory> {
    if !path.exists() {
        return Ok(TaskProgressMemory::default());
    }

    let content = fs::read_to_string(path)?;
    let parsed = serde_json::from_str::<TaskProgressMemory>(&content).unwrap_or_default();
    Ok(parsed)
}

fn save_progress_memory(path: &Path, progress: &TaskProgressMemory) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_string_pretty(progress)?)?;
    Ok(())
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
            cycle_output_file: Some(output.display().to_string()),
            runtime_log_file: None,
            thought_log_file: None,
            noop_streak_limit: 3,
            conformance_interval_unchanged: 3,
            progress_file: None,
            run_lock_file: None,
            provider_cfg: ProviderConfig::default(),
            event_log_path: dir.path().join("events.jsonl").display().to_string(),
        };

        let sink = EventSink::new(dir.path().join("events-hook.jsonl")).unwrap();
        let hooks = run_cycle_hooks(
            &sink,
            "cycle-1",
            dir.path(),
            1,
            &args,
            Some(output.as_path()),
            args.user_prompt.as_deref(),
            &policy,
            false,
            false,
            None,
        )
        .await
        .unwrap();

        assert_eq!(hooks.len(), 2);
        let content = fs::read_to_string(output).unwrap();
        assert!(content.contains("ship this"));
    }

    #[test]
    fn commit_subject_blocks_generic_templates_and_variants() {
        assert!(commit_subject_is_generic(
            "Build a generalizable pipeline — harness: coding cycle"
        ));
        assert!(commit_subject_is_generic("harness: coding cycle"));
        assert!(commit_subject_is_generic("minor fixes"));
        assert!(!commit_subject_is_generic(
            "implement scoped code updates in src/coding.rs"
        ));
    }

    #[test]
    fn commit_subject_must_reference_changed_scope() {
        let files = vec!["src/coding.rs".to_string(), "README.md".to_string()];
        assert!(subject_mentions_changed_scope(
            "implement scoped code updates in src/coding.rs",
            &files
        ));
        assert!(!subject_mentions_changed_scope(
            "improve workflow quality",
            &files
        ));
    }

    #[test]
    fn ranking_applies_cooldown_to_recently_selected_tasks() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("ARCHITECTURE.md"),
            "- [ ] Improve docs quality\n- [ ] Harden lock handling\n",
        )
        .unwrap();

        let mut progress = TaskProgressMemory::default();
        let first = select_next_feature_task_from_docs(dir.path(), &progress, 1, false).unwrap();
        record_task_selection(&mut progress, &first, 1);

        let second = select_next_feature_task_from_docs(dir.path(), &progress, 2, false).unwrap();
        assert_ne!(first.id, second.id);
    }

    #[test]
    fn second_lock_acquisition_fails_fast() {
        let dir = tempfile::tempdir().unwrap();
        let lock_path = dir.path().join(".git/agent.lock");
        let _lock = acquire_repo_run_lock(&lock_path).unwrap();
        let err = match acquire_repo_run_lock(&lock_path) {
            Ok(_) => panic!("second lock acquisition unexpectedly succeeded"),
            Err(err) => err,
        };
        assert!(err.to_string().contains("run lock exists"));
    }
}
