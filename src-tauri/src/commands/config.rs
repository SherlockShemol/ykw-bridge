#![allow(non_snake_case)]

use tauri::AppHandle;
use tauri_plugin_dialog::DialogExt;
use tauri_plugin_opener::OpenerExt;

use crate::app_config::AppType;
use crate::config::{self, get_claude_settings_path, ConfigStatus};
use crate::settings;

#[tauri::command]
pub async fn get_claude_config_status() -> Result<ConfigStatus, String> {
    Ok(config::get_claude_config_status())
}

fn parse_supported_config_app(app: &str) -> Result<AppType, String> {
    match app.trim().to_lowercase().as_str() {
        "claude" => Ok(AppType::Claude),
        "claude_desktop" | "claudedesktop" | "claude-desktop" => Ok(AppType::ClaudeDesktop),
        _ => Err(format!("配置仅支持 claude 或 claude_desktop，收到: {app}")),
    }
}

fn invalid_json_format_error(error: serde_json::Error) -> String {
    let lang = settings::get_settings()
        .language
        .unwrap_or_else(|| "zh".to_string());

    match lang.as_str() {
        "en" => format!("Invalid JSON format: {error}"),
        "ja" => format!("JSON形式が無効です: {error}"),
        _ => format!("无效的 JSON 格式: {error}"),
    }
}

fn validate_common_config_snippet(app_type: &str, snippet: &str) -> Result<(), String> {
    if snippet.trim().is_empty() {
        return Ok(());
    }

    match app_type {
        "claude" | "claude_desktop" => {
            serde_json::from_str::<serde_json::Value>(snippet)
                .map_err(invalid_json_format_error)?;
        }
        _ => {
            return Err(format!(
                "配置片段仅支持 claude 或 claude_desktop，收到: {app_type}"
            ))
        }
    }

    Ok(())
}

#[tauri::command]
pub async fn get_config_status(app: String) -> Result<ConfigStatus, String> {
    match parse_supported_config_app(&app)? {
        AppType::Claude => Ok(config::get_claude_config_status()),
        AppType::ClaudeDesktop => {
            let config_path = crate::claude_desktop_config::resolve_config_path();
            Ok(ConfigStatus {
                exists: config_path.exists(),
                path: crate::claude_desktop_config::resolve_profile_dir()
                    .to_string_lossy()
                    .to_string(),
            })
        }
    }
}

#[tauri::command]
pub async fn get_claude_code_config_path() -> Result<String, String> {
    Ok(get_claude_settings_path().to_string_lossy().to_string())
}

#[tauri::command]
pub async fn get_config_dir(app: String) -> Result<String, String> {
    let dir = match parse_supported_config_app(&app)? {
        AppType::Claude => config::get_claude_config_dir(),
        AppType::ClaudeDesktop => crate::claude_desktop_config::resolve_profile_dir(),
    };

    Ok(dir.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn open_config_folder(handle: AppHandle, app: String) -> Result<bool, String> {
    let config_dir = match parse_supported_config_app(&app)? {
        AppType::Claude => config::get_claude_config_dir(),
        AppType::ClaudeDesktop => crate::claude_desktop_config::resolve_profile_dir(),
    };

    if !config_dir.exists() {
        std::fs::create_dir_all(&config_dir).map_err(|e| format!("创建目录失败: {e}"))?;
    }

    handle
        .opener()
        .open_path(config_dir.to_string_lossy().to_string(), None::<String>)
        .map_err(|e| format!("打开文件夹失败: {e}"))?;

    Ok(true)
}

#[tauri::command]
pub async fn pick_directory(
    app: AppHandle,
    #[allow(non_snake_case)] defaultPath: Option<String>,
) -> Result<Option<String>, String> {
    let initial = defaultPath
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty());

    let result = tauri::async_runtime::spawn_blocking(move || {
        let mut builder = app.dialog().file();
        if let Some(path) = initial {
            builder = builder.set_directory(path);
        }
        builder.blocking_pick_folder()
    })
    .await
    .map_err(|e| format!("弹出目录选择器失败: {e}"))?;

    match result {
        Some(file_path) => {
            let resolved = file_path
                .simplified()
                .into_path()
                .map_err(|e| format!("解析选择的目录失败: {e}"))?;
            Ok(Some(resolved.to_string_lossy().to_string()))
        }
        None => Ok(None),
    }
}

#[tauri::command]
pub async fn get_app_config_path() -> Result<String, String> {
    let config_path = config::get_app_config_path();
    Ok(config_path.to_string_lossy().to_string())
}

#[tauri::command]
pub async fn open_app_config_folder(handle: AppHandle) -> Result<bool, String> {
    let config_dir = config::get_app_config_dir();

    if !config_dir.exists() {
        std::fs::create_dir_all(&config_dir).map_err(|e| format!("创建目录失败: {e}"))?;
    }

    handle
        .opener()
        .open_path(config_dir.to_string_lossy().to_string(), None::<String>)
        .map_err(|e| format!("打开文件夹失败: {e}"))?;

    Ok(true)
}

#[tauri::command]
pub async fn get_claude_common_config_snippet(
    state: tauri::State<'_, crate::store::AppState>,
) -> Result<Option<String>, String> {
    state
        .db
        .get_config_snippet("claude")
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_claude_common_config_snippet(
    snippet: String,
    state: tauri::State<'_, crate::store::AppState>,
) -> Result<(), String> {
    let is_cleared = snippet.trim().is_empty();

    if !snippet.trim().is_empty() {
        serde_json::from_str::<serde_json::Value>(&snippet).map_err(invalid_json_format_error)?;
    }

    let value = if is_cleared { None } else { Some(snippet) };

    state
        .db
        .set_config_snippet("claude", value)
        .map_err(|e| e.to_string())?;
    state
        .db
        .set_config_snippet_cleared("claude", is_cleared)
        .map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
pub async fn get_common_config_snippet(
    app_type: String,
    state: tauri::State<'_, crate::store::AppState>,
) -> Result<Option<String>, String> {
    let app = parse_supported_config_app(&app_type)?;
    state
        .db
        .get_config_snippet(app.provider_storage_str())
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn set_common_config_snippet(
    app_type: String,
    snippet: String,
    state: tauri::State<'_, crate::store::AppState>,
) -> Result<(), String> {
    let app = parse_supported_config_app(&app_type)?;
    let storage_app = app.provider_storage_str().to_string();
    let is_cleared = snippet.trim().is_empty();
    let old_snippet = state
        .db
        .get_config_snippet(&storage_app)
        .map_err(|e| e.to_string())?;

    validate_common_config_snippet(app.as_str(), &snippet)?;

    let value = if is_cleared { None } else { Some(snippet) };

    if let Some(legacy_snippet) = old_snippet
        .as_deref()
        .filter(|value| !value.trim().is_empty())
    {
        crate::services::provider::ProviderService::migrate_legacy_common_config_usage(
            state.inner(),
            app.clone(),
            legacy_snippet,
        )
        .map_err(|e| e.to_string())?;
    }

    state
        .db
        .set_config_snippet(&storage_app, value)
        .map_err(|e| e.to_string())?;
    state
        .db
        .set_config_snippet_cleared(&storage_app, is_cleared)
        .map_err(|e| e.to_string())?;

    crate::services::provider::ProviderService::sync_current_provider_for_app(state.inner(), app)
        .map_err(|e| e.to_string())?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{parse_supported_config_app, validate_common_config_snippet};
    use crate::app_config::AppType;

    #[test]
    fn validate_common_config_snippet_accepts_valid_claude_json() {
        validate_common_config_snippet("claude", r#"{ "includeCoAuthoredBy": false }"#)
            .expect("valid claude snippet should be accepted");
    }

    #[test]
    fn validate_common_config_snippet_rejects_invalid_claude_json() {
        let err = validate_common_config_snippet("claude", "{broken")
            .expect_err("invalid claude snippet should be rejected");
        assert!(
            err.contains("JSON") || err.contains("json") || err.contains("格式"),
            "expected JSON validation error, got {err}"
        );
    }

    #[test]
    fn parse_supported_config_app_rejects_removed_apps() {
        let err =
            parse_supported_config_app("openclaw").expect_err("removed app should be rejected");
        assert!(
            err.contains("仅支持 claude 或 claude_desktop"),
            "unexpected error message: {err}"
        );
    }

    #[test]
    fn parse_supported_config_app_accepts_claude_desktop() {
        assert_eq!(
            parse_supported_config_app("claude_desktop").expect("claude_desktop should parse"),
            AppType::ClaudeDesktop
        );
    }
}

#[tauri::command]
pub async fn extract_common_config_snippet(
    appType: String,
    settingsConfig: Option<String>,
    state: tauri::State<'_, crate::store::AppState>,
) -> Result<String, String> {
    let app = parse_supported_config_app(&appType)?;

    if let Some(settings_config) = settingsConfig.filter(|s| !s.trim().is_empty()) {
        let settings: serde_json::Value =
            serde_json::from_str(&settings_config).map_err(invalid_json_format_error)?;

        return crate::services::provider::ProviderService::extract_common_config_snippet_from_settings(
            app,
            &settings,
        )
        .map_err(|e| e.to_string());
    }

    crate::services::provider::ProviderService::extract_common_config_snippet(&state, app)
        .map_err(|e| e.to_string())
}
