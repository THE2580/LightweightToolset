# Goal

- LightweightToolset 持续迭代：0.6.2 修复自启与隔离、0.6.3 补静默启动通知与开发版默认值、0.6.4 统一快捷键提示图标。
- 状态：`v0.6.4` 已发布（tag → `fa580e8`，Release 已上传安装包）。

# Current Focus

- 图标：`src/shortcutTipIcon.tsx` 自定义「键盘 + 提示气泡」，`toolIcons.shortcut_tip` 与工具页头部、编辑弹窗、空状态统一。
- 自启：`autostart.rs` 用「登录触发 + HighestAvailable + InteractiveToken + PT0S」计划任务，替代 Run 键并清理旧值。
- 隔离：debug 构建 identifier 改 `...-dev`；数据、WebView2、单实例、AUMID、快捷方式、自启项全部独立。
- 静默启动：主窗口 `visible:false` + `setup` 条件 `show()`；启动时发通知（含实际快捷键，失败降级）。
- 开发版专属：默认值（静默开/置顶开/开发者模式开/工具全关/自启关）、开关锁定、存储路径禁改、标题与托盘带「（开发版）」。
- 文档：`AGENTS.md` 含「开发版与正式版」章节与发版固定动作（README/简介、清理开发版痕迹、沉淀）。

# Remaining

1. 用户安装 `轻量化工具集_0.6.4_x64-setup.exe`（`src-tauri/target/release/bundle/nsis/`，SHA-256 `34C167F700B288B40D0384718804265C2387F83981632FEB90EB0D4898FAB5CD`）复核对图标效果。
2. 可选（用户未要求，勿主动做）：Keycastr 借鉴项（按前台软件分组、外观可调、持续输出模式）。
3. 待确认：是否继续 `master` 直推，或引入功能分支/PR。

# Verification

- 发版前：`cargo check` 无告警；`cargo test --lib --test-threads=1` 18/18；`npm run build` 与 `npm run tauri build` 通过；NSIS 产物版本资源 `0.6.4`。
- 开发版真机：新图标 15px/18px 下可辨认，侧栏与页头一致（用户确认）。
- 0.6.3 遗留验证：默认值、静默启动三条唤起路径、启动通知文案均已真机确认。

# Resume Here

- `git log -1` 应为 `fa580e8`（tag `v0.6.4`）；工作树预期只剩 `M src-tauri/src/window_service.rs` 与 `?? .codex/`。
- 图标入口：`src/shortcutTipIcon.tsx`；使用点 `src/App.tsx` 的 `toolIcons`、工具页头部、`ShortcutTipEditorDialog`、空状态。
- 发版流程与开发版/正式版差异见 `AGENTS.md` 对应章节。
