use crate::events::HarnessEvent;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplaySummary {
    pub total_events: usize,
    pub kinds: BTreeMap<String, usize>,
    pub task_ids: BTreeSet<String>,
    pub run_ids: BTreeSet<String>,
    pub first_ts_unix: Option<u64>,
    pub last_ts_unix: Option<u64>,
}

pub fn replay_file(path: &str) -> Result<ReplaySummary> {
    let content = fs::read_to_string(path)?;
    replay_str(&content)
}

pub fn replay_str(content: &str) -> Result<ReplaySummary> {
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    let mut task_ids = BTreeSet::new();
    let mut run_ids = BTreeSet::new();
    let mut total_events = 0usize;
    let mut first_ts = None;
    let mut last_ts = None;

    for line in content.lines().filter(|l| !l.trim().is_empty()) {
        let event: HarnessEvent = serde_json::from_str(line)?;
        *kinds.entry(event.kind).or_default() += 1;
        if let Some(task_id) = event.task_id {
            task_ids.insert(task_id);
        }
        run_ids.insert(event.run_id);

        first_ts = Some(first_ts.map_or(event.ts_unix, |ts: u64| ts.min(event.ts_unix)));
        last_ts = Some(last_ts.map_or(event.ts_unix, |ts: u64| ts.max(event.ts_unix)));
        total_events += 1;
    }

    Ok(ReplaySummary {
        total_events,
        kinds,
        task_ids,
        run_ids,
        first_ts_unix: first_ts,
        last_ts_unix: last_ts,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_parses_fixture() {
        let fixture = include_str!("../fixtures/good_run.jsonl");
        let summary = replay_str(fixture).unwrap();
        assert!(summary.total_events >= 4);
        assert_eq!(summary.kinds.get("task.started").copied().unwrap_or(0), 1);
        assert_eq!(summary.kinds.get("task.finished").copied().unwrap_or(0), 1);
    }
}
