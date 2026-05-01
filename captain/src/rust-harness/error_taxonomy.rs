use crate::events::{kinds, HarnessEvent};
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorClass {
    InvalidArguments,
    UnexpectedEnvironment,
    ProviderError,
    Timeout,
    PolicyBlocked,
    UserAborted,
    Unknown,
}

impl ErrorClass {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorClass::InvalidArguments => "invalid_arguments",
            ErrorClass::UnexpectedEnvironment => "unexpected_environment",
            ErrorClass::ProviderError => "provider_error",
            ErrorClass::Timeout => "timeout",
            ErrorClass::PolicyBlocked => "policy_blocked",
            ErrorClass::UserAborted => "user_aborted",
            ErrorClass::Unknown => "unknown",
        }
    }

    pub fn from_value(value: &Value) -> Option<Self> {
        let raw = value.as_str()?.trim();
        match raw {
            "invalid_arguments" | "InvalidArguments" => Some(Self::InvalidArguments),
            "unexpected_environment" | "UnexpectedEnvironment" => Some(Self::UnexpectedEnvironment),
            "provider_error" | "ProviderError" => Some(Self::ProviderError),
            "timeout" | "Timeout" => Some(Self::Timeout),
            "policy_blocked" | "PolicyBlocked" => Some(Self::PolicyBlocked),
            "user_aborted" | "UserAborted" => Some(Self::UserAborted),
            "unknown" | "Unknown" => Some(Self::Unknown),
            _ => None,
        }
    }
}

impl std::fmt::Display for ErrorClass {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorSurface {
    Provider,
    Tool,
    Coding,
    Git,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClassifiedError {
    pub surface: ErrorSurface,
    pub class: ErrorClass,
    pub event_kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

pub struct ErrorClassifier;

impl ErrorClassifier {
    pub fn classify_event(event: &HarnessEvent) -> Option<ClassifiedError> {
        match event.kind.as_str() {
            kinds::PROVIDER_TIMEOUT => Some(ClassifiedError {
                surface: ErrorSurface::Provider,
                class: ErrorClass::Timeout,
                event_kind: event.kind.clone(),
                tool: None,
                result: None,
                message: None,
            }),
            kinds::PROVIDER_ERROR => {
                let message = string_field(&event.data, "error");
                Some(ClassifiedError {
                    surface: ErrorSurface::Provider,
                    class: event
                        .data
                        .get("error_class")
                        .and_then(ErrorClass::from_value)
                        .unwrap_or_else(|| Self::classify_provider_error(message.as_deref())),
                    event_kind: event.kind.clone(),
                    tool: None,
                    result: None,
                    message,
                })
            }
            kinds::TOOL_ERROR => {
                let message = string_field(&event.data, "error");
                Some(ClassifiedError {
                    surface: ErrorSurface::Tool,
                    class: event
                        .data
                        .get("error_class")
                        .and_then(ErrorClass::from_value)
                        .unwrap_or_else(|| Self::classify_tool_error(message.as_deref())),
                    event_kind: event.kind.clone(),
                    tool: string_field(&event.data, "tool"),
                    result: None,
                    message,
                })
            }
            kinds::GIT_COMMIT | kinds::GIT_PUSH => {
                let success = bool_field(&event.data, "success").unwrap_or(false);
                let skipped = bool_field(&event.data, "skipped").unwrap_or(false);
                let result = string_field(&event.data, "result");
                let detail = string_field(&event.data, "detail");
                let class = event
                    .data
                    .get("error_class")
                    .and_then(ErrorClass::from_value)
                    .or_else(|| {
                        Self::class_for_git_event(
                            success,
                            skipped,
                            result.as_deref().unwrap_or(""),
                            detail.as_deref().unwrap_or(""),
                        )
                    })?;

                Some(ClassifiedError {
                    surface: ErrorSurface::Git,
                    class,
                    event_kind: event.kind.clone(),
                    tool: None,
                    result,
                    message: detail,
                })
            }
            _ => None,
        }
    }

    pub fn classify_provider_error(message: Option<&str>) -> ErrorClass {
        let Some(message) = message else {
            return ErrorClass::ProviderError;
        };
        let lower = message.to_ascii_lowercase();
        if lower.contains("timeout") || lower.contains("timed out") {
            ErrorClass::Timeout
        } else if contains_abort_signal(&lower) {
            ErrorClass::UserAborted
        } else {
            ErrorClass::ProviderError
        }
    }

    pub fn classify_tool_error(message: Option<&str>) -> ErrorClass {
        let Some(message) = message else {
            return ErrorClass::Unknown;
        };
        let lower = message.to_ascii_lowercase();
        if lower.contains("blocked by policy") || lower.contains("not allowlisted") {
            ErrorClass::PolicyBlocked
        } else if lower.contains("expects object")
            || lower.contains("invalid")
            || lower.contains("missing field")
            || lower.contains("unknown tool")
            || lower.contains("deserialize")
            || lower.contains("parse")
        {
            ErrorClass::InvalidArguments
        } else if lower.contains("max_tool_calls")
            || lower.contains("not found")
            || lower.contains("no such file")
            || lower.contains("permission denied")
        {
            ErrorClass::UnexpectedEnvironment
        } else if lower.contains("timeout") || lower.contains("timed out") {
            ErrorClass::Timeout
        } else if contains_abort_signal(&lower) {
            ErrorClass::UserAborted
        } else {
            ErrorClass::Unknown
        }
    }

    pub fn class_for_git_event(
        success: bool,
        skipped: bool,
        result: &str,
        detail: &str,
    ) -> Option<ErrorClass> {
        if success || (skipped && result == "skipped") {
            return None;
        }

        let result_lower = result.to_ascii_lowercase();
        let detail_lower = detail.to_ascii_lowercase();
        if result_lower == "rejected"
            || detail_lower.contains("quality gate")
            || detail_lower.contains("diff rubric")
            || detail_lower.contains("subject rejected")
        {
            Some(ErrorClass::InvalidArguments)
        } else if result_lower == "blocked" {
            Some(ErrorClass::UnexpectedEnvironment)
        } else if detail_lower.contains("timeout") || detail_lower.contains("timed out") {
            Some(ErrorClass::Timeout)
        } else if contains_abort_signal(&detail_lower) {
            Some(ErrorClass::UserAborted)
        } else if detail_lower.contains("permission denied")
            || detail_lower.contains("authentication")
            || detail_lower.contains("auth")
            || detail_lower.contains("conflict")
            || detail_lower.contains("not a git repository")
            || detail_lower.contains("nothing to commit")
        {
            Some(ErrorClass::UnexpectedEnvironment)
        } else {
            Some(ErrorClass::Unknown)
        }
    }
}

fn string_field(data: &Value, key: &str) -> Option<String> {
    data.get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
}

fn bool_field(data: &Value, key: &str) -> Option<bool> {
    data.get(key).and_then(Value::as_bool)
}

fn contains_abort_signal(lower: &str) -> bool {
    lower.contains("aborted") || lower.contains("cancelled") || lower.contains("interrupted")
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn classifies_provider_timeout() {
        let event = HarnessEvent::new(kinds::PROVIDER_TIMEOUT);
        let classified = ErrorClassifier::classify_event(&event).unwrap();
        assert_eq!(classified.surface, ErrorSurface::Provider);
        assert_eq!(classified.class, ErrorClass::Timeout);
    }

    #[test]
    fn classifies_policy_blocked_tool_error() {
        let event = HarnessEvent::new(kinds::TOOL_ERROR).with_data(json!({
            "tool": "echo",
            "error": "tool blocked by policy: echo"
        }));
        let classified = ErrorClassifier::classify_event(&event).unwrap();
        assert_eq!(classified.surface, ErrorSurface::Tool);
        assert_eq!(classified.class, ErrorClass::PolicyBlocked);
        assert_eq!(classified.tool.as_deref(), Some("echo"));
    }

    #[test]
    fn classifies_invalid_argument_tool_error() {
        let event = HarnessEvent::new(kinds::TOOL_ERROR).with_data(json!({
            "tool": "time.now",
            "error": "time.now expects object with optional timezone"
        }));
        let classified = ErrorClassifier::classify_event(&event).unwrap();
        assert_eq!(classified.class, ErrorClass::InvalidArguments);
    }

    #[test]
    fn classifies_git_rejection() {
        let event = HarnessEvent::new(kinds::GIT_COMMIT).with_data(json!({
            "success": false,
            "skipped": true,
            "result": "rejected",
            "detail": "commit subject rejected by quality gate"
        }));
        let classified = ErrorClassifier::classify_event(&event).unwrap();
        assert_eq!(classified.surface, ErrorSurface::Git);
        assert_eq!(classified.class, ErrorClass::InvalidArguments);
    }

    #[test]
    fn classifies_unknown_failure() {
        let event = HarnessEvent::new(kinds::TOOL_ERROR).with_data(json!({
            "tool": "mystery",
            "error": "opaque failure"
        }));
        let classified = ErrorClassifier::classify_event(&event).unwrap();
        assert_eq!(classified.class, ErrorClass::Unknown);
    }
}
