import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { ChevronLeft, Gauge, Home, Keyboard, Minus, MonitorCog, PanelLeftClose, PanelLeftOpen, Settings, Wrench, X } from "lucide-react";
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
  coldStartMs: number;
};

type View = "home" | "settings" | "tool";

const toolIcons = [Keyboard, MonitorCog];

function App() {
  const [view, setView] = useState<View>("home");
  const [activeTool, setActiveTool] = useState<Tool | null>(null);
  const [snapshot, setSnapshot] = useState<AppSnapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busyToolId, setBusyToolId] = useState<string | null>(null);
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);

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

  function openTool(tool: Tool) {
    setActiveTool(tool);
    setView("tool");
  }

  const tools = snapshot?.tools ?? [];

  return (
    <div className="app-shell">
      <header className="window-chrome" data-tauri-drag-region>
        <span className="window-title">LightweightToolset</span>
        <div className="window-controls" onMouseDown={(event) => event.stopPropagation()}>
          <button aria-label="最小化" onClick={() => void getCurrentWindow().minimize()} type="button"><Minus size={14} /></button>
          <button aria-label="关闭" className="window-close" onClick={() => void getCurrentWindow().close()} type="button"><X size={15} /></button>
        </div>
      </header>

      <div className="app-workspace">
        <aside className={`sidebar ${sidebarCollapsed ? "collapsed" : ""}`} aria-label="主导航">
          <button
            aria-label={sidebarCollapsed ? "展开侧边栏" : "折叠侧边栏"}
            className="collapse-button"
            onClick={() => setSidebarCollapsed((collapsed) => !collapsed)}
            type="button"
          >
            {sidebarCollapsed ? <PanelLeftOpen size={16} /> : <PanelLeftClose size={16} />}
          </button>
          <nav className="primary-nav">
            <button
              className={`nav-item ${view === "home" ? "active" : ""}`}
              onClick={() => setView("home")}
              title="首页"
              type="button"
            >
              <Home size={15} />
              <span>首页</span>
            </button>
            <p className="nav-label">生命周期验证</p>
            {tools.map((tool, index) => {
              const Icon = toolIcons[index] ?? Wrench;
              return (
                <div className="tool-nav-item" key={tool.id} title={tool.name}>
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
              title="设置"
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
                    <button className={`tool-card ${tool.enabled ? "" : "disabled"}`} key={tool.id} onClick={() => openTool(tool)} type="button">
                      <div className="tool-card-heading">
                        <div className="tool-icon"><Icon size={19} /></div>
                        <h2>{tool.name}</h2>
                      </div>
                      <p>{tool.description}</p>
                      <div className="tool-meta">
                        <span className={tool.workerRunning ? "state-running" : "state-stopped"}>
                          {tool.workerRunning ? "后台 worker 已启动" : "后台 worker 已停止"}
                        </span>
                        <kbd>{tool.hotkey}</kbd>
                      </div>
                    </button>
                  );
                })}
              </section>

              <section className="status-strip" aria-label="基础服务状态">
                <div><Gauge size={14} /><span>基础服务运行中</span></div>
                <p>冷启动 {snapshot?.coldStartMs ?? "--"} ms</p>
                <p>{tools.filter((tool) => tool.workerRunning).length}/{tools.length} 个工具运行中</p>
              </section>
            </>
          ) : view === "settings" ? (
            <SettingsView coldStartupMs={snapshot?.coldStartMs ?? 0} />
          ) : activeTool ? (
            <ToolPage onBack={() => setView("home")} tool={activeTool} />
          ) : null}
        </main>
      </div>
    </div>
  );
}

function ToolPage({ onBack, tool }: { onBack: () => void; tool: Tool }) {
  return (
    <section className="tool-page">
      <button className="back-button" onClick={onBack} type="button"><ChevronLeft size={14} /> 返回工具总览</button>
      <h1>{tool.name}</h1>
      <p>{tool.description}</p>
      <div className="tool-page-status">
        <span className={tool.workerRunning ? "state-running" : "state-stopped"}>{tool.workerRunning ? "后台 worker 已启动" : "后台 worker 已停止"}</span>
        <kbd>{tool.hotkey}</kbd>
      </div>
    </section>
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
