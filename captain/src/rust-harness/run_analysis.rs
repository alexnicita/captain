use crate::error_taxonomy::{ErrorClass, ErrorClassifier};
use crate::events::{kinds, HarnessEvent};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RunMetrics {
    pub run_ids: BTreeSet<String>,
    pub models: BTreeSet<String>,
    pub executors: BTreeSet<String>,
    pub providers_resolved: BTreeSet<String>,
    pub cycles_total: u64,
    pub cycles_succeeded: u64,
    pub cycles_failed: u64,
    pub act_success: u64,
    pub act_fail: u64,
    pub verify_success: u64,
    pub verify_fail: u64,
    pub provider_requests: u64,
    pub provider_errors: u64,
    pub provider_timeouts: u64,
    pub provider_retries: u64,
    pub tool_calls: u64,
    pub tool_errors: u64,
    pub tool_calls_by_tool: BTreeMap<String, u64>,
    pub tool_errors_by_tool: BTreeMap<String, u64>,
    pub git_commit_ok: u64,
    pub git_commit_rejected: u64,
    pub git_commit_failed: u64,
    pub git_push_ok: u64,
    pub git_push_failed: u64,
    pub git_push_blocked: u64,
    pub max_noop_streak: u64,
    pub forced_mutations: u64,
    pub task_advancements: u64,
    pub error_counts_by_class: BTreeMap<ErrorClass, u64>,
    pub quality_score_100: u8,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RunRollups {
    pub by_model_executor_provider: BTreeMap<String, RunMetrics>,
    pub by_tool: BTreeMap<String, ToolRollup>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolRollup {
    pub calls: u64,
    pub errors: u64,
    pub error_counts_by_class: BTreeMap<ErrorClass, u64>,
}

#[derive(Debug, Clone, Default)]
struct RunContext {
    model: String,
    executor: String,
    provider_resolved: String,
}

impl RunContext {
    fn from_event(event: &HarnessEvent) -> Self {
        Self {
            model: string_field(&event.data, "model")
                .or_else(|| {
                    event
                        .data
                        .get("model_profile")
                        .and_then(|profile| string_field(profile, "model"))
                })
                .unwrap_or_else(|| "unknown".to_string()),
            executor: string_field(&event.data, "executor")
                .or_else(|| string_field(&event.data, "mode"))
                .unwrap_or_else(|| "unknown".to_string()),
            provider_resolved: string_field(&event.data, "provider_resolved")
                .unwrap_or_else(|| "unknown".to_string()),
        }
    }

    fn key(&self) -> String {
        format!(
            "model={} executor={} provider={}",
            self.model, self.executor, self.provider_resolved
        )
    }
}

pub struct RunMetricsCollector {
    metrics: RunMetrics,
    rollups: RunRollups,
    contexts: BTreeMap<String, RunContext>,
}

impl Default for RunMetricsCollector {
    fn default() -> Self {
        Self::new()
    }
}

impl RunMetricsCollector {
    pub fn new() -> Self {
        Self {
            metrics: RunMetrics::default(),
            rollups: RunRollups::default(),
            contexts: BTreeMap::new(),
        }
    }

    pub fn collect(events: &[HarnessEvent]) -> (RunMetrics, RunRollups) {
        let mut collector = Self::new();
        for event in events {
            collector.consume(event);
        }
        collector.finish()
    }

    pub fn consume(&mut self, event: &HarnessEvent) {
        if matches!(
            event.kind.as_str(),
            kinds::RUN_STARTED | kinds::CODING_RUN_STARTED
        ) {
            if let Some(run_id) = event.run_id.clone() {
                self.contexts.insert(run_id, RunContext::from_event(event));
            }
        }

        apply_event_to_metrics(&mut self.metrics, event);

        let group_key = self.group_key(event);
        let group = self
            .rollups
            .by_model_executor_provider
            .entry(group_key)
            .or_default();
        apply_event_to_metrics(group, event);

        self.apply_tool_rollup(event);
    }

    pub fn finish(mut self) -> (RunMetrics, RunRollups) {
        finish_metrics(&mut self.metrics);
        for metrics in self.rollups.by_model_executor_provider.values_mut() {
            finish_metrics(metrics);
        }
        (self.metrics, self.rollups)
    }

    fn group_key(&self, event: &HarnessEvent) -> String {
        event
            .run_id
            .as_ref()
            .and_then(|run_id| self.contexts.get(run_id))
            .cloned()
            .unwrap_or_else(|| RunContext::from_event(event))
            .key()
    }

    fn apply_tool_rollup(&mut self, event: &HarnessEvent) {
        match event.kind.as_str() {
            kinds::TOOL_CALL => {
                let tool = string_field(&event.data, "tool").unwrap_or_else(|| "unknown".into());
                self.rollups.by_tool.entry(tool).or_default().calls += 1;
            }
            kinds::TOOL_ERROR => {
                let tool = string_field(&event.data, "tool").unwrap_or_else(|| "unknown".into());
                let classified = ErrorClassifier::classify_event(event);
                let rollup = self.rollups.by_tool.entry(tool).or_default();
                rollup.errors += 1;
                if let Some(classified) = classified {
                    *rollup
                        .error_counts_by_class
                        .entry(classified.class)
                        .or_default() += 1;
                }
            }
            _ => {}
        }
    }
}

fn apply_event_to_metrics(metrics: &mut RunMetrics, event: &HarnessEvent) {
    if let Some(run_id) = event.run_id.clone() {
        metrics.run_ids.insert(run_id);
    }

    if matches!(
        event.kind.as_str(),
        kinds::RUN_STARTED | kinds::CODING_RUN_STARTED
    ) {
        let context = RunContext::from_event(event);
        metrics.models.insert(context.model);
        metrics.executors.insert(context.executor);
        metrics.providers_resolved.insert(context.provider_resolved);
    }

    match event.kind.as_str() {
        kinds::CODING_CYCLE_FINISHED => {
            metrics.cycles_total += 1;
            if bool_field(&event.data, "success").unwrap_or(false) {
                metrics.cycles_succeeded += 1;
            } else {
                metrics.cycles_failed += 1;
            }
        }
        kinds::CODING_CYCLE_ACT => {
            if bool_field(&event.data, "success").unwrap_or(false) {
                metrics.act_success += 1;
            } else {
                metrics.act_fail += 1;
            }
        }
        kinds::CODING_CYCLE_VERIFY => {
            if bool_field(&event.data, "success").unwrap_or(false) {
                metrics.verify_success += 1;
            } else {
                metrics.verify_fail += 1;
            }
        }
        kinds::PROVIDER_REQUEST => metrics.provider_requests += 1,
        kinds::PROVIDER_ERROR => metrics.provider_errors += 1,
        kinds::PROVIDER_TIMEOUT => metrics.provider_timeouts += 1,
        kinds::PROVIDER_RETRY => metrics.provider_retries += 1,
        kinds::TOOL_CALL => {
            metrics.tool_calls += 1;
            let tool = string_field(&event.data, "tool").unwrap_or_else(|| "unknown".into());
            *metrics.tool_calls_by_tool.entry(tool).or_default() += 1;
        }
        kinds::TOOL_ERROR => {
            metrics.tool_errors += 1;
            let tool = string_field(&event.data, "tool").unwrap_or_else(|| "unknown".into());
            *metrics.tool_errors_by_tool.entry(tool).or_default() += 1;
        }
        kinds::GIT_COMMIT => match string_field(&event.data, "result").as_deref() {
            Some("ok") => metrics.git_commit_ok += 1,
            Some("rejected") => metrics.git_commit_rejected += 1,
            Some("failed") => metrics.git_commit_failed += 1,
            _ => {}
        },
        kinds::GIT_PUSH => match string_field(&event.data, "result").as_deref() {
            Some("ok") => metrics.git_push_ok += 1,
            Some("failed") => metrics.git_push_failed += 1,
            Some("blocked") => metrics.git_push_blocked += 1,
            _ => {}
        },
        kinds::CODING_COUNTER => {
            if let Some(noop_streak) = u64_field(&event.data, "noop_streak") {
                metrics.max_noop_streak = metrics.max_noop_streak.max(noop_streak);
            }
            if let Some(forced_mutation) = u64_field(&event.data, "forced_mutation") {
                metrics.forced_mutations = metrics.forced_mutations.max(forced_mutation);
            }
            if let Some(task_advanced) = u64_field(&event.data, "task_advanced") {
                metrics.task_advancements = metrics.task_advancements.max(task_advanced);
            }
        }
        _ => {}
    }

    if let Some(classified) = ErrorClassifier::classify_event(event) {
        *metrics
            .error_counts_by_class
            .entry(classified.class)
            .or_default() += 1;
    }
}

fn finish_metrics(metrics: &mut RunMetrics) {
    metrics.quality_score_100 = quality_score(metrics);
}

fn quality_score(metrics: &RunMetrics) -> u8 {
    let unknown_errors = count_error(metrics, ErrorClass::Unknown);
    let penalty = (unknown_errors * 20).min(40)
        + (metrics.tool_errors * 6).min(24)
        + ((metrics.provider_errors + metrics.provider_timeouts) * 8).min(24)
        + (metrics.git_commit_rejected * 6).min(24)
        + (metrics.git_commit_failed * 10).min(20)
        + (metrics.cycles_failed * 10).min(30)
        + (metrics.max_noop_streak * 5).min(20);

    100u64.saturating_sub(penalty).min(100) as u8
}

fn count_error(metrics: &RunMetrics, class: ErrorClass) -> u64 {
    metrics
        .error_counts_by_class
        .get(&class)
        .copied()
        .unwrap_or(0)
}

fn string_field(data: &Value, key: &str) -> Option<String> {
    data.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
}

fn bool_field(data: &Value, key: &str) -> Option<bool> {
    data.get(key).and_then(Value::as_bool)
}

fn u64_field(data: &Value, key: &str) -> Option<u64> {
    data.get(key).and_then(Value::as_u64)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn collector_counts_coding_and_error_metrics() {
        let events = vec![
            HarnessEvent::new(kinds::RUN_STARTED)
                .with_run_id("r1")
                .with_data(json!({
                    "model": "gpt-5.3-codex",
                    "executor": "openclaw",
                    "provider_resolved": "http"
                })),
            HarnessEvent::new(kinds::CODING_CYCLE_ACT)
                .with_run_id("r1")
                .with_data(json!({"success": true})),
            HarnessEvent::new(kinds::CODING_CYCLE_VERIFY)
                .with_run_id("r1")
                .with_data(json!({"success": true})),
            HarnessEvent::new(kinds::CODING_CYCLE_FINISHED)
                .with_run_id("r1")
                .with_data(json!({"success": true})),
            HarnessEvent::new(kinds::PROVIDER_REQUEST).with_run_id("r1"),
            HarnessEvent::new(kinds::PROVIDER_ERROR)
                .with_run_id("r1")
                .with_data(json!({
                    "error": "provider returned non-success status=500",
                    "error_class": "provider_error"
                })),
            HarnessEvent::new(kinds::TOOL_CALL)
                .with_run_id("r1")
                .with_data(json!({"tool": "echo"})),
            HarnessEvent::new(kinds::TOOL_ERROR)
                .with_run_id("r1")
                .with_data(json!({
                    "tool": "echo",
                    "error": "tool blocked by policy: echo"
                })),
            HarnessEvent::new(kinds::GIT_COMMIT)
                .with_run_id("r1")
                .with_data(json!({
                    "success": false,
                    "skipped": true,
                    "result": "rejected",
                    "detail": "commit subject rejected by quality gate"
                })),
            HarnessEvent::new(kinds::CODING_COUNTER)
                .with_run_id("r1")
                .with_data(json!({
                    "noop_streak": 2,
                    "forced_mutation": 1,
                    "task_advanced": 1
                })),
        ];

        let (metrics, rollups) = RunMetricsCollector::collect(&events);

        assert_eq!(metrics.cycles_total, 1);
        assert_eq!(metrics.cycles_succeeded, 1);
        assert_eq!(metrics.act_success, 1);
        assert_eq!(metrics.verify_success, 1);
        assert_eq!(metrics.provider_requests, 1);
        assert_eq!(metrics.provider_errors, 1);
        assert_eq!(metrics.tool_calls, 1);
        assert_eq!(metrics.tool_errors, 1);
        assert_eq!(metrics.git_commit_rejected, 1);
        assert_eq!(metrics.max_noop_streak, 2);
        assert_eq!(metrics.forced_mutations, 1);
        assert_eq!(metrics.task_advancements, 1);
        assert_eq!(
            metrics
                .error_counts_by_class
                .get(&ErrorClass::ProviderError)
                .copied(),
            Some(1)
        );
        assert!(metrics.quality_score_100 < 100);
        assert!(rollups
            .by_model_executor_provider
            .contains_key("model=gpt-5.3-codex executor=openclaw provider=http"));
        assert_eq!(rollups.by_tool.get("echo").unwrap().calls, 1);
        assert_eq!(rollups.by_tool.get("echo").unwrap().errors, 1);
    }
}
