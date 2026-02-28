use crate::events::{kinds, EventSink, HarnessEvent};
use crate::orchestrator::{Orchestrator, TaskSpec, TaskSummary};
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::VecDeque;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QueuedTask {
    pub task_id: String,
    pub objective: String,
    pub priority: u8,
}

#[derive(Debug, Default)]
pub struct TaskQueue {
    high: VecDeque<QueuedTask>,
    normal: VecDeque<QueuedTask>,
}

impl TaskQueue {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn enqueue(&mut self, task: QueuedTask) {
        if task.priority > 0 {
            self.high.push_back(task);
        } else {
            self.normal.push_back(task);
        }
    }

    pub fn dequeue(&mut self) -> Option<QueuedTask> {
        self.high.pop_front().or_else(|| self.normal.pop_front())
    }

    pub fn len(&self) -> usize {
        self.high.len() + self.normal.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BatchSummary {
    pub total: usize,
    pub completed: usize,
    pub failed: usize,
    pub task_summaries: Vec<TaskSummary>,
}

pub struct Scheduler<'a> {
    pub orchestrator: &'a Orchestrator<'a>,
    pub event_sink: &'a EventSink,
}

impl<'a> Scheduler<'a> {
    pub async fn run_queue(&self, mut queue: TaskQueue) -> Result<BatchSummary> {
        let total = queue.len();
        let mut completed = 0usize;
        let mut failed = 0usize;
        let mut task_summaries = Vec::new();

        while let Some(item) = queue.dequeue() {
            let task_spec = TaskSpec {
                task_id: item.task_id.clone(),
                objective: item.objective.clone(),
            };
            match self.orchestrator.run_task(task_spec).await {
                Ok(summary) => {
                    completed += 1;
                    task_summaries.push(summary);
                }
                Err(err) => {
                    failed += 1;
                    self.event_sink.emit(
                        &HarnessEvent::new(kinds::TASK_FINISHED)
                            .with_task_id(item.task_id)
                            .with_data(json!({
                                "reason": "scheduler_task_failed",
                                "error": err.to_string(),
                            })),
                    )?;
                }
            }
        }

        Ok(BatchSummary {
            total,
            completed,
            failed,
            task_summaries,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn queue_respects_priority() {
        let mut q = TaskQueue::new();
        q.enqueue(QueuedTask {
            task_id: "normal".to_string(),
            objective: "n".to_string(),
            priority: 0,
        });
        q.enqueue(QueuedTask {
            task_id: "high".to_string(),
            objective: "h".to_string(),
            priority: 1,
        });

        assert_eq!(q.dequeue().unwrap().task_id, "high");
        assert_eq!(q.dequeue().unwrap().task_id, "normal");
        assert!(q.is_empty());
    }
}
