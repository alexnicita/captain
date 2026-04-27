use crate::coding::FeatureTask;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::Path;

const TASK_SELECTION_COOLDOWN_CYCLES: u64 = 2;

#[derive(Debug, Clone)]
struct RankedTaskCandidate {
    task: FeatureTask,
    score: i64,
    impact: i64,
    novelty: i64,
    cooldown_remaining: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub(crate) struct TaskProgressMemory {
    pub(crate) completed_roadmap_lines: BTreeSet<String>,
    pub(crate) attempted_task_ids: BTreeSet<String>,
    pub(crate) completed_task_ids: BTreeSet<String>,
    pub(crate) repeated_no_diff_task_id: Option<String>,
    pub(crate) repeated_no_diff_cycles: u64,
    pub(crate) source_escalation_count: u64,
    pub(crate) task_history: BTreeMap<String, TaskHistory>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub(crate) struct TaskHistory {
    pub(crate) selected_count: u64,
    pub(crate) last_selected_cycle: u64,
    pub(crate) last_outcome: Option<String>,
}

pub(crate) fn select_next_feature_task_from_docs(
    repo_path: &Path,
    progress: &TaskProgressMemory,
    cycle: u64,
    escalate_source: bool,
) -> Option<FeatureTask> {
    let mut ranked = rank_task_candidates(repo_path, progress, cycle, escalate_source);
    ranked.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then_with(|| b.impact.cmp(&a.impact))
            .then_with(|| b.novelty.cmp(&a.novelty))
            .then_with(|| a.task.id.cmp(&b.task.id))
    });

    ranked
        .iter()
        .filter(|candidate| candidate.cooldown_remaining == 0 || escalate_source)
        .find(|candidate| candidate.score >= 420 && candidate.impact >= 5)
        .map(|candidate| candidate.task.clone())
        .or_else(|| {
            ranked
                .iter()
                .find(|candidate| candidate.cooldown_remaining == 0 || escalate_source)
                .map(|candidate| candidate.task.clone())
        })
        .or_else(|| ranked.first().map(|candidate| candidate.task.clone()))
}

fn rank_task_candidates(
    repo_path: &Path,
    progress: &TaskProgressMemory,
    cycle: u64,
    escalate_source: bool,
) -> Vec<RankedTaskCandidate> {
    let supercycle_tasks = collect_supercycle_tasks(repo_path);
    let doc_tasks = collect_doc_tasks(repo_path, escalate_source);

    let mut tasks = Vec::new();
    tasks.extend(supercycle_tasks.clone());
    tasks.extend(doc_tasks);

    if tasks.is_empty() {
        tasks.extend(internal_fallback_tasks());
    }

    let has_supercycle_tasks = !supercycle_tasks.is_empty();
    let has_code_tasks = tasks.iter().any(|task| task.source.starts_with("src/"));

    let mut ranked = Vec::new();
    for task in tasks {
        if progress.completed_task_ids.contains(&task.id) {
            continue;
        }

        let history = progress
            .task_history
            .get(&task.id)
            .cloned()
            .unwrap_or_default();
        let cooldown_remaining = cooldown_remaining(cycle, history.last_selected_cycle);

        if progress.attempted_task_ids.contains(&task.id)
            && cooldown_remaining > 0
            && !escalate_source
        {
            continue;
        }

        let impact = task_impact_score(&task);
        let novelty = task_novelty_score(&task, progress, &history);
        let feasibility = task_feasibility_score(repo_path, &task);
        let cooldown_penalty = (cooldown_remaining as i64) * 120;
        let repeat_penalty = (history.selected_count as i64) * 18;
        let fallback_bonus = if task.id.starts_with("fallback::") {
            60
        } else {
            0
        };
        let supercycle_bonus = if task.source.contains("TASK_PACK") {
            420
        } else {
            0
        };
        let docs_penalty = if has_code_tasks && task.source.ends_with(".md") && !escalate_source {
            320
        } else {
            0
        };
        let fallback_penalty_when_supercycle =
            if has_supercycle_tasks && task.id.starts_with("fallback::") {
                600
            } else {
                0
            };
        let score = impact * 100 + novelty + feasibility * 25 + fallback_bonus + supercycle_bonus
            - cooldown_penalty
            - repeat_penalty
            - docs_penalty
            - fallback_penalty_when_supercycle;

        ranked.push(RankedTaskCandidate {
            task,
            score,
            impact,
            novelty,
            cooldown_remaining,
        });
    }

    ranked
}

pub(crate) fn ensure_roadmap_seed_tasks(repo_path: &Path) -> Result<()> {
    let roadmap_path = repo_path.join("ROADMAP.md");
    let runbook_path = repo_path.join("RUNBOOK.md");

    if !roadmap_path.exists() {
        let body = "# ROADMAP\n\n## Notes\n- Task generation is driven by supercycle planning artifacts (`.harness/supercycle/*TASK_PACK*.md`) first.\n- Keep this file for human roadmap notes; avoid auto-seeded fallback task lists.\n";
        fs::write(&roadmap_path, body)?;
    }

    if !runbook_path.exists() {
        let body = "# RUNBOOK\n\n## Coding loop notes\n- Keep tasks scoped to src/ and tests\n- Reject non-meaningful diffs\n";
        fs::write(&runbook_path, body)?;
    }

    Ok(())
}

fn collect_supercycle_tasks(repo_path: &Path) -> Vec<FeatureTask> {
    let dir = repo_path.join(".harness/supercycle");
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };

    let mut pack_files = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|s| s.to_str())
                .map(|n| n.contains("TASK_PACK") && n.ends_with(".md"))
                .unwrap_or(false)
        })
        .collect::<Vec<_>>();
    pack_files.sort();

    let Some(latest_pack) = pack_files.pop() else {
        return Vec::new();
    };

    let Ok(content) = fs::read_to_string(&latest_pack) else {
        return Vec::new();
    };

    let source_name = latest_pack
        .strip_prefix(repo_path)
        .unwrap_or(&latest_pack)
        .display()
        .to_string();

    let mut tasks = Vec::new();
    for raw_line in content.lines() {
        let line = raw_line.trim();
        if !looks_like_roadmap_task(line) {
            continue;
        }

        let task_id = format!("{}::{}", source_name, slugify_task_line(line));
        tasks.push(FeatureTask {
            id: task_id,
            title: line
                .trim_start_matches('-')
                .trim_start_matches('*')
                .trim_start_matches("[ ]")
                .trim()
                .to_string(),
            source: source_name.clone(),
            selected_line: line.to_string(),
        });
    }

    tasks
}

fn collect_doc_tasks(repo_path: &Path, escalate_source: bool) -> Vec<FeatureTask> {
    let primary = [
        "ARCHITECTURE.md",
        "ROADMAP.md",
        "README.md",
        "RUNBOOK.md",
        "MIGRATION.md",
    ];
    let fallback_only = ["CONTRIBUTING.md"];

    let mut files = primary.to_vec();
    if escalate_source {
        files.extend(fallback_only);
    }

    let mut tasks = Vec::new();
    for file in files {
        let path = repo_path.join(file);
        let Ok(content) = fs::read_to_string(&path) else {
            continue;
        };

        for raw_line in content.lines() {
            let line = raw_line.trim();
            if !looks_like_roadmap_task(line) {
                continue;
            }

            let task_id = format!("{}::{}", file, slugify_task_line(line));
            tasks.push(FeatureTask {
                id: task_id,
                title: line
                    .trim_start_matches('-')
                    .trim_start_matches('*')
                    .trim_start_matches("[ ]")
                    .trim()
                    .to_string(),
                source: file.to_string(),
                selected_line: line.to_string(),
            });
        }
    }

    tasks
}

fn looks_like_roadmap_task(line: &str) -> bool {
    if line.is_empty() {
        return false;
    }

    let roadmap_hint = line.to_ascii_lowercase();
    let is_actionable = line.starts_with("- ")
        || line.starts_with('*')
        || line
            .chars()
            .next()
            .map(|ch| ch.is_ascii_digit())
            .unwrap_or(false);
    let looks_like_roadmap = roadmap_hint.contains("planned")
        || roadmap_hint.contains("next")
        || roadmap_hint.contains("todo")
        || roadmap_hint.contains("increment")
        || roadmap_hint.contains("feature")
        || roadmap_hint.contains("improve")
        || roadmap_hint.contains("harden")
        || roadmap_hint.contains("fix")
        || roadmap_hint.contains("refactor")
        || line.starts_with("- [ ]");

    is_actionable && looks_like_roadmap
}

fn internal_fallback_tasks() -> Vec<FeatureTask> {
    vec![
        FeatureTask {
            id: "fallback::src/coding.rs::tighten-commit-subject-gate".to_string(),
            title: "Tighten commit-subject quality gate in src/coding.rs".to_string(),
            source: "src/coding.rs".to_string(),
            selected_line: "Implement deterministic informative commit subjects for staged files"
                .to_string(),
        },
        FeatureTask {
            id: "fallback::src/coding.rs::improve-task-ranking".to_string(),
            title: "Improve task ranking/cooldown logic in src/coding.rs".to_string(),
            source: "src/coding.rs".to_string(),
            selected_line: "Rank architecture tasks by impact and novelty; avoid repeats"
                .to_string(),
        },
        FeatureTask {
            id: "fallback::src/main.rs::strengthen-lock-observability".to_string(),
            title: "Strengthen lock refusal observability in src/main.rs".to_string(),
            source: "src/main.rs".to_string(),
            selected_line: "Fail fast on concurrent runs with clear process exit reason"
                .to_string(),
        },
    ]
}

pub(crate) fn select_forced_code_change_task(
    progress: &TaskProgressMemory,
    cycle: u64,
) -> FeatureTask {
    let mut forced = internal_fallback_tasks()
        .into_iter()
        .filter(|task| task.source.starts_with("src/"))
        .collect::<Vec<_>>();

    forced.sort_by(|a, b| {
        let ah = progress
            .task_history
            .get(&a.id)
            .cloned()
            .unwrap_or_default();
        let bh = progress
            .task_history
            .get(&b.id)
            .cloned()
            .unwrap_or_default();

        ah.selected_count
            .cmp(&bh.selected_count)
            .then_with(|| ah.last_selected_cycle.cmp(&bh.last_selected_cycle))
            .then_with(|| a.id.cmp(&b.id))
    });

    let mut selected = forced.into_iter().next().unwrap_or(FeatureTask {
        id: "fallback::src/coding.rs::recover-no-diff-streak".to_string(),
        title: "Recover no-diff streak with scoped coding.rs change".to_string(),
        source: "src/coding.rs".to_string(),
        selected_line: "Apply a concrete code change to break no-diff streak".to_string(),
    });

    selected.id = format!("{}::forced-cycle-{}", selected.id, cycle);
    selected
}

fn cooldown_remaining(cycle: u64, last_selected_cycle: u64) -> u64 {
    if last_selected_cycle == 0 {
        return 0;
    }
    let age = cycle.saturating_sub(last_selected_cycle);
    TASK_SELECTION_COOLDOWN_CYCLES.saturating_sub(age)
}

fn task_novelty_score(
    task: &FeatureTask,
    progress: &TaskProgressMemory,
    history: &TaskHistory,
) -> i64 {
    let mut score = 30i64;

    if progress.attempted_task_ids.contains(&task.id) {
        score -= 20;
    }
    if let Some(outcome) = history.last_outcome.as_deref() {
        if outcome == "no_diff" {
            score -= 15;
        }
    }

    score - (history.selected_count as i64 * 4)
}

fn task_feasibility_score(repo_path: &Path, task: &FeatureTask) -> i64 {
    let mut score = 0;
    if task.source.starts_with("src/") {
        score += 2;
    }
    if repo_path.join(&task.source).exists() {
        score += 2;
    }
    if task.title.len() <= 120 {
        score += 1;
    }
    score
}

fn task_impact_score(task: &FeatureTask) -> i64 {
    let mut score = if task.source.starts_with("src/") {
        7
    } else {
        4
    };
    let text = format!("{} {}", task.title, task.selected_line).to_ascii_lowercase();

    for keyword in [
        "security",
        "harden",
        "fail",
        "abort",
        "lock",
        "concurrency",
        "correctness",
    ] {
        if text.contains(keyword) {
            score += 3;
        }
    }
    for keyword in [
        "test",
        "coverage",
        "regression",
        "observe",
        "event",
        "commit",
        "push",
    ] {
        if text.contains(keyword) {
            score += 2;
        }
    }
    for keyword in ["docs", "readme", "runbook"] {
        if text.contains(keyword) {
            score += 1;
        }
    }

    score
}

pub(crate) fn record_task_selection(
    progress: &mut TaskProgressMemory,
    task: &FeatureTask,
    cycle: u64,
) {
    progress.attempted_task_ids.insert(task.id.clone());
    let history = progress.task_history.entry(task.id.clone()).or_default();
    history.selected_count = history.selected_count.saturating_add(1);
    history.last_selected_cycle = cycle;
}

pub(crate) fn record_task_outcome(progress: &mut TaskProgressMemory, task_id: &str, outcome: &str) {
    let history = progress
        .task_history
        .entry(task_id.to_string())
        .or_default();
    history.last_outcome = Some(outcome.to_string());
}

fn slugify_task_line(line: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = false;
    for ch in line.chars() {
        let mapped = if ch.is_ascii_alphanumeric() {
            ch.to_ascii_lowercase()
        } else {
            '-'
        };
        if mapped == '-' {
            if !prev_dash {
                out.push(mapped);
                prev_dash = true;
            }
        } else {
            out.push(mapped);
            prev_dash = false;
        }
    }
    out.trim_matches('-').to_string()
}
