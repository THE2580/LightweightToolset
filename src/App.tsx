import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { open } from "@tauri-apps/plugin-dialog";
import { openUrl } from "@tauri-apps/plugin-opener";
import {
  ChevronLeft,
  ChevronRight,
  Check,
  ClipboardList,
  Copy,
  FileText,
  Gauge,
  FolderOpen,
  Home,
  Info,
  Keyboard,
  Minus,
  Monitor,
  Moon,
  Pencil,
  PanelLeftClose,
  PanelLeftOpen,
  Pin,
  Plus,
  RefreshCw,
  RotateCcw,
  Scissors,
  Search,
  Settings,
  Sun,
  Terminal,
  Trash2,
  Wrench,
  X,
} from "lucide-react";
import { type MouseEvent, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
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

type ClipboardEntry = {
  id: string;
  text: string;
  title: string;
  source: string;
  createdAt: number;
  lastCopiedAt: number;
  lastUsedAt: number | null;
  pinnedAt: number | null;
  deletedAt: number | null;
  copyCount: number;
  useCount: number;
};

type ClipboardSettings = {
  listening: boolean;
  retentionDays: number;
  maxTextBytes: number;
  panelWidth: number;
  panelHeight: number;
};

type ClipboardSnapshot = {
  settings: ClipboardSettings;
  stats: {
    historyCount: number;
    pinnedCount: number;
    trashCount: number;
    storageBytes: number;
    skippedTooLong: number;
    lastCleanupAt: number | null;
  };
  listeningActive: boolean;
};

type ClipboardQueryResult = {
  entries: ClipboardEntry[];
  total: number;
};

type ToastMessage = {
  id: number;
  text: string;
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
const APP_VERSION = "0.1.2";
const GITHUB_REPO = "THE2580/LightweightToolset";
const GITHUB_URL = `https://github.com/${GITHUB_REPO}`;
const AUTHOR_EMAILS = ["2021289500@qq.com", "liangneng20060725@gmail.com"];
const toolIcons = [ClipboardList];
const HOTKEY_MODIFIERS = new Set(["CTRL", "CONTROL", "ALT", "SHIFT", "META", "SUPER", "CMD", "COMMAND"]);
const TOAST_DURATION_MS = 1400;
const CLIPBOARD_PAGE_SIZE = 10;

function isTextInputTarget(target: EventTarget | null) {
  const element = target instanceof HTMLElement ? target : null;
  if (!element) {
    return false;
  }
  return element.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(element.tagName);
}

function useUpdateChecker(settings?: AppSettings) {
  const [updateStatus, setUpdateStatus] = useState("当前已是最新版本");
  const [checkingUpdates, setCheckingUpdates] = useState(false);
  const [updateNotice, setUpdateNotice] = useState<UpdateNotice | null>(null);
  const [updateNoticeClosing, setUpdateNoticeClosing] = useState(false);
  const [autoCheckAttempted, setAutoCheckAttempted] = useState(false);
  const checkingRef = useRef(false);

  const checkUpdates = useCallback(async (manual = true) => {
    if (checkingRef.current) {
      return;
    }
    checkingRef.current = true;
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
      if (manual || settings?.showUpdateNotification) {
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
      checkingRef.current = false;
      setCheckingUpdates(false);
    }
  }, [settings?.showUpdateNotification]);

  useEffect(() => {
    if (!settings?.autoCheckUpdates) {
      setAutoCheckAttempted(false);
      return;
    }
    if (autoCheckAttempted) {
      return;
    }
    setAutoCheckAttempted(true);
    void checkUpdates(false);
  }, [autoCheckAttempted, checkUpdates, settings?.autoCheckUpdates]);

  function closeUpdateNotice() {
    setUpdateNoticeClosing(true);
    window.setTimeout(() => {
      setUpdateNotice(null);
      setUpdateNoticeClosing(false);
    }, 180);
  }

  return {
    checkingUpdates,
    checkUpdates,
    closeUpdateNotice,
    updateNotice,
    updateNoticeClosing,
    updateStatus,
  };
}

function useToastQueue(durationMs = TOAST_DURATION_MS) {
  const [current, setCurrent] = useState<ToastMessage | null>(null);
  const timerRef = useRef<number | null>(null);
  const nextIdRef = useRef(1);

  const clearCurrent = useCallback(() => {
    setCurrent(null);
    timerRef.current = null;
  }, []);

  const pushToast = useCallback((text: string) => {
    const next = { id: nextIdRef.current, text };
    nextIdRef.current += 1;
    if (timerRef.current !== null) {
      window.clearTimeout(timerRef.current);
      timerRef.current = null;
    }
    setCurrent(next);
    timerRef.current = window.setTimeout(clearCurrent, durationMs);
  }, [clearCurrent, durationMs]);

  useEffect(() => () => {
    if (timerRef.current !== null) {
      window.clearTimeout(timerRef.current);
    }
  }, []);

  return { pushToast, toast: current };
}

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
  const [settingsInitialTab, setSettingsInitialTab] = useState<SettingsTab>("general");

  const tools = snapshot?.tools ?? [];
  const settings = snapshot?.settings;
  const updateChecker = useUpdateChecker(settings);
  const { pushToast: pushAppToast, toast: appToast } = useToastQueue();
  const windowTitle = settings?.windowTitle || DEFAULT_TITLE;
  const isHistoryBackAvailable = canNavigateHistory(-1);
  const isHistoryForwardAvailable = canNavigateHistory(1);
  const sidebarTargets = useMemo(() => {
    const targets: Array<{ label: string; target: NavigationTarget }> = [{ label: "首页", target: { view: "home" } }];
    const enabledTools = tools.filter((tool) => tool.enabled);
    targets.push(...enabledTools.map((tool) => ({ label: tool.name, target: { view: "tool", toolId: tool.id } as NavigationTarget })));
    targets.push({ label: "设置", target: { view: "settings" } });
    return targets;
  }, [tools]);

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

  function openSettingsTab(tab: SettingsTab) {
    setSettingsInitialTab(tab);
    navigate({ view: "settings" });
  }

  useEffect(() => {
    function currentSidebarIndex() {
      if (view === "tool" && activeTool) {
        const index = sidebarTargets.findIndex((item) => item.target.view === "tool" && item.target.toolId === activeTool.id);
        return index === -1 ? 0 : index;
      }
      const index = sidebarTargets.findIndex((item) => item.target.view === view);
      return index === -1 ? 0 : index;
    }

    function handleKeyDown(event: KeyboardEvent) {
      if (event.defaultPrevented || isTextInputTarget(event.target)) {
        return;
      }
      const element = event.target instanceof HTMLElement ? event.target : null;
      if (element?.closest('[role="dialog"]')) {
        return;
      }
      if (event.key !== "ArrowUp" && event.key !== "ArrowDown") {
        return;
      }
      if (sidebarTargets.length === 0) {
        return;
      }

      const direction = event.key === "ArrowUp" ? -1 : 1;
      const currentIndex = currentSidebarIndex();
      const nextIndex = Math.max(0, Math.min(currentIndex + direction, sidebarTargets.length - 1));
      if (nextIndex === currentIndex) {
        return;
      }
      const next = sidebarTargets[nextIndex];
      if (!next) {
        return;
      }

      event.preventDefault();
      if (next.target.view === "settings") {
        setSettingsInitialTab("general");
      }
      navigate(next.target);
      pushAppToast(`${event.key === "ArrowUp" ? "↑" : "↓"} ${next.label}`);
    }

    window.addEventListener("keydown", handleKeyDown, true);
    return () => window.removeEventListener("keydown", handleKeyDown, true);
  }, [activeTool, history, historyIndex, pushAppToast, sidebarTargets, view]);

  useEffect(() => {
    let dispose: (() => void) | undefined;
    void listen<string>("navigate-tool", (event) => {
      const tool = tools.find((candidate) => candidate.id === event.payload);
      if (tool) {
        openTool(tool);
      }
    }).then((unlisten) => {
      dispose = unlisten;
    });
    return () => {
      dispose?.();
    };
  }, [tools, history, historyIndex]);

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
          {sidebarCollapsed ? (
            <div className="title-history-controls" aria-label="浏览历史">
              <button aria-label="后退" disabled={!isHistoryBackAvailable} onClick={() => navigateHistory(-1)} type="button">
                <ChevronLeft size={14} />
              </button>
              <button aria-label="前进" disabled={!isHistoryForwardAvailable} onClick={() => navigateHistory(1)} type="button">
                <ChevronRight size={14} />
              </button>
            </div>
          ) : null}
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
              onClick={() => openSettingsTab("general")}
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
                  const badge = tool.enabled ? null : "已禁用";
                  return (
                    <button className={`tool-card ${tool.enabled ? "" : "disabled"}`} disabled={!tool.enabled} key={tool.id} onClick={() => openTool(tool)} type="button">
                      {badge ? <span className="tool-card-badge">{badge}</span> : null}
                      <div className="tool-card-heading">
                        <div className="tool-icon"><Icon size={19} /></div>
                        <h2>{tool.name}</h2>
                      </div>
                      <p>{tool.description}</p>
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
                initialTab={settingsInitialTab}
                settings={settings}
                setAutoStartEnabled={setAutoStartEnabled}
                setSnapshot={setSnapshot}
                tools={tools}
                updateChecker={updateChecker}
                updateSettings={updateSettings}
              />
            </div>
          ) : activeTool ? (
            <div className="page-enter" key={activeTool.id}><ToolPage onOpenSettingsTab={openSettingsTab} tool={activeTool} /></div>
          ) : null}
        </main>
      </div>
      {updateChecker.updateNotice ? (
        <UpdateNoticeDialog
          closing={updateChecker.updateNoticeClosing}
          notice={updateChecker.updateNotice}
          onClose={updateChecker.closeUpdateNotice}
          onOpenDownload={() => {
            void openUrl(updateChecker.updateNotice?.downloadUrl ?? updateChecker.updateNotice?.releaseUrl ?? GITHUB_URL);
            updateChecker.closeUpdateNotice();
          }}
          onOpenRelease={() => {
            void openUrl(updateChecker.updateNotice?.releaseUrl ?? GITHUB_URL);
            updateChecker.closeUpdateNotice();
          }}
          onRetry={() => {
            updateChecker.closeUpdateNotice();
            window.setTimeout(() => void updateChecker.checkUpdates(), 180);
          }}
        />
      ) : null}
      {appToast ? createPortal(<div className="app-toast" key={appToast.id}>{appToast.text}</div>, document.body) : null}
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

function ToolPage({ onOpenSettingsTab, tool }: { onOpenSettingsTab: (tab: SettingsTab) => void; tool: Tool }) {
  if (tool.id === "clipboard") {
    return <ClipboardToolPage onOpenSettingsTab={onOpenSettingsTab} tool={tool} />;
  }

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
  initialTab,
  settings,
  setAutoStartEnabled,
  setSnapshot,
  tools,
  updateChecker,
  updateSettings,
}: {
  coldStartupMs: number;
  initialTab: SettingsTab;
  settings: AppSettings;
  setAutoStartEnabled: (enabled: boolean) => Promise<void>;
  setSnapshot: React.Dispatch<React.SetStateAction<AppSnapshot | null>>;
  tools: Tool[];
  updateChecker: ReturnType<typeof useUpdateChecker>;
  updateSettings: (patch: SettingsPatch) => Promise<void>;
}) {
  const [activeTab, setActiveTab] = useState<SettingsTab>(initialTab);
  const [titleDraft, setTitleDraft] = useState(settings.windowTitle);
  const [storagePathDraft, setStoragePathDraft] = useState(settings.storagePath);
  const [defaultStoragePath, setDefaultStoragePath] = useState("");
  const titleChanged = titleDraft.trim() !== settings.windowTitle;
  const titleResetVisible = titleDraft.trim() !== DEFAULT_TITLE || settings.windowTitle !== DEFAULT_TITLE;

  useEffect(() => setTitleDraft(settings.windowTitle), [settings.windowTitle]);
  useEffect(() => setStoragePathDraft(settings.storagePath), [settings.storagePath]);
  useEffect(() => setActiveTab(initialTab), [initialTab]);
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
                <input className="settings-input mono" placeholder="默认应用配置目录" readOnly value={storagePathDraft} />
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
          <AboutSettingsPanel coldStartupMs={coldStartupMs} settings={settings} updateChecker={updateChecker} updateSettings={updateSettings} />
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

function ClipboardToolPage({ onOpenSettingsTab, tool }: { onOpenSettingsTab: (tab: SettingsTab) => void; tool: Tool }) {
  const [snapshot, setSnapshot] = useState<ClipboardSnapshot | null>(null);
  const [tab, setTab] = useState<"history" | "pinned">("history");
  const [entries, setEntries] = useState<ClipboardEntry[]>([]);
  const [trashEntries, setTrashEntries] = useState<ClipboardEntry[]>([]);
  const [search, setSearch] = useState("");
  const [page, setPage] = useState(0);
  const [pageAnimating, setPageAnimating] = useState(false);
  const [entryTotal, setEntryTotal] = useState(0);
  const { pushToast, toast: currentToast } = useToastQueue();
  const [manualTitle, setManualTitle] = useState("");
  const [manualText, setManualText] = useState("");
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [trashOpen, setTrashOpen] = useState(false);
  const [trashPage, setTrashPage] = useState(0);
  const [trashTotal, setTrashTotal] = useState(0);
  const [manualOpen, setManualOpen] = useState(false);
  const [detailEntry, setDetailEntry] = useState<ClipboardEntry | null>(null);
  const [extractEntry, setExtractEntry] = useState<ClipboardEntry | null>(null);
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const [openActionId, setOpenActionId] = useState<string | null>(null);
  const [selectedTokens, setSelectedTokens] = useState<string[]>([]);
  const [closingDialog, setClosingDialog] = useState<"settings" | "trash" | "manual" | "detail" | "extract" | null>(null);
  const entryNodeMapRef = useRef(new Map<string, HTMLElement>());
  const previousEntryRectsRef = useRef(new Map<string, DOMRect>());
  const previousEntryOrderRef = useRef<string[]>([]);
  const listTransitionRef = useRef(false);
  const listTransitionFrameRef = useRef<number | null>(null);
  const listTransitionTimerRef = useRef<number | null>(null);
  const listTransitionFallbackTimerRef = useRef<number | null>(null);
  const selectedEntries = useMemo(() => entries.filter((entry) => selectedIds.has(entry.id)), [entries, selectedIds]);
  const allEntriesSelected = entries.length > 0 && entries.every((entry) => selectedIds.has(entry.id));
  const pageCount = Math.max(1, Math.ceil(entryTotal / CLIPBOARD_PAGE_SIZE));
  const trashPageCount = Math.max(1, Math.ceil(trashTotal / CLIPBOARD_PAGE_SIZE));
  const extractTokens = useMemo(() => extractClipboardTokens(extractEntry?.text ?? ""), [extractEntry]);

  const loadClipboard = useCallback(async () => {
    const [nextSnapshot, result] = await Promise.all([
      invoke<ClipboardSnapshot>("clipboard_get_snapshot"),
      invoke<ClipboardQueryResult>("clipboard_query", {
        input: { scope: tab, search, offset: page * CLIPBOARD_PAGE_SIZE, limit: CLIPBOARD_PAGE_SIZE },
      }),
    ]);
    setSnapshot(nextSnapshot);
    setEntries(result.entries);
    setEntryTotal(result.total);
  }, [page, search, tab]);

  useEffect(() => {
    void loadClipboard();
    const timer = window.setInterval(() => void loadClipboard(), 1200);
    return () => window.clearInterval(timer);
  }, [loadClipboard]);

  useEffect(() => {
    setSelectedIds(new Set());
    setOpenActionId(null);
    setPage(0);
  }, [tab, search]);

  useEffect(() => {
    if (page > 0 && page >= pageCount) {
      setPage(pageCount - 1);
    }
  }, [page, pageCount]);

  useEffect(() => {
    return () => {
      if (listTransitionFrameRef.current !== null) {
        window.cancelAnimationFrame(listTransitionFrameRef.current);
      }
      if (listTransitionTimerRef.current !== null) {
        window.clearTimeout(listTransitionTimerRef.current);
      }
      if (listTransitionFallbackTimerRef.current !== null) {
        window.clearTimeout(listTransitionFallbackTimerRef.current);
      }
    };
  }, []);

  useLayoutEffect(() => {
    const previousRects = previousEntryRectsRef.current;
    const previousOrder = previousEntryOrderRef.current;
    const nextOrder = entries.map((entry) => entry.id);
    const orderChanged = previousOrder.length !== nextOrder.length || previousOrder.some((id, index) => id !== nextOrder[index]);
    const nextRects = new Map<string, DOMRect>();

    entryNodeMapRef.current.forEach((node, id) => {
      nextRects.set(id, node.getBoundingClientRect());
    });

    if (listTransitionRef.current) {
      previousEntryRectsRef.current = nextRects;
      previousEntryOrderRef.current = nextOrder;
      listTransitionRef.current = false;
      return;
    }

    if (orderChanged) {
      nextRects.forEach((nextRect, id) => {
        const previousRect = previousRects.get(id);
        const node = entryNodeMapRef.current.get(id);
        if (!previousRect || !node) {
          return;
        }
        const deltaY = previousRect.top - nextRect.top;
        if (Math.abs(deltaY) < 1) {
          return;
        }
        node.animate(
          [
            { transform: `translateY(${deltaY}px)` },
            { transform: "translateY(0)" },
          ],
          { duration: 180, easing: "cubic-bezier(0.2, 0.8, 0.2, 1)" },
        );
      });
    }

    previousEntryRectsRef.current = nextRects;
    previousEntryOrderRef.current = nextOrder;
  }, [entries]);

  useEffect(() => {
    function handleKeyDown(event: KeyboardEvent) {
      if (event.defaultPrevented || isTextInputTarget(event.target)) {
        return;
      }
      if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") {
        return;
      }
      if (settingsOpen || manualOpen || detailEntry || extractEntry) {
        return;
      }

      const direction = event.key === "ArrowLeft" ? -1 : 1;
      const currentPage = trashOpen ? trashPage : page;
      const currentPageCount = trashOpen ? trashPageCount : pageCount;
      const nextPage = Math.max(0, Math.min(currentPage + direction, currentPageCount - 1));
      if (nextPage === currentPage) {
        return;
      }

      event.preventDefault();
      toast(`${event.key === "ArrowLeft" ? "←" : "→"} ${event.key === "ArrowLeft" ? "上一页" : "下一页"}`);
      if (trashOpen) {
        void changeTrashPage(nextPage);
      } else {
        changePage(nextPage);
      }
    }

    window.addEventListener("keydown", handleKeyDown, true);
    return () => window.removeEventListener("keydown", handleKeyDown, true);
  }, [detailEntry, extractEntry, manualOpen, page, pageCount, settingsOpen, trashOpen, trashPage, trashPageCount]);

  function registerEntryNode(id: string, node: HTMLElement | null) {
    if (node) {
      entryNodeMapRef.current.set(id, node);
    } else {
      entryNodeMapRef.current.delete(id);
    }
  }

  function closeOpenActionsOnScroll() {
    if (openActionId) {
      setOpenActionId(null);
    }
  }

  async function loadTrash(page = trashPage) {
    const result = await invoke<ClipboardQueryResult>("clipboard_query", {
      input: { scope: "trash", search: "", offset: page * CLIPBOARD_PAGE_SIZE, limit: CLIPBOARD_PAGE_SIZE },
    });
    setTrashEntries(result.entries);
    setTrashTotal(result.total);
  }

  function toast(text: string) {
    pushToast(text);
  }

  function closeClipboardDialog(dialog: "settings" | "trash" | "manual" | "detail" | "extract") {
    setClosingDialog(dialog);
    window.setTimeout(() => {
      if (dialog === "settings") {
        setSettingsOpen(false);
      } else if (dialog === "trash") {
        setTrashOpen(false);
      } else if (dialog === "manual") {
        setManualOpen(false);
      } else if (dialog === "detail") {
        setDetailEntry(null);
      } else {
        setExtractEntry(null);
        setSelectedTokens([]);
      }
      setClosingDialog(null);
    }, 180);
  }

  function openHotkeySettingsFromClipboardSettings() {
    closeClipboardDialog("settings");
    window.setTimeout(() => onOpenSettingsTab("hotkey"), 180);
  }

  function toggleSelect(id: string) {
    setSelectedIds((current) => {
      const next = new Set(current);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  }

  function toggleSelectAll() {
    setSelectedIds(allEntriesSelected ? new Set() : new Set(entries.map((entry) => entry.id)));
  }

  function playListTransition() {
    listTransitionRef.current = true;
    if (listTransitionFrameRef.current !== null) {
      window.cancelAnimationFrame(listTransitionFrameRef.current);
    }
    if (listTransitionTimerRef.current !== null) {
      window.clearTimeout(listTransitionTimerRef.current);
    }
    if (listTransitionFallbackTimerRef.current !== null) {
      window.clearTimeout(listTransitionFallbackTimerRef.current);
    }
    setPageAnimating(false);
    listTransitionFrameRef.current = window.requestAnimationFrame(() => {
      setPageAnimating(true);
      listTransitionTimerRef.current = window.setTimeout(() => {
        setPageAnimating(false);
      }, 180);
      listTransitionFallbackTimerRef.current = window.setTimeout(() => {
        listTransitionRef.current = false;
      }, 700);
    });
  }

  function changePage(nextPage: number) {
    const safePage = Math.max(0, Math.min(nextPage, pageCount - 1));
    if (safePage === page) {
      return;
    }
    playListTransition();
    setPage(safePage);
    setSelectedIds(new Set());
    setOpenActionId(null);
  }

  function changeTab(nextTab: "history" | "pinned") {
    if (nextTab === tab) {
      return;
    }
    playListTransition();
    setTab(nextTab);
    setPage(0);
    setSelectedIds(new Set());
    setOpenActionId(null);
  }

  async function openTrash() {
    setTrashPage(0);
    await loadTrash(0);
    setTrashOpen(true);
  }

  async function restoreTrashEntries(ids: string[]) {
    if (ids.length === 0) {
      return;
    }
    await invoke("clipboard_restore", { ids });
    toast(`已恢复${ids.length}个记录`);
    await loadTrash(trashPage);
    await loadClipboard();
  }

  async function purgeTrashEntries(ids: string[]) {
    if (ids.length === 0) {
      return;
    }
    await invoke("clipboard_purge", { ids });
    toast(`已彻底删除${ids.length}个记录`);
    await loadTrash(trashPage);
    await loadClipboard();
  }

  async function changeTrashPage(page: number) {
    const safePage = Math.max(0, Math.min(page, Math.max(0, Math.ceil(trashTotal / CLIPBOARD_PAGE_SIZE) - 1)));
    setTrashPage(safePage);
    await loadTrash(safePage);
  }

  async function copyEntry(entry: ClipboardEntry) {
    const result = await invoke<{ message: string }>("clipboard_copy", { id: entry.id });
    toast(result.message || "已复制");
    await loadClipboard();
  }

  async function togglePinned(entry: ClipboardEntry) {
    await invoke("clipboard_update_entry", {
      id: entry.id,
      patch: { pinned: !entry.pinnedAt },
    });
    toast(entry.pinnedAt ? "已取消固定" : "已固定");
    await loadClipboard();
  }

  async function pinSelected() {
    const count = selectedEntries.length;
    await Promise.all(selectedEntries.map((entry) => invoke("clipboard_update_entry", {
      id: entry.id,
      patch: { pinned: tab !== "pinned" },
    })));
    setSelectedIds(new Set());
    toast(tab === "pinned" ? `已取消固定${count}个记录` : `已固定${count}个记录`);
    await loadClipboard();
  }

  async function deleteEntry(entry: ClipboardEntry) {
    await invoke("clipboard_delete", { ids: [entry.id] });
    toast("已移入回收站");
    await loadClipboard();
  }

  async function deleteSelected() {
    const count = selectedEntries.length;
    await invoke("clipboard_delete", { ids: selectedEntries.map((entry) => entry.id) });
    setSelectedIds(new Set());
    toast(`已删除${count}个记录`);
    await loadClipboard();
  }

  async function copyExtractedTokens() {
    if (selectedTokens.length === 0) {
      return;
    }
    const result = await invoke<{ message: string }>("clipboard_copy_derived_text", { text: selectedTokens.join("") });
    closeClipboardDialog("extract");
    toast(result.message || "已复制提取内容");
  }

  async function createManual() {
    if (!manualText.trim()) {
      return;
    }
    await invoke("clipboard_create_manual", {
      title: manualTitle.trim(),
      text: manualText,
    });
    setManualTitle("");
    setManualText("");
    setTab("pinned");
    closeClipboardDialog("manual");
    toast("已新增固定文本");
    await loadClipboard();
  }

  async function updateClipboardSettings(patch: Partial<ClipboardSettings>) {
    setSnapshot(await invoke<ClipboardSnapshot>("clipboard_update_settings", { patch }));
    await loadClipboard();
  }

  function closeOpenActionsFromOutside(event: React.PointerEvent<HTMLElement>) {
    if (!openActionId) {
      return;
    }
    const target = event.target;
    if (!(target instanceof Element)) {
      setOpenActionId(null);
      return;
    }
    const openCard = target.closest(".clipboard-entry-card.actions-open");
    if (!openCard) {
      setOpenActionId(null);
    }
  }

  return (
    <section className="tool-page clipboard-page" onPointerDownCapture={closeOpenActionsFromOutside}>
      <div className="clipboard-fixed-region">
        <div className="clipboard-page-header">
          <div>
            <h1>剪贴板</h1>
            <p>本地纯文本历史与固定片段。</p>
          </div>
          <div className={`clipboard-header-actions ${selectedEntries.length > 0 ? "selecting" : ""}`}>
            {selectedEntries.length > 0 ? (
              <div className="clipboard-bulk-actions">
                <button className="clipboard-bulk-action" onClick={toggleSelectAll} type="button">
                  {allEntriesSelected ? "取消全选" : `全选 [${selectedEntries.length}/${entries.length}]`}
                </button>
                <button className="clipboard-bulk-action" onClick={() => void pinSelected()} type="button">
                  <Pin size={12} />{tab === "pinned" ? "取消固定" : "固定选中"}
                </button>
                <button className="clipboard-bulk-action danger" onClick={() => void deleteSelected()} type="button">
                  <Trash2 size={12} />删除选中
                </button>
              </div>
            ) : tab === "pinned" ? (
              <button className="clipboard-add-action" onClick={() => setManualOpen(true)} type="button">
                <Plus size={12} />新增
              </button>
            ) : null}
            <button className="clipboard-square-action" title="回收站" onClick={() => void openTrash()} type="button">
              <Trash2 size={15} />
              {(snapshot?.stats.trashCount ?? 0) > 0 ? <span>{snapshot?.stats.trashCount}</span> : null}
            </button>
            <button className="clipboard-square-action" title="设置" onClick={() => setSettingsOpen(true)} type="button">
              <Settings size={15} />
            </button>
          </div>
        </div>

        <div className="clipboard-toolbar">
          <div className={`clipboard-tabs ${tab === "pinned" ? "pinned" : "history"}`}>
            <span className="clipboard-tab-indicator" />
            <button className={tab === "history" ? "active" : ""} onClick={() => changeTab("history")} type="button"><FileText size={13} />历史</button>
            <button className={tab === "pinned" ? "active" : ""} onClick={() => changeTab("pinned")} type="button"><Pin size={13} />固定</button>
            {snapshot ? (
              <div className="clipboard-stats-popover" role="tooltip">
                <div className="clipboard-stats-title"><Info size={13} />剪贴板统计</div>
                <dl>
                  <div><dt>历史</dt><dd>{snapshot.stats.historyCount}</dd></div>
                  <div><dt>固定</dt><dd>{snapshot.stats.pinnedCount}</dd></div>
                  <div><dt>回收站</dt><dd>{snapshot.stats.trashCount}</dd></div>
                  <div><dt>实际使用量</dt><dd>{formatStorageSize(snapshot.stats.storageBytes)}</dd></div>
                </dl>
              </div>
            ) : null}
          </div>
          <label className="clipboard-search">
            <Search size={13} />
            <input value={search} onChange={(event) => setSearch(event.target.value)} placeholder={tab === "history" ? "搜索历史文本" : "搜索固定片段"} />
          </label>
          <div className="clipboard-pagination">
            <button aria-label="上一页" disabled={page <= 0} onClick={() => changePage(page - 1)} type="button"><ChevronLeft size={13} /></button>
            <span>{page + 1} / {pageCount}</span>
            <button aria-label="下一页" disabled={page >= pageCount - 1} onClick={() => changePage(page + 1)} type="button"><ChevronRight size={13} /></button>
          </div>
        </div>
      </div>

      <div className={`clipboard-entry-list ${pageAnimating ? "page-switching" : ""}`} onScroll={closeOpenActionsOnScroll}>
        {entries.map((entry) => (
          <ClipboardEntryCard
            actionsOpen={openActionId === entry.id}
            entry={entry}
            key={entry.id}
            onCopy={() => void copyEntry(entry)}
            onDelete={() => void deleteEntry(entry)}
            onOpenActions={(open) => setOpenActionId(open ? entry.id : null)}
            onOpenDetail={() => setDetailEntry(entry)}
            onOpenExtract={() => {
              setSelectedTokens([]);
              setExtractEntry(entry);
              setOpenActionId(null);
            }}
            onPrimary={() => setDetailEntry(entry)}
            onSelect={() => toggleSelect(entry.id)}
            onTogglePinned={() => void togglePinned(entry)}
            registerNode={(node) => registerEntryNode(entry.id, node)}
            selectMode={selectedEntries.length > 0}
            selected={selectedIds.has(entry.id)}
          />
        ))}
        {entries.length === 0 ? <p className="clipboard-empty">暂无内容</p> : null}
      </div>

      {settingsOpen && snapshot ? (
        <ClipboardSettingsDialog
          closing={closingDialog === "settings"}
          snapshot={snapshot}
          toolHotkey={formatHotkeyForDisplay(tool.hotkey)}
          onClearHistory={() => void invoke("clipboard_clear_history").then(async () => {
            toast("已移入回收站");
            await loadClipboard();
          })}
          onClose={() => closeClipboardDialog("settings")}
          onOpenHotkeySettings={openHotkeySettingsFromClipboardSettings}
          onUpdateSettings={updateClipboardSettings}
        />
      ) : null}

      {trashOpen ? (
        <ClipboardTrashDialog
          closing={closingDialog === "trash"}
          entries={trashEntries}
          page={trashPage}
          total={trashTotal}
          onClose={() => closeClipboardDialog("trash")}
          onOpenDetail={(entry) => setDetailEntry(entry)}
          onPageChange={(page) => void changeTrashPage(page)}
          onPurge={(ids) => void purgeTrashEntries(ids)}
          onRestore={(ids) => void restoreTrashEntries(ids)}
        />
      ) : null}

      {manualOpen ? (
        <ClipboardManualDialog
          closing={closingDialog === "manual"}
          manualText={manualText}
          manualTitle={manualTitle}
          onChangeText={setManualText}
          onChangeTitle={setManualTitle}
          onClose={() => closeClipboardDialog("manual")}
          onCreate={() => void createManual()}
        />
      ) : null}

      {detailEntry ? (
        <ClipboardDetailDialog
          closing={closingDialog === "detail"}
          entry={detailEntry}
          onClose={() => closeClipboardDialog("detail")}
        />
      ) : null}

      {extractEntry ? (
        <ClipboardExtractDialog
          closing={closingDialog === "extract"}
          entry={extractEntry}
          selectedTokens={selectedTokens}
          tokens={extractTokens}
          onClose={() => closeClipboardDialog("extract")}
          onConfirm={() => void copyExtractedTokens()}
          onToggle={(token) => setSelectedTokens((current) => current.includes(token) ? current.filter((item) => item !== token) : [...current, token])}
        />
      ) : null}

      {currentToast ? createPortal(<div className="app-toast clipboard-toast" key={currentToast.id}>{currentToast.text}</div>, document.body) : null}
    </section>
  );
}

function ClipboardSettingsDialog({
  closing,
  snapshot,
  toolHotkey,
  onClearHistory,
  onClose,
  onOpenHotkeySettings,
  onUpdateSettings,
}: {
  closing: boolean;
  snapshot: ClipboardSnapshot;
  toolHotkey: string;
  onClearHistory: () => void;
  onClose: () => void;
  onOpenHotkeySettings: () => void;
  onUpdateSettings: (patch: Partial<ClipboardSettings>) => Promise<void>;
}) {
  return createPortal(
    <div className={`dialog-backdrop ${closing ? "closing" : ""}`} onMouseDown={onClose}>
      <section className={`update-dialog clipboard-modal ${closing ? "closing" : ""}`} onMouseDown={(event) => event.stopPropagation()} role="dialog" aria-modal="true" aria-label="剪贴板设置">
        <header className="update-dialog-header">
          <div className="update-dialog-icon"><Settings size={16} /></div>
          <div>
            <h2>剪贴板设置</h2>
            <p>监听、保留策略、快捷弹窗与本地数据。</p>
          </div>
          <button aria-label="关闭" className="dialog-close-button" onClick={onClose} type="button"><X size={13} /></button>
        </header>
        <div className="clipboard-modal-body">
          <ToggleRow
            checked={snapshot.settings.listening}
            description="关闭后不再记录外部复制，已有历史仍可使用"
            label="全局监听"
            onChange={(value) => void onUpdateSettings({ listening: value })}
          />
          <div className="clipboard-setting-grid">
            <label>
              保存天数
              <select value={snapshot.settings.retentionDays} onChange={(event) => void onUpdateSettings({ retentionDays: Number(event.target.value) })}>
                <option value={7}>7 天</option>
                <option value={30}>30 天</option>
                <option value={90}>90 天</option>
                <option value={365}>365 天</option>
              </select>
            </label>
            <label>
              单条长度
              <select value={snapshot.settings.maxTextBytes} onChange={(event) => void onUpdateSettings({ maxTextBytes: Number(event.target.value) })}>
                <option value={10 * 1024}>10 KB</option>
                <option value={100 * 1024}>100 KB</option>
                <option value={1024 * 1024}>1 MB</option>
              </select>
            </label>
            <label>
              弹窗宽度
              <span className="clipboard-range-control">
                <input type="range" min={280} max={560} step={10} value={snapshot.settings.panelWidth} onChange={(event) => void onUpdateSettings({ panelWidth: Number(event.target.value) })} />
                <span>{snapshot.settings.panelWidth}px</span>
              </span>
            </label>
            <label>
              弹窗高度
              <span className="clipboard-range-control">
                <input type="range" min={300} max={900} step={10} value={snapshot.settings.panelHeight} onChange={(event) => void onUpdateSettings({ panelHeight: Number(event.target.value) })} />
                <span>{snapshot.settings.panelHeight}px</span>
              </span>
            </label>
          </div>
          <div className="clipboard-settings-summary">
            监听状态：{snapshot.listeningActive ? "运行中" : "已停止"} · 快捷键 {toolHotkey} · 跳过过长内容 {snapshot.stats.skippedTooLong}
          </div>
        </div>
        <footer className="update-dialog-actions">
          <button className="secondary-action icon-text-action" onClick={onOpenHotkeySettings} type="button"><Keyboard size={12} />快捷键设置</button>
          <button className="secondary-action" onClick={onClearHistory} type="button">清空普通历史</button>
          <button className="primary-action" onClick={onClose} type="button">完成</button>
        </footer>
      </section>
    </div>,
    document.body,
  );
}

function ClipboardTrashDialog({
  closing,
  entries,
  page,
  total,
  onClose,
  onOpenDetail,
  onPageChange,
  onPurge,
  onRestore,
}: {
  closing: boolean;
  entries: ClipboardEntry[];
  page: number;
  total: number;
  onClose: () => void;
  onOpenDetail: (entry: ClipboardEntry) => void;
  onPageChange: (page: number) => void;
  onPurge: (ids: string[]) => void;
  onRestore: (ids: string[]) => void;
}) {
  const [selectedIds, setSelectedIds] = useState<Set<string>>(new Set());
  const selectedEntries = useMemo(() => entries.filter((entry) => selectedIds.has(entry.id)), [entries, selectedIds]);
  const selecting = selectedEntries.length > 0;
  const allSelected = entries.length > 0 && entries.every((entry) => selectedIds.has(entry.id));
  const pageCount = Math.max(1, Math.ceil(total / CLIPBOARD_PAGE_SIZE));

  useEffect(() => {
    setSelectedIds((current) => new Set(entries.filter((entry) => current.has(entry.id)).map((entry) => entry.id)));
  }, [entries]);

  function toggleSelected(id: string) {
    setSelectedIds((current) => {
      const next = new Set(current);
      if (next.has(id)) {
        next.delete(id);
      } else {
        next.add(id);
      }
      return next;
    });
  }

  function restoreSelected() {
    const ids = selectedEntries.map((entry) => entry.id);
    setSelectedIds(new Set());
    onRestore(ids);
  }

  function purgeSelected() {
    const ids = selectedEntries.map((entry) => entry.id);
    setSelectedIds(new Set());
    onPurge(ids);
  }

  function toggleSelectAll() {
    setSelectedIds(allSelected ? new Set() : new Set(entries.map((entry) => entry.id)));
  }

  return createPortal(
    <div className={`dialog-backdrop ${closing ? "closing" : ""}`} onMouseDown={onClose}>
      <section className={`update-dialog clipboard-modal clipboard-trash-dialog ${closing ? "closing" : ""}`} onMouseDown={(event) => event.stopPropagation()} role="dialog" aria-modal="true" aria-label="剪贴板回收站">
        <header className="update-dialog-header">
          <div className="update-dialog-icon danger"><Trash2 size={16} /></div>
          <div>
            <h2>回收站</h2>
            <p>删除记录最长保留 30 天。</p>
          </div>
          <button aria-label="关闭" className="dialog-close-button" onClick={onClose} type="button"><X size={13} /></button>
        </header>
        <div className="clipboard-modal-body clipboard-trash-list" key={page}>
          {entries.map((entry) => (
            <article className={`clipboard-trash-entry ${selectedIds.has(entry.id) ? "selected" : ""}`} key={entry.id}>
              <label className="clipboard-entry-check" aria-label="选择回收站条目">
                <input checked={selectedIds.has(entry.id)} onChange={() => toggleSelected(entry.id)} type="checkbox" />
              </label>
              <button className="clipboard-trash-main" onClick={() => (selecting ? toggleSelected(entry.id) : onOpenDetail(entry))} type="button">
                <div className="clipboard-entry-title">
                  <strong>{entry.title || entry.text.split(/\s+/).find(Boolean) || "文本条目"}</strong>
                  <span>{sourceLabel(entry.source)}</span>
                </div>
                <p>{entry.text}</p>
              </button>
              <button className="clipboard-trash-restore" title="恢复" onClick={() => onRestore([entry.id])} type="button">
                <RotateCcw size={13} />
              </button>
            </article>
          ))}
          {entries.length === 0 ? <p className="clipboard-empty">回收站为空</p> : null}
        </div>
        <footer className={`update-dialog-actions clipboard-trash-actions ${selecting ? "selecting" : ""}`}>
          <div className="clipboard-trash-bulk-actions">
            {selecting ? (
              <>
              <button className="secondary-action icon-text-action" onClick={toggleSelectAll} type="button"><Check size={12} />{allSelected ? "取消全选" : `全选 [${selectedEntries.length}/${entries.length}]`}</button>
              <button className="secondary-action icon-text-action" onClick={restoreSelected} type="button"><RotateCcw size={12} />恢复选中</button>
              <button className="secondary-action icon-text-action danger" onClick={purgeSelected} type="button"><Trash2 size={12} />彻底删除选中</button>
              </>
            ) : null}
          </div>
          <div className="clipboard-trash-pagination">
            <button aria-label="上一页" disabled={page <= 0} onClick={() => onPageChange(page - 1)} type="button"><ChevronLeft size={13} /></button>
            <span>{page + 1} / {pageCount}</span>
            <button aria-label="下一页" disabled={page >= pageCount - 1} onClick={() => onPageChange(page + 1)} type="button"><ChevronRight size={13} /></button>
          </div>
        </footer>
      </section>
    </div>,
    document.body,
  );
}

function ClipboardManualDialog({
  closing,
  manualText,
  manualTitle,
  onChangeText,
  onChangeTitle,
  onClose,
  onCreate,
}: {
  closing: boolean;
  manualText: string;
  manualTitle: string;
  onChangeText: (value: string) => void;
  onChangeTitle: (value: string) => void;
  onClose: () => void;
  onCreate: () => void;
}) {
  return createPortal(
    <div className={`dialog-backdrop ${closing ? "closing" : ""}`} onMouseDown={onClose}>
      <section className={`update-dialog clipboard-modal ${closing ? "closing" : ""}`} onMouseDown={(event) => event.stopPropagation()} role="dialog" aria-modal="true" aria-label="新增固定文本">
        <header className="update-dialog-header">
          <div className="update-dialog-icon"><Plus size={16} /></div>
          <div>
            <h2>新增固定文本</h2>
            <p>固定后会显示在剪贴板固定页和快捷弹窗中。</p>
          </div>
          <button aria-label="关闭" className="dialog-close-button" onClick={onClose} type="button"><X size={13} /></button>
        </header>
        <div className="clipboard-modal-body">
          <label className="clipboard-modal-field">
            标题，可选
            <input value={manualTitle} onChange={(event) => onChangeTitle(event.target.value)} placeholder="标题，可选" />
          </label>
          <label className="clipboard-modal-field">
            固定文本
            <textarea autoFocus value={manualText} onChange={(event) => onChangeText(event.target.value)} placeholder="输入要固定的文本" />
          </label>
        </div>
        <footer className="update-dialog-actions">
          <button className="secondary-action" onClick={onClose} type="button">取消</button>
          <button className="primary-action icon-text-action" onClick={onCreate} type="button"><Plus size={12} />新增</button>
        </footer>
      </section>
    </div>,
    document.body,
  );
}

function ClipboardEntryCard({
  actionsOpen = false,
  compact = false,
  entry,
  onCopy,
  onDelete,
  onOpenActions,
  onOpenDetail,
  onOpenExtract,
  onPrimary,
  onSelect,
  onTogglePinned,
  registerNode,
  selectMode = false,
  selected = false,
  showDelete = true,
  showPin = true,
}: {
  actionsOpen?: boolean;
  compact?: boolean;
  entry: ClipboardEntry;
  onCopy: () => void;
  onDelete: () => void;
  onOpenActions?: (open: boolean) => void;
  onOpenDetail?: () => void;
  onOpenExtract?: () => void;
  onPrimary?: () => void;
  onSelect?: () => void;
  onTogglePinned: () => void;
  registerNode?: (node: HTMLElement | null) => void;
  selectMode?: boolean;
  selected?: boolean;
  showDelete?: boolean;
  showPin?: boolean;
}) {
  const title = entry.title || entry.text.split(/\s+/).find(Boolean) || "文本条目";
  const canShowWorkspace = !compact && showDelete && Boolean(onOpenActions);
  const dragRef = useRef<{ x: number; y: number; pointerId: number; moved: boolean; dragging: boolean } | null>(null);
  const suppressClickRef = useRef(false);
  const [dragOffset, setDragOffset] = useState(0);
  const [dragging, setDragging] = useState(false);
  const workspaceWidth = 110;
  function handlePointerDown(event: React.PointerEvent) {
    if (!canShowWorkspace || event.button !== 0) {
      return;
    }
    dragRef.current = { x: event.clientX, y: event.clientY, pointerId: event.pointerId, moved: false, dragging: false };
    setDragOffset(actionsOpen ? -workspaceWidth : 0);
  }
  function handlePointerMove(event: React.PointerEvent) {
    const drag = dragRef.current;
    if (!drag || !canShowWorkspace) {
      return;
    }
    const deltaX = event.clientX - drag.x;
    const deltaY = event.clientY - drag.y;
    if (!drag.dragging && Math.abs(deltaX) < 5 && Math.abs(deltaY) < 5) {
      return;
    }
    if (!drag.dragging && Math.abs(deltaY) > Math.abs(deltaX)) {
      dragRef.current = null;
      setDragging(false);
      setDragOffset(actionsOpen ? -workspaceWidth : 0);
      return;
    }
    drag.dragging = true;
    if (!event.currentTarget.hasPointerCapture(drag.pointerId)) {
      event.currentTarget.setPointerCapture(drag.pointerId);
    }
    setDragging(true);
    drag.moved = Math.abs(deltaX) > 8;
    const base = actionsOpen ? -workspaceWidth : 0;
    setDragOffset(Math.max(-workspaceWidth, Math.min(0, base + deltaX)));
  }
  function handlePointerUp(event: React.PointerEvent) {
    const drag = dragRef.current;
    if (!drag) {
      return;
    }
    const delta = event.clientX - drag.x;
    const open = actionsOpen ? delta > 36 ? false : true : delta < -36;
    suppressClickRef.current = drag.moved || drag.dragging;
    dragRef.current = null;
    setDragging(false);
    setDragOffset(open ? -workspaceWidth : 0);
    onOpenActions?.(open);
    window.setTimeout(() => {
      suppressClickRef.current = false;
    }, 120);
  }
  function handlePointerCancel() {
    dragRef.current = null;
    setDragging(false);
    setDragOffset(actionsOpen ? -workspaceWidth : 0);
  }
  function openDetailFromClick() {
    if (suppressClickRef.current) {
      return;
    }
    if (selectMode && onSelect) {
      onSelect();
      return;
    }
    onPrimary?.() ?? onOpenDetail?.();
  }
  return (
    <article ref={registerNode} className={`clipboard-entry-card ${compact ? "compact" : ""} ${selected ? "selected" : ""} ${actionsOpen ? "actions-open" : ""} ${dragging ? "dragging" : ""}`}>
      <div
        className="clipboard-entry-track"
        onPointerCancel={handlePointerCancel}
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={handlePointerUp}
        style={canShowWorkspace ? { transform: `translateX(${dragging ? dragOffset : actionsOpen ? -workspaceWidth : 0}px)` } : undefined}
      >
        <div className="clipboard-entry-surface">
          {!compact ? (
            <label className="clipboard-entry-check" aria-label="选择条目">
              <input checked={selected} onChange={onSelect} onClick={(event) => event.stopPropagation()} type="checkbox" />
            </label>
          ) : null}
          <button className="clipboard-entry-main" onClick={openDetailFromClick} type="button">
            <div className="clipboard-entry-title">
              {entry.pinnedAt ? <Pin size={12} fill="currentColor" /> : null}
              <strong>{title}</strong>
              <span>{sourceLabel(entry.source)}</span>
            </div>
            <p>{entry.text}</p>
            {!compact ? (
              <div className="clipboard-entry-meta">
                {formatClipboardDate(entry.createdAt)} · 复制 {entry.copyCount} 次 · 使用 {entry.useCount} 次
              </div>
            ) : null}
          </button>
        </div>
        {canShowWorkspace ? (
          <div className="clipboard-entry-workspace">
            <button title="分词" onClick={onOpenExtract} type="button"><Scissors size={14} /></button>
            <button className="danger" title="删除" onClick={onDelete} type="button"><Trash2 size={14} /></button>
          </div>
        ) : null}
      </div>
      <div className="clipboard-entry-actions">
        <button title="复制" onClick={onCopy} type="button"><Copy size={13} /></button>
        {showPin ? <button title={entry.pinnedAt ? "取消固定" : "固定"} onClick={onTogglePinned} type="button"><Pin size={13} fill={entry.pinnedAt ? "currentColor" : "none"} /></button> : null}
        {showDelete && !canShowWorkspace ? <button title="删除" onClick={onDelete} type="button"><Trash2 size={13} /></button> : null}
      </div>
    </article>
  );
}

function ClipboardDetailDialog({ closing, entry, onClose }: { closing: boolean; entry: ClipboardEntry; onClose: () => void }) {
  return createPortal(
    <div className={`dialog-backdrop ${closing ? "closing" : ""}`} onMouseDown={onClose}>
      <section className={`update-dialog clipboard-detail-dialog ${closing ? "closing" : ""}`} onMouseDown={(event) => event.stopPropagation()} role="dialog" aria-modal="true" aria-label="条目详情">
        <header className="update-dialog-header">
          <div className="update-dialog-icon"><Info size={16} /></div>
          <div>
            <h2>条目详情</h2>
          </div>
          <button aria-label="关闭" className="dialog-close-button" onClick={onClose} type="button"><X size={13} /></button>
        </header>
        <div className="clipboard-detail-body">
          <pre>{entry.text}</pre>
          <details>
            <summary>查看元数据</summary>
            <div className="clipboard-detail-meta">
              <span>来源：{sourceLabel(entry.source)}</span>
              <span>长度：{entry.text.length}</span>
              <span>创建：{formatClipboardDateTime(entry.createdAt)}</span>
              <span>复制：{formatClipboardDateTime(entry.lastCopiedAt)}</span>
              <span>使用：{formatClipboardDateTime(entry.lastUsedAt)}</span>
              <span>固定：{formatClipboardDateTime(entry.pinnedAt)}</span>
              <span>删除：{formatClipboardDateTime(entry.deletedAt)}</span>
              <span>ID：{entry.id.slice(0, 8)}</span>
            </div>
          </details>
        </div>
      </section>
    </div>,
    document.body,
  );
}

function ClipboardExtractDialog({
  closing,
  entry,
  onClose,
  onConfirm,
  onToggle,
  selectedTokens,
  tokens,
}: {
  closing: boolean;
  entry: ClipboardEntry;
  onClose: () => void;
  onConfirm: () => void;
  onToggle: (token: string) => void;
  selectedTokens: string[];
  tokens: string[];
}) {
  return createPortal(
    <div className={`dialog-backdrop ${closing ? "closing" : ""}`} onMouseDown={onClose}>
      <section className={`update-dialog clipboard-extract-dialog ${closing ? "closing" : ""}`} onMouseDown={(event) => event.stopPropagation()} role="dialog" aria-modal="true" aria-label="分词提取">
        <header className="update-dialog-header">
          <div className="update-dialog-icon"><Scissors size={16} /></div>
          <div>
            <h2>分词提取</h2>
            <p>{entry.title || "点击片段多选，提取后复制到系统剪贴板。"}</p>
          </div>
          <div className="clipboard-dialog-inline-actions">
            <button className="secondary-action" disabled={selectedTokens.length === 0} onClick={onConfirm} type="button">提取</button>
            <button aria-label="关闭" className="dialog-close-button" onClick={onClose} type="button"><X size={13} /></button>
          </div>
        </header>
        <div className="clipboard-token-grid">
          {tokens.map((token, index) => (
            <button className={selectedTokens.includes(token) ? "selected" : ""} key={`${token}-${index}`} onClick={() => onToggle(token)} type="button">
              {token}
            </button>
          ))}
          {tokens.length === 0 ? <p className="clipboard-empty">没有可提取片段</p> : null}
        </div>
      </section>
    </div>,
    document.body,
  );
}

function formatClipboardDate(value: number | null) {
  if (!value) {
    return "-";
  }
  return new Date(value).toLocaleString("zh-CN", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
  });
}

function formatClipboardDateTime(value: number | null) {
  if (!value) {
    return "-";
  }
  return new Date(value).toLocaleString("zh-CN");
}

function formatStorageSize(bytes: number) {
  if (bytes < 1024) {
    return `${bytes} B`;
  }
  const units = ["KB", "MB", "GB"];
  let value = bytes / 1024;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }
  return `${value.toFixed(1)} ${units[unitIndex]}`;
}

function sourceLabel(source: string) {
  if (source === "manual") {
    return "手动";
  }
  if (source === "derived") {
    return "分词提取";
  }
  return "复制";
}

function extractClipboardTokens(text: string) {
  const matches = text.match(/[a-zA-Z]+:\/\/[^\s"'<>]+|[a-zA-Z]:\\[^\r\n]+|\\\\[^\s]+|[\w.-]+@[\w.-]+\.[A-Za-z]{2,}|[\w.-]+\.[A-Za-z]{2,}(?:\/[^\s]*)?|--?[A-Za-z][\w-]*|v?\d+(?:\.\d+){1,}|[\u4e00-\u9fa5]{2,}|[A-Za-z0-9_./\\:-]{4,}/g) ?? [];
  return Array.from(new Set(matches)).slice(0, 80);
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

function AboutSettingsPanel({
  coldStartupMs,
  settings,
  updateChecker,
  updateSettings,
}: {
  coldStartupMs: number;
  settings: AppSettings;
  updateChecker: ReturnType<typeof useUpdateChecker>;
  updateSettings: (patch: SettingsPatch) => Promise<void>;
}) {
  const [copiedEmail, setCopiedEmail] = useState<string | null>(null);

  async function copyEmail(email: string) {
    await navigator.clipboard.writeText(email);
    setCopiedEmail(email);
    window.setTimeout(() => setCopiedEmail(null), 1200);
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
          <p>{updateChecker.updateStatus}</p>
          <p>冷启动基线：{coldStartupMs} ms</p>
        </div>
        <button className="secondary-action icon-text-action" disabled={updateChecker.checkingUpdates} onClick={() => void updateChecker.checkUpdates()} type="button">
          <RefreshCw className={updateChecker.checkingUpdates ? "spin-icon" : ""} size={13} />
          {updateChecker.checkingUpdates ? "检查中" : "检查更新"}
        </button>
      </div>
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
