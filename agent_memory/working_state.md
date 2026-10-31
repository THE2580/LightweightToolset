# Scope

- 当前范围：剪贴板工具优化迭代（用户逐项给出）。已实现第 1–6 项，其中第 1–5 项已验收通过；第 6 项（系统通知）待用户真机验证。
- 其他工具、P0 相关代码不在本轮范围内。

# Git / Working Tree

- 工作目录：`E:\DeepSeek_Harness\Workspace\LightweightToolset`；分支 `master`；HEAD `76f438f`，已推送 `origin/master`，工作树与远程同步。
- 既有现场（不处理、不提交）：`M src-tauri/src/window_service.rs`（纯 rustfmt 噪声）、`?? .codex/`。
- 剪贴板迭代全部改动已提交并推送；提权已正式落地（`9ee7668 feat: run application as administrator`），按键统计 helper 已移除（待提交）。工作树仅剩 `window_service.rs` 与 `.codex/` 现场。

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
- 2026-10-31 `npx tsc --noEmit` 通过；`npm run build` 通过（分词定位版 5.79s）。
- 2026-10-31 分词抽样（修正后）：路径整体保留、后续句子正常切词、三个“方案”独立；全选提取含首尾标点可还原原句。
- 2026-10-31 串行全量单测 12/12 通过（`TEMP`/`TMP` 需指向 `src-tauri/target/test-tmp`，见 `bugs.md`）。
- 开发版已重启：Vite `http://localhost:1420`（后台任务 `pwsh-fdd9ad1fdcc449e2`）+ `%TEMP%\lt-dev-run\` 的 debug EXE（pid 24348（提权实验版，`elevated=True`）。
- 2026-10-31 CDP 实测：窗口 CSS 视口 676×443（135% 缩放）；设置弹窗最终固定 400px，落在 403px 可用区中央（预计 21.5px 上下留白），主体可滚动。
- 未做：浮窗外观、×N 宽度自适应、按住 0.5s 节流与图标动效未真机验证；注入键被忽略，自动化无法模拟真实复制。

# Blockers

- 无。系统 `%TEMP%` 写入限制与工作区 EXE 沙箱限制有绕行方案，见 `bugs.md`。

# Next Action

- 当前重点：验证提权版在管理员窗口（任务管理器/管理员终端）复制、剪切、粘贴是否弹通知；同时观察每次启动 UAC 与整体高权限运行的体感。
- 若要退回普通权限：还原 `src-tauri/build.rs` 为 `tauri_build::build()`、删除 `src-tauri/windows-app-manifest.xml`，重新 build 并重启。
- 若通知窗口未出现，先看应用内调试日志与 `%TEMP%\lt-dev-run\stderr.log`；必要时在 `clipboard_notify.rs` 的 `ensure_window/show_notification` 增加日志。
- 提交需用户明确授权；提交时排除 `window_service.rs` 与 `.codex/`。
