use std::str::FromStr;

use ykw_bridge_lib::AppType;

#[test]
fn parse_known_apps_case_insensitive_and_trim() {
    assert!(matches!(AppType::from_str("claude"), Ok(AppType::Claude)));
    assert!(matches!(
        AppType::from_str(" ClAuDe \n"),
        Ok(AppType::Claude)
    ));
    assert!(matches!(
        AppType::from_str("\tclaude-desktop\t"),
        Ok(AppType::ClaudeDesktop)
    ));
}

#[test]
fn parse_removed_apps_returns_error() {
    for app in ["openclaw"] {
        let err = AppType::from_str(app).expect_err("removed app should be rejected");
        let msg = err.to_string();
        assert!(msg.contains("claude"));
        assert!(msg.contains(app));
    }
}

#[test]
fn parse_unknown_app_returns_localized_error_message() {
    let err = AppType::from_str("unknown").unwrap_err();
    let msg = err.to_string();
    assert!(msg.contains("可选值") || msg.contains("Allowed"));
    assert!(msg.contains("unknown"));
}
