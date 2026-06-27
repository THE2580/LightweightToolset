import { invoke } from "@tauri-apps/api/core";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  ChevronLeft,
  ChevronRight,
  Check,
  Copy,
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
  Sun,
  Terminal,
  Trash2,
  Wrench,
  X,
} from "lucide-react";
import { type MouseEvent, useCallback, useEffect, useState } from "react";
import { createPortal } from "react-dom";
import "./App.css";

type ThemeMode = "system" | "light" | "dark";
type CloseBehavior = "quit" | "tray";
type SettingsTab = "general" | "hotkey" | "logs" | "about";

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

type CaptureHotkeyDraft = {
  display: string;
  value: string;
};

type DebugLogEntry = {
  timestampMs: number;
  level: string;
  message: string;
};

type UpdateNotice = {
  phase: "available" | "up-to-date" | "error";
  title: string;
  message: string;
  releaseNotes?: string;
  releaseUrl?: string;
  downloadUrl?: string;
};

type HotkeyNotice = {
  phase: "success" | "error";
  title: string;
  message: string;
  detail?: {
    kind: "plain-error" | "conflict";
    hotkey?: string;
    toolName?: string;
  };
};

type View = "home" | "settings" | "tool";

type NavigationTarget = {
  view: View;
  toolId?: string;
};

const DEFAULT_TITLE = "轻量化工具集";
const APP_NAME = "LightweightToolset";
const APP_SUBTITLE = "Windows 桌面工具集";
const APP_VERSION = "0.1.0";
const GITHUB_REPO = "THE2580/LightweightToolset";
const GITHUB_URL = `https://github.com/${GITHUB_REPO}`;
const AUTHOR_EMAILS = ["2021289500@qq.com", "liangneng20060725@gmail.com"];
const toolIcons = [Keyboard, MonitorCog];
const HOTKEY_MODIFIERS = new Set(["CTRL", "CONTROL", "ALT", "SHIFT", "META", "SUPER", "CMD", "COMMAND"]);

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

  const updateSettings = useCallback(async (patch: SettingsPatch) => {
    try {
      setSnapshot(await invoke<AppSnapshot>("update_app_settings", { patch }));
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }, []);

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
                setSnapshot={setSnapshot}
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

function DebugLogPanel() {
  const [logs, setLogs] = useState<DebugLogEntry[]>([]);
  const [error, setError] = useState<string | null>(null);

  const loadLogs = useCallback(async () => {
    try {
      setLogs(await invoke<DebugLogEntry[]>("get_debug_logs"));
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }, []);

  useEffect(() => {
    void loadLogs();
    const interval = window.setInterval(() => void loadLogs(), 1500);
    return () => window.clearInterval(interval);
  }, [loadLogs]);

  async function clearLogs() {
    try {
      await invoke("clear_debug_logs");
      setLogs([]);
      setError(null);
    } catch (reason) {
      setError(String(reason));
    }
  }

  return (
    <div className="settings-section page-enter">
      <div className="log-heading">
        <div>
          <h2>控制台日志</h2>
          <p>保留最近 300 条主进程与页面日志</p>
        </div>
        <div className="log-actions">
          <button className="secondary-action icon-text-action" onClick={() => void loadLogs()} type="button"><RefreshCw size={13} />刷新</button>
          <button className="secondary-action icon-text-action" onClick={() => void clearLogs()} type="button"><Trash2 size={13} />清空</button>
        </div>
      </div>
      {error ? <div className="error-banner">{error}</div> : null}
      <div className="terminal-panel" aria-label="终端日志输出">
        {logs.length ? logs.map((entry, index) => (
          <div className="terminal-line" key={`${entry.timestampMs}-${index}`}>
            <span className="terminal-prefix">
              <span className="terminal-time">{formatLogTime(entry.timestampMs)}</span>
              <span className={`terminal-level ${entry.level}`}>[{entry.level}]</span>
            </span>
            <span className="terminal-message">{entry.message}</span>
          </div>
        )) : (
          <div className="terminal-line muted">暂无日志输出</div>
        )}
      </div>
    </div>
  );
}

function formatLogTime(timestampMs: number) {
  return new Date(timestampMs).toLocaleTimeString("zh-CN", { hour12: false });
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
  setSnapshot,
  tools,
  updateSettings,
}: {
  coldStartupMs: number;
  settings: AppSettings;
  setAutoStartEnabled: (enabled: boolean) => Promise<void>;
  setSnapshot: React.Dispatch<React.SetStateAction<AppSnapshot | null>>;
  tools: Tool[];
  updateSettings: (patch: SettingsPatch) => Promise<void>;
}) {
  const [activeTab, setActiveTab] = useState<SettingsTab>("general");
  const [titleDraft, setTitleDraft] = useState(settings.windowTitle);
  const [storagePathDraft, setStoragePathDraft] = useState(settings.storagePath);
  const [defaultStoragePath, setDefaultStoragePath] = useState("");
  const titleChanged = titleDraft.trim() !== settings.windowTitle;
  const titleResetVisible = titleDraft.trim() !== DEFAULT_TITLE || settings.windowTitle !== DEFAULT_TITLE;

  useEffect(() => setTitleDraft(settings.windowTitle), [settings.windowTitle]);
  useEffect(() => setStoragePathDraft(settings.storagePath), [settings.storagePath]);
  useEffect(() => {
    if (activeTab === "logs" && !settings.developerMode) {
      setActiveTab("general");
    }
  }, [activeTab, settings.developerMode]);
  useEffect(() => {
    void invoke<string>("get_default_storage_path")
      .then(setDefaultStoragePath)
      .catch(() => setDefaultStoragePath(""));
  }, []);

  useEffect(() => {
    if (storagePathDraft.trim() === settings.storagePath) {
      return;
    }
    const timer = window.setTimeout(() => {
      void updateSettings({ storagePath: storagePathDraft });
    }, 450);
    return () => window.clearTimeout(timer);
  }, [settings.storagePath, storagePathDraft, updateSettings]);

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
      void updateSettings({ storagePath: selected });
    }
  }

  function updateStoragePath(value: string) {
    setStoragePathDraft(value);
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
        {settings.developerMode ? (
          <SettingsTabButton active={activeTab === "logs"} icon={<Terminal size={14} />} label="控制台日志" onClick={() => setActiveTab("logs")} />
        ) : null}
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
                <input className="settings-input mono" placeholder="默认应用配置目录" value={storagePathDraft} onChange={(event) => updateStoragePath(event.target.value)} />
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
          <HotkeySettings setSnapshot={setSnapshot} tools={tools} />
        ) : null}

        {activeTab === "logs" ? <DebugLogPanel /> : null}

        {activeTab === "about" ? (
          <AboutSettings coldStartupMs={coldStartupMs} settings={settings} updateSettings={updateSettings} />
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

function HotkeySettings({ setSnapshot, tools }: { setSnapshot: React.Dispatch<React.SetStateAction<AppSnapshot | null>>; tools: Tool[] }) {
  const [editingToolId, setEditingToolId] = useState<string | null>(null);
  const [draft, setDraft] = useState<CaptureHotkeyDraft>({ display: "", value: "" });
  const [notice, setNotice] = useState<HotkeyNotice | null>(null);
  const [noticeClosing, setNoticeClosing] = useState(false);
  const editingTool = tools.find((tool) => tool.id === editingToolId) ?? null;

  async function startEditing(tool: Tool) {
    setEditingToolId(tool.id);
    setDraft({ display: "", value: "" });
    try {
      await invoke("suspend_tool_hotkeys");
    } catch (reason) {
      showNotice({
        phase: "error",
        title: "快捷键监听失败",
        message: String(reason),
      });
    }
  }

  async function stopEditing() {
    setEditingToolId(null);
    setDraft({ display: "", value: "" });
    try {
      await invoke("resume_tool_hotkeys");
    } catch (reason) {
      showNotice({
        phase: "error",
        title: "快捷键恢复失败",
        message: String(reason),
      });
    }
  }

  function captureHotkey(event: React.KeyboardEvent<HTMLElement>) {
    event.preventDefault();
    const key = normalizeKeyboardKey(event.key);
    if (!key) {
      return;
    }
    const parts = [
      event.ctrlKey ? "CTRL" : "",
      event.altKey ? "ALT" : "",
      event.shiftKey ? "SHIFT" : "",
      event.metaKey ? "WIN" : "",
      key,
    ].filter(Boolean);
    const display = parts.join("+");
    setDraft({
      display,
      value: display.replace(/\bWIN\b/g, "SUPER"),
    });
  }

  function showNotice(nextNotice: HotkeyNotice) {
    setNoticeClosing(false);
    setNotice(nextNotice);
  }

  function closeNotice() {
    setNoticeClosing(true);
    window.setTimeout(() => {
      setNotice(null);
      setNoticeClosing(false);
    }, 180);
  }

  async function saveHotkey() {
    if (!editingTool) {
      return;
    }
    const normalizedDraft = normalizeHotkeyDraft(draft.value);
    if (!normalizedDraft) {
      showNotice({
        phase: "error",
        title: "快捷键未填写",
        message: "请先点击输入框并按下一个快捷键组合。",
        detail: { kind: "plain-error" },
      });
      return;
    }
    const validationError = validateHotkeyDraft(normalizedDraft);
    if (validationError) {
      showNotice({
        phase: "error",
        title: "快捷键格式不正确",
        message: validationError,
        detail: { kind: "plain-error" },
      });
      return;
    }
    const conflictTool = tools.find((tool) => tool.id !== editingToolId && normalizeHotkeyDraft(tool.hotkey) === normalizedDraft);
    if (conflictTool) {
      showNotice({
        phase: "error",
        title: "快捷键冲突",
        message: `快捷键 ${formatHotkeyForDisplay(normalizedDraft)} 已被 ${conflictTool.name} 使用。`,
        detail: {
          kind: "conflict",
          hotkey: formatHotkeyForDisplay(normalizedDraft),
          toolName: conflictTool.name,
        },
      });
      return;
    }
    try {
      const nextSnapshot = await invoke<AppSnapshot>("set_tool_hotkey", {
        hotkey: normalizedDraft,
        toolId: editingTool.id,
      });
      setSnapshot(nextSnapshot);
      await stopEditing();
      showNotice({
        phase: "success",
        title: "快捷键已保存",
        message: `${editingTool.name} 已更新为 ${formatHotkeyForDisplay(normalizedDraft)}。`,
      });
    } catch (reason) {
      showNotice({
        phase: "error",
        title: "快捷键注册失败",
        message: String(reason),
        detail: { kind: "plain-error" },
      });
    }
  }

  return (
    <div className="settings-section page-enter">
      {tools.map((tool) => {
        const isEditing = editingToolId === tool.id;
        return (
          <div className="settings-row hotkey-row" key={tool.id}>
            <div>
              <h2>{tool.name}</h2>
              <p>{tool.enabled ? "当前快捷键已注册；保存后会立即重新注册并检查冲突" : "工具已禁用；快捷键会保存，启用后注册"}</p>
            </div>
            <div className="hotkey-controls">
              {isEditing ? (
                <>
                  <button
                    autoFocus
                    className={`hotkey-capture ${draft.display ? "" : "empty"}`}
                    onBlur={() => void stopEditing()}
                    onKeyDown={captureHotkey}
                    type="button"
                  >
                    {draft.display || "按下快捷键"}
                  </button>
                  <button className="primary-action icon-text-action" onMouseDown={(event) => event.preventDefault()} onClick={() => void saveHotkey()} type="button"><Check size={12} />保存</button>
                  <button className="secondary-action" onMouseDown={(event) => event.preventDefault()} onClick={() => void stopEditing()} type="button">取消</button>
                </>
              ) : (
                <>
                  <kbd className="hotkey-preview">{formatHotkeyForDisplay(tool.hotkey)}</kbd>
                  <button className="secondary-action planned-action" onClick={() => void startEditing(tool)} type="button"><Pencil size={12} />编辑</button>
                </>
              )}
            </div>
          </div>
        );
      })}
      <p className="settings-note">点击捕获框后直接按下组合键；同一工具集内会在保存瞬间检查重复，系统级冲突会在保存注册时弹窗提示。</p>
      {notice ? <HotkeyNoticeDialog closing={noticeClosing} notice={notice} onClose={closeNotice} /> : null}
    </div>
  );
}

function normalizeHotkeyDraft(value: string) {
  return value.split("+").map((part) => part.trim().toUpperCase()).filter(Boolean).join("+");
}

function formatHotkeyForDisplay(value: string) {
  return normalizeHotkeyDraft(value).replace(/\bSUPER\b/g, "WIN");
}

function validateHotkeyDraft(value: string) {
  const parts = value.split("+").map((part) => part.trim().toUpperCase()).filter(Boolean);
  if (parts.length < 2) {
    return "快捷键必须包含修饰键和一个按键";
  }
  const keyCount = parts.filter((part) => !HOTKEY_MODIFIERS.has(part)).length;
  if (!HOTKEY_MODIFIERS.has(parts[0])) {
    return "首个按键必须是 Ctrl/Alt/Shift/Win";
  }
  if (HOTKEY_MODIFIERS.has(parts[parts.length - 1])) {
    return "末尾必须是普通按键";
  }
  if (keyCount !== 1) {
    return "只能包含一个普通按键";
  }
  return null;
}

function normalizeKeyboardKey(key: string) {
  if (["Control", "Alt", "Shift", "Meta"].includes(key)) {
    return "";
  }
  if (key === " ") {
    return "SPACE";
  }
  if (key.length === 1) {
    return key.toUpperCase();
  }
  return key.toUpperCase();
}

function HotkeyNoticeDialog({ closing, notice, onClose }: { closing: boolean; notice: HotkeyNotice; onClose: () => void }) {
  const isSuccess = notice.phase === "success";
  return createPortal(
    <div className={`dialog-backdrop ${closing ? "closing" : ""}`} onMouseDown={onClose}>
      <section aria-label="快捷键提示" aria-modal="true" className={`update-dialog ${closing ? "closing" : ""}`} onMouseDown={(event) => event.stopPropagation()} role="dialog">
        <header className="update-dialog-header">
          <div className={`update-dialog-icon ${isSuccess ? "" : "danger"}`}>
            {isSuccess ? <Check size={16} /> : <Info size={16} />}
          </div>
          <div>
            <h2>{notice.title}</h2>
            <HotkeyNoticeMessage notice={notice} />
          </div>
          <button aria-label="关闭" className="dialog-close-button" onClick={onClose} type="button"><X size={13} /></button>
        </header>
        <footer className="update-dialog-actions">
          <button className="primary-action" onClick={onClose} type="button">知道了</button>
        </footer>
      </section>
    </div>,
    document.body,
  );
}

function HotkeyNoticeMessage({ notice }: { notice: HotkeyNotice }) {
  if (notice.detail?.kind === "conflict" && notice.detail.hotkey && notice.detail.toolName) {
    return (
      <p>
        快捷键 {notice.detail.hotkey} 已被 <span className="dialog-danger-text">{notice.detail.toolName}</span> 使用。
      </p>
    );
  }
  return <p className={notice.detail?.kind === "plain-error" ? "dialog-danger-text" : ""}>{notice.message}</p>;
}

function AboutSettings({ coldStartupMs, settings, updateSettings }: { coldStartupMs: number; settings: AppSettings; updateSettings: (patch: SettingsPatch) => Promise<void> }) {
  const [copiedEmail, setCopiedEmail] = useState<string | null>(null);
  const [updateStatus, setUpdateStatus] = useState("当前已是最新版本");
  const [checkingUpdates, setCheckingUpdates] = useState(false);
  const [updateNotice, setUpdateNotice] = useState<UpdateNotice | null>(null);
  const [updateNoticeClosing, setUpdateNoticeClosing] = useState(false);

  useEffect(() => {
    if (settings.autoCheckUpdates) {
      void checkUpdates(false);
    }
  }, [settings.autoCheckUpdates]);

  async function copyEmail(email: string) {
    await navigator.clipboard.writeText(email);
    setCopiedEmail(email);
    window.setTimeout(() => setCopiedEmail(null), 1200);
  }

  async function checkUpdates(manual = true) {
    if (checkingUpdates) {
      return;
    }
    setCheckingUpdates(true);
    setUpdateStatus("正在检查更新...");
    try {
      const response = await fetch(`https://api.github.com/repos/${GITHUB_REPO}/releases/latest`, {
        headers: { Accept: "application/vnd.github+json" },
      });
      if (response.status === 404) {
        setUpdateStatus("暂无发布版本");
        if (manual) {
          setUpdateNoticeClosing(false);
          setUpdateNotice({
            phase: "up-to-date",
            title: "当前已经是最新版本",
            message: "远端仓库暂未发布正式 Release，当前版本无需更新。",
          });
        }
        return;
      }
      if (!response.ok) {
        throw new Error(`GitHub 返回 ${response.status}`);
      }
      const release = await response.json() as { assets?: Array<{ browser_download_url?: string; name?: string }>; body?: string; html_url?: string; tag_name?: string };
      const tag = release.tag_name ?? "";
      if (!isRemoteVersionNewer(tag, APP_VERSION)) {
        setUpdateStatus("当前已是最新版本");
        if (manual) {
          setUpdateNoticeClosing(false);
          setUpdateNotice({
            phase: "up-to-date",
            title: "当前已经是最新版本",
            message: `当前版本 v${APP_VERSION}，未发现可用更新。`,
          });
        }
        return;
      }
      const asset = release.assets?.find((item) => item.name?.toLowerCase().endsWith(".exe")) ?? release.assets?.[0];
      const notice = {
        phase: "available" as const,
        title: `发现新版本 ${tag}`,
        message: `当前版本 v${APP_VERSION}，建议更新以获得最新改进。`,
        releaseNotes: release.body?.trim() || "本次 Release 暂未填写更新日志。",
        releaseUrl: release.html_url ?? GITHUB_URL,
        downloadUrl: asset?.browser_download_url,
      };
      setUpdateStatus(`发现新版本 ${tag}`);
      if (manual || settings.showUpdateNotification) {
        setUpdateNoticeClosing(false);
        setUpdateNotice(notice);
      }
    } catch (reason) {
      const message = `检查失败：${String(reason)}`;
      setUpdateStatus(message);
      if (manual) {
        setUpdateNoticeClosing(false);
        setUpdateNotice({
          phase: "error",
          title: "检查更新失败",
          message,
        });
      }
    } finally {
      setCheckingUpdates(false);
    }
  }

  function closeUpdateNotice() {
    setUpdateNoticeClosing(true);
    window.setTimeout(() => {
      setUpdateNotice(null);
      setUpdateNoticeClosing(false);
    }, 180);
  }

  return (
    <div className="settings-section page-enter about-section">
      <section className="about-block">
        <h2>{APP_NAME}</h2>
        <p>{APP_SUBTITLE}</p>
      </section>
      <InfoBlock label="作者">
        <span>THE2580</span>
      </InfoBlock>
      <InfoBlock label="GitHub">
        <button className="about-link" onClick={() => void openUrl(GITHUB_URL)} type="button">
          github.com/{GITHUB_REPO}
        </button>
      </InfoBlock>
      <InfoBlock label="邮箱">
        <div className="email-list">
          {AUTHOR_EMAILS.map((email) => (
            <div className="email-line" key={email}>
              <button className="about-link" onClick={() => void openUrl(`mailto:${email}`)} type="button">{email}</button>
              <button className="icon-action compact" aria-label={`复制 ${email}`} onClick={() => void copyEmail(email)} type="button">
                {copiedEmail === email ? <Check size={12} /> : <Copy size={12} />}
              </button>
            </div>
          ))}
        </div>
      </InfoBlock>
      <InfoBlock label="版本">
        <span>{APP_VERSION}</span>
      </InfoBlock>
      <ToggleRow
        checked={settings.autoCheckUpdates}
        description="软件启动时自动检查更新"
        label="自动检查更新"
        onChange={(value) => updateSettings({ autoCheckUpdates: value })}
      />
      {settings.autoCheckUpdates ? (
        <div className="update-card">
          <ToggleRow
            checked={settings.showUpdateNotification}
            description="自动检查发现新版本时弹出提示"
            label="新版本提示弹窗"
            onChange={(value) => updateSettings({ showUpdateNotification: value })}
          />
        </div>
      ) : null}
      <div className="update-card update-status-card">
        <div>
          <h2>软件更新</h2>
          <p>{updateStatus}</p>
          <p>冷启动基线：{coldStartupMs} ms</p>
        </div>
        <button className="secondary-action icon-text-action" disabled={checkingUpdates} onClick={() => void checkUpdates()} type="button">
          <RefreshCw className={checkingUpdates ? "spin-icon" : ""} size={13} />
          {checkingUpdates ? "检查中" : "检查更新"}
        </button>
      </div>
      {updateNotice ? (
        <UpdateNoticeDialog
          closing={updateNoticeClosing}
          notice={updateNotice}
          onClose={closeUpdateNotice}
          onOpenDownload={() => {
            void openUrl(updateNotice.downloadUrl ?? updateNotice.releaseUrl ?? GITHUB_URL);
            closeUpdateNotice();
          }}
          onOpenRelease={() => {
            void openUrl(updateNotice.releaseUrl ?? GITHUB_URL);
            closeUpdateNotice();
          }}
          onRetry={() => {
            setUpdateNotice(null);
            void checkUpdates();
          }}
        />
      ) : null}
    </div>
  );
}

function UpdateNoticeDialog({
  closing,
  notice,
  onClose,
  onOpenDownload,
  onOpenRelease,
  onRetry,
}: {
  closing: boolean;
  notice: UpdateNotice;
  onClose: () => void;
  onOpenDownload: () => void;
  onOpenRelease: () => void;
  onRetry: () => void;
}) {
  const isAvailable = notice.phase === "available";
  const isLatest = notice.phase === "up-to-date";
  return createPortal(
    <div className={`dialog-backdrop ${closing ? "closing" : ""}`} onMouseDown={onClose}>
      <section aria-label="软件更新提示" aria-modal="true" className={`update-dialog ${closing ? "closing" : ""}`} onMouseDown={(event) => event.stopPropagation()} role="dialog">
        <header className="update-dialog-header">
          <div className="update-dialog-icon">
            {isAvailable ? <Info size={16} /> : isLatest ? <Check size={16} /> : <RefreshCw size={16} />}
          </div>
          <div>
            <h2>{notice.title}</h2>
            <p>{notice.message}</p>
          </div>
          <button aria-label="关闭" className="dialog-close-button" onClick={onClose} type="button"><X size={13} /></button>
        </header>
        {isAvailable ? (
          <div className="update-dialog-body">
            <p>更新日志</p>
            <div>{notice.releaseNotes}</div>
          </div>
        ) : null}
        <footer className="update-dialog-actions">
          <button className="secondary-action" onClick={onClose} type="button">{isAvailable ? "稍后处理" : "知道了"}</button>
          {isAvailable ? <button className="secondary-action" onClick={onOpenRelease} type="button">查看 Release</button> : null}
          {isAvailable ? <button className="primary-action" onClick={onOpenDownload} type="button">前往更新</button> : null}
          {notice.phase === "error" ? <button className="primary-action" onClick={onRetry} type="button">重新检查</button> : null}
        </footer>
      </section>
    </div>,
    document.body,
  );
}

function InfoBlock({ children, label }: { children: React.ReactNode; label: string }) {
  return (
    <section className="about-block">
      <p>{label}</p>
      <div className="about-value">{children}</div>
    </section>
  );
}

function isRemoteVersionNewer(tag: string, current: string) {
  const remote = tag.replace(/^v/i, "").split(".").map((part) => Number.parseInt(part, 10) || 0);
  const local = current.replace(/^v/i, "").split(".").map((part) => Number.parseInt(part, 10) || 0);
  for (let index = 0; index < Math.max(remote.length, local.length); index += 1) {
    const diff = (remote[index] ?? 0) - (local[index] ?? 0);
    if (diff !== 0) {
      return diff > 0;
    }
  }
  return false;
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

export default App;
