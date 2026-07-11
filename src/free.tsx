import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { Copy, Moon, Pause, Play, RotateCcw, Square, Sun, X } from "lucide-react";
import React, { useEffect, useMemo, useRef, useState } from "react";
import ReactDOM from "react-dom/client";
import "./free.css";

type TimerKind = "stopwatch" | "countdown";
type TimerStatus = "paused" | "running" | "finished";
type ThemeMode = "light" | "dark" | "system";

type TimerEntry = {
  id: string;
  name: string;
  note: string;
  kind: TimerKind;
  status: TimerStatus;
  elapsedMs: number;
  durationMs: number | null;
  remainingMs: number | null;
  notificationsEnabled: boolean;
};

type TimerSnapshot = {
  timers: TimerEntry[];
};

function FreeWindowApp() {
  const params = useMemo(() => new URLSearchParams(window.location.search), []);
  const kind = params.get("kind");
  const id = params.get("id") ?? "";

  useEffect(() => {
    const preventContextMenu = (event: MouseEvent) => event.preventDefault();
    window.addEventListener("contextmenu", preventContextMenu);
    return () => window.removeEventListener("contextmenu", preventContextMenu);
  }, []);

  if (kind === "clock") {
    return <ClockFreeWindow />;
  }
  if (kind === "timer" && id) {
    return <TimerFreeWindow id={id} />;
  }
  return <Shell title="自由窗口"><p className="free-empty">窗口参数无效</p></Shell>;
}

function Shell({
  title,
  badge,
  kind,
  status,
  children,
}: {
  title: string;
  badge?: string;
  kind?: TimerKind;
  status?: TimerStatus;
  children: React.ReactNode;
}) {
  const [isMaximized, setIsMaximized] = useState(false);
  const [resolvedTheme, setResolvedTheme] = useState<"light" | "dark">("light");
  const manualThemeRef = useRef<"light" | "dark" | null>(null);

  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    let appTheme: ThemeMode = "system";
    let disposeThemeListener: (() => void) | undefined;
    const applyTheme = () => {
      if (manualThemeRef.current) return;
      const nextTheme = appTheme === "system" ? (media.matches ? "dark" : "light") : appTheme;
      document.documentElement.dataset.theme = nextTheme;
      setResolvedTheme(nextTheme);
    };
    media.addEventListener("change", applyTheme);
    void listen<ThemeMode>("app-theme-changed", ({ payload }) => {
      appTheme = payload;
      applyTheme();
    }).then((dispose) => {
      disposeThemeListener = dispose;
    });
    invoke<{ settings?: { theme?: ThemeMode } }>("get_app_snapshot")
      .then((snapshot) => {
        appTheme = snapshot.settings?.theme ?? "system";
        applyTheme();
      })
      .catch(applyTheme);
    return () => {
      media.removeEventListener("change", applyTheme);
      disposeThemeListener?.();
    };
  }, []);

  useEffect(() => {
    const currentWindow = getCurrentWindow();
    const syncMaximized = () => void currentWindow.isMaximized().then(setIsMaximized).catch(() => undefined);
    syncMaximized();
    let unlisten: (() => void) | undefined;
    void currentWindow.onResized(() => syncMaximized()).then((cleanup) => {
      unlisten = cleanup;
    });
    return () => {
      unlisten?.();
    };
  }, []);

  async function closeWindow(event: React.PointerEvent<HTMLButtonElement> | React.MouseEvent<HTMLButtonElement>) {
    event.preventDefault();
    event.stopPropagation();
    await getCurrentWindow().close();
  }

  async function toggleMaximized(event: React.MouseEvent<HTMLElement>) {
    event.preventDefault();
    event.stopPropagation();
    const currentWindow = getCurrentWindow();
    await currentWindow.toggleMaximize();
    setIsMaximized(await currentWindow.isMaximized());
  }

  function toggleTheme(event: React.MouseEvent<HTMLButtonElement>) {
    event.preventDefault();
    event.stopPropagation();
    const nextTheme = resolvedTheme === "dark" ? "light" : "dark";
    manualThemeRef.current = nextTheme;
    document.documentElement.dataset.theme = nextTheme;
    setResolvedTheme(nextTheme);
  }

  return (
    <main className={`free-shell ${status ?? ""}`}>
      <header className="free-titlebar" onPointerDown={() => void getCurrentWindow().startDragging()}>
        <span className="free-window-title">{title}</span>
        <div className="free-title-actions">
          {badge ? <span className={`free-type-badge ${kind ?? ""}`}>{badge}</span> : null}
          <button aria-label={`切换为${resolvedTheme === "dark" ? "浅色" : "深色"}主题`} onClick={toggleTheme} onPointerDown={(event) => event.stopPropagation()} title={`切换为${resolvedTheme === "dark" ? "浅色" : "深色"}主题`} type="button">{resolvedTheme === "dark" ? <Sun size={18} /> : <Moon size={18} />}</button>
          <button aria-label={isMaximized ? "还原窗口" : "窗口化全屏"} onClick={(event) => void toggleMaximized(event)} onPointerDown={(event) => event.stopPropagation()} title={isMaximized ? "还原窗口" : "窗口化全屏"} type="button">{isMaximized ? <Copy size={18} strokeWidth={2} /> : <Square size={17} strokeWidth={2} />}</button>
          <button aria-label="关闭" onClick={(event) => void closeWindow(event)} onPointerDown={(event) => event.stopPropagation()} type="button"><X size={18} /></button>
        </div>
      </header>
      {children}
    </main>
  );
}

function TimerFreeWindow({ id }: { id: string }) {
  const [timer, setTimer] = useState<TimerEntry | null>(null);
  const [error, setError] = useState("");

  async function load() {
    try {
      const snapshot = await invoke<TimerSnapshot>("timer_get_snapshot");
      setTimer(snapshot.timers.find((item) => item.id === id) ?? null);
      setError("");
    } catch (reason) {
      setError(String(reason));
    }
  }

  async function run(command: string) {
    try {
      await invoke(command, { id });
      await load();
    } catch (reason) {
      setError(String(reason));
    }
  }

  useEffect(() => {
    void load();
    const interval = window.setInterval(() => void load(), 500);
    return () => window.clearInterval(interval);
  }, [id]);

  if (!timer) {
    return <Shell title="计时器"><p className="free-empty">{error || "计时器不存在"}</p></Shell>;
  }

  const isRunning = timer.status === "running";
  const hasStarted = timer.kind === "countdown"
    ? Boolean(timer.durationMs && timer.remainingMs !== timer.durationMs)
    : timer.elapsedMs > 0;
  const visualStatus = timer.status === "paused" && !hasStarted ? undefined : timer.status;
  const showReset = timer.status === "running" || timer.status === "finished" || (timer.status === "paused" && hasStarted);

  return (
    <Shell title={timer.name} badge={timer.kind === "countdown" ? "倒计时" : "正计时"} kind={timer.kind} status={visualStatus}>
      <section className={`free-timer-card ${timer.status}`}>
        <div className="free-timer-panel">
          <div className="free-status">{timerStatusLabel(timer)}</div>
          <div className="free-readout">{formatTimerReadout(timer)}</div>
          {timer.kind === "countdown" && timer.durationMs ? <div className="free-duration">原始 {formatDuration(timer.durationMs)}</div> : null}
          {error ? <p className="free-error">{error}</p> : null}
          <div className="free-actions">
            {isRunning ? (
              <button className="primary" onClick={() => void run("timer_pause")} title="暂停" type="button"><Pause size={24} /></button>
            ) : timer.status !== "finished" ? (
              <button className="primary" onClick={() => void run("timer_start")} title="开始" type="button"><Play size={24} /></button>
            ) : null}
            {showReset ? (
              <button onClick={() => void run("timer_reset")} title="重置" type="button"><RotateCcw size={24} /></button>
            ) : null}
          </div>
        </div>
      </section>
    </Shell>
  );
}

function ClockFreeWindow() {
  const [now, setNow] = useState(() => new Date());
  useEffect(() => {
    const interval = window.setInterval(() => setNow(new Date()), 1000);
    return () => window.clearInterval(interval);
  }, []);

  return (
    <Shell title="本地时间" badge="时钟">
      <section className="free-clock-card">
        <div className="free-clock-panel">
          <span>{formatDate(now)}</span>
          <strong>{formatTime(now)}</strong>
        </div>
      </section>
    </Shell>
  );
}

function timerStatusLabel(timer: TimerEntry) {
  if (timer.status === "paused") {
    return timer.elapsedMs > 0 ? "已暂停" : "未开始";
  }
  return ({ running: "运行中", finished: "已结束" } as Record<Exclude<TimerStatus, "paused">, string>)[timer.status];
}

function formatTimerReadout(timer: TimerEntry) {
  const ms = timer.kind === "countdown" ? timer.remainingMs ?? 0 : timer.elapsedMs;
  return formatDuration(ms);
}

function formatDuration(ms: number) {
  const totalSeconds = Math.max(0, Math.floor(ms / 1000));
  const hours = Math.floor(totalSeconds / 3600);
  const minutes = Math.floor((totalSeconds % 3600) / 60);
  const seconds = totalSeconds % 60;
  return [hours, minutes, seconds].map((value) => String(value).padStart(2, "0")).join(":");
}

function formatDate(value: Date) {
  return value.toLocaleDateString("zh-CN", { year: "numeric", month: "2-digit", day: "2-digit", weekday: "short" });
}

function formatTime(value: Date) {
  return value.toLocaleTimeString("zh-CN", { hour12: false });
}

ReactDOM.createRoot(document.getElementById("free-root") as HTMLElement).render(
  <React.StrictMode>
    <FreeWindowApp />
  </React.StrictMode>,
);
