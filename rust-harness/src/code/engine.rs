use crate::code::task::{
    ArchitecturePlan, CodeApplyResult, CodeCycleReport, CodeDiffProposal, CodeTask,
};
use anyhow::{anyhow, Result};
use async_trait::async_trait;
use std::path::Path;
use std::sync::Arc;

#[async_trait]
pub trait CodePlanner: Send + Sync {
    async fn plan_task(&self, task: &CodeTask, repo_snapshot: &str) -> Result<ArchitecturePlan>;
}

#[async_trait]
pub trait CodeDiffGenerator: Send + Sync {
    async fn generate_diff(
        &self,
        task: &CodeTask,
        plan: &ArchitecturePlan,
        repo_snapshot: &str,
    ) -> Result<CodeDiffProposal>;
}

#[async_trait]
pub trait CodeDiffApplier: Send + Sync {
    async fn apply_diff(&self, repo_path: &Path, proposal: &CodeDiffProposal) -> Result<CodeApplyResult>;
}

pub struct CodeCycleEngine {
    planner: Arc<dyn CodePlanner>,
    diff_generator: Arc<dyn CodeDiffGenerator>,
    diff_applier: Arc<dyn CodeDiffApplier>,
}

impl CodeCycleEngine {
    pub fn new(
        planner: Arc<dyn CodePlanner>,
        diff_generator: Arc<dyn CodeDiffGenerator>,
        diff_applier: Arc<dyn CodeDiffApplier>,
    ) -> Self {
        Self {
            planner,
            diff_generator,
            diff_applier,
        }
    }

    pub async fn run_cycle(
        &self,
        repo_path: &Path,
        task: &CodeTask,
        repo_snapshot: &str,
    ) -> Result<CodeCycleReport> {
        let plan = self.planner.plan_task(task, repo_snapshot).await?;
        let proposal = self
            .diff_generator
            .generate_diff(task, &plan, repo_snapshot)
            .await?;

        if proposal.unified_diff.trim().is_empty() {
            return Err(anyhow!("diff generator returned an empty patch"));
        }

        let apply_result = self.diff_applier.apply_diff(repo_path, &proposal).await?;
        let summary = format!(
            "task={} applied={} changed_files={}",
            task.id,
            apply_result.applied,
            apply_result.changed_files.len()
        );

        Ok(CodeCycleReport {
            task_id: task.id.clone(),
            planned: plan,
            diff_generated: proposal,
            diff_applied: apply_result,
            summary,
        })
    }
}
