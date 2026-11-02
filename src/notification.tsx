import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  AppWindow,
  ClipboardPaste,
  Copy,
  FilePlus2,
  Keyboard,
  Redo2,
  RotateCcw,
  Save,
  Scissors,
  Search,
  SquareDashed,
  SquareX,
  Undo2,
} from "lucide-react";
import { useEffect, useRef, useState } from "react";
import ReactDOM from "react-dom/client";
import "./notification.css";

type TipSide = "top" | "bottom" | "left" | "right";
type ThemeMode = "light" | "dark" | "system";

type TipPayload = {
  title: string;
  keys?: string | null;
  preset?: boolean;
  count: number;
  side: TipSide;
};

const PRESET_ICONS: Record<string, typeof Copy> = {
  "Ctrl+C": Copy,
  "Ctrl+V": ClipboardPaste,
  "Ctrl+X": Scissors,
  "Ctrl+A": SquareDashed,
  "Ctrl+Z": Undo2,
  "Ctrl+Y": Redo2,
  "Ctrl+S": Save,
  "Ctrl+F": Search,
  "Ctrl+N": FilePlus2,
  "Ctrl+W": SquareX,
  "Ctrl+Shift+T": RotateCcw,
  "Alt+Tab": AppWindow,
};

function TipIcon({ keys, preset }: { keys?: string | null; preset?: boolean }) {
  const Icon = (preset && keys && PRESET_ICONS[keys]) || Keyboard;
  return <Icon size={17} />;
}

function NotificationApp() {
  const [tip, setTip] = useState<TipPayload>({ title: "", keys: null, count: 1, side: "bottom" });
  const [visible, setVisible] = useState(false);
  const [flash, setFlash] = useState(0);
  const visibleRef = useRef(false);

  useEffect(() => {
    const media = window.matchMedia("(prefers-color-scheme: dark)");
    let theme: ThemeMode = "system";
    let disposeThemeListener: (() => void) | undefined;
    const applyTheme = () => {
      document.documentElement.dataset.theme = theme === "system" ? (media.matches ? "dark" : "light") : theme;
    };
    const handleSystemTheme = () => applyTheme();
    media.addEventListener("change", handleSystemTheme);
    void listen<ThemeMode>("app-theme-changed", ({ payload }) => {
      theme = payload;
      applyTheme();
    }).then((dispose) => {
      disposeThemeListener = dispose;
    });
    invoke<{ settings?: { theme?: ThemeMode } }>("get_app_snapshot")
      .then((snapshot) => {
        theme = snapshot.settings?.theme ?? "system";
        applyTheme();
      })
      .catch(applyTheme);

    let disposeShow: (() => void) | undefined;
    let disposeHide: (() => void) | undefined;
    let disposed = false;
    void (async () => {
      disposeShow = await listen<TipPayload>("shortcut-tip-show", ({ payload }) => {
        setTip(payload);
        setFlash((current) => current + 1);
        if (!visibleRef.current) {
          setVisible(false);
          window.requestAnimationFrame(() => {
            window.requestAnimationFrame(() => {
              visibleRef.current = true;
              setVisible(true);
            });
          });
        }
      });
      disposeHide = await listen("shortcut-tip-hide", () => {
        visibleRef.current = false;
        setVisible(false);
      });
      if (!disposed) {
        void invoke("shortcut_tip_ready");
      }
    })();

    return () => {
      disposed = true;
      media.removeEventListener("change", handleSystemTheme);
      disposeThemeListener?.();
      disposeShow?.();
      disposeHide?.();
    };
  }, []);

  return (
    <div className={`notify-card side-${tip.side} ${visible ? "visible" : ""}`}>
      <span className="notify-icon" key={`icon-${flash}`}>
        <TipIcon keys={tip.keys} preset={tip.preset} />
      </span>
      <span className="notify-text">{tip.title}</span>
      {tip.keys ? <kbd className="notify-keys">{tip.keys}</kbd> : null}
      <span className="notify-badge" key={`${tip.title}-${tip.count}-${flash}`}>
        ×{tip.count}
      </span>
    </div>
  );
}

ReactDOM.createRoot(document.getElementById("notification-root") as HTMLElement).render(
  <NotificationApp />,
);
