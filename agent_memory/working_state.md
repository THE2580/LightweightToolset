# Scope

- 当前范围：剪贴板工具优化迭代（用户逐项给出）。已实现第 1–6 项，其中第 1–5 项已验收通过；第 6 项（系统通知）待用户真机验证。
- 其他工具、P0 相关代码不在本轮范围内。

# Git / Working Tree

- 工作目录：`E:\DeepSeek_Harness\Workspace\LightweightToolset`；分支 `master`；HEAD 为本次 `feat: improve clipboard workflows and notifications` 提交（本地，未推送），领先 `origin/master` 2 个提交。
- 既有现场（不处理、不提交）：`M src-tauri/src/window_service.rs`（纯 rustfmt 噪声）、`?? .codex/`。
- 本轮改动已全部提交（剪贴板 6 项 + 记忆更新）；提交未包含 `window_service.rs` 与 `.codex/`。

# Active Files

- `src-tauri/src/clipboard.rs`：`purge_trash()`；`ClipboardSettings.notifications`、`notifications_enabled()`；启停 `Ctrl+C` 钩子与清空通知队列。
- `src-tauri/src/clipboard_notify.rs`：单浮窗 OSD（无队列）、×N 计数、2s 计时、800ms 复制/剪切意图窗口、`WM_CLIPBOARDUPDATE` 确认、低级键盘钩子（忽略注入键、按住 0.5s 节流）、宽度随位数增长、ready 握手。
- `src/notification.tsx` / `notification.html` / `src/notification.css`：WinTips 风格提示页，图标 + 文案 + ×N 徽章，卡片淡入淡出 300ms、图标/徽章弹跳。
- `src-tauri/src/lib.rs`：注册模块与 `clipboard_notify_ready`、粘贴后 `notify_pasted()`、设置更新后 `refresh()`、`setup` 中 `clipboard_notify::init`。
- `src/App.tsx`：回收站“全部清空”+二次确认、`Ctrl+A`、剪贴板设置“系统通知”开关、`ClipboardSettings.notifications`。
- `src/popup.tsx` / `src/popup.css`：仅图标分段、全局搜索、搜索态铺满、220ms 过渡、焦点驻留。

# Verification

- 2026-10-31 `npx tsc --noEmit` 通过（通知功能加入后再次通过）。
- 2026-10-31 `npm run build` 通过（最新 7.64s，已生成 notification 入口）。
- 2026-10-31 `cargo check` 通过（无警告）；`cargo build` 通过（细节修正后 35.76s）。
- 2026-10-31 `npx tsc --noEmit` 通过；`npm run build` 通过（6.09s）。
- 2026-10-31 串行全量单测 12/12 通过（`TEMP`/`TMP` 需指向 `src-tauri/target/test-tmp`，见 `bugs.md`）。
- 开发版已重启：Vite `http://localhost:1420`（后台任务 `pwsh-fdd9ad1fdcc449e2`）+ `%TEMP%\lt-dev-run\` 的 debug EXE（pid 18424）。
- 未做：浮窗外观、×N 宽度自适应、按住 0.5s 节流与图标动效未真机验证；注入键被忽略，自动化无法模拟真实复制。

# Blockers

- 无。系统 `%TEMP%` 写入限制与工作区 EXE 沙箱限制有绕行方案，见 `bugs.md`。

# Next Action

- 等用户验证通知：真实 `Ctrl+C`/`Ctrl+X` 应在剪贴板更新后弹“已复制/已剪切”，`Ctrl+V` 立即弹“已粘贴”；连续同动作显示 ×N；关闭“系统通知”或停用工具后不弹。
- 若通知窗口未出现，先看应用内调试日志与 `%TEMP%\lt-dev-run\stderr.log`；必要时在 `clipboard_notify.rs` 的 `ensure_window/show_notification` 增加日志。
- 提交需用户明确授权；提交时排除 `window_service.rs` 与 `.codex/`。
