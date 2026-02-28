use crate::replay::ReplaySummary;
use serde::{Deserialize, Serialize};

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

    let has_task_finished = summary.kinds.get("task.finished").copied().unwrap_or(0) > 0;
    checks.push(EvalCheck {
        name: "has_task_finished".to_string(),
        pass: has_task_finished,
        detail: format!(
            "task.finished count={}",
            summary.kinds.get("task.finished").copied().unwrap_or(0)
        ),
    });

    let has_provider_response = summary
        .kinds
        .get("provider.response")
        .copied()
        .unwrap_or(0)
        > 0;
    checks.push(EvalCheck {
        name: "has_provider_response".to_string(),
        pass: has_provider_response,
        detail: format!(
            "provider.response count={}",
            summary.kinds.get("provider.response").copied().unwrap_or(0)
        ),
    });

    let pass = checks.iter().all(|c| c.pass);
    EvalReport { pass, checks }
}
