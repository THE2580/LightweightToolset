import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { Check, ClipboardCheck, Scissors } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import ReactDOM from "react-dom/client";
import "./notification.css";

type TipKind = "copied" | "cut" | "pasted";
type TipSide = "top" | "bottom" | "left" | "right";
type ThemeMode = "light" | "dark" | "system";

type TipPayload = {
  kind: TipKind;
  count: number;
  side: TipSide;
};

const LABELS: Record<TipKind, string> = {
  copied: "已复制",
  cut: "已剪切",
  pasted: "已粘贴",
};

function TipIcon({ kind }: { kind: TipKind }) {
  if (kind === "cut") {
    return <Scissors size={19} />;
  }
  if (kind === "pasted") {
    return <ClipboardCheck size={19} />;
  }
  return <Check size={19} strokeWidth={3} />;
}

function NotificationApp() {
  const [tip, setTip] = useState<TipPayload>({ kind: "copied", count: 1, side: "bottom" });
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
      disposeShow = await listen<TipPayload>("clipboard-notify-show", ({ payload }) => {
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
      disposeHide = await listen("clipboard-notify-hide", () => {
        visibleRef.current = false;
        setVisible(false);
      });
      if (!disposed) {
        void invoke("clipboard_notify_ready");
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
      <span className={`notify-icon ${tip.kind}`} key={`icon-${tip.kind}-${flash}`}>
        <TipIcon kind={tip.kind} />
      </span>
      <span className="notify-text">{LABELS[tip.kind]}</span>
      <span className="notify-badge" key={`${tip.kind}-${tip.count}-${flash}`}>
        ×{tip.count}
      </span>
    </div>
  );
}

ReactDOM.createRoot(document.getElementById("notification-root") as HTMLElement).render(
  <NotificationApp />,
);
