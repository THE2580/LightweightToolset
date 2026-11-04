# Goal

- LightweightToolset 修复开机自启不生效，并让开发版与正式安装版完全隔离、不再抢单实例与数据。
- 基线：`v0.6.0` 已发布（tag → `5f12c12`）；本次改动已升到 `0.6.1` 并打包，**未提交、未推送**。

# Current Focus

- 自启由 Run 键改为计划任务：`autostart.rs` 生成 UTF-16 任务 XML，`schtasks /Create /XML` 注册「登录触发 + `HighestAvailable` + `InteractiveToken`」，显式 `ExecutionTimeLimit=PT0S`，并清理旧 Run 值。
- 开发版隔离：debug 构建 `run()` 中把 identifier 改为 `com.the.lightweight-toolset.dev`；数据目录、WebView2 数据、单实例互斥量/窗口类、通知 AUMID、开始菜单快捷方式（「轻量化工具集 (开发版).lnk」）全部随 identifier 隔离。
- 开发版自启：`set_auto_start_enabled` 直接拒绝且不写设置，`autostart::apply` 空操作，UI 开关固定关闭并置灰（`.switch.locked`）。
- 已移除 `tauri-plugin-autostart` 依赖；`AppSnapshot` 新增 `devBuild`。

# Remaining

1. 用户安装 `轻量化工具集_0.6.1_x64-setup.exe`（`src-tauri/target/release/bundle/nsis/`，SHA-256 `03B248684EBED21B43C05665E1DC7FCE7789F16C3468310D3F01F5CA21545032`）做真机验收：注销/重启 → 登录后自动启动且不弹 UAC；`schtasks /Query /TN LightweightToolset /XML` 应显示 `HighestAvailable` + `PT0S`。
2. 真机验收：安装版运行中同时启动开发版，两者可共存、数据目录互不写入；开发版自启开关显示关闭且点击无效。
3. 用户确认后提交（排除 `window_service.rs`、`.codex/`）、推送、打 tag、发 Release；推送前先按 `AGENTS.md`「发版前必须清理开发版痕迹」清理。
4. 发版后按固定动作沉淀 AGENTS.md 与 `agent_memory`。

# Verification

- 2026-11-04：`cargo check`（debug、release）无告警；`cargo build --release` 通过；`cargo test --lib --test-threads=1` 14/14（`TEMP/TMP` 需指向 `src-tauri/target/test-tmp`）；`npm run build` 通过。
- schtasks 模板已用真实注册验证：创建成功 → 查询确认最高权限/交互式令牌/无执行时限/中文路径 → 删除成功，无残留任务。
- 未做：安装新版后的真实登录自启验证（需用户安装）；UI 置灰效果未在真机截图确认。

# Resume Here

- 先 `git status --short` 核对现场：预期为本次 8 个文件改动 + `autostart.rs` 新增，外加既有 `M src-tauri/src/window_service.rs`、`?? .codex/`。
- 自启入口：`src-tauri/src/autostart.rs`（XML/schtasks/清理 Run 值）；调用点：`lib.rs` 的 `set_auto_start_enabled` 与 `start_runtime_initialization`。
- 隔离入口：`lib.rs` 的 `run()` 顶部 identifier 改写；`windows_notification::prepare_app_identity(&identifier)`。
- 下一步从「Remaining 1」开始，需用户明确授权才升版/提交/打包。
