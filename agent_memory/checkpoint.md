# Goal

- LightweightToolset 持续迭代：`v0.6.0` 已交付「快捷键提示」新工具；等待用户下一项需求。

# Current Focus

- 状态：`v0.6.0` 已发布（tag `v0.6.0` → `5f12c12`；Release 资产 `LightweightToolset_0.6.0_x64-setup.exe`，SHA-256 `93D2315A30BDEDF01C7AAF0817EA102245DAC4734456776A4CB3E64C9F447499`）。
- 新工具 `shortcut_tip`（第 6 个工具，默认启用）：预设 24 条（常用编辑 12 + Windows 系统 12）+ 本软件快捷键 3 条（跟随设置、回调触发）+ 自定义录制（录制框焦点绑定）。
- 采键三条通道：页面上报（Chromium 前台普通键）、进程内钩子（焦点在别处）、独立子进程（`--shortcut-tip-helper <port>` + 回环 TCP，覆盖系统/他软件热键）。三者节流与去重统一在 `claim_report`。
- 剪贴板：`clipboard_notify.rs` 已删除，条目点击提示改调 `shortcut_tip::notify_text`；模拟粘贴后短暂抑制快捷键匹配。

# Remaining

- 无进行中的任务；等用户下一条需求。
- 可选优化（用户未要求，勿主动做）：按前台软件的预设分组与自动切换、外观可调（颜色/透明度/字号/拖拽定位）、持续输出模式——来自 Keycastr 的借鉴清单。
- 已知限制：`Ctrl+Alt+Del`、`Win+L` 这类内核/安全层处理的组合无法捕获或拦截。

# Verification

- 2026-11-02：`npm run build`、`cargo check`、串行单测 11/11、`npm run tauri build`（release 2m34s + NSIS）全部通过。
- 真机验收：本软件前台时 `Win+D`/`Win+I`/`Win+Shift+S`/PixPin `Ctrl+1` 均能提示且可录制；普通快捷键无重复提示；浮窗 2 秒后正常淡出且显示期间不被其他置顶窗口盖住。
- 发版前必须重跑：`npm run build`、`cargo check`、`cargo test --lib -- --test-threads=1`、`npm run tauri build`。

# Resume Here

- 先 `git status --short` 确认现场（预期只剩 `M src-tauri/src/window_service.rs` 的 rustfmt 噪声与 `?? .codex/`）。
- 快捷键提示相关代码入口：`src-tauri/src/shortcut_tip.rs`（存储/匹配/浮窗/子进程桥接）、`src-tauri/src/main.rs`（`--shortcut-tip-helper` 分支）、`src/App.tsx` 的 `ShortcutTipToolPage` / `ShortcutTipEditorDialog`、`src/notification.tsx`。
- 动手前务必先读 `AGENTS.md` 的「平台约束与踩坑经验」，其中锁顺序、浮窗尺寸、录制焦点绑定等几条都是踩过坑写下的。
