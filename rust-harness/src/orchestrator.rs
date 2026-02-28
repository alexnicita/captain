use crate::config::OrchestratorConfig;
use crate::events::{EventSink, HarnessEvent};
use crate::provider::{Provider, ProviderRequest, ProviderResponse};
use crate::tools::ToolRegistry;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSpec {
    pub id: String,
    pub objective: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSummary {
    pub task_id: String,
    pub steps: u32,
    pub tool_calls: u32,
    pub stopped_reason: String,
    pub transcript: Vec<String>,
}

pub struct Orchestrator<'a> {
    pub provider: &'a dyn Provider,
    pub tools: &'a ToolRegistry,
    pub cfg: OrchestratorConfig,
    pub event_sink: &'a EventSink,
}

impl<'a> Orchestrator<'a> {
    pub fn run_task(&self, task: TaskSpec) -> Result<TaskSummary> {
        let start = Instant::now();
        let mut steps = 0;
        let mut tool_calls = 0;
        let mut transcript = vec![];
        let mut context: Vec<String> = vec![];
        let budget = Duration::from_secs(self.cfg.max_runtime_seconds);

        self.event_sink.emit(
            &HarnessEvent::new("task.started")
                .with_task_id(task.id.clone())
                .with_data(json!({"objective": task.objective})),
        )?;

        let stopped_reason = loop {
            if start.elapsed() > budget {
                break "max_runtime_seconds".to_string();
            }
            if steps >= self.cfg.max_steps {
                break "max_steps".to_string();
            }

            steps += 1;
            let req = ProviderRequest {
                objective: task.objective.clone(),
                context: context.clone(),
            };
            let res = self.provider.generate(&req)?;
            transcript.push(format!("assistant: {}", res.message));
            context.push(res.message.clone());
            self.event_sink.emit(
                &HarnessEvent::new("provider.response")
                    .with_task_id(task.id.clone())
                    .with_data(json!({"message": res.message, "done": res.done})),
            )?;

            if let Some(call) = res.tool_call {
                if tool_calls >= self.cfg.max_tool_calls {
                    break "max_tool_calls".to_string();
                }
                tool_calls += 1;
                let output = self.tools.dispatch(&call.tool_name, call.input_json)?;
                let line = format!("tool:{} => {}", call.tool_name, output.content);
                transcript.push(line.clone());
                context.push(line.clone());
                self.event_sink.emit(
                    &HarnessEvent::new("tool.output")
                        .with_task_id(task.id.clone())
                        .with_data(json!({"tool": call.tool_name, "output": output.content})),
                )?;
            }

            if res.done {
                break "provider_done".to_string();
            }
        };

        self.event_sink.emit(
            &HarnessEvent::new("task.finished")
                .with_task_id(task.id.clone())
                .with_data(json!({
                    "steps": steps,
                    "tool_calls": tool_calls,
                    "reason": stopped_reason,
                })),
        )?;

        Ok(TaskSummary {
            task_id: task.id,
            steps,
            tool_calls,
            stopped_reason,
            transcript,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct DoneProvider;

    impl Provider for DoneProvider {
        fn name(&self) -> &'static str {
            "done"
        }

        fn generate(&self, req: &ProviderRequest) -> Result<ProviderResponse> {
            Ok(ProviderResponse {
                message: format!("ok: {}", req.objective),
                tool_call: None,
                done: true,
            })
        }
    }

    #[test]
    fn run_task_stops_on_provider_done() {
        let provider = DoneProvider;
        let tools = ToolRegistry::with_defaults();
        let sink = EventSink::new("./tmp/test-events.jsonl").unwrap();
        let orchestrator = Orchestrator {
            provider: &provider,
            tools: &tools,
            cfg: OrchestratorConfig {
                max_steps: 4,
                max_tool_calls: 2,
                max_runtime_seconds: 5,
            },
            event_sink: &sink,
        };

        let summary = orchestrator
            .run_task(TaskSpec {
                id: "t1".to_string(),
                objective: "test".to_string(),
            })
            .unwrap();
        assert_eq!(summary.stopped_reason, "provider_done");
        assert_eq!(summary.steps, 1);
    }
}
