use crate::events::now_unix;
use anyhow::{anyhow, Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub input_schema: Value,
    pub output_schema: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolOutput {
    pub ok: bool,
    pub content: Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ToolPolicyMode {
    AllowAll,
    AllowList,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolPolicy {
    pub mode: ToolPolicyMode,
    pub allowed_tools: HashSet<String>,
    pub denied_tools: HashSet<String>,
}

impl Default for ToolPolicy {
    fn default() -> Self {
        Self {
            mode: ToolPolicyMode::AllowAll,
            allowed_tools: HashSet::new(),
            denied_tools: HashSet::new(),
        }
    }
}

impl ToolPolicy {
    pub fn allows(&self, tool_name: &str) -> bool {
        if self.denied_tools.contains(tool_name) {
            return false;
        }
        match self.mode {
            ToolPolicyMode::AllowAll => true,
            ToolPolicyMode::AllowList => self.allowed_tools.contains(tool_name),
        }
    }

    pub fn allow_only(names: impl IntoIterator<Item = String>) -> Self {
        Self {
            mode: ToolPolicyMode::AllowList,
            allowed_tools: names.into_iter().collect(),
            denied_tools: HashSet::new(),
        }
    }
}

type ToolHandler = Box<dyn Fn(Value) -> Result<ToolOutput> + Send + Sync>;

struct RegisteredTool {
    spec: ToolSpec,
    handler: ToolHandler,
}

pub struct ToolRegistry {
    handlers: HashMap<String, RegisteredTool>,
}

impl Default for ToolRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ToolRegistry {
    pub fn new() -> Self {
        Self {
            handlers: HashMap::new(),
        }
    }

    pub fn with_defaults() -> Self {
        let mut registry = Self::new();
        registry.register(
            ToolSpec {
                name: "echo".to_string(),
                description: "Echo structured input for dry runs and debugging".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "message": { "type": "string" },
                        "payload": {}
                    },
                    "additionalProperties": true
                }),
                output_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "echo": {}
                    },
                    "required": ["echo"]
                }),
            },
            |input| {
                let parsed: EchoInput = serde_json::from_value(input.clone())
                    .context("echo tool expects object with optional message/payload")?;
                Ok(ToolOutput {
                    ok: true,
                    content: serde_json::json!({
                        "echo": {
                            "message": parsed.message,
                            "payload": parsed.payload,
                            "raw": input
                        }
                    }),
                })
            },
        );

        registry.register(
            ToolSpec {
                name: "time.now".to_string(),
                description: "Return current unix timestamp".to_string(),
                input_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "timezone": { "type": "string" }
                    },
                    "additionalProperties": true
                }),
                output_schema: serde_json::json!({
                    "type": "object",
                    "properties": {
                        "ts_unix": { "type": "integer" },
                        "timezone": { "type": "string" }
                    },
                    "required": ["ts_unix"]
                }),
            },
            |input| {
                let parsed: TimeNowInput = serde_json::from_value(input)
                    .context("time.now expects object with optional timezone")?;
                Ok(ToolOutput {
                    ok: true,
                    content: serde_json::json!({
                        "ts_unix": now_unix(),
                        "timezone": parsed.timezone.unwrap_or_else(|| "UTC".to_string())
                    }),
                })
            },
        );

        registry
    }

    pub fn register<F>(&mut self, spec: ToolSpec, handler: F)
    where
        F: Fn(Value) -> Result<ToolOutput> + Send + Sync + 'static,
    {
        self.handlers.insert(
            spec.name.clone(),
            RegisteredTool {
                spec,
                handler: Box::new(handler),
            },
        );
    }

    pub fn dispatch_with_policy(
        &self,
        name: &str,
        input: Value,
        policy: &ToolPolicy,
    ) -> Result<ToolOutput> {
        if !policy.allows(name) {
            return Err(anyhow!("tool blocked by policy: {name}"));
        }
        self.dispatch(name, input)
    }

    pub fn dispatch(&self, name: &str, input: Value) -> Result<ToolOutput> {
        let registered = self
            .handlers
            .get(name)
            .ok_or_else(|| anyhow!("unknown tool: {name}"))?;
        (registered.handler)(input)
    }

    pub fn specs(&self) -> Vec<ToolSpec> {
        let mut specs = self
            .handlers
            .values()
            .map(|registered| registered.spec.clone())
            .collect::<Vec<_>>();
        specs.sort_by(|a, b| a.name.cmp(&b.name));
        specs
    }

    pub fn names(&self) -> Vec<String> {
        self.specs().into_iter().map(|spec| spec.name).collect()
    }
}

#[derive(Debug, Deserialize)]
struct EchoInput {
    #[serde(default)]
    message: Option<String>,
    #[serde(default)]
    payload: Option<Value>,
}

#[derive(Debug, Deserialize)]
struct TimeNowInput {
    #[serde(default)]
    timezone: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dispatches_default_tool() {
        let reg = ToolRegistry::with_defaults();
        let out = reg
            .dispatch("echo", serde_json::json!({"message": "hi"}))
            .unwrap();
        assert!(out.ok);
        assert_eq!(out.content["echo"]["message"], "hi");
    }

    #[test]
    fn policy_blocks_denied_tool() {
        let reg = ToolRegistry::with_defaults();
        let mut policy = ToolPolicy::default();
        policy.denied_tools.insert("time.now".to_string());
        let err = reg
            .dispatch_with_policy("time.now", serde_json::json!({}), &policy)
            .unwrap_err();
        assert!(err.to_string().contains("blocked by policy"));
    }
}
