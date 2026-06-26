import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import {
  ChevronLeft,
  ChevronRight,
  Gauge,
  Home,
  Keyboard,
  Minus,
  MonitorCog,
  PanelLeftClose,
  PanelLeftOpen,
  Settings,
  Wrench,
  X,
} from "lucide-react";
import { type MouseEvent, useCallback, useEffect, useState } from "react";
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

type NavigationTarget = {
  view: View;
  toolId?: string;
};

const toolIcons = [Keyboard, MonitorCog];

function App() {
  const [view, setView] = useState<View>("home");
  const [activeTool, setActiveTool] = useState<Tool | null>(null);
  const [snapshot, setSnapshot] = useState<AppSnapshot | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busyToolId, setBusyToolId] = useState<string | null>(null);
  const [sidebarCollapsed, setSidebarCollapsed] = useState(false);
  const [history, setHistory] = useState<NavigationTarget[]>([{ view: "home" }]);
  const [historyIndex, setHistoryIndex] = useState(0);

  const tools = snapshot?.tools ?? [];

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
      const nextSnapshot = await invoke<AppSnapshot>("set_tool_enabled", {
        toolId: tool.id,
        enabled,
      });
      setSnapshot(nextSnapshot);
      if (activeTool?.id === tool.id) {
        setActiveTool(nextSnapshot.tools.find((nextTool) => nextTool.id === tool.id) ?? null);
      }
      setError(null);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusyToolId(null);
    }
  }

  function applyNavigation(target: NavigationTarget) {
    setView(target.view);
    setActiveTool(target.toolId ? tools.find((tool) => tool.id === target.toolId) ?? null : null);
  }

  function navigate(target: NavigationTarget) {
    const current = history[historyIndex];
    if (current.view === target.view && current.toolId === target.toolId) {
      return;
    }

    const nextHistory = [...history.slice(0, historyIndex + 1), target];
    setHistory(nextHistory);
    setHistoryIndex(nextHistory.length - 1);
    applyNavigation(target);
  }

  function navigateHistory(direction: -1 | 1) {
    const nextIndex = historyIndex + direction;
    const target = history[nextIndex];
    if (!target) {
      return;
    }

    setHistoryIndex(nextIndex);
    applyNavigation(target);
  }

  function openTool(tool: Tool) {
    navigate({ view: "tool", toolId: tool.id });
  }

  function handleTitlebarMouseDown(event: MouseEvent<HTMLElement>) {
    if (event.button !== 0 || (event.target as HTMLElement).closest("button")) {
      return;
    }
    void getCurrentWindow().startDragging();
  }

  return (
    <div className="app-shell" onContextMenu={(event) => event.preventDefault()}>
      <header className="window-chrome">
        <div className="window-drag-area" onMouseDown={handleTitlebarMouseDown}>
          <span className="window-title">轻量化工具集</span>
        </div>
        <div className="window-controls">
          <button aria-label="最小化" onClick={() => void getCurrentWindow().minimize()} type="button">
            <Minus size={14} />
          </button>
          <button aria-label="关闭" className="window-close" onClick={() => void getCurrentWindow().close()} type="button">
            <X size={15} />
          </button>
        </div>
      </header>

      <div className="app-workspace">
        <aside className={`sidebar ${sidebarCollapsed ? "collapsed" : ""}`} aria-label="主导航">
          <div className="sidebar-actions">
            <div className="history-controls" aria-label="浏览历史">
              <button aria-label="后退" disabled={historyIndex === 0} onClick={() => navigateHistory(-1)} type="button">
                <ChevronLeft size={14} />
              </button>
              <button aria-label="前进" disabled={historyIndex >= history.length - 1} onClick={() => navigateHistory(1)} type="button">
                <ChevronRight size={14} />
              </button>
            </div>
            <button
              aria-label={sidebarCollapsed ? "展开侧边栏" : "折叠侧边栏"}
              className="collapse-button"
              onClick={() => setSidebarCollapsed((collapsed) => !collapsed)}
              type="button"
            >
              {sidebarCollapsed ? <PanelLeftOpen size={16} /> : <PanelLeftClose size={16} />}
            </button>
          </div>

          <nav className="primary-nav">
            <button
              className={`nav-item ${view === "home" ? "active" : ""}`}
              onClick={() => navigate({ view: "home" })}
              title="首页"
              type="button"
            >
              <Home size={15} />
              <span>首页</span>
            </button>
            <p className="nav-label">生命周期验证</p>
            {tools.map((tool, index) => {
              const Icon = toolIcons[index] ?? Wrench;
              const isActive = view === "tool" && activeTool?.id === tool.id;
              return (
                <div
                  aria-current={isActive ? "page" : undefined}
                  className={`tool-nav-item ${isActive ? "active" : ""}`}
                  key={tool.id}
                  onClick={() => openTool(tool)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter" || event.key === " ") {
                      event.preventDefault();
                      openTool(tool);
                    }
                  }}
                  role="button"
                  tabIndex={0}
                  title={tool.name}
                >
                  <Icon size={15} />
                  <span>{tool.name}</span>
                  <button
                    aria-label={`${tool.enabled ? "禁用" : "启用"}${tool.name}`}
                    className={`switch ${tool.enabled ? "on" : ""}`}
                    disabled={busyToolId === tool.id}
                    onClick={(event) => {
                      event.stopPropagation();
                      void setToolEnabled(tool, !tool.enabled);
                    }}
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
              onClick={() => navigate({ view: "settings" })}
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
            <div className="page-enter" key="home">
              <header className="page-header home-header">
                <div>
                  <h1>轻量化工具集</h1>
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
            </div>
          ) : view === "settings" ? (
            <div className="page-enter" key="settings"><SettingsView coldStartupMs={snapshot?.coldStartMs ?? 0} /></div>
          ) : activeTool ? (
            <div className="page-enter" key={activeTool.id}><ToolPage tool={activeTool} /></div>
          ) : null}
        </main>
      </div>
    </div>
  );
}

function ToolPage({ tool }: { tool: Tool }) {
  return (
    <section className="tool-page">
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
            <p>首次界面快照冻结的进程启动耗时；后续补充安装包、内存、CPU 与快捷弹窗指标。</p>
          </div>
          <span className="setting-value">{coldStartupMs} ms</span>
        </div>
      </section>
    </>
  );
}

export default App;
