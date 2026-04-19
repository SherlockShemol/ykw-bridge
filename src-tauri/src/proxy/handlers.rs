//! 请求处理器
//!
//! 处理各种API端点的HTTP请求
//!
//! 重构后的结构：
//! - 通用逻辑提取到 `handler_context` 和 `response_processor` 模块
//! - 各 handler 只保留独特的业务逻辑
//! - Claude 的格式转换逻辑保留在此文件（用于 OpenRouter 旧接口回退）

use super::{
    error_mapper::{get_error_message, map_proxy_error_to_status},
    handler_config::CLAUDE_PARSER_CONFIG,
    handler_context::RequestContext,
    local_session_title_watcher::build_session_title_sync_key,
    providers::{
        get_adapter, get_claude_api_format, streaming::create_anthropic_sse_stream,
        streaming_responses::create_anthropic_sse_stream_from_responses, transform,
        transform_responses,
    },
    response_processor::{
        create_logged_passthrough_stream, process_response, read_decoded_body,
        strip_entity_headers_for_rebuilt_body, strip_hop_by_hop_response_headers,
        SseUsageCollector,
    },
    server::ProxyState,
    types::*,
    usage::parser::TokenUsage,
    ProxyError,
};
use crate::app_config::AppType;
use crate::proxy::sse::strip_sse_field;
use axum::{
    extract::{Path, State},
    http::HeaderMap,
    http::StatusCode,
    response::IntoResponse,
    Json,
};
use bytes::Bytes;
use http_body_util::BodyExt;
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::{BTreeSet, HashSet};
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

// ============================================================================
// 健康检查和状态查询（简单端点）
// ============================================================================

/// 健康检查
pub async fn health_check() -> (StatusCode, Json<Value>) {
    (
        StatusCode::OK,
        Json(json!({
            "status": "healthy",
            "timestamp": chrono::Utc::now().to_rfc3339(),
        })),
    )
}

/// 获取服务状态
pub async fn get_status(State(state): State<ProxyState>) -> Result<Json<ProxyStatus>, ProxyError> {
    let status = state.status.read().await.clone();
    Ok(Json(status))
}

// ============================================================================
// Claude API 处理器（包含格式转换逻辑）
// ============================================================================

/// 处理 /v1/messages 请求（Claude API）
///
/// Claude 处理器包含独特的格式转换逻辑：
/// - 过去用于 OpenRouter 的 OpenAI Chat Completions 兼容接口（Anthropic ↔ OpenAI 转换）
/// - 现在 OpenRouter 已推出 Claude Code 兼容接口，默认不再启用该转换（逻辑保留以备回退）
pub async fn handle_messages(
    State(state): State<ProxyState>,
    request: axum::extract::Request,
) -> Result<axum::response::Response, ProxyError> {
    handle_claude_messages_for_app(state, request, AppType::Claude, "Claude", "claude").await
}

pub async fn handle_claude_desktop_messages(
    State(state): State<ProxyState>,
    request: axum::extract::Request,
) -> Result<axum::response::Response, ProxyError> {
    validate_claude_desktop_gateway_auth(request.headers())?;
    handle_claude_messages_for_app(
        state,
        request,
        AppType::ClaudeDesktop,
        "Claude Desktop",
        "claude_desktop",
    )
    .await
}

pub async fn handle_claude_desktop_models(
    State(_state): State<ProxyState>,
    request: axum::extract::Request,
) -> Result<Json<Value>, ProxyError> {
    validate_claude_desktop_gateway_auth(request.headers())?;

    let config = crate::claude_desktop_config::read_live_config().unwrap_or_else(|_| json!({}));
    let model_ids = extract_claude_desktop_model_ids(&config);

    Ok(Json(json!({
        "object": "list",
        "data": model_ids.into_iter().map(|id| {
            json!({
                "id": id,
                "object": "model",
                "created": 0,
                "owned_by": "ykw-bridge"
            })
        }).collect::<Vec<_>>()
    })))
}

fn extract_gateway_auth_token(headers: &axum::http::HeaderMap) -> Option<String> {
    headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .and_then(|value| value.strip_prefix("Bearer "))
        .map(|value| value.trim().to_string())
        .or_else(|| {
            headers
                .get("x-api-key")
                .and_then(|value| value.to_str().ok())
                .map(|value| value.trim().to_string())
        })
        .or_else(|| {
            headers
                .get("anthropic-api-key")
                .and_then(|value| value.to_str().ok())
                .map(|value| value.trim().to_string())
        })
}

fn validate_claude_desktop_gateway_auth(headers: &axum::http::HeaderMap) -> Result<(), ProxyError> {
    let expected = crate::settings::ensure_claude_desktop_gateway_secret()
        .map_err(|e| ProxyError::Internal(format!("Failed to load gateway secret: {e}")))?;

    match extract_gateway_auth_token(headers) {
        Some(token) if token == expected => Ok(()),
        _ => Err(ProxyError::AuthError(
            "Claude Desktop gateway authentication failed".to_string(),
        )),
    }
}

fn extract_claude_desktop_model_ids(config: &Value) -> Vec<String> {
    let mut ordered = Vec::new();
    let mut seen = BTreeSet::new();

    let fallback = config
        .get("enterpriseConfig")
        .and_then(|value| value.get("fallbackModels"))
        .and_then(|value| value.as_object());

    for key in [
        "model",
        "haiku",
        "sonnet",
        "opus",
        "haikuModel",
        "sonnetModel",
        "opusModel",
    ] {
        if let Some(id) = fallback
            .and_then(|value| value.get(key))
            .and_then(|value| value.as_str())
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            let id = id.to_string();
            if seen.insert(id.clone()) {
                ordered.push(id);
            }
        }
    }

    if ordered.is_empty() {
        ordered.push("claude-sonnet-4-20250514".to_string());
    }

    ordered
}

const OFFICIAL_SESSION_TITLE_PROMPT: &str = concat!(
    "You are coming up with a succinct title for a coding session based on the provided description.\n",
    "The title should be clear, concise, and accurately reflect the content of the coding task.\n",
    "You should keep it short and simple, ideally no more than 6 words. Avoid using jargon or\n",
    "overly technical terms unless absolutely necessary. The title should be easy to understand\n",
    "for anyone reading it.\n",
    "You should wrap the title in <title> tags.\n\n",
    "For example:\n",
    "<title>Fix login button not working on mobile</title>\n",
    "<title>Update README with installation instructions</title>\n",
    "<title>Improve performance of data processing script</title>\n"
);
const TITLE_GENERATION_SKIP_HEADER: &str = "x-ykw-bridge-title-gen";

static TARGETED_SESSION_TITLE_SYNC_RUNNING: LazyLock<Mutex<HashSet<String>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));
static LOCAL_SESSION_TITLE_PATH_SYNC_RUNNING: LazyLock<Mutex<HashSet<String>>> =
    LazyLock::new(|| Mutex::new(HashSet::new()));

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ChatConversationTitleRequest {
    pub message_content: String,
    #[serde(default)]
    pub recent_titles: Vec<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionTitleRequest {
    pub first_session_message: String,
}

fn should_skip_title_sync(headers: &axum::http::HeaderMap) -> bool {
    headers
        .get(TITLE_GENERATION_SKIP_HEADER)
        .and_then(|value| value.to_str().ok())
        .map(|value| matches!(value.trim(), "1" | "true" | "yes"))
        .unwrap_or(false)
}

fn collapse_title_prompt_text(raw: &str) -> Option<String> {
    let collapsed = strip_title_prompt_wrapper_sections(raw)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let trimmed = collapsed.trim();
    if trimmed.is_empty()
        || trimmed.contains("<local-command-caveat>")
        || trimmed.starts_with("<command-name>")
    {
        None
    } else {
        Some(trimmed.to_string())
    }
}

fn strip_title_prompt_wrapper_sections(raw: &str) -> String {
    let mut cleaned = raw.to_string();
    {
        let tag = "system-reminder";
        cleaned = strip_tagged_prompt_sections(&cleaned, tag);
    }
    cleaned
}

fn strip_tagged_prompt_sections(raw: &str, tag: &str) -> String {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let mut cleaned = raw.to_string();

    while let Some(start) = cleaned.find(&open) {
        let search_start = start + open.len();
        let Some(relative_end) = cleaned[search_start..].find(&close) else {
            break;
        };
        let end = search_start + relative_end + close.len();
        cleaned.replace_range(start..end, " ");
    }

    cleaned
}

fn is_session_title_generation_prompt(prompt: &str) -> bool {
    let prompt = prompt.trim();
    prompt.starts_with("You are coming up with a succinct title for")
        && prompt.contains("You should wrap the title in <title> tags.")
        && prompt.contains("Please generate a title for this session.")
}

fn extract_title_prompt_text(value: &Value) -> Option<String> {
    match value {
        Value::String(raw) => collapse_title_prompt_text(raw),
        Value::Array(items) => {
            let text = items
                .iter()
                .filter_map(|item| match item {
                    Value::String(raw) => Some(raw.as_str()),
                    Value::Object(obj) => match obj.get("type").and_then(Value::as_str) {
                        Some("text") | Some("input_text") | Some("output_text") => {
                            obj.get("text").and_then(Value::as_str)
                        }
                        _ => None,
                    },
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join(" ");
            collapse_title_prompt_text(&text)
        }
        _ => None,
    }
}

fn extract_first_turn_title_prompt_from_messages(messages: &[Value]) -> Option<String> {
    let user_messages = messages
        .iter()
        .filter(|message| message.get("role").and_then(Value::as_str) == Some("user"))
        .filter_map(|message| message.get("content"))
        .filter_map(extract_title_prompt_text)
        .collect::<Vec<_>>();

    if user_messages.len() == 1 {
        user_messages.into_iter().next()
    } else {
        None
    }
}

fn extract_first_turn_title_prompt_from_responses_input(input: &Value) -> Option<String> {
    match input {
        Value::String(raw) => collapse_title_prompt_text(raw),
        Value::Array(items) => {
            let user_messages = items
                .iter()
                .filter(|item| item.get("role").and_then(Value::as_str) == Some("user"))
                .filter_map(|item| item.get("content"))
                .filter_map(extract_title_prompt_text)
                .collect::<Vec<_>>();

            if user_messages.len() == 1 {
                user_messages.into_iter().next()
            } else {
                None
            }
        }
        Value::Object(_) => input
            .get("content")
            .filter(|_| input.get("role").and_then(Value::as_str) == Some("user"))
            .and_then(extract_title_prompt_text),
        _ => None,
    }
}

fn extract_first_turn_title_prompt(body: &Value) -> Option<String> {
    body.get("messages")
        .and_then(Value::as_array)
        .and_then(|messages| extract_first_turn_title_prompt_from_messages(messages))
        .or_else(|| {
            body.get("input")
                .and_then(extract_first_turn_title_prompt_from_responses_input)
        })
}

pub(crate) async fn run_targeted_session_title_sync(
    state: ProxyState,
    session_id: &str,
    prompt: &str,
    preference: crate::claude_desktop_config::LocalSessionTitleLookupPreference,
) -> Result<bool, ProxyError> {
    let session_id = session_id.trim();
    let prompt = prompt.trim();
    if session_id.is_empty() && prompt.is_empty() {
        return Ok(false);
    }

    let lookup_session_id = session_id.to_string();
    let lookup_prompt = prompt.to_string();
    let lookup = tokio::task::spawn_blocking(move || {
        crate::claude_desktop_config::lookup_local_session_title_target(
            &lookup_session_id,
            &lookup_prompt,
            preference,
        )
    })
    .await
    .map_err(|e| ProxyError::Internal(format!("Targeted title lookup task failed: {e}")))?
    .map_err(|e| ProxyError::Internal(format!("Targeted title lookup failed: {e}")))?;

    match lookup {
        crate::claude_desktop_config::LocalSessionTitleLookup::NotFound => Ok(false),
        crate::claude_desktop_config::LocalSessionTitleLookup::AlreadyTitled { kind, path } => {
            log::debug!(
                "Claude Desktop {} 会话已有标题，跳过自动命名: sessionId={}, path={}",
                kind,
                session_id,
                path.display()
            );
            Ok(true)
        }
        crate::claude_desktop_config::LocalSessionTitleLookup::Pending {
            kind,
            path,
            description,
        } => {
            let path_key = path.display().to_string();
            {
                let mut running = LOCAL_SESSION_TITLE_PATH_SYNC_RUNNING
                    .lock()
                    .expect("local session title sync lock poisoned");
                if !running.insert(path_key.clone()) {
                    return Ok(true);
                }
            }

            let result = async {
            let description = if prompt.is_empty() {
                description
            } else {
                Some(prompt.to_string())
            };
            let Some(description) = description
                .map(|value| value.trim().to_string())
                .filter(|value| !value.is_empty())
            else {
                return Ok(false);
            };

            let temp_path = path.clone();
            let temp_prompt = description.clone();
            let persisted_temp = tokio::task::spawn_blocking(move || {
                crate::claude_desktop_config::persist_prompt_session_title(&temp_path, &temp_prompt)
            })
            .await
            .map_err(|e| {
                ProxyError::Internal(format!("Prompt title persistence task failed: {e}"))
            })?
            .map_err(|e| ProxyError::Internal(format!("Prompt title persistence failed: {e}")))?;

            if persisted_temp {
                log::info!(
                    "Claude Desktop {} 会话已写入临时标题: sessionId={}, path={}, title={}",
                    kind,
                    session_id,
                    path.display(),
                    description
                );
            }

            let generated_title = match generate_claude_desktop_title_via_gateway(
                &state,
                &description,
                &[],
            )
            .await
            {
                Ok(title) => title,
                Err(err) => {
                    log::debug!(
                        "Claude Desktop {} 会话自然标题生成失败，保留临时标题: sessionId={}, error={}",
                        kind,
                        session_id,
                        err
                    );
                    return Ok(true);
                }
            };

            let persist_path = path.clone();
            let generated_title_for_persist = generated_title.clone();
            let persisted_generated = tokio::task::spawn_blocking(move || {
                crate::claude_desktop_config::replace_prompt_session_title(
                    &persist_path,
                    &generated_title_for_persist,
                )
            })
            .await
            .map_err(|e| {
                ProxyError::Internal(format!("Generated title persistence task failed: {e}"))
            })?
            .map_err(|e| {
                ProxyError::Internal(format!("Generated title persistence failed: {e}"))
            })?;

            if persisted_generated {
                log::info!(
                    "Claude Desktop {} 会话自然标题已更新: sessionId={}, path={}, title={}",
                    kind,
                    session_id,
                    path.display(),
                    generated_title
                );
            }

            Ok(true)
            }
            .await;

            let mut running = LOCAL_SESSION_TITLE_PATH_SYNC_RUNNING
                .lock()
                .expect("local session title sync lock poisoned");
            running.remove(&path_key);

            result
        }
    }
}

fn schedule_targeted_session_title_sync(
    state: ProxyState,
    session_id: String,
    prompt: String,
    preference: crate::claude_desktop_config::LocalSessionTitleLookupPreference,
) {
    if session_id.trim().is_empty() && prompt.trim().is_empty() {
        return;
    }

    let has_prompt = !prompt.trim().is_empty();
    let sync_key = build_session_title_sync_key(&session_id, &prompt);
    {
        let mut running = TARGETED_SESSION_TITLE_SYNC_RUNNING
            .lock()
            .expect("targeted title sync lock poisoned");
        if !running.insert(sync_key.clone()) {
            return;
        }
    }

    log::info!(
        "Claude Desktop 会话标题同步已触发: sessionId={}, promptExtracted={}, preference={:?}",
        session_id,
        has_prompt,
        preference
    );

    tokio::spawn(async move {
        let result =
            run_targeted_session_title_sync(state.clone(), &session_id, &prompt, preference).await;

        match result {
            Ok(true) => {
                state
                    .local_session_title_watcher
                    .clear_pending_sync(&session_id, &prompt);
            }
            Ok(false) => {
                state.local_session_title_watcher.register_pending_sync(
                    state.clone(),
                    session_id.clone(),
                    prompt.clone(),
                    preference,
                );
                log::debug!(
                    "Claude Desktop 会话自动命名暂未命中本地会话，等待 watcher 捕获本地文件落地: sessionId={}, promptExtracted={}",
                    session_id,
                    has_prompt
                );
            }
            Err(err) => {
                log::debug!(
                    "Claude Desktop 会话自动命名失败: sessionId={}, error={}",
                    session_id,
                    err
                );
            }
        }

        let mut running = TARGETED_SESSION_TITLE_SYNC_RUNNING
            .lock()
            .expect("targeted title sync lock poisoned");
        running.remove(&sync_key);
    });
}

fn maybe_schedule_claude_desktop_title_sync(
    state: &ProxyState,
    headers: &HeaderMap,
    session_id: &str,
    body: &Value,
    preference: crate::claude_desktop_config::LocalSessionTitleLookupPreference,
) {
    if should_skip_title_sync(headers) {
        return;
    }

    let prompt = extract_first_turn_title_prompt(body).unwrap_or_default();
    if is_session_title_generation_prompt(&prompt) {
        return;
    }
    schedule_targeted_session_title_sync(
        state.clone(),
        session_id.trim().to_string(),
        prompt,
        preference,
    );
}

fn sanitize_generated_title(raw: &str) -> Option<String> {
    let mut text = raw.trim().to_string();

    if let (Some(start), Some(end)) = (text.find("<title>"), text.find("</title>")) {
        if end > start + "<title>".len() {
            text = text[start + "<title>".len()..end].to_string();
        }
    }

    let stripped = text.trim().trim_matches('\"').trim_matches('\'').trim();
    let stripped = stripped.strip_prefix("Title:").unwrap_or(stripped).trim();
    text = stripped.to_string();

    let collapsed = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        None
    } else if collapsed.chars().count() <= 80 {
        Some(collapsed)
    } else {
        let mut truncated = collapsed.chars().take(80).collect::<String>();
        truncated.push_str("...");
        Some(truncated)
    }
}

fn extract_anthropic_text_response(response: &Value) -> Option<String> {
    response
        .get("content")
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter(|item| item.get("type").and_then(Value::as_str) == Some("text"))
                .filter_map(|item| item.get("text").and_then(Value::as_str))
                .collect::<Vec<_>>()
                .join("\n")
        })
        .filter(|text| !text.trim().is_empty())
}

fn select_claude_desktop_title_model() -> String {
    let config = crate::claude_desktop_config::read_live_config().unwrap_or_else(|_| json!({}));
    let fallback = config
        .get("enterpriseConfig")
        .and_then(|value| value.get("fallbackModels"))
        .and_then(Value::as_object);

    for key in ["haikuModel", "haiku", "model", "sonnetModel", "sonnet"] {
        if let Some(model) = fallback
            .and_then(|value| value.get(key))
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            return model.to_string();
        }
    }

    "claude-sonnet-4-20250514".to_string()
}

fn build_official_session_title_prompt(description: &str, recent_titles: &[String]) -> String {
    let mut prompt = String::from(OFFICIAL_SESSION_TITLE_PROMPT);
    prompt.push_str("\nHere is the session description:\n");
    prompt.push_str("<description>");
    prompt.push_str(description.trim());
    prompt.push_str("</description>\n");

    if !recent_titles.is_empty() {
        prompt.push_str("Avoid duplicating these recent titles:\n");
        for title in recent_titles {
            let title = title.trim();
            if !title.is_empty() {
                prompt.push_str("- ");
                prompt.push_str(title);
                prompt.push('\n');
            }
        }
    }

    prompt.push_str("Please generate a title for this session.");
    prompt
}

async fn generate_claude_desktop_title_via_gateway(
    state: &ProxyState,
    description: &str,
    recent_titles: &[String],
) -> Result<String, ProxyError> {
    let description = description.trim();
    if description.is_empty() {
        return Err(ProxyError::InvalidRequest(
            "Missing session description for title generation".to_string(),
        ));
    }

    let config = state.config.read().await.clone();
    let host =
        crate::claude_desktop_config::loopback_host_for_listen_address(&config.listen_address);
    let gateway_secret = crate::settings::ensure_claude_desktop_gateway_secret()
        .map_err(|e| ProxyError::Internal(format!("Failed to load gateway secret: {e}")))?;
    let url = format!(
        "http://{}:{}/claude-desktop/v1/messages",
        host, config.listen_port
    );
    let model = select_claude_desktop_title_model();
    let prompt = build_official_session_title_prompt(description, recent_titles);

    let response = reqwest::Client::new()
        .post(&url)
        .timeout(Duration::from_secs(90))
        .header(
            axum::http::header::AUTHORIZATION.as_str(),
            format!("Bearer {gateway_secret}"),
        )
        .header(TITLE_GENERATION_SKIP_HEADER, "1")
        .json(&json!({
            "model": model,
            "max_tokens": 64,
            "temperature": 0,
            "stream": false,
            "messages": [
                {
                    "role": "user",
                    "content": prompt
                }
            ]
        }))
        .send()
        .await
        .map_err(|e| ProxyError::Internal(format!("Title generation request failed: {e}")))?;

    let status = response.status();
    let body = response
        .text()
        .await
        .map_err(|e| ProxyError::Internal(format!("Failed to read title generation body: {e}")))?;
    if !status.is_success() {
        return Err(ProxyError::UpstreamError {
            status: status.as_u16(),
            body: Some(body),
        });
    }

    let parsed: Value = serde_json::from_str(&body).map_err(|e| {
        ProxyError::TransformError(format!("Failed to parse title generation response: {e}"))
    })?;
    let raw_text = extract_anthropic_text_response(&parsed).ok_or_else(|| {
        ProxyError::TransformError("Title generation response did not contain text".to_string())
    })?;

    sanitize_generated_title(&raw_text).ok_or_else(|| {
        ProxyError::TransformError("Title generation returned an empty title".to_string())
    })
}

pub async fn handle_chat_conversation_title(
    State(state): State<ProxyState>,
    Path((_org_id, _conversation_id)): Path<(String, String)>,
    Json(payload): Json<ChatConversationTitleRequest>,
) -> Result<Json<Value>, ProxyError> {
    let title = generate_claude_desktop_title_via_gateway(
        &state,
        &payload.message_content,
        &payload.recent_titles,
    )
    .await?;
    Ok(Json(json!({ "title": title })))
}

pub async fn handle_session_title(
    State(state): State<ProxyState>,
    Path(_org_id): Path<String>,
    Json(payload): Json<SessionTitleRequest>,
) -> Result<Json<Value>, ProxyError> {
    let title =
        generate_claude_desktop_title_via_gateway(&state, &payload.first_session_message, &[])
            .await?;
    Ok(Json(json!({ "title": title })))
}

fn merge_missing_fields(target: &mut Value, fallback: &Value) {
    let (Some(target_obj), Some(fallback_obj)) = (target.as_object_mut(), fallback.as_object())
    else {
        return;
    };

    for (key, value) in fallback_obj {
        if !target_obj.contains_key(key) {
            target_obj.insert(key.clone(), value.clone());
        }
    }
}

fn parse_responses_sse_body(body_str: &str) -> Result<Value, ProxyError> {
    let mut response_created: Option<Value> = None;
    let mut response_completed: Option<Value> = None;
    let mut output_items: Vec<Value> = Vec::new();

    for block in body_str.split("\n\n") {
        if block.trim().is_empty() {
            continue;
        }

        let mut event_type: Option<&str> = None;
        let mut data_parts: Vec<&str> = Vec::new();

        for line in block.lines() {
            if let Some(event) = strip_sse_field(line, "event") {
                event_type = Some(event.trim());
            } else if let Some(data) = strip_sse_field(line, "data") {
                data_parts.push(data);
            }
        }

        if data_parts.is_empty() {
            continue;
        }

        let data = data_parts.join("\n");
        if data.trim() == "[DONE]" {
            continue;
        }

        let payload: Value = match serde_json::from_str(&data) {
            Ok(value) => value,
            Err(_) => continue,
        };

        let event_name = event_type
            .or_else(|| payload.get("type").and_then(|value| value.as_str()))
            .unwrap_or_default();

        match event_name {
            "response.created" => {
                if let Some(response) = payload.get("response") {
                    response_created = Some(response.clone());
                }
            }
            "response.output_item.done" => {
                if let Some(item) = payload.get("item") {
                    output_items.push(item.clone());
                }
            }
            "response.completed" => {
                if let Some(response) = payload.get("response") {
                    response_completed = Some(response.clone());
                }
            }
            _ => {}
        }
    }

    let mut response = response_completed
        .or(response_created.clone())
        .ok_or_else(|| {
            ProxyError::TransformError("Failed to reconstruct Responses SSE body".to_string())
        })?;

    if let Some(created) = response_created.as_ref() {
        merge_missing_fields(&mut response, created);
    }

    let needs_output_fill = response
        .get("output")
        .and_then(|value| value.as_array())
        .map(|items| items.is_empty())
        .unwrap_or(true);

    if needs_output_fill && !output_items.is_empty() {
        response["output"] = Value::Array(output_items);
    }

    Ok(response)
}

fn parse_upstream_openai_responses_body(body_bytes: &[u8]) -> Result<Value, ProxyError> {
    if let Ok(json_value) = serde_json::from_slice(body_bytes) {
        return Ok(json_value);
    }

    let body_str = String::from_utf8_lossy(body_bytes);
    if body_str.contains("event:") && body_str.contains("data:") {
        return parse_responses_sse_body(&body_str);
    }

    Err(ProxyError::TransformError(
        "Failed to parse upstream Responses body".to_string(),
    ))
}

async fn handle_claude_messages_for_app(
    state: ProxyState,
    request: axum::extract::Request,
    app_type: AppType,
    tag: &'static str,
    app_type_str: &'static str,
) -> Result<axum::response::Response, ProxyError> {
    let (parts, body) = request.into_parts();
    let uri = parts.uri;
    let headers = parts.headers;
    let extensions = parts.extensions;
    let body_bytes = body
        .collect()
        .await
        .map_err(|e| ProxyError::Internal(format!("Failed to read request body: {e}")))?
        .to_bytes();
    let body: Value = serde_json::from_slice(&body_bytes)
        .map_err(|e| ProxyError::Internal(format!("Failed to parse request body: {e}")))?;

    let mut ctx =
        RequestContext::new(&state, &body, &headers, app_type.clone(), tag, app_type_str).await?;
    if app_type == AppType::ClaudeDesktop {
        maybe_schedule_claude_desktop_title_sync(
            &state,
            &headers,
            &ctx.session_id,
            &body,
            crate::claude_desktop_config::LocalSessionTitleLookupPreference::CoworkFirst,
        );
    }

    let endpoint = uri
        .path_and_query()
        .map(|path_and_query| path_and_query.as_str())
        .unwrap_or(uri.path());

    let is_stream = body
        .get("stream")
        .and_then(|s| s.as_bool())
        .unwrap_or(false);

    // 转发请求
    let forwarder = ctx.create_forwarder(&state);
    let result = match forwarder
        .forward_with_retry(
            &app_type,
            endpoint,
            body.clone(),
            headers,
            extensions,
            ctx.get_providers(),
        )
        .await
    {
        Ok(result) => result,
        Err(mut err) => {
            if let Some(provider) = err.provider.take() {
                ctx.provider = provider;
            }
            log_forward_error(&state, &ctx, is_stream, &err.error);
            return Err(err.error);
        }
    };

    ctx.provider = result.provider;
    let api_format = result
        .claude_api_format
        .as_deref()
        .unwrap_or_else(|| get_claude_api_format(&ctx.provider))
        .to_string();
    let response = result.response;

    // 检查是否需要格式转换（OpenRouter 等中转服务）
    let adapter = get_adapter(&app_type);
    let needs_transform = adapter.needs_transform(&ctx.provider);

    // Claude 特有：格式转换处理
    if needs_transform {
        return handle_claude_transform(response, &ctx, &state, &body, is_stream, &api_format)
            .await;
    }

    // 通用响应处理（透传模式）
    process_response(response, &ctx, &state, &CLAUDE_PARSER_CONFIG).await
}

#[cfg(test)]
#[allow(clippy::items_after_test_module)]
mod tests {
    use super::{
        build_official_session_title_prompt, extract_claude_desktop_model_ids,
        extract_first_turn_title_prompt, is_session_title_generation_prompt,
        parse_responses_sse_body, sanitize_generated_title,
    };
    use serde_json::json;

    #[test]
    fn claude_desktop_model_ids_use_fallback_models_and_deduplicate() {
        let config = json!({
            "enterpriseConfig": {
                "fallbackModels": {
                    "model": "claude-sonnet-4",
                    "haiku": "claude-haiku-4",
                    "sonnet": "claude-sonnet-4",
                    "opus": "claude-opus-4",
                    "haikuModel": "claude-haiku-4",
                    "sonnetModel": "claude-sonnet-4",
                    "opusModel": "claude-opus-4"
                }
            }
        });

        assert_eq!(
            extract_claude_desktop_model_ids(&config),
            vec![
                "claude-sonnet-4".to_string(),
                "claude-haiku-4".to_string(),
                "claude-opus-4".to_string(),
            ]
        );
    }

    #[test]
    fn claude_desktop_model_ids_fall_back_to_default_when_missing() {
        assert_eq!(
            extract_claude_desktop_model_ids(&json!({})),
            vec!["claude-sonnet-4-20250514".to_string()]
        );
    }

    #[test]
    fn responses_sse_body_reconstructs_output_items_for_non_stream_requests() {
        let sse = concat!(
            "event: response.created\n",
            "data: {\"type\":\"response.created\",\"response\":{\"id\":\"resp_1\",\"model\":\"gpt-5.4-mini\",\"object\":\"response\",\"output\":[]}}\n\n",
            "event: response.output_item.done\n",
            "data: {\"type\":\"response.output_item.done\",\"item\":{\"id\":\"msg_1\",\"type\":\"message\",\"role\":\"assistant\",\"status\":\"completed\",\"content\":[{\"type\":\"output_text\",\"text\":\"Could you clarify what you want me to count?\"}]}}\n\n",
            "event: response.completed\n",
            "data: {\"type\":\"response.completed\",\"response\":{\"status\":\"completed\",\"usage\":{\"input_tokens\":11,\"output_tokens\":9}}}\n\n"
        );

        let reconstructed = parse_responses_sse_body(sse).expect("should reconstruct SSE body");

        assert_eq!(
            reconstructed.get("id").and_then(|v| v.as_str()),
            Some("resp_1")
        );
        assert_eq!(
            reconstructed.get("model").and_then(|v| v.as_str()),
            Some("gpt-5.4-mini")
        );
        assert_eq!(
            reconstructed
                .get("output")
                .and_then(|v| v.as_array())
                .and_then(|items| items.first())
                .and_then(|item| item.pointer("/content/0/text"))
                .and_then(|v| v.as_str()),
            Some("Could you clarify what you want me to count?")
        );
        assert_eq!(
            reconstructed
                .pointer("/usage/input_tokens")
                .and_then(|v| v.as_u64()),
            Some(11)
        );
    }

    #[test]
    fn sanitize_generated_title_extracts_title_tag() {
        assert_eq!(
            sanitize_generated_title("prefix <title>Fix mobile login</title> suffix"),
            Some("Fix mobile login".to_string())
        );
    }

    #[test]
    fn official_session_title_prompt_includes_description_and_recent_titles() {
        let prompt = build_official_session_title_prompt(
            "Fix auth redirect loop",
            &["Fix login bug".to_string(), "Improve auth flow".to_string()],
        );

        assert!(prompt.contains("You should wrap the title in <title> tags."));
        assert!(prompt.contains("<description>Fix auth redirect loop</description>"));
        assert!(prompt.contains("Avoid duplicating these recent titles:"));
        assert!(prompt.contains("- Fix login bug"));
    }

    #[test]
    fn first_turn_title_prompt_extracts_single_user_message() {
        let body = json!({
            "messages": [
                {"role": "user", "content": [{"type": "text", "text": "请帮我检查一下标题为什么这么慢"}]}
            ]
        });

        assert_eq!(
            extract_first_turn_title_prompt(&body).as_deref(),
            Some("请帮我检查一下标题为什么这么慢")
        );
    }

    #[test]
    fn first_turn_title_prompt_skips_follow_up_turns() {
        let body = json!({
            "messages": [
                {"role": "user", "content": "第一句"},
                {"role": "assistant", "content": "收到"},
                {"role": "user", "content": "第二句"}
            ]
        });

        assert!(extract_first_turn_title_prompt(&body).is_none());
    }

    #[test]
    fn first_turn_title_prompt_extracts_responses_input() {
        let body = json!({
            "input": [
                {
                    "role": "user",
                    "content": [
                        {"type": "input_text", "text": "给这个新 session 起个标题"}
                    ]
                }
            ]
        });

        assert_eq!(
            extract_first_turn_title_prompt(&body).as_deref(),
            Some("给这个新 session 起个标题")
        );
    }

    #[test]
    fn first_turn_title_prompt_strips_system_reminders() {
        let body = json!({
            "messages": [
                {
                    "role": "user",
                    "content": [
                        {
                            "type": "text",
                            "text": "<system-reminder>skills</system-reminder>\n<system-reminder>date</system-reminder>\n你好"
                        }
                    ]
                }
            ]
        });

        assert_eq!(
            extract_first_turn_title_prompt(&body).as_deref(),
            Some("你好")
        );
    }

    #[test]
    fn detect_session_title_generation_prompt() {
        let prompt = concat!(
            "You are coming up with a succinct title for an agent chat session based on the provided description.\n",
            "The title should be clear, concise, and accurately reflect the content of the session.\n",
            "You should wrap the title in <title> tags.\n",
            "Please generate a title for this session."
        );

        assert!(is_session_title_generation_prompt(prompt));
        assert!(!is_session_title_generation_prompt("你好"));
    }
}

/// Claude 格式转换处理（独有逻辑）
///
/// 支持 OpenAI Chat Completions 和 Responses API 两种格式的转换
async fn handle_claude_transform(
    response: super::hyper_client::ProxyResponse,
    ctx: &RequestContext,
    state: &ProxyState,
    _original_body: &Value,
    is_stream: bool,
    api_format: &str,
) -> Result<axum::response::Response, ProxyError> {
    let status = response.status();

    if is_stream {
        // 根据 api_format 选择流式转换器
        let stream = response.bytes_stream();
        let sse_stream: Box<
            dyn futures::Stream<Item = Result<Bytes, std::io::Error>> + Send + Unpin,
        > = if api_format == "openai_responses" {
            Box::new(Box::pin(create_anthropic_sse_stream_from_responses(stream)))
        } else {
            Box::new(Box::pin(create_anthropic_sse_stream(stream)))
        };

        // 创建使用量收集器
        let usage_collector = {
            let state = state.clone();
            let provider_id = ctx.provider.id.clone();
            let model = ctx.request_model.clone();
            let app_type_str = ctx.app_type_str;
            let status_code = status.as_u16();
            let start_time = ctx.start_time;

            SseUsageCollector::new(start_time, move |events, first_token_ms| {
                if let Some(usage) = TokenUsage::from_claude_stream_events(&events) {
                    let latency_ms = start_time.elapsed().as_millis() as u64;
                    let state = state.clone();
                    let provider_id = provider_id.clone();
                    let model = model.clone();

                    tokio::spawn(async move {
                        log_usage(
                            &state,
                            &provider_id,
                            app_type_str,
                            &model,
                            &model,
                            usage,
                            latency_ms,
                            first_token_ms,
                            true,
                            status_code,
                        )
                        .await;
                    });
                } else {
                    log::debug!("[Claude] OpenRouter 流式响应缺少 usage 统计，跳过消费记录");
                }
            })
        };

        // 获取流式超时配置
        let timeout_config = ctx.streaming_timeout_config();

        let logged_stream = create_logged_passthrough_stream(
            sse_stream,
            "Claude/OpenRouter",
            Some(usage_collector),
            timeout_config,
        );

        let mut headers = axum::http::HeaderMap::new();
        headers.insert(
            "Content-Type",
            axum::http::HeaderValue::from_static("text/event-stream"),
        );
        headers.insert(
            "Cache-Control",
            axum::http::HeaderValue::from_static("no-cache"),
        );

        let body = axum::body::Body::from_stream(logged_stream);
        return Ok((headers, body).into_response());
    }

    // 非流式响应转换 (OpenAI/Responses → Anthropic)
    let body_timeout =
        if ctx.app_config.auto_failover_enabled && ctx.app_config.non_streaming_timeout > 0 {
            std::time::Duration::from_secs(ctx.app_config.non_streaming_timeout as u64)
        } else {
            std::time::Duration::ZERO
        };
    let (mut response_headers, _status, body_bytes) =
        read_decoded_body(response, ctx.tag, body_timeout).await?;

    let body_str = String::from_utf8_lossy(&body_bytes);

    let upstream_response: Value = if api_format == "openai_responses" {
        parse_upstream_openai_responses_body(&body_bytes).map_err(|e| {
            log::error!("[Claude] 解析上游 Responses 响应失败: {e}, body: {body_str}");
            e
        })?
    } else {
        serde_json::from_slice(&body_bytes).map_err(|e| {
            log::error!("[Claude] 解析上游响应失败: {e}, body: {body_str}");
            ProxyError::TransformError(format!("Failed to parse upstream response: {e}"))
        })?
    };

    // 根据 api_format 选择非流式转换器
    let anthropic_response = if api_format == "openai_responses" {
        transform_responses::responses_to_anthropic(upstream_response)
    } else {
        transform::openai_to_anthropic(upstream_response)
    }
    .map_err(|e| {
        log::error!("[Claude] 转换响应失败: {e}");
        e
    })?;

    // 记录使用量
    if let Some(usage) = TokenUsage::from_claude_response(&anthropic_response) {
        let model = anthropic_response
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or("unknown");
        let latency_ms = ctx.latency_ms();

        let request_model = ctx.request_model.clone();
        tokio::spawn({
            let state = state.clone();
            let provider_id = ctx.provider.id.clone();
            let model = model.to_string();
            let app_type_str = ctx.app_type_str;
            async move {
                log_usage(
                    &state,
                    &provider_id,
                    app_type_str,
                    &model,
                    &request_model,
                    usage,
                    latency_ms,
                    None,
                    false,
                    status.as_u16(),
                )
                .await;
            }
        });
    }

    // 构建响应
    let mut builder = axum::response::Response::builder().status(status);
    strip_entity_headers_for_rebuilt_body(&mut response_headers);
    strip_hop_by_hop_response_headers(&mut response_headers);

    for (key, value) in response_headers.iter() {
        builder = builder.header(key, value);
    }

    builder = builder.header("content-type", "application/json");

    let response_body = serde_json::to_vec(&anthropic_response).map_err(|e| {
        log::error!("[Claude] 序列化响应失败: {e}");
        ProxyError::TransformError(format!("Failed to serialize response: {e}"))
    })?;

    let body = axum::body::Body::from(response_body);
    builder.body(body).map_err(|e| {
        log::error!("[Claude] 构建响应失败: {e}");
        ProxyError::Internal(format!("Failed to build response: {e}"))
    })
}

// ============================================================================
// 使用量记录（保留用于 Claude 转换逻辑）
// ============================================================================

fn log_forward_error(
    state: &ProxyState,
    ctx: &RequestContext,
    is_streaming: bool,
    error: &ProxyError,
) {
    use super::usage::logger::UsageLogger;

    let logger = UsageLogger::new(&state.db);
    let status_code = map_proxy_error_to_status(error);
    let error_message = get_error_message(error);
    let request_id = uuid::Uuid::new_v4().to_string();

    if let Err(e) = logger.log_error_with_context(
        request_id,
        ctx.provider.id.clone(),
        ctx.app_type_str.to_string(),
        ctx.request_model.clone(),
        status_code,
        error_message,
        ctx.latency_ms(),
        is_streaming,
        Some(ctx.session_id.clone()),
        None,
    ) {
        log::warn!("记录失败请求日志失败: {e}");
    }
}

/// 记录请求使用量
#[allow(clippy::too_many_arguments)]
async fn log_usage(
    state: &ProxyState,
    provider_id: &str,
    app_type: &str,
    model: &str,
    request_model: &str,
    usage: TokenUsage,
    latency_ms: u64,
    first_token_ms: Option<u64>,
    is_streaming: bool,
    status_code: u16,
) {
    use super::usage::logger::UsageLogger;

    let logger = UsageLogger::new(&state.db);

    let (multiplier, pricing_model_source) =
        logger.resolve_pricing_config(provider_id, app_type).await;
    let pricing_model = if pricing_model_source == "request" {
        request_model
    } else {
        model
    };

    let request_id = usage.dedup_request_id();

    if let Err(e) = logger.log_with_calculation(
        request_id,
        provider_id.to_string(),
        app_type.to_string(),
        model.to_string(),
        request_model.to_string(),
        pricing_model.to_string(),
        usage,
        multiplier,
        latency_ms,
        first_token_ms,
        status_code,
        None,
        None, // provider_type
        is_streaming,
    ) {
        log::warn!("[USG-001] 记录使用量失败: {e}");
    }
}
