use crate::events::HarnessEvent;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ReplayFilter {
    pub run_id: Option<String>,
    pub latest_run: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplaySummary {
    pub total_events: usize,
    pub kinds: BTreeMap<String, usize>,
    pub task_ids: BTreeSet<String>,
    pub run_ids: BTreeSet<String>,
    pub selected_run_id: Option<String>,
    pub first_ts_unix: Option<u64>,
    pub last_ts_unix: Option<u64>,
    pub sequence_monotonic_per_run: bool,
}

pub fn replay_file(path: &str) -> Result<ReplaySummary> {
    replay_file_with_filter(path, &ReplayFilter::default())
}

pub fn replay_file_with_filter(path: &str, filter: &ReplayFilter) -> Result<ReplaySummary> {
    let content = fs::read_to_string(path)?;
    replay_str_with_filter(&content, filter)
}

pub fn replay_str(content: &str) -> Result<ReplaySummary> {
    replay_str_with_filter(content, &ReplayFilter::default())
}

pub fn replay_str_with_filter(content: &str, filter: &ReplayFilter) -> Result<ReplaySummary> {
    if filter.latest_run && filter.run_id.is_some() {
        return Err(anyhow!("cannot set both run_id and latest_run"));
    }

    let mut events = Vec::new();
    for line in content.lines().filter(|l| !l.trim().is_empty()) {
        let event: HarnessEvent = serde_json::from_str(line)?;
        events.push(event);
    }

    let selected_run_id = if let Some(run_id) = &filter.run_id {
        Some(run_id.clone())
    } else if filter.latest_run {
        events.iter().rev().find_map(|evt| evt.run_id.clone())
    } else {
        None
    };

    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    let mut task_ids = BTreeSet::new();
    let mut run_ids = BTreeSet::new();
    let mut total_events = 0usize;
    let mut first_ts = None;
    let mut last_ts = None;
    let mut previous_seq_by_run: BTreeMap<String, u64> = BTreeMap::new();
    let mut sequence_monotonic_per_run = true;

    for event in events.into_iter().filter(|event| {
        selected_run_id
            .as_ref()
            .map(|run_id| event.run_id.as_deref() == Some(run_id.as_str()))
            .unwrap_or(true)
    }) {
        *kinds.entry(event.kind.clone()).or_default() += 1;
        if let Some(task_id) = event.task_id {
            task_ids.insert(task_id);
        }

        if let Some(run_id) = event.run_id.clone() {
            if let Some(seq) = event.seq {
                if let Some(previous_seq) = previous_seq_by_run.get(&run_id).copied() {
                    if seq <= previous_seq {
                        sequence_monotonic_per_run = false;
                    }
                }
                previous_seq_by_run.insert(run_id.clone(), seq);
            }

            run_ids.insert(run_id);
        }

        first_ts = Some(first_ts.map_or(event.ts_unix, |ts: u64| ts.min(event.ts_unix)));
        last_ts = Some(last_ts.map_or(event.ts_unix, |ts: u64| ts.max(event.ts_unix)));
        total_events += 1;
    }

    Ok(ReplaySummary {
        total_events,
        kinds,
        task_ids,
        run_ids,
        selected_run_id,
        first_ts_unix: first_ts,
        last_ts_unix: last_ts,
        sequence_monotonic_per_run,
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
        assert!(summary.sequence_monotonic_per_run);
    }

    #[test]
    fn replay_filters_latest_run() {
        let content = r#"{"kind":"run.started","ts_unix":1,"run_id":"r1","seq":1}
{"kind":"run.finished","ts_unix":2,"run_id":"r1","seq":2}
{"kind":"run.started","ts_unix":3,"run_id":"r2","seq":1}
{"kind":"task.started","ts_unix":4,"run_id":"r2","seq":2,"task_id":"t1"}
"#;
        let summary = replay_str_with_filter(
            content,
            &ReplayFilter {
                latest_run: true,
                ..Default::default()
            },
        )
        .unwrap();
        assert_eq!(summary.selected_run_id.as_deref(), Some("r2"));
        assert_eq!(summary.total_events, 2);
        assert_eq!(summary.run_ids.len(), 1);
        assert!(summary.run_ids.contains("r2"));
    }

    #[test]
    fn replay_filters_specific_run() {
        let content = r#"{"kind":"task.started","ts_unix":1,"run_id":"r1","seq":1,"task_id":"t1"}
{"kind":"task.started","ts_unix":2,"run_id":"r2","seq":1,"task_id":"t2"}
"#;
        let summary = replay_str_with_filter(
            content,
            &ReplayFilter {
                run_id: Some("r1".to_string()),
                latest_run: false,
            },
        )
        .unwrap();
        assert_eq!(summary.total_events, 1);
        assert!(summary.run_ids.contains("r1"));
        assert!(!summary.run_ids.contains("r2"));
    }

    #[test]
    fn replay_rejects_conflicting_filters() {
        let content = r#"{"kind":"run.started","ts_unix":1,"run_id":"r1","seq":1}"#;
        let err = replay_str_with_filter(
            content,
            &ReplayFilter {
                run_id: Some("r1".to_string()),
                latest_run: true,
            },
        )
        .unwrap_err();
        assert!(err
            .to_string()
            .contains("cannot set both run_id and latest_run"));
    }
}
