use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct AppConfig {
    pub provider: ProviderConfig,
    pub orchestrator: OrchestratorConfig,
    pub scheduler: SchedulerConfig,
    pub event_log_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ProviderConfig {
    pub kind: String,
    pub model: String,
    pub endpoint: Option<String>,
    pub api_key_env: Option<String>,
    pub timeout_ms: u64,
    pub max_retries: u32,
    pub retry_backoff_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct OrchestratorConfig {
    pub max_steps: u32,
    pub max_tool_calls: u32,
    pub max_runtime_seconds: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SchedulerConfig {
    pub max_concurrent_tasks: usize,
    pub queue_poll_ms: u64,
}

impl Default for ProviderConfig {
    fn default() -> Self {
        Self {
            kind: "echo".to_string(),
            model: "local-stub".to_string(),
            endpoint: Some("http://localhost:11434/v1/chat/completions".to_string()),
            api_key_env: Some("OPENAI_API_KEY".to_string()),
            timeout_ms: 20_000,
            max_retries: 2,
            retry_backoff_ms: 350,
        }
    }
}

impl Default for OrchestratorConfig {
    fn default() -> Self {
        Self {
            max_steps: 8,
            max_tool_calls: 8,
            max_runtime_seconds: 60,
        }
    }
}

impl Default for SchedulerConfig {
    fn default() -> Self {
        Self {
            max_concurrent_tasks: 2,
            queue_poll_ms: 50,
        }
    }
}

impl AppConfig {
    pub fn load(config_path: Option<&str>) -> Result<Self> {
        let mut cfg = if let Some(path) = config_path {
            let text = fs::read_to_string(Path::new(path))?;
            toml::from_str::<AppConfig>(&text)?
        } else {
            AppConfig {
                provider: ProviderConfig::default(),
                orchestrator: OrchestratorConfig::default(),
                scheduler: SchedulerConfig::default(),
                event_log_path: "./runs/events.jsonl".to_string(),
            }
        };

        if cfg.event_log_path.is_empty() {
            cfg.event_log_path = "./runs/events.jsonl".to_string();
        }

        if let Ok(kind) = std::env::var("HARNESS_PROVIDER") {
            cfg.provider.kind = kind;
        }
        if let Ok(model) = std::env::var("HARNESS_MODEL") {
            cfg.provider.model = model;
        }
        if let Ok(path) = std::env::var("HARNESS_EVENT_LOG") {
            cfg.event_log_path = path;
        }
        if let Ok(endpoint) = std::env::var("HARNESS_PROVIDER_ENDPOINT") {
            cfg.provider.endpoint = Some(endpoint);
        }
        if let Ok(timeout_ms) = std::env::var("HARNESS_PROVIDER_TIMEOUT_MS") {
            if let Ok(parsed) = timeout_ms.parse::<u64>() {
                cfg.provider.timeout_ms = parsed;
            }
        }
        if let Ok(max_retries) = std::env::var("HARNESS_PROVIDER_MAX_RETRIES") {
            if let Ok(parsed) = max_retries.parse::<u32>() {
                cfg.provider.max_retries = parsed;
            }
        }
        if let Ok(backoff_ms) = std::env::var("HARNESS_PROVIDER_RETRY_BACKOFF_MS") {
            if let Ok(parsed) = backoff_ms.parse::<u64>() {
                cfg.provider.retry_backoff_ms = parsed;
            }
        }

        Ok(cfg)
    }
}
