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
                format!("acceptance_criteria={}", task.acceptance_criteria.join(" | ")),
                format!("repo_snapshot={repo_snapshot}"),
            ],
            available_tools: vec![],
        };

        let resp = self.provider.generate(&req).await?;
        let steps = parse_steps(&resp.message, &task.target_files);

        Ok(ArchitecturePlan {
            summary: resp.message.lines().next().unwrap_or("planned via provider").to_string(),
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
        if trimmed.starts_with('-') || trimmed.starts_with('*') || trimmed.chars().next().is_some_and(|c| c.is_ascii_digit()) {
            steps.push(ArchitecturePlanStep {
                title: trimmed
                    .trim_start_matches(|c: char| c == '-' || c == '*' || c.is_ascii_digit() || c == '.' || c == ')')
                    .trim()
                    .to_string(),
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
