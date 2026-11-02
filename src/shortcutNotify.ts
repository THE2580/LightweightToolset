import { invoke } from "@tauri-apps/api/core";

const MODIFIER_KEYS = new Set(["Control", "Alt", "Shift", "Meta", "OS", "AltGraph"]);
const KEY_NAMES: Record<string, string> = {
  ArrowUp: "Up",
  ArrowDown: "Down",
  ArrowLeft: "Left",
  ArrowRight: "Right",
  Escape: "Esc",
  " ": "Space",
  Spacebar: "Space",
  Enter: "Enter",
  Tab: "Tab",
  Backspace: "Backspace",
  Delete: "Delete",
  Insert: "Insert",
  Home: "Home",
  End: "End",
  PageUp: "PageUp",
  PageDown: "PageDown",
};

/** 把一次按键事件拼成与后端一致的 `Ctrl+Shift+F` 形式；无法识别的键返回 null。 */
export function chordFromEvent(event: KeyboardEvent): string | null {
  if (MODIFIER_KEYS.has(event.key)) {
    return null;
  }
  let key = KEY_NAMES[event.key];
  if (!key && event.key.length === 1) {
    key = event.key.toUpperCase();
  }
  if (!key && /^F([1-9]|1[0-9]|2[0-4])$/.test(event.key)) {
    key = event.key;
  }
  if (!key) {
    return null;
  }
  const parts: string[] = [];
  if (event.ctrlKey) {
    parts.push("Ctrl");
  }
  if (event.altKey) {
    parts.push("Alt");
  }
  if (event.shiftKey) {
    parts.push("Shift");
  }
  if (event.metaKey) {
    parts.push("Win");
  }
  parts.push(key);
  return parts.join("+");
}

/**
 * 应用内快捷键上报。
 *
 * WebView2（Chromium）窗口获得焦点时，Windows 会间断地不给全局低级键盘钩子
 * 派发按键，因此软件自己的窗口用页面 keydown 上报，由「快捷键提示」工具统一
 * 匹配并弹浮窗（与全局钩子去重、节流）。
 */
export function installShortcutNotifyBridge() {
  function handleKeyDown(event: KeyboardEvent) {
    const chord = chordFromEvent(event);
    if (!chord) {
      return;
    }
    void invoke("shortcut_tip_report", { keys: chord }).catch(() => {});
  }

  window.addEventListener("keydown", handleKeyDown, true);
  return () => window.removeEventListener("keydown", handleKeyDown, true);
}
