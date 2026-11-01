# Scope

- 当前范围：`v0.5.0` 装机后回归修复，已完成并经用户真机验收：
  1. 「关于」页版号改为读 `package.json`（此前写死 `0.4.4`，装机后误报 `v0.5.0` 更新）。
  2. 软件自身窗口内的 `Ctrl+C / Ctrl+X / Ctrl+V` 现在都会弹浮窗，累加按 0.5s 节流、松手即停，剪切不再延迟/丢弹。
- 待确认项：通知浮窗**置顶层级**（每次显示 `SetWindowPos(HWND_TOPMOST)`）尚未得到用户复测反馈。
- 其他工具、P0 相关代码不在本轮范围内。

# Git / Working Tree

- 工作目录：`E:\DeepSeek_Harness\Workspace\LightweightToolset`；分支 `master`；`v0.5.0` 已发布（tag 指向 `8c68e2b`）。
- 本轮改动**未提交**：`src/App.tsx`、`src/popup.tsx`、`src/free.tsx`、`src/shortcutNotify.ts`（新增）、`src-tauri/src/lib.rs`、`src-tauri/src/clipboard_notify.rs`、`agent_memory/*`。
- 既有现场（不处理、不提交）：`M src-tauri/src/window_service.rs`（纯 rustfmt 噪声）、`?? .codex/`。

# Active Files

- `src/shortcutNotify.ts`：三个应用窗口共用的快捷键上报；`Ctrl+C/X` 先判断有无选中内容（输入框选区或 `getSelection()`），空选不弹。
- `src-tauri/src/clipboard_notify.rs`：`notify_ui_shortcut()` 立即弹提示；`claim_report()` 负责 0.5s 同源节流 + 400ms 跨源去重；`raise_to_top()` 每次显示 `SetWindowPos(HWND_TOPMOST)`；全局钩子只服务其他程序。
- `src-tauri/src/lib.rs`：新增命令 `clipboard_notify_shortcut`；`clipboard_copy*` 成功后 `notify_copied_now()`；`clipboard_paste` 走 `notify_pasted_from_command()`。

# Verification

- 2026-11-01 `npm run build` 通过；`cargo check` 通过；串行全量单测 11/11 通过；debug 重编译通过。
- 2026-11-01 用户真机验收通过：软件内三种快捷键都弹、长按松手即停、剪切即时且不漏弹、空选不弹（用户明确接受空选不弹）。
- 未验证：浮窗压过其他置顶窗口的层级表现。

# Blockers

- 无。注意：不要对仓库 Rust 文件直接跑 `rustfmt`（会按 100 列重排整文件）。

# Next Action

- 等用户确认置顶层级；若通过，询问是否授权发 `v0.5.1`（需同步 `package.json` / `Cargo.toml` / `tauri.conf.json` 三处版本号，并提交推送 + tag + Release）。
- 提交需用户明确授权；提交时排除 `window_service.rs` 与 `.codex/`。
