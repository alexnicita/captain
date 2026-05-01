use crate::events::kinds;
use crate::events::HarnessEvent;
use crate::replay::ReplaySummary;
use crate::run_analysis::{RunMetrics, RunMetricsCollector, RunRollups};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalReport {
    pub pass: bool,
    pub checks: Vec<EvalCheck>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metrics: Option<RunMetrics>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rollups: Option<RunRollups>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EvalCheck {
    pub name: String,
    pub pass: bool,
    pub detail: String,
}

pub fn evaluate_replay(summary: &ReplaySummary) -> EvalReport {
    let checks = evaluate_summary_checks(summary);
    let pass = checks.iter().all(|c| c.pass);
    EvalReport {
        pass,
        checks,
        metrics: None,
        rollups: None,
    }
}

pub fn evaluate_events(summary: &ReplaySummary, events: &[HarnessEvent]) -> EvalReport {
    let mut checks = evaluate_summary_checks(summary);
    let (metrics, rollups) = RunMetricsCollector::collect(events);

    let unknown_errors = metrics
        .error_counts_by_class
        .get(&crate::error_taxonomy::ErrorClass::Unknown)
        .copied()
        .unwrap_or(0);
    checks.push(EvalCheck {
        name: "no_unknown_errors".to_string(),
        pass: unknown_errors == 0,
        detail: format!("unknown_errors={unknown_errors}"),
    });

    if metrics.provider_requests > 0 {
        let timeout_rate = metrics.provider_timeouts as f64 / metrics.provider_requests as f64;
        checks.push(EvalCheck {
            name: "provider_timeout_rate_under_threshold".to_string(),
            pass: timeout_rate <= 0.10,
            detail: format!(
                "provider_timeouts={} provider_requests={} rate={timeout_rate:.3}",
                metrics.provider_timeouts, metrics.provider_requests
            ),
        });
    }

    if metrics.tool_calls > 0 {
        let tool_error_rate = metrics.tool_errors as f64 / metrics.tool_calls as f64;
        checks.push(EvalCheck {
            name: "tool_error_rate_under_threshold".to_string(),
            pass: tool_error_rate <= 0.05,
            detail: format!(
                "tool_errors={} tool_calls={} rate={tool_error_rate:.3}",
                metrics.tool_errors, metrics.tool_calls
            ),
        });
    }

    if metrics.cycles_total > 0 {
        checks.push(EvalCheck {
            name: "coding_quality_score_minimum".to_string(),
            pass: metrics.quality_score_100 >= 70,
            detail: format!("quality_score_100={}", metrics.quality_score_100),
        });
    }

    let pass = checks.iter().all(|c| c.pass);
    EvalReport {
        pass,
        checks,
        metrics: Some(metrics),
        rollups: Some(rollups),
    }
}

fn evaluate_summary_checks(summary: &ReplaySummary) -> Vec<EvalCheck> {
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

    checks
}

fn known_kinds() -> BTreeSet<&'static str> {
    kinds::all().iter().copied().collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::replay::{replay_events_str_with_filter, replay_str, ReplayFilter};

    #[test]
    fn eval_passes_good_fixture() {
        let summary = replay_str(include_str!(
            "../../harnesses/rust-harness/fixtures/good_run.jsonl"
        ))
        .unwrap();
        let report = evaluate_replay(&summary);
        assert!(report.pass);
    }

    #[test]
    fn eval_fails_bad_fixture() {
        let summary = replay_str(include_str!(
            "../../harnesses/rust-harness/fixtures/bad_run_missing_finish.jsonl"
        ))
        .unwrap();
        let report = evaluate_replay(&summary);
        assert!(!report.pass);
        assert!(report
            .checks
            .iter()
            .any(|c| c.name == "has_task_finished" && !c.pass));
    }

    #[test]
    fn eval_events_passes_coding_fixture_with_metrics() {
        let fixture = include_str!("../../harnesses/rust-harness/fixtures/coding_run.jsonl");
        let summary = replay_str(fixture).unwrap();
        let events = replay_events_str_with_filter(fixture, &ReplayFilter::default()).unwrap();

        let report = evaluate_events(&summary, &events);

        assert!(report.pass);
        let metrics = report.metrics.as_ref().expect("metrics");
        assert_eq!(metrics.cycles_total, 1);
        assert_eq!(metrics.cycles_succeeded, 1);
        assert_eq!(metrics.git_commit_ok, 1);
        assert_eq!(metrics.quality_score_100, 100);
        assert!(report
            .rollups
            .as_ref()
            .expect("rollups")
            .by_model_executor_provider
            .contains_key("model=gpt-5.3-codex executor=openclaw provider=http"));
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
