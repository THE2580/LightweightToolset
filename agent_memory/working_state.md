# Scope

- 当前范围：新工具「快捷键提示」（`shortcut_tip`）开发，功能与界面已基本验收，**尚未发版**（当前版本号仍是 `0.5.1`，发版需提到 `0.6.0`）。
- 已完成并通过用户验收：预设/自定义分区、预设只读键值可改文案、排序（名称/创建时间/提示次数 + 升降序）、提示次数统计与 2s 轮询、浮窗样式与图标、固定宽窗口 + `max-content` 居中、剪贴板条目点击联动、本软件快捷键进预设（回调触发）、Win 系统快捷键进预设、录制只跟录制框焦点绑定。
- 已定论的限制：本软件窗口为前台时，被系统或其他软件注册走的快捷键（`Win+D`/`Win+I`/`Win+Shift+S`/PixPin `Ctrl+1`）不提示；详见 `bugs.md`。用户已接受，暂不做 helper。

# Git / Working Tree

- 工作目录：`E:\DeepSeek_Harness\Workspace\LightweightToolset`；分支 `master`；`v0.5.1` 已发布（tag 指向 `1d4e65d`）。
- 本轮改动**全部未提交**：新增 `src-tauri/src/shortcut_tip.rs`、`src/shortcutNotify.ts`，删除 `src-tauri/src/clipboard_notify.rs`，前端 `App.tsx` / `popup.tsx` / `free.tsx` / `notification.tsx` / `notification.css` / `App.css`，后端 `lib.rs` / `tools.rs` / `clipboard.rs`，以及 `agent_memory/*`。
- 既有现场（不处理、不提交）：`M src-tauri/src/window_service.rs`（纯 rustfmt 噪声）、`?? .codex/`。
- 本机 git 走代理 `http://127.0.0.1:7897`；代理未启动时推送会失败。

# Active Files

- `src-tauri/src/shortcut_tip.rs`：工具实现核心——存储（`shortcut_tip.json`）、预设（24 条常用/系统 + 3 条本软件快捷键）、钩子匹配、页面/命令上报、浮窗 worker、置顶重压、次数统计（5s 节流落盘）。
- `src-tauri/src/lib.rs`：`shortcut_tip_*` 命令、快捷键回调里用独立线程触发 `notify_app_hotkey`（主线程不碰锁，避免死锁）、剪贴板命令改调 `notify_text`。
- `src-tauri/src/tools.rs`：注册第 6 个工具 `shortcut_tip`（默认启用），启停走 `shortcut_tip::start/stop`。
- `src/shortcutNotify.ts`：页面 `keydown` 上报 + `chordFromEvent`。
- `src/notification.tsx`：浮窗渲染（标题 + 快捷键胶囊 + ×N，预设图标映射）。
- `src/App.tsx`：`ShortcutTipToolPage` / `ShortcutTipEditorDialog`（录制与文案、焦点绑定录制）。

# Verification

- 2026-11-02 `npm run build`、`cargo check`、debug 构建均通过；清理 Raw 兜底后重新验证编译干净。
- 用户真机验收：系统预设显示与触发（焦点在外部程序）、录制 `Win+I`、界面各轮改动、本软件快捷键回调触发（修复死锁后）。
- 未复测：`npm run tauri build` 打包与串行单测（发版前需补跑）。

# Blockers

- 无。注意：不要对仓库 Rust 文件直接跑 `rustfmt`；快捷键回调不得在主线程里做加锁+窗口操作，否则会与前端的快照轮询死锁。

# Next Action

- 等用户确认是否发 `0.6.0`：补跑 `npm run tauri build` + 串行单测，三处版本号同步，提交推送 + tag + Release。
- 提交需用户明确授权；提交时排除 `window_service.rs` 与 `.codex/`。
