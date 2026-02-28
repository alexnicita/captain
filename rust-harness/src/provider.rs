use crate::config::ProviderConfig;
use anyhow::{anyhow, Context, Result};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::Duration;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderRequest {
    pub objective: String,
    pub context: Vec<String>,
    pub available_tools: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderResponse {
    pub message: String,
    pub tool_calls: Vec<PlannedToolCall>,
    pub done: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub raw: Option<Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedToolCall {
    pub tool_name: String,
    pub input_json: Value,
}

#[async_trait]
pub trait Provider: Send + Sync {
    fn name(&self) -> &'static str;
    async fn generate(&self, req: &ProviderRequest) -> Result<ProviderResponse>;
}

#[derive(Default)]
pub struct EchoProvider;

#[async_trait]
impl Provider for EchoProvider {
    fn name(&self) -> &'static str {
        "echo"
    }

    async fn generate(&self, req: &ProviderRequest) -> Result<ProviderResponse> {
        let objective_lower = req.objective.to_lowercase();
        let has_time_result = req
            .context
            .iter()
            .any(|line| line.contains("tool:time.now"));

        let mut tool_calls = Vec::new();
        if objective_lower.contains("time") && !has_time_result {
            tool_calls.push(PlannedToolCall {
                tool_name: "time.now".to_string(),
                input_json: serde_json::json!({}),
            });
        }

        Ok(ProviderResponse {
            message: format!(
                "EchoProvider objective='{}' context_items={} tools={}.",
                req.objective,
                req.context.len(),
                req.available_tools.join(",")
            ),
            tool_calls,
            done: has_time_result || !objective_lower.contains("time"),
            raw: None,
        })
    }
}

pub struct HttpProvider {
    client: reqwest::Client,
    endpoint: String,
    model: String,
    api_key: Option<String>,
}

impl HttpProvider {
    pub fn new(cfg: &ProviderConfig) -> Result<Self> {
        let endpoint = cfg
            .endpoint
            .clone()
            .unwrap_or_else(|| "http://localhost:11434/v1/chat/completions".to_string());

        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(cfg.timeout_ms))
            .build()
            .context("failed to construct reqwest client")?;

        let api_key = cfg
            .api_key_env
            .as_ref()
            .and_then(|key_name| std::env::var(key_name).ok());

        Ok(Self {
            client,
            endpoint,
            model: cfg.model.clone(),
            api_key,
        })
    }

    fn build_messages(&self, req: &ProviderRequest) -> Vec<Value> {
        let mut user_content = format!("Objective: {}", req.objective);
        if !req.context.is_empty() {
            user_content.push_str("\n\nContext:\n");
            for line in &req.context {
                user_content.push_str("- ");
                user_content.push_str(line);
                user_content.push('\n');
            }
        }
        if !req.available_tools.is_empty() {
            user_content.push_str("\nAvailable tools: ");
            user_content.push_str(&req.available_tools.join(", "));
        }

        vec![
            serde_json::json!({
                "role": "system",
                "content": "You are a general-purpose task orchestrator. Return concise progress and optional tool usage."
            }),
            serde_json::json!({
                "role": "user",
                "content": user_content
            }),
        ]
    }
}

#[async_trait]
impl Provider for HttpProvider {
    fn name(&self) -> &'static str {
        "http"
    }

    async fn generate(&self, req: &ProviderRequest) -> Result<ProviderResponse> {
        let payload = serde_json::json!({
            "model": self.model.clone(),
            "messages": self.build_messages(req),
            "temperature": 0.2,
        });

        let mut request = self.client.post(&self.endpoint).json(&payload);
        if let Some(key) = &self.api_key {
            request = request.bearer_auth(key);
        }

        let response = request
            .send()
            .await
            .with_context(|| format!("provider request failed endpoint={}", self.endpoint))?;

        let status = response.status();
        let body: Value = response
            .json()
            .await
            .with_context(|| format!("failed to decode provider response status={status}"))?;

        if !status.is_success() {
            return Err(anyhow!(
                "provider returned non-success status={status} body={} ",
                body
            ));
        }

        let message = body
            .pointer("/choices/0/message/content")
            .and_then(Value::as_str)
            .unwrap_or("(empty provider content)")
            .to_string();

        let mut tool_calls = Vec::new();
        if let Some(calls) = body
            .pointer("/choices/0/message/tool_calls")
            .and_then(Value::as_array)
        {
            for call in calls {
                if let Some(name) = call.pointer("/function/name").and_then(Value::as_str) {
                    let args = call
                        .pointer("/function/arguments")
                        .and_then(Value::as_str)
                        .unwrap_or("{}");
                    let input_json: Value =
                        serde_json::from_str(args).unwrap_or_else(|_| serde_json::json!({}));
                    tool_calls.push(PlannedToolCall {
                        tool_name: name.to_string(),
                        input_json,
                    });
                }
            }
        }

        Ok(ProviderResponse {
            message,
            done: tool_calls.is_empty(),
            tool_calls,
            raw: Some(body),
        })
    }
}

pub struct HttpProviderStub {
    pub endpoint: String,
    pub model: String,
}

#[async_trait]
impl Provider for HttpProviderStub {
    fn name(&self) -> &'static str {
        "http-stub"
    }

    async fn generate(&self, req: &ProviderRequest) -> Result<ProviderResponse> {
        Ok(ProviderResponse {
            message: format!(
                "HTTP stub accepted objective='{}' endpoint='{}' model='{}'",
                req.objective, self.endpoint, self.model
            ),
            tool_calls: Vec::new(),
            done: true,
            raw: None,
        })
    }
}

pub fn build_provider(cfg: &ProviderConfig) -> Box<dyn Provider> {
    match cfg.kind.as_str() {
        "http" | "openai-compatible" => match HttpProvider::new(cfg) {
            Ok(provider) => Box::new(provider),
            Err(_) => Box::new(HttpProviderStub {
                endpoint: cfg
                    .endpoint
                    .clone()
                    .unwrap_or_else(|| "http://localhost:11434/v1/chat/completions".to_string()),
                model: cfg.model.clone(),
            }),
        },
        "http-stub" => Box::new(HttpProviderStub {
            endpoint: cfg
                .endpoint
                .clone()
                .unwrap_or_else(|| "http://localhost:11434/v1/chat/completions".to_string()),
            model: cfg.model.clone(),
        }),
        _ => Box::new(EchoProvider),
    }
}
