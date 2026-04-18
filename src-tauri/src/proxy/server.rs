//! HTTP代理服务器
//!
//! 基于Axum的HTTP服务器，处理代理请求
//!
//! Uses a manual hyper HTTP/1.1 accept loop with `preserve_header_case(true)` so
//! that the original header-name casing from the CLI client is captured in a
//! `HeaderCaseMap` extension.  This map is later forwarded to the upstream via
//! the hyper-based HTTP client, producing wire-level header casing identical to
//! a direct (non-proxied) CLI request.

use super::{
    failover_switch::FailoverSwitchManager, handlers,
    local_session_title_watcher::LocalSessionTitleWatcher, log_codes::srv as log_srv,
    provider_router::ProviderRouter, types::*, ProxyError,
};
use crate::database::Database;
use axum::{
    extract::DefaultBodyLimit,
    routing::{get, post},
    Router,
};
use hyper_util::rt::TokioIo;
use rustls::ServerConfig as RustlsServerConfig;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::{oneshot, RwLock};
use tokio::task::JoinHandle;
use tokio_rustls::TlsAcceptor;

/// 代理服务器状态（共享）
#[derive(Clone)]
pub struct ProxyState {
    pub db: Arc<Database>,
    pub config: Arc<RwLock<ProxyConfig>>,
    pub status: Arc<RwLock<ProxyStatus>>,
    pub start_time: Arc<RwLock<Option<std::time::Instant>>>,
    /// 每个应用类型当前使用的 provider (app_type -> (provider_id, provider_name))
    pub current_providers: Arc<RwLock<std::collections::HashMap<String, (String, String)>>>,
    /// 共享的 ProviderRouter（持有熔断器状态，跨请求保持）
    pub provider_router: Arc<ProviderRouter>,
    /// AppHandle，用于发射事件和更新托盘菜单
    pub app_handle: Option<tauri::AppHandle>,
    /// 故障转移切换管理器
    pub failover_manager: Arc<FailoverSwitchManager>,
    /// Claude Desktop 本地会话标题 watcher
    pub local_session_title_watcher: Arc<LocalSessionTitleWatcher>,
}

/// 代理HTTP服务器
pub struct ProxyServer {
    config: ProxyConfig,
    state: ProxyState,
    shutdown_tx: Arc<RwLock<Option<oneshot::Sender<()>>>>,
    /// 服务器任务句柄，用于等待服务器实际关闭
    server_handle: Arc<RwLock<Option<JoinHandle<()>>>>,
    https_shutdown_tx: Arc<RwLock<Option<oneshot::Sender<()>>>>,
    https_server_handle: Arc<RwLock<Option<JoinHandle<()>>>>,
}

impl ProxyServer {
    pub fn new(
        config: ProxyConfig,
        db: Arc<Database>,
        app_handle: Option<tauri::AppHandle>,
    ) -> Self {
        // 创建共享的 ProviderRouter（熔断器状态将跨所有请求保持）
        let provider_router = Arc::new(ProviderRouter::new(db.clone()));
        // 创建故障转移切换管理器
        let failover_manager = Arc::new(FailoverSwitchManager::new(db.clone()));

        let state = ProxyState {
            db,
            config: Arc::new(RwLock::new(config.clone())),
            status: Arc::new(RwLock::new(ProxyStatus::default())),
            start_time: Arc::new(RwLock::new(None)),
            current_providers: Arc::new(RwLock::new(std::collections::HashMap::new())),
            provider_router,
            app_handle,
            failover_manager,
            local_session_title_watcher: Arc::new(LocalSessionTitleWatcher::new()),
        };

        Self {
            config,
            state,
            shutdown_tx: Arc::new(RwLock::new(None)),
            server_handle: Arc::new(RwLock::new(None)),
            https_shutdown_tx: Arc::new(RwLock::new(None)),
            https_server_handle: Arc::new(RwLock::new(None)),
        }
    }

    fn load_tls_acceptor() -> Result<TlsAcceptor, ProxyError> {
        crate::rustls_provider::ensure_rustls_crypto_provider();

        let cert_path = crate::claude_desktop_config::resolve_server_cert_path();
        let key_path = crate::claude_desktop_config::resolve_server_key_path();

        let mut cert_reader = std::io::BufReader::new(
            std::fs::File::open(&cert_path)
                .map_err(|e| ProxyError::BindFailed(format!("读取 TLS 证书失败: {e}")))?,
        );
        let certs = rustls_pemfile::certs(&mut cert_reader)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| ProxyError::BindFailed(format!("解析 TLS 证书失败: {e}")))?;

        let mut key_reader = std::io::BufReader::new(
            std::fs::File::open(&key_path)
                .map_err(|e| ProxyError::BindFailed(format!("读取 TLS 私钥失败: {e}")))?,
        );
        let key = rustls_pemfile::private_key(&mut key_reader)
            .map_err(|e| ProxyError::BindFailed(format!("解析 TLS 私钥失败: {e}")))?
            .ok_or_else(|| ProxyError::BindFailed("TLS 私钥为空".to_string()))?;

        let mut config = RustlsServerConfig::builder()
            .with_no_client_auth()
            .with_single_cert(certs, key)
            .map_err(|e| ProxyError::BindFailed(format!("构建 TLS 配置失败: {e}")))?;
        config.alpn_protocols = vec![b"http/1.1".to_vec()];

        Ok(TlsAcceptor::from(Arc::new(config)))
    }

    pub async fn start(&self) -> Result<ProxyServerInfo, ProxyError> {
        // 检查是否已在运行
        if self.shutdown_tx.read().await.is_some() {
            return Err(ProxyError::AlreadyRunning);
        }

        let addr: SocketAddr = crate::claude_desktop_config::format_socket_address(
            &self.config.listen_address,
            self.config.listen_port,
        )
        .parse()
        .map_err(|e| ProxyError::BindFailed(format!("无效的地址: {e}")))?;

        // 创建关闭通道
        let (shutdown_tx, shutdown_rx) = oneshot::channel();

        // 构建路由
        let app = self.build_router();
        let http_app = app.clone();
        let https_app = self.build_claude_desktop_https_router();

        // 绑定监听器
        let listener = TcpListener::bind(&addr)
            .await
            .map_err(|e| ProxyError::BindFailed(e.to_string()))?;

        log::info!("[{}] 代理服务器启动于 {addr}", log_srv::STARTED);

        // 更新全局代理端口，用于系统代理检测
        crate::proxy::http_client::set_proxy_port(self.config.listen_port);

        // 保存关闭句柄
        *self.shutdown_tx.write().await = Some(shutdown_tx);

        // 更新状态
        let mut status = self.state.status.write().await;
        status.running = true;
        status.address = self.config.listen_address.clone();
        status.port = self.config.listen_port;
        drop(status);

        // 记录启动时间
        *self.state.start_time.write().await = Some(std::time::Instant::now());

        // 启动服务器 — 使用手动 hyper HTTP/1.1 accept loop
        // 开启 preserve_header_case 以捕获客户端请求头的原始大小写
        let state = self.state.clone();
        let handle = tokio::spawn(async move {
            let mut shutdown_rx = shutdown_rx;
            loop {
                tokio::select! {
                    result = listener.accept() => {
                        let (stream, _remote_addr) = match result {
                            Ok(v) => v,
                            Err(e) => {
                                log::error!("[{SRV}] accept 失败: {e}", SRV = log_srv::ACCEPT_ERR);
                                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                                continue;
                            }
                        };

                        let app = http_app.clone();
                        tokio::spawn(async move {
                            // Peek raw TCP bytes to capture original header casing
                            // before hyper parses (and lowercases) the header names.
                            let original_cases = {
                                let mut peek_buf = vec![0u8; 8192];
                                match stream.peek(&mut peek_buf).await {
                                    Ok(n) => {
                                        let cases = super::hyper_client::OriginalHeaderCases::from_raw_bytes(&peek_buf[..n]);
                                        log::debug!(
                                            "[ProxyServer] Peeked {} bytes, captured {} header casings",
                                            n, cases.cases.len()
                                        );
                                        cases
                                    }
                                    Err(e) => {
                                        log::debug!("[ProxyServer] peek failed (non-fatal): {e}");
                                        super::hyper_client::OriginalHeaderCases::default()
                                    }
                                }
                            };

                            // service_fn 将 axum Router（tower::Service）桥接到 hyper
                            let service = hyper::service::service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
                                let mut router = app.clone();
                                let cases = original_cases.clone();
                                async move {
                                    // 将 hyper::body::Incoming 转为 axum::body::Body，保留 extensions
                                    let (mut parts, body) = req.into_parts();

                                    // Insert our own header case map alongside hyper's internal one
                                    parts.extensions.insert(cases);

                                    let body = axum::body::Body::new(body);
                                    let axum_req = http::Request::from_parts(parts, body);
                                    <Router as tower::Service<http::Request<axum::body::Body>>>::call(&mut router, axum_req).await
                                }
                            });

                            if let Err(e) = hyper::server::conn::http1::Builder::new()
                                .preserve_header_case(true)
                                .serve_connection(TokioIo::new(stream), service)
                                .await
                            {
                                // Connection reset / broken pipe 等在代理场景下很常见，debug 级别
                                log::debug!("[{SRV}] connection error: {e}", SRV = log_srv::CONN_ERR);
                            }
                        });
                    }
                    _ = &mut shutdown_rx => {
                        break;
                    }
                }
            }

            // 服务器停止后更新状态
            state.status.write().await.running = false;
            *state.start_time.write().await = None;
        });

        // 保存服务器任务句柄
        *self.server_handle.write().await = Some(handle);

        if crate::claude_desktop_config::cert_files_exist() {
            let https_port =
                crate::claude_desktop_config::https_port_for_proxy_port(self.config.listen_port);
            let https_bind_address = crate::claude_desktop_config::https_listener_bind_address(
                &self.config.listen_address,
            );
            let https_addr: SocketAddr = crate::claude_desktop_config::format_socket_address(
                &https_bind_address,
                https_port,
            )
            .parse()
            .map_err(|e| ProxyError::BindFailed(format!("无效的 HTTPS 地址: {e}")))?;
            let https_listener = TcpListener::bind(&https_addr)
                .await
                .map_err(|e| ProxyError::BindFailed(format!("HTTPS 绑定失败: {e}")))?;
            let tls_acceptor = Self::load_tls_acceptor()?;
            let (https_shutdown_tx, https_shutdown_rx) = oneshot::channel();
            *self.https_shutdown_tx.write().await = Some(https_shutdown_tx);

            let https_state = self.state.clone();
            let https_app = https_app.clone();
            let https_handle = tokio::spawn(async move {
                let mut https_shutdown_rx = https_shutdown_rx;
                loop {
                    tokio::select! {
                        result = https_listener.accept() => {
                            let (stream, _remote_addr) = match result {
                                Ok(v) => v,
                                Err(e) => {
                                    log::error!("[{SRV}] https accept 失败: {e}", SRV = log_srv::ACCEPT_ERR);
                                    tokio::time::sleep(std::time::Duration::from_millis(50)).await;
                                    continue;
                                }
                            };

                            let app = https_app.clone();
                            let acceptor = tls_acceptor.clone();
                            tokio::spawn(async move {
                                let tls_stream = match acceptor.accept(stream).await {
                                    Ok(stream) => stream,
                                    Err(e) => {
                                        log::debug!("[ProxyServer] TLS handshake failed: {e}");
                                        return;
                                    }
                                };

                                let service = hyper::service::service_fn(move |req: hyper::Request<hyper::body::Incoming>| {
                                    let mut router = app.clone();
                                    async move {
                                        let (parts, body) = req.into_parts();
                                        let body = axum::body::Body::new(body);
                                        let axum_req = http::Request::from_parts(parts, body);
                                        <Router as tower::Service<http::Request<axum::body::Body>>>::call(&mut router, axum_req).await
                                    }
                                });

                                if let Err(e) = hyper::server::conn::http1::Builder::new()
                                    .serve_connection(TokioIo::new(tls_stream), service)
                                    .await
                                {
                                    log::debug!("[{SRV}] tls connection error: {e}", SRV = log_srv::CONN_ERR);
                                }
                            });
                        }
                        _ = &mut https_shutdown_rx => {
                            break;
                        }
                    }
                }

                https_state.status.write().await.running = false;
                *https_state.start_time.write().await = None;
            });

            *self.https_server_handle.write().await = Some(https_handle);
            log::info!("[{}] HTTPS gateway 启动于 {https_addr}", log_srv::STARTED);
        } else {
            log::info!("Claude Desktop TLS 证书未配置，跳过 HTTPS gateway listener");
        }

        Ok(ProxyServerInfo {
            address: self.config.listen_address.clone(),
            port: self.config.listen_port,
            started_at: chrono::Utc::now().to_rfc3339(),
        })
    }

    pub async fn stop(&self) -> Result<(), ProxyError> {
        // 1. 发送关闭信号
        if let Some(tx) = self.shutdown_tx.write().await.take() {
            let _ = tx.send(());
        } else {
            return Err(ProxyError::NotRunning);
        }
        if let Some(tx) = self.https_shutdown_tx.write().await.take() {
            let _ = tx.send(());
        }

        // 2. 等待服务器任务结束（带 5 秒超时保护）
        let mut stop_error: Option<ProxyError> = None;

        if let Some(handle) = self.server_handle.write().await.take() {
            match tokio::time::timeout(std::time::Duration::from_secs(5), handle).await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    log::warn!("[{}] 代理服务器任务异常终止: {e}", log_srv::TASK_ERROR);
                    stop_error = Some(ProxyError::StopFailed(e.to_string()));
                }
                Err(_) => {
                    log::warn!(
                        "[{}] 代理服务器停止超时（5秒），强制继续",
                        log_srv::STOP_TIMEOUT
                    );
                    stop_error = Some(ProxyError::StopTimeout);
                }
            }
        }

        if let Some(handle) = self.https_server_handle.write().await.take() {
            match tokio::time::timeout(std::time::Duration::from_secs(5), handle).await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    log::warn!("[{}] HTTPS gateway 任务异常终止: {e}", log_srv::TASK_ERROR);
                    if stop_error.is_none() {
                        stop_error = Some(ProxyError::StopFailed(e.to_string()));
                    }
                }
                Err(_) => {
                    log::warn!(
                        "[{}] HTTPS gateway 停止超时（5秒），强制继续",
                        log_srv::STOP_TIMEOUT
                    );
                    if stop_error.is_none() {
                        stop_error = Some(ProxyError::StopTimeout);
                    }
                }
            }
        }

        if let Some(err) = stop_error {
            Err(err)
        } else {
            log::info!("[{}] 代理服务器已完全停止", log_srv::STOPPED);
            Ok(())
        }
    }

    pub async fn get_status(&self) -> ProxyStatus {
        let mut status = self.state.status.read().await.clone();

        // 计算运行时间
        if let Some(start) = *self.state.start_time.read().await {
            status.uptime_seconds = start.elapsed().as_secs();
        }

        // 从 current_providers HashMap 获取每个应用类型当前正在使用的 provider
        let current_providers = self.state.current_providers.read().await;
        status.active_targets = current_providers
            .iter()
            .map(|(app_type, (provider_id, provider_name))| ActiveTarget {
                app_type: app_type.clone(),
                provider_id: provider_id.clone(),
                provider_name: provider_name.clone(),
            })
            .collect();

        status
    }

    /// 更新某个应用类型当前“目标供应商”（用于 UI 展示 active_targets）
    ///
    /// 注意：这不代表该供应商一定已经处理过请求，而是用于“热切换/启用故障转移立即切 P1”
    /// 等场景下，让 UI 能立刻反映最新目标。
    pub async fn set_active_target(&self, app_type: &str, provider_id: &str, provider_name: &str) {
        let mut current_providers = self.state.current_providers.write().await;
        current_providers.insert(
            app_type.to_string(),
            (provider_id.to_string(), provider_name.to_string()),
        );
    }

    fn build_router(&self) -> Router {
        Router::new()
            // 健康检查
            .route("/health", get(handlers::health_check))
            .route("/status", get(handlers::get_status))
            // Claude API (支持带前缀和不带前缀两种格式)
            .route("/v1/messages", post(handlers::handle_messages))
            .route("/claude/v1/messages", post(handlers::handle_messages))
            .route(
                "/claude-desktop/v1/messages",
                post(handlers::handle_claude_desktop_messages),
            )
            .route(
                "/claude-desktop/v1/models",
                get(handlers::handle_claude_desktop_models),
            )
            .route(
                "/claude_desktop/v1/messages",
                post(handlers::handle_claude_desktop_messages),
            )
            .route(
                "/claude_desktop/v1/models",
                get(handlers::handle_claude_desktop_models),
            )
            .route(
                "/api/organizations/{org_id}/chat_conversations/{conversation_id}/title",
                post(handlers::handle_chat_conversation_title),
            )
            .route(
                "/api/organizations/{org_id}/dust/generate_session_title",
                post(handlers::handle_session_title),
            )
            // 提高默认请求体大小限制（避免 413 Payload Too Large）
            .layer(DefaultBodyLimit::max(200 * 1024 * 1024))
            .with_state(self.state.clone())
    }

    fn build_claude_desktop_https_router(&self) -> Router {
        Router::new()
            .route("/health", get(handlers::health_check))
            .route(
                "/claude-desktop/v1/messages",
                post(handlers::handle_claude_desktop_messages),
            )
            .route(
                "/claude-desktop/v1/models",
                get(handlers::handle_claude_desktop_models),
            )
            .route(
                "/claude_desktop/v1/messages",
                post(handlers::handle_claude_desktop_messages),
            )
            .route(
                "/claude_desktop/v1/models",
                get(handlers::handle_claude_desktop_models),
            )
            .route(
                "/api/organizations/{org_id}/chat_conversations/{conversation_id}/title",
                post(handlers::handle_chat_conversation_title),
            )
            .route(
                "/api/organizations/{org_id}/dust/generate_session_title",
                post(handlers::handle_session_title),
            )
            .layer(DefaultBodyLimit::max(200 * 1024 * 1024))
            .with_state(self.state.clone())
    }

    /// 在不重启服务的情况下更新运行时配置
    pub async fn apply_runtime_config(&self, config: &ProxyConfig) {
        *self.state.config.write().await = config.clone();
    }

    /// 热更新熔断器配置
    ///
    /// 将新配置应用到所有已创建的熔断器实例
    pub async fn update_circuit_breaker_configs(
        &self,
        config: super::circuit_breaker::CircuitBreakerConfig,
    ) {
        self.state.provider_router.update_all_configs(config).await;
    }

    /// 重置指定 Provider 的熔断器
    pub async fn reset_provider_circuit_breaker(&self, provider_id: &str, app_type: &str) {
        self.state
            .provider_router
            .reset_provider_breaker(provider_id, app_type)
            .await;
    }
}
