use crate::events::now_unix;
use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutput {
    pub ok: bool,
    pub content: Value,
}

type ToolHandler = Box<dyn Fn(Value) -> Result<ToolOutput> + Send + Sync>;

pub struct ToolRegistry {
    handlers: HashMap<String, ToolHandler>,
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    pub fn with_defaults() -> Self {
        let mut registry = Self::new();
        registry.register("echo", |input| {
            Ok(ToolOutput {
                ok: true,
                content: serde_json::json!({ "echo": input }),
            })
        });
        registry.register("time.now", |_input| {
            Ok(ToolOutput {
                ok: true,
                content: serde_json::json!({ "ts_unix": now_unix() }),
            })
        });
        registry
    }

    pub fn register<F>(&mut self, name: &str, handler: F)
    where
        F: Fn(Value) -> Result<ToolOutput> + Send + Sync + 'static,
    {
        self.handlers.insert(name.to_string(), Box::new(handler));
    }

    pub fn dispatch(&self, name: &str, input: Value) -> Result<ToolOutput> {
        let handler = self
            .handlers
            .get(name)
            .ok_or_else(|| anyhow!("unknown tool: {name}"))?;
        handler(input)
    }
}
