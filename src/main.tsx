import { invoke } from "@tauri-apps/api/core";
import React from "react";
import ReactDOM from "react-dom/client";
import App from "./App";

/// 前端异常兜底：页面一旦白屏，内存日志读不到，这里把错误写进
/// `<存储目录>/logs/frontend.log`，并在界面上显示，避免只剩一片白。
function reportFrontendError(message: string) {
  void invoke("frontend_error_log", { message }).catch(() => {});
}

window.addEventListener("error", (event) => {
  reportFrontendError(
    `error: ${event.message} @ ${event.filename ?? "?"}:${event.lineno ?? 0}:${event.colno ?? 0}`,
  );
});

window.addEventListener("unhandledrejection", (event) => {
  const reason = event.reason instanceof Error ? `${event.reason.message}\n${event.reason.stack ?? ""}` : String(event.reason);
  reportFrontendError(`unhandledrejection: ${reason}`);
});

class AppErrorBoundary extends React.Component<
  { children: React.ReactNode },
  { message: string | null }
> {
  state: { message: string | null } = { message: null };

  static getDerivedStateFromError(error: unknown) {
    return { message: error instanceof Error ? `${error.message}\n${error.stack ?? ""}` : String(error) };
  }

  componentDidCatch(error: unknown, info: React.ErrorInfo) {
    reportFrontendError(`render: ${String(error)} | ${info.componentStack ?? ""}`);
  }

  render() {
    if (this.state.message) {
      return (
        <div style={{ padding: 24, fontFamily: "Segoe UI, Microsoft YaHei UI, sans-serif", color: "#edf2f9" }}>
          <h2 style={{ fontSize: 16, marginBottom: 12 }}>页面出错了</h2>
          <pre style={{ whiteSpace: "pre-wrap", fontSize: 12, opacity: 0.85 }}>{this.state.message}</pre>
          <p style={{ fontSize: 12, opacity: 0.7 }}>
            详细错误已写入存储目录下的 logs/frontend.log，重启软件即可恢复。
          </p>
        </div>
      );
    }
    return this.props.children;
  }
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <React.StrictMode>
    <AppErrorBoundary>
      <App />
    </AppErrorBoundary>
  </React.StrictMode>,
);
