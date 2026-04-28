use crate::code::engine::CodeDiffGenerator;
use crate::code::task::{ArchitecturePlan, CodeDiffProposal, CodeTask};
use crate::provider::{Provider, ProviderRequest};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::Value;
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
            "Generate a valid unified git diff for task {}. Follow plan summary: {}. Return ONLY raw patch text that starts with 'diff --git'. If valid diff formatting is not possible, return JSON: {{\"edits\":[{{\"path\":\"relative/file.rs\",\"content\":\"full new file content\"}}]}}",
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
        if let Some(patch) = extract_diff(&resp.message) {
            let touched_files = extract_touched_files(&patch);
            return Ok(CodeDiffProposal {
                summary: format!(
                    "provider generated patch touching {} files",
                    touched_files.len()
                ),
                unified_diff: patch,
                touched_files,
            });
        }

        if let Some((paths, sentinel)) = extract_json_edits(&resp.message) {
            return Ok(CodeDiffProposal {
                summary: format!(
                    "provider generated json edits touching {} files",
                    paths.len()
                ),
                unified_diff: sentinel,
                touched_files: paths,
            });
        }

        Err(anyhow!(
            "provider response did not include a unified diff or json edits"
        ))
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

fn extract_json_edits(message: &str) -> Option<(Vec<String>, String)> {
    let trimmed = message.trim();

    let json_candidate = if trimmed.starts_with("```") {
        let start = trimmed.find('{')?;
        let end = trimmed.rfind('}')?;
        &trimmed[start..=end]
    } else {
        trimmed
    };

    let payload: Value = serde_json::from_str(json_candidate).ok()?;
    let edits = payload.get("edits")?.as_array()?;
    if edits.is_empty() {
        return None;
    }

    let mut paths = Vec::new();
    for edit in edits {
        let path = edit.get("path")?.as_str()?.trim();
        let content = edit.get("content")?.as_str()?;
        if path.is_empty() || path.starts_with('/') || path.contains("..") || content.is_empty() {
            return None;
        }
        paths.push(path.to_string());
    }

    let mut dedup = paths.clone();
    dedup.sort();
    dedup.dedup();
    if dedup.len() != paths.len() {
        return None;
    }

    paths = dedup;

    let sentinel = format!("HARNESS_JSON_EDITS\n{payload}");
    Some((paths, sentinel))
}

fn extract_touched_files(diff: &str) -> Vec<String> {
    let mut files = Vec::new();
    for line in diff.lines() {
        if let Some(path) = line
            .strip_prefix("+++ b/")
            .or_else(|| line.strip_prefix("--- a/"))
        {
            let trimmed = path.trim();
            if !trimmed.is_empty() && trimmed != "/dev/null" {
                files.push(trimmed.to_string());
            }
        }
    }
    files.sort();
    files.dedup();
    files
}

#[cfg(test)]
mod tests {
    use super::{extract_json_edits, extract_touched_files};

    #[test]
    fn touched_files_includes_add_delete_and_rename_paths() {
        let diff = r#"
diff --git a/old.txt b/new.txt
similarity index 100%
rename from old.txt
rename to new.txt
--- a/old.txt
+++ b/new.txt
@@ -1 +1 @@
-old
+new
diff --git a/src/gone.rs b/src/gone.rs
deleted file mode 100644
--- a/src/gone.rs
+++ /dev/null
@@ -1 +0,0 @@
-fn gone() {}
diff --git a/src/added.rs b/src/added.rs
new file mode 100644
--- /dev/null
+++ b/src/added.rs
@@ -0,0 +1 @@
+fn added() {}
"#;

        let files = extract_touched_files(diff);
        assert_eq!(files, vec!["new.txt", "old.txt", "src/added.rs", "src/gone.rs"]);
    }

    #[test]
    fn json_edits_rejects_duplicate_paths() {
        let dup = r#"{"edits":[{"path":"src/a.rs","content":"fn a() {}"},{"path":"src/a.rs","content":"fn b() {}"}]}"#;
        assert!(extract_json_edits(dup).is_none());
    }
}
