import { invoke } from "@tauri-apps/api/core";
import { Boxes, Gauge, Home, Keyboard, MonitorCog, Settings, Wrench } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import "./App.css";

type Tool = {
  id: string;
  name: string;
  description: string;
  hotkey: string;
  enabled: boolean;
  workerRunning: boolean;
};

type AppSnapshot = {
  tools: Tool[];
  coldStartupMs: number;
};

type View = "home" | "settings";

const toolIcons = [Keyboard, MonitorCog];

function App() {
  const [view, setView] = useState<View>("home");
  const [snapshot, setSnapshot] = useState<AppSnapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busyToolId, setBusyToolId] = useState<string | null>(null);

  const loadSnapshot = useCallback(async () => {
    try {
      setSnapshot(await invoke<AppSnapshot>("get_app_snapshot"));
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }, []);

  useEffect(() => {
    void loadSnapshot();
  }, [loadSnapshot]);

  async function setToolEnabled(tool: Tool, enabled: boolean) {
    setBusyToolId(tool.id);
    try {
      setSnapshot(
        await invoke<AppSnapshot>("set_tool_enabled", {
          toolId: tool.id,
          enabled,
        }),
      );
      setError(null);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusyToolId(null);
    }
  }

  const tools = snapshot?.tools ?? [];

  return (
    <div className="app-shell">
      <aside className="sidebar" aria-label="主导航">
        <div className="brand-mark" aria-hidden="true">
          <Boxes size={16} strokeWidth={2.2} />
        </div>
        <nav className="primary-nav">
          <button
            className={`nav-item ${view === "home" ? "active" : ""}`}
            onClick={() => setView("home")}
            type="button"
          >
            <Home size={15} />
            <span>首页</span>
          </button>
          <p className="nav-label">生命周期验证</p>
          {tools.map((tool, index) => {
            const Icon = toolIcons[index] ?? Wrench;
            return (
              <div className="tool-nav-item" key={tool.id}>
                <Icon size={15} />
                <span>{tool.name}</span>
                <button
                  aria-label={`${tool.enabled ? "禁用" : "启用"}${tool.name}`}
                  className={`switch ${tool.enabled ? "on" : ""}`}
                  disabled={busyToolId === tool.id}
                  onClick={() => void setToolEnabled(tool, !tool.enabled)}
                  type="button"
                >
                  <span />
                </button>
              </div>
            );
          })}
        </nav>
        <div className="sidebar-footer">
          <button
            className={`nav-item ${view === "settings" ? "active" : ""}`}
            onClick={() => setView("settings")}
            type="button"
          >
            <Settings size={15} />
            <span>设置</span>
          </button>
        </div>
      </aside>

      <main className="content">
        {view === "home" ? (
          <>
            <header className="page-header">
              <div>
                <h1>轻量化工具集</h1>
                <p>LightweightToolset</p>
              </div>
            </header>

            {error ? <div className="error-banner">{error}</div> : null}

            <section className="tool-grid" aria-label="已注册工具">
              {tools.map((tool, index) => {
                const Icon = toolIcons[index] ?? Wrench;
                return (
                  <article className={`tool-card ${tool.enabled ? "" : "disabled"}`} key={tool.id}>
                    <div className="tool-card-topline">
                      <div className="tool-icon"><Icon size={19} /></div>
                      <button
                        aria-label={`${tool.enabled ? "禁用" : "启用"}${tool.name}`}
                        className={`switch ${tool.enabled ? "on" : ""}`}
                        disabled={busyToolId === tool.id}
                        onClick={() => void setToolEnabled(tool, !tool.enabled)}
                        type="button"
                      >
                        <span />
                      </button>
                    </div>
                    <h2>{tool.name}</h2>
                    <p>{tool.description}</p>
                    <div className="tool-meta">
                      <span className={tool.workerRunning ? "state-running" : "state-stopped"}>
                        {tool.workerRunning ? "后台 worker 已启动" : "后台 worker 已停止"}
                      </span>
                      <kbd>{tool.hotkey}</kbd>
                    </div>
                  </article>
                );
              })}
            </section>

            <section className="status-strip" aria-label="基础服务状态">
              <div><Gauge size={14} /><span>基础服务运行中</span></div>
              <p>冷启动 {snapshot?.coldStartupMs ?? "--"} ms</p>
              <p>{tools.filter((tool) => tool.workerRunning).length}/{tools.length} 个工具运行中</p>
            </section>
          </>
        ) : (
          <SettingsView coldStartupMs={snapshot?.coldStartupMs ?? 0} />
        )}
      </main>
    </div>
  );
}

function SettingsView({ coldStartupMs }: { coldStartupMs: number }) {
  return (
    <>
      <header className="page-header settings-header">
        <div>
          <h1>设置</h1>
          <p>基础能力与性能基线</p>
        </div>
      </header>
      <section className="settings-panel">
        <div className="settings-row">
          <div>
            <h2>窗口服务</h2>
            <p>主窗口已按旧版外框尺寸校准；快捷弹窗、自由窗口和透明窗口能力已预留。</p>
          </div>
          <span className="setting-value">已锁定</span>
        </div>
        <div className="settings-row">
          <div>
            <h2>工具生命周期</h2>
            <p>禁用时统一注销快捷键、停止后台 worker，并关闭关联窗口。</p>
          </div>
          <span className="setting-value">已启用</span>
        </div>
        <div className="settings-row">
          <div>
            <h2>冷启动基线</h2>
            <p>首次界面快照时冻结的进程启动耗时；后续补充安装包、内存、CPU 与快捷弹窗指标。</p>
          </div>
          <span className="setting-value">{coldStartupMs} ms</span>
        </div>
      </section>
    </>
  );
}

export default App;
