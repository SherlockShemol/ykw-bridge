use super::{handlers::run_targeted_session_title_sync, server::ProxyState};
use crate::claude_desktop_config::resolve_profile_dir;
use crate::session_links::{self, SessionLinkIdentityInput};
use notify::{Config, Event, RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tokio::sync::mpsc;

const LOCAL_AGENT_MODE_SESSIONS_DIRNAME: &str = "local-agent-mode-sessions";
const CLAUDE_CODE_SESSIONS_DIRNAME: &str = "claude-code-sessions";
const PENDING_LOCAL_SESSION_TITLE_SYNC_MAX_AGE_MS: u64 = 5 * 60 * 1000;

#[derive(Clone, Default)]
pub(crate) struct LocalSessionTitleWatcher {
    inner: Arc<LocalSessionTitleWatcherInner>,
}

#[derive(Default)]
struct LocalSessionTitleWatcherInner {
    started: AtomicBool,
    pending: Mutex<HashMap<String, PendingSessionTitleSync>>,
}

#[derive(Clone)]
struct PendingSessionTitleSync {
    identity: SessionLinkIdentityInput,
    registered_at_ms: u64,
}

pub(crate) fn build_session_title_sync_key(identity: &SessionLinkIdentityInput) -> String {
    let remote_session_id = identity.remote_session_id.trim();
    if !remote_session_id.is_empty() {
        return session_links::sync_key(remote_session_id);
    }
    let prompt_hash = identity
        .initial_prompt_hash
        .as_deref()
        .unwrap_or_default()
        .trim();
    if !prompt_hash.is_empty() {
        return session_links::sync_key(prompt_hash);
    }
    let prompt = identity
        .initial_prompt
        .as_deref()
        .unwrap_or_default()
        .trim();
    if prompt.is_empty() {
        session_links::sync_key("")
    } else {
        session_links::sync_key(prompt)
    }
}

impl LocalSessionTitleWatcher {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn register_pending_sync(
        &self,
        state: ProxyState,
        identity: SessionLinkIdentityInput,
    ) {
        if identity.remote_session_id.trim().is_empty()
            && identity
                .initial_prompt
                .as_deref()
                .map(str::trim)
                .unwrap_or_default()
                .is_empty()
        {
            return;
        }

        self.ensure_started(state);

        let key = build_session_title_sync_key(&identity);
        let pending = PendingSessionTitleSync {
            identity,
            registered_at_ms: current_unix_time_ms(),
        };

        let mut entries = self
            .inner
            .pending
            .lock()
            .expect("local session title watcher lock poisoned");
        entries.insert(key, pending);
    }

    pub(crate) fn clear_pending_sync(&self, identity: &SessionLinkIdentityInput) {
        let key = build_session_title_sync_key(identity);
        let mut entries = self
            .inner
            .pending
            .lock()
            .expect("local session title watcher lock poisoned");
        entries.remove(&key);
    }

    fn ensure_started(&self, state: ProxyState) {
        if self.inner.started.swap(true, Ordering::SeqCst) {
            return;
        }

        let watcher = self.clone();
        tokio::spawn(async move {
            if let Err(err) = watcher.run(state).await {
                log::warn!("Claude Desktop 本地会话 watcher 已停止: {err}");
                watcher.inner.started.store(false, Ordering::SeqCst);
            }
        });
    }

    async fn run(&self, state: ProxyState) -> Result<(), String> {
        let roots = watched_session_roots()
            .map_err(|e| format!("Failed to prepare watched session roots: {e}"))?;
        let (tx, mut rx) = mpsc::unbounded_channel();
        let mut watcher = RecommendedWatcher::new(
            move |result| {
                let _ = tx.send(result);
            },
            Config::default(),
        )
        .map_err(|e| format!("Failed to create file watcher: {e}"))?;

        for root in &roots {
            watcher
                .watch(root, RecursiveMode::Recursive)
                .map_err(|e| format!("Failed to watch {}: {e}", root.display()))?;
        }

        log::info!(
            "Claude Desktop 本地会话 watcher 已启动: {}",
            roots
                .iter()
                .map(|path| path.display().to_string())
                .collect::<Vec<_>>()
                .join(", ")
        );

        while let Some(result) = rx.recv().await {
            match result {
                Ok(event) => {
                    if event_has_relevant_session_path(&event) {
                        self.process_pending_syncs(&state).await;
                    }
                }
                Err(err) => {
                    log::debug!("Claude Desktop 本地会话 watcher 事件异常: {err}");
                }
            }
        }

        Ok(())
    }

    async fn process_pending_syncs(&self, state: &ProxyState) {
        let pending_entries = {
            let mut entries = self
                .inner
                .pending
                .lock()
                .expect("local session title watcher lock poisoned");
            let now_ms = current_unix_time_ms();
            entries.retain(|_, entry| {
                now_ms.saturating_sub(entry.registered_at_ms)
                    <= PENDING_LOCAL_SESSION_TITLE_SYNC_MAX_AGE_MS
            });
            entries.values().cloned().collect::<Vec<_>>()
        };

        if pending_entries.is_empty() {
            return;
        }

        for entry in pending_entries {
            match run_targeted_session_title_sync(state.clone(), entry.identity.clone()).await {
                Ok(true) => {
                    self.clear_pending_sync(&entry.identity);
                }
                Ok(false) => {}
                Err(err) => {
                    log::debug!(
                        "Claude Desktop 本地会话 watcher 标题同步失败: sessionId={}, error={}",
                        entry.identity.remote_session_id,
                        err
                    );
                }
            }
        }
    }
}

fn watched_session_roots() -> std::io::Result<Vec<PathBuf>> {
    let profile_dir = resolve_profile_dir();
    let roots = vec![
        profile_dir.join(LOCAL_AGENT_MODE_SESSIONS_DIRNAME),
        profile_dir.join(CLAUDE_CODE_SESSIONS_DIRNAME),
    ];

    for root in &roots {
        std::fs::create_dir_all(root)?;
    }

    Ok(roots)
}

fn event_has_relevant_session_path(event: &Event) -> bool {
    event
        .paths
        .iter()
        .any(|path| is_relevant_session_json_path(path))
}

fn is_relevant_session_json_path(path: &Path) -> bool {
    is_local_session_json_path(path)
        && (path_contains_component(path, LOCAL_AGENT_MODE_SESSIONS_DIRNAME)
            || path_contains_component(path, CLAUDE_CODE_SESSIONS_DIRNAME))
}

fn is_local_session_json_path(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("json"))
        .unwrap_or(false)
        && path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .map(|stem| stem.starts_with("local_"))
            .unwrap_or(false)
}

fn path_contains_component(path: &Path, component: &str) -> bool {
    path.components()
        .any(|segment| segment.as_os_str() == component)
}

fn current_unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::{build_session_title_sync_key, is_relevant_session_json_path};
    use crate::session_links::build_identity_input;
    use std::path::Path;

    #[test]
    fn session_title_sync_key_prefers_remote_session_id() {
        assert_eq!(
            build_session_title_sync_key(&build_identity_input(
                "abc".to_string(),
                Some("hello".to_string()),
                Some("hash".to_string()),
                Some("hello".to_string())
            )),
            crate::session_links::sync_key("abc")
        );
        assert_eq!(
            build_session_title_sync_key(&build_identity_input(
                "".to_string(),
                Some("hello".to_string()),
                Some("hash".to_string()),
                Some("hello".to_string())
            )),
            crate::session_links::sync_key("hash")
        );
    }

    #[test]
    fn relevant_session_path_matches_cowork_and_code_json() {
        assert!(is_relevant_session_json_path(Path::new(
            "/tmp/local-agent-mode-sessions/a/local_1.json"
        )));
        assert!(is_relevant_session_json_path(Path::new(
            "/tmp/claude-code-sessions/local_2.json"
        )));
        assert!(!is_relevant_session_json_path(Path::new(
            "/tmp/claude-code-sessions/not_local.json"
        )));
    }
}
