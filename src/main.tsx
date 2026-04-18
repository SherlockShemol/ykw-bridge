import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";
import { UpdateProvider } from "./contexts/UpdateContext";
import "./index.css";
// 导入国际化配置
import i18n from "./i18n";
import { QueryClientProvider } from "@tanstack/react-query";
import { ThemeProvider } from "@/components/theme-provider";
import { queryClient } from "@/lib/query";
import { Toaster } from "@/components/ui/sonner";
import { listen } from "@tauri-apps/api/event";
import { invoke } from "@tauri-apps/api/core";
import { message } from "@tauri-apps/plugin-dialog";
import { exit } from "@tauri-apps/plugin-process";

// 根据平台添加 body class，便于平台特定样式
try {
  const ua = navigator.userAgent || "";
  const plat = (navigator.platform || "").toLowerCase();
  const isMac = /mac/i.test(ua) || plat.includes("mac");
  if (isMac) {
    document.body.classList.add("is-mac");
  }
} catch {
  // 忽略平台检测失败
}

// 配置加载错误payload类型
interface ConfigLoadErrorPayload {
  path?: string;
  error?: string;
}

function hasTauriRuntime(): boolean {
  return (
    typeof window !== "undefined" &&
    typeof (window as typeof window & { __TAURI_INTERNALS__?: unknown })
      .__TAURI_INTERNALS__ !== "undefined"
  );
}

function renderNonTauriHint(): void {
  ReactDOM.createRoot(document.getElementById("root")!).render(
    <React.StrictMode>
      <div
        style={{
          minHeight: "100vh",
          display: "flex",
          alignItems: "center",
          justifyContent: "center",
          padding: "24px",
          background: "#0f1115",
          color: "#f5f7fa",
          fontFamily:
            '-apple-system, BlinkMacSystemFont, "Segoe UI", sans-serif',
        }}
      >
        <div style={{ maxWidth: "640px", lineHeight: 1.6 }}>
          <h1 style={{ fontSize: "24px", marginBottom: "12px" }}>
            YKW Bridge needs the Tauri desktop runtime
          </h1>
          <p style={{ marginBottom: "12px", color: "#d0d7de" }}>
            This page was opened in a regular browser, so backend commands are
            unavailable. That is why calls to Tauri `invoke(...)` fail.
          </p>
          <p style={{ marginBottom: "12px", color: "#d0d7de" }}>
            Start the desktop app with:
          </p>
          <pre
            style={{
              margin: 0,
              padding: "12px 14px",
              background: "#161b22",
              borderRadius: "8px",
              overflowX: "auto",
            }}
          >
            {`cd /Users/shemol/Code/claude-desktop-switch/ykw-bridge
pnpm dev`}
          </pre>
          <p style={{ marginTop: "12px", color: "#9da7b3" }}>
            Use the YKW Bridge desktop window that Tauri opens, not the
            `localhost:3000` browser tab.
          </p>
        </div>
      </div>
    </React.StrictMode>,
  );
}

/**
 * 处理配置加载失败：显示错误消息并强制退出应用
 * 不给用户"取消"选项，因为配置损坏时应用无法正常运行
 */
async function handleConfigLoadError(
  payload: ConfigLoadErrorPayload | null,
): Promise<void> {
  const path = payload?.path ?? "~/.ykw-bridge/config.json";
  const detail = payload?.error ?? "Unknown error";

  await message(
    i18n.t("errors.configLoadFailedMessage", {
      path,
      detail,
      defaultValue:
        "无法读取配置文件：\n{{path}}\n\n错误详情：\n{{detail}}\n\n请手动检查 JSON 是否有效，或从同目录的备份文件（如 config.json.bak）恢复。\n\n应用将退出以便您进行修复。",
    }),
    {
      title: i18n.t("errors.configLoadFailedTitle", {
        defaultValue: "配置加载失败",
      }),
      kind: "error",
    },
  );

  await exit(1);
}

// 监听后端的配置加载错误事件：仅提醒用户并强制退出，不修改任何配置文件
try {
  if (hasTauriRuntime()) {
    void listen("configLoadError", async (evt) => {
      await handleConfigLoadError(evt.payload as ConfigLoadErrorPayload | null);
    });
  }
} catch (e) {
  // 忽略事件订阅异常（例如在非 Tauri 环境下）
  console.error("订阅 configLoadError 事件失败", e);
}

async function bootstrap() {
  if (!hasTauriRuntime()) {
    renderNonTauriHint();
    return;
  }

  // 启动早期主动查询后端初始化错误，避免事件竞态
  try {
    const initError = (await invoke(
      "get_init_error",
    )) as ConfigLoadErrorPayload | null;
    if (initError && (initError.path || initError.error)) {
      await handleConfigLoadError(initError);
      // 注意：不会执行到这里，因为 exit(1) 会终止进程
      return;
    }
  } catch (e) {
    // 忽略拉取错误，继续渲染
    console.error("拉取初始化错误失败", e);
  }

  ReactDOM.createRoot(document.getElementById("root")!).render(
    <React.StrictMode>
      <QueryClientProvider client={queryClient}>
        <ThemeProvider defaultTheme="system" storageKey="ykw-bridge-theme">
          <UpdateProvider>
            <App />
            <Toaster />
          </UpdateProvider>
        </ThemeProvider>
      </QueryClientProvider>
    </React.StrictMode>,
  );
}

void bootstrap();
