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
            "Generate a valid unified git diff for task {}. Follow plan summary: {}. Return ONLY raw patch text that starts with 'diff --git'. If no valid patch can be produced, return exactly NO_VALID_PATCH.",
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
            summary: format!(
                "provider generated patch touching {} files",
                touched_files.len()
            ),
            unified_diff: patch,
            touched_files,
        })
    }
}

fn extract_diff(message: &str) -> Option<String> {
    let trimmed = message.trim();
    if trimmed.eq_ignore_ascii_case("NO_VALID_PATCH") {
        return None;
    }

    if let Some(start) = trimmed.find("```diff") {
        let rest = &trimmed[start + 7..];
        if let Some(end) = rest.find("```") {
            return sanitize_unified_diff(&rest[..end]);
        }
    }

    if let Some(start) = trimmed.find("diff --git") {
        return sanitize_unified_diff(&trimmed[start..]);
    }

    None
}

fn sanitize_unified_diff(raw: &str) -> Option<String> {
    let mut lines = raw.lines().map(str::trim_end).collect::<Vec<_>>();

    let first_diff = lines
        .iter()
        .position(|line| line.starts_with("diff --git "))?;
    let mut normalized = lines.split_off(first_diff);

    while matches!(normalized.last(), Some(last) if last.trim().is_empty() || last.trim() == "```")
    {
        normalized.pop();
    }

    Some(normalized.join("\n"))
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
