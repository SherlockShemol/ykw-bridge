use std::collections::{BTreeSet, HashMap};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::net::IpAddr;
use std::net::Ipv4Addr;
use std::net::TcpListener;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use serde_json::{json, Value};

use crate::config::{get_app_config_dir, read_json_file, write_json_file};
use crate::error::AppError;
use crate::provider::Provider;

const DEFAULT_APP_PATH: &str = "/Applications/Claude.app";
const APP_BUNDLE_BINARY_RELATIVE_PATH: &str = "Contents/MacOS/Claude";
const CONFIG_FILENAME: &str = "claude_desktop_config.json";
const CERT_DIRNAME: &str = "certs";
const SERVER_CERT_FILENAME: &str = "server.pem";
const SERVER_KEY_FILENAME: &str = "server-key.pem";
const CERT_COMMON_NAME: &str = "CC Switch Claude Desktop Local Gateway";
const LOCAL_SESSION_TITLE_MAX_CHARS: usize = 80;
const LOCAL_SESSION_TITLE_SOURCE_AUTO: &str = "auto";
const LOCAL_SESSION_TITLE_SOURCE_PROMPT: &str = "prompt";
const LOCAL_SESSION_TITLE_RECENT_FALLBACK_MAX_AGE_MS: u64 = 5 * 60 * 1000;
const LAUNCH_SHIM_MARKER: &str = "# cc-switch-claude-launch-shim";
const LAUNCH_SHIM_BACKUP_SUFFIX: &str = ".cc-switch-original";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LocalSessionTitleLookupPreference {
    CoworkFirst,
    CodeFirst,
}

#[derive(Debug, Clone)]
pub enum LocalSessionTitleLookup {
    NotFound,
    AlreadyTitled {
        path: PathBuf,
        kind: &'static str,
    },
    Pending {
        path: PathBuf,
        kind: &'static str,
        description: Option<String>,
    },
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeDesktopStatus {
    pub supported: bool,
    pub experimental: bool,
    pub app_path: String,
    pub app_exists: bool,
    pub binary_path: String,
    pub binary_exists: bool,
    pub profile_dir: String,
    pub config_path: String,
    pub certificate_installed: bool,
    pub certificate_path: String,
    pub key_path: String,
    pub gateway_base_url: Option<String>,
    pub managed_config_exists: bool,
    pub launch_shim_installed: bool,
    pub launch_shim_recovery_available: bool,
    pub proxy_running: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClaudeDesktopDoctor {
    pub status: ClaudeDesktopStatus,
    pub gateway_healthy: bool,
    pub http_port_available: bool,
    pub https_port_available: bool,
    pub blockers: Vec<String>,
}

pub fn default_app_path() -> PathBuf {
    PathBuf::from(DEFAULT_APP_PATH)
}

pub fn resolve_app_path() -> PathBuf {
    crate::settings::get_claude_desktop_app_path().unwrap_or_else(default_app_path)
}

pub fn resolve_binary_path() -> PathBuf {
    resolve_app_path().join(APP_BUNDLE_BINARY_RELATIVE_PATH)
}

fn launch_shim_backup_path_for(binary_path: &Path) -> PathBuf {
    let file_name = binary_path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("Claude");
    binary_path.with_file_name(format!("{file_name}{LAUNCH_SHIM_BACKUP_SUFFIX}"))
}

pub fn default_profile_dir() -> PathBuf {
    get_app_config_dir().join("claude-desktop").join("profile")
}

pub fn resolve_profile_dir() -> PathBuf {
    crate::settings::get_claude_desktop_profile_dir().unwrap_or_else(default_profile_dir)
}

pub fn resolve_config_path() -> PathBuf {
    resolve_profile_dir().join(CONFIG_FILENAME)
}

fn local_agent_sessions_dir(profile_dir: &Path) -> PathBuf {
    profile_dir.join("local-agent-mode-sessions")
}

fn claude_code_sessions_dir(profile_dir: &Path) -> PathBuf {
    profile_dir.join("claude-code-sessions")
}

fn default_claude_projects_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .map(|home| home.join(".claude").join("projects"))
}

pub fn resolve_cert_dir() -> PathBuf {
    get_app_config_dir()
        .join("claude-desktop")
        .join(CERT_DIRNAME)
}

pub fn resolve_server_cert_path() -> PathBuf {
    resolve_cert_dir().join(SERVER_CERT_FILENAME)
}

pub fn resolve_server_key_path() -> PathBuf {
    resolve_cert_dir().join(SERVER_KEY_FILENAME)
}

fn is_local_session_json(path: &Path) -> bool {
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

fn collect_local_session_json_paths(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), AppError> {
    if !dir.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(dir).map_err(|e| AppError::io(dir, e))? {
        let entry = entry.map_err(|e| AppError::io(dir, e))?;
        let path = entry.path();
        if path.is_dir() {
            collect_local_session_json_paths(&path, out)?;
        } else if is_local_session_json(&path) {
            out.push(path);
        }
    }

    Ok(())
}

fn truncate_local_session_title(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.is_empty() {
        return String::new();
    }
    if trimmed.chars().count() <= LOCAL_SESSION_TITLE_MAX_CHARS {
        return trimmed.to_string();
    }

    let mut result = trimmed
        .chars()
        .take(LOCAL_SESSION_TITLE_MAX_CHARS)
        .collect::<String>();
    result.push_str("...");
    result
}

fn collapse_local_session_text(raw: &str) -> Option<String> {
    let collapsed = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if collapsed.is_empty() {
        None
    } else {
        Some(collapsed)
    }
}

fn extract_local_session_text_from_value(value: &Value) -> Option<String> {
    match value {
        Value::String(raw) => collapse_local_session_text(raw),
        Value::Array(items) => {
            let text = items
                .iter()
                .filter_map(|item| {
                    if item.get("type").and_then(Value::as_str) == Some("text") {
                        item.get("text").and_then(Value::as_str)
                    } else {
                        None
                    }
                })
                .collect::<Vec<_>>()
                .join(" ");
            collapse_local_session_text(&text)
        }
        _ => None,
    }
}

#[cfg(test)]
fn derive_local_session_title_from_value(value: &Value) -> Option<String> {
    extract_local_session_text_from_value(value)
        .map(|text| truncate_local_session_title(&text))
        .filter(|title| !title.is_empty())
}

#[cfg(test)]
fn derive_local_session_title(doc: &Value) -> Option<String> {
    derive_local_session_title_from_value(doc.get("initialMessage")?)
}

fn derive_local_session_description(doc: &Value) -> Option<String> {
    extract_local_session_text_from_value(doc.get("initialMessage")?)
}

fn session_doc_title(doc: &Value) -> Option<&str> {
    doc.get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn session_doc_title_source(doc: &Value) -> Option<&str> {
    doc.get("titleSource")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
}

fn session_doc_has_title_metadata(doc: &Value) -> (bool, bool) {
    let has_title = session_doc_title(doc).is_some();
    let has_title_source = session_doc_title_source(doc).is_some();
    (has_title, has_title_source)
}

fn session_doc_id(doc: &Value, path: &Path) -> String {
    doc.get("sessionId")
        .and_then(Value::as_str)
        .unwrap_or_else(|| {
            path.file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or("unknown-session")
        })
        .to_string()
}

fn session_doc_matches_request_session_id(
    doc: &Value,
    path: &Path,
    request_session_id: &str,
) -> bool {
    let request_session_id = request_session_id.trim();
    if request_session_id.is_empty() {
        return false;
    }

    if session_doc_id(doc, path) == request_session_id {
        return true;
    }

    doc.get("cliSessionId")
        .and_then(Value::as_str)
        .map(|value| value == request_session_id)
        .unwrap_or(false)
}

fn session_lookup_from_doc(
    path: PathBuf,
    kind: &'static str,
    doc: &Value,
    description: Option<String>,
) -> LocalSessionTitleLookup {
    let (has_title, _) = session_doc_has_title_metadata(doc);
    if has_title {
        LocalSessionTitleLookup::AlreadyTitled { path, kind }
    } else {
        LocalSessionTitleLookup::Pending {
            path,
            kind,
            description,
        }
    }
}

#[derive(Debug, Clone)]
struct RecentPendingSessionCandidate {
    path: PathBuf,
    kind: &'static str,
    description: String,
    activity_ms: u64,
    preference_rank: u8,
}

fn current_unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

fn path_modified_at_ms(path: &Path) -> Option<u64> {
    fs::metadata(path)
        .ok()
        .and_then(|metadata| metadata.modified().ok())
        .and_then(|modified| modified.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_millis() as u64)
}

fn session_doc_activity_ms(doc: &Value, path: &Path) -> Option<u64> {
    doc.get("lastActivityAt")
        .and_then(Value::as_u64)
        .or_else(|| doc.get("createdAt").and_then(Value::as_u64))
        .or_else(|| path_modified_at_ms(path))
}

fn recent_pending_session_candidate_from_doc(
    path: PathBuf,
    kind: &'static str,
    doc: &Value,
    description: Option<String>,
    preference_rank: u8,
) -> Option<RecentPendingSessionCandidate> {
    if session_doc_title(doc).is_some() {
        return None;
    }

    let description = description
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())?;
    let activity_ms = session_doc_activity_ms(doc, &path)?;

    Some(RecentPendingSessionCandidate {
        path,
        kind,
        description,
        activity_ms,
        preference_rank,
    })
}

fn write_title_metadata(
    path: &Path,
    doc: &mut Value,
    title: &str,
    source: &str,
) -> Result<bool, AppError> {
    let normalized = truncate_local_session_title(title);
    if normalized.is_empty() {
        return Ok(false);
    }

    let Some(obj) = doc.as_object_mut() else {
        return Ok(false);
    };

    let current_title = obj
        .get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());
    let current_source = obj
        .get("titleSource")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());

    if current_title == Some(normalized.as_str()) && current_source == Some(source) {
        return Ok(false);
    }

    obj.insert("title".to_string(), Value::String(normalized));
    obj.insert("titleSource".to_string(), Value::String(source.to_string()));

    write_json_file(path, doc)?;
    Ok(true)
}

fn lookup_cowork_session_title_target_in_profile(
    profile_dir: &Path,
    request_session_id: &str,
    prompt: &str,
) -> Result<LocalSessionTitleLookup, AppError> {
    let mut session_paths = Vec::new();
    collect_local_session_json_paths(&local_agent_sessions_dir(profile_dir), &mut session_paths)?;

    let prompt = prompt.trim();

    for path in session_paths {
        let doc = match read_json_file::<Value>(&path) {
            Ok(value) => value,
            Err(err) => {
                log::debug!(
                    "跳过无法读取的 Claude Desktop Cowork 会话文件 {}: {}",
                    path.display(),
                    err
                );
                continue;
            }
        };

        let description = derive_local_session_description(&doc);
        let matches_session =
            session_doc_matches_request_session_id(&doc, &path, request_session_id);
        let matches_prompt = !prompt.is_empty() && description.as_deref() == Some(prompt);
        if !matches_session && !matches_prompt {
            continue;
        }

        return Ok(session_lookup_from_doc(path, "cowork", &doc, description));
    }

    Ok(LocalSessionTitleLookup::NotFound)
}

fn collect_claude_project_transcript_index(
    dir: &Path,
    out: &mut HashMap<String, PathBuf>,
) -> Result<(), AppError> {
    if !dir.exists() {
        return Ok(());
    }

    for entry in fs::read_dir(dir).map_err(|e| AppError::io(dir, e))? {
        let entry = entry.map_err(|e| AppError::io(dir, e))?;
        let path = entry.path();
        if path.is_dir() {
            collect_claude_project_transcript_index(&path, out)?;
        } else if path
            .extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| ext.eq_ignore_ascii_case("jsonl"))
            .unwrap_or(false)
        {
            if let Some(file_name) = path.file_name().and_then(|name| name.to_str()) {
                out.entry(file_name.to_string()).or_insert(path);
            }
        }
    }

    Ok(())
}

fn derive_claude_code_description_from_transcript(path: &Path) -> Result<Option<String>, AppError> {
    let file = fs::File::open(path).map_err(|e| AppError::io(path, e))?;
    let reader = BufReader::new(file);
    let mut fallback = None;

    for line in reader.lines() {
        let line = line.map_err(|e| AppError::io(path, e))?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let Ok(doc) = serde_json::from_str::<Value>(trimmed) else {
            continue;
        };

        let event_type = doc.get("type").and_then(Value::as_str);
        if event_type == Some("user") {
            let Some(message) = doc.get("message") else {
                continue;
            };
            if message.get("role").and_then(Value::as_str) != Some("user") {
                continue;
            }
            if let Some(description) = extract_local_session_text_from_value(
                message.get("content").unwrap_or(&Value::Null),
            ) {
                return Ok(Some(description));
            }
        }

        if fallback.is_none()
            && event_type == Some("queue-operation")
            && doc.get("operation").and_then(Value::as_str) == Some("enqueue")
        {
            fallback =
                extract_local_session_text_from_value(doc.get("content").unwrap_or(&Value::Null));
        }

        if fallback.is_none() && event_type == Some("last-prompt") {
            fallback =
                extract_local_session_text_from_value(doc.get("prompt").unwrap_or(&Value::Null));
        }
    }

    Ok(fallback)
}

fn derive_claude_code_session_description(
    doc: &Value,
    transcript_index: &HashMap<String, PathBuf>,
) -> Result<Option<String>, AppError> {
    if let Some(description) = derive_local_session_description(doc) {
        return Ok(Some(description));
    }

    let Some(cli_session_id) = doc.get("cliSessionId").and_then(Value::as_str) else {
        return Ok(None);
    };
    let transcript_name = format!("{cli_session_id}.jsonl");
    let Some(transcript_path) = transcript_index.get(&transcript_name) else {
        return Ok(None);
    };

    derive_claude_code_description_from_transcript(transcript_path)
}

fn lookup_claude_code_session_title_target_in_profile(
    profile_dir: &Path,
    projects_dir: &Path,
    request_session_id: &str,
    prompt: &str,
) -> Result<LocalSessionTitleLookup, AppError> {
    let mut session_paths = Vec::new();
    collect_local_session_json_paths(&claude_code_sessions_dir(profile_dir), &mut session_paths)?;
    if session_paths.is_empty() {
        return Ok(LocalSessionTitleLookup::NotFound);
    }

    let prompt = prompt.trim();
    let mut matched_session = None;
    let mut docs = Vec::new();

    for path in session_paths {
        let doc = match read_json_file::<Value>(&path) {
            Ok(value) => value,
            Err(err) => {
                log::debug!(
                    "跳过无法读取的 Claude Desktop Code 会话文件 {}: {}",
                    path.display(),
                    err
                );
                continue;
            }
        };

        let description = derive_local_session_description(&doc);
        let matches_session =
            session_doc_matches_request_session_id(&doc, &path, request_session_id);
        if matches_session && (session_doc_title(&doc).is_some() || description.is_some()) {
            return Ok(session_lookup_from_doc(path, "code", &doc, description));
        }

        if !prompt.is_empty() && description.as_deref() == Some(prompt) {
            return Ok(session_lookup_from_doc(path, "code", &doc, description));
        }

        if matches_session && matched_session.is_none() {
            matched_session = Some((path.clone(), doc.clone(), description.clone()));
        }

        docs.push((path, doc, description));
    }

    if prompt.is_empty() && matched_session.is_none() {
        return Ok(LocalSessionTitleLookup::NotFound);
    }

    let mut transcript_index = HashMap::new();
    collect_claude_project_transcript_index(projects_dir, &mut transcript_index)?;
    if !transcript_index.is_empty() {
        if let Some((path, doc, description)) = matched_session.take() {
            let description = if description.is_some() {
                description
            } else {
                derive_claude_code_session_description(&doc, &transcript_index)?
            };
            if description.is_some() {
                return Ok(session_lookup_from_doc(path, "code", &doc, description));
            }
            matched_session = Some((path, doc, description));
        }

        for (path, doc, description) in docs {
            let description = if description.is_some() {
                description
            } else {
                derive_claude_code_session_description(&doc, &transcript_index)?
            };
            if !prompt.is_empty() && description.as_deref() == Some(prompt) {
                return Ok(session_lookup_from_doc(path, "code", &doc, description));
            }
        }
    }

    if let Some((path, doc, description)) = matched_session {
        return Ok(session_lookup_from_doc(path, "code", &doc, description));
    }

    Ok(LocalSessionTitleLookup::NotFound)
}

fn lookup_recent_local_session_title_target(
    profile_dir: &Path,
    projects_dir: Option<&Path>,
    preference: LocalSessionTitleLookupPreference,
) -> Result<LocalSessionTitleLookup, AppError> {
    let now_ms = current_unix_time_ms();
    let cutoff_ms = now_ms.saturating_sub(LOCAL_SESSION_TITLE_RECENT_FALLBACK_MAX_AGE_MS);
    let (cowork_rank, code_rank) = match preference {
        LocalSessionTitleLookupPreference::CoworkFirst => (0, 1),
        LocalSessionTitleLookupPreference::CodeFirst => (1, 0),
    };
    let mut candidates = Vec::new();

    let mut cowork_paths = Vec::new();
    collect_local_session_json_paths(&local_agent_sessions_dir(profile_dir), &mut cowork_paths)?;
    for path in cowork_paths {
        let doc = match read_json_file::<Value>(&path) {
            Ok(value) => value,
            Err(_) => continue,
        };
        let Some(candidate) = recent_pending_session_candidate_from_doc(
            path,
            "cowork",
            &doc,
            derive_local_session_description(&doc),
            cowork_rank,
        ) else {
            continue;
        };
        if candidate.activity_ms >= cutoff_ms {
            candidates.push(candidate);
        }
    }

    if let Some(projects_dir) = projects_dir {
        let mut code_paths = Vec::new();
        collect_local_session_json_paths(&claude_code_sessions_dir(profile_dir), &mut code_paths)?;
        let mut transcript_index = HashMap::new();
        collect_claude_project_transcript_index(projects_dir, &mut transcript_index)?;

        for path in code_paths {
            let doc = match read_json_file::<Value>(&path) {
                Ok(value) => value,
                Err(_) => continue,
            };
            let description = if transcript_index.is_empty() {
                derive_local_session_description(&doc)
            } else {
                derive_claude_code_session_description(&doc, &transcript_index)?
            };
            let Some(candidate) = recent_pending_session_candidate_from_doc(
                path,
                "code",
                &doc,
                description,
                code_rank,
            ) else {
                continue;
            };
            if candidate.activity_ms >= cutoff_ms {
                candidates.push(candidate);
            }
        }
    }

    candidates.sort_by(|left, right| {
        right
            .activity_ms
            .cmp(&left.activity_ms)
            .then_with(|| left.preference_rank.cmp(&right.preference_rank))
    });

    let Some(candidate) = candidates.into_iter().next() else {
        return Ok(LocalSessionTitleLookup::NotFound);
    };

    Ok(LocalSessionTitleLookup::Pending {
        path: candidate.path,
        kind: candidate.kind,
        description: Some(candidate.description),
    })
}

pub fn lookup_local_session_title_target(
    request_session_id: &str,
    prompt: &str,
    preference: LocalSessionTitleLookupPreference,
) -> Result<LocalSessionTitleLookup, AppError> {
    let profile_dir = resolve_profile_dir();
    let projects_dir = default_claude_projects_dir();

    let lookup_code = || -> Result<LocalSessionTitleLookup, AppError> {
        let Some(projects_dir) = projects_dir.as_deref() else {
            return Ok(LocalSessionTitleLookup::NotFound);
        };
        lookup_claude_code_session_title_target_in_profile(
            &profile_dir,
            projects_dir,
            request_session_id,
            prompt,
        )
    };

    let lookup_cowork = || -> Result<LocalSessionTitleLookup, AppError> {
        lookup_cowork_session_title_target_in_profile(&profile_dir, request_session_id, prompt)
    };

    let direct = match preference {
        LocalSessionTitleLookupPreference::CoworkFirst => match lookup_cowork()? {
            LocalSessionTitleLookup::NotFound => lookup_code()?,
            found => found,
        },
        LocalSessionTitleLookupPreference::CodeFirst => match lookup_code()? {
            LocalSessionTitleLookup::NotFound => lookup_cowork()?,
            found => found,
        },
    };

    if !matches!(direct, LocalSessionTitleLookup::NotFound) {
        return Ok(direct);
    }

    lookup_recent_local_session_title_target(&profile_dir, projects_dir.as_deref(), preference)
}

pub fn persist_prompt_session_title(path: &Path, prompt: &str) -> Result<bool, AppError> {
    let mut doc = read_json_file::<Value>(path)?;
    if session_doc_title(&doc).is_some() {
        return Ok(false);
    }

    let Some(prompt_title) = collapse_local_session_text(prompt)
        .map(|text| truncate_local_session_title(&text))
        .filter(|title| !title.is_empty())
    else {
        return Ok(false);
    };

    write_title_metadata(
        path,
        &mut doc,
        &prompt_title,
        LOCAL_SESSION_TITLE_SOURCE_PROMPT,
    )
}

pub fn replace_prompt_session_title(path: &Path, title: &str) -> Result<bool, AppError> {
    let mut doc = read_json_file::<Value>(path)?;
    if session_doc_title(&doc).is_some()
        && session_doc_title_source(&doc) != Some(LOCAL_SESSION_TITLE_SOURCE_PROMPT)
    {
        return Ok(false);
    }

    write_title_metadata(path, &mut doc, title, LOCAL_SESSION_TITLE_SOURCE_AUTO)
}

fn profile_lock_paths() -> [PathBuf; 2] {
    let profile_dir = resolve_profile_dir();
    [
        profile_dir
            .join("IndexedDB")
            .join("app_localhost_0.indexeddb.leveldb")
            .join("LOCK"),
        profile_dir
            .join("Local Storage")
            .join("leveldb")
            .join("LOCK"),
    ]
}

fn managed_profile_in_use() -> bool {
    profile_lock_paths().iter().any(|lock_path| {
        lock_path.exists()
            && Command::new("lsof")
                .arg(lock_path)
                .output()
                .map(|output| output.status.success() && !output.stdout.is_empty())
                .unwrap_or(false)
    })
}

fn activate_claude_app() -> Result<(), AppError> {
    let status = Command::new("osascript")
        .arg("-e")
        .arg(r#"tell application "Claude" to activate"#)
        .status()
        .map_err(|e| AppError::Message(format!("激活 Claude Desktop 失败: {e}")))?;

    if !status.success() {
        return Err(AppError::Message(
            "激活已运行的 Claude Desktop 失败".to_string(),
        ));
    }

    Ok(())
}

fn shell_single_quote(raw: &str) -> String {
    format!("'{}'", raw.replace('\'', r#"'"'"'"#))
}

#[cfg_attr(target_os = "macos", allow(dead_code))]
fn build_launch_shim_script(backup_path: &Path, profile_dir: &Path) -> String {
    let backup = shell_single_quote(&backup_path.to_string_lossy());
    let profile = shell_single_quote(&profile_dir.to_string_lossy());
    format!(
        "#!/bin/zsh\n{LAUNCH_SHIM_MARKER}\nexport CLAUDE_USER_DATA_DIR={profile}\nexec {backup} -3p \"$@\"\n"
    )
}

fn binary_contains_launch_shim(binary_path: &Path) -> bool {
    fs::read_to_string(binary_path)
        .map(|content| content.contains(LAUNCH_SHIM_MARKER))
        .unwrap_or(false)
}

fn launch_shim_installed_for(binary_path: &Path) -> bool {
    binary_path.exists()
        && launch_shim_backup_path_for(binary_path).exists()
        && binary_contains_launch_shim(binary_path)
}

pub fn is_launch_shim_installed() -> bool {
    launch_shim_installed_for(&resolve_binary_path())
}

fn launch_shim_recovery_available_for(binary_path: &Path) -> bool {
    let backup_path = launch_shim_backup_path_for(binary_path);
    backup_path.exists() && (!binary_path.exists() || binary_contains_launch_shim(binary_path))
}

#[cfg(target_os = "macos")]
fn escape_osascript(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn is_launch_shim_permission_denied(binary_path: &Path, err: &AppError) -> bool {
    matches!(
        err,
        AppError::Io { path, source }
            if source.kind() == std::io::ErrorKind::PermissionDenied
                && (path == &binary_path.display().to_string()
                    || path
                        == &launch_shim_backup_path_for(binary_path)
                            .display()
                            .to_string())
    )
}

#[cfg_attr(target_os = "macos", allow(dead_code))]
fn build_privileged_install_launch_shim_script(
    app_path: &Path,
    binary_path: &Path,
    backup_path: &Path,
    profile_dir: &Path,
) -> String {
    let app = shell_single_quote(&app_path.to_string_lossy());
    let binary = shell_single_quote(&binary_path.to_string_lossy());
    let backup = shell_single_quote(&backup_path.to_string_lossy());
    let marker = shell_single_quote(LAUNCH_SHIM_MARKER);
    let shim = build_launch_shim_script(backup_path, profile_dir);
    let damaged = shell_single_quote("检测到损坏的 Claude launch shim：缺少原始二进制备份");
    format!(
        "#!/bin/zsh
set -euo pipefail

app={app}
binary={binary}
backup={backup}

if [ ! -e \"$binary\" ]; then
  echo \"未找到 Claude Desktop 可执行文件: $binary\" >&2
  exit 1
fi

binary_has_shim=0
if /usr/bin/grep -aFq -- {marker} \"$binary\" 2>/dev/null; then
  binary_has_shim=1
fi

if [ \"$binary_has_shim\" -eq 1 ] && [ ! -e \"$backup\" ]; then
  echo {damaged} >&2
  exit 1
fi

shim_installed=0
if [ \"$binary_has_shim\" -eq 1 ] && [ -e \"$backup\" ]; then
  shim_installed=1
fi

if [ \"$shim_installed\" -eq 0 ]; then
  if [ -e \"$backup\" ]; then
    rm -f \"$backup\"
  fi
  mv \"$binary\" \"$backup\"
fi

cat > \"$binary\" <<'CC_SWITCH_SHIM'
{shim}CC_SWITCH_SHIM
chmod 755 \"$binary\"
/usr/bin/codesign --force --deep --sign - \"$app\"
/usr/bin/xattr -dr com.apple.quarantine \"$app\" >/dev/null 2>&1 || true
"
    )
}

fn build_privileged_remove_launch_shim_script(
    app_path: &Path,
    binary_path: &Path,
    backup_path: &Path,
) -> String {
    let app = shell_single_quote(&app_path.to_string_lossy());
    let binary = shell_single_quote(&binary_path.to_string_lossy());
    let backup = shell_single_quote(&backup_path.to_string_lossy());
    let marker = shell_single_quote(LAUNCH_SHIM_MARKER);
    let damaged = shell_single_quote("检测到损坏的 Claude launch shim：缺少原始二进制备份");
    format!(
        "#!/bin/zsh
set -euo pipefail

app={app}
binary={binary}
backup={backup}

binary_has_shim=0
if [ -e \"$binary\" ] && /usr/bin/grep -aFq -- {marker} \"$binary\" 2>/dev/null; then
  binary_has_shim=1
fi

if [ \"$binary_has_shim\" -eq 1 ] && [ ! -e \"$backup\" ]; then
  echo {damaged} >&2
  exit 1
fi

if [ \"$binary_has_shim\" -eq 1 ]; then
  rm -f \"$binary\"
fi

if [ -e \"$backup\" ]; then
  if [ -e \"$binary\" ]; then
    rm -f \"$binary\"
  fi
  mv \"$backup\" \"$binary\"
fi

/usr/bin/codesign --force --deep --sign - \"$app\"
/usr/bin/xattr -dr com.apple.quarantine \"$app\" >/dev/null 2>&1 || true
"
    )
}

#[cfg(target_os = "macos")]
fn run_privileged_launch_shim_script(script: &str) -> Result<(), AppError> {
    let mut temp = tempfile::NamedTempFile::new().map_err(|e| AppError::IoContext {
        context: "创建临时管理员脚本失败".to_string(),
        source: e,
    })?;
    temp.write_all(script.as_bytes())
        .map_err(|e| AppError::io(temp.path(), e))?;
    temp.flush().map_err(|e| AppError::io(temp.path(), e))?;

    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(temp.path())
            .map_err(|e| AppError::io(temp.path(), e))?
            .permissions();
        permissions.set_mode(0o700);
        fs::set_permissions(temp.path(), permissions).map_err(|e| AppError::io(temp.path(), e))?;
    }

    let command = format!(
        "/bin/zsh {}",
        shell_single_quote(&temp.path().to_string_lossy())
    );
    let output = Command::new("/usr/bin/osascript")
        .arg("-e")
        .arg(format!(
            "do shell script \"{}\" with administrator privileges",
            escape_osascript(&command)
        ))
        .output()
        .map_err(|e| AppError::Message(format!("请求管理员权限失败: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if !stderr.is_empty() {
            stderr
        } else if !stdout.is_empty() {
            stdout
        } else {
            format!("exit code {:?}", output.status.code())
        };
        if detail.contains("User canceled")
            || detail.contains("用户已取消")
            || detail.contains("(-128)")
        {
            return Err(AppError::Message(
                "已取消管理员授权，Claude launch shim 未安装".to_string(),
            ));
        }
        if detail.contains("Operation not permitted") {
            return Err(AppError::Message(
                "macOS 仍阻止修改 Claude.app。请到 系统设置 > 隐私与安全性 > App Management，允许 cc-switch（以及你用来启动它的终端，如果是 dev 模式），然后重试 Install Launch Shim。".to_string(),
            ));
        }
        return Err(AppError::Message(format!(
            "管理员权限安装 Claude launch shim 失败: {detail}"
        )));
    }

    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn run_privileged_launch_shim_script(_script: &str) -> Result<(), AppError> {
    Ok(())
}

#[cfg(target_os = "macos")]
fn resign_app_bundle(app_path: &Path) -> Result<(), AppError> {
    let output = Command::new("/usr/bin/codesign")
        .arg("--force")
        .arg("--deep")
        .arg("--sign")
        .arg("-")
        .arg(app_path)
        .output()
        .map_err(|e| {
            AppError::Message(format!(
                "Claude launch shim 已写入，但无法调用 macOS codesign 完成重签名: {e}. \
请先完全退出 Claude，再在终端执行: sudo codesign --force --deep --sign - \"{}\"",
                app_path.display()
            ))
        })?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let detail = if !stderr.is_empty() {
            stderr
        } else if !stdout.is_empty() {
            stdout
        } else {
            format!("exit code {:?}", output.status.code())
        };
        return Err(AppError::Message(format!(
            "Claude launch shim 已写入，但 macOS 重签名失败: {detail}. \
请先完全退出 Claude，再在终端执行: sudo codesign --force --deep --sign - \"{}\"",
            app_path.display()
        )));
    }

    let _ = Command::new("/usr/bin/xattr")
        .arg("-dr")
        .arg("com.apple.quarantine")
        .arg(app_path)
        .output();

    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn resign_app_bundle(_app_path: &Path) -> Result<(), AppError> {
    Ok(())
}

fn map_launch_shim_error(binary_path: &Path, app_path: &Path, err: AppError) -> AppError {
    match err {
        AppError::Io { path, source }
            if source.kind() == std::io::ErrorKind::PermissionDenied
                && (path == binary_path.display().to_string()
                    || path
                        == launch_shim_backup_path_for(binary_path)
                            .display()
                            .to_string()) =>
        {
            AppError::Message(format!(
                "无法修改 Claude 可执行文件，macOS 拒绝了写入: {path}. \
请先完全退出 Claude，再重试 Install Launch Shim。若仍失败，请在终端用管理员权限执行，随后运行: sudo codesign --force --deep --sign - \"{}\"",
                app_path.display()
            ))
        }
        other => other,
    }
}

#[cfg_attr(target_os = "macos", allow(dead_code))]
fn install_launch_shim_for(binary_path: &Path, profile_dir: &Path) -> Result<(), AppError> {
    if !binary_path.exists() {
        return Err(AppError::Message(format!(
            "未找到 Claude Desktop 可执行文件: {}",
            binary_path.display()
        )));
    }

    let backup_path = launch_shim_backup_path_for(binary_path);
    if binary_contains_launch_shim(binary_path) && !backup_path.exists() {
        return Err(AppError::Message(
            "检测到损坏的 Claude launch shim：缺少原始二进制备份".to_string(),
        ));
    }
    let shim_installed = launch_shim_installed_for(binary_path);

    if !shim_installed {
        if backup_path.exists() {
            fs::remove_file(&backup_path).map_err(|e| AppError::io(&backup_path, e))?;
        }
        fs::rename(binary_path, &backup_path).map_err(|e| AppError::io(binary_path, e))?;
    }

    let script = build_launch_shim_script(&backup_path, profile_dir);
    fs::write(binary_path, script).map_err(|e| AppError::io(binary_path, e))?;

    #[cfg(unix)]
    {
        let mut permissions = fs::metadata(binary_path)
            .map_err(|e| AppError::io(binary_path, e))?
            .permissions();
        permissions.set_mode(0o755);
        fs::set_permissions(binary_path, permissions).map_err(|e| AppError::io(binary_path, e))?;
    }

    Ok(())
}

#[cfg(target_os = "macos")]
pub fn install_launch_shim() -> Result<(), AppError> {
    Err(AppError::Message(
        "Direct Launch Shim 已暂时禁用：Claude 当前会把被替换的主可执行文件判定为 Invalid installation。请先使用 Launch，后续需要换成不修改 Claude.app 的方案。".to_string(),
    ))
}

#[cfg(not(target_os = "macos"))]
pub fn install_launch_shim() -> Result<(), AppError> {
    let app_path = resolve_app_path();
    let binary_path = app_path.join(APP_BUNDLE_BINARY_RELATIVE_PATH);
    let backup_path = launch_shim_backup_path_for(&binary_path);
    let profile_dir = resolve_profile_dir();
    std::fs::create_dir_all(&profile_dir).map_err(|e| AppError::io(&profile_dir, e))?;
    match install_launch_shim_for(&binary_path, &profile_dir) {
        Ok(()) => {}
        Err(err)
            if cfg!(target_os = "macos")
                && is_launch_shim_permission_denied(&binary_path, &err) =>
        {
            let script = build_privileged_install_launch_shim_script(
                &app_path,
                &binary_path,
                &backup_path,
                &profile_dir,
            );
            return run_privileged_launch_shim_script(&script);
        }
        Err(err) => return Err(map_launch_shim_error(&binary_path, &app_path, err)),
    }
    resign_app_bundle(&app_path)
}

fn remove_launch_shim_for(binary_path: &Path) -> Result<(), AppError> {
    let backup_path = launch_shim_backup_path_for(binary_path);
    let shim_installed = launch_shim_installed_for(binary_path);

    if shim_installed {
        fs::remove_file(binary_path).map_err(|e| AppError::io(binary_path, e))?;
    }

    if backup_path.exists() {
        if binary_path.exists() {
            fs::remove_file(binary_path).map_err(|e| AppError::io(binary_path, e))?;
        }
        fs::rename(&backup_path, binary_path).map_err(|e| AppError::io(&backup_path, e))?;
    }

    Ok(())
}

pub fn remove_launch_shim() -> Result<(), AppError> {
    let app_path = resolve_app_path();
    let binary_path = app_path.join(APP_BUNDLE_BINARY_RELATIVE_PATH);
    let backup_path = launch_shim_backup_path_for(&binary_path);
    match remove_launch_shim_for(&binary_path) {
        Ok(()) => {}
        Err(err)
            if cfg!(target_os = "macos")
                && is_launch_shim_permission_denied(&binary_path, &err) =>
        {
            let script =
                build_privileged_remove_launch_shim_script(&app_path, &binary_path, &backup_path);
            return run_privileged_launch_shim_script(&script);
        }
        Err(err) => return Err(map_launch_shim_error(&binary_path, &app_path, err)),
    }
    resign_app_bundle(&app_path)
}

pub fn https_port_for_proxy_port(proxy_port: u16) -> u16 {
    proxy_port.saturating_add(1)
}

fn normalize_host(host: &str) -> String {
    host.trim_matches(|c| c == '[' || c == ']').to_string()
}

fn format_socket_host(host: &str) -> String {
    let normalized = normalize_host(host);
    if normalized.contains(':') && !normalized.starts_with('[') {
        format!("[{normalized}]")
    } else {
        normalized
    }
}

pub fn format_socket_address(host: &str, port: u16) -> String {
    format!("{}:{port}", format_socket_host(host))
}

fn is_wildcard_listen_address(listen_address: &str) -> bool {
    matches!(listen_address.trim(), "" | "0.0.0.0" | "::" | "[::]")
}

fn is_loopback_listen_address(listen_address: &str) -> bool {
    matches!(
        listen_address.trim(),
        "127.0.0.1" | "localhost" | "::1" | "[::1]"
    )
}

pub fn loopback_host_for_listen_address(listen_address: &str) -> String {
    match listen_address.trim() {
        "" | "0.0.0.0" | "::" | "[::]" => "127.0.0.1".to_string(),
        other => normalize_host(other),
    }
}

fn is_candidate_ipv4(addr: Ipv4Addr) -> bool {
    let octets = addr.octets();
    !addr.is_loopback()
        && !addr.is_link_local()
        && !addr.is_unspecified()
        && !addr.is_multicast()
        && !(octets[0] == 198 && matches!(octets[1], 18 | 19))
}

#[cfg(not(test))]
fn parse_first_non_loopback_ipv4(output: &str) -> Option<String> {
    for line in output.lines() {
        let trimmed = line.trim();
        let Some(rest) = trimmed.strip_prefix("inet ") else {
            continue;
        };
        let Some(raw_ip) = rest.split_whitespace().next() else {
            continue;
        };
        let Ok(ip) = raw_ip.parse::<Ipv4Addr>() else {
            continue;
        };
        if is_candidate_ipv4(ip) {
            return Some(ip.to_string());
        }
    }

    None
}

#[cfg(not(test))]
fn parse_default_route_interface(output: &str) -> Option<String> {
    output.lines().find_map(|line| {
        line.trim()
            .strip_prefix("interface:")
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(ToOwned::to_owned)
    })
}

fn interface_priority(name: &str) -> usize {
    if name.starts_with("en") || name.starts_with("eth") || name.starts_with("wlan") {
        0
    } else if name.starts_with("bridge") {
        4
    } else if name.starts_with("utun") || name.starts_with("tun") {
        5
    } else if name.starts_with("awdl") || name.starts_with("llw") {
        6
    } else if name.starts_with("lo") || name.starts_with("gif") || name.starts_with("stf") {
        7
    } else {
        2
    }
}

fn parse_non_loopback_ipv4_candidates_from_ifconfig(output: &str) -> Vec<String> {
    let mut entries: Vec<(bool, usize, String)> = Vec::new();
    let mut current_iface = String::new();
    let mut current_active = false;
    let mut current_ips: Vec<String> = Vec::new();

    let mut flush = |iface: &str, active: bool, ips: &mut Vec<String>| {
        if iface.is_empty() {
            ips.clear();
            return;
        }
        let priority = interface_priority(iface);
        for ip in ips.drain(..) {
            entries.push((active, priority, ip));
        }
    };

    for line in output.lines().chain(std::iter::once("")) {
        let trimmed = line.trim();
        let is_iface_header =
            !line.starts_with(' ') && !line.starts_with('\t') && line.contains(':');

        if is_iface_header || trimmed.is_empty() {
            flush(&current_iface, current_active, &mut current_ips);
            current_active = false;
            current_iface.clear();

            if is_iface_header {
                current_iface = line
                    .split_once(':')
                    .map(|(name, _)| name.trim().to_string())
                    .unwrap_or_default();
            }
        }

        if trimmed.eq_ignore_ascii_case("status: active") {
            current_active = true;
            continue;
        }

        let Some(rest) = trimmed.strip_prefix("inet ") else {
            continue;
        };
        let Some(raw_ip) = rest.split_whitespace().next() else {
            continue;
        };
        let Ok(ip) = raw_ip.parse::<Ipv4Addr>() else {
            continue;
        };
        if is_candidate_ipv4(ip) {
            current_ips.push(ip.to_string());
        }
    }

    entries.sort_by(|left, right| {
        right
            .0
            .cmp(&left.0)
            .then(left.1.cmp(&right.1))
            .then(left.2.cmp(&right.2))
    });

    let mut seen = BTreeSet::new();
    let mut result = Vec::new();
    for (_, _, ip) in entries {
        if seen.insert(ip.clone()) {
            result.push(ip);
        }
    }
    result
}

fn detected_non_loopback_ipv4s() -> Vec<String> {
    let output = Command::new("ifconfig").output();
    match output {
        Ok(output) if output.status.success() => parse_non_loopback_ipv4_candidates_from_ifconfig(
            &String::from_utf8_lossy(&output.stdout),
        ),
        _ => Vec::new(),
    }
}

#[cfg(not(test))]
fn detect_primary_non_loopback_ipv4() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        if let Ok(route_output) = Command::new("route")
            .arg("-n")
            .arg("get")
            .arg("default")
            .output()
        {
            if route_output.status.success() {
                let route_text = String::from_utf8_lossy(&route_output.stdout);
                if let Some(interface) = parse_default_route_interface(&route_text) {
                    if let Ok(ifconfig_output) = Command::new("ifconfig").arg(&interface).output() {
                        if ifconfig_output.status.success() {
                            if let Some(ip) = parse_first_non_loopback_ipv4(
                                &String::from_utf8_lossy(&ifconfig_output.stdout),
                            ) {
                                return Some(ip);
                            }
                        }
                    }
                }
            }
        }
    }

    detected_non_loopback_ipv4s().into_iter().next()
}

fn preferred_gateway_host_for_candidates(
    listen_address: &str,
    detected_ipv4s: &[String],
) -> String {
    if !is_wildcard_listen_address(listen_address) && !is_loopback_listen_address(listen_address) {
        return normalize_host(listen_address);
    }

    detected_ipv4s
        .first()
        .cloned()
        .unwrap_or_else(|| loopback_host_for_listen_address(listen_address))
}

pub fn preferred_gateway_host(listen_address: &str) -> String {
    #[cfg(test)]
    {
        loopback_host_for_listen_address(listen_address)
    }

    #[cfg(not(test))]
    {
        if let Some(ip) = detect_primary_non_loopback_ipv4() {
            return preferred_gateway_host_for_candidates(listen_address, &[ip]);
        }

        preferred_gateway_host_for_candidates(listen_address, &[])
    }
}

fn gateway_certificate_hosts_for_candidates(detected_ipv4s: &[String]) -> Vec<String> {
    let mut hosts = vec![
        "localhost".to_string(),
        "127.0.0.1".to_string(),
        "::1".to_string(),
    ];
    hosts.extend(detected_ipv4s.iter().cloned());
    hosts.sort();
    hosts.dedup();
    hosts
}

fn gateway_certificate_hosts() -> Vec<String> {
    gateway_certificate_hosts_for_candidates(&detected_non_loopback_ipv4s())
}

fn gateway_https_origin_for_host(host: &str, proxy_port: u16) -> String {
    format!(
        "https://{}:{}",
        format_socket_host(host),
        https_port_for_proxy_port(proxy_port)
    )
}

fn gateway_base_url_for_host(host: &str, proxy_port: u16) -> String {
    format!(
        "{}{}",
        gateway_https_origin_for_host(host, proxy_port),
        "/claude-desktop"
    )
}

fn gateway_health_url_for_host(host: &str, proxy_port: u16) -> String {
    format!(
        "{}{}",
        gateway_https_origin_for_host(host, proxy_port),
        "/health"
    )
}

pub fn gateway_base_url(listen_address: &str, proxy_port: u16) -> String {
    gateway_base_url_for_host(&preferred_gateway_host(listen_address), proxy_port)
}

fn local_gateway_health_host_for_bind_address(bind_address: &str) -> String {
    match bind_address.trim() {
        "" | "0.0.0.0" => "127.0.0.1".to_string(),
        "::" | "[::]" => "::1".to_string(),
        other => normalize_host(other),
    }
}

fn local_gateway_health_url(listen_address: &str, proxy_port: u16) -> String {
    let https_bind_address = https_listener_bind_address(listen_address);
    gateway_health_url_for_host(
        &local_gateway_health_host_for_bind_address(&https_bind_address),
        proxy_port,
    )
}

fn https_listener_bind_address_for_host(listen_address: &str, gateway_host: &str) -> String {
    if is_wildcard_listen_address(listen_address) {
        return match listen_address.trim() {
            "::" | "[::]" => "::".to_string(),
            _ => "0.0.0.0".to_string(),
        };
    }

    if is_loopback_listen_address(listen_address) {
        if gateway_host != "127.0.0.1" && !gateway_host.eq_ignore_ascii_case("localhost") {
            return "0.0.0.0".to_string();
        }
        return loopback_host_for_listen_address(listen_address);
    }

    normalize_host(listen_address)
}

pub fn https_listener_bind_address(listen_address: &str) -> String {
    let gateway_host = preferred_gateway_host(listen_address);
    https_listener_bind_address_for_host(listen_address, &gateway_host)
}

fn subject_alt_name_argument(hosts: &[String]) -> String {
    hosts
        .iter()
        .map(|host| {
            if host.parse::<IpAddr>().is_ok() {
                format!("IP:{host}")
            } else {
                format!("DNS:{host}")
            }
        })
        .collect::<Vec<_>>()
        .join(",")
}

fn openssl_ip_address_repr(host: &str) -> Option<String> {
    match host.parse::<IpAddr>().ok()? {
        IpAddr::V4(ip) => Some(ip.to_string()),
        IpAddr::V6(ip) => Some(
            ip.segments()
                .iter()
                .map(|segment| format!("{segment:x}"))
                .collect::<Vec<_>>()
                .join(":"),
        ),
    }
}

fn certificate_covers_hosts(cert_path: &Path, hosts: &[String]) -> bool {
    let output = Command::new("openssl")
        .arg("x509")
        .arg("-in")
        .arg(cert_path)
        .arg("-noout")
        .arg("-text")
        .output();

    let Ok(output) = output else {
        return false;
    };
    if !output.status.success() {
        return false;
    }

    let text = String::from_utf8_lossy(&output.stdout);
    hosts.iter().all(|host| {
        if let Some(ip_repr) = openssl_ip_address_repr(host) {
            text.contains(&format!("IP Address:{ip_repr}"))
        } else {
            text.contains(&format!("DNS:{host}"))
        }
    })
}

fn certificate_covers_current_hosts() -> bool {
    let cert_path = resolve_server_cert_path();
    cert_files_exist() && certificate_covers_hosts(&cert_path, &gateway_certificate_hosts())
}

fn extract_desktop_models(provider: &Provider) -> Value {
    let env = provider
        .settings_config
        .get("env")
        .and_then(|value| value.as_object());

    let model = env
        .and_then(|value| value.get("ANTHROPIC_MODEL"))
        .and_then(|value| value.as_str())
        .unwrap_or("claude-sonnet-4-20250514");
    let haiku = env
        .and_then(|value| value.get("ANTHROPIC_DEFAULT_HAIKU_MODEL"))
        .and_then(|value| value.as_str())
        .unwrap_or(model);
    let sonnet = env
        .and_then(|value| value.get("ANTHROPIC_DEFAULT_SONNET_MODEL"))
        .and_then(|value| value.as_str())
        .unwrap_or(model);
    let opus = env
        .and_then(|value| value.get("ANTHROPIC_DEFAULT_OPUS_MODEL"))
        .and_then(|value| value.as_str())
        .unwrap_or(model);

    json!({
        "model": model,
        "haiku": haiku,
        "sonnet": sonnet,
        "opus": opus,
        "haikuModel": haiku,
        "sonnetModel": sonnet,
        "opusModel": opus,
    })
}

pub fn build_live_config(
    provider: &Provider,
    gateway_base_url: &str,
    gateway_secret: &str,
) -> Value {
    json!({
        "enterpriseConfig": {
            "inferenceProvider": "gateway",
            "inferenceGatewayBaseUrl": gateway_base_url,
            "inferenceGatewayApiKey": gateway_secret,
            "fallbackModels": extract_desktop_models(provider),
        }
    })
}

pub fn read_live_config() -> Result<Value, AppError> {
    let path = resolve_config_path();
    read_json_file(&path)
}

pub fn write_live_config(config: &Value) -> Result<(), AppError> {
    let path = resolve_config_path();
    write_json_file(&path, config)
}

pub fn write_provider_live_config(
    provider: &Provider,
    listen_address: &str,
    proxy_port: u16,
    gateway_secret: &str,
) -> Result<Value, AppError> {
    let config = build_live_config(
        provider,
        &gateway_base_url(listen_address, proxy_port),
        gateway_secret,
    );
    write_live_config(&config)?;
    Ok(config)
}

fn is_managed_gateway_config_for_hosts(config: &Value, allowed_hosts: &[String]) -> bool {
    let Some(enterprise) = config
        .get("enterpriseConfig")
        .and_then(|value| value.as_object())
    else {
        return false;
    };

    let provider = enterprise
        .get("inferenceProvider")
        .and_then(|value| value.as_str())
        .unwrap_or_default();
    let base_url = enterprise
        .get("inferenceGatewayBaseUrl")
        .and_then(|value| value.as_str())
        .unwrap_or_default();

    if provider != "gateway" {
        return false;
    }

    let Ok(parsed) = url::Url::parse(base_url) else {
        return false;
    };
    if parsed.scheme() != "https" || parsed.path() != "/claude-desktop" {
        return false;
    }

    let Some(host) = parsed.host_str() else {
        return false;
    };
    let normalized_host = normalize_host(host);
    let mut allowed = BTreeSet::from([
        "127.0.0.1".to_string(),
        "localhost".to_string(),
        "::1".to_string(),
    ]);
    allowed.extend(allowed_hosts.iter().map(|host| normalize_host(host)));
    allowed.contains(&normalized_host)
}

pub fn is_managed_gateway_config(config: &Value) -> bool {
    is_managed_gateway_config_for_hosts(config, &detected_non_loopback_ipv4s())
}

pub fn has_managed_live_config() -> bool {
    read_live_config()
        .map(|config| is_managed_gateway_config(&config))
        .unwrap_or(false)
}

pub fn cert_files_exist() -> bool {
    resolve_server_cert_path().exists() && resolve_server_key_path().exists()
}

#[cfg(target_os = "macos")]
pub fn certificate_installed() -> bool {
    if !cert_files_exist() {
        return false;
    }
    if !certificate_covers_current_hosts() {
        return false;
    }

    Command::new("security")
        .arg("find-certificate")
        .arg("-c")
        .arg(CERT_COMMON_NAME)
        .arg("-a")
        .arg(
            std::env::var("HOME")
                .map(|home| format!("{home}/Library/Keychains/login.keychain-db"))
                .unwrap_or_else(|_| "login.keychain-db".to_string()),
        )
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

#[cfg(not(target_os = "macos"))]
pub fn certificate_installed() -> bool {
    false
}

pub fn install_certificate() -> Result<(), AppError> {
    #[cfg(not(target_os = "macos"))]
    {
        return Err(AppError::Message(
            "Claude Desktop experimental 目前仅支持 macOS".to_string(),
        ));
    }

    #[cfg(target_os = "macos")]
    {
        let cert_dir = resolve_cert_dir();
        std::fs::create_dir_all(&cert_dir).map_err(|e| AppError::io(&cert_dir, e))?;

        let cert_path = resolve_server_cert_path();
        let key_path = resolve_server_key_path();
        let desired_hosts = gateway_certificate_hosts();
        let regenerate_certificate =
            !cert_files_exist() || !certificate_covers_hosts(&cert_path, &desired_hosts);

        if regenerate_certificate {
            let _ = std::fs::remove_file(&cert_path);
            let _ = std::fs::remove_file(&key_path);
            let openssl_status = Command::new("openssl")
                .arg("req")
                .arg("-x509")
                .arg("-newkey")
                .arg("rsa:2048")
                .arg("-sha256")
                .arg("-nodes")
                .arg("-days")
                .arg("825")
                .arg("-subj")
                .arg(format!("/CN={CERT_COMMON_NAME}"))
                .arg("-addext")
                .arg(format!(
                    "subjectAltName={}",
                    subject_alt_name_argument(&desired_hosts)
                ))
                .arg("-keyout")
                .arg(&key_path)
                .arg("-out")
                .arg(&cert_path)
                .status()
                .map_err(|e| AppError::Message(format!("调用 openssl 失败: {e}")))?;

            if !openssl_status.success() {
                return Err(AppError::Message(
                    "生成 Claude Desktop 本地证书失败".to_string(),
                ));
            }

            log::info!(
                "已生成 Claude Desktop 本地证书，覆盖主机: {}",
                desired_hosts.join(", ")
            );
        }

        if regenerate_certificate || !certificate_installed() {
            let keychain = std::env::var("HOME")
                .map(|home| format!("{home}/Library/Keychains/login.keychain-db"))
                .unwrap_or_else(|_| "login.keychain-db".to_string());
            let output = Command::new("security")
                .arg("add-trusted-cert")
                .arg("-r")
                .arg("trustRoot")
                .arg("-k")
                .arg(keychain)
                .arg(&cert_path)
                .output()
                .map_err(|e| AppError::Message(format!("调用 security 失败: {e}")))?;

            if !output.status.success() {
                let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
                let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
                let detail = if !stderr.is_empty() {
                    stderr
                } else if !stdout.is_empty() {
                    stdout
                } else {
                    "未知错误".to_string()
                };
                return Err(AppError::Message(format!(
                    "安装 Claude Desktop 本地证书失败: {detail}"
                )));
            }
        }

        Ok(())
    }
}

pub fn launch_app() -> Result<(), AppError> {
    #[cfg(not(target_os = "macos"))]
    {
        return Err(AppError::Message(
            "Claude Desktop experimental 目前仅支持 macOS".to_string(),
        ));
    }

    #[cfg(target_os = "macos")]
    {
        let binary_path = resolve_binary_path();
        if !binary_path.exists() {
            return Err(AppError::Message(format!(
                "未找到 Claude Desktop 可执行文件: {}",
                binary_path.display()
            )));
        }

        let profile_dir = resolve_profile_dir();
        std::fs::create_dir_all(&profile_dir).map_err(|e| AppError::io(&profile_dir, e))?;

        if managed_profile_in_use() {
            log::warn!("Claude Desktop managed profile 已被现有实例占用，尝试激活现有窗口");
            return activate_claude_app();
        }

        let mut command = Command::new(&binary_path);
        if !launch_shim_installed_for(&binary_path) {
            command.arg("-3p").env("CLAUDE_USER_DATA_DIR", &profile_dir);
        }
        command
            .spawn()
            .map_err(|e| AppError::Message(format!("启动 Claude Desktop 失败: {e}")))?;

        Ok(())
    }
}

pub async fn build_status(
    listen_address: &str,
    proxy_port: u16,
    proxy_running: bool,
) -> ClaudeDesktopStatus {
    let app_path = resolve_app_path();
    let binary_path = resolve_binary_path();
    let profile_dir = resolve_profile_dir();
    let config_path = resolve_config_path();
    let cert_path = resolve_server_cert_path();
    let key_path = resolve_server_key_path();

    ClaudeDesktopStatus {
        supported: cfg!(target_os = "macos"),
        experimental: true,
        app_path: app_path.to_string_lossy().to_string(),
        app_exists: app_path.exists(),
        binary_path: binary_path.to_string_lossy().to_string(),
        binary_exists: binary_path.exists(),
        profile_dir: profile_dir.to_string_lossy().to_string(),
        config_path: config_path.to_string_lossy().to_string(),
        certificate_installed: certificate_installed(),
        certificate_path: cert_path.to_string_lossy().to_string(),
        key_path: key_path.to_string_lossy().to_string(),
        gateway_base_url: Some(gateway_base_url(listen_address, proxy_port)),
        managed_config_exists: has_managed_live_config(),
        launch_shim_installed: is_launch_shim_installed(),
        launch_shim_recovery_available: launch_shim_recovery_available_for(&binary_path),
        proxy_running,
    }
}

fn port_available(host: &str, port: u16) -> bool {
    TcpListener::bind(format_socket_address(host, port)).is_ok()
}

pub async fn build_doctor(
    listen_address: &str,
    proxy_port: u16,
    proxy_running: bool,
) -> ClaudeDesktopDoctor {
    crate::rustls_provider::ensure_rustls_crypto_provider();

    let status = build_status(listen_address, proxy_port, proxy_running).await;
    let health_url = local_gateway_health_url(listen_address, proxy_port);
    let http_port_available = port_available(listen_address, proxy_port);
    let https_bind_address = https_listener_bind_address(listen_address);
    let https_port_available =
        port_available(&https_bind_address, https_port_for_proxy_port(proxy_port));

    let gateway_healthy = if status.certificate_installed {
        match reqwest::Client::builder()
            .danger_accept_invalid_certs(true)
            .build()
        {
            Ok(client) => client
                .get(health_url)
                .send()
                .await
                .map(|resp| resp.status().is_success())
                .unwrap_or(false),
            Err(_) => false,
        }
    } else {
        false
    };

    let mut blockers = Vec::new();
    if !status.supported {
        blockers.push("仅支持 macOS".to_string());
    }
    if !status.app_exists || (!status.binary_exists && !status.launch_shim_recovery_available) {
        blockers.push("未发现 /Applications/Claude.app".to_string());
    }
    if status.launch_shim_recovery_available && !status.binary_exists {
        blockers.push("Claude.app 安装不完整，请重新安装官方 Claude.app".to_string());
    }
    if !status.certificate_installed {
        blockers.push("本地 HTTPS 证书尚未安装或需要更新".to_string());
    }
    if !proxy_running && !http_port_available {
        blockers.push(format!("HTTP 代理端口 {proxy_port} 已被占用"));
    }
    if !proxy_running && !https_port_available {
        blockers.push(format!(
            "HTTPS gateway 端口 {} 已被占用",
            https_port_for_proxy_port(proxy_port)
        ));
    }
    if status.certificate_installed && proxy_running && !gateway_healthy {
        blockers.push("本地 gateway 健康检查失败".to_string());
    }

    ClaudeDesktopDoctor {
        status,
        gateway_healthy,
        http_port_available,
        https_port_available,
        blockers,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::provider::Provider;
    use tempfile::tempdir;

    fn sample_provider(settings_config: Value) -> Provider {
        Provider::with_id(
            "provider-1".to_string(),
            "Sample Provider".to_string(),
            settings_config,
            None,
        )
    }

    #[test]
    fn gateway_url_uses_https_loopback_path() {
        assert_eq!(
            gateway_base_url_for_host("127.0.0.1", 15721),
            "https://127.0.0.1:15722/claude-desktop"
        );
        assert_eq!(
            gateway_base_url_for_host("::1", 15721),
            "https://[::1]:15722/claude-desktop"
        );
    }

    #[test]
    fn gateway_health_url_targets_root_health_endpoint() {
        assert_eq!(
            gateway_health_url_for_host("127.0.0.1", 15721),
            "https://127.0.0.1:15722/health"
        );
        assert_eq!(
            gateway_health_url_for_host("::1", 15721),
            "https://[::1]:15722/health"
        );
    }

    #[test]
    fn local_gateway_health_url_uses_loopback_for_wildcard_https_bind() {
        assert_eq!(
            local_gateway_health_host_for_bind_address("0.0.0.0"),
            "127.0.0.1"
        );
        assert_eq!(local_gateway_health_host_for_bind_address("::"), "::1");
        assert_eq!(
            local_gateway_health_url("127.0.0.1", 15721),
            "https://127.0.0.1:15722/health"
        );
    }

    #[test]
    fn preferred_gateway_host_prefers_detected_non_loopback_ipv4_for_loopback_listeners() {
        assert_eq!(
            preferred_gateway_host_for_candidates("127.0.0.1", &["10.29.161.134".to_string()]),
            "10.29.161.134"
        );
        assert_eq!(
            preferred_gateway_host_for_candidates("0.0.0.0", &["10.29.161.134".to_string()]),
            "10.29.161.134"
        );
        assert_eq!(
            preferred_gateway_host_for_candidates("192.168.1.20", &["10.29.161.134".to_string()]),
            "192.168.1.20"
        );
    }

    #[test]
    fn https_listener_bind_address_expands_loopback_when_gateway_uses_lan_ip() {
        assert_eq!(
            https_listener_bind_address_for_host("127.0.0.1", "10.29.161.134"),
            "0.0.0.0"
        );
        assert_eq!(
            https_listener_bind_address_for_host("127.0.0.1", "127.0.0.1"),
            "127.0.0.1"
        );
        assert_eq!(
            https_listener_bind_address_for_host("0.0.0.0", "10.29.161.134"),
            "0.0.0.0"
        );
    }

    #[test]
    fn parse_non_loopback_ipv4_candidates_sorts_active_interfaces_first() {
        let ifconfig = r#"
en7: flags=8863<UP,BROADCAST,RUNNING,SIMPLEX,MULTICAST> mtu 1500
    inet 10.0.0.24 netmask 0xffffff00 broadcast 10.0.0.255
    status: inactive
en0: flags=8863<UP,BROADCAST,RUNNING,SIMPLEX,MULTICAST> mtu 1500
    inet 10.29.161.134 netmask 0xffffff00 broadcast 10.29.161.255
    status: active
bridge0: flags=8822<BROADCAST,SIMPLEX,MULTICAST> mtu 1500
    inet 192.168.2.1 netmask 0xffffff00 broadcast 192.168.2.255
    status: active
lo0: flags=8049<UP,LOOPBACK,RUNNING,MULTICAST> mtu 16384
    inet 127.0.0.1 netmask 0xff000000
"#;

        assert_eq!(
            parse_non_loopback_ipv4_candidates_from_ifconfig(ifconfig),
            vec![
                "10.29.161.134".to_string(),
                "192.168.2.1".to_string(),
                "10.0.0.24".to_string(),
            ]
        );
    }

    #[test]
    fn subject_alt_name_argument_formats_dns_ipv4_and_ipv6_hosts() {
        assert_eq!(
            subject_alt_name_argument(&[
                "localhost".to_string(),
                "127.0.0.1".to_string(),
                "::1".to_string(),
            ]),
            "DNS:localhost,IP:127.0.0.1,IP:::1"
        );
    }

    #[test]
    fn openssl_ip_address_repr_expands_ipv6_loopback() {
        assert_eq!(
            openssl_ip_address_repr("::1"),
            Some("0:0:0:0:0:0:0:1".to_string())
        );
        assert_eq!(
            openssl_ip_address_repr("127.0.0.1"),
            Some("127.0.0.1".to_string())
        );
    }

    #[test]
    fn build_live_config_writes_gateway_enterprise_payload() {
        let provider = sample_provider(json!({
            "env": {
                "ANTHROPIC_MODEL": "claude-sonnet-4-20250514",
                "ANTHROPIC_DEFAULT_HAIKU_MODEL": "claude-haiku-4-20250514",
                "ANTHROPIC_DEFAULT_SONNET_MODEL": "claude-sonnet-4-20250514",
                "ANTHROPIC_DEFAULT_OPUS_MODEL": "claude-opus-4-20250514"
            }
        }));

        let config = build_live_config(
            &provider,
            "https://127.0.0.1:15722/claude-desktop",
            "desktop-secret",
        );

        assert_eq!(config["enterpriseConfig"]["inferenceProvider"], "gateway");
        assert_eq!(
            config["enterpriseConfig"]["inferenceGatewayBaseUrl"],
            "https://127.0.0.1:15722/claude-desktop"
        );
        assert_eq!(
            config["enterpriseConfig"]["inferenceGatewayApiKey"],
            "desktop-secret"
        );
        assert_eq!(
            config["enterpriseConfig"]["fallbackModels"]["haikuModel"],
            "claude-haiku-4-20250514"
        );
        assert_eq!(
            config["enterpriseConfig"]["fallbackModels"]["opusModel"],
            "claude-opus-4-20250514"
        );
    }

    #[test]
    fn build_live_config_falls_back_to_primary_model() {
        let provider = sample_provider(json!({
            "env": {
                "ANTHROPIC_MODEL": "claude-custom-sonnet"
            }
        }));

        let config = build_live_config(
            &provider,
            "https://127.0.0.1:15722/claude-desktop",
            "desktop-secret",
        );

        assert_eq!(
            config["enterpriseConfig"]["fallbackModels"]["model"],
            "claude-custom-sonnet"
        );
        assert_eq!(
            config["enterpriseConfig"]["fallbackModels"]["haiku"],
            "claude-custom-sonnet"
        );
        assert_eq!(
            config["enterpriseConfig"]["fallbackModels"]["sonnetModel"],
            "claude-custom-sonnet"
        );
        assert_eq!(
            config["enterpriseConfig"]["fallbackModels"]["opusModel"],
            "claude-custom-sonnet"
        );
    }

    #[test]
    fn managed_gateway_config_detection_requires_local_https_gateway() {
        assert!(is_managed_gateway_config_for_hosts(
            &json!({
                "enterpriseConfig": {
                    "inferenceProvider": "gateway",
                    "inferenceGatewayBaseUrl": "https://127.0.0.1:15722/claude-desktop"
                }
            }),
            &["10.29.161.134".to_string()]
        ));

        assert!(is_managed_gateway_config_for_hosts(
            &json!({
                "enterpriseConfig": {
                    "inferenceProvider": "gateway",
                    "inferenceGatewayBaseUrl": "https://10.29.161.134:15722/claude-desktop"
                }
            }),
            &["10.29.161.134".to_string()]
        ));

        assert!(!is_managed_gateway_config_for_hosts(
            &json!({
                "enterpriseConfig": {
                    "inferenceProvider": "gateway",
                    "inferenceGatewayBaseUrl": "https://api.example.com/claude-desktop"
                }
            }),
            &["10.29.161.134".to_string()]
        ));

        assert!(!is_managed_gateway_config_for_hosts(
            &json!({
                "enterpriseConfig": {
                    "inferenceProvider": "gateway",
                    "inferenceGatewayBaseUrl": "http://10.29.161.134:15722/claude-desktop"
                }
            }),
            &["10.29.161.134".to_string()]
        ));

        assert!(!is_managed_gateway_config_for_hosts(
            &json!({
                "enterpriseConfig": {
                    "inferenceProvider": "gateway",
                    "inferenceGatewayBaseUrl": "https://10.29.161.134:15722/not-claude-desktop"
                }
            }),
            &["10.29.161.134".to_string()]
        ));
    }

    #[test]
    fn derive_local_session_title_uses_initial_message_and_truncates() {
        let doc = json!({
            "initialMessage": "  Please    rename   this   session after the first user message arrives and keep it readable for the sidebar display  "
        });

        let title = derive_local_session_title(&doc).expect("title should be derived");
        assert!(
            title.starts_with("Please rename this session after the first user message arrives")
        );
        assert!(title.len() <= LOCAL_SESSION_TITLE_MAX_CHARS + 3);
    }

    #[test]
    fn persist_prompt_session_title_writes_temp_title() {
        let temp = tempdir().expect("tempdir");
        let profile_dir = temp.path().join("profile");
        let sessions_dir = profile_dir
            .join("local-agent-mode-sessions")
            .join("org")
            .join("workspace");
        fs::create_dir_all(&sessions_dir).expect("create sessions dir");

        let original_prompt = "Please help me investigate why the new Claude Desktop proxy path still leaves code sessions untitled after the first reply arrives";
        write_json_file(
            &sessions_dir.join("local_pending.json"),
            &json!({
                "sessionId": "local_pending",
                "initialMessage": original_prompt
            }),
        )
        .expect("write pending session");

        let persisted =
            persist_prompt_session_title(&sessions_dir.join("local_pending.json"), original_prompt)
                .expect("persist prompt title");
        assert!(persisted);

        let after = read_json_file::<Value>(&sessions_dir.join("local_pending.json"))
            .expect("read updated session");
        assert!(after
            .get("title")
            .and_then(Value::as_str)
            .expect("title should exist")
            .starts_with("Please help me investigate why the new Claude Desktop proxy path"));
        assert_eq!(
            after.get("titleSource").and_then(Value::as_str),
            Some(LOCAL_SESSION_TITLE_SOURCE_PROMPT)
        );
    }

    #[test]
    fn lookup_cowork_session_title_target_matches_session_id() {
        let temp = tempdir().expect("tempdir");
        let profile_dir = temp.path().join("profile");
        let sessions_dir = profile_dir
            .join("local-agent-mode-sessions")
            .join("org")
            .join("workspace");
        fs::create_dir_all(&sessions_dir).expect("create sessions dir");

        write_json_file(
            &sessions_dir.join("local_pending.json"),
            &json!({
                "sessionId": "local_pending",
                "initialMessage": "帮我检查 session 标题",
            }),
        )
        .expect("write pending session");

        let lookup = lookup_cowork_session_title_target_in_profile(
            &profile_dir,
            "local_pending",
            "帮我检查 session 标题",
        )
        .expect("lookup should work");

        match lookup {
            LocalSessionTitleLookup::Pending {
                kind: "cowork",
                description,
                ..
            } => assert_eq!(description.as_deref(), Some("帮我检查 session 标题")),
            other => panic!("unexpected lookup result: {other:?}"),
        }
    }

    #[test]
    fn lookup_claude_code_session_title_target_matches_cli_session_id() {
        let temp = tempdir().expect("tempdir");
        let profile_dir = temp.path().join("profile");
        let sessions_dir = profile_dir
            .join("claude-code-sessions")
            .join("org")
            .join("workspace");
        fs::create_dir_all(&sessions_dir).expect("create code sessions dir");

        let session_path = sessions_dir.join("local_code.json");
        write_json_file(
            &session_path,
            &json!({
                "sessionId": "local_code",
                "cliSessionId": "cli-session-123",
                "cwd": "/tmp/project"
            }),
        )
        .expect("write code session");

        let lookup = lookup_claude_code_session_title_target_in_profile(
            &profile_dir,
            &temp.path().join("projects"),
            "cli-session-123",
            "请帮我重构这个函数",
        )
        .expect("lookup should work");

        match lookup {
            LocalSessionTitleLookup::Pending {
                kind: "code",
                description,
                ..
            } => assert!(description.is_none()),
            other => panic!("unexpected lookup result: {other:?}"),
        }
    }

    #[test]
    fn lookup_claude_code_session_title_target_derives_description_from_transcript() {
        let temp = tempdir().expect("tempdir");
        let profile_dir = temp.path().join("profile");
        let sessions_dir = profile_dir
            .join("claude-code-sessions")
            .join("org")
            .join("workspace");
        let projects_dir = temp.path().join("projects").join("demo-project");
        fs::create_dir_all(&sessions_dir).expect("create code sessions dir");
        fs::create_dir_all(&projects_dir).expect("create projects dir");

        let session_path = sessions_dir.join("local_code.json");
        write_json_file(
            &session_path,
            &json!({
                "sessionId": "local_code",
                "cliSessionId": "cli-session-456",
                "cwd": "/tmp/project"
            }),
        )
        .expect("write code session");

        fs::write(
            projects_dir.join("cli-session-456.jsonl"),
            concat!(
                "{\"type\":\"queue-operation\",\"operation\":\"enqueue\",\"content\":\"请帮我重构这个函数\"}\n",
                "{\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":\"请帮我重构这个函数\"}}\n"
            ),
        )
        .expect("write transcript");

        let lookup = lookup_claude_code_session_title_target_in_profile(
            &profile_dir,
            &temp.path().join("projects"),
            "cli-session-456",
            "",
        )
        .expect("lookup should work");

        match lookup {
            LocalSessionTitleLookup::Pending {
                kind: "code",
                description,
                ..
            } => assert_eq!(description.as_deref(), Some("请帮我重构这个函数")),
            other => panic!("unexpected lookup result: {other:?}"),
        }
    }

    #[test]
    fn lookup_claude_code_session_title_target_matches_prompt_via_transcript() {
        let temp = tempdir().expect("tempdir");
        let profile_dir = temp.path().join("profile");
        let sessions_dir = profile_dir
            .join("claude-code-sessions")
            .join("org")
            .join("workspace");
        let projects_dir = temp.path().join("projects").join("demo-project");
        fs::create_dir_all(&sessions_dir).expect("create code sessions dir");
        fs::create_dir_all(&projects_dir).expect("create projects dir");

        let session_path = sessions_dir.join("local_code.json");
        write_json_file(
            &session_path,
            &json!({
                "sessionId": "local_code",
                "cliSessionId": "cli-session-789",
                "cwd": "/tmp/project"
            }),
        )
        .expect("write code session");

        fs::write(
            projects_dir.join("cli-session-789.jsonl"),
            "{\"type\":\"last-prompt\",\"prompt\":\"请帮我检查标题问题\"}\n",
        )
        .expect("write transcript");

        let lookup = lookup_claude_code_session_title_target_in_profile(
            &profile_dir,
            &temp.path().join("projects"),
            "",
            "请帮我检查标题问题",
        )
        .expect("lookup should work");

        match lookup {
            LocalSessionTitleLookup::Pending {
                kind: "code",
                description,
                ..
            } => assert_eq!(description.as_deref(), Some("请帮我检查标题问题")),
            other => panic!("unexpected lookup result: {other:?}"),
        }
    }

    #[test]
    fn lookup_recent_local_session_title_target_prefers_newest_pending_session() {
        let temp = tempdir().expect("tempdir");
        let now_ms = current_unix_time_ms();
        let profile_dir = temp.path().join("profile");
        let cowork_sessions_dir = profile_dir
            .join("local-agent-mode-sessions")
            .join("org")
            .join("workspace");
        let code_sessions_dir = profile_dir
            .join("claude-code-sessions")
            .join("org")
            .join("workspace");
        let projects_dir = temp.path().join("projects").join("demo-project");
        fs::create_dir_all(&cowork_sessions_dir).expect("create cowork sessions dir");
        fs::create_dir_all(&code_sessions_dir).expect("create code sessions dir");
        fs::create_dir_all(&projects_dir).expect("create projects dir");

        write_json_file(
            &cowork_sessions_dir.join("local_cowork.json"),
            &json!({
                "sessionId": "local_cowork",
                "initialMessage": "旧会话",
                "createdAt": now_ms.saturating_sub(20_000),
                "lastActivityAt": now_ms.saturating_sub(15_000),
            }),
        )
        .expect("write cowork session");

        write_json_file(
            &code_sessions_dir.join("local_code.json"),
            &json!({
                "sessionId": "local_code",
                "cliSessionId": "cli-session-recent",
                "createdAt": now_ms.saturating_sub(5_000),
                "lastActivityAt": now_ms.saturating_sub(2_000),
            }),
        )
        .expect("write code session");
        fs::write(
            projects_dir.join("cli-session-recent.jsonl"),
            "{\"type\":\"last-prompt\",\"prompt\":\"新会话\"}\n",
        )
        .expect("write transcript");

        let lookup = lookup_recent_local_session_title_target(
            &profile_dir,
            Some(temp.path().join("projects").as_path()),
            LocalSessionTitleLookupPreference::CoworkFirst,
        )
        .expect("lookup should work");

        match lookup {
            LocalSessionTitleLookup::Pending {
                kind: "code",
                description,
                ..
            } => assert_eq!(description.as_deref(), Some("新会话")),
            other => panic!("unexpected lookup result: {other:?}"),
        }
    }

    #[test]
    fn lookup_recent_local_session_title_target_ignores_stale_sessions() {
        let temp = tempdir().expect("tempdir");
        let now_ms = current_unix_time_ms();
        let profile_dir = temp.path().join("profile");
        let sessions_dir = profile_dir
            .join("local-agent-mode-sessions")
            .join("org")
            .join("workspace");
        fs::create_dir_all(&sessions_dir).expect("create sessions dir");

        write_json_file(
            &sessions_dir.join("local_stale.json"),
            &json!({
                "sessionId": "local_stale",
                "initialMessage": "太久之前的会话",
                "createdAt": now_ms.saturating_sub(LOCAL_SESSION_TITLE_RECENT_FALLBACK_MAX_AGE_MS + 30_000),
                "lastActivityAt": now_ms.saturating_sub(LOCAL_SESSION_TITLE_RECENT_FALLBACK_MAX_AGE_MS + 20_000),
            }),
        )
        .expect("write stale session");

        let lookup = lookup_recent_local_session_title_target(
            &profile_dir,
            Some(temp.path().join("projects").as_path()),
            LocalSessionTitleLookupPreference::CoworkFirst,
        )
        .expect("lookup should work");

        assert!(matches!(lookup, LocalSessionTitleLookup::NotFound));
    }

    #[test]
    fn launch_shim_install_and_remove_roundtrip() {
        let temp = tempdir().expect("tempdir");
        let macos_dir = temp
            .path()
            .join("Claude.app")
            .join("Contents")
            .join("MacOS");
        fs::create_dir_all(&macos_dir).expect("create app bundle");

        let binary_path = macos_dir.join("Claude");
        fs::write(&binary_path, b"original-binary").expect("write original binary");

        let profile_dir = temp.path().join("profile");
        fs::create_dir_all(&profile_dir).expect("create profile dir");

        install_launch_shim_for(&binary_path, &profile_dir).expect("install launch shim");
        assert!(launch_shim_installed_for(&binary_path));

        let backup_path = launch_shim_backup_path_for(&binary_path);
        assert_eq!(
            fs::read(&backup_path).expect("read backup"),
            b"original-binary"
        );

        let shim = fs::read_to_string(&binary_path).expect("read shim");
        assert!(shim.contains(LAUNCH_SHIM_MARKER));
        assert!(shim.contains("CLAUDE_USER_DATA_DIR"));
        assert!(shim.contains("-3p \"$@\""));

        remove_launch_shim_for(&binary_path).expect("remove launch shim");
        assert!(!launch_shim_installed_for(&binary_path));
        assert!(!backup_path.exists());
        assert_eq!(
            fs::read(&binary_path).expect("read restored binary"),
            b"original-binary"
        );
    }

    #[test]
    fn launch_shim_reinstall_refreshes_profile_path() {
        let temp = tempdir().expect("tempdir");
        let macos_dir = temp
            .path()
            .join("Claude.app")
            .join("Contents")
            .join("MacOS");
        fs::create_dir_all(&macos_dir).expect("create app bundle");

        let binary_path = macos_dir.join("Claude");
        fs::write(&binary_path, b"original-binary").expect("write original binary");

        let first_profile = temp.path().join("profile-a");
        let second_profile = temp.path().join("profile-b");
        fs::create_dir_all(&first_profile).expect("create first profile");
        fs::create_dir_all(&second_profile).expect("create second profile");

        install_launch_shim_for(&binary_path, &first_profile).expect("install launch shim");
        install_launch_shim_for(&binary_path, &second_profile).expect("reinstall launch shim");

        let shim = fs::read_to_string(&binary_path).expect("read shim");
        assert!(
            shim.contains(second_profile.to_string_lossy().as_ref()),
            "shim should point at latest managed profile"
        );
        assert!(
            !shim.contains(first_profile.to_string_lossy().as_ref()),
            "old managed profile should not remain in shim"
        );
    }

    #[test]
    fn launch_shim_recovery_available_when_backup_exists_and_binary_is_missing() {
        let temp = tempdir().expect("tempdir");
        let macos_dir = temp
            .path()
            .join("Claude.app")
            .join("Contents")
            .join("MacOS");
        fs::create_dir_all(&macos_dir).expect("create app bundle");

        let binary_path = macos_dir.join("Claude");
        let backup_path = launch_shim_backup_path_for(&binary_path);
        fs::write(&backup_path, b"original-binary").expect("write backup");

        assert!(launch_shim_recovery_available_for(&binary_path));
    }

    #[test]
    fn privileged_install_launch_shim_script_wraps_binary_and_resigns_app() {
        let temp = tempdir().expect("tempdir");
        let app_path = temp.path().join("Claude.app");
        let binary_path = app_path.join("Contents").join("MacOS").join("Claude");
        let backup_path = launch_shim_backup_path_for(&binary_path);
        let profile_dir = temp.path().join("profile");

        let script = build_privileged_install_launch_shim_script(
            &app_path,
            &binary_path,
            &backup_path,
            &profile_dir,
        );

        assert!(script.contains("mv \"$binary\" \"$backup\""));
        assert!(script.contains("CC_SWITCH_SHIM"));
        assert!(script.contains(LAUNCH_SHIM_MARKER));
        assert!(script.contains("CLAUDE_USER_DATA_DIR"));
        assert!(script.contains("/usr/bin/codesign --force --deep --sign - \"$app\""));
    }

    #[test]
    fn privileged_remove_launch_shim_script_restores_backup_and_resigns_app() {
        let temp = tempdir().expect("tempdir");
        let app_path = temp.path().join("Claude.app");
        let binary_path = app_path.join("Contents").join("MacOS").join("Claude");
        let backup_path = launch_shim_backup_path_for(&binary_path);

        let script =
            build_privileged_remove_launch_shim_script(&app_path, &binary_path, &backup_path);

        assert!(script.contains("mv \"$backup\" \"$binary\""));
        assert!(script.contains("rm -f \"$binary\""));
        assert!(script.contains("/usr/bin/codesign --force --deep --sign - \"$app\""));
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn install_launch_shim_is_disabled_on_macos() {
        let err = install_launch_shim().expect_err("install launch shim should be disabled");
        assert!(err.to_string().contains("Direct Launch Shim 已暂时禁用"));
    }

    #[test]
    fn replace_prompt_session_title_promotes_temp_title_to_auto() {
        let temp = tempdir().expect("tempdir");
        let profile_dir = temp.path().join("profile");
        let sessions_dir = profile_dir
            .join("local-agent-mode-sessions")
            .join("org")
            .join("workspace");
        fs::create_dir_all(&sessions_dir).expect("create sessions dir");

        let session_path = sessions_dir.join("local_prompt.json");
        write_json_file(
            &session_path,
            &json!({
                "sessionId": "local_prompt",
                "initialMessage": "帮我看一下为什么标题很慢",
                "title": "帮我看一下为什么标题很慢",
                "titleSource": "prompt"
            }),
        )
        .expect("write prompt-titled session");

        let replaced =
            replace_prompt_session_title(&session_path, "Investigate slow session title sync")
                .expect("replace prompt title");
        assert!(replaced);

        let after = read_json_file::<Value>(&session_path).expect("read updated");
        assert_eq!(
            after.get("title").and_then(Value::as_str),
            Some("Investigate slow session title sync")
        );
        assert_eq!(
            after.get("titleSource").and_then(Value::as_str),
            Some(LOCAL_SESSION_TITLE_SOURCE_AUTO)
        );
    }

    #[test]
    fn replace_prompt_session_title_leaves_existing_manual_title_untouched() {
        let temp = tempdir().expect("tempdir");
        let profile_dir = temp.path().join("profile");
        let sessions_dir = profile_dir
            .join("local-agent-mode-sessions")
            .join("org")
            .join("workspace");
        fs::create_dir_all(&sessions_dir).expect("create sessions dir");

        let session_path = sessions_dir.join("local_manual.json");
        write_json_file(
            &session_path,
            &json!({
                "sessionId": "local_manual",
                "initialMessage": "别覆盖我",
                "title": "Existing title",
                "titleSource": "manual"
            }),
        )
        .expect("write manual session");

        let replaced = replace_prompt_session_title(&session_path, "Generated title")
            .expect("try replace manual title");
        assert!(!replaced);

        let after = read_json_file::<Value>(&session_path).expect("read unchanged session");
        assert_eq!(
            after.get("title").and_then(Value::as_str),
            Some("Existing title")
        );
        assert_eq!(
            after.get("titleSource").and_then(Value::as_str),
            Some("manual")
        );
    }
}
