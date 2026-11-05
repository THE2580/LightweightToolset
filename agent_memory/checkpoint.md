# Goal

- LightweightToolset：修复开机自启、开发版与安装版完全隔离、静默启动、开发版专属默认值与标识。
- 状态：`v0.6.3` 已发布（tag → `ed31f94`，Release 已上传安装包）。

# Current Focus

- 自启：`autostart.rs` 用「登录触发 + HighestAvailable + InteractiveToken + PT0S」计划任务，替代被 UAC 拦截的 Run 键，并清理旧 Run 值。
- 隔离：debug 构建 `run()` 中 identifier 改为 `com.the.lightweight-toolset.dev`；数据、WebView2、单实例、AUMID、快捷方式、自启项全部独立。
- 静默启动：主窗口 `visible:false`，`setup` 里 `silent_start` 为 false 才 `show()+set_focus()`；真正静默启动时发通知，文案含实际快捷键，注册失败时降级。
- 开发版专属：默认值（静默开/置顶开/开发者模式开/工具全关/自启关）、自启与开发者模式开关锁定、存储路径禁改、标题追加「（开发版）」、托盘与通知中文名带标识。
- 约束文档已重组：`AGENTS.md` 新增「开发版与正式版（差异与注意事项）」整节（含默认值对照表）。

# Remaining

1. 用户安装 `轻量化工具集_0.6.3_x64-setup.exe`（`src-tauri/target/release/bundle/nsis/`，SHA-256 `3A02BC5B13FECEC35D7AB68732F860AEE3B180676E24D4444AE5A7C37848C6C4`）做最终复核。
2. 可选（用户未要求，勿主动做）：Keycastr 借鉴项（按前台软件分组、外观可调、持续输出模式）。
3. 待确认：是否继续 `master` 直推，或引入功能分支/PR。

# Verification

- 发版前：`cargo check` 无告警；`cargo test --lib --test-threads=1` 18/18；`npm run build` 与 `npm run tauri build` 通过；NSIS 产物版本资源 `0.6.3`。
- 开发版默认值实测（清空数据目录后）：`silentStart/alwaysOnTop/developerMode=true`、`autoStart=false`、工具全关、`storagePath` 空；主窗口隐藏、标题「轻量化工具集（开发版）」。
- 静默启动与启动通知：用户真机确认文案与三种唤起方式；托盘悬停为中文名。
- 窗口枚举确认默认显示 / 静默隐藏 / 二次启动唤起三条路径。

# Resume Here

- `git log -1` 应为 `ed31f94`（tag `v0.6.3`）；工作树预期只剩 `M src-tauri/src/window_service.rs` 与 `?? .codex/`。
- 自启入口：`src-tauri/src/autostart.rs`；调用点 `lib.rs` 的 `set_auto_start_enabled`、`start_runtime_initialization`。
- 静默启动与默认值入口：`lib.rs` 的 `setup`（`silent_start` 分支、`apply_development_defaults`）、`tauri.conf.json` 的 `visible`。
- 发版流程与开发版/正式版差异见 `AGENTS.md` 对应章节，发版前先清理开发版痕迹。
