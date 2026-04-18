#[cfg(not(target_os = "macos"))]
use tauri::AppHandle;

#[cfg(target_os = "macos")]
mod macos {
    use std::path::Path;
    use std::process::Command;
    use std::sync::OnceLock;
    use std::time::{Duration, Instant};

    use tauri::AppHandle;

    use crate::error::AppError;

    const WATCHDOG_POLL_INTERVAL: Duration = Duration::from_millis(1000);
    const TERM_WAIT_TIMEOUT: Duration = Duration::from_millis(1200);
    const KILL_WAIT_TIMEOUT: Duration = Duration::from_millis(400);
    const RELAUNCH_DELAY: Duration = Duration::from_millis(250);

    static STARTED: OnceLock<()> = OnceLock::new();

    #[derive(Debug, Clone, PartialEq, Eq)]
    struct ClaudeProcess {
        pid: u32,
        command: String,
    }

    pub fn start_worker(app: AppHandle) {
        if STARTED.set(()).is_err() {
            return;
        }

        tauri::async_runtime::spawn(async move {
            run_loop(app).await;
        });
    }

    async fn run_loop(_app: AppHandle) {
        loop {
            if crate::settings::claude_desktop_launch_watchdog_enabled() {
                if let Err(err) = intercept_direct_launches().await {
                    log::warn!("[ClaudeDesktopWatchdog] 拦截直接启动失败: {err}");
                }
            }

            tokio::time::sleep(WATCHDOG_POLL_INTERVAL).await;
        }
    }

    async fn intercept_direct_launches() -> Result<(), AppError> {
        let binary_path = crate::claude_desktop_config::resolve_binary_path();
        let processes = list_claude_main_processes(&binary_path)?;
        let direct_launches: Vec<ClaudeProcess> = processes
            .into_iter()
            .filter(|process| !is_managed_command(&process.command))
            .collect();

        if direct_launches.is_empty() {
            return Ok(());
        }

        for process in &direct_launches {
            log::warn!(
                "[ClaudeDesktopWatchdog] 检测到直接启动的 Claude Desktop，准备接管: pid={}, command={}",
                process.pid,
                process.command
            );
            signal_process(process.pid, "-TERM")?;
        }

        for process in &direct_launches {
            wait_for_exit(process.pid, TERM_WAIT_TIMEOUT).await;
            if process_exists(process.pid) {
                log::warn!(
                    "[ClaudeDesktopWatchdog] Claude Desktop 进程未及时退出，升级为 SIGKILL: pid={}",
                    process.pid
                );
                signal_process(process.pid, "-KILL")?;
                wait_for_exit(process.pid, KILL_WAIT_TIMEOUT).await;
            }
        }

        tokio::time::sleep(RELAUNCH_DELAY).await;
        crate::claude_desktop_config::launch_app()?;
        Ok(())
    }

    fn list_claude_main_processes(binary_path: &Path) -> Result<Vec<ClaudeProcess>, AppError> {
        let output = Command::new("ps")
            .arg("-ax")
            .arg("-o")
            .arg("pid=,command=")
            .output()
            .map_err(|e| AppError::Message(format!("读取 Claude Desktop 进程失败: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(AppError::Message(format!(
                "读取 Claude Desktop 进程失败: {}",
                if stderr.is_empty() {
                    "ps 返回非零状态".to_string()
                } else {
                    stderr
                }
            )));
        }

        Ok(parse_claude_main_processes_from_ps(
            &String::from_utf8_lossy(&output.stdout),
            binary_path,
        ))
    }

    fn parse_claude_main_processes_from_ps(raw: &str, binary_path: &Path) -> Vec<ClaudeProcess> {
        let binary = binary_path.to_string_lossy();
        raw.lines()
            .filter_map(parse_ps_line)
            .filter(|(_, command)| command.starts_with(binary.as_ref()))
            .map(|(pid, command)| ClaudeProcess {
                pid,
                command: command.to_string(),
            })
            .collect()
    }

    fn parse_ps_line(line: &str) -> Option<(u32, &str)> {
        let trimmed = line.trim_start();
        if trimmed.is_empty() {
            return None;
        }

        let split_at = trimmed.find(char::is_whitespace)?;
        let pid = trimmed[..split_at].parse().ok()?;
        let command = trimmed[split_at..].trim_start();
        if command.is_empty() {
            return None;
        }

        Some((pid, command))
    }

    fn is_managed_command(command: &str) -> bool {
        command.split_whitespace().any(|part| part == "-3p")
    }

    fn signal_process(pid: u32, signal: &str) -> Result<(), AppError> {
        let status = Command::new("kill")
            .arg(signal)
            .arg(pid.to_string())
            .status()
            .map_err(|e| AppError::Message(format!("结束 Claude Desktop 进程失败: {e}")))?;

        if status.success() {
            Ok(())
        } else {
            Err(AppError::Message(format!(
                "结束 Claude Desktop 进程失败: kill {signal} {pid} 返回非零状态"
            )))
        }
    }

    fn process_exists(pid: u32) -> bool {
        Command::new("kill")
            .arg("-0")
            .arg(pid.to_string())
            .status()
            .map(|status| status.success())
            .unwrap_or(false)
    }

    async fn wait_for_exit(pid: u32, timeout: Duration) {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            if !process_exists(pid) {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }

    #[cfg(test)]
    mod tests {
        use std::path::PathBuf;

        use super::{
            is_managed_command, parse_claude_main_processes_from_ps, parse_ps_line, ClaudeProcess,
        };

        #[test]
        fn parse_ps_line_extracts_pid_and_command() {
            let parsed = parse_ps_line(
                " 43210 /Applications/Claude.app/Contents/MacOS/Claude -3p --inspect=0",
            );

            assert_eq!(
                parsed,
                Some((
                    43210,
                    "/Applications/Claude.app/Contents/MacOS/Claude -3p --inspect=0"
                ))
            );
        }

        #[test]
        fn parse_claude_processes_only_keeps_main_binary() {
            let binary = PathBuf::from("/Applications/Claude.app/Contents/MacOS/Claude");
            let parsed = parse_claude_main_processes_from_ps(
                "123 /Applications/Claude.app/Contents/MacOS/Claude\n\
                 124 /Applications/Claude.app/Contents/MacOS/Claude -3p\n\
                 125 /Applications/Claude.app/Contents/Frameworks/Claude Helper.app/Contents/MacOS/Claude Helper\n\
                 126 /opt/other-app",
                &binary,
            );

            assert_eq!(
                parsed,
                vec![
                    ClaudeProcess {
                        pid: 123,
                        command: "/Applications/Claude.app/Contents/MacOS/Claude".to_string(),
                    },
                    ClaudeProcess {
                        pid: 124,
                        command: "/Applications/Claude.app/Contents/MacOS/Claude -3p".to_string(),
                    }
                ]
            );
        }

        #[test]
        fn managed_command_detection_requires_3p_flag() {
            assert!(is_managed_command(
                "/Applications/Claude.app/Contents/MacOS/Claude -3p"
            ));
            assert!(!is_managed_command(
                "/Applications/Claude.app/Contents/MacOS/Claude --inspect=0"
            ));
        }
    }
}

#[cfg(target_os = "macos")]
pub use macos::start_worker;

#[cfg(not(target_os = "macos"))]
pub fn start_worker(_app: AppHandle) {}
