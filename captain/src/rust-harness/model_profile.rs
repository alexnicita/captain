use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderApi {
    ChatCompletions,
    Responses,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EditFormat {
    UnifiedDiff,
    JsonEdits,
    StringReplacement,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PromptStyle {
    Precise,
    Conversational,
    Generic,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelProfile {
    pub model: String,
    pub family: String,
    pub provider_api: ProviderApi,
    pub edit_format: EditFormat,
    pub prompt_style: PromptStyle,
    pub supports_function_calls: bool,
    pub context_anxiety_guard: bool,
}

impl ModelProfile {
    pub fn for_model(model: &str) -> Self {
        let normalized = model.trim();
        let lower = normalized.to_ascii_lowercase();

        if lower.contains("codex") {
            return Self {
                model: normalized.to_string(),
                family: "openai-codex".to_string(),
                provider_api: ProviderApi::Responses,
                edit_format: EditFormat::UnifiedDiff,
                prompt_style: PromptStyle::Precise,
                supports_function_calls: true,
                context_anxiety_guard: false,
            };
        }

        if lower.contains("claude") || lower.contains("anthropic") {
            return Self {
                model: normalized.to_string(),
                family: "anthropic-claude".to_string(),
                provider_api: ProviderApi::ChatCompletions,
                edit_format: EditFormat::StringReplacement,
                prompt_style: PromptStyle::Conversational,
                supports_function_calls: true,
                context_anxiety_guard: true,
            };
        }

        if lower.starts_with("gpt-") || lower.contains("openai") {
            return Self {
                model: normalized.to_string(),
                family: "openai-compatible".to_string(),
                provider_api: ProviderApi::ChatCompletions,
                edit_format: EditFormat::UnifiedDiff,
                prompt_style: PromptStyle::Precise,
                supports_function_calls: true,
                context_anxiety_guard: false,
            };
        }

        Self {
            model: normalized.to_string(),
            family: "unknown".to_string(),
            provider_api: ProviderApi::ChatCompletions,
            edit_format: EditFormat::Unknown,
            prompt_style: PromptStyle::Generic,
            supports_function_calls: false,
            context_anxiety_guard: false,
        }
    }

    pub fn system_instruction(&self) -> &'static str {
        match self.prompt_style {
            PromptStyle::Precise => {
                "You are a precise task orchestrator. Return concise progress and use tools only when they materially advance the task."
            }
            PromptStyle::Conversational => {
                "You are a practical task orchestrator. Keep responses concise, concrete, and grounded in available tools."
            }
            PromptStyle::Generic => {
                "You are a general-purpose task orchestrator. Return concise progress and optional tool usage."
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn codex_models_use_responses_and_patch_profile() {
        let profile = ModelProfile::for_model("gpt-5.3-codex");
        assert_eq!(profile.family, "openai-codex");
        assert_eq!(profile.provider_api, ProviderApi::Responses);
        assert_eq!(profile.edit_format, EditFormat::UnifiedDiff);
        assert_eq!(profile.prompt_style, PromptStyle::Precise);
    }

    #[test]
    fn generic_openai_models_use_chat_compatible_profile() {
        let profile = ModelProfile::for_model("gpt-4o");
        assert_eq!(profile.family, "openai-compatible");
        assert_eq!(profile.provider_api, ProviderApi::ChatCompletions);
        assert_eq!(profile.edit_format, EditFormat::UnifiedDiff);
    }

    #[test]
    fn unknown_models_keep_safe_generic_defaults() {
        let profile = ModelProfile::for_model("local-test-model");
        assert_eq!(profile.family, "unknown");
        assert_eq!(profile.provider_api, ProviderApi::ChatCompletions);
        assert_eq!(profile.edit_format, EditFormat::Unknown);
        assert_eq!(profile.prompt_style, PromptStyle::Generic);
    }
}
