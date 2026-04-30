use std::cmp::Ordering;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskCandidate {
    pub id: String,
    pub priority: i32,
    pub last_selected_cycle: Option<u64>,
    pub consecutive_misses: u32,
}

impl TaskCandidate {
    pub fn new(id: impl Into<String>, priority: i32) -> Self {
        Self {
            id: id.into(),
            priority,
            last_selected_cycle: None,
            consecutive_misses: 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RankingConfig {
    pub cooldown_cycles: u64,
    pub miss_boost_per_cycle: i32,
}

impl Default for RankingConfig {
    fn default() -> Self {
        Self {
            cooldown_cycles: 2,
            miss_boost_per_cycle: 1,
        }
    }
}

fn cooldown_remaining(current_cycle: u64, last_selected: Option<u64>, cooldown_cycles: u64) -> u64 {
    match last_selected {
        None => 0,
        Some(last) => {
            let elapsed = current_cycle.saturating_sub(last);
            cooldown_cycles.saturating_sub(elapsed)
        }
    }
}

fn effective_score(task: &TaskCandidate, cfg: RankingConfig, current_cycle: u64) -> i64 {
    let boost = (task.consecutive_misses as i64) * (cfg.miss_boost_per_cycle as i64);
    let mut score = task.priority as i64 + boost;

    let remaining =
        cooldown_remaining(current_cycle, task.last_selected_cycle, cfg.cooldown_cycles);
    if remaining > 0 {
        // Strong penalty while cooling down. Keep deterministic ordering by still allowing tie-breaks.
        score -= (remaining as i64) * 1_000_000;
    }

    score
}

pub fn rank_tasks(
    tasks: &[TaskCandidate],
    cfg: RankingConfig,
    current_cycle: u64,
) -> Vec<TaskCandidate> {
    let mut ranked = tasks.to_vec();
    ranked.sort_by(|a, b| compare_tasks(a, b, cfg, current_cycle));
    ranked
}

fn compare_tasks(
    a: &TaskCandidate,
    b: &TaskCandidate,
    cfg: RankingConfig,
    current_cycle: u64,
) -> Ordering {
    let sa = effective_score(a, cfg, current_cycle);
    let sb = effective_score(b, cfg, current_cycle);

    sb.cmp(&sa)
        .then_with(|| a.last_selected_cycle.cmp(&b.last_selected_cycle))
        .then_with(|| b.consecutive_misses.cmp(&a.consecutive_misses))
        .then_with(|| a.id.cmp(&b.id))
}

pub fn select_best_task(
    tasks: &[TaskCandidate],
    cfg: RankingConfig,
    current_cycle: u64,
) -> Option<TaskCandidate> {
    rank_tasks(tasks, cfg, current_cycle).into_iter().next()
}

pub fn apply_selection_feedback(
    tasks: &mut [TaskCandidate],
    selected_id: &str,
    current_cycle: u64,
) {
    for task in tasks.iter_mut() {
        if task.id == selected_id {
            task.last_selected_cycle = Some(current_cycle);
            task.consecutive_misses = 0;
        } else {
            task.consecutive_misses = task.consecutive_misses.saturating_add(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cooldown_penalizes_recently_selected_task() {
        let cfg = RankingConfig {
            cooldown_cycles: 3,
            miss_boost_per_cycle: 1,
        };

        let a = TaskCandidate {
            id: "a".to_string(),
            priority: 10,
            last_selected_cycle: Some(10),
            consecutive_misses: 0,
        };
        let b = TaskCandidate {
            id: "b".to_string(),
            priority: 5,
            last_selected_cycle: None,
            consecutive_misses: 0,
        };

        let ranked = rank_tasks(&[a, b], cfg, 11);
        assert_eq!(ranked[0].id, "b");
    }

    #[test]
    fn miss_boost_eventually_overcomes_base_priority_gap() {
        let cfg = RankingConfig {
            cooldown_cycles: 0,
            miss_boost_per_cycle: 3,
        };

        let high = TaskCandidate {
            id: "high".to_string(),
            priority: 10,
            last_selected_cycle: None,
            consecutive_misses: 0,
        };
        let starving = TaskCandidate {
            id: "starving".to_string(),
            priority: 1,
            last_selected_cycle: None,
            consecutive_misses: 4,
        };

        let best = select_best_task(&[high, starving], cfg, 100).expect("must pick one");
        assert_eq!(best.id, "starving");
    }

    #[test]
    fn selection_feedback_resets_winner_and_increments_others() {
        let mut tasks = vec![
            TaskCandidate {
                id: "t1".to_string(),
                priority: 1,
                last_selected_cycle: None,
                consecutive_misses: 2,
            },
            TaskCandidate {
                id: "t2".to_string(),
                priority: 1,
                last_selected_cycle: Some(3),
                consecutive_misses: 0,
            },
        ];

        apply_selection_feedback(&mut tasks, "t1", 8);

        let t1 = tasks.iter().find(|t| t.id == "t1").unwrap();
        let t2 = tasks.iter().find(|t| t.id == "t2").unwrap();

        assert_eq!(t1.last_selected_cycle, Some(8));
        assert_eq!(t1.consecutive_misses, 0);
        assert_eq!(t2.consecutive_misses, 1);
    }
}
