use serde_json::json;
use std::fs;
use std::path::PathBuf;

use ykw_bridge_lib::{
    get_claude_settings_path, read_json_file, AppError, AppType, ConfigService, MultiAppConfig,
    Provider,
};

#[path = "support.rs"]
mod support;
use support::{
    create_test_state, create_test_state_with_config, ensure_test_home, reset_test_fs, test_mutex,
};

#[test]
fn sync_claude_provider_writes_live_settings() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let mut config = MultiAppConfig::default();
    let provider_config = json!({
        "env": {
            "ANTHROPIC_AUTH_TOKEN": "test-key",
            "ANTHROPIC_BASE_URL": "https://api.test"
        },
        "ui": {
            "displayName": "Test Provider"
        }
    });

    let provider = Provider::with_id(
        "prov-1".to_string(),
        "Test Claude".to_string(),
        provider_config.clone(),
        None,
    );

    let manager = config
        .get_manager_mut(&AppType::Claude)
        .expect("claude manager");
    manager.providers.insert("prov-1".to_string(), provider);
    manager.current = "prov-1".to_string();

    ConfigService::sync_current_providers_to_live(&mut config).expect("sync live settings");

    let settings_path = get_claude_settings_path();
    assert!(
        settings_path.exists(),
        "live settings should be written to {}",
        settings_path.display()
    );

    let live_value: serde_json::Value = read_json_file(&settings_path).expect("read live file");
    assert_eq!(live_value, provider_config);

    // 确认 SSOT 中的供应商也同步了最新内容
    let updated = config
        .get_manager(&AppType::Claude)
        .and_then(|m| m.providers.get("prov-1"))
        .expect("provider in config");
    assert_eq!(updated.settings_config, provider_config);

    // 额外确认写入位置位于测试 HOME 下
    assert!(
        settings_path.starts_with(home),
        "settings path {settings_path:?} should reside under test HOME {home:?}"
    );
}

#[test]
fn sync_claude_enabled_mcp_projects_to_user_config() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    // 模拟 Claude 已安装/已初始化：存在 ~/.claude 目录
    fs::create_dir_all(home.join(".claude")).expect("create claude dir");

    let mut config = MultiAppConfig::default();

    config.mcp.claude.servers.insert(
        "stdio-enabled".into(),
        json!({
            "id": "stdio-enabled",
            "enabled": true,
            "server": {
                "type": "stdio",
                "command": "echo",
                "args": ["hi"],
            }
        }),
    );
    config.mcp.claude.servers.insert(
        "http-disabled".into(),
        json!({
            "id": "http-disabled",
            "enabled": false,
            "server": {
                "type": "http",
                "url": "https://example.com",
            }
        }),
    );

    ykw_bridge_lib::sync_enabled_to_claude(&config).expect("sync Claude MCP");

    let claude_path = ykw_bridge_lib::get_claude_mcp_path();
    assert!(claude_path.exists(), "claude config should exist");
    let text = fs::read_to_string(&claude_path).expect("read .claude.json");
    let value: serde_json::Value = serde_json::from_str(&text).expect("parse claude json");
    let servers = value
        .get("mcpServers")
        .and_then(|v| v.as_object())
        .expect("mcpServers map");
    assert_eq!(servers.len(), 1, "only enabled entries should be written");
    let enabled = servers.get("stdio-enabled").expect("enabled entry");
    assert_eq!(
        enabled
            .get("command")
            .and_then(|v| v.as_str())
            .unwrap_or_default(),
        "echo"
    );
    assert!(servers.get("http-disabled").is_none());
}

#[test]
fn import_from_claude_merges_into_config() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let claude_path = home.join(".claude.json");

    fs::write(
        &claude_path,
        serde_json::to_string_pretty(&json!({
            "mcpServers": {
                "stdio-enabled": {
                    "type": "stdio",
                    "command": "echo",
                    "args": ["hello"]
                }
            }
        }))
        .unwrap(),
    )
    .expect("write claude json");

    let mut config = MultiAppConfig::default();
    // v3.7.0: 在统一结构中创建已存在的服务器
    config.mcp.servers = Some(std::collections::HashMap::new());
    config.mcp.servers.as_mut().unwrap().insert(
        "stdio-enabled".to_string(),
        ykw_bridge_lib::McpServer {
            id: "stdio-enabled".to_string(),
            name: "stdio-enabled".to_string(),
            server: json!({
                "type": "stdio",
                "command": "prev"
            }),
            apps: ykw_bridge_lib::McpApps {
                claude: false, // 初始未启用
                codex: false,
                gemini: false,
            },
            description: None,
            homepage: None,
            docs: None,
            tags: Vec::new(),
        },
    );

    let changed = ykw_bridge_lib::import_from_claude(&mut config).expect("import from claude");
    assert!(changed >= 1, "should mark at least one change");

    // v3.7.0: 检查统一结构
    let entry = config
        .mcp
        .servers
        .as_ref()
        .unwrap()
        .get("stdio-enabled")
        .expect("entry exists");

    // 验证 Claude 应用已启用
    assert!(
        entry.apps.claude,
        "Claude app should be enabled after import"
    );

    // 验证现有配置被保留（server 不应被覆盖）
    let server = entry.server.as_object().expect("server obj");
    assert_eq!(
        server.get("command").and_then(|v| v.as_str()).unwrap_or(""),
        "prev",
        "existing server config should be preserved"
    );
}

#[test]
fn create_backup_skips_missing_file() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let config_path = home.join(".ykw-bridge").join("config.json");

    // 未创建文件时应返回空字符串，不报错
    let result = ConfigService::create_backup(&config_path).expect("create backup");
    assert!(
        result.is_empty(),
        "expected empty backup id when config file missing"
    );
}

#[test]
fn create_backup_generates_snapshot_file() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let config_dir = home.join(".ykw-bridge");
    let config_path = config_dir.join("config.json");
    fs::create_dir_all(&config_dir).expect("prepare config dir");
    fs::write(&config_path, r#"{"version":2}"#).expect("write config file");

    let backup_id = ConfigService::create_backup(&config_path).expect("backup success");
    assert!(
        !backup_id.is_empty(),
        "backup id should contain timestamp information"
    );

    let backup_path = config_dir.join("backups").join(format!("{backup_id}.json"));
    assert!(
        backup_path.exists(),
        "expected backup file at {}",
        backup_path.display()
    );

    let backup_content = fs::read_to_string(&backup_path).expect("read backup");
    assert!(
        backup_content.contains(r#""version":2"#),
        "backup content should match original config"
    );
}

#[test]
fn create_backup_retains_only_latest_entries() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();
    let config_dir = home.join(".ykw-bridge");
    let config_path = config_dir.join("config.json");
    fs::create_dir_all(&config_dir).expect("prepare config dir");
    fs::write(&config_path, r#"{"version":3}"#).expect("write config file");

    let backups_dir = config_dir.join("backups");
    fs::create_dir_all(&backups_dir).expect("create backups dir");
    for idx in 0..12 {
        let manual = backups_dir.join(format!("manual_{idx:02}.json"));
        fs::write(&manual, format!("{{\"idx\":{idx}}}")).expect("seed manual backup");
    }

    std::thread::sleep(std::time::Duration::from_secs(1));

    let latest_backup_id =
        ConfigService::create_backup(&config_path).expect("create backup with cleanup");
    assert!(
        !latest_backup_id.is_empty(),
        "backup id should not be empty when config exists"
    );

    let entries: Vec<_> = fs::read_dir(&backups_dir)
        .expect("read backups dir")
        .filter_map(|entry| entry.ok())
        .collect();
    assert!(
        entries.len() <= 10,
        "expected backups to be trimmed to at most 10 files, got {}",
        entries.len()
    );

    let latest_path = backups_dir.join(format!("{latest_backup_id}.json"));
    assert!(
        latest_path.exists(),
        "latest backup {} should be preserved",
        latest_path.display()
    );

    // 进一步确认保留的条目包含一些历史文件，说明清理逻辑仅裁剪多余部分
    let manual_kept = entries
        .iter()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .any(|name| name.starts_with("manual_"));
    assert!(
        manual_kept,
        "cleanup should keep part of the older backups to maintain history"
    );
}

#[test]
fn export_sql_writes_to_target_path() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    // Create test state with some data
    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Claude)
            .expect("claude manager");
        manager.current = "test-provider".to_string();
        manager.providers.insert(
            "test-provider".to_string(),
            Provider::with_id(
                "test-provider".to_string(),
                "Test Provider".to_string(),
                json!({"env": {"ANTHROPIC_API_KEY": "test-key"}}),
                None,
            ),
        );
    }

    let state = create_test_state_with_config(&config).expect("create test state");

    // Export to SQL file
    let export_path = home.join("test-export.sql");
    state
        .db
        .export_sql(&export_path)
        .expect("export should succeed");

    // Verify file exists and contains data
    assert!(export_path.exists(), "export file should exist");
    let content = fs::read_to_string(&export_path).expect("read exported file");
    assert!(
        content.contains("INSERT INTO") && content.contains("providers"),
        "exported SQL should contain INSERT statements for providers"
    );
    assert!(
        content.contains("test-provider"),
        "exported SQL should contain test data"
    );
}

#[test]
fn export_sql_returns_error_for_invalid_path() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let _home = ensure_test_home();

    let state = create_test_state().expect("create test state");

    // Try to export to an invalid path (nonexistent parent or invalid name on Windows)
    let invalid_parent = if cfg!(windows) {
        std::env::temp_dir().join("ykw-bridge-test-invalid<>dir")
    } else {
        PathBuf::from("/nonexistent/directory")
    };
    let invalid_path = invalid_parent.join("export.sql");
    let err = state
        .db
        .export_sql(&invalid_path)
        .expect_err("export to invalid path should fail");
    let invalid_prefix = invalid_parent.to_string_lossy();

    // The error can be either IoContext or Io depending on where it fails
    match err {
        AppError::IoContext { context, .. } => {
            assert!(
                context.contains("原子写入失败") || context.contains("写入失败"),
                "expected IO error message about atomic write failure, got: {context}"
            );
        }
        AppError::Io { path, .. } => {
            assert!(
                path.starts_with(invalid_prefix.as_ref()),
                "expected error for {invalid_parent:?}, got: {path:?}"
            );
        }
        other => panic!("expected IoContext or Io error, got {other:?}"),
    }
}

#[test]
fn import_sql_rejects_non_cc_switch_backup() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    let state = create_test_state().expect("create test state");

    let import_path = home.join("not-ykw-bridge.sql");
    fs::write(&import_path, "CREATE TABLE x (id INTEGER);").expect("write import sql");

    let err = state
        .db
        .import_sql(&import_path)
        .expect_err("non-ykw-bridge sql should be rejected");

    match err {
        AppError::Localized { key, .. } => {
            assert_eq!(key, "backup.sql.invalid_format");
        }
        other => panic!("expected Localized error, got {other:?}"),
    }
}

#[test]
fn import_sql_accepts_cc_switch_exported_backup() {
    let _guard = test_mutex().lock().expect("acquire test mutex");
    reset_test_fs();
    let home = ensure_test_home();

    // Create a database with some data and export it.
    let mut config = MultiAppConfig::default();
    {
        let manager = config
            .get_manager_mut(&AppType::Claude)
            .expect("claude manager");
        manager.current = "test-provider".to_string();
        manager.providers.insert(
            "test-provider".to_string(),
            Provider::with_id(
                "test-provider".to_string(),
                "Test Provider".to_string(),
                json!({"env": {"ANTHROPIC_API_KEY": "test-key"}}),
                None,
            ),
        );
    }

    let state = create_test_state_with_config(&config).expect("create test state");
    let export_path = home.join("ykw-bridge-export.sql");
    state
        .db
        .export_sql(&export_path)
        .expect("export should succeed");

    // Reset database, then import into a fresh one.
    reset_test_fs();
    let state = create_test_state().expect("create test state");
    state
        .db
        .import_sql(&export_path)
        .expect("import should succeed");

    let providers = state
        .db
        .get_all_providers(AppType::Claude.as_str())
        .expect("load providers");
    assert!(
        providers.contains_key("test-provider"),
        "imported providers should contain test-provider"
    );
}
