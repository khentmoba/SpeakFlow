use reqwest::StatusCode;

use crate::services::llm_provider::LlmEndpoint;

use super::protocol::{extract_api_error_code, extract_api_error_message};

/// ChatGPT plan rejections that another transport cannot fix.
const TERMINAL_PLAN_CODES: [&str; 2] = [
    "subscription_sharing_usage_limit_exceeded",
    "subscription_sharing_user_not_eligible",
];

/// Why an LLM request failed. The kind travels with the error, so retry
/// decisions never depend on the wording of a message.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LlmError {
    /// The provider answered with a non-success status.
    #[error("API returned error {status}: {message}")]
    Http {
        status: StatusCode,
        code: Option<String>,
        message: String,
    },
    /// Sign-in or plan entitlement was rejected; the user has to act.
    #[error("{0}")]
    Auth(String),
    /// Network, timeout, parsing, and empty-response failures.
    #[error("{0}")]
    Other(String),
}

impl LlmError {
    pub(crate) fn http(endpoint: &LlmEndpoint, status: StatusCode, body_text: &str) -> Self {
        Self::Http {
            status,
            code: extract_api_error_code(body_text),
            message: extract_api_error_message(endpoint, body_text),
        }
    }

    /// Auth, policy, and quota failures that changing transport cannot recover.
    pub fn is_terminal(&self) -> bool {
        match self {
            Self::Http { status, code, .. } => {
                matches!(status.as_u16(), 401..=403) || is_terminal_plan_code(code.as_deref())
            }
            Self::Auth(_) => true,
            Self::Other(_) => false,
        }
    }
}

pub(crate) fn is_terminal_plan_code(code: Option<&str>) -> bool {
    code.is_some_and(|code| TERMINAL_PLAN_CODES.contains(&code))
}

impl From<String> for LlmError {
    fn from(message: String) -> Self {
        Self::Other(message)
    }
}

impl From<&str> for LlmError {
    fn from(message: &str) -> Self {
        Self::Other(message.to_string())
    }
}

impl From<LlmError> for String {
    fn from(error: LlmError) -> Self {
        error.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn http(status: u16, code: Option<&str>) -> LlmError {
        LlmError::Http {
            status: StatusCode::from_u16(status).unwrap(),
            code: code.map(str::to_string),
            message: "rejected".to_string(),
        }
    }

    #[test]
    fn auth_and_entitlement_failures_are_terminal() {
        for status in [401, 402, 403] {
            assert!(http(status, None).is_terminal(), "{status}");
        }
        for code in TERMINAL_PLAN_CODES {
            assert!(http(429, Some(code)).is_terminal(), "{code}");
        }
        assert!(LlmError::Auth("sign in again".into()).is_terminal());
    }

    #[test]
    fn transient_failures_are_not_terminal_whatever_the_wording() {
        assert!(!http(429, Some("subscription_sharing_usage_unavailable")).is_terminal());
        assert!(!http(500, None).is_terminal());
        assert!(!LlmError::Other("API returned error 401: signed out".into()).is_terminal());
    }

    #[test]
    fn http_errors_keep_the_established_message_format() {
        assert_eq!(
            http(401, None).to_string(),
            "API returned error 401 Unauthorized: rejected"
        );
    }
}
