# Scope

- 当前范围：`v0.6.4` 已发布（tag `v0.6.4` → `fa580e8`），交付「快捷键提示」专用图标并统一四处用法。
- 无进行中任务。

# Git / Working Tree

- 工作目录：`E:\DeepSeek_Harness\Workspace\LightweightToolset`；分支 `master`；代码提交 `fa580e8` 已推送，tag `v0.6.4` 已推送。
- 既有现场（不处理、不提交）：`M src-tauri/src/window_service.rs`（纯 rustfmt 噪声）、`?? .codex/`。
- 本机 git 走代理 `http://127.0.0.1:7897`；代理未启动时推送/gh 失败，需先让用户开启。

# Active Files

- `src/shortcutTipIcon.tsx`：自定义「键盘 + 提示气泡」图标组件（24 网格、2px 描边、`currentColor`）。
- `src/App.tsx`：`toolIcons` 增加 `shortcut_tip` 映射；工具页头部、编辑弹窗、空状态统一使用该图标。
- `src-tauri/src/lib.rs`、`autostart.rs`、`tools.rs`、`windows_notification.rs`、`settings.rs`、`tauri.conf.json`：自启计划任务、开发版隔离/默认值/锁定、静默启动与通知（0.6.2–0.6.3 已交付）。
- `AGENTS.md`：含「开发版与正式版（差异与注意事项）」与发版固定动作（README/简介、清理开发版痕迹、事后沉淀）。
- `README.md`：已更新到 0.6.4。

# Verification

- 2026-11-05 发版前：`cargo check` 无告警、`cargo test --lib --test-threads=1` 18/18、`npm run build`、`npm run tauri build`（NSIS）通过；exe 版本资源 `0.6.4`。
- 开发版真机确认：新图标在侧栏 15px、页头 18px 下可辨认，与页头/弹窗/空状态一致（用户已确认「通过」）。
- 本次发布资产：`轻量化工具集_0.6.4_x64-setup.exe`（2,677,923 字节，SHA-256 `34C167F700B288B40D0384718804265C2387F83981632FEB90EB0D4898FAB5CD`）。
- README 已更新到 0.6.4；仓库简介复核后仍准确（未改工具列表），未改动。

# Blockers

- 无。安装版进程全程未被结束或改动。

# Next Action

- 等用户安装 0.6.4 复核；如需再发版，按「三处版本号 + 更新 README/简介 + 清理开发版痕迹 + 提交推送 + 注释 tag + Release + 事后沉淀」流程。
- 提交时排除 `window_service.rs` 与 `.codex/`。
