# Scope

- 工具集已含七个工具：**截图与贴图**（第一版，`v0.7.0` 发布）、剪贴板、软件使用统计、按键使用统计、计时器、窗口置顶、快捷键提示。
- **快捷键入口迁移与录制 UI 改版已完成（2026-11-19，用户已验收）**：剪贴板、窗口置顶的快捷键都迁到各自设置页/工具页，全局设置只剩「呼出主窗口」；录制逻辑集中到 `HotkeyRowEditor`（四处在用，`hideTitle` 供工具栏）；录制框单框单按钮、失焦自动保存、外观按 2px 边框 / 宽度随内容 / 空态虚线 / 录制态主色 + 光晕定稿。
- 开发版默认值：静默启动默认关闭（代码默认值 + 本机 `settings.json` 均为 false）。
- 截图与贴图范围：区域截图（原生覆盖层/放大镜取色/辅助线/压暗）、12 种标注与二级栏、贴图窗口（拖动/缩放/锁定/穿透/阴影/右键图像处理）、贴图标注（窗口外伴生工具栏、文字与序号）、贴图文本选择（本地 OCR）、超级拖放与跟踪缩略图。
- 明确不在本版：标注拖动/文本再编辑、贴图历史浏览界面（暂缓）、系统级 DragImage 缩略图。

# Git / Working Tree

- 工作目录：`E:\DeepSeek_Harness\Workspace\LightweightToolset`；分支 `master`；HEAD **`5397dc3`**（`docs: tidy project constraints and memory for v0.7.0`），发布提交 `b5775dc`，tag `v0.7.0` 已推送，远程与本地一致。
- 本次改动已提交（HEAD，**未推送**）：`feat: move tool hotkeys into their own settings and rework recorder`。工作树只剩既有三项。
- 既有现场（不处理、不提交）：`M src-tauri/src/window_service.rs`（rustfmt 重排，非逻辑改动）、`M src-tauri/src/window_pinner.rs`（CRLF 换行差异）、`?? .codex/`。
- 本机 git 走代理 `http://127.0.0.1:7897`；代理未启动时推送/gh 失败，需先让用户开启。

# Active Files

- `src/App.tsx`：新增 `HotkeyRowTarget` / `HotkeyRowEditor`（录制、格式校验、冲突检查、`suspend/resume_tool_hotkeys`、保存与停用），`toHotkeyRowTarget` 供全局页转换条目；`HotkeySettings` 去掉剪贴板分组但保留它参与冲突检查；`ScreenshotToolPage` 与 `ClipboardToolPage` 均改用该组件。
- `src/App.tsx` 剪贴板侧：`ClipboardToolPage` 新增 `appHotkeys`/`tools`/`setAppSnapshot` 入参、`hotkeyNotice` 提示与 `hotkeyTargets` 冲突列表、`currentHotkey` 从实时 `tools` 取值；`ClipboardSettingsDialog` 改为接收 `tool`/`hotkeyTargets`/`setSnapshot`/`onNotice`，内嵌快捷键行。
- `src/App.tsx` 窗口置顶侧：`WindowPinnerToolPage` 改为接收 `appHotkeys`/`tools`/`setAppSnapshot`，工具栏「快捷键」后直接内嵌 `HotkeyRowEditor`（`hideTitle` 紧凑模式），删除跳转全局快捷键页的齿轮按钮；空状态文案改用实时 `currentHotkey`。`ToolPage` 不再需要 `onOpenSettingsTab`，已从签名与调用点移除。
- 2026-11-19 截图 UI 检测（第一阶段，未提交）：
  - `settings.rs`/`lib.rs`：新增 `screenshotUiDetection`（none/window/window_ui，默认 none，patch 只接受这三个值）；`Cargo.toml` 打开 `Win32_Graphics_Dwm`。
  - `overlay.rs`：`UiDetection` 枚举 + `OverlayOptions.ui_detection`；`OverlayState` 增加 `ui_detection`/`preselect`/`press`；进入截图预选全屏，`window` 模式鼠标移动用 `EnumWindows` 取光标下窗口（排除本进程、隐藏/遮蔽窗口，DWM 外框，任务栏也算）；`selecting_rect()` 统一绘制；LBUTTONDOWN 只记起点，移动超过 3px 才转拖拽，否则松手时若在预选内则确认进入标注；`WM_LBUTTONUP` 自定义选区路径保持原样。
  - `App.tsx`：新增 `UiDetectionMode`、设置类型字段、「UI 检测」行（SegmentedControl，`窗口和UI` 置灰）；`SegmentedControl` 支持 `disabled` 档位（点击/拖动都会回到当前值）。
- 2026-11-19 新增 `CountPicker`（数量档位选择器）：显示当前值的胶囊 + 悬停/聚焦展开的档位轨道，支持拖动滑块与点选；窗口置顶「最大数量」与截图「历史保留数量」共用，档位 `WINDOW_PIN_LIMITS = [1,5,10,15,20]`；`align="end"` 用于贴近右缘时向左展开。截图侧原数字输入框与「张」后缀已删除，存储仍为 `screenshotPinHistory`。
- `HotkeyRowEditor` 增加 `hideTitle?: boolean`：为 true 时只渲染 `.hotkey-inline-controls` 控件，不渲染标题与整行留白（供工具栏类紧凑位置使用）。
- `HotkeyRowEditor` 录制 UI（2026-11-19 改版）：单一 `.hotkey-box`（高 30、`padding: 0 8px`、**不设 min-width**，宽度完全随内容），三态靠边框线型与颜色区分——有值=2px 深灰实线 `#8b98aa`，未配置=同色**虚线** + 次要文字色，录制中=主色实线 + 主色文字 + 3px 蓝色光晕；右侧**只有一个按钮**：常态铅笔（进入录制），录制态换成垃圾桶（删除快捷键）。保存/取消按钮已删除，录制框失焦即 `saveHotkey()` 自动校验保存，空草稿只退出不报错。
- `src/App.css`：`.clipboard-settings-dialog` 保持固定 400px（不随内容增高）；`.clipboard-modal-body` 允许内部滚动但隐藏滚动条（`scrollbar-width: none` + `::-webkit-scrollbar` 0 宽）；`.clipboard-hotkey-section` 只留分隔线与间距，行本身与全局/截图页一致的横向单行；`.settings-section.hotkey-settings .hotkey-row` 上下 padding 收到 6px，消除「行很高但只有一行字」的空旷感（曾试过 `align-content: start` / `display: block`，实测该页不存在网格拉伸，已删除这两条死规则）；已删除 `.clipboard-settings-summary`。
- `src/App.tsx` 快捷键行：`HotkeyRowTarget` 只保留 `id`/`name`/`hotkey`，组件不再渲染说明文字，`hotkeyDescription()` 已删除。

- `src-tauri/src/screenshot/mod.rs`：模块装配、`capture_config`（返回 `PinOptions` + 历史数量）、`pin_from_hotkey` / `restore_pin` / `close_all_pins` / `toggle_pins_visible` / `toggle_pins_passthrough`、`pin_hotkey_text()`、`stop()`；`set_pin_auto_copy_text` / `set_pin_auto_copy_persist` 转发。
- `src-tauri/src/screenshot/overlay.rs`：全屏覆盖层（三 DIB 段、脏区增量重绘、辅助线、放大镜、选区翻转、标注阶段与工具栏 `Mode::Screenshot`）；`OverlayOutcome.pin` 与 `request_pin()`。
- `src-tauri/src/screenshot/annotation.rs`：`Item`/`Document`/撤销重做（快照式 `begin_change`）/`hit_test`/绘制；`Item.bounds()` 已就绪，**尚无平移/换文本 API**。
- `src-tauri/src/screenshot/toolbar.rs`：`Mode{Screenshot,Pin}` 两套布局与命中、`Action::Done`/`Pin`、二级栏与下拉。
- `src-tauri/src/screenshot/pin.rs`（约 4400 行）：`screenshot-pin` 线程与指令通道；贴图 wndproc（绘制/拖动/缩放/滚轮/锁定/穿透/双击/关闭确认/右键菜单）；`PinShadow` 三态阴影（灰/蓝/金，`ShadowTone`）；旋转翻转；贴图标注（伴生工具栏小窗 `LightweightToolsetPinToolbar`）；文本选择（OCR 状态机、字符单元、离屏高亮层、`选中后自动复制`）；拖放（自绘 ghost + `ghost_thumbnail` 缩略图 + 自实现 `IDataObject`/`IDropSource`）；单测 9 条。
- `src-tauri/src/screenshot/ocr.rs`：PP-OCRv6 small + `pure-onnx-ocr`（tract 纯 Rust）；单工作线程串行识别、按需加载、用完 drop 引擎并 `trim_working_set`；模型从 exe 同级 `ocr/`（`#[cfg(debug_assertions)]` 才回落仓库）。
- `src-tauri/src/screenshot/{dib,save,clipboard_image}.rs`：DIB 段、PNG 编码与命名模板、剪贴板（图像 + 文本）。
- `src-tauri/src/settings.rs` / `lib.rs`：截图/贴图设置项（含 `screenshot_pin_auto_copy_text`，默认 false）、patch 校验、`set_pin_auto_copy_persist` 注入。
- `src-tauri/src/tools.rs`：`APP_HOTKEYS` 6 条（截图、贴图、恢复、关闭全部、隐藏显示、穿透），逐条注册并汇总失败。
- `src/App.tsx`：截图工具页五个标签（截图/贴图/保存/标注/快捷键）、`PIN_HOTKEY_IDS`、`选中后自动复制` 开关与 `screenshot-pin-auto-copy-changed` 事件同步。
- `src-tauri/resources/ocr/ppocr_v6_small/`：det/rec 的 `inference.onnx` + `inference.yml`（约 30MB，随安装包分发）。
- `src-tauri/tauri.conf.json`：`bundle.resources` 把 `resources/ocr` 映射到安装目录 `ocr/`。

# Verification

- 2026-11-20 预选态「整屏压暗 / 放大镜亮痕」修复（本轮）：`band_needs_full_paint` 增加「脏区与 `selecting_rect()` 相交即完整绘制」，不再只靠预选变化触发整屏重绘。实机：光标停在全屏桌面处触发截图 → 覆盖层正常显示全屏预选（0,0–2560×1600 标签、无压暗、无亮痕）；`cargo check` 与 49 项单测通过。
- 2026-11-19 截图 UI 检测实机验证：
  - `cargo check` / `npm run build` 通过；单测 49 passed / 2 ignored（新增 `ui_detection_parses_modes`）；
  - 进入截图即全屏预选（覆盖层 2560×1600，蓝色边框贴屏幕边缘）；
  - 仅窗口模式：光标移到窗口上后预选切换成窗口矩形（截图对比可见）；
  - 在预选内单击 → 进入标注阶段，工具栏出现、尺寸标签显示（实测量到 1470×1247 的窗口选区）；
  - 「UI 检测」行三档：`不检测`/`仅窗口` 可点并写入 `settings.json`（none/window 互切验证过），`窗口和UI` 置灰不可点；
  - 开发版截图快捷键临时改为 `CTRL+ALT+F9`（避开安装版占用的 A），测试后保留该值便于继续验证。
- 2026-11-19 截图「历史保留数量」改为档位选择器（本轮）：
  - `CountPicker` 从窗口置顶的「最大数量」抽成共享组件，两处复用 `WINDOW_PIN_LIMITS = [1,5,10,15,20]`；截图侧原数字输入框与「张」后缀删除；
  - 实机验证：贴图标签显示胶囊「20」，悬停后向左展开 1/5/10/15/20 五档；点击 10 档写入 `settings.json` 的 `screenshotPinHistory`，控件即时刷新；验证后已改回默认 20；
  - 修复贴近窗口右缘时档位被裁切的问题：新增 `align="end"` 变体（`left:auto; right:0`），截图侧使用。
- 2026-11-19 快捷键框外观定稿（本轮）：边框 2px `#8b98aa`（空态同色虚线）、`padding` 8px、**去掉 min-width** 让宽度真正随内容变化（`CTRL+Q` 明显短于 `CTRL+ALT+A`）；三态实机核对：有值实线 / 未配置虚线 / 录制蓝框+光晕，均清晰可辨。
  - `npm run build` 通过；开发版实机三态外观核对：有值（CTRL+ALT+T + 铅笔）、未配置（剪贴板弹窗「未配置快捷键」虚线边框 + 铅笔）、录制中（「按下快捷键」主色边框 + 垃圾桶）；
  - 录制 `CTRL+ALT+Y` 后点空白处失焦 → 自动保存并提示「窗口置顶 已更新为 CTRL+ALT+Y」，`settings.json` 同步；
  - 录制态点垃圾桶 → 提示「快捷键已停用」，`settings.json` 中该键清空、框回到「未配置快捷键」；
  - 验证后已把窗口置顶快捷键改回默认 `CTRL+ALT+T`。
- 2026-11-19 窗口置顶快捷键迁移（本轮）：
  - `npm run build` 通过；开发版实机：工具栏显示「快捷键 CTRL+ALT+T ✎ 🗑 | 最大数量 | 全部取消」，录制 `CTRL+ALT+Y` 保存后提示「窗口置顶 已更新为 CTRL+ALT+Y」，`settings.json` 同步；
  - 全局「设置 → 快捷键」只剩「软件 → 呼出主窗口」，窗口置顶分组已移除；
  - 验证后已把窗口置顶快捷键改回默认 `CTRL+ALT+T`。
- 2026-11-19 剪贴板快捷键入口迁移（本轮）：
  - `npm run build` 通过（tsc + vite）。Rust 未改动，未跑 `cargo check`。
  - 开发版实机：`npm run dev`(:1420) → 提权启动 `%TEMP%\lwt-dev` 副本（pid 14776，来自该目录）→ 页面正常加载。
  - 剪贴板页「设置」弹窗：全局监听/四项设置后接「剪贴板快捷弹窗」行，摘要行与「快捷键设置」跳转按钮均已消失。
  - **UI 返工（用户验收意见）**：弹窗恢复固定 400px、内容区可滚动但隐藏滚动条；快捷键行去掉说明文字、恢复横向单行（全局页/截图页/剪贴板弹窗三处一致）。
  - 录制 `CTRL+ALT+U` → 保存 → 弹「快捷键已保存」提示（层级在设置弹窗之上）→ 行内即时刷新为新键，无需刷新页面；`settings.json` 中 `hotkeys.clipboard` 同步为 `CTRL+ALT+U`。
  - 全局设置「快捷键」标签：只剩「软件」（呼出主窗口）与「窗口置顶」两组，剪贴板分组已移除。
  - 截图工具页「快捷键」标签回归：截图/贴图/恢复/关闭全部/隐藏显示/鼠标穿透 6 行显示正常。
  - 测试后已停用截图工具；开发版当前状态：剪贴板启用、快捷键 `CTRL+ALT+U`（测试值，非默认 `CTRL+ALT+V`）。
- 2026-11-19 开发版静默启动默认关闭：`cargo build` 通过（仅既有 4 条 warning）；开发版数据目录内 `silentStart=false`；重启后**不借助任何 show 调用**探测到窗口 `visible=True`，页面正常。
- 2026-11-11 `v0.7.0` 发版（已发布）：
  - 发布前：`npm run build` / `cargo check` / `cargo test --lib -- --test-threads=1`（48 passed / 2 ignored）；`npm run tauri build` 成功。
  - 安装验证：静默安装覆盖 `D:\轻量化工具集`；`ocr/ppocr_v6_small/{det,rec}/inference.onnx` 落位、exe 0.7.0（41,409,536 字节）；`Ctrl+Alt+A` 截图 → `Ctrl+Alt+P` 贴图 → OCR `ocr.ready chars=255 lines=15`、菜单恢复「选择所有文本」，证明 release 从安装目录加载模型成功。
  - 验证后已还原正式版 `settings.json`（截图工具保持关闭）并重启安装版；测试产生的 `screenshot/` 数据已删除。
  - 发布：推送 `master`、tag `v0.7.0`、Release 资产 `LightweightToolset_0.7.0_x64-setup.exe`（名称完整、digest 与本地 SHA-256 一致）、README 与 About 更新到 0.7.0。
- 截图功能逐轮验证记录（S1/S2/S3、贴图标注、文本选择、缩略图、金色阴影）见 `archive/2026-11-11-screenshot-progress.md`。

# Blockers

- 无阻塞。项目处于已发版稳定状态（v0.7.0）。

# Next Action

- **开发环境已恢复**（2026-11-20 11:09）：Vite 监听 1420（HTTP 200），开发版实例 pid 4860，来自 `%TEMP%\lwt-dev`，窗口正常加载；截图快捷键 `CTRL+ALT+F9`、`screenshotUiDetection = window` 沿用昨天设置。
- 待办：等用户确认放大镜/鼠标跟手是否达标；若仍慢则拆细放大镜面板重绘；UI 检测这批**未提交**（10 个文件 + 记忆），验收后再提交。
- 快捷键这批已验收并已提交（HEAD，未推送）；推送等你决定。
- 恢复开发：`npm run dev`（:1420）→ `cargo build`（改过 Rust 时）→ **直接** `powershell -Command "Start-Process powershell -Verb RunAs -ArgumentList ... -File %TEMP%\lwt-dev-launch.ps1"`（不要用 `cmd start` 包一层，见 `bugs.md`）→ `%TEMP%\lwt-ui.ps1` 注入点击并截图核对。
- 未做的待办见 `progress.md`：静默启动托盘唤起失效（用户要求稍后）、标注拖动/文本再编辑、缩略图观感、贴图历史浏览（暂缓）。
