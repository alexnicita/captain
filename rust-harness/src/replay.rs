use crate::events::HarnessEvent;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fs;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplaySummary {
    pub total_events: usize,
    pub kinds: BTreeMap<String, usize>,
}

pub fn replay_file(path: &str) -> Result<ReplaySummary> {
    let content = fs::read_to_string(path)?;
    let mut kinds: BTreeMap<String, usize> = BTreeMap::new();
    let mut total_events = 0usize;

    for line in content.lines().filter(|l| !l.trim().is_empty()) {
        let event: HarnessEvent = serde_json::from_str(line)?;
        *kinds.entry(event.kind).or_default() += 1;
        total_events += 1;
    }

    Ok(ReplaySummary { total_events, kinds })
}
