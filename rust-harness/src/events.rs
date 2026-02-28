use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs::{create_dir_all, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub mod kinds {
    pub const RUN_STARTED: &str = "run.started";
    pub const RUN_FINISHED: &str = "run.finished";
    pub const TASK_STARTED: &str = "task.started";
    pub const TASK_FINISHED: &str = "task.finished";
    pub const PROVIDER_REQUEST: &str = "provider.request";
    pub const PROVIDER_RESPONSE: &str = "provider.response";
    pub const PROVIDER_RETRY: &str = "provider.retry";
    pub const PROVIDER_TIMEOUT: &str = "provider.timeout";
    pub const PROVIDER_ERROR: &str = "provider.error";
    pub const TOOL_CALL: &str = "tool.call";
    pub const TOOL_OUTPUT: &str = "tool.output";
    pub const TOOL_ERROR: &str = "tool.error";
    pub const SCHEDULER_DISPATCH: &str = "scheduler.dispatch";
    pub const SCHEDULER_RESULT: &str = "scheduler.result";
    pub const SCHEDULER_TICK: &str = "scheduler.tick";
    pub const CLI_RUN_SUMMARY: &str = "cli.run.summary";
    pub const CLI_BATCH_SUMMARY: &str = "cli.batch.summary";
    pub const CODING_RUN_STARTED: &str = "coding.run.started";
    pub const CODING_RUN_FINISHED: &str = "coding.run.finished";
    pub const CODING_HEARTBEAT: &str = "coding.heartbeat";
    pub const CODING_CYCLE_STARTED: &str = "coding.cycle.started";
    pub const CODING_CYCLE_PLAN: &str = "coding.cycle.plan";
    pub const CODING_CYCLE_ACT: &str = "coding.cycle.act";
    pub const CODING_CYCLE_VERIFY: &str = "coding.cycle.verify";
    pub const CODING_CYCLE_HOOK: &str = "coding.cycle.hook";
    pub const CODING_CYCLE_FINISHED: &str = "coding.cycle.finished";
    pub const CODING_PHASE: &str = "coding.phase";
    pub const CODING_COUNTER: &str = "coding.counter";
    pub const CODING_LOCK_EXISTS: &str = "coding.lock.exists";
    pub const CODING_LOCK_ACQUIRED: &str = "coding.lock.acquired";
    pub const CODING_CONFORMANCE_SKIPPED: &str = "coding.conformance.skipped";
    pub const GIT_COMMIT: &str = "git.commit";
    pub const GIT_PUSH: &str = "git.push";
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarnessEvent {
    pub kind: String,
    pub ts_unix: u64,
    pub run_id: String,
    pub seq: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl HarnessEvent {
    pub fn new(kind: impl Into<String>) -> Self {
        Self {
            kind: kind.into(),
            ts_unix: now_unix(),
            run_id: String::new(),
            seq: 0,
            task_id: None,
            data: None,
        }
    }

    pub fn with_task_id(mut self, task_id: impl Into<String>) -> Self {
        self.task_id = Some(task_id.into());
        self
    }

    pub fn with_data(mut self, data: Value) -> Self {
        self.data = Some(data);
        self
    }
}

pub struct EventSink {
    path: PathBuf,
    run_id: String,
    seq: AtomicU64,
}

impl EventSink {
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        let run_id = format!("run-{}-{}", now_unix(), std::process::id());
        Self::with_run_id(path, run_id)
    }

    pub fn with_run_id(path: impl AsRef<Path>, run_id: impl Into<String>) -> Result<Self> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            create_dir_all(parent)?;
        }
        Ok(Self {
            path,
            run_id: run_id.into(),
            seq: AtomicU64::new(1),
        })
    }

    pub fn emit(&self, event: &HarnessEvent) -> Result<()> {
        let mut enriched = event.clone();
        if enriched.run_id.is_empty() {
            enriched.run_id = self.run_id.clone();
        }
        if enriched.seq == 0 {
            enriched.seq = self.seq.fetch_add(1, Ordering::Relaxed);
        }
        if enriched.ts_unix == 0 {
            enriched.ts_unix = now_unix();
        }

        let line = serde_json::to_string(&enriched)?;
        let mut f = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&self.path)?;
        writeln!(f, "{line}")?;
        Ok(())
    }

    pub fn emit_kind(&self, kind: &str, task_id: Option<&str>, data: Option<Value>) -> Result<()> {
        let mut event = HarnessEvent::new(kind);
        if let Some(id) = task_id {
            event = event.with_task_id(id.to_string());
        }
        if let Some(payload) = data {
            event = event.with_data(payload);
        }
        self.emit(&event)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn run_id(&self) -> &str {
        &self.run_id
    }
}

pub fn now_unix() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}
