# Goal
复刻 PixPin 截图工具（LightweightToolset 新工具）。S3 贴图已实现并提交 `cd1052a`，等用户验收。

# Current Focus
本批交付贴图闭环：贴图窗口（置顶/拖动/缩放/滚轮/锁定/穿透/关闭确认/双击/投影）、覆盖层「贴图」按钮（去掉字母与数字单键）、贴图历史与保留数量、「贴图」标签五项设置、贴图快捷键组（贴图/恢复上次关闭/关闭所有/隐藏显示/鼠标穿透）。

开发版已按固定流程重启并从 `%TEMP%\lwt-dev` 运行；Vite `:1420` 在跑；开发版设置已恢复测试前的值（`screenshotPinLock=none`、`DoubleClick=none`、`History=20`、`pin_restore=CTRL+ALT+R`、`pin_passthrough` 空）。

# Remaining
1. 用户验收本批；重点手测：双击动作=复制、关闭前确认弹窗、阴影关闭、无覆盖层时按贴图钉「最近一次截图」。
2. 贴图历史浏览界面（当前只有「恢复上一次」+ 保留数量，没有列表）。
3. **待办（本工具完成后）**：其他有快捷键的工具逐个迁到各自工具页的「快捷键」标签。
4. 不做：截图查看、GIF、UI 检测、多语言、主题/托盘图标自定义等（见 `progress.md`）。

# Verification
- `npm run build`、`cargo check`、`cargo test --lib screenshot::` 23/23 通过；全量串行 38 passed / 3 failed（`window_pinner` 沙箱写 `%TEMP%` 失败，既有问题）。
- 端到端注入验证通过：贴图位置尺寸与选区一致、不抢焦点、拖动/滚轮/锁定/双击关闭/隐藏显示/穿透/关闭全部/恢复上一次/历史清理。
- 本机 `CTRL+ALT+R` 被其他软件占用（注册失败），验证时曾临时换成 `CTRL+ALT+J`，现已改回。

# Resume Here
1. 环境：Vite `:1420` 与开发版 `%TEMP%\lwt-dev` 应在运行；重启流程：`npm run dev` → `cargo build` → 复制 `src-tauri/target/debug/lightweight-toolset.exe` 到 `%TEMP%\lwt-dev` → 提权 `Start-Process powershell.exe -ArgumentList '-File',<脚本> -Verb RunAs -WindowStyle Hidden` → 核对进程路径与页面加载。
2. 向用户汇报验收要点，等反馈；下一块是贴图历史浏览界面，开工前照例先复述等放行。
3. 提交时排除 `window_service.rs`、`window_pinner.rs`、`.codex/`。
4. 注意：开发版 `main_window` 快捷键已被改回默认 `CTRL+ALT+M`（原值 `CTRL+ALT+P` 与贴图默认键冲突）；`screenshot` 保持 `CTRL+ALT+A`，该键在本机常被安装版占用，测试时若无效先查注册状态。
