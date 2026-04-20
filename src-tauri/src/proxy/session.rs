//! Proxy Session - 请求会话管理
//!
//! ## Session Identity 提取
//!
//! 支持从客户端请求中提取远端会话标识与标题同步所需的首轮 prompt：
//! - Claude Desktop: 从 `metadata.user_id` 的 JSON 字符串中提取 `session_id`，或从 `metadata.session_id` 提取
//! - 其他: 生成新的 UUID

use super::handlers::extract_first_turn_title_prompt;
use axum::http::HeaderMap;
use sha2::{Digest, Sha256};
use std::fmt::Write as _;
use uuid::Uuid;

// ============================================================================
// Session Identity 提取器
// ============================================================================

/// Session ID 来源
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionIdSource {
    /// 从 metadata.user_id 提取 (Claude)
    MetadataUserId,
    /// 从 metadata.session_id 提取
    MetadataSessionId,
    /// 新生成
    Generated,
}

/// 兼容旧调用方的 Session ID 提取结果
#[derive(Debug, Clone)]
pub struct SessionIdResult {
    /// 提取或生成的 Session ID
    pub session_id: String,
    /// Session ID 来源
    pub source: SessionIdSource,
    /// 是否为客户端提供的 ID（非新生成）
    pub client_provided: bool,
}

/// 标题同步使用的完整会话身份信息
#[derive(Debug, Clone)]
pub struct SessionIdentityResult {
    /// 远端请求可见的会话 ID
    pub remote_session_id: String,
    /// 远端会话 ID 来源
    pub source: SessionIdSource,
    /// 是否为客户端提供的 ID（非新生成）
    pub client_provided: bool,
    /// 归一化后的首轮 prompt
    pub initial_prompt: Option<String>,
    /// 首轮 prompt 的稳定哈希
    pub initial_prompt_hash: Option<String>,
    /// 调试用的 prompt 摘要
    pub initial_prompt_preview: Option<String>,
}

/// 从请求中提取完整 Session Identity
pub fn extract_session_identity(
    _headers: &HeaderMap,
    body: &serde_json::Value,
) -> SessionIdentityResult {
    let session = extract_from_metadata(body).unwrap_or_else(generate_new_session_id);
    let initial_prompt = extract_first_turn_title_prompt(body);
    let initial_prompt_hash = initial_prompt.as_deref().map(hash_prompt_text);
    let initial_prompt_preview = initial_prompt
        .as_deref()
        .map(build_prompt_preview)
        .filter(|value| !value.is_empty());

    SessionIdentityResult {
        remote_session_id: session.session_id,
        source: session.source,
        client_provided: session.client_provided,
        initial_prompt,
        initial_prompt_hash,
        initial_prompt_preview,
    }
}

/// 从请求中提取或生成 Session ID
///
/// 兼容旧调用方，内部委托给 `extract_session_identity`。
#[allow(dead_code)]
pub fn extract_session_id(headers: &HeaderMap, body: &serde_json::Value) -> SessionIdResult {
    let identity = extract_session_identity(headers, body);
    SessionIdResult {
        session_id: identity.remote_session_id,
        source: identity.source,
        client_provided: identity.client_provided,
    }
}

/// 从 metadata 提取 Session ID (Claude)
fn extract_from_metadata(body: &serde_json::Value) -> Option<SessionIdResult> {
    let metadata = body.get("metadata")?;

    if let Some(user_id) = metadata.get("user_id").and_then(|v| v.as_str()) {
        if let Some(session_id) = parse_session_from_user_id(user_id) {
            return Some(SessionIdResult {
                session_id,
                source: SessionIdSource::MetadataUserId,
                client_provided: true,
            });
        }
    }

    if let Some(session_id) = metadata.get("session_id").and_then(|v| v.as_str()) {
        if !session_id.is_empty() {
            return Some(SessionIdResult {
                session_id: session_id.to_string(),
                source: SessionIdSource::MetadataSessionId,
                client_provided: true,
            });
        }
    }

    None
}

/// 从 user_id JSON 字符串解析 session_id
///
/// 格式: `{"device_id":"...","account_uuid":"...","session_id":"..."}`
pub(super) fn parse_session_from_user_id(user_id: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(user_id)
        .ok()?
        .get("session_id")
        .and_then(serde_json::Value::as_str)
        .filter(|session_id| !session_id.is_empty())
        .map(str::to_string)
}

fn generate_new_session_id() -> SessionIdResult {
    SessionIdResult {
        session_id: Uuid::new_v4().to_string(),
        source: SessionIdSource::Generated,
        client_provided: false,
    }
}

fn hash_prompt_text(prompt: &str) -> String {
    let digest = Sha256::digest(prompt.as_bytes());
    let mut value = String::with_capacity(digest.len() * 2);
    for byte in digest {
        let _ = write!(&mut value, "{byte:02x}");
    }
    value
}

fn build_prompt_preview(prompt: &str) -> String {
    let mut preview = prompt.trim().chars().take(80).collect::<String>();
    if prompt.trim().chars().count() > 80 {
        preview.push_str("...");
    }
    preview
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_extract_session_from_claude_metadata_user_id_json() {
        let headers = HeaderMap::new();
        let body = json!({
            "model": "claude-3-5-sonnet",
            "messages": [{"role": "user", "content": "Hello"}],
            "metadata": {
                "user_id": "{\"device_id\":\"device-123\",\"account_uuid\":\"\",\"session_id\":\"abc123def456\"}"
            }
        });

        let result = extract_session_identity(&headers, &body);

        assert_eq!(result.remote_session_id, "abc123def456");
        assert_eq!(result.source, SessionIdSource::MetadataUserId);
        assert!(result.client_provided);
        assert_eq!(result.initial_prompt.as_deref(), Some("Hello"));
        assert!(result.initial_prompt_hash.is_some());
    }

    #[test]
    fn test_extract_session_generates_new_for_legacy_user_id_format() {
        let headers = HeaderMap::new();
        let body = json!({
            "model": "claude-3-5-sonnet",
            "messages": [{"role": "user", "content": "Hello"}],
            "metadata": {
                "user_id": "user_john_doe_session_abc123def456"
            }
        });

        let result = extract_session_identity(&headers, &body);

        assert!(!result.remote_session_id.is_empty());
        assert_eq!(result.source, SessionIdSource::Generated);
        assert!(!result.client_provided);
    }

    #[test]
    fn test_extract_session_from_claude_metadata_session_id() {
        let headers = HeaderMap::new();
        let body = json!({
            "model": "claude-3-5-sonnet",
            "messages": [{"role": "user", "content": "Hello"}],
            "metadata": {
                "session_id": "my-session-123"
            }
        });

        let result = extract_session_identity(&headers, &body);

        assert_eq!(result.remote_session_id, "my-session-123");
        assert_eq!(result.source, SessionIdSource::MetadataSessionId);
        assert!(result.client_provided);
    }

    #[test]
    fn test_extract_session_generates_new_when_not_found() {
        let headers = HeaderMap::new();
        let body = json!({
            "model": "claude-3-5-sonnet",
            "messages": [{"role": "user", "content": "Hello"}]
        });

        let result = extract_session_identity(&headers, &body);

        assert!(!result.remote_session_id.is_empty());
        assert_eq!(result.source, SessionIdSource::Generated);
        assert!(!result.client_provided);
    }

    #[test]
    fn test_extract_session_identity_hash_is_stable() {
        let headers = HeaderMap::new();
        let first = json!({
            "messages": [{"role": "user", "content": "Hello   world"}]
        });
        let second = json!({
            "messages": [{"role": "user", "content": "Hello world"}]
        });

        let first = extract_session_identity(&headers, &first);
        let second = extract_session_identity(&headers, &second);

        assert_eq!(first.initial_prompt_hash, second.initial_prompt_hash);
    }

    #[test]
    fn test_parse_session_from_user_id_json() {
        assert_eq!(
            parse_session_from_user_id(
                "{\"device_id\":\"device-123\",\"account_uuid\":\"\",\"session_id\":\"abc123\"}"
            ),
            Some("abc123".to_string())
        );
        assert_eq!(
            parse_session_from_user_id(
                "{\"device_id\":\"device-123\",\"account_uuid\":\"\",\"session_id\":\"xyz789\"}"
            ),
            Some("xyz789".to_string())
        );
        assert_eq!(parse_session_from_user_id("user_john_session_abc123"), None);
        assert_eq!(parse_session_from_user_id("not-json"), None);
        assert_eq!(
            parse_session_from_user_id("{\"device_id\":\"device-123\"}"),
            None
        );
    }
}
