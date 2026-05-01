use crate::config::{OrchestratorConfig, ProviderConfig};
use crate::error_taxonomy::{ErrorClass, ErrorClassifier};
use crate::events::{kinds, EventSink, HarnessEvent};
use crate::provider::{Provider, ProviderRequest, ProviderResponse};
use crate::tools::{ToolPolicy, ToolRegistry};
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::time::{Duration, Instant};
use tokio::time::{sleep, timeout};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSpec {
    pub task_id: String,
    pub objective: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TaskSummary {
    pub task_id: String,
    pub steps: u32,
    pub tool_calls: u32,
    pub stopped_reason: String,
    pub runtime_ms: u64,
    pub transcript: Vec<String>,
}

pub struct Orchestrator<'a> {
    pub provider: &'a dyn Provider,
    pub provider_cfg: ProviderConfig,
    pub tools: &'a ToolRegistry,
    pub tool_policy: ToolPolicy,
    pub cfg: OrchestratorConfig,
    pub event_sink: &'a EventSink,
}

impl<'a> Orchestrator<'a> {
    pub async fn run_task(&self, task: TaskSpec) -> Result<TaskSummary> {
        let start = Instant::now();
        let mut steps = 0;
        let mut tool_calls = 0;
        let mut transcript = vec![];
        let mut context: Vec<String> = vec![];
        let budget = Duration::from_secs(self.cfg.max_runtime_seconds);

        self.event_sink.emit(
            &HarnessEvent::new(kinds::TASK_STARTED)
                .with_task_id(task.task_id.clone())
                .with_data(json!({
                    "objective": task.objective.clone(),
                    "provider": self.provider.name(),
                })),
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
                available_tools: self.tools.names(),
            };

            let res = self
                .generate_with_retries(&task.task_id, steps, &req)
                .await?;

            let provider_message = res.message.clone();
            let provider_done = res.done;
            let planned_tool_calls = res.tool_calls;

            transcript.push(format!("assistant: {provider_message}"));
            context.push(provider_message.clone());
            self.event_sink.emit(
                &HarnessEvent::new(kinds::PROVIDER_RESPONSE)
                    .with_task_id(task.task_id.clone())
                    .with_data(json!({
                        "message": provider_message,
                        "done": provider_done,
                        "tool_calls": planned_tool_calls.len()
                    })),
            )?;

            for call in planned_tool_calls {
                if tool_calls >= self.cfg.max_tool_calls {
                    self.event_sink.emit(
                        &HarnessEvent::new(kinds::TOOL_ERROR)
                            .with_task_id(task.task_id.clone())
                            .with_data(json!({
                                "tool": call.tool_name,
                                "error": "max_tool_calls reached",
                                "error_class": ErrorClass::UnexpectedEnvironment,
                            })),
                    )?;
                    break;
                }

                tool_calls += 1;
                let tool_name = call.tool_name;
                let tool_input = call.input_json;

                self.event_sink.emit(
                    &HarnessEvent::new(kinds::TOOL_CALL)
                        .with_task_id(task.task_id.clone())
                        .with_data(json!({"tool": tool_name.clone(), "input": tool_input.clone()})),
                )?;

                match self
                    .tools
                    .dispatch_with_policy(&tool_name, tool_input, &self.tool_policy)
                {
                    Ok(output) => {
                        let line = format!("tool:{} => {}", tool_name, output.content);
                        transcript.push(line.clone());
                        context.push(line);
                        self.event_sink.emit(
                            &HarnessEvent::new(kinds::TOOL_OUTPUT)
                                .with_task_id(task.task_id.clone())
                                .with_data(json!({"tool": tool_name, "output": output.content})),
                        )?;
                    }
                    Err(err) => {
                        let error = err.to_string();
                        let error_class = ErrorClassifier::classify_tool_error(Some(&error));
                        transcript.push(format!("tool:{tool_name} error: {error}"));
                        self.event_sink.emit(
                            &HarnessEvent::new(kinds::TOOL_ERROR)
                                .with_task_id(task.task_id.clone())
                                .with_data(json!({
                                    "tool": tool_name,
                                    "error": error,
                                    "error_class": error_class,
                                })),
                        )?;
                    }
                }
            }

            if provider_done {
                break "provider_done".to_string();
            }
            if tool_calls >= self.cfg.max_tool_calls {
                break "max_tool_calls".to_string();
            }
        };

        let runtime_ms = start.elapsed().as_millis() as u64;

        self.event_sink.emit(
            &HarnessEvent::new(kinds::TASK_FINISHED)
                .with_task_id(task.task_id.clone())
                .with_data(json!({
                    "steps": steps,
                    "tool_calls": tool_calls,
                    "reason": stopped_reason.clone(),
                    "runtime_ms": runtime_ms,
                })),
        )?;

        Ok(TaskSummary {
            task_id: task.task_id,
            steps,
            tool_calls,
            stopped_reason,
            runtime_ms,
            transcript,
        })
    }

    async fn generate_with_retries(
        &self,
        task_id: &str,
        step: u32,
        req: &ProviderRequest,
    ) -> Result<ProviderResponse> {
        let max_attempts = self.provider_cfg.max_retries + 1;

        for attempt in 1..=max_attempts {
            self.event_sink.emit(
                &HarnessEvent::new(kinds::PROVIDER_REQUEST)
                    .with_task_id(task_id.to_string())
                    .with_data(json!({
                        "step": step,
                        "attempt": attempt,
                        "objective": req.objective.clone(),
                    })),
            )?;

            let call = self.provider.generate(req);
            match timeout(Duration::from_millis(self.provider_cfg.timeout_ms), call).await {
                Ok(Ok(response)) => return Ok(response),
                Ok(Err(err)) => {
                    let error = err.to_string();
                    let error_class = ErrorClassifier::classify_provider_error(Some(&error));
                    self.event_sink.emit(
                        &HarnessEvent::new(kinds::PROVIDER_ERROR)
                            .with_task_id(task_id.to_string())
                            .with_data(json!({
                                "step": step,
                                "attempt": attempt,
                                "error": error,
                                "error_class": error_class,
                            })),
                    )?;
                }
                Err(_) => {
                    self.event_sink.emit(
                        &HarnessEvent::new(kinds::PROVIDER_TIMEOUT)
                            .with_task_id(task_id.to_string())
                            .with_data(json!({
                                "step": step,
                                "attempt": attempt,
                                "timeout_ms": self.provider_cfg.timeout_ms,
                                "error_class": ErrorClass::Timeout,
                            })),
                    )?;
                }
            }

            if attempt < max_attempts {
                let backoff_ms = self.provider_cfg.retry_backoff_ms * attempt as u64;
                self.event_sink.emit(
                    &HarnessEvent::new(kinds::PROVIDER_RETRY)
                        .with_task_id(task_id.to_string())
                        .with_data(json!({
                            "step": step,
                            "next_attempt": attempt + 1,
                            "backoff_ms": backoff_ms
                        })),
                )?;
                sleep(Duration::from_millis(backoff_ms)).await;
            }
        }

        Err(anyhow!(
            "provider failed after {} attempts",
            self.provider_cfg.max_retries + 1
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::{PlannedToolCall, Provider};

    struct DoneProvider;

    #[async_trait::async_trait]
    impl Provider for DoneProvider {
        fn name(&self) -> &'static str {
            "done"
        }

        async fn generate(&self, req: &ProviderRequest) -> Result<ProviderResponse> {
            Ok(ProviderResponse {
                message: format!("ok: {}", req.objective),
                tool_calls: vec![PlannedToolCall {
                    tool_name: "time.now".to_string(),
                    input_json: serde_json::json!({}),
                }],
                done: true,
                raw: None,
            })
        }
    }

    #[tokio::test]
    async fn run_task_stops_on_provider_done() {
        let provider = DoneProvider;
        let tools = ToolRegistry::with_defaults();
        let sink = EventSink::new("./tmp/test-events.jsonl").unwrap();
        let orchestrator = Orchestrator {
            provider: &provider,
            provider_cfg: ProviderConfig {
                kind: "done".to_string(),
                model: "test".to_string(),
                endpoint: None,
                api_key_env: None,
                timeout_ms: 1_000,
                max_retries: 0,
                retry_backoff_ms: 1,
            },
            tools: &tools,
            tool_policy: ToolPolicy::default(),
            cfg: OrchestratorConfig {
                max_steps: 4,
                max_tool_calls: 2,
                max_runtime_seconds: 5,
            },
            event_sink: &sink,
        };

        let summary = orchestrator
            .run_task(TaskSpec {
                task_id: "t1".to_string(),
                objective: "test".to_string(),
            })
            .await
            .unwrap();
        assert_eq!(summary.stopped_reason, "provider_done");
        assert_eq!(summary.steps, 1);
        assert_eq!(summary.tool_calls, 1);
    }
}
