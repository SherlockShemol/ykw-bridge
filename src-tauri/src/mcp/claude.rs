//! Claude MCP 同步和导入模块

use serde_json::{Map, Value};
use std::collections::HashMap;

use crate::app_config::{McpApps, McpConfig, McpServer, MultiAppConfig};
use crate::error::AppError;

use super::validation::{extract_server_spec, validate_server_spec};

fn should_sync_claude_mcp() -> bool {
    // Claude 未安装/未初始化时：通常 ~/.claude 目录与 ~/.claude.json 都不存在。
    // 按用户偏好：此时跳过写入/删除，不创建任何文件或目录。
    crate::config::get_claude_config_dir().exists() || crate::config::get_claude_mcp_path().exists()
}

/// 返回已启用的 MCP 服务器（过滤 enabled==true）
fn collect_enabled_servers(cfg: &McpConfig) -> HashMap<String, Value> {
    let mut out = HashMap::new();
    for (id, entry) in cfg.servers.iter() {
        let enabled = entry
            .get("enabled")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);
        if !enabled {
            continue;
        }
        match extract_server_spec(entry) {
            Ok(spec) => {
                out.insert(id.clone(), spec);
            }
            Err(err) => {
                log::warn!("跳过无效的 MCP 条目 '{id}': {err}");
            }
        }
    }
    out
}

/// 将 config.json 中 enabled==true 的项投影写入 ~/.claude.json
pub fn sync_enabled_to_claude(config: &MultiAppConfig) -> Result<(), AppError> {
    if !should_sync_claude_mcp() {
        return Ok(());
    }
    let enabled = collect_enabled_servers(&config.mcp.claude);
    crate::claude_mcp::set_mcp_servers_map(&enabled)
}

/// 从 ~/.claude.json 导入 mcpServers 到统一结构（v3.7.0+）
/// 已存在的服务器将启用 Claude 应用，不覆盖其他字段和应用状态
pub fn import_from_claude(config: &mut MultiAppConfig) -> Result<usize, AppError> {
    let text_opt = crate::claude_mcp::read_mcp_json()?;
    let Some(text) = text_opt else { return Ok(0) };

    let v: Value = serde_json::from_str(&text)
        .map_err(|e| AppError::McpValidation(format!("解析 ~/.claude.json 失败: {e}")))?;
    let Some(map) = v.get("mcpServers").and_then(|x| x.as_object()) else {
        return Ok(0);
    };

    // 确保新结构存在
    let servers = config.mcp.servers.get_or_insert_with(HashMap::new);

    let mut changed = 0;
    let mut errors = Vec::new();

    for (id, spec) in map.iter() {
        // 校验：单项失败不中止，收集错误继续处理
        if let Err(e) = validate_server_spec(spec) {
            log::warn!("跳过无效 MCP 服务器 '{id}': {e}");
            errors.push(format!("{id}: {e}"));
            continue;
        }

        if let Some(existing) = servers.get_mut(id) {
            // 已存在：仅启用 Claude 应用
            if !existing.apps.claude {
                existing.apps.claude = true;
                changed += 1;
                log::info!("MCP 服务器 '{id}' 已启用 Claude 应用");
            }
        } else {
            // 新建服务器：默认仅启用 Claude
            servers.insert(
                id.clone(),
                McpServer {
                    id: id.clone(),
                    name: id.clone(),
                    server: spec.clone(),
                    apps: McpApps {
                        claude: true,
                        codex: false,
                        gemini: false,
                    },
                    description: None,
                    homepage: None,
                    docs: None,
                    tags: Vec::new(),
                },
            );
            changed += 1;
            log::info!("导入新 MCP 服务器 '{id}'");
        }
    }

    if !errors.is_empty() {
        log::warn!("导入完成，但有 {} 项失败: {:?}", errors.len(), errors);
    }

    Ok(changed)
}

fn desktop_mcp_servers_map_from_config(config: &Value) -> HashMap<String, Value> {
    config
        .get("mcpServers")
        .and_then(|v| v.as_object())
        .map(|obj| obj.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
        .unwrap_or_default()
}

fn sanitize_mcp_servers_map(
    servers: &HashMap<String, Value>,
) -> Result<Map<String, Value>, AppError> {
    let mut out = Map::new();
    for (id, spec) in servers {
        let mut obj = if let Some(map) = spec.as_object() {
            map.clone()
        } else {
            return Err(AppError::McpValidation(format!(
                "MCP 服务器 '{id}' 不是对象"
            )));
        };

        if let Some(server_val) = obj.remove("server") {
            let server_obj = server_val.as_object().cloned().ok_or_else(|| {
                AppError::McpValidation(format!("MCP 服务器 '{id}' server 字段不是对象"))
            })?;
            obj = server_obj;
        }

        obj.remove("enabled");
        obj.remove("source");
        obj.remove("id");
        obj.remove("name");
        obj.remove("description");
        obj.remove("tags");
        obj.remove("homepage");
        obj.remove("docs");

        out.insert(id.clone(), Value::Object(obj));
    }

    Ok(out)
}

pub fn read_mcp_servers_map_for_app(
    app: &crate::app_config::AppType,
) -> Result<HashMap<String, Value>, AppError> {
    match app {
        crate::app_config::AppType::Claude => crate::claude_mcp::read_mcp_servers_map(),
        crate::app_config::AppType::ClaudeDesktop => {
            let config = crate::claude_desktop_config::read_live_config()
                .unwrap_or_else(|_| serde_json::json!({}));
            Ok(desktop_mcp_servers_map_from_config(&config))
        }
    }
}

pub fn set_mcp_servers_map_for_app(
    app: &crate::app_config::AppType,
    servers: &HashMap<String, Value>,
) -> Result<(), AppError> {
    match app {
        crate::app_config::AppType::Claude => crate::claude_mcp::set_mcp_servers_map(servers),
        crate::app_config::AppType::ClaudeDesktop => {
            let mut config = crate::claude_desktop_config::read_live_config()
                .unwrap_or_else(|_| serde_json::json!({}));
            let obj = config.as_object_mut().ok_or_else(|| {
                AppError::Config("Claude Desktop live config root must be an object".into())
            })?;
            obj.insert(
                "mcpServers".into(),
                Value::Object(sanitize_mcp_servers_map(servers)?),
            );
            crate::claude_desktop_config::write_live_config(&config)
        }
    }
}

/// 将单个 MCP 服务器同步到指定应用的 live 配置
pub fn sync_single_server_to_app(
    _config: &MultiAppConfig,
    app: &crate::app_config::AppType,
    id: &str,
    server_spec: &Value,
) -> Result<(), AppError> {
    if matches!(app, crate::app_config::AppType::Claude) && !should_sync_claude_mcp() {
        return Ok(());
    }

    let mut updated = read_mcp_servers_map_for_app(app)?;
    updated.insert(id.to_string(), server_spec.clone());
    set_mcp_servers_map_for_app(app, &updated)
}

/// 从指定应用的 live 配置中移除单个 MCP 服务器
pub fn remove_server_from_app(app: &crate::app_config::AppType, id: &str) -> Result<(), AppError> {
    if matches!(app, crate::app_config::AppType::Claude) && !should_sync_claude_mcp() {
        return Ok(());
    }

    let mut current = read_mcp_servers_map_for_app(app)?;
    current.remove(id);
    set_mcp_servers_map_for_app(app, &current)
}

/// 将单个 MCP 服务器同步到 Claude live 配置
pub fn sync_single_server_to_claude(
    config: &MultiAppConfig,
    id: &str,
    server_spec: &Value,
) -> Result<(), AppError> {
    sync_single_server_to_app(config, &crate::app_config::AppType::Claude, id, server_spec)
}

/// 从 Claude live 配置中移除单个 MCP 服务器
pub fn remove_server_from_claude(id: &str) -> Result<(), AppError> {
    remove_server_from_app(&crate::app_config::AppType::Claude, id)
}
