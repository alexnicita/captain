use crate::events::kinds;
use crate::replay::ReplaySummary;
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalReport {
    pub pass: bool,
    pub checks: Vec<EvalCheck>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalCheck {
    pub name: String,
    pub pass: bool,
    pub detail: String,
}

pub fn evaluate_replay(summary: &ReplaySummary) -> EvalReport {
    let mut checks = Vec::new();

    checks.push(EvalCheck {
        name: "has_events".to_string(),
        pass: summary.total_events > 0,
        detail: format!("total_events={}", summary.total_events),
    });

    let run_started = summary.kinds.get(kinds::RUN_STARTED).copied().unwrap_or(0);
    let run_finished = summary.kinds.get(kinds::RUN_FINISHED).copied().unwrap_or(0);

    checks.push(EvalCheck {
        name: "has_run_started".to_string(),
        pass: run_started > 0,
        detail: format!("{}={run_started}", kinds::RUN_STARTED),
    });

    checks.push(EvalCheck {
        name: "has_run_finished".to_string(),
        pass: run_finished > 0,
        detail: format!("{}={run_finished}", kinds::RUN_FINISHED),
    });

    checks.push(EvalCheck {
        name: "run_finished_not_exceed_started".to_string(),
        pass: run_finished <= run_started,
        detail: format!("started={run_started}, finished={run_finished}"),
    });

    let task_started = summary.kinds.get(kinds::TASK_STARTED).copied().unwrap_or(0);
    let task_finished = summary
        .kinds
        .get(kinds::TASK_FINISHED)
        .copied()
        .unwrap_or(0);

    checks.push(EvalCheck {
        name: "has_task_started".to_string(),
        pass: task_started > 0,
        detail: format!("{}={task_started}", kinds::TASK_STARTED),
    });

    checks.push(EvalCheck {
        name: "has_task_finished".to_string(),
        pass: task_finished > 0,
        detail: format!("{}={task_finished}", kinds::TASK_FINISHED),
    });

    checks.push(EvalCheck {
        name: "task_finished_not_exceed_started".to_string(),
        pass: task_finished <= task_started,
        detail: format!("started={task_started}, finished={task_finished}"),
    });

    checks.push(EvalCheck {
        name: "sequence_monotonic_per_run".to_string(),
        pass: summary.sequence_monotonic_per_run,
        detail: format!(
            "sequence_monotonic_per_run={}",
            summary.sequence_monotonic_per_run
        ),
    });

    let selected_run_consistent = summary
        .selected_run_id
        .as_ref()
        .map(|run_id| summary.run_ids.len() <= 1 && summary.run_ids.contains(run_id))
        .unwrap_or(true);
    checks.push(EvalCheck {
        name: "selected_run_consistent".to_string(),
        pass: selected_run_consistent,
        detail: format!(
            "selected_run_id={:?}, run_ids={}",
            summary.selected_run_id,
            summary
                .run_ids
                .iter()
                .cloned()
                .collect::<Vec<_>>()
                .join(",")
        ),
    });

    let known = known_kinds();
    let unknown: Vec<String> = summary
        .kinds
        .keys()
        .filter(|kind| !known.contains(kind.as_str()))
        .cloned()
        .collect();
    checks.push(EvalCheck {
        name: "no_unknown_event_kinds".to_string(),
        pass: unknown.is_empty(),
        detail: if unknown.is_empty() {
            "ok".to_string()
        } else {
            format!("unknown={}", unknown.join(","))
        },
    });

    let pass = checks.iter().all(|c| c.pass);
    EvalReport { pass, checks }
}

fn known_kinds() -> BTreeSet<&'static str> {
    BTreeSet::from([
        kinds::RUN_STARTED,
        kinds::RUN_FINISHED,
        kinds::TASK_STARTED,
        kinds::TASK_FINISHED,
        kinds::PROVIDER_REQUEST,
        kinds::PROVIDER_RESPONSE,
        kinds::PROVIDER_RETRY,
        kinds::PROVIDER_TIMEOUT,
        kinds::PROVIDER_ERROR,
        kinds::TOOL_CALL,
        kinds::TOOL_OUTPUT,
        kinds::TOOL_ERROR,
        kinds::SCHEDULER_DISPATCH,
        kinds::SCHEDULER_RESULT,
        kinds::SCHEDULER_TICK,
        kinds::CLI_RUN_SUMMARY,
        kinds::CLI_BATCH_SUMMARY,
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay::replay_str;

    #[test]
    fn eval_passes_good_fixture() {
        let summary = replay_str(include_str!("../fixtures/good_run.jsonl")).unwrap();
        let report = evaluate_replay(&summary);
        assert!(report.pass);
    }

    #[test]
    fn eval_fails_bad_fixture() {
        let summary = replay_str(include_str!("../fixtures/bad_run_missing_finish.jsonl")).unwrap();
        let report = evaluate_replay(&summary);
        assert!(!report.pass);
        assert!(report
            .checks
            .iter()
            .any(|c| c.name == "has_task_finished" && !c.pass));
    }

    #[test]
    fn eval_fails_missing_run_finished() {
        let content = r#"{"kind":"run.started","ts_unix":1,"run_id":"r1","seq":1}
{"kind":"task.started","ts_unix":2,"run_id":"r1","seq":2,"task_id":"t1"}
{"kind":"task.finished","ts_unix":3,"run_id":"r1","seq":3,"task_id":"t1"}
"#;
        let summary = replay_str(content).unwrap();
        let report = evaluate_replay(&summary);
        assert!(report
            .checks
            .iter()
            .any(|c| c.name == "has_run_finished" && !c.pass));
    }

    #[test]
    fn eval_detects_non_monotonic_sequence() {
        let content = r#"{"kind":"run.started","ts_unix":1,"run_id":"r1","seq":2}
{"kind":"task.started","ts_unix":2,"run_id":"r1","seq":1,"task_id":"t1"}
{"kind":"task.finished","ts_unix":3,"run_id":"r1","seq":3,"task_id":"t1"}
"#;
        let summary = replay_str(content).unwrap();
        let report = evaluate_replay(&summary);
        assert!(report
            .checks
            .iter()
            .any(|c| c.name == "sequence_monotonic_per_run" && !c.pass));
    }
}
