# Scope

- 截图工具（PixPin 复刻）：S1 抓屏/覆盖层、S2 标注/工具栏/设置页全部完成；S3 贴图两轮落地并由用户验收通过。
- 已提交：`2183a2c`（保存目录 + 8 色预设）、`727dff0`（工具页标签 + 配置项 + 辅助线重构 + 选区翻转）、`cc66594`/`a12535a`/`08160d1`（docs）、`cd1052a`（贴图首轮）、**`9c72761`（贴图修偏 + 三态阴影 + 右键菜单，最新）**。
- 本批新增：贴图上下颠倒修复、滚轮缩放改左上角固定、压贴图出现延迟（线程预热 + 历史后台落盘）、PixPin 同款三态阴影（出现淡蓝泛光/焦点淡蓝/失焦灰，独立分层窗口）、右键菜单（图像处理五项、复制图像、图像另存为、窗口阴影、锁定、关闭；标注与文本选择置灰）。
- 「截图查看」已取消，不做。
- 进行中：贴图标注。`toolbar.rs` 已支持 `Mode::Pin` + `Action::Done`（主栏固定窗口顶部、二级栏与下拉向下生长）；pin 端改为**贴图窗口外的独立伴生小窗**（`LightweightToolsetPinToolbar`）承载工具栏，主栏贴贴图下沿、放不下翻上方；右键菜单「标注/隐藏标注」已解禁。**用户已验收，已提交 `d32b36f`**；贴图文字/序号/马赛克、文本选择、历史浏览未做。
- 已提交：工具栏 `d32b36f`、批 A `27c669d`、批 B `4785eea`、文字与序号 `450c122`。

# Git / Working Tree

- 工作目录：`E:\DeepSeek_Harness\Workspace\LightweightToolset`；分支 `master`；最新提交 **`450c122`**（`feat: support text and serial tools in pin annotation`，未推送），工作树只剩既有三项。
- 既有现场（不处理、不提交）：`M src-tauri/src/window_service.rs`（纯 rustfmt 噪声）、`M src-tauri/src/window_pinner.rs`（仅换行差异）、`?? .codex/`。
- 本机 git 走代理 `http://127.0.0.1:7897`；代理未启动时推送/gh 失败，需先让用户开启。

# Active Files

- `src-tauri/src/screenshot/pin.rs`（约 3150 行）：常驻 `screenshot-pin` 线程 + `WM_APP+1` 指令通道；贴图 wndproc（绘制/拖动/缩放/滚轮/锁定/穿透/双击/关闭确认）；`PinShadow` 分层阴影三态；`processed()` 旋转翻转与 `install_image`；右键菜单（`WM_COMMAND` 派发）与 `save_as`（`GetSaveFileNameW`）；历史读写与 `restore_last`；**标注模式**（`annotating/doc/draft/picks/style_open/line_menu_open/arrow_menu_open/delete_mode/composite/annot_dirty/toolbar`）与**工具栏伴生小窗**（`LightweightToolsetPinToolbar` + `toolbar_proc` + `toolbar_refresh`）；6 个单测。
- `src-tauri/src/screenshot/toolbar.rs`：`Mode{Screenshot,Pin}`——截图模式工具栏贴选区下方（放不下翻上），贴图模式主栏固定在工具栏窗口顶部、二级栏与下拉菜单向下生长；`Action::Done` 对勾。
- `src-tauri/src/screenshot/overlay.rs`：`OverlayOutcome.pin`、`Action::Pin`、`WM_OVERLAY_PIN`（`WM_APP+2`）与 `request_pin()`；删掉 `R/O/C/L/A/B/P/H/M/F/T/N` 与 `1–4`、`[`/`]` 单键。
- `src-tauri/src/screenshot/toolbar.rs`：`Action::Pin`、主栏 14 个按钮、图钉图标、提示文案带贴图快捷键。
- `src-tauri/src/screenshot/mod.rs`：`capture_config` 返回 `PinOptions` + 历史数量、`pin::warm_up()`、`pin_from_hotkey/restore_pin/close_all_pins/toggle_pins_visible/toggle_pins_passthrough`、`pin_hotkey_text()`、`stop()` 关闭全部贴图。
- `src-tauri/src/settings.rs` / `lib.rs`：`screenshotPinLock/DoubleClick/ConfirmClose/Shadow/History`（含 serde 默认值与 patch 校验）、`get_screenshot_defaults` 五项。
- `src-tauri/src/tools.rs`：`APP_HOTKEYS` 由 1 条扩到 6 条（`screenshot_pin`/`pin_restore`/`pin_close_all`/`pin_toggle_visible`/`pin_passthrough`），`register_app_hotkeys` 改为逐条注册并汇总失败。
- `src/App.tsx`：`PIN_HOTKEY_IDS`、`renderHotkeyRow` 通用快捷键行、「贴图」标签五项设置、全局快捷键页过滤贴图组。

# Verification

- 2026-11-09 贴图标注工具栏（已提交 `d32b36f`，用户验收通过；开发已暂停）：
  - 提交前检查：`npm run build` 通过；`cargo check` 通过（4 条既有告警）；全量串行 `cargo test --lib` **38 passed / 3 failed**——3 条均为 `window_pinner` 沙箱写 `%TEMP%` 失败，与本次无关；`cargo test --lib screenshot::` 23/23。
  - 注入实测（热键临时改 `CTRL+ALT+SHIFT+A/P` 避开安装版）：900×600 贴图进标注 → 点形状组 → 小窗 1017×112（二级栏 8 色全见）→ 拖红框（`items=1`）→ 对勾退出、工具栏销毁、红框留存；320×240 贴图工具栏完整不被裁切；贴屏幕底部（700,1380–1000,1540）的贴图工具栏翻到上方；右键菜单「标注/隐藏标注」解禁后可点。
  - 截图：`%TEMP%\lwt-pin-tb8-c.png`、`lwt-pin-tb6-c.png`、`lwt-pin-tb10-a.png`、`lwt-pin-menu.png`。
  - 未验证：文字/序号/马赛克绘制、线型/箭头下拉、滚轮调数值、删除模式、拖动时小窗跟随。
  - 收工状态：开发版进程与 Vite `:1420` 已关闭（只剩安装版 `D:\轻量化工具集` 两个进程）；开发版热键已恢复 `CTRL+ALT+A/P/X/H/R`。

- 2026-11-08 S3 第二轮自检（`9c72761`）：
  - `npm run build`、`cargo check`、`cargo test --lib screenshot::` 23/23 通过。
  - 注入实测：贴图不再上下颠倒；按下贴图到窗口可见 80–125 ms；滚轮 `600,400` 起点不动（400×296 → 440×326）；阴影三态截图确认（淡蓝泛光 / 失焦灰 / 焦点淡蓝）；`WM_COMMAND` 11/12/14/40/20/50 分别验证右旋 400×296→296×400 且左上不动、重置、阴影 1→0→1、复制进剪贴板 400×296、菜单关闭销毁窗口；关闭全部与恢复上一次关闭正常（恢复用 `CTRL+ALT+J` 验证，因本机 R 被其它软件占用）。
  - 排查记录：关闭一度全部失效，实为开发版「关闭前确认」开着、pin 线程堆了 4 个 `MessageBoxW` 模态框；把该设置关掉后恢复正常，不是代码缺陷。
- 2026-11-08 S3 首轮自检（`cd1052a`）：
  - 构建：`npm run build` 通过；`cargo check` 通过（仅既有 2 条 `unused_mut` + 1 条 annotation 死代码告警）；`cargo test --lib screenshot::` **23/23**；全量串行 38 passed / 3 failed（`window_pinner` 三条均为沙箱写 `%TEMP%` 失败，与本次无关）。
  - 端到端（提权 PowerShell 注入）：`Ctrl+Alt+Shift+A` 覆盖层 → 拖选 → `Ctrl+Alt+P` 贴图：贴图窗口矩形与选区完全一致（400×320），**没有抢走前台焦点**；拖动 +120/+90 精确跟随；滚轮 400×300 → 440×330（110%）以中心缩放；`Ctrl+Alt+X` 关闭全部生效。
  - `pin_restore` 与原尺寸恢复（500,350,900,670 一致）；`pin_toggle_visible` 隐藏 `IsWindowVisible=False`、再按恢复；`pin_passthrough` 扩展样式 `0x800A8`（LAYERED|TRANSPARENT|TOOLWINDOW|TOPMOST）。
  - 锁定类型=全部锁定时拖动位移为 0；双击动作=关闭时双击后窗口数 1→0；保留数量=2 时历史 `.png`/`.meta` 都只剩 2 份。
  - 工具栏截图确认：14 个按钮、图钉图标在最右、栏宽与图标间距正常。
- **未测**：双击动作=复制、关闭前确认的弹窗、阴影关闭（`LightweightToolsetPinFlat` 类）、无覆盖层时按贴图钉「最近一次截图」、历史浏览。
- 历史基线与既有验证（S1/S2）见 `progress.md` 与 `archive/`。

# 现场交接（2026-11-10 19:27，换新会话前记录）

- HEAD `eacede0`；工作树只剩既有三项（`window_pinner.rs`、`window_service.rs`、`.codex/`），无未提交功能代码。
- 开发版进程：`%TEMP%\lwt-dev\lightweight-toolset.exe`，pid **9620**（19:11 启动）；安装版 `11732`、`17968` 同时在跑（`D:\轻量化工具集`）。
- Vite：`bash-038bb9734b8e43ae` 后台任务跑着 `npm run dev`（:1420）。**新会话里这个后台任务可能已随旧会话结束**，先 `curl http://localhost:1420` 确认，不在就重启。
- 开发版热键已恢复：`CTRL+ALT+A/P/X/H/R`（`pin_restore` 仍被其它软件占用，测试用 `CTRL+ALT+J`）。
- 下一件事（用户已定：**先讨论、不要直接动工**）：贴图文本选择（右键「选择所有文本」「文本可选择」仍置灰），开工前必须先和用户确认 OCR 方案（系统 Windows OCR vs 本地 OCR 模型）。
- 用户已明确「贴图历史浏览界面」暂缓不做。

# Blockers

- 无阻塞。文字与序号已验收提交；开发版与 Vite 在跑。下一步贴图文本选择与历史浏览。

# Next Action

- 恢复开发时：`npm run dev` → `cargo build` → 复制 debug exe 到 `%TEMP%\lwt-dev` → 提权启动 → 核对进程路径与页面加载。
- 下一批开发内容：贴图标注的文字 / 序号 / 马赛克（工具栏已能选工具，缺贴图内实现）；之后是贴图文本选择（含 OCR）与贴图历史浏览。
