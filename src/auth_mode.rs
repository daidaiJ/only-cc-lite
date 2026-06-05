//! Authentication mode classification for compression policy.

use http::HeaderMap;

/// Authentication mode detected from request headers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AuthMode {
    Payg,
    OAuth,
    Subscription,
}

impl AuthMode {
    pub fn as_str(self) -> &'static str {
        match self {
            AuthMode::Payg => "payg",
            AuthMode::OAuth => "oauth",
            AuthMode::Subscription => "subscription",
        }
    }
}

/// Classify auth mode from request headers.
pub fn classify(headers: &HeaderMap) -> AuthMode {
    // Anthropic subscription header
    if headers
        .get("x-api-key")
        .and_then(|v| v.to_str().ok())
        .map(|s| s.starts_with("sk-ant-"))
        .unwrap_or(false)
    {
        return AuthMode::Payg;
    }
    // OpenAI API key
    if headers.contains_key(http::header::AUTHORIZATION) {
        return AuthMode::Payg;
    }
    AuthMode::Payg
}
