use crate::config::SchedulerConfig;
use crate::events::{kinds, EventSink, HarnessEvent};
use crate::orchestrator::{Orchestrator, TaskSpec, TaskSummary};
use anyhow::Result;
use futures::stream::{FuturesUnordered, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::VecDeque;
use tokio::time::{sleep, timeout, Duration};

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
    pub cfg: SchedulerConfig,
}

impl<'a> Scheduler<'a> {
    pub async fn run_queue(&self, mut queue: TaskQueue) -> Result<BatchSummary> {
        let total = queue.len();
        let mut completed = 0usize;
        let mut failed = 0usize;
        let mut task_summaries = Vec::new();
        let max_concurrent = self.cfg.max_concurrent_tasks.max(1);
        let poll_ms = self.cfg.queue_poll_ms.max(1);

        let mut in_flight = FuturesUnordered::new();

        while !queue.is_empty() || !in_flight.is_empty() {
            while in_flight.len() < max_concurrent {
                let Some(item) = queue.dequeue() else {
                    break;
                };
                let task_id = item.task_id.clone();

                self.event_sink.emit(
                    &HarnessEvent::new(kinds::SCHEDULER_DISPATCH)
                        .with_task_id(task_id.clone())
                        .with_data(json!({
                            "priority": item.priority,
                            "queue_depth": queue.len(),
                            "in_flight": in_flight.len() + 1,
                        })),
                )?;

                let task_spec = TaskSpec {
                    task_id: item.task_id,
                    objective: item.objective,
                };
                let fut = async move { (task_id, self.orchestrator.run_task(task_spec).await) };
                in_flight.push(fut);
            }

            if in_flight.is_empty() {
                if !queue.is_empty() {
                    sleep(Duration::from_millis(poll_ms)).await;
                }
                continue;
            }

            let next_result = timeout(Duration::from_millis(poll_ms), in_flight.next()).await;
            let Ok(Some((task_id, result))) = next_result else {
                self.event_sink
                    .emit(&HarnessEvent::new(kinds::SCHEDULER_TICK).with_data(json!({
                        "queue_depth": queue.len(),
                        "in_flight": in_flight.len(),
                        "poll_ms": poll_ms,
                    })))?;
                continue;
            };

            match result {
                Ok(summary) => {
                    completed += 1;
                    self.event_sink.emit(
                        &HarnessEvent::new(kinds::SCHEDULER_RESULT)
                            .with_task_id(task_id)
                            .with_data(json!({
                                "status": "ok",
                                "steps": summary.steps,
                                "tool_calls": summary.tool_calls,
                                "reason": summary.stopped_reason,
                            })),
                    )?;
                    task_summaries.push(summary);
                }
                Err(err) => {
                    failed += 1;
                    self.event_sink.emit(
                        &HarnessEvent::new(kinds::SCHEDULER_RESULT)
                            .with_task_id(task_id.clone())
                            .with_data(json!({
                                "status": "error",
                                "error": err.to_string(),
                            })),
                    )?;
                    self.event_sink.emit(
                        &HarnessEvent::new(kinds::TASK_FINISHED)
                            .with_task_id(task_id)
                            .with_data(json!({
                                "reason": "scheduler_task_failed",
                                "error": err.to_string(),
                            })),
                    )?;
                }
            }
        }

        task_summaries.sort_by(|a, b| a.task_id.cmp(&b.task_id));

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
