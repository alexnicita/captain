use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

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
    pub const CODING_CYCLE_STARTED: &str = "coding.cycle.started";
    pub const CODING_CYCLE_FINISHED: &str = "coding.cycle.finished";
    pub const CODING_PHASE: &str = "coding.phase";
    pub const CODING_CYCLE_PLAN: &str = "coding.cycle.plan";
    pub const CODING_CYCLE_ACT: &str = "coding.cycle.act";
    pub const CODING_CYCLE_VERIFY: &str = "coding.cycle.verify";
    pub const CODING_CYCLE_HOOK: &str = "coding.cycle.hook";
    pub const CODING_CONFORMANCE_SKIPPED: &str = "coding.conformance.skipped";
    pub const CODING_COUNTER: &str = "coding.counter";
    pub const CODING_LOCK_ACQUIRED: &str = "coding.lock.acquired";
    pub const CODING_LOCK_EXISTS: &str = "coding.lock.exists";
    pub const CODING_HEARTBEAT: &str = "coding.heartbeat";
    pub const GIT_COMMIT: &str = "git.commit";
    pub const GIT_PUSH: &str = "git.push";

    pub const fn all() -> &'static [&'static str] {
        &[
            RUN_STARTED,
            RUN_FINISHED,
            TASK_STARTED,
            TASK_FINISHED,
            PROVIDER_REQUEST,
            PROVIDER_RESPONSE,
            PROVIDER_RETRY,
            PROVIDER_TIMEOUT,
            PROVIDER_ERROR,
            TOOL_CALL,
            TOOL_OUTPUT,
            TOOL_ERROR,
            SCHEDULER_DISPATCH,
            SCHEDULER_RESULT,
            SCHEDULER_TICK,
            CLI_RUN_SUMMARY,
            CLI_BATCH_SUMMARY,
            CODING_RUN_STARTED,
            CODING_RUN_FINISHED,
            CODING_CYCLE_STARTED,
            CODING_CYCLE_FINISHED,
            CODING_PHASE,
            CODING_CYCLE_PLAN,
            CODING_CYCLE_ACT,
            CODING_CYCLE_VERIFY,
            CODING_CYCLE_HOOK,
            CODING_CONFORMANCE_SKIPPED,
            CODING_COUNTER,
            CODING_LOCK_ACQUIRED,
            CODING_LOCK_EXISTS,
            CODING_HEARTBEAT,
            GIT_COMMIT,
            GIT_PUSH,
        ]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarnessEvent {
    pub ts_unix: u64,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub seq: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub run_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub task_id: Option<String>,
    #[serde(default)]
    pub data: Value,
}

impl HarnessEvent {
    pub fn new(kind: impl Into<String>) -> Self {
        Self {
            ts_unix: now_unix(),
            kind: kind.into(),
            seq: None,
            run_id: None,
            task_id: None,
            data: Value::Null,
        }
    }

    pub fn with_run_id(mut self, run_id: impl Into<String>) -> Self {
        self.run_id = Some(run_id.into());
        self
    }

    pub fn with_seq(mut self, seq: u64) -> Self {
        self.seq = Some(seq);
        self
    }

    pub fn with_task_id(mut self, task_id: impl Into<String>) -> Self {
        self.task_id = Some(task_id.into());
        self
    }

    pub fn with_data(mut self, data: Value) -> Self {
        self.data = data;
        self
    }
}

#[derive(Clone)]
pub struct EventSink {
    file: Arc<Mutex<std::fs::File>>,
    path: Arc<PathBuf>,
    run_id: Arc<String>,
    next_seq: Arc<Mutex<u64>>,
}

impl EventSink {
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        Self::new_with_run_id(path, default_run_id())
    }

    pub fn new_with_run_id(path: impl AsRef<Path>, run_id: impl Into<String>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!(
                    "failed to create event log parent dir: {}",
                    parent.display()
                )
            })?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .with_context(|| format!("failed to open event log: {}", path.display()))?;
        Ok(Self {
            file: Arc::new(Mutex::new(file)),
            path: Arc::new(path.to_path_buf()),
            run_id: Arc::new(run_id.into()),
            next_seq: Arc::new(Mutex::new(1)),
        })
    }

    pub fn emit(&self, event: &HarnessEvent) -> Result<()> {
        let mut event = event.clone();
        if event.run_id.is_none() {
            event.run_id = Some(self.run_id().to_string());
        }
        if event.seq.is_none() {
            let mut next_seq = self.next_seq.lock().expect("event seq mutex poisoned");
            event.seq = Some(*next_seq);
            *next_seq += 1;
        }

        let mut line = serde_json::to_vec(&event).context("failed to serialize event")?;
        line.push(b'\n');

        let mut guard = self.file.lock().expect("event sink mutex poisoned");
        guard
            .write_all(&line)
            .context("failed to append event line")?;
        guard.flush().context("failed to flush event sink")?;
        Ok(())
    }

    pub fn path(&self) -> &Path {
        self.path.as_path()
    }

    pub fn run_id(&self) -> &str {
        self.run_id.as_str()
    }
}

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn default_run_id() -> String {
    format!("run-{}-{}", now_unix(), process::id())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn event_sink_emits_newline_terminated_jsonl() {
        let dir = tempfile::tempdir().expect("tempdir");
        let path = dir.path().join("events.jsonl");
        let sink = EventSink::new(&path).expect("sink");

        sink.emit(&HarnessEvent::new("test.event"))
            .expect("emit should work");

        let bytes = std::fs::read(&path).expect("read events file");
        assert!(!bytes.is_empty(), "events file should not be empty");
        assert_eq!(bytes.last().copied(), Some(b'\n'));

        let text = String::from_utf8(bytes).expect("utf8");
        for line in text.lines() {
            serde_json::from_str::<serde_json::Value>(line).expect("valid json line");
        }
    }
}
