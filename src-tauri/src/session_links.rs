use crate::claude_desktop_config::{
    derive_claude_code_description_from_transcript, derive_local_session_description,
};
use crate::config::{read_json_file, write_json_file};
use crate::error::AppError;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

const SESSION_LINKS_FILENAME: &str = "session-links.json";
const SESSION_LINKS_VERSION: u32 = 1;
const LOCAL_AGENT_MODE_SESSIONS_DIRNAME: &str = "local-agent-mode-sessions";
const CLAUDE_CODE_SESSIONS_DIRNAME: &str = "claude-code-sessions";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum LocalSessionKind {
    Cowork,
    Code,
}

impl LocalSessionKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Cowork => "cowork",
            Self::Code => "code",
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionLinkRecord {
    pub canonical_session_id: String,
    pub kind: LocalSessionKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remote_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cli_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_session_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub local_session_path: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_prompt_hash: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub initial_prompt_preview: Option<String>,
    pub created_at: u64,
    pub last_seen_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SessionLinksRegistry {
    version: u32,
    #[serde(default)]
    links: HashMap<String, SessionLinkRecord>,
}

impl Default for SessionLinksRegistry {
    fn default() -> Self {
        Self {
            version: SESSION_LINKS_VERSION,
            links: HashMap::new(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct SessionLinkIdentityInput {
    pub remote_session_id: String,
    pub initial_prompt: Option<String>,
    pub initial_prompt_hash: Option<String>,
    pub initial_prompt_preview: Option<String>,
}

#[derive(Debug, Clone)]
pub struct ResolvedSessionLink {
    pub record: SessionLinkRecord,
    pub path: PathBuf,
    pub description: Option<String>,
    pub has_title: bool,
}

#[derive(Debug, Clone)]
pub struct CodeSessionLinkRefresh {
    pub canonical_session_id: String,
    pub cli_session_id: String,
    pub local_session_id: String,
    pub local_session_path: String,
    pub initial_prompt_hash: Option<String>,
    pub initial_prompt_preview: Option<String>,
    pub created_at: u64,
    pub last_seen_at: u64,
}

pub fn build_identity_input(
    remote_session_id: String,
    initial_prompt: Option<String>,
    initial_prompt_hash: Option<String>,
    initial_prompt_preview: Option<String>,
) -> SessionLinkIdentityInput {
    SessionLinkIdentityInput {
        remote_session_id,
        initial_prompt,
        initial_prompt_hash,
        initial_prompt_preview,
    }
}

pub fn sync_key(canonical_session_id: &str) -> String {
    format!("session-link:{canonical_session_id}")
}

pub fn hash_prompt(prompt: &str) -> String {
    let digest = Sha256::digest(prompt.as_bytes());
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn update_last_seen(
    canonical_session_id: &str,
    local_session_path: &Path,
) -> Result<(), AppError> {
    let profile_dir = crate::claude_desktop_config::resolve_profile_dir();
    let mut registry = read_registry(&profile_dir)?;
    let Some(record) = registry.links.get_mut(canonical_session_id) else {
        return Ok(());
    };
    record.last_seen_at = current_unix_time_ms();
    record.local_session_path = Some(local_session_path.display().to_string());
    write_registry(&profile_dir, &registry)
}

pub fn annotate_session_doc(path: &Path, canonical_session_id: &str) -> Result<(), AppError> {
    let mut doc = read_json_file::<Value>(path)?;
    let Some(obj) = doc.as_object_mut() else {
        return Ok(());
    };
    if obj
        .get("canonicalSessionId")
        .and_then(Value::as_str)
        .map(str::trim)
        == Some(canonical_session_id)
    {
        return Ok(());
    }
    obj.insert(
        "canonicalSessionId".to_string(),
        Value::String(canonical_session_id.to_string()),
    );
    write_json_file(path, &doc)
}

pub fn resolve_title_target(
    profile_dir: &Path,
    projects_dir: Option<&Path>,
    identity: &SessionLinkIdentityInput,
) -> Result<Option<ResolvedSessionLink>, AppError> {
    let mut registry = read_registry(profile_dir)?;
    let remote_session_id = normalize_value(&identity.remote_session_id);

    if let Some((canonical_id, record)) = registry
        .links
        .iter()
        .find(|(_, record)| {
            matches_identity(
                record,
                remote_session_id.as_deref(),
                identity.initial_prompt_hash.as_deref(),
            )
        })
        .map(|(canonical_id, record)| (canonical_id.clone(), record.clone()))
    {
        if let Some(mut resolved) = resolve_existing_link(profile_dir, projects_dir, &record)? {
            refresh_record(&mut resolved.record, identity, &resolved.path);
            registry.links.insert(canonical_id, resolved.record.clone());
            write_registry(profile_dir, &registry)?;
            annotate_session_doc(&resolved.path, &resolved.record.canonical_session_id)?;
            return Ok(Some(resolved));
        }
    }

    if let Some(resolved) = bootstrap_cowork(profile_dir, identity)? {
        annotate_session_doc(&resolved.path, &resolved.record.canonical_session_id)?;
        registry.links.insert(
            resolved.record.canonical_session_id.clone(),
            resolved.record.clone(),
        );
        write_registry(profile_dir, &registry)?;
        return Ok(Some(resolved));
    }

    if let Some(projects_dir) = projects_dir {
        if let Some(resolved) = bootstrap_code(profile_dir, projects_dir, identity)? {
            annotate_session_doc(&resolved.path, &resolved.record.canonical_session_id)?;
            registry.links.insert(
                resolved.record.canonical_session_id.clone(),
                resolved.record.clone(),
            );
            write_registry(profile_dir, &registry)?;
            return Ok(Some(resolved));
        }
    }

    Ok(None)
}

pub fn build_code_refreshes(
    profile_dir: &Path,
    bucket: &Path,
    sessions: &[(String, String, Option<String>, u64, u64)],
) -> Result<Vec<CodeSessionLinkRefresh>, AppError> {
    let registry = read_registry(profile_dir)?;
    let mut refreshes = Vec::new();

    for (cli_session_id, local_session_id, title, created_at, last_seen_at) in sessions {
        let canonical_session_id = registry
            .links
            .values()
            .find(|record| {
                record.kind == LocalSessionKind::Code
                    && record.cli_session_id.as_deref() == Some(cli_session_id.as_str())
            })
            .map(|record| record.canonical_session_id.clone())
            .unwrap_or_else(|| Uuid::new_v4().to_string());
        refreshes.push(CodeSessionLinkRefresh {
            canonical_session_id,
            cli_session_id: cli_session_id.clone(),
            local_session_id: local_session_id.clone(),
            local_session_path: bucket
                .join(format!("{local_session_id}.json"))
                .display()
                .to_string(),
            initial_prompt_hash: title.as_deref().map(hash_prompt),
            initial_prompt_preview: title.clone(),
            created_at: *created_at,
            last_seen_at: *last_seen_at,
        });
    }

    Ok(refreshes)
}

pub fn apply_code_refreshes(
    profile_dir: &Path,
    refreshes: &[CodeSessionLinkRefresh],
) -> Result<(), AppError> {
    let mut registry = read_registry(profile_dir)?;
    registry
        .links
        .retain(|_, record| record.kind != LocalSessionKind::Code);
    for refresh in refreshes {
        registry.links.insert(
            refresh.canonical_session_id.clone(),
            SessionLinkRecord {
                canonical_session_id: refresh.canonical_session_id.clone(),
                kind: LocalSessionKind::Code,
                remote_session_id: Some(refresh.cli_session_id.clone()),
                cli_session_id: Some(refresh.cli_session_id.clone()),
                local_session_id: Some(refresh.local_session_id.clone()),
                local_session_path: Some(refresh.local_session_path.clone()),
                initial_prompt_hash: refresh.initial_prompt_hash.clone(),
                initial_prompt_preview: refresh.initial_prompt_preview.clone(),
                created_at: refresh.created_at,
                last_seen_at: refresh.last_seen_at,
            },
        );
    }
    write_registry(profile_dir, &registry)
}

#[cfg(test)]
pub fn registry_link_for_cli_session(
    profile_dir: &Path,
    cli_session_id: &str,
) -> Result<Option<SessionLinkRecord>, AppError> {
    Ok(read_registry(profile_dir)?
        .links
        .values()
        .find(|record| record.cli_session_id.as_deref() == Some(cli_session_id))
        .cloned())
}

fn registry_path(profile_dir: &Path) -> PathBuf {
    profile_dir.join(SESSION_LINKS_FILENAME)
}

fn read_registry(profile_dir: &Path) -> Result<SessionLinksRegistry, AppError> {
    let path = registry_path(profile_dir);
    if !path.exists() {
        return Ok(SessionLinksRegistry::default());
    }
    read_json_file(&path)
}

#[cfg(test)]
pub fn read_registry_links(
    profile_dir: &Path,
) -> Result<HashMap<String, SessionLinkRecord>, AppError> {
    Ok(read_registry(profile_dir)?.links)
}

fn write_registry(profile_dir: &Path, registry: &SessionLinksRegistry) -> Result<(), AppError> {
    write_json_file(&registry_path(profile_dir), registry)
}

fn matches_identity(
    record: &SessionLinkRecord,
    remote_session_id: Option<&str>,
    prompt_hash: Option<&str>,
) -> bool {
    record
        .remote_session_id
        .as_deref()
        .zip(remote_session_id)
        .map(|(left, right)| left == right)
        .unwrap_or(false)
        || record
            .initial_prompt_hash
            .as_deref()
            .zip(prompt_hash)
            .map(|(left, right)| left == right)
            .unwrap_or(false)
}

fn resolve_existing_link(
    profile_dir: &Path,
    projects_dir: Option<&Path>,
    record: &SessionLinkRecord,
) -> Result<Option<ResolvedSessionLink>, AppError> {
    match record.kind {
        LocalSessionKind::Cowork => resolve_cowork_target(profile_dir, record),
        LocalSessionKind::Code => resolve_code_target(profile_dir, projects_dir, record),
    }
}

fn resolve_cowork_target(
    profile_dir: &Path,
    record: &SessionLinkRecord,
) -> Result<Option<ResolvedSessionLink>, AppError> {
    let mut session_paths = Vec::new();
    collect_local_session_json_paths(
        &profile_dir.join(LOCAL_AGENT_MODE_SESSIONS_DIRNAME),
        &mut session_paths,
    )?;
    for path in session_paths {
        let doc = match read_json_file::<Value>(&path) {
            Ok(doc) => doc,
            Err(_) => continue,
        };
        let local_session_id = session_doc_id(&doc, &path);
        let matched = session_doc_canonical_id(&doc) == Some(record.canonical_session_id.as_str())
            || record.local_session_path.as_deref() == Some(path.display().to_string().as_str())
            || record.local_session_id.as_deref() == Some(local_session_id.as_str())
            || record.remote_session_id.as_deref() == Some(local_session_id.as_str());
        if !matched {
            continue;
        }
        return Ok(Some(ResolvedSessionLink {
            record: record.clone(),
            path,
            description: derive_local_session_description(&doc),
            has_title: session_doc_has_title(&doc),
        }));
    }
    Ok(None)
}

fn resolve_code_target(
    profile_dir: &Path,
    projects_dir: Option<&Path>,
    record: &SessionLinkRecord,
) -> Result<Option<ResolvedSessionLink>, AppError> {
    let mut session_paths = Vec::new();
    collect_local_session_json_paths(
        &profile_dir.join(CLAUDE_CODE_SESSIONS_DIRNAME),
        &mut session_paths,
    )?;
    let transcript_index = collect_transcript_index(projects_dir)?;

    for path in session_paths {
        let doc = match read_json_file::<Value>(&path) {
            Ok(doc) => doc,
            Err(_) => continue,
        };
        let local_session_id = session_doc_id(&doc, &path);
        let cli_session_id = doc.get("cliSessionId").and_then(Value::as_str);
        let matched = session_doc_canonical_id(&doc) == Some(record.canonical_session_id.as_str())
            || record.local_session_path.as_deref() == Some(path.display().to_string().as_str())
            || record.local_session_id.as_deref() == Some(local_session_id.as_str())
            || record
                .cli_session_id
                .as_deref()
                .zip(cli_session_id)
                .map(|(left, right)| left == right)
                .unwrap_or(false)
            || record
                .remote_session_id
                .as_deref()
                .zip(cli_session_id)
                .map(|(left, right)| left == right)
                .unwrap_or(false);
        if !matched {
            continue;
        }
        let description = derive_local_session_description(&doc).or_else(|| {
            derive_code_description(&doc, &transcript_index)
                .ok()
                .flatten()
        });
        return Ok(Some(ResolvedSessionLink {
            record: record.clone(),
            path,
            description,
            has_title: session_doc_has_title(&doc),
        }));
    }
    Ok(None)
}

fn bootstrap_cowork(
    profile_dir: &Path,
    identity: &SessionLinkIdentityInput,
) -> Result<Option<ResolvedSessionLink>, AppError> {
    let mut session_paths = Vec::new();
    collect_local_session_json_paths(
        &profile_dir.join(LOCAL_AGENT_MODE_SESSIONS_DIRNAME),
        &mut session_paths,
    )?;
    let remote_session_id = normalize_value(&identity.remote_session_id);

    for path in session_paths {
        let doc = match read_json_file::<Value>(&path) {
            Ok(doc) => doc,
            Err(_) => continue,
        };
        let local_session_id = session_doc_id(&doc, &path);
        let matches_remote = remote_session_id.as_deref() == Some(local_session_id.as_str());
        let matches_prompt_hash =
            prompt_hash_matches(&doc, identity.initial_prompt_hash.as_deref());
        if !matches_remote && !matches_prompt_hash {
            continue;
        }
        let record = build_record(
            LocalSessionKind::Cowork,
            identity,
            Some(local_session_id),
            None,
            Some(path.clone()),
        );
        return Ok(Some(ResolvedSessionLink {
            record,
            path,
            description: derive_local_session_description(&doc),
            has_title: session_doc_has_title(&doc),
        }));
    }
    Ok(None)
}

fn bootstrap_code(
    profile_dir: &Path,
    projects_dir: &Path,
    identity: &SessionLinkIdentityInput,
) -> Result<Option<ResolvedSessionLink>, AppError> {
    let mut session_paths = Vec::new();
    collect_local_session_json_paths(
        &profile_dir.join(CLAUDE_CODE_SESSIONS_DIRNAME),
        &mut session_paths,
    )?;
    let transcript_index = collect_transcript_index(Some(projects_dir))?;
    let remote_session_id = normalize_value(&identity.remote_session_id);

    for path in session_paths {
        let doc = match read_json_file::<Value>(&path) {
            Ok(doc) => doc,
            Err(_) => continue,
        };
        let local_session_id = session_doc_id(&doc, &path);
        let cli_session_id = doc
            .get("cliSessionId")
            .and_then(Value::as_str)
            .map(str::to_string);
        let description = derive_local_session_description(&doc).or_else(|| {
            derive_code_description(&doc, &transcript_index)
                .ok()
                .flatten()
        });
        let matches_remote = remote_session_id
            .as_deref()
            .map(|value| Some(value) == cli_session_id.as_deref() || value == local_session_id)
            .unwrap_or(false);
        let matches_prompt_hash = identity
            .initial_prompt_hash
            .as_deref()
            .zip(description.as_deref())
            .map(|(prompt_hash, description)| hash_prompt(description) == prompt_hash)
            .unwrap_or(false);
        if !matches_remote && !matches_prompt_hash {
            continue;
        }
        let record = build_record(
            LocalSessionKind::Code,
            identity,
            Some(local_session_id),
            cli_session_id,
            Some(path.clone()),
        );
        return Ok(Some(ResolvedSessionLink {
            record,
            path,
            description,
            has_title: session_doc_has_title(&doc),
        }));
    }
    Ok(None)
}

fn build_record(
    kind: LocalSessionKind,
    identity: &SessionLinkIdentityInput,
    local_session_id: Option<String>,
    cli_session_id: Option<String>,
    local_session_path: Option<PathBuf>,
) -> SessionLinkRecord {
    let now = current_unix_time_ms();
    SessionLinkRecord {
        canonical_session_id: Uuid::new_v4().to_string(),
        kind,
        remote_session_id: normalize_value(&identity.remote_session_id),
        cli_session_id,
        local_session_id,
        local_session_path: local_session_path.map(|path| path.display().to_string()),
        initial_prompt_hash: identity.initial_prompt_hash.clone(),
        initial_prompt_preview: identity.initial_prompt_preview.clone(),
        created_at: now,
        last_seen_at: now,
    }
}

fn refresh_record(
    record: &mut SessionLinkRecord,
    identity: &SessionLinkIdentityInput,
    path: &Path,
) {
    record.last_seen_at = current_unix_time_ms();
    if let Some(remote_session_id) = normalize_value(&identity.remote_session_id) {
        record.remote_session_id = Some(remote_session_id);
    }
    if let Some(initial_prompt_hash) = identity.initial_prompt_hash.as_ref() {
        record.initial_prompt_hash = Some(initial_prompt_hash.clone());
    }
    if let Some(initial_prompt_preview) = identity.initial_prompt_preview.as_ref() {
        record.initial_prompt_preview = Some(initial_prompt_preview.clone());
    }
    record.local_session_path = Some(path.display().to_string());
}

fn prompt_hash_matches(doc: &Value, prompt_hash: Option<&str>) -> bool {
    let Some(prompt_hash) = prompt_hash else {
        return false;
    };
    derive_local_session_description(doc)
        .map(|description| hash_prompt(description.as_str()) == prompt_hash)
        .unwrap_or(false)
}

fn normalize_value(value: &str) -> Option<String> {
    let value = value.trim();
    if value.is_empty() {
        None
    } else {
        Some(value.to_string())
    }
}

fn session_doc_canonical_id(doc: &Value) -> Option<&str> {
    doc.get("canonicalSessionId")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
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

fn session_doc_has_title(doc: &Value) -> bool {
    doc.get("title")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .is_some()
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

fn collect_transcript_index(
    projects_dir: Option<&Path>,
) -> Result<HashMap<String, PathBuf>, AppError> {
    let mut transcript_index = HashMap::new();
    let Some(projects_dir) = projects_dir else {
        return Ok(transcript_index);
    };
    if !projects_dir.exists() {
        return Ok(transcript_index);
    }
    collect_transcript_index_dir(projects_dir, &mut transcript_index)?;
    Ok(transcript_index)
}

fn collect_transcript_index_dir(
    dir: &Path,
    index: &mut HashMap<String, PathBuf>,
) -> Result<(), AppError> {
    for entry in fs::read_dir(dir).map_err(|e| AppError::io(dir, e))? {
        let entry = entry.map_err(|e| AppError::io(dir, e))?;
        let path = entry.path();
        if path.is_dir() {
            collect_transcript_index_dir(&path, index)?;
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("jsonl") {
            if let Some(name) = path.file_name().and_then(|name| name.to_str()) {
                index.insert(name.to_string(), path);
            }
        }
    }
    Ok(())
}

fn derive_code_description(
    doc: &Value,
    transcript_index: &HashMap<String, PathBuf>,
) -> Result<Option<String>, AppError> {
    let Some(cli_session_id) = doc.get("cliSessionId").and_then(Value::as_str) else {
        return Ok(None);
    };
    let transcript_name = format!("{cli_session_id}.jsonl");
    let Some(transcript_path) = transcript_index.get(&transcript_name) else {
        return Ok(None);
    };
    derive_claude_code_description_from_transcript(transcript_path)
}

fn current_unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::{read_json_file, write_json_file};
    use serde_json::{json, Value};
    use tempfile::tempdir;

    #[test]
    fn bootstrap_cowork_matches_remote_session_id() {
        let temp = tempdir().expect("tempdir");
        let profile_dir = temp.path().join("profile");
        let sessions_dir = profile_dir
            .join("local-agent-mode-sessions")
            .join("org")
            .join("workspace");
        fs::create_dir_all(&sessions_dir).expect("create sessions dir");

        let session_path = sessions_dir.join("local_pending.json");
        write_json_file(
            &session_path,
            &json!({
                "sessionId": "local_pending",
                "initialMessage": "帮我检查 session 标题"
            }),
        )
        .expect("write pending session");

        let identity = build_identity_input(
            "local_pending".to_string(),
            Some("帮我检查 session 标题".to_string()),
            Some(hash_prompt("帮我检查 session 标题")),
            Some("帮我检查 session 标题".to_string()),
        );
        let resolved = resolve_title_target(&profile_dir, None, &identity)
            .expect("resolve should work")
            .expect("resolved session");

        assert_eq!(resolved.record.kind, LocalSessionKind::Cowork);
        assert_eq!(
            resolved.description.as_deref(),
            Some("帮我检查 session 标题")
        );
        assert_eq!(resolved.path, session_path);

        let after = read_json_file::<Value>(&resolved.path).expect("read updated doc");
        assert!(after
            .get("canonicalSessionId")
            .and_then(Value::as_str)
            .is_some());
    }

    #[test]
    fn bootstrap_code_matches_cli_session_id_and_transcript() {
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

        let identity = build_identity_input("cli-session-456".to_string(), None, None, None);
        let resolved = resolve_title_target(
            &profile_dir,
            Some(temp.path().join("projects").as_path()),
            &identity,
        )
        .expect("resolve should work")
        .expect("resolved session");

        assert_eq!(resolved.record.kind, LocalSessionKind::Code);
        assert_eq!(
            resolved.record.cli_session_id.as_deref(),
            Some("cli-session-456")
        );
        assert_eq!(resolved.description.as_deref(), Some("请帮我重构这个函数"));
        assert_eq!(resolved.path, session_path);
    }

    #[test]
    fn resolve_title_target_does_not_fallback_to_recent_session() {
        let temp = tempdir().expect("tempdir");
        let profile_dir = temp.path().join("profile");
        let sessions_dir = profile_dir
            .join("local-agent-mode-sessions")
            .join("org")
            .join("workspace");
        fs::create_dir_all(&sessions_dir).expect("create sessions dir");

        write_json_file(
            &sessions_dir.join("local_other.json"),
            &json!({
                "sessionId": "local_other",
                "initialMessage": "别的会话",
                "createdAt": current_unix_time_ms(),
                "lastActivityAt": current_unix_time_ms(),
            }),
        )
        .expect("write other session");

        let identity = build_identity_input(
            "unknown-session".to_string(),
            Some("不匹配的 prompt".to_string()),
            Some(hash_prompt("不匹配的 prompt")),
            Some("不匹配的 prompt".to_string()),
        );

        let resolved = resolve_title_target(
            &profile_dir,
            Some(temp.path().join("projects").as_path()),
            &identity,
        )
        .expect("resolve should work");

        assert!(resolved.is_none());
    }
}
