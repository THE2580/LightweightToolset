# Scope

- 当前范围：`v0.6.0` 已发布并完成真机验收，「快捷键提示」工具（第 6 个工具）交付完毕。
- 已验收：预设/自定义分区、预设只读键值可改文案、排序（名称/创建时间/提示次数 + 升降序）、提示次数统计与 2s 轮询、浮窗样式与图标、内容自适应宽度、显示期间每 400ms 重新压顶、剪贴板条目点击联动、本软件快捷键进预设（回调触发）、Win 系统快捷键进预设、录制只跟录制框焦点绑定、独立采键子进程。
- 已解决的历史限制：本软件窗口为前台时，系统/其他软件注册的热键（`Win+D`/`Win+I`/`Win+Shift+S`/PixPin `Ctrl+1`）此前不提示，现由采键子进程解决，详情见 `bugs.md`。

# Git / Working Tree

- 工作目录：`E:\DeepSeek_Harness\Workspace\LightweightToolset`；分支 `master`；`v0.6.0` 已发布；当前 HEAD `3184d0b`（`docs: consolidate Windows key capture lessons and refresh checkpoints`），已推送 `origin/master`，tag `v0.6.0` 指向 `5f12c12`。
- 既有现场（不处理、不提交）：`M src-tauri/src/window_service.rs`（纯 rustfmt 噪声）、`?? .codex/`。
- 本机 git 走代理 `http://127.0.0.1:7897`；代理未启动时推送会失败。

# Active Files

- `src-tauri/src/shortcut_tip.rs`：工具核心——存储、预设（24 条常用/系统 + 3 条本软件快捷键）、钩子匹配、页面/命令上报、采键子进程桥接、浮窗 worker（含 400ms 持续压顶）、次数统计。
- `src-tauri/src/main.rs`：`--shortcut-tip-helper <port>` 子进程分支（不初始化 Tauri）。
- `src-tauri/src/lib.rs`：`shortcut_tip_*` 命令、快捷键回调用独立线程触发提示（避免死锁）、剪贴板命令改调 `notify_text`。
- `src-tauri/src/tools.rs`：注册第 6 个工具 `shortcut_tip`（默认启用）。
- `src/shortcutNotify.ts` / `src/notification.tsx` / `src/App.tsx`：页面上报、浮窗渲染、工具页与录制弹窗。

# Verification

- 2026-11-02 `npm run build`、`cargo check`、串行单测 11/11、`npm run tauri build`（release 2m34s + NSIS）全部通过。
- 用户真机验收：本软件前台时 `Win+D`/`Win+I`/`Win+Shift+S`/PixPin `Ctrl+1` 均能提示且可录制；普通快捷键无重复提示；浮窗显示期间不被其他置顶窗口盖住；2 秒正常淡出。
- 安装包 SHA-256 `93D2315A30BDEDF01C7AAF0817EA102245DAC4734456776A4CB3E64C9F447499`（2,684,815 字节）。

# Blockers

- 无。注意：不要对仓库 Rust 文件直接跑 `rustfmt`；快捷键回调不得在主线程里做加锁+窗口操作，否则会与前端的快照轮询死锁。

# Next Action

- 等用户下一条需求；如需再发版，按「三处版本号同步 + 提交推送 + 注释 tag + Release」流程执行。
- 提交需用户明确授权；提交时排除 `window_service.rs` 与 `.codex/`。
