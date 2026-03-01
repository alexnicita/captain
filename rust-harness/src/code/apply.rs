use crate::code::engine::CodeDiffApplier;
use crate::code::task::{CodeApplyResult, CodeDiffProposal};
use crate::events::now_unix;
use anyhow::Result;
use async_trait::async_trait;
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

        let changed = Command::new("git")
            .arg("diff")
            .arg("--cached")
            .arg("--name-only")
            .current_dir(repo_path)
            .output()
            .await?;

        let changed_files = String::from_utf8_lossy(&changed.stdout)
            .lines()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(ToOwned::to_owned)
            .collect::<Vec<_>>();

        Ok(CodeApplyResult {
            applied: true,
            changed_files,
            detail: "git apply --index succeeded".to_string(),
        })
    }
}

fn write_patch(repo_path: &Path, patch: &str) -> Result<PathBuf> {
    let dir = repo_path.join(".harness/tmp");
    fs::create_dir_all(&dir)?;
    let path = dir.join(format!("llm-patch-{}.diff", now_unix()));
    fs::write(&path, patch)?;
    Ok(path)
}
