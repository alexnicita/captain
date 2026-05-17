use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;
use tokio::time::sleep;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ChecklistStats {
    pub total: usize,
    pub done: usize,
}

impl ChecklistStats {
    /// Returns the number of pending checklist items.
    pub fn pending(self) -> usize {
        self.total.saturating_sub(self.done)
    }

    /// Returns true if all checklist items are completed.
    pub fn all_done(self) -> bool {
        self.total > 0 && self.done == self.total
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunState {
    pub run_dir: String,
    pub checklist_path: String,
    pub start_epoch: u64,
    pub min_runtime_sec: u64,
    pub heartbeat_sec: u64,
    pub poll_sec: u64,
    pub dry_run: bool,
    pub status: String,
    pub stop_epoch: Option<u64>,
    pub finish_epoch: Option<u64>,
    pub elapsed_sec: Option<u64>,
}

#[derive(Debug, Clone)]
pub struct GateStartArgs {
    pub checklist: String,
    pub run_id: Option<String>,
    pub min_runtime_minutes: f64,
    pub heartbeat_minutes: f64,
    pub poll_seconds: u64,
    pub dry_run: bool,
    pub dry_runtime_sec: u64,
    pub dry_heartbeat_sec: u64,
    pub base_dir: Option<String>,
}

#[derive(Debug, Clone)]
pub struct GateStatusArgs {
    pub run_dir: Option<String>,
    pub base_dir: Option<String>,
}

#[derive(Debug, Clone)]
pub struct GateStopArgs {
    pub run_dir: Option<String>,
    pub base_dir: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RuntimeGate {
    start_epoch: u64,
    min_runtime_sec: u64,
}

impl RuntimeGate {
    pub fn new(start_epoch: u64, min_runtime_sec: u64) -> Self {
        Self {
            start_epoch,
            min_runtime_sec,
        }
    }

    pub fn start_epoch(&self) -> u64 {
        self.start_epoch
    }

    pub fn min_runtime_sec(&self) -> u64 {
        self.min_runtime_sec
    }

    pub fn deadline_epoch(&self) -> u64 {
        self.start_epoch.saturating_add(self.min_runtime_sec)
    }

    pub fn elapsed_sec_at(&self, now_epoch: u64) -> u64 {
        now_epoch.saturating_sub(self.start_epoch)
    }

    pub fn remaining_sec_at(&self, now_epoch: u64) -> u64 {
        self.min_runtime_sec
            .saturating_sub(self.elapsed_sec_at(now_epoch))
    }

    pub fn is_active_at(&self, now_epoch: u64) -> bool {
        self.remaining_sec_at(now_epoch) > 0
    }

    pub fn is_open_at(&self, now_epoch: u64) -> bool {
        self.remaining_sec_at(now_epoch) == 0
    }
}

pub async fn gate_start(args: GateStartArgs) -> Result<()> {
    if args.poll_seconds == 0 {
        return Err(anyhow!("poll_seconds must be > 0"));
    }

    let checklist = PathBuf::from(&args.checklist);
    let min_runtime_sec = if args.dry_run {
        args.dry_runtime_sec
    } else {
        (args.min_runtime_minutes * 60.0).round() as u64
    };
    let heartbeat_sec = if args.dry_run {
        args.dry_heartbeat_sec
    } else {
        (args.heartbeat_minutes * 60.0).round() as u64
    };
    if heartbeat_sec == 0 {
        return Err(anyhow!("heartbeat interval must be > 0"));
    }

    let runs_root = runs_root(args.base_dir.as_deref());
    let run_dir = create_run_dir(&runs_root, args.run_id.as_deref())?;

    let mut state = RunState {
        run_dir: run_dir.display().to_string(),
        checklist_path: checklist.display().to_string(),
        start_epoch: now_unix(),
        min_runtime_sec,
        heartbeat_sec,
        poll_sec: args.poll_seconds,
        dry_run: args.dry_run,
        status: "running".to_string(),
        stop_epoch: None,
        finish_epoch: None,
        elapsed_sec: None,
    };
    let runtime_gate = RuntimeGate::new(state.start_epoch, min_runtime_sec);
    write_state(&run_dir, &state)?;
    append_log(
        &run_dir,
        &format!(
            "START checklist={} min_runtime_sec={} heartbeat_sec={} poll_sec={}",
            state.checklist_path, min_runtime_sec, heartbeat_sec, args.poll_seconds
        ),
    )?;

    println!("Run directory: {}", run_dir.display());
    println!("Started at epoch: {}", state.start_epoch);

    let stop_path = run_dir.join("STOP");
    let mut next_heartbeat = 0u64;

    loop {
        if stop_path.exists() {
            state.status = "stopped".to_string();
            state.stop_epoch = Some(now_unix());
            write_state(&run_dir, &state)?;
            append_log(&run_dir, "STOP file detected; run stopped")?;
            println!("Stopped by operator request.");
            return Ok(());
        }

        let checklist_stats = parse_checklist(&checklist)?;
        let now = now_unix();
        let elapsed = runtime_gate.elapsed_sec_at(now);
        let remaining = runtime_gate.remaining_sec_at(now);
        let gate_open = runtime_gate.is_open_at(now);
        if now >= next_heartbeat {
            append_log(
                &run_dir,
                &format!(
                    "HEARTBEAT elapsed_sec={} remaining_sec={} checklist={}/{} pending={} gate_open={} all_done={}",
                    elapsed,
                    remaining,
                    checklist_stats.done,
                    checklist_stats.total,
                    checklist_stats.pending(),
                    gate_open,
                    checklist_stats.all_done()
                ),
            )?;
            next_heartbeat = now.saturating_add(heartbeat_sec);
        }

        if gate_open && checklist_stats.all_done() {
            state.status = "complete".to_string();
            state.finish_epoch = Some(now_unix());
            state.elapsed_sec = Some(elapsed);
            write_state(&run_dir, &state)?;
            append_log(
                &run_dir,
                &format!(
                    "COMPLETE elapsed_sec={} checklist={}/{}",
                    elapsed, checklist_stats.done, checklist_stats.total
                ),
            )?;
            println!("DONE: runtime gate + checklist completion both satisfied.");
            println!("Progress log: {}", run_dir.join("progress.log").display());
            return Ok(());
        }

        sleep(Duration::from_secs(args.poll_seconds)).await;
    }
}

pub fn gate_status(args: GateStatusArgs) -> Result<serde_json::Value> {
    let run_dir = resolve_run_dir(args.run_dir.as_deref(), args.base_dir.as_deref())?;
    let state = read_state(&run_dir)?;
    let checklist = parse_checklist(Path::new(&state.checklist_path))?;
    let now = now_unix();
    let elapsed = effective_elapsed_sec(&state, now);
    let runtime_gate = RuntimeGate::new(state.start_epoch, state.min_runtime_sec);
    let remaining = runtime_gate.min_runtime_sec().saturating_sub(elapsed);
    let runtime_gate_open = runtime_gate.is_open_at(now);
    let last_heartbeat_epoch = read_last_heartbeat_epoch(&run_dir)?;

    Ok(json!({
        "run_dir": run_dir.display().to_string(),
        "status": state.status,
        "terminal": matches!(state.status.as_str(), "complete" | "stopped"),
        "start_epoch": state.start_epoch,
        "finish_epoch": state.finish_epoch,
        "stop_epoch": state.stop_epoch,
        "elapsed_sec": elapsed,
        "remaining_sec": remaining,
        "runtime_gate_open": runtime_gate_open,
        "checklist": {
            "done": checklist.done,
            "total": checklist.total,
            "pending": checklist.pending(),
            "all_done": checklist.all_done(),
        },
        "can_finish_now": runtime_gate_open && checklist.all_done(),
        "last_heartbeat_epoch": last_heartbeat_epoch,
        "heartbeat_stale_sec": last_heartbeat_epoch.map(|epoch| now.saturating_sub(epoch)),
        "progress_log": run_dir.join("progress.log").display().to_string(),
    }))
}

pub fn gate_stop(args: GateStopArgs) -> Result<serde_json::Value> {
    let run_dir = resolve_run_dir(args.run_dir.as_deref(), args.base_dir.as_deref())?;
    let stop_path = run_dir.join("STOP");
    fs::write(
        &stop_path,
        format!("stop requested at epoch={}\n", now_unix()),
    )?;
    append_log(&run_dir, "STOP requested by operator")?;
    Ok(json!({
        "stop_requested": true,
        "run_dir": run_dir.display().to_string(),
        "stop_path": stop_path.display().to_string(),
    }))
}

pub fn parse_checklist(path: &Path) -> Result<ChecklistStats> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("failed to read checklist: {}", path.display()))?;
    let mut total = 0usize;
    let mut done = 0usize;

    for raw_line in text.lines() {
        let line = raw_line.trim_start();
        let marker = if line.starts_with("- [") {
            Some("- [")
        } else if line.starts_with("* [") {
            Some("* [")
        } else {
            None
        };
        let Some(marker) = marker else {
            continue;
        };

        let rest = &line[marker.len()..];
        let mut chars = rest.chars();
        let Some(box_char) = chars.next() else {
            continue;
        };
        let Some(next_char) = chars.next() else {
            continue;
        };
        if next_char != ']' {
            continue;
        }

        total += 1;
        if matches!(box_char, 'x' | 'X') {
            done += 1;
        }
    }

    Ok(ChecklistStats { total, done })
}

fn runs_root(base_dir: Option<&str>) -> PathBuf {
    base_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("./runs/runtime-gate"))
}

fn latest_ptr(runs_root: &Path) -> PathBuf {
    runs_root.join("latest_run.txt")
}

fn create_run_dir(runs_root: &Path, run_id: Option<&str>) -> Result<PathBuf> {
    fs::create_dir_all(runs_root)?;
    let suffix = run_id.map(|s| format!("-{s}")).unwrap_or_default();
    let run_dir = runs_root.join(format!("run-{}{}", now_unix(), suffix));
    fs::create_dir(&run_dir)
        .with_context(|| format!("failed to create run dir: {}", run_dir.display()))?;
    fs::write(latest_ptr(runs_root), run_dir.display().to_string())?;
    Ok(run_dir)
}

fn resolve_run_dir(run_dir: Option<&str>, base_dir: Option<&str>) -> Result<PathBuf> {
    if let Some(dir) = run_dir {
        return Ok(PathBuf::from(dir));
    }

    let runs_root = runs_root(base_dir);
    let ptr = latest_ptr(&runs_root);
    let path_text = fs::read_to_string(&ptr)
        .with_context(|| format!("no latest run pointer found: {}", ptr.display()))?;
    let resolved = PathBuf::from(path_text.trim());
    if !resolved.exists() {
        return Err(anyhow!(
            "latest run path does not exist: {}",
            resolved.display()
        ));
    }
    Ok(resolved)
}

fn write_state(run_dir: &Path, state: &RunState) -> Result<()> {
    fs::write(
        run_dir.join("state.json"),
        serde_json::to_string_pretty(state)?,
    )?;
    Ok(())
}

fn read_state(run_dir: &Path) -> Result<RunState> {
    let text = fs::read_to_string(run_dir.join("state.json"))?;
    Ok(serde_json::from_str(&text)?)
}

fn append_log(run_dir: &Path, message: &str) -> Result<()> {
    let line = format!("[epoch={}] {}\n", now_unix(), message);
    let path = run_dir.join("progress.log");
    let mut existing = String::new();
    if path.exists() {
        existing = fs::read_to_string(&path)?;
    }
    existing.push_str(&line);
    fs::write(path, existing)?;
    Ok(())
}

fn effective_elapsed_sec(state: &RunState, now_epoch: u64) -> u64 {
    if let Some(elapsed) = state.elapsed_sec {
        return elapsed;
    }
    if let Some(finish_epoch) = state.finish_epoch {
        return finish_epoch.saturating_sub(state.start_epoch);
    }
    if let Some(stop_epoch) = state.stop_epoch {
        return stop_epoch.saturating_sub(state.start_epoch);
    }
    now_epoch.saturating_sub(state.start_epoch)
}

fn read_last_heartbeat_epoch(run_dir: &Path) -> Result<Option<u64>> {
    let progress = run_dir.join("progress.log");
    if !progress.exists() {
        return Ok(None);
    }

    let mut last = None;
    let content = fs::read_to_string(progress)?;
    for line in content.lines() {
        if !line.contains("HEARTBEAT") {
            continue;
        }
        let Some(rest) = line.strip_prefix("[epoch=") else {
            continue;
        };
        let Some((epoch_part, _)) = rest.split_once(']') else {
            continue;
        };
        if let Ok(epoch) = epoch_part.parse::<u64>() {
            last = Some(epoch);
        }
    }

    Ok(last)
}

fn now_unix() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_gate_deadline_and_remaining_behavior() {
        let gate = RuntimeGate::new(100, 10);
        assert_eq!(gate.start_epoch(), 100);
        assert_eq!(gate.deadline_epoch(), 110);
        assert_eq!(gate.elapsed_sec_at(105), 5);
        assert_eq!(gate.remaining_sec_at(105), 5);
        assert!(gate.is_active_at(109));
        assert!(!gate.is_active_at(110));
        assert!(gate.is_open_at(110));
    }

    #[test]
    fn parse_checklist_counts_done_and_total() {
        let dir = tempfile::tempdir().unwrap();
        let checklist = dir.path().join("checklist.md");
        fs::write(&checklist, "- [ ] one\n- [x] two\n* [X] three\n- nope\n").unwrap();
        let stats = parse_checklist(&checklist).unwrap();
        assert_eq!(stats.total, 3);
        assert_eq!(stats.done, 2);
        assert_eq!(stats.pending(), 1);
        assert!(!stats.all_done());
    }

    #[test]
    fn parse_checklist_all_done() {
        let dir = tempfile::tempdir().unwrap();
        let checklist = dir.path().join("checklist.md");
        fs::write(&checklist, "- [x] one\n- [X] two\n").unwrap();
        let stats = parse_checklist(&checklist).unwrap();
        assert!(stats.all_done());
    }

    #[test]
    fn effective_elapsed_uses_terminal_state() {
        let state = RunState {
            run_dir: "./tmp/x".to_string(),
            checklist_path: "./tmp/checklist.md".to_string(),
            start_epoch: 100,
            min_runtime_sec: 10,
            heartbeat_sec: 1,
            poll_sec: 1,
            dry_run: true,
            status: "complete".to_string(),
            stop_epoch: None,
            finish_epoch: Some(107),
            elapsed_sec: Some(7),
        };

        assert_eq!(effective_elapsed_sec(&state, 999), 7);
    }

    #[test]
    fn read_last_heartbeat_epoch_parses_log() {
        let dir = tempfile::tempdir().unwrap();
        let run_dir = dir.path();
        fs::write(
            run_dir.join("progress.log"),
            "[epoch=10] HEARTBEAT one\n[epoch=12] HEARTBEAT two\n[epoch=15] COMPLETE\n",
        )
        .unwrap();

        let heartbeat = read_last_heartbeat_epoch(run_dir).unwrap();
        assert_eq!(heartbeat, Some(12));
    }
}
