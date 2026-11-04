# Goal

- LightweightToolset 修复开机自启不生效、让开发版与安装版完全隔离、新增静默启动。
- 状态：`v0.6.2` 已发布（tag → `9b5257e`，Release 已上传安装包）；0.6.1 仅为本地测试包，未发布。

# Current Focus

- 自启：`autostart.rs` 用「登录触发 + HighestAvailable + InteractiveToken + PT0S」计划任务，替代被 UAC 拦截的 Run 键，并清理旧 Run 值。
- 隔离：debug 构建 `run()` 中 identifier 改为 `com.the.lightweight-toolset.dev`；数据、WebView2、单实例、AUMID、快捷方式、自启项全部独立；开发版自启开关置灰且不写设置。
- 静默启动：主窗口 `visible:false`，`setup` 里 `silent_start` 为 false 才 `show()+set_focus()`；首次开启发通知；唤起入口统一走 `show_main_window`。

# Remaining

1. 用户安装 `轻量化工具集_0.6.2_x64-setup.exe`（`src-tauri/target/release/bundle/nsis/`，SHA-256 `F3ADC3CFFF9807A490580C51074021E3EF210E27D864062E1A184173C88AB601`）做最终复核。
2. 可选（用户未要求，勿主动做）：Keycastr 借鉴项（按前台软件分组、外观可调、持续输出模式）。
3. 待确认：是否继续 `master` 直推，或引入功能分支/PR。

# Verification

- 发版前：`cargo check` 无告警；`cargo test --lib --test-threads=1` 16/16；`npm run build` 与 `npm run tauri build` 通过；NSIS 产物版本资源 `0.6.2`。
- schtasks 模板真实注册/查询/删除通过；真机确认登录自启正常、无 UAC。
- 窗口枚举确认静默启动三条路径；两版并存时数据与单实例互不干扰。

# Resume Here

- `git log -1` 应为 `9b5257e`（tag `v0.6.2`）；工作树预期只剩 `M src-tauri/src/window_service.rs` 与 `?? .codex/`。
- 自启入口：`src-tauri/src/autostart.rs`；调用点 `lib.rs` 的 `set_auto_start_enabled`、`start_runtime_initialization`。
- 静默启动入口：`lib.rs` 的 `setup`（`silent_start` 分支）、`tauri.conf.json` 的 `visible`。
- 发版流程见 `AGENTS.md`「分支、提交与发布」，发版前先清理开发版痕迹。
