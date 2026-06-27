import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  ChevronLeft,
  ChevronRight,
  Gauge,
  FolderOpen,
  Home,
  Info,
  Keyboard,
  Minus,
  Monitor,
  MonitorCog,
  Moon,
  Pencil,
  PanelLeftClose,
  PanelLeftOpen,
  RefreshCw,
  RotateCcw,
  Settings,
  ShieldCheck,
  Sun,
  Wrench,
  X,
} from "lucide-react";
import { type MouseEvent, useCallback, useEffect, useState } from "react";
import "./App.css";

type ThemeMode = "system" | "light" | "dark";
type CloseBehavior = "quit" | "tray";
type SettingsTab = "general" | "hotkey" | "about";

type Tool = {
  id: string;
  name: string;
  description: string;
  hotkey: string;
  enabled: boolean;
  workerRunning: boolean;
};

type AppSettings = {
  tools: Record<string, boolean>;
  theme: ThemeMode;
  autoStart: boolean;
  autoCheckUpdates: boolean;
  showUpdateNotification: boolean;
  windowTitle: string;
  closeBehavior: CloseBehavior;
  developerMode: boolean;
  storagePath: string;
};

type SettingsPatch = Partial<Omit<AppSettings, "tools" | "autoStart">>;

type AppSnapshot = {
  tools: Tool[];
  coldStartMs: number;
  settings: AppSettings;
};

type View = "home" | "settings" | "tool";

type NavigationTarget = {
  view: View;
  toolId?: string;
};

const DEFAULT_TITLE = "轻量化工具集";
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
  const [contentScrolled, setContentScrolled] = useState(false);

  const tools = snapshot?.tools ?? [];
  const settings = snapshot?.settings;
  const windowTitle = settings?.windowTitle || DEFAULT_TITLE;
  const isHistoryBackAvailable = canNavigateHistory(-1);
  const isHistoryForwardAvailable = canNavigateHistory(1);

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

  useEffect(() => {
    const theme = settings?.theme ?? "system";
    const resolved = theme === "system"
      ? (window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light")
      : theme;
    document.documentElement.dataset.theme = resolved;
  }, [settings?.theme]);

  async function setToolEnabled(tool: Tool, enabled: boolean) {
    setBusyToolId(tool.id);
    try {
      const nextSnapshot = await invoke<AppSnapshot>("set_tool_enabled", {
        toolId: tool.id,
        enabled,
      });
      setSnapshot(nextSnapshot);
      if (!enabled) {
        setHistory((entries) => entries.map((entry) => (entry.view === "tool" && entry.toolId === tool.id ? { view: "home" } : entry)));
      }
      if (activeTool?.id === tool.id) {
        const nextTool = nextSnapshot.tools.find((candidate) => candidate.id === tool.id) ?? null;
        if (nextTool?.enabled) {
          setActiveTool(nextTool);
        } else {
          setActiveTool(null);
          applyNavigation({ view: "home" }, nextSnapshot.tools);
        }
      }
      setError(null);
    } catch (reason) {
      setError(String(reason));
    } finally {
      setBusyToolId(null);
    }
  }

  async function updateSettings(patch: SettingsPatch) {
    try {
      setSnapshot(await invoke<AppSnapshot>("update_app_settings", { patch }));
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function setAutoStartEnabled(enabled: boolean) {
    try {
      setSnapshot(await invoke<AppSnapshot>("set_auto_start_enabled", { enabled }));
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }

  function isNavigationAllowed(target: NavigationTarget, sourceTools = tools) {
    if (target.view !== "tool") {
      return true;
    }
    return Boolean(target.toolId && sourceTools.some((tool) => tool.id === target.toolId && tool.enabled));
  }

  function applyNavigation(target: NavigationTarget, sourceTools = tools) {
    if (!isNavigationAllowed(target, sourceTools)) {
      return false;
    }
    setView(target.view);
    setActiveTool(target.toolId ? sourceTools.find((tool) => tool.id === target.toolId && tool.enabled) ?? null : null);
    return true;
  }

  function navigate(target: NavigationTarget) {
    if (!isNavigationAllowed(target)) {
      return;
    }
    const current = history[historyIndex];
    if (current.view === target.view && current.toolId === target.toolId) {
      return;
    }

    const nextHistory = [...history.slice(0, historyIndex + 1), target];
    setHistory(nextHistory);
    setHistoryIndex(nextHistory.length - 1);
    applyNavigation(target);
  }

  function findNavigableHistoryIndex(direction: -1 | 1) {
    for (let index = historyIndex + direction; index >= 0 && index < history.length; index += direction) {
      if (isNavigationAllowed(history[index])) {
        return index;
      }
    }
    return -1;
  }

  function canNavigateHistory(direction: -1 | 1) {
    return findNavigableHistoryIndex(direction) !== -1;
  }

  function navigateHistory(direction: -1 | 1) {
    const nextIndex = findNavigableHistoryIndex(direction);
    if (nextIndex === -1) {
      return;
    }
    const target = history[nextIndex];
    if (!target) {
      return;
    }

    setHistoryIndex(nextIndex);
    applyNavigation(target);
  }

  function openTool(tool: Tool) {
    if (!tool.enabled) {
      return;
    }
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
          <span className="window-title">{windowTitle}</span>
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
              <button aria-label="后退" disabled={!isHistoryBackAvailable} onClick={() => navigateHistory(-1)} type="button">
                <ChevronLeft size={14} />
              </button>
              <button aria-label="前进" disabled={!isHistoryForwardAvailable} onClick={() => navigateHistory(1)} type="button">
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
                  aria-disabled={!tool.enabled}
                  className={`tool-nav-item ${isActive ? "active" : ""} ${tool.enabled ? "" : "disabled"}`}
                  key={tool.id}
                  onClick={() => openTool(tool)}
                  onKeyDown={(event) => {
                    if (event.key === "Enter" || event.key === " ") {
                      event.preventDefault();
                      openTool(tool);
                    }
                  }}
                  role="button"
                  tabIndex={tool.enabled ? 0 : -1}
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

        <main
          className={`content ${view === "settings" ? "settings-content" : ""} ${contentScrolled ? "scrolled" : ""}`}
          onScroll={(event) => setContentScrolled(event.currentTarget.scrollTop > 42)}
        >
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
                    <button className={`tool-card ${tool.enabled ? "" : "disabled"}`} disabled={!tool.enabled} key={tool.id} onClick={() => openTool(tool)} type="button">
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
          ) : view === "settings" && settings ? (
            <div className="page-enter" key="settings">
              <SettingsView
                coldStartupMs={snapshot?.coldStartMs ?? 0}
                settings={settings}
                setAutoStartEnabled={setAutoStartEnabled}
                tools={tools}
                updateSettings={updateSettings}
              />
            </div>
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

function SettingsView({
  coldStartupMs,
  settings,
  setAutoStartEnabled,
  tools,
  updateSettings,
}: {
  coldStartupMs: number;
  settings: AppSettings;
  setAutoStartEnabled: (enabled: boolean) => Promise<void>;
  tools: Tool[];
  updateSettings: (patch: SettingsPatch) => Promise<void>;
}) {
  const [activeTab, setActiveTab] = useState<SettingsTab>("general");
  const [titleDraft, setTitleDraft] = useState(settings.windowTitle);
  const [storagePathDraft, setStoragePathDraft] = useState(settings.storagePath);
  const [defaultStoragePath, setDefaultStoragePath] = useState("");
  const titleChanged = titleDraft.trim() !== settings.windowTitle;
  const titleResetVisible = titleDraft.trim() !== DEFAULT_TITLE || settings.windowTitle !== DEFAULT_TITLE;
  const storageChanged = storagePathDraft.trim() !== settings.storagePath;

  useEffect(() => setTitleDraft(settings.windowTitle), [settings.windowTitle]);
  useEffect(() => setStoragePathDraft(settings.storagePath), [settings.storagePath]);
  useEffect(() => {
    void invoke<string>("get_default_storage_path")
      .then(setDefaultStoragePath)
      .catch(() => setDefaultStoragePath(""));
  }, []);

  function openStoragePath() {
    void invoke("open_storage_path", { storagePath: storagePathDraft });
  }

  async function changeStoragePath() {
    const selected = await open({
      defaultPath: storagePathDraft || defaultStoragePath || undefined,
      directory: true,
      multiple: false,
      title: "选择存储路径",
    });
    if (typeof selected === "string") {
      setStoragePathDraft(selected);
    }
  }

  function restoreDefaultStoragePath() {
    setStoragePathDraft("");
    void updateSettings({ storagePath: "" });
  }

  return (
    <>
      <header className="page-header settings-header">
        <div>
          <h1>设置</h1>
        </div>
      </header>
      <div className="settings-tabs" role="tablist" aria-label="设置分类">
        <SettingsTabButton active={activeTab === "general"} icon={<Monitor size={14} />} label="通用" onClick={() => setActiveTab("general")} />
        <SettingsTabButton active={activeTab === "hotkey"} icon={<Keyboard size={14} />} label="快捷键" onClick={() => setActiveTab("hotkey")} />
        <SettingsTabButton active={activeTab === "about"} icon={<Info size={14} />} label="关于" onClick={() => setActiveTab("about")} />
      </div>

      <section className="settings-panel">
        {activeTab === "general" ? (
          <div className="settings-section page-enter">
            <div className="settings-row stack">
              <div>
                <h2>主窗口标题</h2>
                <p>显示在窗口标题栏的文字</p>
              </div>
              <div className="settings-inline">
                <input className="settings-input" value={titleDraft} onChange={(event) => setTitleDraft(event.target.value)} />
                {titleChanged ? (
                  <button className="primary-action" onClick={() => updateSettings({ windowTitle: titleDraft })} type="button">保存</button>
                ) : null}
                {titleResetVisible ? (
                  <button className="icon-action" aria-label="重置标题" onClick={() => updateSettings({ windowTitle: DEFAULT_TITLE })} type="button"><RefreshCw size={13} /></button>
                ) : null}
              </div>
            </div>
            <ToggleRow
              checked={settings.autoStart}
              description="登录 Windows 后自动启动；关闭时会同步取消自启"
              label="开机自启"
              onChange={(value) => setAutoStartEnabled(value)}
            />
            <div className="settings-row">
              <div>
                <h2>主题模式</h2>
                <p>切换深色/浅色外观</p>
              </div>
              <div className="segmented">
                <button className={settings.theme === "system" ? "active" : ""} onClick={() => updateSettings({ theme: "system" })} type="button"><Monitor size={13} />跟随系统</button>
                <button className={settings.theme === "light" ? "active" : ""} onClick={() => updateSettings({ theme: "light" })} type="button"><Sun size={13} />浅色</button>
                <button className={settings.theme === "dark" ? "active" : ""} onClick={() => updateSettings({ theme: "dark" })} type="button"><Moon size={13} />深色</button>
              </div>
            </div>
            <div className="settings-row">
              <div>
                <h2>关闭应用时</h2>
                <p>点击关闭按钮的行为</p>
              </div>
              <select className="settings-select" value={settings.closeBehavior} onChange={(event) => updateSettings({ closeBehavior: event.target.value as CloseBehavior })}>
                <option value="quit">直接退出</option>
                <option value="tray">缩小到托盘</option>
              </select>
            </div>
            <ToggleRow
              checked={settings.developerMode}
              description="后续用于显示诊断日志与开发辅助入口"
              label="开发者模式"
              onChange={(value) => updateSettings({ developerMode: value })}
            />
            <div className="settings-row stack">
              <div>
                <h2>存储路径</h2>
                <p>{defaultStoragePath ? `默认目录：${defaultStoragePath}` : "默认使用应用配置目录；可打开当前目录或恢复默认"}</p>
              </div>
              <div className="settings-inline wide">
                <input className="settings-input mono" placeholder="默认应用配置目录" value={storagePathDraft} onChange={(event) => setStoragePathDraft(event.target.value)} />
                {storageChanged ? (
                  <button className="primary-action" onClick={() => updateSettings({ storagePath: storagePathDraft })} type="button">保存</button>
                ) : null}
                <button className="secondary-action icon-text-action" onClick={() => void changeStoragePath()} type="button"><FolderOpen size={13} />更改</button>
                <button className="secondary-action" onClick={openStoragePath} type="button">打开</button>
                {settings.storagePath ? (
                  <button className="icon-action" aria-label="恢复默认存储路径" onClick={restoreDefaultStoragePath} type="button"><RotateCcw size={13} /></button>
                ) : null}
              </div>
            </div>
          </div>
        ) : null}

        {activeTab === "hotkey" ? (
          <div className="settings-section page-enter">
            {tools.map((tool) => (
              <div className="settings-row hotkey-row" key={tool.id}>
                <div>
                  <h2>{tool.name}</h2>
                  <p>{tool.enabled ? "当前快捷键已注册；后续编辑时会先做冲突检查再保存" : "工具已禁用；快捷键已释放，启用后才允许编辑"}</p>
                </div>
                <div className="hotkey-controls">
                  <kbd>{tool.hotkey}</kbd>
                  <span className="setting-value neutral"><ShieldCheck size={12} />无冲突</span>
                  <button className="secondary-action planned-action" disabled type="button"><Pencil size={12} />编辑</button>
                </div>
              </div>
            ))}
            <p className="settings-note">本页已固定为“当前值 + 冲突状态 + 编辑入口”的结构；快捷键改写会在真实工具迁移时接入统一注册器，避免先做只改 UI 的伪编辑。</p>
          </div>
        ) : null}

        {activeTab === "about" ? (
          <div className="settings-section page-enter">
            <InfoRow label="应用" value="LightweightToolset" />
            <InfoRow label="作者" value="THE2580" />
            <InfoRow label="版本" value="0.1.0" />
            <div className="settings-row">
              <div>
                <h2>GitHub</h2>
              </div>
              <button
                className="link-action"
                onClick={() => void openUrl("https://github.com/THE2580/LightweightToolset")}
                type="button"
              >
                github.com/THE2580/LightweightToolset
              </button>
            </div>
            <ToggleRow
              checked={settings.autoCheckUpdates}
              description="仅保存偏好并保留更新入口；当前不接 release 更新器"
              label="自动检查更新"
              onChange={(value) => updateSettings({ autoCheckUpdates: value })}
            />
            {settings.autoCheckUpdates ? (
              <ToggleRow
                checked={settings.showUpdateNotification}
                description="仅作为后续更新通道的提示策略预留"
                label="新版本提示弹窗"
                onChange={(value) => updateSettings({ showUpdateNotification: value })}
              />
            ) : null}
            <div className="settings-row">
              <div>
                <h2>冷启动基线</h2>
                <p>首次界面快照冻结的进程启动耗时</p>
              </div>
              <span className="setting-value">{coldStartupMs} ms</span>
            </div>
          </div>
        ) : null}
      </section>
    </>
  );
}

function SettingsTabButton({ active, icon, label, onClick }: { active: boolean; icon: React.ReactNode; label: string; onClick: () => void }) {
  return (
    <button className={`settings-tab ${active ? "active" : ""}`} onClick={onClick} role="tab" type="button">
      {icon}
      <span>{label}</span>
    </button>
  );
}

function ToggleRow({ checked, description, label, onChange }: { checked: boolean; description: string; label: string; onChange: (value: boolean) => void }) {
  return (
    <div className="settings-row">
      <div>
        <h2>{label}</h2>
        <p>{description}</p>
      </div>
      <button className={`switch ${checked ? "on" : ""}`} onClick={() => onChange(!checked)} type="button">
        <span />
      </button>
    </div>
  );
}

function InfoRow({ label, value }: { label: string; value: string }) {
  return (
    <div className="settings-row">
      <div>
        <h2>{label}</h2>
      </div>
      <span className="setting-value">{value}</span>
    </div>
  );
}

export default App;
