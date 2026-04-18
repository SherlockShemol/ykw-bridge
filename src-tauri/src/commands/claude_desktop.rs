#![allow(non_snake_case)]

use crate::app_config::AppType;
use crate::claude_desktop_config::{ClaudeDesktopDoctor, ClaudeDesktopStatus};
use crate::store::AppState;

async fn current_proxy_settings(state: &AppState) -> Result<(String, u16, bool), String> {
    let config = state
        .db
        .get_global_proxy_config()
        .await
        .map_err(|e| format!("获取代理配置失败: {e}"))?;
    let proxy_running = state.proxy_service.is_running().await;
    Ok((config.listen_address, config.listen_port, proxy_running))
}

#[tauri::command]
pub async fn get_claude_desktop_status(
    state: tauri::State<'_, AppState>,
) -> Result<ClaudeDesktopStatus, String> {
    let (listen_address, listen_port, proxy_running) =
        current_proxy_settings(state.inner()).await?;
    Ok(
        crate::claude_desktop_config::build_status(&listen_address, listen_port, proxy_running)
            .await,
    )
}

#[tauri::command]
pub async fn doctor_claude_desktop(
    state: tauri::State<'_, AppState>,
) -> Result<ClaudeDesktopDoctor, String> {
    let (listen_address, listen_port, proxy_running) =
        current_proxy_settings(state.inner()).await?;
    Ok(
        crate::claude_desktop_config::build_doctor(&listen_address, listen_port, proxy_running)
            .await,
    )
}

#[tauri::command]
pub async fn install_claude_desktop_certificate(
    state: tauri::State<'_, AppState>,
) -> Result<ClaudeDesktopStatus, String> {
    crate::claude_desktop_config::install_certificate().map_err(|e| e.to_string())?;
    get_claude_desktop_status(state).await
}

#[tauri::command]
pub async fn install_claude_desktop_launch_shim(
    state: tauri::State<'_, AppState>,
) -> Result<ClaudeDesktopStatus, String> {
    crate::claude_desktop_config::install_launch_shim().map_err(|e| e.to_string())?;
    get_claude_desktop_status(state).await
}

#[tauri::command]
pub async fn remove_claude_desktop_launch_shim(
    state: tauri::State<'_, AppState>,
) -> Result<ClaudeDesktopStatus, String> {
    crate::claude_desktop_config::remove_launch_shim().map_err(|e| e.to_string())?;
    get_claude_desktop_status(state).await
}

#[tauri::command]
pub async fn launch_claude_desktop(
    state: tauri::State<'_, AppState>,
) -> Result<ClaudeDesktopStatus, String> {
    let doctor = doctor_claude_desktop(state.clone()).await?;
    if !doctor.blockers.is_empty() {
        return Err(format!(
            "Claude Desktop 启动前检查未通过: {}",
            doctor.blockers.join("；")
        ));
    }

    crate::services::provider::ProviderService::sync_current_provider_for_app(
        state.inner(),
        AppType::ClaudeDesktop,
    )
    .map_err(|e| format!("同步 Claude Desktop 配置失败: {e}"))?;

    state
        .proxy_service
        .set_takeover_for_app(AppType::ClaudeDesktop.as_str(), true)
        .await?;

    crate::claude_desktop_config::launch_app().map_err(|e| e.to_string())?;
    get_claude_desktop_status(state).await
}
