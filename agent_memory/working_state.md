# Scope

- 当前范围：`v0.6.2` 已发布（tag `v0.6.2` → `9b5257e`），交付「开机自启改计划任务」「开发版完全隔离」「静默启动」三项。
- 无进行中任务；等用户安装 0.6.2 复核（0.6.1 是本地测试包，未发布）。

# Git / Working Tree

- 工作目录：`E:\DeepSeek_Harness\Workspace\LightweightToolset`；分支 `master`；代码提交 `9b5257e` 已推送，tag `v0.6.2` 已推送。
- 既有现场（不处理、不提交）：`M src-tauri/src/window_service.rs`（纯 rustfmt 噪声）、`?? .codex/`。
- 本机 git 走代理 `http://127.0.0.1:7897`；代理未启动时推送/gh 失败（本次发版曾因未开代理失败一次，用户开启后成功）。

# Active Files

- `src-tauri/src/autostart.rs`：计划任务 XML 生成、`schtasks /Create|/Query|/Delete`、UTF-16 写入、旧 Run 值清理；debug 下全部空操作。
- `src-tauri/src/lib.rs`：`AppSnapshot.dev_build`、`set_auto_start_enabled` 开发版拒绝、启动自愈调 `autostart::apply`、debug identifier 改 `...-dev`、`silent_start` 决定 `show()+set_focus()`。
- `src-tauri/src/settings.rs`：`silent_start` 字段 + 旧配置兼容测试。
- `src-tauri/src/windows_notification.rs`：AUMID/开始菜单快捷方式跟随 identifier；新增「静默启动已开启」通知。
- `src-tauri/tauri.conf.json`：主窗口 `visible: false`。
- `src/App.tsx` / `src/App.css`：`devBuild` 自启开关置灰、静默启动开关。
- `AGENTS.md`：自启计划任务、开发版隔离、静默启动三条平台约束 + 发版前清理开发版痕迹。

# Verification

- 2026-11-04 发版前：`cargo check`（无告警）、`cargo test --lib --test-threads=1` 16/16、`npm run build`、`npm run tauri build`（NSIS）全部通过。
- 计划任务：真实 schtasks 创建 → 查询确认 `HighestAvailable`/`InteractiveToken`/`PT0S`/中文路径 → 删除，无残留。
- 真机（用户确认）：0.6.1 安装后多次重启，登录自启正常、无 UAC；旧 Run 值已消失。
- 真机隔离：两版同时运行，数据目录/WebView2/快捷方式/单实例全部独立；开发版自启开关置灰。
- 静默启动（窗口枚举）：默认显示、`silentStart=true` 时主窗口 `visible=False` 且 WebView 已加载、二次启动唤起窗口。
- 本次发布资产：`轻量化工具集_0.6.2_x64-setup.exe`（2,675,727 字节，SHA-256 `F3ADC3CFFF9807A490580C51074021E3EF210E27D864062E1A184173C88AB601`）。

# Blockers

- 无。安装版（10920/13064）全程未被结束或改动。

# Next Action

- 等用户安装 0.6.2 复核；如需再发版，按「三处版本号 + 发版前清理开发版痕迹 + 提交推送 + 注释 tag + Release」流程。
- 提交时排除 `window_service.rs` 与 `.codex/`。
