# 当前现场

- 2026-11-21 收工：开发版进程已全部结束、Vite（1420）已关闭；`origin/master` = `67a2a54`，工作树只剩既有的 `window_pinner.rs` 换行差异与 `.codex/`（都不该提交）。
- 明天开工：先 `npm run dev` 起 Vite（1420），再 `cargo build` 后把 `src-tauri/target/debug/lightweight-toolset.exe` 复制到 `%TEMP%\lwt-dev\` 并 `Start-Process -Verb RunAs` 启动；注意安装版仍在跑（pid 6112，`D:\轻量化工具集\`），不要结束它。

# Scope

- 工具集已含七个工具：**截图与贴图**（`v0.7.0` 首发，`v0.7.1` 跟随发版）、剪贴板、软件使用统计、按键使用统计、计时器、窗口置顶、快捷键提示。
- **快捷键入口迁移与录制 UI 改版已完成（2026-11-19，用户已验收）**：剪贴板、窗口置顶的快捷键都迁到各自设置页/工具页，全局设置只剩「呼出主窗口」；录制逻辑集中到 `HotkeyRowEditor`（四处在用，`hideTitle` 供工具栏）；录制框单框单按钮、失焦自动保存、外观按 2px 边框 / 宽度随内容 / 空态虚线 / 录制态主色 + 光晕定稿。
- 截图 UI 检测第一阶段已完成（提交 `a198249`，未推送）：UI 检测三档、进入即全屏预选、仅窗口模式跟随窗口（只排除覆盖层自身，本软件窗口/任务栏也可预选）、单击确认/拖拽自定义；UI 元素级检测用户明确暂缓，不再推进。
- 开发版默认值：静默启动默认关闭（代码默认值 + 本机 `settings.json` 均为 false）。
- 截图与贴图范围：区域截图（原生覆盖层/放大镜取色/辅助线/压暗）、12 种标注与二级栏、贴图窗口（拖动/缩放/锁定/穿透/阴影/右键图像处理）、贴图标注（窗口外伴生工具栏、文字与序号）、贴图文本选择（本地 OCR）、超级拖放与跟踪缩略图。
- 明确不在本版：标注拖动/文本再编辑、贴图历史浏览界面（暂缓）、系统级 DragImage 缩略图。

# Git / Working Tree

- 工作目录：`E:\DeepSeek_Harness\Workspace\LightweightToolset`；分支 `master`；HEAD `f81a99e`（`fix: match symbol-key hotkeys by canonical key name`）**已提交、未推送**；`v0.7.2` 为最新发布。
- 本轮（2026-11-21）：修「符号键快捷键被当成呼出主窗口」，改了 `src-tauri/src/tools.rs` 与 `src-tauri/src/lib.rs`；用户确认通过后已提交 `f81a99e`。
- 工作树只剩既有现场：`M src-tauri/src/window_pinner.rs`（换行差异）、`?? .codex/`——不处理、不提交。
- 本机 git 走代理 `http://127.0.0.1:7897`；代理未启动时推送/gh 失败，需先让用户开启。

# Active Files

- 2026-11-21 符号键快捷键匹配修复（已提交 `f81a99e`，未推送）：
  - `src-tauri/src/tools.rs`：新增 `canonical_hotkey` / `canonical_key_token`（符号 ↔ DOM code 名、`ESC/ESCAPE`、`UP/ARROWUP`、`PGUP/PAGEUP` 等别名收敛）；`tool_for_shortcut` / `app_hotkey_for_shortcut` / `ensure_hotkey_available` 改为比较规范化标识（存储值不变、显示文案不变）；`tests` 增 `canonical_hotkey_matches_symbol_keys_with_code_names`、`shortcut_callback_text_uses_code_names_for_symbol_keys`。
  - `src-tauri/src/lib.rs`：快捷键回调把 `handled=false` 的 `main_window` 分支改成独立 `show_main_window_hotkey` 标记；兜底由 `else { show_main_window }` 改为「只有匹配到 main_window 才唤出，其余记 `hotkey.unmatched`」，贴图组快捷键在截图工具停用时不再误弹主窗口。

- 2026-11-20 快捷键体系与主窗口唤出（提交 `c4d3207` / `7fd2878`）：
  - `src-tauri/src/lib.rs`：`show_main_window`（`unminimize` → `show` → 回读可见/最小化 → `FindWindowExW` 兜底）、`log_main_window_event`（写 `<存储目录>/logs/window.log`，256KB 重置）、`find_main_window_hwnd` / `main_window_webview_state`（诊断 `webview=alive/missing/no_window`）；快捷键回调里 `shortcut_tip::notify_app_hotkey` 派发提前到贴图组命令分支之前。
  - `src/App.tsx`：`formatHotkeyForDisplay` 重写（修饰键归类 + 固定顺序 `Win→Ctrl→Alt→Shift`，录制态同口径），覆盖设置页、工具页、剪贴板弹窗、窗口置顶工具栏与空状态、提示列表。
  - `src-tauri/src/tools.rs`：`format_hotkey_display`（同规则）与 `app_hotkey_text` 改用它；单测覆盖 `CTRL+SUPER+/ → Win+Ctrl+/`。
  - `src-tauri/src/shortcut_tip.rs`：`APP_HOTKEY_PRESETS` 3 → 9 条（截图 6 条）、`hotkey_owner_tool` 归属判定、`PRESETS` 24 → 29 条、`canonical_chord` 补 `super`/`command` 别名、提示工具改用统一格式化。
- `src/App.tsx`：新增 `HotkeyRowTarget` / `HotkeyRowEditor`（录制、格式校验、冲突检查、`suspend/resume_tool_hotkeys`、保存与停用），`toHotkeyRowTarget` 供全局页转换条目；`HotkeySettings` 去掉剪贴板分组但保留它参与冲突检查；`ScreenshotToolPage` 与 `ClipboardToolPage` 均改用该组件。
- `src/App.tsx` 剪贴板侧：`ClipboardToolPage` 新增 `appHotkeys`/`tools`/`setAppSnapshot` 入参、`hotkeyNotice` 提示与 `hotkeyTargets` 冲突列表、`currentHotkey` 从实时 `tools` 取值；`ClipboardSettingsDialog` 改为接收 `tool`/`hotkeyTargets`/`setSnapshot`/`onNotice`，内嵌快捷键行。
- `src/App.tsx` 窗口置顶侧：`WindowPinnerToolPage` 改为接收 `appHotkeys`/`tools`/`setAppSnapshot`，工具栏「快捷键」后直接内嵌 `HotkeyRowEditor`（`hideTitle` 紧凑模式），删除跳转全局快捷键页的齿轮按钮；空状态文案改用实时 `currentHotkey`。`ToolPage` 不再需要 `onOpenSettingsTab`，已从签名与调用点移除。
- 2026-11-19 截图 UI 检测（第一阶段，已提交 `a198249`）：
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

- 2026-11-21 已提交并推送：`67a2a54 refactor: render shortcut tip popup with native window`（含入正、设置弹窗、文档）；`origin/master` 已同步，未打 tag、未发版，版本号仍 `0.7.2`。
- 2026-11-21 外观弹窗排版定稿：分隔线分区、两列网格撑满（修 `align-items: center` 导致内容居中）、固定高度、编辑态 X/Y 两列、只显示当前生效主题的模糊/不透明度；开发版已重启。
- 2026-11-21 外观弹窗交互调整：位置行放「编辑/重置」（重置仅编辑期间显示）、预览提示移到底栏、预览固定键盘图标（payload 新增 `icon` 覆盖）、滑块改本地草稿 + 180ms 去抖提交（拖动跟手）并右对齐；`cargo test` 54 passed / 3 ignored、`npm run build` 通过，开发版已重启（pid 26296）。
- 2026-11-21 外观弹窗内容改为纵向分行（主题/动画各占一行、按钮单独一行、分段控件 `width: max-content` 不再压缩），弹窗图标改用设置图标；`npm run build` 通过，开发版已重启（pid 16256）。
- 2026-11-21 外观弹窗改为项目标准弹窗：`createPortal` 到 `document.body` + `dialog-backdrop` 全窗遮盖（z-index 9999、模糊遮罩）+ 进退场动画 + 底部「完成」；`npm run build` 通过，开发版已重启（pid 21984）。
- 2026-11-21 入正 UI 调整：外观设置从页面正文移入弹窗（header 右上角「设置」按钮 → `shortcut-tip-dialog`），页面只保留列表；`npm run build` 通过；开发版已重启（pid 7364 + 子进程 23104）。
- 2026-11-21 入正（未提交）：原生浮窗接管「快捷键提示」，`native_tip` 实验工具与 WebView 浮窗文件删除；`cargo test` 54 passed / 3 ignored、`npm run build` 通过；开发版已重启（pid 25652 + 采键子进程 8384）。待用户验收。
- 2026-11-21 第 3 轮收尾通过：回归（54 passed / 3 ignored，冒烟两次）、`npm run build`、`cargo check`（仅 4 条既有 warning）。工作树全部未提交，等待是否提交/转正的决定。
- 2026-11-21 弹动加速：`POP_MS` 300→230ms（图标/徽标跳动更利落，进入/退出时长未动）。`cargo test` 54 passed / 3 ignored；开发版已重启（pid 17068）。
- 2026-11-21 第 2 轮补丁 7：定位预览图标固定为键盘图标（不再按 Ctrl+C 推断成复制图标）。`cargo test` 54 passed / 3 ignored；开发版已重启（pid 6600）。第 2 轮至此全部通过用户验收。
- 2026-11-21 第 2 轮补丁 6：`start()` 改为「先 `paint()` 再 `ShowWindow`」（消除退出定位后闪旧画面）；`preview` 新增状态标记，静态预览时不开动画定时器且 `tick()` 不再推进 pop（修图标徽标仍弹动）。`cargo test` 54 passed / 3 ignored；开发版已重启（pid 16252）。
- 2026-11-21 第 2 轮补丁 5：定位态"示例瞬间消失/不显示"的**真因**是前端 effect cleanup 误触发（进入时上一次 cleanup 先跑且 ref 已为 true，立刻 hide + 关压制）。改为 `if (!positioning) return;` 只在定位态注册 cleanup；退出定位只改状态、收起交给 cleanup 统一做。开发版已重启（pid 25308），window.log 已清空待验。
- 2026-11-21 第 2 轮补丁 4（重构）：显示/隐藏/预览改为「最新意图 + 单调序号」状态机（`REQUEST`/`NEXT_SEQ`/`APPLIED_SEQ`），`start()` 无条件 ShowWindow，新增 `hide_now()`；删掉原 PENDING/WM_APP+3/HIDE_ACK。`cargo test` 54 passed / 3 ignored；开发版已重启（pid 12812）。
- 2026-11-21 第 2 轮补丁 3：定位态生命周期闭合（后端拒绝迟到预览、前端 ref 拦截在途滑块请求、进入先收起再接管、退出先关压制再收起）；`cargo test` 54 passed / 3 ignored、`npm run build` 通过；开发版已重启（pid 25228）。
- 2026-11-21 第 2 轮补丁 2：定位预览不播跳动（pop 直接 1）、hide() 等窗口线程确认（消除退出后首弹"闪一下"）、累加窗口跟随停留时长、预览标题改「示例提示」；`cargo test` 54 passed / 3 ignored、`npm run build` 通过；开发版已重启（pid 25416）。
- 2026-11-21 第 2 轮补丁：定位预览不播进入动画、淡入中请求不重放（修"弹两次"）、后端 `POSITIONING` 全局压制普通弹出（`native_tip_set_positioning`）；`cargo test` 54 passed / 3 ignored、`npm run build` 通过；开发版已重启（pid 24796）。
- 2026-11-21 第 2 轮完成（功能，未提交）：停留时间滑块、下线实时背景、X/Y 定位滑块 + 定位模式（`native_tip_metrics` / `native_tip_hide`）；拖动定位整段代码移除。`cargo test` 54 passed / 3 ignored、`npm run build` 通过；开发版已重启（pid 7012）。未做：长快捷键超宽时窗口自动加宽（记为后续小项）。
- 2026-11-21 第 1 轮收尾：进入动画 260→200ms（用户确认"差不多了，可以快一点"），退出保持 300ms 未动；`cargo test` 54 passed / 3 ignored；开发版已重启（pid 19044）。第 1 轮功能项已完成，等待用户验收后进入第 2 轮。
- 2026-11-21 第 1 轮补丁 3：淡入期间冻结内容（消除重绘抖动）、重触发改从当前 alpha 续上（修半透明卡住）、进入 260ms；`cargo test` 54 passed / 3 ignored；开发版已重启（pid 5192）。
- 2026-11-21 第 1 轮补丁 2：窗口缩放配置取消（固定 1.00）、进入动画 320ms 缓入缓出、动画定时器 8ms + `timeBeginPeriod(1)`；`cargo test` 54 passed / 3 ignored、`npm run build` 通过；开发版已重启（pid 22436）。
- 2026-11-21 第 1 轮补丁（性能）：帧缓存（内容指纹不变只换整体 alpha）+ 淡入 200ms/淡出 300ms；`cargo test` 54 passed / 3 ignored、`npm run build` 通过；开发版已重启（pid 20932）。
- 2026-11-21 第 1 轮完成（视觉与动画，未提交）：标题 13.5→14.5、键帽 11.5→12.5 加粗并加大内边距、徽标 11.5/700，三处文字改用「墨迹居中」（`GetTextMetricsW` 推算大写字高，解决数字偏上）；滑入淡出改缓出曲线（`1-(1-t)^3`）；动画进度改**真实时间**驱动（缩放不再改变时长）；窗口尺寸改按 payload 现算（修刚缩放时阴影被裁）；阴影边距 9→11、扩散 0.75×radius。`cargo test` 54 passed / 3 ignored、`npm run build` 通过；开发版已重启（pid 16216）。
- 2026-11-21 现场：原生提示实验源码**全部未提交**（新增 `src-tauri/src/native_tip.rs`；改 `Cargo.toml`/`lib.rs`/`tools.rs`/`App.tsx`/`App.css`/`main.tsx`）；HEAD 仍是 `3e9fde2`。开发版在跑（Vite + `%TEMP%\lwt-dev`）。下一轮从「第 1 轮：视觉与动画」开始。
- 2026-11-21 白屏修复：滑块 `onChange` 在 state updater 里读 `event.currentTarget`，React 延迟执行时已置空 → 渲染期 TypeError 白屏。已改为事件内先取值；`npm run build` 通过，开发版已重启。前端错误落盘命令 `frontend_error_log` + 错误边界保留。
- 2026-11-21 原生提示实验 v4（内容比例 1.20 固定并随窗口自适应，去掉内容缩放滑块；新增前端错误边界与 `logs/frontend.log` 落盘用于排查白屏，未提交）：`cargo test --lib -- --test-threads=1` 54 passed / 3 ignored、`npm run build` 通过；开发版已重启（pid 5808）。
- 2026-11-21 原生提示实验 v3（DIY 面板：内容/窗口缩放、主题三档、动画、实时背景、拖动定位，未提交）：`cargo test --lib -- --test-threads=1` 54 passed / 3 ignored、`npm run build` 通过；开发版已重启（pid 19816）。
- 2026-11-21 原生提示实验 v2（内容 1.10 + 主题三档 + 深浅参数各自可调，未提交）：`cargo test --lib -- --test-threads=1` 54 passed / 3 ignored、`npm run build` 通过；开发版已重启（pid 16604）。
- 2026-11-21 原生提示实验（未提交）：`cargo test --lib -- --test-threads=1` 54 passed / 3 ignored；冒烟测试（`native_tip -- --ignored`）跑「显示→停用→再显示→再停用」两轮通过；`NATIVE_TIP_DUMP` 导出的深/浅主题帧已人工核对（亚克力底、圆角、图标胶囊、键帽、徽标）。开发版已用新构建重启（进程 23208）。

- 2026-11-21 「隐藏即挂起」试验（已回退，不改代码）：新加 `window_service::set_webview_suspended`（`ICoreWebView2_3::TrySuspend/Resume`，依赖 `webview2-com 0.38`）接在主窗口隐藏/唤出；实测 主窗口可见→`WM_CLOSE` 隐藏后渲染进程 WS 94.1→91.0MB、Private 49.4→45.6MB，CPU 无可测变化，`logs/window.log` 无 `webview.power` 报错。收益在噪声内 → `git checkout` 还原 `Cargo.toml`/`Cargo.lock`/`lib.rs`/`window_service.rs`，`cargo check` 与 `cargo build` 恢复干净（4 条既有 warning）。
- 2026-11-21 WebView2 占用基线：见 `progress.md` 待办 11（正式版单实例 WS ≈ 518MB / Private ≈ 338MB，8 进程明细齐全）。

- 2026-11-21 符号键快捷键修复（本轮）：
  - `cargo test --lib -- --test-threads=1`（`TEMP`/`TMP` 指向 `src-tauri/target/test-tmp`）**54 passed / 0 failed / 2 ignored**；`cargo check` 通过（4 条既有 warning）。
  - 根因用单测固化：`"CTRL+SUPER+/".parse::<Shortcut>().unwrap().to_string()` == `control+super+Slash`（存储侧 `CTRL+SUPER+/`，回调侧 `CTRL+SUPER+SLASH`）。
  - 开发版（`%TEMP%\lwt-dev`，提权）注入 `Ctrl+Win+/` → `Clipboard` 窗口可见；Esc 后消失。`%APPDATA%\com.the.lightweight-toolset.dev\settings.json` 预置 `tools.clipboard=true`、`hotkeys.clipboard=CTRL+SUPER+/`。
  - 回归：主窗口快捷键临时设为 `CTRL+ALT+.`，`WM_CLOSE` 隐藏到托盘后按 `Ctrl+Alt+.` → 主窗口重新 `visible=True`（此前只靠兜底碰巧生效）。
  - 观察（既有行为，非本轮引入）：`shortcut_tip` 工具未启用时，本软件快捷键仍会弹提示浮窗（`notify_app_hotkey` 只判断归属工具是否启用，不判断提示工具本身）。


- 2026-11-21 环境核对：安装版 `D:\轻量化工具集\lightweight-toolset.exe` 为 `0.7.2`，两个进程（主进程 + 采键 helper）`Responding=True`；`%APPDATA%\com.the.lightweight-toolset\logs\window.log` 不存在（无建窗让路/唤出失败记录）；开发环境未运行（`:1420` 无监听、`%TEMP%\lwt-dev` 与开发版数据目录均已删除）。
- 2026-11-20 `v0.7.2` 发版（上一轮）：
  - 定位：正式版 `v0.7.1` 卡死进程 `11808`（`Responding=False`、所有窗口 `WM_NULL` HUNG）；用 `StackWalk64`+dbghelp 回溯主线程栈，确认「嵌套建窗互等窗口锁」死锁。
  - 修复后开发版复现同一路径：`window.log` 记录 `window_build.busy defer=shortcut_tip` 两次，剪贴板弹窗与提示浮窗均正常出现、主窗口保持隐藏、`ping=OK`、进程 `responding=True`；连按三次稳定。
  - `cargo test --lib -- --test-threads=1` 52 passed / 2 ignored；`npm run tauri build` 成功：release exe 41,463,808 字节量级、NSIS `轻量化工具集_0.7.2_x64-setup.exe` 33,851,424 字节；ASCII 资产名 `LightweightToolset_0.7.2_x64-setup.exe` 上传后核对名称与 digest 一致。
- 2026-11-20 `v0.7.1` 发版（上一轮）：
  - 发版前：`npm run build`、`cargo check`、`cargo test --lib -- --test-threads=1`（52 passed / 2 ignored）全部通过；已按固定清理项停掉开发版并删除 `%APPDATA%`/`%LOCALAPPDATA%` 开发版目录、开始菜单开发版快捷方式与 `%TEMP%\lwt-dev`。
  - `npm run tauri build` 成功：release exe 41,460,224 字节，NSIS 产物 `轻量化工具集_0.7.1_x64-setup.exe` 33,856,072 字节；复制为 ASCII 名 `LightweightToolset_0.7.1_x64-setup.exe` 后上传，`gh release view` 核对资产名完整、digest 与本地 SHA-256 一致。
  - 已建带注释 tag `v0.7.1` 并推送；`gh release create` 发布 `https://github.com/THE2580/LightweightToolset/releases/tag/v0.7.1`。
- 2026-11-20 截图设置页调整（`c02cd27`，随 0.7.1 发布）：「UI 检测」独立成行；「显示辅助线」整行可点、箭头右置、默认收起、切「不显示」自动收起、切标签回来恢复折叠、去掉模式重置按钮；实机逐项截图核对。
- 2026-11-20 快捷键提示预设扩充（`7fd2878`，随 0.7.1 发布）：
  - 新增固定预设 `Win+P`（投影/切换显示模式）、`Win+R`（运行）、`Win+T`（任务栏预览）、`Win+G`（打开 Game Bar）、`Alt+F4`（关闭当前窗口）；
  - 绑定设置的预设 3 → 9 条：新增截图 6 条（截图/贴图/恢复/关闭全部/隐藏显示/鼠标穿透）；`hotkey_owner_tool` 统一判定归属工具，`build_snapshot` 的可用状态与 `notify_app_hotkey` 的停用判断都改用它；
  - 实机：提示页 41 项（29 条固定预设 + 9 条绑定条目 + 3 条用户自定义），列表可见 `Win+G 打开 Game Bar`、`Win+T 任务栏预览`、`Win+P 投影/切换显示模式`、`Ctrl+Alt+T 窗口置顶`、`Ctrl+Alt+P 贴图`、`鼠标穿透 未设置`；
  - `cargo test --lib -- --test-threads=1` **52 passed / 0 failed / 2 ignored**（新增 `app_hotkey_presets_map_to_owning_tools`）。
- 2026-11-20 贴图组快捷键不提示修复（本轮，未提交）：`lib.rs` 回调里把 `shortcut_tip::notify_app_hotkey` 派发提前到贴图组命令分支之前（该分支会直接 `return`）。实机用「窗口出现即判」的方式逐条核对：`Ctrl+Alt+P`（贴图）、`Ctrl+Q`（恢复）、`Ctrl+Alt+X`（关闭全部）、`Ctrl+Alt+H`（隐藏/显示）按下后 `Shortcut tip` 窗口均在 100ms 内可见。

- 2026-11-20 快捷键外显口径统一（本轮，未提交）：
  - 前端 `formatHotkeyForDisplay` 重写（`App.tsx`）：修饰键 `Ctrl/Alt/Shift/Win`，`SUPER/META/CMD/COMMAND/WINDOWS` 一律 `Win`，功能键名转写（`Space/Esc/PageUp`…）；覆盖设置页、截图页、剪贴板设置弹窗、窗口置顶工具栏与空状态、快捷键提示列表、录制态。
  - 后端新增 `tools::format_hotkey_display`，用于 `app_hotkey_text`（启动通知）、提示工具的本软件快捷键条目与浮窗；`canonical_chord` 补 `super` 别名（此前 `CTRL+SUPER+/` 解析失败，直接把原值弹到浮窗，这就是 `SUPER` 的来源）。
  - 实机核对（顺序按要求最终定为 `Win` → `Ctrl` → `Alt` → `Shift` → 主键）：设置→快捷键 `Ctrl+Alt+.`；截图页 `Ctrl+Alt+F9 / Ctrl+Alt+P / Ctrl+Q / Ctrl+Alt+X / Ctrl+Alt+H`；剪贴板设置弹窗 **`Win+Ctrl+/`**；快捷键提示列表 `Win+Ctrl+/`、`Ctrl+Alt+T`、`Win+Down`；录制态实时显示 `Ctrl+Alt+K`。
  - `npm run build` 通过；`cargo check` 通过；`cargo test --lib -- --test-threads=1` **51 passed / 0 failed / 2 ignored**（新增 `formats_hotkey_for_display_with_titlecase_modifiers`、`canonical_chord_accepts_super_alias`）。
  - 测试用的 `pin_passthrough` 已清空，`shortcut_tip` 工具已恢复关闭，开发版重启后窗口尺寸恢复默认。

- 2026-11-20 主窗口唤出加固（本轮，未提交）：
  - `show_main_window`（`lib.rs`）改为：`log_main_window_event` 落盘 → `get_webview_window("main")` 有界重试（5×20ms）→ 取不到时 `FindWindowExW` 按「本进程 + `Tauri Window` + 窗口标题」找回 HWND → `is_minimized()` 先 `unminimize()` → `show()` → `set_focus()`。
  - 实机：静默启动（`silentStart=true`）→ 隐藏态按 `CTRL+ALT+.` → 窗口出现；`SW_MINIMIZE` 最小化后按同一快捷键 → `visible=True iconic=False`（修复前是 `iconic=True`，即仍缩在任务栏）。
  - `%APPDATA%\com.the.lightweight-toolset.dev\logs\window.log` 生成并记录 `main_window.show_requested`。
  - `npm run build` 通过；`cargo check` 通过（4 条既有 warning）；`cargo test --lib -- --test-threads=1` **49 passed / 0 failed / 2 ignored**。
  - 未能复现用户报的「静默启动后整个软件像死的」：9 次静默冷启动后 `SendMessageTimeout(WM_NULL)` 全部 `ping=OK`，窗口隐藏、快捷键可唤出。
  - 测试后已把开发版 `silentStart` 改回 `false` 并重启开发版（pid 4456，窗口可见）。

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

- 无阻塞。项目处于已发版稳定状态（v0.7.1）。

# Next Action

- 本轮修复已提交 `f81a99e` + 文档 `3e9fde2`，**未推送、未发版**；下次发版随 `v0.7.3` 带上（缺陷修复调第三位）。
- **WebView2 占用优化**：基线数据与「挂起已试并回退」的结论见 `progress.md` 待办 11；下一步候选是「提示浮窗空闲销毁」与「提示浮窗原生化」（待办 12），都**未开工**，等用户拍板。
- 推送前必须先拿到明确指令，并让用户开代理 `127.0.0.1:7897`。
- 开发版仍在跑（Vite `:1420` + `%TEMP%\lwt-dev`，已还原为提交状态构建）。
- 用户已安装并日常使用 `v0.7.2`（`D:\轻量化工具集\`，主进程 + 采键 helper 常驻，响应正常）；升级后未再复现整机卡死。
- 等用户指定下一批功能；**UI 元素级检测已明确暂缓，不要主动做**。

- 需要重新开发时按「验证方式」重建环境：`npm run dev`（:1420）→ `cargo build` → 复制 exe 到 `%TEMP%\lwt-dev\` → 提权启动。
- 未做的待办见 `progress.md`：标注拖动/文本再编辑、缩略图观感、贴图历史浏览（暂缓）。
