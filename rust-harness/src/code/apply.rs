use crate::code::engine::CodeDiffApplier;
use crate::code::task::{CodeApplyResult, CodeDiffProposal};
use crate::events::now_unix;
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use serde_json::Value;
use std::fs;
use std::path::{Path, PathBuf};
use tokio::process::Command;

pub struct GitApplyDiffApplier;

impl Default for GitApplyDiffApplier {
    fn default() -> Self {
        Self
    }
}

#[async_trait]
impl CodeDiffApplier for GitApplyDiffApplier {
    async fn apply_diff(
        &self,
        repo_path: &Path,
        proposal: &CodeDiffProposal,
    ) -> Result<CodeApplyResult> {
        if proposal.unified_diff.starts_with("HARNESS_JSON_EDITS\n") {
            return apply_json_edits(repo_path, &proposal.unified_diff).await;
        }

        let patch_path = write_patch(repo_path, &proposal.unified_diff)?;

        let apply = Command::new("git")
            .arg("apply")
            .arg("--index")
            .arg("--whitespace=nowarn")
            .arg(&patch_path)
            .current_dir(repo_path)
            .output()
            .await?;

        if !apply.status.success() {
            let first_err = String::from_utf8_lossy(&apply.stderr).trim().to_string();

            // Retry with --recount to recover from occasional hunk-header drift.
            let retry = Command::new("git")
                .arg("apply")
                .arg("--index")
                .arg("--recount")
                .arg("--whitespace=nowarn")
                .arg(&patch_path)
                .current_dir(repo_path)
                .output()
                .await?;

            if !retry.status.success() {
                let retry_err = String::from_utf8_lossy(&retry.stderr).trim().to_string();
                return Ok(CodeApplyResult {
                    applied: false,
                    changed_files: Vec::new(),
                    detail: format!("{first_err}; retry(--recount)={retry_err}"),
                });
            }
        }

        let changed_files = staged_files(repo_path).await?;

        Ok(CodeApplyResult {
            applied: true,
            changed_files,
            detail: "git apply --index succeeded".to_string(),
        })
    }
}

async fn staged_files(repo_path: &Path) -> Result<Vec<String>> {
    let changed = Command::new("git")
        .arg("diff")
        .arg("--cached")
        .arg("--name-only")
        .current_dir(repo_path)
        .output()
        .await?;

    Ok(String::from_utf8_lossy(&changed.stdout)
        .lines()
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
        .collect::<Vec<_>>())
}

async fn apply_json_edits(repo_path: &Path, sentinel: &str) -> Result<CodeApplyResult> {
    let payload = sentinel
        .strip_prefix("HARNESS_JSON_EDITS\n")
        .ok_or_else(|| anyhow!("invalid json edit sentinel"))?;

    let root: Value = serde_json::from_str(payload)
        .map_err(|err| anyhow!("failed to parse json edits payload: {err}"))?;
    let edits = root
        .get("edits")
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow!("json edits payload missing edits array"))?;

    let mut changed_files = Vec::new();
    for edit in edits {
        let path = edit
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("json edit missing path"))?
            .trim();
        let content = edit
            .get("content")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow!("json edit missing content"))?;

        if path.is_empty() || path.starts_with('/') || path.contains("..") {
            return Ok(CodeApplyResult {
                applied: false,
                changed_files: Vec::new(),
                detail: format!("unsafe json edit path rejected: {path}"),
            });
        }

        let abs = repo_path.join(path);
        if let Some(parent) = abs.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&abs, content)?;

        let add = Command::new("git")
            .arg("add")
            .arg(path)
            .current_dir(repo_path)
            .output()
            .await?;
        if !add.status.success() {
            return Ok(CodeApplyResult {
                applied: false,
                changed_files: Vec::new(),
                detail: format!(
                    "git add failed for {path}: {}",
                    String::from_utf8_lossy(&add.stderr).trim()
                ),
            });
        }
        changed_files.push(path.to_string());
    }

    if changed_files.is_empty() {
        return Ok(CodeApplyResult {
            applied: false,
            changed_files,
            detail: "json edits contained no files".to_string(),
        });
    }

    let staged = staged_files(repo_path).await?;
    Ok(CodeApplyResult {
        applied: !staged.is_empty(),
        changed_files: staged,
        detail: "applied json file edits and staged with git add".to_string(),
    })
}

fn write_patch(repo_path: &Path, patch: &str) -> Result<PathBuf> {
    let dir = repo_path.join(".harness/tmp");
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("llm-patch-{}.diff", now_unix()));
    fs::write(&path, patch)?;
    Ok(path)
}
