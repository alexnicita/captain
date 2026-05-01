use crate::code::{
    CodeCycleEngine, CodeDiffApplier, CodeDiffProposal, CodeTask, GitApplyDiffApplier,
    ProviderCodePlanner, ProviderDiffGenerator,
};
use crate::coding_tasks::{
    ensure_roadmap_seed_tasks, record_task_outcome, record_task_selection,
    select_forced_code_change_task, select_next_feature_task_from_docs, TaskProgressMemory,
};
use crate::commit_subject_quality::{
    deterministic_subject_from_files, has_informative_subject_scope, is_generic_subject,
};
use crate::config::ProviderConfig;
pub use crate::duration::parse_duration_seconds;
use crate::error_taxonomy::ErrorClassifier;
use crate::events::{kinds, now_unix, EventSink, HarnessEvent};
use crate::model_profile::ModelProfile;
use crate::provider::{build_provider, Provider};
use crate::runtime_gate::RuntimeGate;
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use tokio::process::Command;
use tokio::time::{sleep, Duration};

const OUTPUT_TAIL_LIMIT: usize = 4_000;
const TASK_NO_DIFF_ESCALATION_THRESHOLD: u64 = 2;
const CODEGEN_MAX_ATTEMPTS: usize = 3;
const AGENT_CLI_ATTEMPTS_PER_CYCLE: usize = 1;
const AGENT_CLI_PLAN_TIMEOUT_SEC: u64 = 240;
const AGENT_CLI_CODE_TIMEOUT_SEC: u64 = 600;
const DIFF_RUBRIC_REJECTION_STREAK_LIMIT: u64 = 3;
const PRECOMMIT_GATE_SKIP_STREAK_LIMIT: u64 = 2;
const COMMIT_WATCHDOG_SEC: u64 = 600;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutorPreset {
    Shell,
    Cargo,
    OpenClaw,
    Hermes,
}

#[derive(Debug, Clone)]
pub struct CodingRunArgs {
    pub repo_path: String,
    pub duration_sec: u64,
    pub heartbeat_sec: u64,
    pub cycle_pause_sec: u64,
    pub supercycle: bool,
    pub research_budget_sec: u64,
    pub planning_budget_sec: u64,
    pub require_commit_each_cycle: bool,
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

    fn passed(stage: WorkStage) -> Self {
        Self {
            stage,
            success: true,
            error: None,
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

#[derive(Debug, Clone, Serialize)]
struct WatchdogRecovery {
    success: bool,
    subject: String,
    message: String,
    commit_detail: String,
    push_attempted: bool,
    push_success: Option<bool>,
    push_detail: Option<String>,
}

#[derive(Debug, Clone)]
struct AgentExecutionPlan {
    target_files: Vec<String>,
    target_symbols: Vec<String>,
    behavior_delta: String,
    test_delta: String,
    commit_subject: String,
    raw_json: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AgentCliKind {
    OpenClaw,
    Hermes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct AgentCli {
    kind: AgentCliKind,
}

impl AgentCli {
    fn openclaw() -> Self {
        Self {
            kind: AgentCliKind::OpenClaw,
        }
    }

    fn hermes() -> Self {
        Self {
            kind: AgentCliKind::Hermes,
        }
    }

    fn for_preset(preset: ExecutorPreset) -> Option<Self> {
        match preset {
            ExecutorPreset::OpenClaw => Some(Self::openclaw()),
            ExecutorPreset::Hermes => Some(Self::hermes()),
            ExecutorPreset::Shell | ExecutorPreset::Cargo => None,
        }
    }

    fn name(self) -> &'static str {
        match self.kind {
            AgentCliKind::OpenClaw => "openclaw",
            AgentCliKind::Hermes => "hermes",
        }
    }

    fn binary(self) -> &'static str {
        self.name()
    }

    fn argv_preview(self) -> Vec<String> {
        match self.kind {
            AgentCliKind::OpenClaw => vec!["openclaw".to_string(), "agent".to_string()],
            AgentCliKind::Hermes => vec!["hermes".to_string(), "chat".to_string()],
        }
    }

    fn command_label(self, label: &str) -> String {
        match self.kind {
            AgentCliKind::OpenClaw => format!("openclaw agent {label}"),
            AgentCliKind::Hermes => format!("hermes chat {label}"),
        }
    }

    async fn run_text_once(
        self,
        repo_path: &Path,
        prompt: &str,
        session_id: &str,
        timeout_sec: u64,
    ) -> Result<String> {
        let (payload, _) = self
            .run_once_with_timeout(repo_path, prompt, session_id, timeout_sec)
            .await?;
        Ok(payload)
    }

    async fn run_code_once(
        self,
        repo_path: &Path,
        prompt: &str,
        session_id: &str,
    ) -> Result<(String, String)> {
        self.run_once_with_timeout(repo_path, prompt, session_id, AGENT_CLI_CODE_TIMEOUT_SEC)
            .await
    }

    async fn run_once_with_timeout(
        self,
        repo_path: &Path,
        prompt: &str,
        session_id: &str,
        timeout_sec: u64,
    ) -> Result<(String, String)> {
        let mut command = self.command(prompt, session_id, timeout_sec);

        let output = tokio::time::timeout(
            Duration::from_secs(timeout_sec),
            command.current_dir(repo_path).output(),
        )
        .await
        .map_err(|_| anyhow!("{} agent timed out after {}s", self.name(), timeout_sec))?
        .with_context(|| format!("failed to execute {} agent", self.name()))?;

        let stdout_text = String::from_utf8_lossy(&output.stdout).to_string();
        let stderr_text = String::from_utf8_lossy(&output.stderr).to_string();
        if !output.status.success() {
            return Err(anyhow!(
                "{} agent exited with {}: {}",
                self.name(),
                output.status,
                truncate_tail(&stderr_text)
            ));
        }

        let payload = self
            .extract_payload_text(&stdout_text)
            .unwrap_or(stdout_text);
        Ok((payload, stderr_text))
    }

    fn command(self, prompt: &str, session_id: &str, timeout_sec: u64) -> Command {
        match self.kind {
            AgentCliKind::OpenClaw => {
                let mut command = Command::new("openclaw");
                command
                    .arg("agent")
                    .arg("--local")
                    .arg("--agent")
                    .arg("main")
                    .arg("--session-id")
                    .arg(session_id)
                    .arg("--timeout")
                    .arg(timeout_sec.to_string())
                    .arg("--thinking")
                    .arg("low")
                    .arg("--json")
                    .arg("--message")
                    .arg(prompt);
                command
            }
            AgentCliKind::Hermes => {
                let toolsets = std::env::var("CAPTAIN_HERMES_TOOLSETS")
                    .ok()
                    .map(|value| value.trim().to_string())
                    .filter(|value| !value.is_empty())
                    .unwrap_or_else(|| "terminal,skills".to_string());

                let mut command = Command::new("hermes");
                command
                    .arg("chat")
                    .arg("--quiet")
                    .arg("--source")
                    .arg("captain-harness")
                    .arg("--max-turns")
                    .arg("90")
                    .arg("--toolsets")
                    .arg(toolsets)
                    .arg("--pass-session-id")
                    .arg("--yolo")
                    .arg("-q")
                    .arg(prompt);
                command
            }
        }
    }

    fn extract_payload_text(self, stdout: &str) -> Option<String> {
        match self.kind {
            AgentCliKind::OpenClaw => extract_openclaw_payload_text(stdout),
            AgentCliKind::Hermes => {
                let trimmed = stdout.trim();
                if trimmed.is_empty() {
                    None
                } else {
                    Some(trimmed.to_string())
                }
            }
        }
    }
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
    pub selected_task: Option<FeatureTask>,
    pub run_session_id: String,
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
        let commands = if self.verify_cmd.is_empty() {
            derive_verify_commands(ctx)
        } else {
            self.verify_cmd.clone()
        };
        run_stage_commands(WorkStage::Verify, &commands, ctx, &self.policy).await
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
        .unwrap_or_else(|| default_repo_run_lock_path(&repo_path));
    let noop_streak_limit = args.noop_streak_limit.max(1);
    let conformance_interval_unchanged = args.conformance_interval_unchanged.max(1);

    let agent_cli = AgentCli::for_preset(args.preset);
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
    let model_profile = ModelProfile::for_model(&args.provider_cfg.model);
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
    let run_session_id = format!("harness-run-{start_epoch}");
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
        "model": args.provider_cfg.model.clone(),
        "model_profile": model_profile.clone(),
        "deadline_epoch": gate.deadline_epoch(),
        "prompt_provided": user_prompt.is_some(),
        "user_prompt": user_prompt.clone(),
        "supercycle": args.supercycle,
        "research_budget_sec": args.research_budget_sec,
        "planning_budget_sec": args.planning_budget_sec,
        "require_commit_each_cycle": args.require_commit_each_cycle,
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
            "model": args.provider_cfg.model.clone(),
            "model_profile": model_profile.clone(),
            "deadline_epoch": gate.deadline_epoch(),
            "prompt_provided": user_prompt.is_some(),
            "user_prompt": user_prompt.clone(),
            "supercycle": args.supercycle,
            "research_budget_sec": args.research_budget_sec,
            "planning_budget_sec": args.planning_budget_sec,
            "require_commit_each_cycle": args.require_commit_each_cycle,
            "noop_streak_limit": noop_streak_limit,
            "conformance_interval_unchanged": conformance_interval_unchanged,
            "run_session_id": run_session_id,
        })),
    )?;

    if let Some(agent_cli) = agent_cli.as_ref() {
        let bootstrap_prompt = format!(
            "Harness run bootstrap. Keep persistent context for this run. User objective: {}",
            user_prompt.clone().unwrap_or_default()
        );
        let _ = agent_cli
            .run_text_once(
                &repo_path,
                &bootstrap_prompt,
                &run_session_id,
                AGENT_CLI_PLAN_TIMEOUT_SEC,
            )
            .await;
    }

    let mut cycles_total = 0u64;
    let mut cycles_succeeded = 0u64;
    let mut cycles_failed = 0u64;
    let mut next_heartbeat_epoch = start_epoch;
    let planning_window_sec = if args.supercycle {
        ((args.duration_sec as f64) * 0.20).ceil() as u64
    } else {
        0
    };
    let mut noop_streak = 0u64;
    let mut unchanged_since_conformance = 0u64;
    let mut counters = EventCounters::default();
    let mut last_commit_ok_epoch = start_epoch;
    let mut diff_rubric_rejection_streak = 0u64;
    let mut precommit_gate_skip_streak = 0u64;

    while gate.is_active_at(now_unix()) {
        let now = now_unix();
        if now >= next_heartbeat_epoch {
            sink.emit(
                &HarnessEvent::new(kinds::CODING_HEARTBEAT).with_data(json!({
                    "elapsed_sec": gate.elapsed_sec_at(now),
                    "remaining_sec": gate.remaining_sec_at(now),
                    "deadline_epoch": gate.deadline_epoch(),
                    "cycles_total": cycles_total,
                    "supercycle": args.supercycle,
                    "prompt_provided": user_prompt.is_some(),
                })),
            )?;
            next_heartbeat_epoch = now.saturating_add(args.heartbeat_sec);
        }

        cycles_total += 1;
        let cycle_id = format!("cycle-{cycles_total}");
        let cycle_start = Instant::now();

        sink.emit(
            &HarnessEvent::new(kinds::TASK_STARTED)
                .with_task_id(cycle_id.clone())
                .with_data(json!({
                    "mode": "coding",
                    "cycle": cycles_total,
                    "executor": executor.name(),
                    "remaining_sec": gate.remaining_sec_at(now_unix()),
                    "supercycle": args.supercycle,
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
                    "supercycle": args.supercycle,
                    "prompt_provided": user_prompt.is_some(),
                    "user_prompt": user_prompt.clone(),
                })),
        )?;

        let mut phase_results = Vec::new();

        if args.supercycle {
            let elapsed_now = gate.elapsed_sec_at(now_unix());
            if elapsed_now <= planning_window_sec {
                run_supercycle_planning(
                    &repo_path,
                    cycles_total,
                    &run_session_id,
                    user_prompt.as_deref(),
                    args.research_budget_sec,
                    args.planning_budget_sec,
                    agent_cli.as_ref(),
                )
                .await?;
            }
        }

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
            args.supercycle,
        )?;
        let selected_task = architecture_result.selected_task.clone();

        let ctx = CycleContext {
            cycle: cycles_total,
            repo_path: repo_path.clone(),
            user_prompt: user_prompt.clone(),
            selected_task: selected_task.clone(),
            run_session_id: run_session_id.clone(),
        };

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
            if should_run_codegen_stage(agent_cli.as_ref()) {
                run_codegen_stage(
                    &code_engine,
                    &ctx,
                    selected_task.as_ref(),
                    user_prompt.as_deref(),
                    agent_cli.as_ref(),
                )
                .await
            } else {
                StageResult::passed(WorkStage::Act)
            }
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
            args.supercycle,
        )?;
        phase_results.push(feature_result);

        let mutated_this_cycle = repo_dirty(&repo_path).await.unwrap_or(false);
        let pending_for_eligibility = pending_file_names(&repo_path).await;
        let commit_eligible_this_cycle =
            commit_has_meaningful_scope(&pending_for_eligibility, selected_task.as_ref());
        let should_run_conformance = mutated_this_cycle
            || unchanged_since_conformance >= conformance_interval_unchanged.saturating_sub(1);

        let verify_result = if !(plan_result.success && act_result.success) {
            StageResult::skipped(WorkStage::Verify, "feature stage failed")
        } else if !commit_eligible_this_cycle {
            StageResult::skipped(
                WorkStage::Verify,
                "pre-commit eligibility gate: non-meaningful pending changes",
            )
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
            args.supercycle,
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
            args.supercycle,
        )?;
        phase_results.push(cleanup_result);

        sink.emit(
            &HarnessEvent::new(kinds::CODING_CYCLE_HOOK)
                .with_task_id(cycle_id.clone())
                .with_data(json!({ "hooks": hook_results })),
        )?;

        let commit_rejected_by_rubric = hook_results.iter().any(|hook| {
            hook.name == "commit"
                && !hook.success
                && !hook.skipped
                && hook.detail.contains("diff rubric")
        });

        if commit_rejected_by_rubric {
            diff_rubric_rejection_streak = diff_rubric_rejection_streak.saturating_add(1);
        } else {
            diff_rubric_rejection_streak = 0;
        }

        let precommit_gate_skipped = verify_result
            .error
            .as_deref()
            .map(|e| e.contains("pre-commit eligibility gate"))
            .unwrap_or(false);
        if precommit_gate_skipped {
            precommit_gate_skip_streak = precommit_gate_skip_streak.saturating_add(1);
        } else {
            precommit_gate_skip_streak = 0;
        }

        if diff_rubric_rejection_streak >= DIFF_RUBRIC_REJECTION_STREAK_LIMIT
            || precommit_gate_skip_streak >= PRECOMMIT_GATE_SKIP_STREAK_LIMIT
        {
            let reset_detail = run_dirty_tree_recovery(&repo_path)
                .await
                .unwrap_or_else(|e| format!("dirty-tree recovery failed: {e}"));
            sink.emit(
                &HarnessEvent::new(kinds::CODING_HEARTBEAT)
                    .with_task_id(cycle_id.clone())
                    .with_data(json!({
                        "cycle": cycles_total,
                        "warning": "dirty-tree spin detected; forced reset + fresh re-architecture",
                        "recovery": reset_detail,
                        "diff_rubric_streak": diff_rubric_rejection_streak,
                        "precommit_gate_streak": precommit_gate_skip_streak,
                    })),
            )?;

            diff_rubric_rejection_streak = 0;
            precommit_gate_skip_streak = 0;
            progress_memory.repeated_no_diff_task_id = None;
            progress_memory.repeated_no_diff_cycles = 0;
            save_progress_memory(&progress_path, &progress_memory)?;
            continue;
        }

        if meaningful_diff_this_cycle {
            noop_streak = 0;
        } else {
            noop_streak = noop_streak.saturating_add(1);
        }
        counters.noop_streak = noop_streak;

        if forced_mutation_cycle {
            counters.forced_mutation = counters.forced_mutation.saturating_add(1);
            if !meaningful_diff_this_cycle {
                progress_memory.source_escalation_count =
                    progress_memory.source_escalation_count.saturating_add(1);
                sink.emit(
                    &HarnessEvent::new(kinds::CODING_COUNTER)
                        .with_task_id(cycle_id.clone())
                        .with_data(json!({
                            "forced_no_diff": true,
                            "note": "forced scoped task still yielded no meaningful diff; continuing run with escalation"
                        })),
                )?;
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

        let mut commit_executed_ok = hook_results
            .iter()
            .find(|hook| hook.name == "commit")
            .map(|hook| hook.success && !hook.skipped)
            .unwrap_or(false);

        if commit_executed_ok {
            last_commit_ok_epoch = now_unix();
        }

        let mut commit_watchdog_breached =
            now_unix().saturating_sub(last_commit_ok_epoch) >= COMMIT_WATCHDOG_SEC;

        if commit_watchdog_breached {
            let recovery = run_commit_watchdog_recovery(
                &sink,
                &cycle_id,
                &repo_path,
                cycles_total,
                &args,
                user_prompt.as_deref(),
                executor.policy(),
                selected_task.as_ref(),
            )
            .await?;

            sink.emit(
                &HarnessEvent::new(kinds::CODING_HEARTBEAT)
                    .with_task_id(cycle_id.clone())
                    .with_data(json!({
                        "cycle": cycles_total,
                        "warning": if recovery.success {
                            "commit watchdog breached; recovered with forced allow-empty commit"
                        } else {
                            "commit watchdog breached: forced allow-empty commit failed"
                        },
                        "recovery": recovery,
                        "commit_watchdog_sec": COMMIT_WATCHDOG_SEC,
                        "seconds_since_last_commit": now_unix().saturating_sub(last_commit_ok_epoch),
                    })),
            )?;

            if recovery.success {
                commit_executed_ok = true;
                last_commit_ok_epoch = now_unix();
                commit_watchdog_breached = false;
            }
        }

        let cycle_success = phase_results.iter().all(|phase| phase.success)
            && plan_result.success
            && act_result.success
            && verify_result.success
            && hook_results.iter().all(|hook| hook.success || hook.skipped)
            && (!args.require_commit_each_cycle || commit_executed_ok)
            && !commit_watchdog_breached;
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
                    "reason": if commit_watchdog_breached {
                        "commit_watchdog_breach"
                    } else if cycle_success {
                        "cycle_complete"
                    } else {
                        "cycle_failed"
                    },
                    "runtime_ms": cycle_runtime_ms,
                    "commit_watchdog_sec": COMMIT_WATCHDOG_SEC,
                    "seconds_since_last_commit": now_unix().saturating_sub(last_commit_ok_epoch),
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
                    "supercycle": args.supercycle,
                })),
        )?;

        if commit_watchdog_breached {
            break;
        }

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
                args.supercycle,
            )?;

            if args.cycle_pause_sec > 0 {
                sleep(Duration::from_secs(args.cycle_pause_sec)).await;
            }
        }
    }

    let elapsed_sec = gate.elapsed_sec_at(now_unix());

    if args.supercycle {
        let _ = write_postcoding_task_refresh(&repo_path, cycles_total, user_prompt.as_deref());
    }

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
    let mut policy = CommandPolicy::with_extra_commands(&args.allow_cmd);

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
        ExecutorPreset::OpenClaw | ExecutorPreset::Hermes => {
            let agent_cli = AgentCli::for_preset(args.preset)
                .expect("agent executor preset must resolve to agent cli");
            policy
                .allowlisted_commands
                .insert(agent_cli.binary().to_string());
            (
                if args.plan_cmd.is_empty() {
                    vec!["git status --short".to_string()]
                } else {
                    args.plan_cmd.clone()
                },
                if args.act_cmd.is_empty() {
                    Vec::new()
                } else {
                    args.act_cmd.clone()
                },
                if args.verify_cmd.is_empty() {
                    Vec::new()
                } else {
                    args.verify_cmd.clone()
                },
                agent_cli.name(),
            )
        }
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

fn should_run_codegen_stage(agent_cli: Option<&AgentCli>) -> bool {
    agent_cli.is_some()
}

async fn run_codegen_stage(
    engine: &CodeCycleEngine,
    ctx: &CycleContext,
    selected_task: Option<&FeatureTask>,
    user_prompt: Option<&str>,
    agent_cli: Option<&AgentCli>,
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

    if let Some(agent_cli) = agent_cli {
        return run_agent_cli_codegen_stage(*agent_cli, ctx, selected_task, user_prompt).await;
    }

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

    let base_task = build_code_task(selected_task, ctx, user_prompt);
    let mut commands = Vec::new();
    let mut last_error: Option<String> = None;

    for attempt in 1..=CODEGEN_MAX_ATTEMPTS {
        let attempt_task = build_codegen_attempt_task(&base_task, attempt, last_error.as_deref());

        match engine
            .run_cycle(&ctx.repo_path, &attempt_task, &repo_snapshot)
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
                    "attempt": attempt,
                    "attempt_limit": CODEGEN_MAX_ATTEMPTS,
                    "plan_summary": report.planned.summary,
                    "diff_summary": report.diff_generated.summary,
                    "touched_files": report.diff_generated.touched_files,
                    "changed_files": changed_files,
                    "apply_detail": report.diff_applied.detail,
                });

                commands.push(CommandExecution {
                    stage: WorkStage::Act,
                    command: format!(
                        "code-engine plan->diff->apply (attempt {attempt}/{CODEGEN_MAX_ATTEMPTS})"
                    ),
                    argv: vec!["code-engine".to_string(), attempt_task.id.clone()],
                    success,
                    exit_code: Some(if success { 0 } else { 1 }),
                    duration_ms: stage_start.elapsed().as_millis() as u64,
                    stdout_tail: truncate_tail(&payload.to_string()),
                    stderr_tail: if success {
                        String::new()
                    } else {
                        truncate_tail(&detail)
                    },
                    error: if success { None } else { Some(detail.clone()) },
                });

                if success {
                    return StageResult {
                        stage: WorkStage::Act,
                        success: true,
                        error: None,
                        commands,
                    };
                }

                last_error = Some(detail);
            }
            Err(err) => {
                let message = format!("code-diff engine failed: {err}");
                commands.push(CommandExecution {
                    stage: WorkStage::Act,
                    command: format!(
                        "code-engine plan->diff->apply (attempt {attempt}/{CODEGEN_MAX_ATTEMPTS})"
                    ),
                    argv: vec!["code-engine".to_string(), attempt_task.id],
                    success: false,
                    exit_code: Some(1),
                    duration_ms: stage_start.elapsed().as_millis() as u64,
                    stdout_tail: String::new(),
                    stderr_tail: truncate_tail(&message),
                    error: Some(message.clone()),
                });
                last_error = Some(message);
            }
        }
    }

    StageResult {
        stage: WorkStage::Act,
        success: false,
        error: Some(
            last_error.unwrap_or_else(|| "code-diff engine exhausted retry attempts".to_string()),
        ),
        commands,
    }
}

async fn run_agent_cli_codegen_stage(
    agent_cli: AgentCli,
    ctx: &CycleContext,
    selected_task: &FeatureTask,
    user_prompt: Option<&str>,
) -> StageResult {
    let stage_start = Instant::now();
    let agent_name = agent_cli.name();
    let agent_argv = agent_cli.argv_preview();

    let repo_snapshot = match build_repo_snapshot(&ctx.repo_path, selected_task, user_prompt).await
    {
        Ok(snapshot) => snapshot,
        Err(err) => {
            let message = format!("failed to build repo snapshot for {agent_name} agent: {err}");
            return StageResult {
                stage: WorkStage::Act,
                success: false,
                error: Some(message.clone()),
                commands: vec![CommandExecution {
                    stage: WorkStage::Act,
                    command: agent_cli.command_label("codegen"),
                    argv: agent_argv.clone(),
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

    let applier = GitApplyDiffApplier;
    let mut attempts = Vec::new();
    let mut last_error: Option<String> = None;

    let plan_prompt = build_agent_cli_plan_prompt(ctx, selected_task, user_prompt, &repo_snapshot);
    let validated_plan = match agent_cli
        .run_text_once(
            &ctx.repo_path,
            &plan_prompt,
            &ctx.run_session_id,
            AGENT_CLI_PLAN_TIMEOUT_SEC,
        )
        .await
    {
        Ok(plan_text) => match parse_agent_execution_plan(&plan_text) {
            Some(plan) => plan,
            None => {
                return StageResult {
                    stage: WorkStage::Act,
                    success: false,
                    error: Some(format!(
                        "{agent_name} plan pass did not return valid execution plan json"
                    )),
                    commands: vec![CommandExecution {
                        stage: WorkStage::Act,
                        command: agent_cli.command_label("planning pass"),
                        argv: agent_argv.clone(),
                        success: false,
                        exit_code: Some(1),
                        duration_ms: stage_start.elapsed().as_millis() as u64,
                        stdout_tail: truncate_tail(&plan_text),
                        stderr_tail: String::new(),
                        error: Some("invalid execution plan schema".to_string()),
                    }],
                };
            }
        },
        Err(err) => {
            return StageResult {
                stage: WorkStage::Act,
                success: false,
                error: Some(format!("{agent_name} planning pass failed: {err}")),
                commands: vec![CommandExecution {
                    stage: WorkStage::Act,
                    command: agent_cli.command_label("planning pass"),
                    argv: agent_argv.clone(),
                    success: false,
                    exit_code: Some(1),
                    duration_ms: stage_start.elapsed().as_millis() as u64,
                    stdout_tail: String::new(),
                    stderr_tail: truncate_tail(&err.to_string()),
                    error: Some(err.to_string()),
                }],
            };
        }
    };

    for attempt in 1..=AGENT_CLI_ATTEMPTS_PER_CYCLE {
        let prompt = build_agent_cli_json_only_prompt_with_attempt(
            ctx,
            selected_task,
            user_prompt,
            &repo_snapshot,
            &validated_plan,
            attempt,
            last_error.as_deref(),
        );

        let prompt_path =
            write_agent_cli_prompt_artifact(agent_cli, &ctx.repo_path, ctx.cycle, attempt, &prompt)
                .ok();
        let (payload, stderr_text) = match agent_cli
            .run_code_once(&ctx.repo_path, &prompt, &ctx.run_session_id)
            .await
        {
            Ok(result) => result,
            Err(err) => {
                let message = err.to_string();
                last_error = Some(message.clone());
                attempts.push(CommandExecution {
                    stage: WorkStage::Act,
                    command: format!(
                        "{} (attempt {attempt}/{AGENT_CLI_ATTEMPTS_PER_CYCLE})",
                        agent_cli.command_label("json edits")
                    ),
                    argv: agent_argv.clone(),
                    success: false,
                    exit_code: Some(1),
                    duration_ms: stage_start.elapsed().as_millis() as u64,
                    stdout_tail: String::new(),
                    stderr_tail: truncate_tail(&message),
                    error: Some(message),
                });
                continue;
            }
        };

        let proposal = match extract_json_edits_payload(&payload).map(|(paths, sentinel)| {
            CodeDiffProposal {
                summary: format!(
                    "{agent_name} agent json edits touching {} files",
                    paths.len()
                ),
                unified_diff: sentinel,
                touched_files: paths,
            }
        }) {
            Some(proposal) => proposal,
            None => {
                let message =
                    format!("{agent_name} agent response did not contain valid json edits payload");
                last_error = Some(message.clone());
                attempts.push(CommandExecution {
                    stage: WorkStage::Act,
                    command: format!(
                        "{} (attempt {attempt}/{AGENT_CLI_ATTEMPTS_PER_CYCLE})",
                        agent_cli.command_label("json edits")
                    ),
                    argv: agent_argv.clone(),
                    success: false,
                    exit_code: Some(1),
                    duration_ms: stage_start.elapsed().as_millis() as u64,
                    stdout_tail: truncate_tail(
                        &json!({
                            "prompt_path": prompt_path,
                            "payload_preview": payload,
                        })
                        .to_string(),
                    ),
                    stderr_tail: truncate_tail(&stderr_text),
                    error: Some(message),
                });
                continue;
            }
        };

        let apply = match applier.apply_diff(&ctx.repo_path, &proposal).await {
            Ok(result) => result,
            Err(err) => {
                let message = format!("failed to apply {agent_name}-generated json edits: {err}");
                last_error = Some(message.clone());
                attempts.push(CommandExecution {
                    stage: WorkStage::Act,
                    command: format!(
                        "{} (attempt {attempt}/{AGENT_CLI_ATTEMPTS_PER_CYCLE})",
                        agent_cli.command_label("json edits")
                    ),
                    argv: agent_argv.clone(),
                    success: false,
                    exit_code: Some(1),
                    duration_ms: stage_start.elapsed().as_millis() as u64,
                    stdout_tail: truncate_tail(
                        &json!({
                            "prompt_path": prompt_path,
                            "payload_preview": payload,
                        })
                        .to_string(),
                    ),
                    stderr_tail: truncate_tail(&message),
                    error: Some(message),
                });
                continue;
            }
        };

        let success = apply.applied && !apply.changed_files.is_empty();
        let detail = if success {
            format!(
                "{agent_name} agent json edits applied touching {} files",
                apply.changed_files.len()
            )
        } else {
            format!(
                "{agent_name} agent returned unapplied/empty json edits: {}",
                apply.detail
            )
        };

        attempts.push(CommandExecution {
            stage: WorkStage::Act,
            command: format!(
                "{} (attempt {attempt}/{AGENT_CLI_ATTEMPTS_PER_CYCLE})",
                agent_cli.command_label("json edits")
            ),
            argv: agent_argv.clone(),
            success,
            exit_code: Some(if success { 0 } else { 1 }),
            duration_ms: stage_start.elapsed().as_millis() as u64,
            stdout_tail: truncate_tail(
                &json!({
                    "prompt_path": prompt_path,
                    "changed_files": apply.changed_files,
                    "apply_detail": apply.detail,
                })
                .to_string(),
            ),
            stderr_tail: if success {
                String::new()
            } else {
                truncate_tail(&detail)
            },
            error: if success { None } else { Some(detail.clone()) },
        });

        if success {
            return StageResult {
                stage: WorkStage::Act,
                success: true,
                error: None,
                commands: attempts,
            };
        }

        last_error = Some(detail);
    }

    StageResult {
        stage: WorkStage::Act,
        success: false,
        error: Some(
            last_error.unwrap_or_else(|| format!("{agent_name} json codegen failed after retries")),
        ),
        commands: attempts,
    }
}

async fn run_parallel_research_once(
    repo_path: &Path,
    prompt: &str,
    _timeout_sec: u64,
) -> Result<String> {
    let api_key = std::env::var("PARALLEL_API_KEY").context("PARALLEL_API_KEY not set")?;
    let query = truncate_tail(prompt);
    let script_path = parallel_search_script_path();

    let output = Command::new("node")
        .arg(&script_path)
        .arg("--query")
        .arg(&query)
        .arg("--count")
        .arg("5")
        .arg("--mode")
        .arg("one-shot")
        .arg("--max-chars")
        .arg("700")
        .env("PARALLEL_API_KEY", api_key)
        .current_dir(repo_path)
        .output()
        .await
        .with_context(|| format!("failed to execute {}", script_path.display()))?;

    if !output.status.success() {
        return Err(anyhow!(
            "parallel search failed with {}: {}",
            output.status,
            truncate_tail(&String::from_utf8_lossy(&output.stderr))
        ));
    }

    let body = String::from_utf8_lossy(&output.stdout).to_string();
    let parsed: Value = serde_json::from_str(&body)
        .map_err(|e| anyhow!("parallel search response parse failed: {e}"))?;
    let mut out = String::from("# PARALLEL_RESEARCH\n\n");
    if let Some(results) = parsed.get("results").and_then(Value::as_array) {
        for (idx, item) in results.iter().enumerate() {
            let title = item
                .get("title")
                .and_then(Value::as_str)
                .unwrap_or("(untitled)");
            let url = item
                .get("url")
                .and_then(Value::as_str)
                .unwrap_or("(no url)");
            let snippet = item.get("snippet").and_then(Value::as_str).unwrap_or("");
            out.push_str(&format!(
                "## Result {}: {}\n{}\n{}\n\n",
                idx + 1,
                title,
                url,
                truncate_tail(snippet)
            ));
        }
    }
    Ok(out)
}

fn parallel_search_script_path() -> PathBuf {
    if let Ok(home) = std::env::var("CAPTAIN_HOME") {
        return PathBuf::from(home).join("tools/parallel-search.js");
    }

    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("tools/parallel-search.js")
}

fn build_agent_cli_plan_prompt(
    ctx: &CycleContext,
    selected_task: &FeatureTask,
    user_prompt: Option<&str>,
    repo_snapshot: &str,
) -> String {
    let user_prompt = user_prompt.unwrap_or("");
    let research = load_latest_research_context(&ctx.repo_path);
    format!(
        "Return STRICT JSON only.\n\nCycle: {cycle}\nTask: {task}\nTask source: {source}\nTask line: {line}\nUser prompt: {user_prompt}\n\nRepository snapshot:\n{snapshot}\n\nResearch context:\n{research}\n\nSchema:\n{{\n  \"target_files\": [\"src/path.rs\"],\n  \"target_symbols\": [\"fn_name_or_module\"],\n  \"behavior_delta\": \"one concrete behavior improvement\",\n  \"test_delta\": \"one concrete test change\",\n  \"commit_subject\": \"fix(scope): specific change\"\n}}\n\nRules:\n- Output JSON object only.\n- Be concrete and executable.\n- Must target src/ and tests.\n- No docs-only plans.\n- No extra keys.",
        cycle = ctx.cycle,
        task = selected_task.title,
        source = selected_task.source,
        line = selected_task.selected_line,
        user_prompt = user_prompt,
        snapshot = repo_snapshot,
        research = research,
    )
}

fn parse_agent_execution_plan(message: &str) -> Option<AgentExecutionPlan> {
    let trimmed = message.trim();
    let candidate = if trimmed.starts_with("```") {
        let start = trimmed.find('{')?;
        let end = trimmed.rfind('}')?;
        &trimmed[start..=end]
    } else {
        trimmed
    };

    let value: Value = serde_json::from_str(candidate).ok()?;
    let target_files = value
        .get("target_files")?
        .as_array()?
        .iter()
        .filter_map(|v| v.as_str().map(ToOwned::to_owned))
        .collect::<Vec<_>>();
    let target_symbols = value
        .get("target_symbols")?
        .as_array()?
        .iter()
        .filter_map(|v| v.as_str().map(ToOwned::to_owned))
        .collect::<Vec<_>>();
    let behavior_delta = value.get("behavior_delta")?.as_str()?.trim().to_string();
    let test_delta = value.get("test_delta")?.as_str()?.trim().to_string();
    let commit_subject = value.get("commit_subject")?.as_str()?.trim().to_string();

    if target_files.is_empty()
        || target_symbols.is_empty()
        || behavior_delta.is_empty()
        || test_delta.is_empty()
        || commit_subject.is_empty()
    {
        return None;
    }

    if !target_files.iter().any(|f| f.starts_with("src/")) {
        return None;
    }

    let raw_json = serde_json::to_string_pretty(&value).ok()?;
    Some(AgentExecutionPlan {
        target_files,
        target_symbols,
        behavior_delta,
        test_delta,
        commit_subject,
        raw_json,
    })
}

fn build_agent_cli_json_only_prompt_with_attempt(
    ctx: &CycleContext,
    selected_task: &FeatureTask,
    user_prompt: Option<&str>,
    repo_snapshot: &str,
    validated_plan: &AgentExecutionPlan,
    attempt: usize,
    previous_error: Option<&str>,
) -> String {
    let user_prompt = user_prompt.unwrap_or("");
    let previous_error = previous_error.unwrap_or("");
    let nrp = load_nrp_protocol(&ctx.repo_path);
    let research = load_latest_research_context(&ctx.repo_path);
    format!(
        "Return STRICT JSON only for code edits.\n\nCycle: {cycle}\nAttempt: {attempt}/{attempt_limit}\nTask: {task}\nTask source: {source}\nTask line: {line}\nUser prompt: {user_prompt}\nPrevious error: {previous_error}\n\nValidated execution plan (must follow exactly):\n{validated_plan}\n\nPlan summary:\n- target_files: {target_files}\n- target_symbols: {target_symbols}\n- behavior_delta: {behavior_delta}\n- test_delta: {test_delta}\n- commit_subject: {commit_subject}\n\nRepository snapshot:\n{snapshot}\n\nResearch context:\n{research}\n\nNRP protocol:\n{nrp}\n\nSchema:\n{{\n  \"rationale\": \"short explanation\",\n  \"acceptance_checks\": [\"check 1\", \"check 2\"],\n  \"edits\": [{{\"path\":\"relative/path\",\"content\":\"full file content\"}}]\n}}\n\nRules:\n- Output JSON object only, no markdown fences.\n- Keep edits minimal and task-focused.\n- Prefer existing files over creating new files unless required.\n- Prefer src/ + tests when task is code.\n- Do not include extra keys.",
        cycle = ctx.cycle,
        attempt = attempt,
        attempt_limit = AGENT_CLI_ATTEMPTS_PER_CYCLE,
        task = selected_task.title,
        source = selected_task.source,
        line = selected_task.selected_line,
        user_prompt = user_prompt,
        previous_error = previous_error,
        validated_plan = validated_plan.raw_json,
        target_files = validated_plan.target_files.join(", "),
        target_symbols = validated_plan.target_symbols.join(", "),
        behavior_delta = validated_plan.behavior_delta,
        test_delta = validated_plan.test_delta,
        commit_subject = validated_plan.commit_subject,
        snapshot = repo_snapshot,
        research = research,
        nrp = nrp,
    )
}

fn load_latest_research_context(repo_path: &Path) -> String {
    let base = repo_path.join(".harness/supercycle");
    if !base.exists() {
        return "(no research context available for this cycle)".to_string();
    }

    let mut latest: Option<PathBuf> = None;
    if let Ok(entries) = fs::read_dir(&base) {
        for entry in entries.flatten() {
            let p = entry.path();
            let Some(name) = p.file_name().and_then(|s| s.to_str()) else {
                continue;
            };
            if name.contains("RESEARCH.md") && latest.as_ref().map(|x| p > *x).unwrap_or(true) {
                latest = Some(p);
            }
        }
    }

    let Some(path) = latest else {
        return "(no research context available for this cycle)".to_string();
    };

    match fs::read_to_string(path) {
        Ok(content) => {
            let trimmed = content.trim();
            if trimmed.is_empty() {
                "(research context file was empty)".to_string()
            } else if trimmed.len() > 3000 {
                format!("{}...", &trimmed[..3000])
            } else {
                trimmed.to_string()
            }
        }
        Err(_) => "(failed to load research context)".to_string(),
    }
}

fn write_agent_cli_prompt_artifact(
    agent_cli: AgentCli,
    repo_path: &Path,
    cycle: u64,
    attempt: usize,
    prompt: &str,
) -> Result<String> {
    let dir = repo_path.join(".harness/supercycle/prompts");
    fs::create_dir_all(&dir).context("failed to create agent prompt artifact dir")?;
    let path = dir.join(format!(
        "cycle-{cycle:04}-{}-attempt-{attempt}.prompt.txt",
        agent_cli.name()
    ));
    fs::write(&path, prompt).context("failed to write agent prompt artifact")?;
    Ok(path.display().to_string())
}

fn load_nrp_protocol(repo_path: &Path) -> String {
    let candidates = [repo_path.join("docs/NRP.md"), repo_path.join("AGENTS.md")];

    for path in candidates {
        if let Ok(content) = fs::read_to_string(&path) {
            let trimmed = content.trim();
            if !trimmed.is_empty() {
                let max = 4000usize;
                return if trimmed.len() > max {
                    format!("{}...", &trimmed[..max])
                } else {
                    trimmed.to_string()
                };
            }
        }
    }

    "(no NRP/AGENTS protocol file found)".to_string()
}

fn extract_openclaw_payload_text(stdout: &str) -> Option<String> {
    let value: serde_json::Value = serde_json::from_str(stdout).ok()?;
    let payloads = value.get("payloads")?.as_array()?;
    let first = payloads.first()?;
    first
        .get("text")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string())
}

fn extract_json_edits_payload(message: &str) -> Option<(Vec<String>, String)> {
    let trimmed = message.trim();

    let json_candidate = if trimmed.starts_with("```") {
        let start = trimmed.find('{')?;
        let end = trimmed.rfind('}')?;
        &trimmed[start..=end]
    } else {
        trimmed
    };

    let payload: serde_json::Value = serde_json::from_str(json_candidate).ok()?;
    let rationale = payload.get("rationale")?.as_str()?.trim();
    if rationale.is_empty() {
        return None;
    }
    let acceptance_checks = payload.get("acceptance_checks")?.as_array()?;
    if acceptance_checks.is_empty() {
        return None;
    }
    if !acceptance_checks
        .iter()
        .all(|v| v.as_str().map(|s| !s.trim().is_empty()).unwrap_or(false))
    {
        return None;
    }

    let edits = payload.get("edits")?.as_array()?;
    if edits.is_empty() {
        return None;
    }

    let mut paths = Vec::new();
    for edit in edits {
        let path = edit.get("path")?.as_str()?.trim();
        let content = edit.get("content")?.as_str()?;
        if path.is_empty() || path.starts_with('/') || path.contains("..") || content.is_empty() {
            return None;
        }
        paths.push(path.to_string());
    }

    paths.sort();
    paths.dedup();

    let sentinel = format!("HARNESS_JSON_EDITS\n{payload}");
    Some((paths, sentinel))
}

fn build_code_task(
    selected_task: &FeatureTask,
    ctx: &CycleContext,
    user_prompt: Option<&str>,
) -> CodeTask {
    let target_files = infer_target_files(selected_task);
    let mut objective = selected_task.title.clone();
    if let Some(prompt) = user_prompt.map(str::trim).filter(|p| !p.is_empty()) {
        objective = format!("{objective} | user prompt: {prompt}");
    }

    let mut constraints = vec![
        "Produce a valid unified diff patch.".to_string(),
        "Keep edits minimal and aligned to the selected task.".to_string(),
        "Avoid unrelated formatting churn.".to_string(),
        "Output raw patch text beginning with 'diff --git' (no markdown fences).".to_string(),
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
            "Patch must apply with `git apply --index` without manual fixes.".to_string(),
            "Keep project checks green after apply.".to_string(),
        ],
    }
}

fn build_codegen_attempt_task(
    base_task: &CodeTask,
    attempt: usize,
    previous_error: Option<&str>,
) -> CodeTask {
    let mut task = base_task.clone();
    task.id = format!("{}::attempt-{}", base_task.id, attempt);

    if attempt > 1 {
        task.objective = format!(
            "{} | retry attempt {} of {}",
            base_task.objective, attempt, CODEGEN_MAX_ATTEMPTS
        );
        task.constraints.push(
            "Repair prior invalid diff output; ensure strict unified diff syntax and valid hunks."
                .to_string(),
        );
        if let Some(err) = previous_error {
            task.constraints
                .push(format!("Previous attempt failed with: {err}"));
        }
    }

    task
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

fn derive_verify_commands(ctx: &CycleContext) -> Vec<String> {
    let mut commands = vec!["cargo fmt --all".to_string()];

    if let Some(task) = ctx.selected_task.as_ref() {
        if task.source.starts_with("src/") {
            let module = task
                .source
                .trim_start_matches("src/")
                .trim_end_matches(".rs")
                .replace('/', "::");
            if !module.is_empty() {
                commands.push(format!("cargo test --all-targets {module}"));
            }
        }
    }

    commands.push("cargo check --all-targets".to_string());
    commands.push("cargo test --all-targets".to_string());
    commands
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
        selected_task: selected_task.cloned(),
        run_session_id: format!("harness-run-hook-{cycle}"),
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

    let add_result = execute_command_line(WorkStage::Act, "git add -A .", &hook_ctx, policy).await;
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

    let unstage_internal_result = unstage_internal_artifacts(&hook_ctx, policy).await;
    if !unstage_internal_result.success {
        let detail = format!(
            "failed to unstage internal artifacts: {}",
            summarize_error(&unstage_internal_result)
        );
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
            "push skipped because unstage-internal gate failed",
        )?;
        return Ok(hooks);
    }

    let staged_names = staged_file_names(repo_path).await;
    let staged_meaningful = filter_meaningful_scope_files(&staged_names);
    if staged_meaningful.is_empty() {
        let detail = "git add produced only internal/non-meaningful staged files".to_string();
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

    if !passes_commit_diff_rubric(repo_path, &staged_meaningful, selected_task).await {
        let detail = format!(
            "commit rejected by diff rubric (scope/size/relevance); staged_meaningful={}",
            staged_meaningful.join(", ")
        );
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
            "push skipped because commit diff failed rubric",
        )?;
        return Ok(hooks);
    }

    let commit_kind = infer_commit_kind(repo_path, args.user_prompt.as_deref()).await;
    let mut subject = summarize_commit_focus(repo_path, args.user_prompt.as_deref()).await;
    if commit_subject_is_generic(&subject)
        || !subject_mentions_changed_scope(&subject, &staged_meaningful)
    {
        subject = deterministic_subject_from_files(&staged_meaningful);
    }
    if commit_subject_is_generic(&subject)
        || !subject_mentions_changed_scope(&subject, &staged_meaningful)
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
        || !subject_mentions_changed_scope(&deduped_subject, &staged_meaningful)
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
    let commit_message = format!("{commit_kind}(harness): {commit_subject}");

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

    if args.push_each_cycle {
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
async fn run_commit_watchdog_recovery(
    sink: &EventSink,
    cycle_id: &str,
    repo_path: &Path,
    cycle: u64,
    args: &CodingRunArgs,
    user_prompt: Option<&str>,
    policy: &CommandPolicy,
    selected_task: Option<&FeatureTask>,
) -> Result<WatchdogRecovery> {
    let subject = format!("watchdog keepalive cycle {cycle}");
    let message = format!("chore(harness): {subject}");

    let hook_ctx = CycleContext {
        cycle,
        repo_path: repo_path.to_path_buf(),
        user_prompt: user_prompt.map(ToOwned::to_owned),
        selected_task: selected_task.cloned(),
        run_session_id: format!("harness-run-watchdog-{cycle}"),
    };

    let commit_cmd = format!(
        "git commit --allow-empty -m {}",
        shell_words::quote(&message)
    );
    let commit_result = execute_command_line(WorkStage::Act, &commit_cmd, &hook_ctx, policy).await;
    let commit_detail = if commit_result.success {
        "git commit --allow-empty ok".to_string()
    } else {
        summarize_error(&commit_result)
    };

    emit_git_commit_event(
        sink,
        cycle_id,
        cycle,
        commit_result.success,
        false,
        Some(subject.as_str()),
        Some(message.as_str()),
        if commit_result.success {
            "ok"
        } else {
            "failed"
        },
        &commit_detail,
    )?;

    let mut push_attempted = false;
    let mut push_success = None;
    let mut push_detail = None;

    if commit_result.success && args.push_each_cycle {
        push_attempted = true;
        let push_result = execute_command_line(WorkStage::Act, "git push", &hook_ctx, policy).await;
        let detail = if push_result.success {
            "git push ok".to_string()
        } else {
            summarize_error(&push_result)
        };
        push_success = Some(push_result.success);
        push_detail = Some(detail.clone());
        emit_git_push_event(
            sink,
            cycle_id,
            cycle,
            push_result.success,
            false,
            Some(subject.as_str()),
            Some(message.as_str()),
            if push_result.success { "ok" } else { "failed" },
            &detail,
        )?;
    } else {
        emit_git_push_event(
            sink,
            cycle_id,
            cycle,
            true,
            true,
            Some(subject.as_str()),
            Some(message.as_str()),
            "skipped",
            "watchdog recovery push not requested",
        )?;
    }

    Ok(WatchdogRecovery {
        success: commit_result.success,
        subject,
        message,
        commit_detail,
        push_attempted,
        push_success,
        push_detail,
    })
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
    let mut data = json!({
        "cycle": cycle,
        "success": success,
        "skipped": skipped,
        "result": result,
        "subject": subject,
        "message": message,
        "detail": detail,
    });
    if let Some(error_class) =
        ErrorClassifier::class_for_git_event(success, skipped, result, detail)
    {
        data["error_class"] = json!(error_class);
    }

    sink.emit(
        &HarnessEvent::new(kinds::GIT_COMMIT)
            .with_task_id(cycle_id.to_string())
            .with_data(data),
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
    let mut data = json!({
        "cycle": cycle,
        "success": success,
        "skipped": skipped,
        "result": result,
        "subject": subject,
        "message": message,
        "detail": detail,
    });
    if let Some(error_class) =
        ErrorClassifier::class_for_git_event(success, skipped, result, detail)
    {
        data["error_class"] = json!(error_class);
    }

    sink.emit(
        &HarnessEvent::new(kinds::GIT_PUSH)
            .with_task_id(cycle_id.to_string())
            .with_data(data),
    )
}

async fn run_supercycle_planning(
    repo_path: &Path,
    cycle: u64,
    run_session_id: &str,
    user_prompt: Option<&str>,
    research_budget_sec: u64,
    planning_budget_sec: u64,
    agent_cli: Option<&AgentCli>,
) -> Result<()> {
    let planning_dir = repo_path.join(".harness/supercycle");
    fs::create_dir_all(&planning_dir)?;

    let mut src_files = Vec::new();
    collect_src_files(repo_path.join("src"), &mut src_files)?;
    src_files.sort();

    let architecture = planning_dir.join(format!("cycle-{cycle}-ARCH_REMAP.md"));
    let task_graph = planning_dir.join(format!("cycle-{cycle}-TASK_GRAPH.md"));
    let task_pack = planning_dir.join(format!("cycle-{cycle}-TASK_PACK.md"));
    let research_prompt_path = planning_dir.join(format!("cycle-{cycle}-RESEARCH_PROMPT.md"));
    let research_result_path = planning_dir.join(format!("cycle-{cycle}-RESEARCH.md"));

    let prompt = user_prompt.unwrap_or("");

    let research_prompt = format!(
        "You are preparing a cycle planning brief for an autonomous Rust coding harness.\n\nUser goal:\n{prompt}\n\nRepository focus:\n- Rust project\n- Prioritize src/ + tests improvements\n- Avoid docs-only churn\n\nProduce concise best-practices guidance for this cycle in markdown with sections:\n1) Risks to avoid\n2) Code quality checklist\n3) Test strategy\n4) Concrete task-shaping advice (file/function scoped)\n5) Commit-quality guardrails\n\nKeep it practical and execution-oriented."
    );
    fs::write(&research_prompt_path, &research_prompt)?;

    let research_doc = if research_budget_sec > 0 {
        let timeout_sec = research_budget_sec.clamp(30, 600);
        let fallback_agent = agent_cli.copied().unwrap_or_else(AgentCli::openclaw);
        let fallback_agent_name = fallback_agent.name();
        if std::env::var("PARALLEL_API_KEY")
            .ok()
            .filter(|s| !s.trim().is_empty())
            .is_some()
        {
            match run_parallel_research_once(repo_path, &research_prompt, timeout_sec).await {
                Ok(text) if !text.trim().is_empty() => text,
                Ok(_) => "(parallel research returned empty output)".to_string(),
                Err(err) => {
                    match fallback_agent
                        .run_text_once(repo_path, &research_prompt, run_session_id, timeout_sec)
                        .await
                    {
                        Ok(text) if !text.trim().is_empty() => {
                            format!("(parallel fallback: {err})\n\n{text}")
                        }
                        Ok(_) => {
                            format!(
                                "(parallel failed: {err}; {fallback_agent_name} fallback empty)"
                            )
                        }
                        Err(err2) => format!(
                            "(research phase failed: parallel={err}; {fallback_agent_name}={err2})"
                        ),
                    }
                }
            }
        } else {
            match fallback_agent
                .run_text_once(repo_path, &research_prompt, run_session_id, timeout_sec)
                .await
            {
                Ok(text) if !text.trim().is_empty() => text,
                Ok(_) => "(research phase returned empty output)".to_string(),
                Err(err) => format!("(research phase failed: {err})"),
            }
        }
    } else {
        "(research disabled: set --research-budget-sec > 0 to enable)".to_string()
    };
    fs::write(&research_result_path, &research_doc)?;

    fs::write(
        &architecture,
        format!(
            "# ARCH_REMAP cycle {cycle}\n\n## Goal\n{prompt}\n\n## Research brief\nSee: `{}`\n\n## Subsystems\n- runtime loop + phase state machine\n- task synthesis + ranking\n- agent execution adapter\n- quality + commit rubric\n\n## Candidate remaps\n1. Extract supercycle planner from coding.rs into dedicated module\n2. Introduce strict one-cycle gate with rollback\n3. Split quality rubric into reusable engine\n\n## Source index size\n{} files under src/\n",
            research_result_path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("cycle-RESEARCH.md"),
            src_files.len()
        ),
    )?;

    let mut graph = String::from("# TASK_GRAPH\n\n");
    graph.push_str("1. research brief generation\n");
    graph.push_str("2. architecture remap doc generation\n");
    graph.push_str("3. generate file/function task pack\n");
    graph.push_str("4. execute one scoped change + test\n");
    graph.push_str("5. verify + commit\n");
    fs::write(&task_graph, graph)?;

    let mut pack = String::from("# TASK_PACK\n\n## Concrete file/function tasks\n");
    for path in src_files.iter().take(24) {
        let rel = path.strip_prefix(repo_path).unwrap_or(path);
        let rel = rel.display().to_string();
        pack.push_str(&format!(
            "- [ ] Review `{rel}` and propose one reliability improvement with matching test\n"
        ));
    }
    fs::write(&task_pack, pack)?;

    if planning_budget_sec > 0 {
        sleep(Duration::from_secs(planning_budget_sec.min(30))).await;
    }

    Ok(())
}

fn collect_src_files(dir: PathBuf, out: &mut Vec<PathBuf>) -> Result<()> {
    if !dir.exists() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_dir() {
            collect_src_files(path, out)?;
        } else if path.extension().and_then(|x| x.to_str()) == Some("rs") {
            out.push(path);
        }
    }
    Ok(())
}

fn write_postcoding_task_refresh(
    repo_path: &Path,
    cycle: u64,
    user_prompt: Option<&str>,
) -> Result<()> {
    let planning_dir = repo_path.join(".harness/supercycle");
    fs::create_dir_all(&planning_dir)?;

    let mut src_files = Vec::new();
    collect_src_files(repo_path.join("src"), &mut src_files)?;
    src_files.sort();

    let followup = planning_dir.join(format!("cycle-{cycle}-FOLLOWUP_TASKS.md"));
    let prompt = user_prompt.unwrap_or("");
    let mut out = String::from("# FOLLOWUP_TASKS\n\n");
    out.push_str(&format!("Prompt context: {prompt}\n\n"));
    out.push_str("## Next coding backlog\n");
    for path in src_files.iter().take(30) {
        let rel = path
            .strip_prefix(repo_path)
            .unwrap_or(path)
            .display()
            .to_string();
        out.push_str(&format!(
            "- [ ] Add one reliability/test improvement in `{rel}`\n"
        ));
    }
    fs::write(followup, out)?;
    Ok(())
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

    if !force_mutation {
        let _ = ensure_roadmap_seed_tasks(repo_path);
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

#[allow(clippy::too_many_arguments)]
fn emit_phase_event(
    sink: &EventSink,
    runtime_log_path: Option<&Path>,
    thought_log_path: Option<&Path>,
    cycle_id: &str,
    cycle: u64,
    result: PhaseResult,
    next_step: &str,
    supercycle: bool,
) -> Result<()> {
    let data = json!({
        "cycle": cycle,
        "phase": result.phase.label(),
        "reason": result.reason,
        "selected_task": result.selected_task,
        "success": result.success,
        "result": result.result,
        "next": next_step,
        "supercycle": supercycle,
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
    let meaningful = filter_meaningful_scope_files(&files);
    deterministic_subject_from_files(&meaningful)
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

fn path_starts_with_component(path: &str, component: &str) -> bool {
    path == component
        || path
            .strip_prefix(component)
            .is_some_and(|rest| rest.starts_with('/'))
        || path.contains(&format!("/{component}/"))
        || path.ends_with(&format!("/{component}"))
}

fn is_internal_artifact_path(path: &str) -> bool {
    path_starts_with_component(path, ".harness") || path_starts_with_component(path, "runs")
}

fn filter_meaningful_scope_files(files: &[String]) -> Vec<String> {
    files
        .iter()
        .filter(|f| !is_internal_artifact_path(f))
        .cloned()
        .collect()
}

async fn unstage_internal_artifacts(
    ctx: &CycleContext,
    policy: &CommandPolicy,
) -> CommandExecution {
    execute_command_line(
        WorkStage::Act,
        "git reset -q HEAD -- .harness runs",
        ctx,
        policy,
    )
    .await
}

fn commit_subject_is_generic(subject: &str) -> bool {
    is_generic_subject(subject)
}

fn subject_mentions_changed_scope(subject: &str, files: &[String]) -> bool {
    let meaningful = filter_meaningful_scope_files(files);
    let changed_files = meaningful.iter().map(String::as_str).collect::<Vec<_>>();
    has_informative_subject_scope(subject, &changed_files)
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
        candidate = format!("{candidate} [cycle-refresh]");
    }
    candidate
}

fn commit_has_meaningful_scope(files: &[String], selected_task: Option<&FeatureTask>) -> bool {
    if files.is_empty() {
        return false;
    }

    let only_internal_state = files.iter().all(|f| is_internal_artifact_path(f));
    if only_internal_state {
        return false;
    }

    let has_src = files.iter().any(|f| path_starts_with_component(f, "src"));
    let has_tests = files.iter().any(|f| {
        path_starts_with_component(f, "tests") || f.contains("/tests/") || f.contains("_test")
    });
    let has_docs = files.iter().any(|f| {
        f.ends_with("README.md")
            || f.ends_with("RUNBOOK.md")
            || f.ends_with("ARCHITECTURE.md")
            || f.ends_with("MIGRATION.md")
    });

    if has_src {
        return true;
    }

    if has_tests {
        if let Some(task) = selected_task {
            return task.source.starts_with("src/");
        }
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

async fn passes_commit_diff_rubric(
    repo_path: &Path,
    staged_files: &[String],
    selected_task: Option<&FeatureTask>,
) -> bool {
    if staged_files.is_empty() {
        return false;
    }

    let output = Command::new("git")
        .arg("diff")
        .arg("--cached")
        .arg("--numstat")
        .current_dir(repo_path)
        .output()
        .await;

    let Ok(output) = output else {
        return false;
    };
    if !output.status.success() {
        return false;
    }

    let mut touched = 0u64;
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let mut parts = line.split_whitespace();
        let add = parts
            .next()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0);
        let del = parts
            .next()
            .and_then(|v| v.parse::<u64>().ok())
            .unwrap_or(0);
        touched = touched.saturating_add(add + del);
    }

    if touched < 4 {
        return false;
    }

    if let Some(task) = selected_task {
        if path_starts_with_component(&task.source, "src") {
            return staged_files
                .iter()
                .any(|f| f == &task.source || path_starts_with_component(f, "src"));
        }
    }

    staged_files
        .iter()
        .any(|f| path_starts_with_component(f, "src"))
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

async fn run_dirty_tree_recovery(repo_path: &Path) -> Result<String> {
    let reset = Command::new("git")
        .args(["reset", "--hard", "HEAD"])
        .current_dir(repo_path)
        .output()
        .await
        .context("git reset --hard HEAD failed")?;

    let clean = Command::new("git")
        .args(["clean", "-fd", ".harness/supercycle", "runs"])
        .current_dir(repo_path)
        .output()
        .await
        .context("git clean -fd .harness/supercycle runs failed")?;

    let reset_ok = reset.status.success();
    let clean_ok = clean.status.success();
    let reset_stderr = String::from_utf8_lossy(&reset.stderr).trim().to_string();
    let clean_stderr = String::from_utf8_lossy(&clean.stderr).trim().to_string();

    Ok(format!(
        "reset_ok={reset_ok} clean_ok={clean_ok} reset_stderr={} clean_stderr={}",
        truncate_tail(&reset_stderr),
        truncate_tail(&clean_stderr)
    ))
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
        .env("HARNESS_CODING_CYCLE", ctx.cycle.to_string())
        .env("HARNESS_CODING_STAGE", stage_label(stage))
        .env("OPENCLAW_CODING_CYCLE", ctx.cycle.to_string())
        .env("OPENCLAW_CODING_STAGE", stage_label(stage));

    if let Some(prompt) = ctx.user_prompt.as_deref() {
        command_handle
            .env("HARNESS_USER_PROMPT", prompt)
            .env("OPENCLAW_USER_PROMPT", prompt);
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

fn default_repo_run_lock_path(repo_path: &Path) -> PathBuf {
    let dot_git = repo_path.join(".git");
    if dot_git.is_dir() {
        return dot_git.join(".agent-harness-code.lock");
    }

    if dot_git.is_file() {
        if let Ok(content) = fs::read_to_string(&dot_git) {
            if let Some(gitdir) = content.trim().strip_prefix("gitdir:") {
                let gitdir = gitdir.trim();
                let gitdir_path = PathBuf::from(gitdir);
                let resolved = if gitdir_path.is_absolute() {
                    gitdir_path
                } else {
                    repo_path.join(gitdir_path)
                };
                return resolved.join(".agent-harness-code.lock");
            }
        }
    }

    repo_path.join(".git/.agent-harness-code.lock")
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

    #[test]
    fn agent_cli_maps_agent_presets_to_shared_adapter() {
        let openclaw = AgentCli::for_preset(ExecutorPreset::OpenClaw).unwrap();
        assert_eq!(openclaw.name(), "openclaw");
        assert_eq!(
            openclaw.argv_preview(),
            vec!["openclaw".to_string(), "agent".to_string()]
        );

        let hermes = AgentCli::for_preset(ExecutorPreset::Hermes).unwrap();
        assert_eq!(hermes.name(), "hermes");
        assert_eq!(
            hermes.argv_preview(),
            vec!["hermes".to_string(), "chat".to_string()]
        );

        assert!(AgentCli::for_preset(ExecutorPreset::Cargo).is_none());
    }

    #[tokio::test]
    async fn stage_fails_for_blocked_command() {
        let policy = CommandPolicy::default_safe();
        let dir = tempfile::tempdir().unwrap();
        let ctx = CycleContext {
            cycle: 1,
            repo_path: dir.path().to_path_buf(),
            user_prompt: None,
            selected_task: None,
            run_session_id: "test-run".to_string(),
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
            supercycle: false,
            research_budget_sec: 0,
            planning_budget_sec: 0,
            require_commit_each_cycle: false,
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
    fn shell_executor_does_not_require_provider_codegen() {
        assert!(!should_run_codegen_stage(None));
        let hermes = AgentCli::hermes();
        assert!(should_run_codegen_stage(Some(&hermes)));
    }

    #[test]
    fn commit_scope_accepts_monorepo_prefixed_src_paths() {
        let files = vec!["captain/harnesses/rust-harness/src/dogfood_smoke.rs".to_string()];
        assert!(commit_has_meaningful_scope(&files, None));
    }

    #[test]
    fn filter_meaningful_scope_files_excludes_internal_artifacts() {
        let files = vec![
            ".harness/coding-progress.json".to_string(),
            "runs/runtime.log".to_string(),
            "src/coding.rs".to_string(),
        ];
        let filtered = filter_meaningful_scope_files(&files);
        assert_eq!(filtered, vec!["src/coding.rs".to_string()]);
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

    #[tokio::test]
    async fn repo_dirty_detects_untracked_files() {
        let dir = tempfile::tempdir().unwrap();
        let status = Command::new("git")
            .args(["init"])
            .current_dir(dir.path())
            .output()
            .await
            .unwrap();
        assert!(status.status.success());
        fs::write(
            dir.path().join("new_file.rs"),
            "pub const NEW_FILE: bool = true;\n",
        )
        .unwrap();

        assert!(repo_dirty(dir.path()).await.unwrap());
    }

    #[test]
    fn default_lock_path_resolves_git_file_worktree_metadata() {
        let dir = tempfile::tempdir().unwrap();
        let gitdir = dir.path().join("real-git-dir");
        fs::create_dir_all(&gitdir).unwrap();
        fs::write(dir.path().join(".git"), "gitdir: real-git-dir\n").unwrap();

        assert_eq!(
            default_repo_run_lock_path(dir.path()),
            gitdir.join(".agent-harness-code.lock")
        );
        let _lock = acquire_repo_run_lock(&default_repo_run_lock_path(dir.path())).unwrap();
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
