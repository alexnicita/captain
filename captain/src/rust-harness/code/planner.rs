use crate::code::engine::CodePlanner;
use crate::code::task::{ArchitecturePlan, ArchitecturePlanStep, CodeTask};
use crate::provider::{Provider, ProviderRequest};
use anyhow::Result;
use async_trait::async_trait;
use std::sync::Arc;

pub struct ProviderCodePlanner {
    provider: Arc<dyn Provider>,
}

impl ProviderCodePlanner {
    pub fn new(provider: Arc<dyn Provider>) -> Self {
        Self { provider }
    }
}

#[async_trait]
impl CodePlanner for ProviderCodePlanner {
    async fn plan_task(&self, task: &CodeTask, repo_snapshot: &str) -> Result<ArchitecturePlan> {
        let constraints = if task.constraints.is_empty() {
            "none".to_string()
        } else {
            task.constraints.join("; ")
        };

        let objective = format!(
            "Create an architecture-first coding plan. Objective: {}. Architecture goal: {}. Constraints: {}. Return concise implementation steps.",
            task.objective, task.architecture_goal, constraints
        );

        let req = ProviderRequest {
            objective,
            context: vec![
                format!("task_id={}", task.id),
                format!("target_files={}", task.target_files.join(",")),
                format!(
                    "acceptance_criteria={}",
                    task.acceptance_criteria.join(" | ")
                ),
                format!("repo_snapshot={repo_snapshot}"),
            ],
            available_tools: vec![],
        };

        let resp = self.provider.generate(&req).await?;
        let steps = parse_steps(&resp.message, &task.target_files);

        Ok(ArchitecturePlan {
            summary: resp
                .message
                .lines()
                .next()
                .unwrap_or("planned via provider")
                .to_string(),
            steps,
            risk_checks: task.acceptance_criteria.clone(),
        })
    }
}

fn parse_steps(message: &str, target_files: &[String]) -> Vec<ArchitecturePlanStep> {
    let mut steps = Vec::new();

    for line in message.lines().take(6) {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if is_step_line(trimmed) {
            steps.push(ArchitecturePlanStep {
                title: strip_step_prefix(trimmed).to_string(),
                rationale: "generated from provider planning response".to_string(),
                expected_files: target_files.to_vec(),
            });
        }
    }

    if steps.is_empty() {
        steps.push(ArchitecturePlanStep {
            title: "Implement smallest viable change that advances objective".to_string(),
            rationale: "fallback plan when provider returns unstructured text".to_string(),
            expected_files: target_files.to_vec(),
        });
    }

    steps
}

fn is_step_line(line: &str) -> bool {
    if line.starts_with('-') || line.starts_with('*') {
        return true;
    }

    let digit_prefix_len = line.chars().take_while(|c| c.is_ascii_digit()).count();
    if digit_prefix_len == 0 {
        return false;
    }

    matches!(line.chars().nth(digit_prefix_len), Some('.') | Some(')'))
}

fn strip_step_prefix(line: &str) -> &str {
    line.trim_start_matches(|c: char| c == '-' || c == '*' || c.is_ascii_digit() || c == '.' || c == ')')
        .trim()
}

#[cfg(test)]
mod tests {
    use super::{is_step_line, parse_steps};

    #[test]
    fn numeric_step_requires_delimiter() {
        assert!(is_step_line("1. do thing"));
        assert!(is_step_line("2) do thing"));
        assert!(!is_step_line("2026 roadmap item"));
    }

    #[test]
    fn parse_steps_uses_fallback_for_unstructured_lines() {
        let steps = parse_steps("Architecture: improve reliability", &["src/a.rs".to_string()]);
        assert_eq!(steps.len(), 1);
        assert!(steps[0]
            .title
            .contains("Implement smallest viable change that advances objective"));
    }
}
