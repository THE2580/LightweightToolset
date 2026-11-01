import { invoke } from "@tauri-apps/api/core";

export type ShortcutKind = "copied" | "cut" | "pasted";

/** 当前窗口里是否有可复制/剪切的选中内容（输入框选区与普通选区都算）。 */
function hasSelection() {
  const active = document.activeElement;
  if (active instanceof HTMLInputElement || active instanceof HTMLTextAreaElement) {
    const { selectionStart, selectionEnd } = active;
    if (typeof selectionStart === "number" && typeof selectionEnd === "number") {
      return selectionStart !== selectionEnd;
    }
  }
  return (window.getSelection()?.toString() ?? "").length > 0;
}

/**
 * 应用内快捷键上报。
 *
 * WebView2（Chromium）窗口获得焦点时，全局低级键盘钩子可能收不到按键
 * （见 `clipboard_notify.rs` 中的说明），因此软件自己的窗口用页面 keydown
 * 上报 Ctrl+C / Ctrl+X / Ctrl+V，由后端与钩子上报去重、节流后统一弹提示。
 *
 * 复制/剪切要求确实有选中内容，避免空选也弹“已复制”。
 */
export function installShortcutNotifyBridge() {
  function handleKeyDown(event: KeyboardEvent) {
    if (event.altKey || event.shiftKey || !(event.ctrlKey || event.metaKey)) {
      return;
    }
    const key = event.key.toLowerCase();
    let kind: ShortcutKind | null = null;
    if (key === "v") {
      kind = "pasted";
    } else if (key === "c" && hasSelection()) {
      kind = "copied";
    } else if (key === "x" && hasSelection()) {
      kind = "cut";
    }
    if (!kind) {
      return;
    }
    void invoke("clipboard_notify_shortcut", { kind }).catch(() => {});
  }

  window.addEventListener("keydown", handleKeyDown, true);
  return () => window.removeEventListener("keydown", handleKeyDown, true);
}
