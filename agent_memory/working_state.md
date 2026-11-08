# Scope

- 截图工具（PixPin 复刻）：S1 抓屏/覆盖层、S2 标注/工具栏/设置页全部完成；S3 贴图本批落地。
- 已提交：`2183a2c`（保存目录 + 8 色预设）、`727dff0`（工具页标签 + 配置项 + 辅助线重构 + 选区翻转）、`cc66594`/`a12535a`（docs）、**`cd1052a`（S3 贴图，待用户验收）**。
- 本批范围：贴图窗口（置顶/拖动/拖边角缩放/滚轮缩放/锁定/鼠标穿透/关闭确认/双击动作/系统投影）、覆盖层工具栏第 14 个「贴图」按钮、去掉工具切换的字母与数字单键、贴图历史（`<存储>/screenshot/pins` 的 `.png` + `.meta`，按保留数量清理）、「贴图」标签五项设置、快捷键组（贴图 / 恢复上次关闭 / 关闭所有 / 隐藏显示 / 鼠标穿透）、软件快捷键逐条注册。
- 「截图查看」已取消，不做。
- 下一步候选：贴图历史浏览界面（目前只有「恢复上一次」+ 保留数量）、双击动作=复制与关闭确认的真机验证。

# Git / Working Tree

- 工作目录：`E:\DeepSeek_Harness\Workspace\LightweightToolset`；分支 `master`；最新提交 **`cd1052a`**（未推送）。
- 既有现场（不处理、不提交）：`M src-tauri/src/window_service.rs`（纯 rustfmt 噪声）、`M src-tauri/src/window_pinner.rs`（仅换行差异）、`?? .codex/`。
- 本机 git 走代理 `http://127.0.0.1:7897`；代理未启动时推送/gh 失败，需先让用户开启。

# Active Files

- `src-tauri/src/screenshot/pin.rs`（新，约 930 行）：常驻 `screenshot-pin` 线程 + 消息循环、`WM_APP+1` 指令通道、贴图 wndproc（绘制/拖动/缩放/滚轮/锁定/穿透/双击/关闭确认）、历史读写与 `restore_last`、6 个单测。
- `src-tauri/src/screenshot/overlay.rs`：`OverlayOutcome.pin`、`Action::Pin`、`WM_OVERLAY_PIN`（`WM_APP+2`）与 `request_pin()`；删掉 `R/O/C/L/A/B/P/H/M/F/T/N` 与 `1–4`、`[`/`]` 单键。
- `src-tauri/src/screenshot/toolbar.rs`：`Action::Pin`、主栏 14 个按钮、图钉图标、提示文案带贴图快捷键。
- `src-tauri/src/screenshot/mod.rs`：`capture_config` 返回 `PinOptions` + 历史数量、`pin_from_hotkey/restore_pin/close_all_pins/toggle_pins_visible/toggle_pins_passthrough`、`pin_hotkey_text()`、`stop()` 关闭全部贴图。
- `src-tauri/src/settings.rs` / `lib.rs`：`screenshotPinLock/DoubleClick/ConfirmClose/Shadow/History`（含 serde 默认值与 patch 校验）、`get_screenshot_defaults` 五项。
- `src-tauri/src/tools.rs`：`APP_HOTKEYS` 由 1 条扩到 6 条（`screenshot_pin`/`pin_restore`/`pin_close_all`/`pin_toggle_visible`/`pin_passthrough`），`register_app_hotkeys` 改为逐条注册并汇总失败。
- `src/App.tsx`：`PIN_HOTKEY_IDS`、`renderHotkeyRow` 通用快捷键行、「贴图」标签五项设置、全局快捷键页过滤贴图组。

# Verification

- 2026-11-08 S3 自检（`cd1052a`）：
  - 构建：`npm run build` 通过；`cargo check` 通过（仅既有 2 条 `unused_mut` + 1 条 annotation 死代码告警）；`cargo test --lib screenshot::` **23/23**；全量串行 38 passed / 3 failed（`window_pinner` 三条均为沙箱写 `%TEMP%` 失败，与本次无关）。
  - 端到端（提权 PowerShell 注入）：`Ctrl+Alt+Shift+A` 覆盖层 → 拖选 → `Ctrl+Alt+P` 贴图：贴图窗口矩形与选区完全一致（400×320），**没有抢走前台焦点**；拖动 +120/+90 精确跟随；滚轮 400×300 → 440×330（110%）以中心缩放；`Ctrl+Alt+X` 关闭全部生效。
  - `pin_restore` 与原尺寸恢复（500,350,900,670 一致）；`pin_toggle_visible` 隐藏 `IsWindowVisible=False`、再按恢复；`pin_passthrough` 扩展样式 `0x800A8`（LAYERED|TRANSPARENT|TOOLWINDOW|TOPMOST）。
  - 锁定类型=全部锁定时拖动位移为 0；双击动作=关闭时双击后窗口数 1→0；保留数量=2 时历史 `.png`/`.meta` 都只剩 2 份。
  - 工具栏截图确认：14 个按钮、图钉图标在最右、栏宽与图标间距正常。
- **未测**：双击动作=复制、关闭前确认的弹窗、阴影关闭（`LightweightToolsetPinFlat` 类）、无覆盖层时按贴图钉「最近一次截图」、历史浏览。
- 历史基线与既有验证（S1/S2）见 `progress.md` 与 `archive/`。
