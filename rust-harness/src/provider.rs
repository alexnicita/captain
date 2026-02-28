use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderRequest {
    pub objective: String,
    pub context: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderResponse {
    pub message: String,
    pub tool_call: Option<PlannedToolCall>,
    pub done: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlannedToolCall {
    pub tool_name: String,
    pub input_json: serde_json::Value,
}

pub trait Provider: Send + Sync {
    fn name(&self) -> &'static str;
    fn generate(&self, req: &ProviderRequest) -> Result<ProviderResponse>;
}

#[derive(Default)]
pub struct EchoProvider;

impl Provider for EchoProvider {
    fn name(&self) -> &'static str {
        "echo"
    }

    fn generate(&self, req: &ProviderRequest) -> Result<ProviderResponse> {
        // Simple deterministic behavior for harness bootstrapping.
        let tool_call = if req.objective.contains("time") {
            Some(PlannedToolCall {
                tool_name: "time.now".to_string(),
                input_json: serde_json::json!({}),
            })
        } else {
            None
        };

        Ok(ProviderResponse {
            message: format!("EchoProvider ({}) objective: {}", self.name(), req.objective),
            tool_call,
            done: req.objective.to_lowercase().contains("done"),
        })
    }
}

pub struct HttpProviderStub {
    pub endpoint: String,
    pub model: String,
}

impl Provider for HttpProviderStub {
    fn name(&self) -> &'static str {
        "http-stub"
    }

    fn generate(&self, req: &ProviderRequest) -> Result<ProviderResponse> {
        Ok(ProviderResponse {
            message: format!(
                "HTTP provider stub (endpoint={}, model={}) accepted objective={}",
                self.endpoint, self.model, req.objective
            ),
            tool_call: None,
            done: false,
        })
    }
}
