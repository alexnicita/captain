use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};

pub mod kinds {
    pub const CODING_RUN_STARTED: &str = "coding.run.started";
    pub const CODING_RUN_FINISHED: &str = "coding.run.finished";
    pub const CODING_CYCLE_STARTED: &str = "coding.cycle.started";
    pub const CODING_PHASE: &str = "coding.phase";
    pub const CODING_CYCLE_PLAN: &str = "coding.cycle.plan";
    pub const CODING_CYCLE_ACT: &str = "coding.cycle.act";
    pub const CODING_CYCLE_VERIFY: &str = "coding.cycle.verify";
    pub const CODING_CYCLE_HOOK: &str = "coding.cycle.hook";
    pub const CODING_HEARTBEAT: &str = "coding.heartbeat";
    pub const GIT_COMMIT: &str = "git.commit";
    pub const GIT_PUSH: &str = "git.push";
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarnessEvent {
    pub ts_unix: u64,
    pub kind: String,
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
            run_id: None,
            task_id: None,
            data: Value::Null,
        }
    }

    pub fn with_run_id(mut self, run_id: impl Into<String>) -> Self {
        self.run_id = Some(run_id.into());
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
}

impl EventSink {
    pub fn new(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).with_context(|| {
                format!("failed to create event log parent dir: {}", parent.display())
            })?;
        }
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
            .with_context(|| format!("failed to open event log: {}", path.display()))?;
        Ok(Self {
            file: Arc::new(Mutex::new(file)),
        })
    }

    pub fn emit(&self, event: &HarnessEvent) -> Result<()> {
        let mut line = serde_json::to_vec(event).context("failed to serialize event")?;
        line.push(b'\n');

        let mut guard = self.file.lock().expect("event sink mutex poisoned");
        guard
            .write_all(&line)
            .context("failed to append event line")?;
        guard.flush().context("failed to flush event sink")?;
        Ok(())
    }
}

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
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
