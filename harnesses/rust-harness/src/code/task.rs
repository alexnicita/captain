use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeTask {
    pub id: String,
    pub objective: String,
    pub architecture_goal: String,
    pub constraints: Vec<String>,
    pub target_files: Vec<String>,
    pub acceptance_criteria: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchitecturePlanStep {
    pub title: String,
    pub rationale: String,
    pub expected_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchitecturePlan {
    pub summary: String,
    pub steps: Vec<ArchitecturePlanStep>,
    pub risk_checks: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeDiffProposal {
    pub summary: String,
    pub unified_diff: String,
    pub touched_files: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeApplyResult {
    pub applied: bool,
    pub changed_files: Vec<String>,
    pub detail: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CodeCycleReport {
    pub task_id: String,
    pub planned: ArchitecturePlan,
    pub diff_generated: CodeDiffProposal,
    pub diff_applied: CodeApplyResult,
    pub summary: String,
}
