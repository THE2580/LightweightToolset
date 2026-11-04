# Scope

- 当前范围：修复「开机自启开关开启但登录后不启动」+ 开发版与安装版完全隔离（数据、WebView2、单实例、AUMID、开始菜单快捷方式、自启项）+ 新增「静默启动」设置。
- 版本 `0.6.1` 的 NSIS 安装包已产出，但**不含静默启动**（打包含义早于该改动）；**未提交、未推送、未打 tag**。`v0.6.0` 仍是线上最新发布。

# Git / Working Tree

- 工作目录：`E:\DeepSeek_Harness\Workspace\LightweightToolset`；分支 `master`；HEAD 以 `git log -1` 为准。
- 本次改动：`M src-tauri/Cargo.toml`、`M src-tauri/Cargo.lock`、`M src-tauri/src/lib.rs`、`M src-tauri/src/windows_notification.rs`、`M src/App.tsx`、`M src/App.css`、`?? src-tauri/src/autostart.rs`、`M AGENTS.md`、`M agent_memory/*`。
- 既有现场（不处理、不提交）：`M src-tauri/src/window_service.rs`（纯 rustfmt 噪声）、`?? .codex/`。
- 本机 git 走代理 `http://127.0.0.1:7897`；代理未启动时推送会失败。

# Active Files

- `src-tauri/src/autostart.rs`（新增）：计划任务 XML 生成、`schtasks /Create|/Query|/Delete`、UTF-16 写入、旧 Run 值清理；debug 下全部空操作。
- `src-tauri/src/lib.rs`：`AppSnapshot.dev_build`、`set_auto_start_enabled` 开发版拒绝、启动自愈改为 `autostart::apply`、debug 构建 identifier 改 `...-dev`、移除 `tauri-plugin-autostart`。
- `src-tauri/src/windows_notification.rs`：AUMID 与开始菜单快捷方式跟随 identifier，开发版独立快捷方式名。
- `src-tauri/src/settings.rs`：新增 `silent_start`（camelCase `silentStart`，默认 false），含旧配置兼容测试。
- `src-tauri/src/lib.rs`：`SettingsPatch.silent_start`、`update_app_settings` 处理并首次开启时发通知、setup 按设置决定 `show()+set_focus()`、`main_window.silent_start` 日志。
- `src-tauri/tauri.conf.json`：主窗口 `visible: false`（窗口不再默认可见）。
- `src/App.tsx`：`AppSettings.silentStart` + 「通用」页新增「静默启动」开关。
- `src/App.tsx` / `src/App.css`：`devBuild` 传入设置页，自启开关置灰不可点（`.switch.locked`）。
- `AGENTS.md`：新增「自启必须用计划任务」「开发版必须隔离」两条平台约束。

# Verification

- 2026-11-04 开发版验证静默启动（Win32 窗口枚举）：`silentStart=false` 时主窗口 `visible=True`；`silentStart=true` 重启后主窗口 `visible=False`、托盘窗口存在、`msedgewebview2.exe` 已加载；再启动第二个实例后进程仍是同一个且主窗口变为 `visible=True`（单实例回调唤起）。开发版 debug 是 console 子系统，会额外出现一个控制台窗口（release 无）。
- 2026-11-04：`cargo check`（debug/release 均无告警）、`cargo test --lib --test-threads=1` 16/16 通过（需 `TEMP/TMP` 指向 `src-tauri/target/test-tmp`）、`npm run build` 通过。
- 2026-11-04：`npm run tauri build` 通过（release 2m22s + NSIS），产物 `轻量化工具集_0.6.1_x64-setup.exe`（2,676,700 字节，SHA-256 `03B248684EBED21B43C05665E1DC7FCE7789F16C3468310D3F01F5CA21545032`），exe 版本资源为 `0.6.1`。
- 计划任务模板已用真实 schtasks 端到端验证：静默提权创建 → 查询确认 `HighestAvailable`/`InteractiveToken`/`ExecutionTimeLimit=PT0S`/中文 exe 路径 → 删除成功（临时任务名 `LightweightToolset_Verify_DSH`，已清理）。
- 2026-11-04 真机（用户确认）：安装 0.6.1 后多次重启验证，开机自启正常；`schtasks /Query /XML` 显示 `HighestAvailable`/`InteractiveToken`/`PT0S`，任务上次运行时间与进程启动时间一致；旧 Run 值已消失。
- 2026-11-04 真机（开发版隔离）：`npm run dev` + `%TEMP%\lwt-dev\lightweight-toolset.exe` 提权启动，开发版与安装版同时运行（4 个进程：两版主进程 + 各自 helper）；生成独立 `%APPDATA%`/`%LOCALAPPDATA%\com.the.lightweight-toolset.dev` 与「轻量化工具集 (开发版).lnk」，第二个开发版实例合并回开发版而不影响安装版；安装版 settings.json 未被改动，计划任务保持原样。

# Blockers

- 无阻塞。本机安装版正在运行，全程未结束其进程、未改动其 Run 项与数据。
- 注意：全局快捷键是系统级独占，两版同时运行时后启动的一方可能注册失败（既有行为，非数据串扰）。

# Next Action

- 等待用户安装 0.6.1 做真机验收（登录自启、双版本共存、开发版开关置灰）。
- 用户确认后再提交/推送/打 tag/发 Release；届时先按 `AGENTS.md`「发版前必须清理开发版痕迹」清理，不要清理正式版目录。
- 提交时排除 `window_service.rs` 与 `.codex/`。
