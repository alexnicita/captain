use crate::code::engine::CodeDiffGenerator;
use crate::code::task::{ArchitecturePlan, CodeDiffProposal, CodeTask};
use crate::provider::{Provider, ProviderRequest};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use std::sync::Arc;

pub struct ProviderDiffGenerator {
    provider: Arc<dyn Provider>,
}

impl ProviderDiffGenerator {
    pub fn new(provider: Arc<dyn Provider>) -> Self {
        Self { provider }
    }
}

#[async_trait]
impl CodeDiffGenerator for ProviderDiffGenerator {
    async fn generate_diff(
        &self,
        task: &CodeTask,
        plan: &ArchitecturePlan,
        repo_snapshot: &str,
    ) -> Result<CodeDiffProposal> {
        let objective = format!(
            "Generate a unified git diff for task {}. Follow plan summary: {}. Return only a patch in ```diff``` when possible.",
            task.id, plan.summary
        );

        let req = ProviderRequest {
            objective,
            context: vec![
                format!("task_objective={}", task.objective),
                format!("target_files={}", task.target_files.join(",")),
                format!("repo_snapshot={repo_snapshot}"),
            ],
            available_tools: vec![],
        };

        let resp = self.provider.generate(&req).await?;
        let patch = extract_diff(&resp.message)
            .ok_or_else(|| anyhow!("provider response did not include a unified diff"))?;
        let touched_files = extract_touched_files(&patch);

        Ok(CodeDiffProposal {
            summary: format!("provider generated patch touching {} files", touched_files.len()),
            unified_diff: patch,
            touched_files,
        })
    }
}

fn extract_diff(message: &str) -> Option<String> {
    if let Some(start) = message.find("```diff") {
        let rest = &message[start + 7..];
        if let Some(end) = rest.find("```") {
            return Some(rest[..end].trim().to_string());
        }
    }

    if message.contains("diff --git") {
        return Some(message.trim().to_string());
    }

    None
}

fn extract_touched_files(diff: &str) -> Vec<String> {
    let mut files = Vec::new();
    for line in diff.lines() {
        if let Some(path) = line.strip_prefix("+++ b/") {
            files.push(path.trim().to_string());
        }
    }
    files.sort();
    files.dedup();
    files
}
