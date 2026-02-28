use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub provider: ProviderConfig,
    pub orchestrator: OrchestratorConfig,
    pub event_log_path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderConfig {
    pub kind: String,
    pub model: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OrchestratorConfig {
    pub max_steps: u32,
    pub max_tool_calls: u32,
    pub max_runtime_seconds: u64,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            provider: ProviderConfig {
                kind: "echo".to_string(),
                model: "local-stub".to_string(),
            },
            orchestrator: OrchestratorConfig {
                max_steps: 8,
                max_tool_calls: 8,
                max_runtime_seconds: 60,
            },
            event_log_path: "./runs/events.jsonl".to_string(),
        }
    }
}

impl AppConfig {
    pub fn load(config_path: Option<&str>) -> Result<Self> {
        let mut cfg = if let Some(path) = config_path {
            let text = fs::read_to_string(Path::new(path))?;
            toml::from_str::<AppConfig>(&text)?
        } else {
            AppConfig::default()
        };

        if let Ok(kind) = std::env::var("HARNESS_PROVIDER") {
            cfg.provider.kind = kind;
        }
        if let Ok(model) = std::env::var("HARNESS_MODEL") {
            cfg.provider.model = model;
        }
        if let Ok(path) = std::env::var("HARNESS_EVENT_LOG") {
            cfg.event_log_path = path;
        }

        Ok(cfg)
    }
}
