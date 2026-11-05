# Scope

- 当前范围：`v0.6.3` 已发布（tag `v0.6.3` → `ed31f94`），交付「静默启动通知」「开发版专属默认值与锁定项」「窗口标题（开发版）标识」「托盘/通知中文应用名」。
- 无进行中任务；约束文档已整理为 `AGENTS.md` 的「开发版与正式版（差异与注意事项）」。

# Git / Working Tree

- 工作目录：`E:\DeepSeek_Harness\Workspace\LightweightToolset`；分支 `master`；代码提交 `ed31f94` 已推送，tag `v0.6.3` 已推送。
- 既有现场（不处理、不提交）：`M src-tauri/src/window_service.rs`（纯 rustfmt 噪声）、`?? .codex/`。
- 本机 git 走代理 `http://127.0.0.1:7897`；代理未启动时推送/gh 失败，需先让用户开启。

# Active Files

- `src-tauri/src/autostart.rs`：计划任务 XML 生成、`schtasks` 注册/查询/删除、旧 Run 值清理；debug 下空操作。
- `src-tauri/src/lib.rs`：`dev_build` 标记、开发版默认值与锁定（自启/开发者模式/存储路径）、`display_window_title`、`display_app_name`、静默启动通知发送。
- `src-tauri/src/tools.rs`：`apply_development_defaults`、`app_hotkey_text`。
- `src-tauri/src/windows_notification.rs`：AUMID/快捷方式跟随 identifier、静默启动开启提示与启动通知。
- `src/App.tsx`：`devBuild` 驱动的开关锁定、`displayedWindowTitle`、静默启动开关。
- `AGENTS.md`：新增「开发版与正式版（差异与注意事项）」整节（标识、默认值对照表、常驻行为、并存限制、发版前清理）。

# Verification

- 2026-11-05 发版前：`cargo check`（无告警）、`cargo test --lib --test-threads=1` 18/18、`npm run build`、`npm run tauri build`（NSIS）全部通过；exe 版本资源 `0.6.3`。
- 开发版默认值实测：清空数据目录后启动，`silentStart/alwaysOnTop/developerMode=true`、`autoStart=false`、工具全关、`storagePath` 空；主窗口 `visible=False`、标题「轻量化工具集（开发版）」。
- 静默启动与启动通知：用户真机确认通知文案正确、三种唤起方式可用。
- 本次发布资产：`轻量化工具集_0.6.3_x64-setup.exe`（2,677,488 字节，SHA-256 `3A02BC5B13FECEC35D7AB68732F860AEE3B180676E24D4444AE5A7C37848C6C4`）。

# Blockers

- 无。安装版进程全程未被结束或改动。

# Next Action

- 等用户安装 0.6.3 复核；如需再发版，按「三处版本号 + 发版前清理开发版痕迹 + 提交推送 + 注释 tag + Release」流程。
- 提交时排除 `window_service.rs` 与 `.codex/`。
