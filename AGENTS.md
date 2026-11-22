# AGENTS.md

## 项目目标

LightweightToolset 是基于 Tauri 2 + Rust + React + TypeScript 的 Windows 轻量化桌面工具集：固定主窗口、单实例、单托盘，工具编译期注册、可独立启用/停用，所有数据本地存储。现有工具：截图与贴图、剪贴板、软件使用统计、按键使用统计、计时器、窗口置顶、快捷键提示。当前稳定版 `v0.7.2`。旧 Electron 版 LightweightWindowsToolset 只作迁移参考，新功能不回写。

## 技术栈

- 前端：React 19、TypeScript 5.8（`strict`、`noUnusedLocals`、`noUnusedParameters`）、Vite 7、framer-motion、lucide-react。
- 后端：Tauri 2、Rust 2021，直接调用 `windows` / `windows-sys` 的 Win32 API。
- 打包：NSIS（`src-tauri/target/release/bundle/nsis/`）。
- 本机工具链：Node 24.12、cargo/rustc 1.96；npm 在 `E:\devtools\node\v24.12.0\`（Git Bash 的 PATH 里可能没有，必要时用 PowerShell 或绝对路径）。

## 目录结构

- `src/`：React 前端。`App.tsx` 为主窗口（体量大，优先局部修改）；`popup.tsx` / `free.tsx` 为快捷弹窗与自由窗口；`shortcutNotify.ts` 是页面按键上报桥。快捷键提示浮窗由原生实现绘制（见 `native_tip.rs`），已无 WebView 浮窗页；样式集中在 `App.css`、`popup.css`、`free.css`。
- `src-tauri/src/`：Rust 后端。`lib.rs` 为应用装配与命令入口；`main.rs` 兼作采键子进程入口（`--shortcut-tip-helper`）；`autostart.rs` 开机自启（计划任务）；`tools.rs` 工具注册与生命周期；`clipboard.rs`、`shortcut_tip.rs`、`app_usage.rs`、`key_usage.rs`、`input_monitor.rs`、`timer.rs`、`window_pinner.rs`、`window_service.rs`、`windows_notification.rs`、`settings.rs` 按功能拆分；`native_tip.rs` 是快捷键提示的原生浮窗（Win32 分层窗口 + GDI 自绘）。
- `src-tauri/src/screenshot/`：截图与贴图工具。`overlay.rs` 全屏覆盖层（选区/放大镜/辅助线/标注阶段）、`annotation.rs` 标注模型与绘制、`toolbar.rs` 工具栏两套布局（截图/贴图）、`pin.rs` 贴图窗口（含标注、文本选择、拖放缩略图、三态阴影）、`ocr.rs` 本地 OCR（PP-OCRv6 + pure-onnx-ocr）、`dib.rs` / `save.rs` / `clipboard_image.rs` 像素与落盘。
- `src-tauri/resources/ocr/ppocr_v6_small/`：OCR 模型（det/rec 的 `inference.onnx` + `inference.yml`，约 30MB），由 `tauri.conf.json` 的 `bundle.resources` 放到安装目录 `ocr/`；运行时只认 exe 同级 `ocr/`。
- `src-tauri/capabilities/default.json`：Tauri 权限清单；`src-tauri/tauri.conf.json`：窗口/打包配置。
- `agent_memory/`：项目记忆（权威状态源，随仓库提交）；`archive/` 存旧摘要。

## 常用命令

```powershell
npm install                          # 首次/依赖变更后
npm run dev                          # 仅前端 Vite
npm run build                        # tsc + vite build（前端类型与产物验证）
npm run tauri build                  # release + NSIS 安装包
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --lib --manifest-path src-tauri/Cargo.toml -- --test-threads=1
```

**启动开发版**（`npm run tauri dev` 不可用）：

- 原因：cargo 用 CreateProcess 拉起 `target\debug\lightweight-toolset.exe`，`requireAdministrator` 清单返回 `os error 740`，Vite 随之退出。
- 做法：`npm run dev` 起 Vite → 把 `src-tauri/target/debug/lightweight-toolset.exe` 复制到 `%TEMP%` → `Start-Process -Verb RunAs`（或双击）启动；改了 Rust 代码要先 `cargo build` 再重新复制。
- 静默启动会把主窗口隐藏，隐藏期间 Vite HMR 不一定落到页面，改前端后重启开发版再验证。
- 开发版数据与正式版隔离，测试完按「分支、提交与发布」的清理项处理。

全仓 `cargo fmt --check` 与严格 clippy 目前有既有告警/格式问题，不作为准入条件；也不要对仓库 Rust 文件直接跑 `rustfmt`（原因见「平台约束与踩坑经验」），按既有行宽手工对齐即可。

## 代码风格

- 沟通与文档用中文；代码标识、命令、路径保持仓库既有英文风格。
- 默认最小改动：复用现有工具注册、`AppState`、`push_debug_log`、全局 Toast/Tooltip、`SegmentedControl` 等既有实现；不做无关重构。
- 修 bug 先找共享入口与根因，同时检查同类 sibling 路径。
- 前端只通过现有 Tauri command 取数；新增能力先加 Rust command 与 capability，再在前端调用。
- 数量档位控件统一用 `CountPicker`（窗口置顶「最大数量」与截图「历史保留数量」共用）：档位由 `limits` 传入，靠近右缘时用 `align="end"` 向左展开；不要再为这类“几档数值”另写数字输入框。
- Rust 错误统一转 `String` 返回给前端；日志走 `push_debug_log`，不要 `println!` 留生产噪声。
- **应用内弹窗统一走 `createPortal(document.body)` + `.dialog-backdrop` 规范**：遮罩必须盖住整个窗口（`position: fixed; inset: 0; z-index: 9999; backdrop-filter: blur(3px)`），内容用 `.update-dialog`（含 `.update-dialog-header` / body / `.update-dialog-actions`），进场 `dialog-fade` + `dialog-pop`、退场靠 `closing` class 再延迟 170ms 卸载。禁止把弹窗直接渲染在工具页 DOM 里（那样遮罩只盖内容区、侧栏不被遮住、也没有淡入淡出）。新弹窗复用这套 class 与开合写法，不另写遮罩/动画/CSS。
- 设置项里的可折叠分组用 `.settings-row-toggle`：整行可点、箭头放在标题右侧、默认收起，点右侧控件不触发折叠；子设置不能挂在别的开关的条件渲染里（「UI 检测」曾被放进辅助线条件块，辅助线关掉时它一起消失）。

## 快捷键设置约定

- 工具快捷键的配置入口放在**该工具自己的页面/设置页里**，全局设置只保留软件级动作（目前只有「呼出主窗口」）。新增带快捷键的工具：后端在 `tools.rs` 注册 `supportsHotkey`，前端在其页面渲染一行 `HotkeyRowEditor`，不要复制录制逻辑；同时在全局 `HotkeySettings` 的分组里排除该工具，但**保留它参与冲突检查**。
- `HotkeyRowEditor`（`src/App.tsx`）是唯一的快捷键录制实现，全局页、截图页、剪贴板设置弹窗、窗口置顶工具栏共用；工具栏这类紧凑位置用 `hideTitle` 只渲染控件部分。
- 录制交互固定为「单框 + 单按钮」：常态显示当前键（或「未配置快捷键」）+ 铅笔，录制态显示「按下快捷键」+ 垃圾桶；**不设保存/取消按钮**，录制框失焦即自动校验保存，空草稿静默退出。
- 录制框 `.hotkey-box` 的视觉口径：宽度随内容（不设 `min-width`）、2px 边框；有值实线、未配置虚线、录制中主色 + 光晕。
- 工具页内保存快捷键后必须从实时 `tools` 读 `hotkey`：`activeTool` 是导航时的快照，不会随保存刷新。
- **快捷键外显口径统一**：存储值仍是 `normalize_hotkey` 的大写形式（`CTRL+SUPER+X`，`SUPER` 是 `Win` 的存储别名），但任何展示给用户的文本都必须过格式化函数：前端 `formatHotkeyForDisplay`（`src/App.tsx`）、后端 `tools::format_hotkey_display`。规则：修饰键固定 `Win` → `Ctrl` → `Alt` → `Shift` → 主键 的顺序（首字母大写、`+` 连接），`SUPER` / `META` / `CMD` / `COMMAND` / `WINDOWS` 一律显示为 `Win`，功能键名 `Space` / `Esc` / `PageUp` 等也按此表转写。不要在 JSX 或通知文案里直接渲染 `hotkey` 原值，也不要在录制态另拼一套大写文案。
- **快捷键提示预设要跟着工具走**：新增或调整带快捷键的工具时，同步在 `shortcut_tip.rs` 的 `APP_HOTKEY_PRESETS` 加条目，并在 `hotkey_owner_tool` 里登记它归属的工具 id；否则该快捷键在提示工具里没有条目、或会被当成「工具已停用」而永不提示。系统级固定预设加在 `PRESETS`（顺序即新装后的展示顺序，只在缺失时补入，不覆盖用户改过的条目）。快捷键回调里若新增「执行完就 return」的分支，必须把 `notify_app_hotkey` 的派发放在该分支之前。

## 硬性约束（禁止事项）

- 未经用户明确要求：不提交、不推送、不发布。开发版除外：按「验证方式」的固定动作，每次改动完成即主动启动供用户测试，不需等待用户指示。
- 不提交 `node_modules/`、`dist/`、`src-tauri/target/`、QA 截图、日志或缓存；`agent_memory/` 必须入库，不要加回 `.gitignore`。
- 不恢复用户主动删除的内容；不修改与本任务无关的既有工作树现场（当前只剩 `.codex/` 与 `window_pinner.rs` 的换行差异）。
- 工具停用必须真正停止快捷键、worker、监听、计时器、关联窗口和 helper，不能只隐藏 UI。
- 不做插件市场、远程插件、脚本扩展或运行时安装工具；只做编译期注册。
- 不恢复 AI 聊天、API 设置、DeepSeek 或资源捕获入口。
- 主程序以管理员权限运行（`requireAdministrator` 清单 `src-tauri/windows-app-manifest.xml`）；按键捕获依赖进程内低级键盘钩子，覆盖同级及更低完整性窗口，UAC 安全桌面与 System 完整性进程不可见。手动启动会走 UAC（本机策略 `ConsentPromptBehaviorAdmin=0` 时静默提权）；开机自启必须走计划任务（见「平台约束与踩坑经验」），不要改回 Run 键。
- 开发版与正式版必须完全隔离；禁止让开发版复用正式版 identifier、改动正式版数据或自启项。差异与操作要求见「开发版与正式版」。
- 不绕过 DSH 沙箱或审批；权限不足时说明并等待用户处理。
- 保留首次 release 构建产物；`src-tauri/target` 膨胀时只清理可再生的 `target/debug`，禁止 `cargo clean`（会连带删除 release）。

## 开发版与正式版（差异与注意事项）

### 标识与隔离的实现

- 正式版 identifier 固定 `com.the.lightweight-toolset`，**不得修改**（改了会换数据目录，等于丢用户数据）。
- 开发版（debug 构建）在 `run()` 中用 `context.config_mut().identifier` 改为 `com.the.lightweight-toolset.dev`。以下全部由 identifier 派生，因此自动独立：
  - `%APPDATA%` / `%LOCALAPPDATA%` 下的配置与数据目录；
  - WebView2 用户数据（缓存、登录态等）；
  - 单实例互斥量与窗口类（`<identifier>-sim` / `-siw` / `-sic`）；
  - 通知 AUMID；
  - 开始菜单快捷方式（开发版为「轻量化工具集 (开发版).lnk」）；
  - 开机自启项（开发版不注册、不写入设置）。
- 两版可同时运行、互不抢单实例。禁止退回「同一 identifier + 手动换端口」的做法。

### 默认值对照（开发版默认值仅在数据目录全新、未改过设置时生效）

| 项 | 正式版 | 开发版 |
|---|---|---|
| 开机自启 | 默认关闭，可开关 | 默认关闭，**开关锁定、不可点** |
| 静默启动 | 默认关闭，可开关 | 默认关闭，可开关 |
| 主窗口始终置顶 | 默认关闭，可开关 | 默认**开启**，可开关 |
| 开发者模式 | 默认关闭，可开关 | 默认**开启**，**开关锁定、不可点** |
| 工具启用 | 剪贴板、快捷键提示默认开启 | **全部工具默认关闭** |
| 数据存储路径 | 可修改 | **不可修改**（界面禁用 + 后端拒绝） |

### 常驻行为差异

- **窗口标题**：展示时开发版在用户设置的标题后追加「（开发版）」，存储值仍是用户原文；自绘标题栏与系统窗口标题一并生效。正式版不追加。
- **托盘悬停与应用名**：统一使用中文产品名「轻量化工具集」，开发版追加「（开发版）」；不要再出现 `LightweightToolset` 这类程序名。
- **锁定项**：开发版的开机自启、开发者模式在界面上置灰不可点，后端 `set_auto_start_enabled` / `update_app_settings` 也会直接忽略并记日志；开发版修改存储路径的请求同样被忽略。

### 并存时的已知限制

- Windows 全局快捷键系统级独占：两版同时运行时，后启动的一方可能注册失败（控制台 `HotKey already registered`），属预期；托盘图标、再次启动程序仍可唤起窗口。不要为此去结束另一版进程。
- 两版的托盘图标、窗口、数据互不影响；测试破坏性操作（清空历史等）时确认操作的是开发版窗口。

### 发版前（固定动作）

- 结束开发版进程，删除开发版数据/缓存/快捷方式与 `%TEMP%\lwt-dev`；**不得**清理正式版目录。具体命令见「分支、提交与发布」。

## 平台约束与踩坑经验（必须遵守）

- **开机自启必须用计划任务，不能用 Run 键**：`requireAdministrator` 的 exe 无法由 Explorer 从 `HKCU\...\Run` 拉起——Explorer 只做 CreateProcess，提权失败返回 `ERROR_ELEVATION_REQUIRED` 并被静默丢弃（证据：`Microsoft-Windows-Shell-Core/Operational` 事件 9708 PID 0，`Microsoft-Windows-UAC/Operational` 事件 1）。现实现为 `autostart.rs`：生成 UTF-16 任务 XML，`schtasks /Create /XML` 注册「登录触发 + `HighestAvailable` + `InteractiveToken`」任务，登录时由任务计划程序以完整令牌启动、不弹 UAC；必须显式 `<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>`，否则默认 72 小时会杀掉常驻进程；同时清理旧版写入的 Run 值。不要引入 `tauri-plugin-autostart`（它只会写 Run 键，正是本缺陷来源）。
- **静默启动必须靠 `visible:false` + 条件 `show()`**：`tauri.conf.json` 主窗口固定 `"visible": false`，在 `setup` 里读设置：`silent_start` 为 false 才 `show()` + `set_focus()`，为 true 就保持隐藏、只留托盘。不要在启动后调 `hide()`（会先闪一帧窗口）；隐藏状态下 WebView 照常加载，快捷键/剪贴板/统计/计时器不受影响。任何唤起入口（托盘、主窗口快捷键、单实例二次启动）都必须走 `show_main_window`。设置说明与启动提示各发一条通知，文案里的快捷键取实际生效值。
- **`show_main_window` 必须先还原再显示**：窗口只是最小化到任务栏（未隐藏到托盘）时，`show()` 只做 `SW_SHOW`，不会把窗口从最小化还原，表现就是「按快捷键没反应、窗口还缩在任务栏」。固定口径：`is_minimized()` 为真先 `unminimize()`，再 `show()`、`set_focus()`；`get_webview_window("main")` 允许有界重试（运行时对窗口记录的挂载存在竞态），最终仍取不到时用原生 `FindWindowExW` 按「本进程 + `Tauri Window` 类 + 窗口标题」找回 HWND 兜底还原。唤出事件同时写 `<存储目录>/logs/window.log`（超过 256KB 重置），因为「唤不出」卡死时内存里的调试日志读不到。
- **版本号单一来源**：前端版本号从 `../package.json` 直接 `import`（`src/App.tsx` 的 `APP_VERSION`）。禁止用 Vite `define` 注入全局常量——打包时会替换、dev 服务器不替换，会直接白屏。`package.json` / `src-tauri/Cargo.toml` / `src-tauri/tauri.conf.json` 三处版本号发版时同步修改。
- **采键通道与 Chromium 限制**：Chromium/WebView2 窗口在前台时，Windows 会间断地不给 `WH_KEYBOARD_LL` 派发按键；被系统或其他软件注册走的热键（`Win+D`、`Win+I`、`Win+Shift+S`、他软件全局热键）连 Raw Input 都收不到。现有的三条通道缺一不可：① 软件自身窗口的普通按键走页面 `keydown`（`src/shortcutNotify.ts` → `shortcut_tip_report`）；② 焦点在别处时走进程内钩子；③ 系统/他软件热键走独立采键子进程——复用同一个 exe，带 `--shortcut-tip-helper <port>` 启动，不初始化 Tauri/WebView（分叉要在进入 Tauri 之前），装钩子后经 `127.0.0.1` 回环 TCP 回传组合键；子进程继承提权、不加 UAC，连接断开或回传失败必须自行退出，工具停用时主程序负责结束它。三条通道共用节流与去重。
- **已排除方案：Raw Input 兜底**。实测 Raw Input（`RIDEV_INPUTSINK`）收得到普通键，但收不到被系统/他软件注册走的 Win 组合键；A/B 对照（禁用/启用）现象完全一致。不要再为「系统热键收不到」接 Raw Input。
- **浮窗提示节奏**：同一来源重复上报按 `0.5s` 节流（按住快捷键自动重复时逐 0.5s 累加），跨来源按 `400ms` 去重；应用内触发一律**立即**提示，不等剪贴板确认；松手后累加必须立即停止，不允许积压成队列。
- **浮窗尺寸与层级**：不要按字符估算宽度再动态改窗口（后续每次显示都会用估算值覆盖实测值，表现为闪一下又变窄）；用固定宽窗口 + 卡片 `width: max-content` 居中。只设一次 `always_on_top` 压不过同样置顶的窗口，显示时与显示期间（每 400ms）都要 `SetWindowPos(HWND_TOPMOST)` 重新压顶。
- **锁顺序与死锁**：`AppState.registry` 与工具自有存储锁不能嵌套持有；快照类接口必须「先拷贝数据、释放存储锁，再查注册表」。快捷键回调运行在主线程，**禁止**在其中做加锁 + 窗口操作，一律丢到独立线程，否则会与前端 2 秒轮询的快照形成环形等待、整窗卡死。
- **禁止在主线程上嵌套创建 WebviewWindow（`v0.7.1` 致命卡死的根因）**：tauri 的建窗消息走 `send_user_message`，在主线程上会**就地执行**；建窗期间 Windows 派发窗口消息，会把排队的另一个建窗任务也执行，两份建窗嵌套会互相等 tauri 的窗口锁 → 主线程永久死锁，表现为「进程和托盘图标都在，但托盘菜单、快捷键、所有窗口全部无响应」。固定口径：所有 `WebviewWindowBuilder::build()` 前先用 `window_service::begin_window_build()` 占位（已覆盖剪贴板弹窗、计时器自由窗口/时钟窗口、快捷键提示浮窗）；后到的建窗任务见 `window_build_busy()` 为真时必须让路——浮窗退避重试、弹窗交给独立线程排队，绝不嵌套建。让路行为会写 `<存储目录>/logs/window.log`（`window_build.busy defer=...`）。
- **主线程卡死的取证方法（本机没有 procdump/cdb/windbg）**：先用 `SendMessageTimeout(HWND, WM_NULL, 2, 3000)` 给该进程每个顶层窗口发探活消息——返回 0 即主线程卡死（`Get-Process.Responding=False` 可交叉验证）；确认后从**提权** PowerShell 里用 `OpenProcess + CreateToolhelp32Snapshot + GetThreadContext + StackWalk64`（dbghelp）走主线程栈，再用 `SymInitialize(PDB 目录, invade=true) + SymFromAddrW` 解析符号。注意 `SYMBOL_INFOW` 的 `MaxNameLen` 在偏移 80、`Name` 在偏移 84，写错会只拿到空名字；本项目的 `target/release/*.pdb` 可匹配已安装的 release 产物。
- **截图预选与窗口检测**：进入覆盖层即预选全屏（虚拟桌面并集）；`screenshotUiDetection = window` 时鼠标移动会把预选切换成光标下窗口（`EnumWindows` 按 z 序取第一个命中，只排除截图覆盖层自身窗口 `OVERLAY_HWND`——本软件的其它窗口（主窗口/贴图）也允许被预选；`DwmGetWindowAttribute(DWMWA_EXTENDED_FRAME_BOUNDS/CLOAKED)` 取真实外框并跳过隐藏窗口，任务栏也算窗口）。选区阶段用位移阈值区分「单击确认预选」与「拖拽自定义」（拖拽松手即确认），预选态不显示工具栏，进入标注才显示。`window_ui` 目前按 `window` 处理，UI 元素识别未实现。
- **条目 id 必须唯一**：不要用毫秒时间戳单独做 id（同一毫秒批量生成会撞车，React `key` 重复会导致列表重复渲染、编辑/删除打错条目）；统一用「时间戳-自增序号」，并在加载时修复历史重复 id。
- **弹窗退场状态要能复位**：带淡出动画的弹窗在提交失败（校验不过、后端拒绝）时必须把 `closing` 复位，否则遮罩停在透明却覆盖全屏的状态，界面表现为「卡死」。
- **禁止对仓库 Rust 文件直接跑 `rustfmt`**：仓库没有 `rustfmt.toml`，默认 100 列会把整文件重排，产生上百行无关噪声；误格式化后立即 `git checkout --` 还原。
- **截图贴图的 OCR 模型随包分发**：模型放 `src-tauri/resources/ocr/ppocr_v6_small/`，`bundle.resources` 映射成安装目录下的 `ocr/`（NSIS 生成脚本里是 `$INSTDIR\ocr\ppocr_v6_small\{det,rec}`）；运行时**只认 exe 同级的 `ocr/`**，release 里不得保留 `CARGO_MANIFEST_DIR` 兜底，否则在开发机上安装测试会“假通过”，掩盖资源放错位置。
- **OCR 内存要按需加载、用完释放**：不要把 30MB 模型 `include_bytes!` 进二进制（映射页会常驻，达不到「识别完回落到无 OCR 基线」的要求）；单工作线程串行识别、每张识别完 drop 引擎并 `SetProcessWorkingSetSize` 归还工作集。
- **改 `settings.json` 一律用 node 或带 BOM 的 UTF-8**：PowerShell 5.1 读写无 BOM 的 UTF-8 会按 ANSI 解码并写入 BOM，导致中文乱码、应用启动读配置失败（开发版 `windowTitle` 与正式版配置各踩过一次）。
- **注入测试注意 `Ctrl+Esc`**：按住 Ctrl 时按 Esc 等于 `Ctrl+Esc`，会弹出开始菜单；模拟取消拖放前先松开 Ctrl。
- **本机网络**：`git` 走代理 `http://127.0.0.1:7897`；代理未启动时推送/gh 均失败，需先让用户开启代理，不要擅自清空代理配置直连。

- **分层窗口（`UpdateLayeredWindow`）显示顺序固定为「先画后显」**：`UpdateLayeredWindow` 在窗口隐藏时也会更新图层内容，所以必须先把新内容画进图层再 `ShowWindow`。反过来（先显示再画）用户会先看到上一次留在图层里的旧画面，表现为「弹出时闪一下上一个内容」。
- **原生浮窗动画的实现口径**：内容指纹不变时只换 `BLENDFUNCTION.SourceConstantAlpha` 重新合成，不重画整张卡片；动画进度一律按**真实时间**（`Instant`）计算，不按 tick 次数累加（否则帧率/窗口尺寸一变，时长就漂）。要顺滑还需把系统计时精度提到 1ms（`timeBeginPeriod(1)`，退出时 `timeEndPeriod(1)`），动画定时器用 8ms；`WM_TIMER` 默认约 15.6ms 且会被合并，直接用会一顿一顿。静态/预览态要显式**不启动动画定时器**且在 tick 里不推进动画——只在 `start()` 把进度置成终值会被下一帧覆盖。
- **窗口显示/隐藏状态用「最新意图 + 单调递增序号」**，不要用「排队消息 + 缓存 `visible`」：显示与隐藏各发一条消息时，先发的隐藏可能后到并作用在刚弹出的窗口上（「瞬间出现又消失」）；缓存状态也会与真实窗口不同步（「该显示不显示」）。做法：命令线程只覆盖一个「待执行意图」槽并递增序号，窗口线程每次唤醒只取最新一条，序号不新就丢弃；显示入口无条件 `ShowWindow`。
- **React 前端两条硬约束**：①`setState` 的 updater（`(current) => ...`）里**不能读合成事件**（`event.currentTarget.value/checked`）——React 延迟执行 updater，那时 `currentTarget` 已置空，会抛 `TypeError` 并让整棵树卸载成白屏；必须先在事件回调里取值。②`useEffect` 的 cleanup **不能「无条件注册 + 在里面读 ref 判断状态」**——状态由 false→true 时会先执行上一次 cleanup，而 ref 已是新值，清理动作会立刻发生（本项目表现为「刚进入定位态就把刚弹出的窗口收掉」）；正确写法是 `if (!state) return;` 只在状态成立时注册 cleanup。
- **前端异常要能看见**：主入口 `src/main.tsx` 有错误边界（页面出错显示「页面出错了」而不是白屏），并把 `window.onerror`/`unhandledrejection`/渲染异常写入 `<存储目录>/logs/frontend.log`（命令 `frontend_error_log`）。排查白屏优先读这个文件。

- **截图覆盖层的重绘口径（踩过两次）**：① 增量重绘的「脏区」必须覆盖**上一帧实际画过的范围**，不能只记"按当前状态算出来的框"——否则强制整屏重绘那一帧多画的部分没人擦，会留下块状残留；② 拖拽/移动期间不要拿"预选矩形"当亮块（预选可能是全屏），亮块只能是当前拖拽框/选区；③ **同一帧不要分两次呈现到屏幕**：先呈现辅助线带、再呈现内容区，中间态会表现为辅助线闪烁；要么逐块"合成+呈现"一次到位（当前实现），要么先全部合成进后备缓冲、帧末统一呈现（改动前需先抽象「内容区/亮区」，否则会擦出黑线）。

## 分支、提交与发布

- 主分支 `master`，远程 `origin`（https://github.com/THE2580/LightweightToolset.git）。当前为单人开发，提交直接落在 `master`；是否引入功能分支/PR 待确认。
- 提交信息沿用 Conventional Commits：`feat:`、`fix:`、`chore:`、`docs:`、`refactor:`，发布提交在末尾追加版本号（如 `fix: release startup initialization v0.4.4`）。
- 提交前必须：`git status --short` 核对仅包含预期文件；`npm run build`；`cargo check`；涉及 Rust 逻辑时跑串行单测。
- 版本规则：新增工具调整第一/二位版本号；缺陷修复或既有工具扩展调整第三位。
- 发布 = 带注释 tag + GitHub Release + 安装包 SHA-256 记录，须用户明确授权；当前最新为 `v0.7.2`。
- **发版时必须同步更新 README 与项目简介（固定动作，不是可选项）**：每次发版都要把对外说明更新到当版，不允许只发代码、文档滞后。
  1. **README**：版本号、功能列表（新增 / 变更 / 移除）、下载与安装说明；过期的截图、命令或描述一并修正。
  2. **项目简介**：GitHub 仓库 About 描述随版本与功能更新（`gh repo edit --description "..."`）。
  3. 二者与 tag/Release 同批确认；发版完成后核对仓库首页展示的是当版信息。
- **Release 资产名必须用 ASCII 英文名（固定动作）**：统一为 `LightweightToolset_<版本>_x64-setup.exe`。本地构建产物由 `productName` 生成中文名属正常，**上传前复制成英文名副本**再上传；禁止直接上传中文文件名（实测会被截断成 `_0.6.x_x64-setup.exe`，0.6.2–0.6.4 已踩过并修正）。上传后必须用 `gh release view` 或 `gh api` 核对资产名完整，发现截断立即用 `gh api -X PATCH .../releases/assets/<id> -f name=...` 修正。README 的下载说明也必须引用英文资产名。
- **发版前必须清理开发版痕迹（固定检查项，不是可选项）**：每次发布/推送正式版之前，删除开发版产生的全部数据与缓存，确认发布环境里没有开发版残留。
  1. 先结束仍在运行的开发版（`target/debug` 或 `%TEMP%\lwt-dev` 里的那份），不要动安装版进程。
  2. 若开发版设置过自定义存储目录，先读 `%APPDATA%\com.the.lightweight-toolset.dev\settings.json` 的 `storagePath`，一并清理。
  3. 删除开发版配置/数据、WebView2 缓存、开发版快捷方式与临时运行目录：

```powershell
Remove-Item -Recurse -Force "$env:APPDATA\com.the.lightweight-toolset.dev" -ErrorAction SilentlyContinue
Remove-Item -Recurse -Force "$env:LOCALAPPDATA\com.the.lightweight-toolset.dev" -ErrorAction SilentlyContinue
Remove-Item -Force "$env:APPDATA\Microsoft\Windows\Start Menu\Programs\轻量化工具集 (开发版).lnk" -ErrorAction SilentlyContinue
Remove-Item -Recurse -Force "$env:TEMP\lwt-dev" -ErrorAction SilentlyContinue
```

  4. 不得清理正式版目录 `%APPDATA%\com.the.lightweight-toolset`、`%LOCALAPPDATA%\com.the.lightweight-toolset` 与用户的自定义存储目录；`LightweightToolset_Verify_DSH` 这类验证用计划任务也应确认已删除。
- **发版完工后必须做一次沉淀（固定动作，不等用户提醒）**：确认发布完成后，① 通读本次改动，把可复用的经验、新约束、已排除方案补进本文件「平台约束与踩坑经验」与「开发版与正式版」；② 刷新 `agent_memory` 五件套（`context` 发布索引、`progress` 里程碑、`bugs` 风险与经验、`working_state` 现场、`checkpoint` 恢复快照），过时内容压缩进 `archive/`；③ 核对全库是否残留「未发版 / 旧版本号 / 已删除模块」等过时描述。
- 推送前确认本地工作树无未预期的其他改动。

## 验证方式

- **改动完成后主动启动开发版（固定动作，不需用户再提）**：每次交付可运行的改动后，先确认 `npm run dev`（Vite，端口 1420）在跑，再把 `src-tauri/target/debug/lightweight-toolset.exe` 复制到 `%TEMP%\lwt-dev\`，用 `Start-Process -Verb RunAs` 启动，并在汇报里写清本次测试点与快捷键。纯文档、纯配置或不影响运行时的改动可跳过，但要在汇报里说明原因。
- **启动开发版必须逐项自检（固定动作，禁止盲测）**：每次启动开发版前后都要核对：① `netstat` 确认 1420 在监听，并实际请求 `http://localhost:1420` 返回 200；② 结束所有 `%TEMP%\lwt-dev` 旧实例，确认 exe 复制成功（出现 Device busy 说明旧进程未退，不得继续）；③ 启动后核对进程路径确实来自 `%TEMP%\lwt-dev`；④ 确认窗口能正常加载页面——页面空白时先查 Vite/WebView，不得继续做热键或功能测试；⑤ 截图测试使用实际注册成功的热键；为排查临时改过热键，必须改回并重启；⑥ 不得凭“可能被安装版占用”臆测热键失效，要以实际注册结果/日志为准。

- 前端/Build：`npm run build` 无 TS 错误与 Vite 构建错误。
- 快捷键入口改动：确认全局设置不再列出该工具、工具页/设置页内能录制、失焦自动保存、删除可用、冲突提示正常。
- 截图预选/窗口检测改动：覆盖层进入即全屏预选；`仅窗口` 下移动鼠标预选跟随窗口（含任务栏）；单击预选区域进入标注、拖拽松手进入标注；`不检测` 时不跟随窗口；`窗口和UI` 置灰不可选。
- Rust：`cargo check` 通过；改动模块的单测通过，全量用 `--test-threads=1`（`input_monitor` 等测试共享全局按键状态，并行会偶发失败）。
- 运行时/UI 变更：需在 Windows 真机验证；应用为管理员权限，测试高完整性窗口按键时注意同 App ID 的已安装版干扰（开发版已隔离标识，两者可同时运行）。
- 开机自启改动：需在正式版上真实验证一次「注销/重启 → 登录后自动启动」；可用 `schtasks /Query /TN LightweightToolset /XML` 核对 `HighestAvailable`、`InteractiveToken`、`ExecutionTimeLimit=PT0S`。注意运行中的安装版不要随意结束进程。
- 开发版默认值改动：清空开发版数据目录后启动，核对 `settings.json` 中 `silentStart` 为 false、`mainWindowAlwaysOnTop` / `developerMode` 为 true、`autoStart` 为 false、`tools` 全部 false、`storagePath` 为空；窗口应正常显示、标题带「（开发版）」、托盘悬停为中文名。
- 静默启动改动：分别在「关闭/开启静默启动」下重启，确认窗口显示/隐藏正确，托盘、主窗口快捷键、再次启动三种方式都能唤起窗口，启动通知文案与当前快捷键一致。
- 冷启动类改动：以 release 构建多次冷启动统计为准（正常退出或留足资源释放间隔），不以开发热重启为样本。

## 项目记忆

- `agent_memory/context.md` 长期背景与约束；`progress.md` 阶段与待办；`bugs.md` 风险与修复线索；`working_state.md` 当前现场；`checkpoint.md` 恢复快照；`archive/` 旧摘要。
- 非简单任务开始前先读 `context.md`、`progress.md`、`bugs.md`、`working_state.md`；恢复/切线程时加读 `checkpoint.md`。
- 长任务、阶段切换、阻塞或会话结束前刷新 `progress.md`、`bugs.md`、`working_state.md`，必要时更新 `checkpoint.md`。记忆只保留当前有效信息，旧内容压缩进 `archive/`。
