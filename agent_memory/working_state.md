# Scope

- 当前范围：`v0.5.0` 装机后回归修复，已发布 `v0.5.1` 并经用户真机验收：
  1. 「关于」页版号改为读 `package.json`（此前写死 `0.4.4`，装机后误报 `v0.5.0` 更新）。
  2. 软件自身窗口内的 `Ctrl+C / Ctrl+X / Ctrl+V` 都会弹浮窗，累加按 0.5s 节流、松手即停，剪切不再延迟/丢弹。
  3. 通知浮窗每次显示重压置顶层，压过其他置顶窗口；用户已确认通过。
- 其他工具、P0 相关代码不在本轮范围内。

# Git / Working Tree

- 工作目录：`E:\DeepSeek_Harness\Workspace\LightweightToolset`；分支 `master`；`v0.5.1` 已发布（Release 提交 `1d4e65d`，tag `v0.5.1` 已推送）。
- 既有现场（不处理、不提交）：`M src-tauri/src/window_service.rs`（纯 rustfmt 噪声）、`?? .codex/`。
- 本机 git 走代理 `http://127.0.0.1:7897`；代理未启动时推送会失败，需用户先开代理。

# Active Files

- `src/shortcutNotify.ts`：三个应用窗口共用的快捷键上报；`Ctrl+C/X` 先判断有无选中内容，空选不弹。
- `src-tauri/src/clipboard_notify.rs`：`notify_ui_shortcut()` 立即提示；`claim_report()` 负责 0.5s 同源节流 + 400ms 跨源去重；`raise_to_top()` 每次显示 `SetWindowPos(HWND_TOPMOST)`。
- `src-tauri/src/lib.rs`：命令 `clipboard_notify_shortcut`；`clipboard_copy*` → `notify_copied_now()`；`clipboard_paste` → `notify_pasted_from_command()`。
- `src/App.tsx`：`APP_VERSION` 直接来自 `../package.json`。

# Verification

- 2026-11-01 `npm run build`、`cargo check`、串行单测 11/11、`npm run tauri build`（NSIS 打包 2m04s）全部通过。
- 2026-11-01 用户真机验收通过：版号、三种软件内快捷键、松手即停、剪切即时不漏弹、空选不弹（已接受）、浮窗置顶层级。
- 安装包 SHA-256 `0D37D40604CC46D5EC09CABFB97C0E1F1A7A3F57EB2E3FA2C34B230279AEAC53`（2,662,153 字节）。

# Blockers

- 无。注意：不要对仓库 Rust 文件直接跑 `rustfmt`（会按 100 列重排整文件）。

# Next Action

- 等用户下一条需求；若要再发版，三处版本号同步 + 推送 + tag + Release 的流程不变。
- 提交需用户明确授权；提交时排除 `window_service.rs` 与 `.codex/`。
