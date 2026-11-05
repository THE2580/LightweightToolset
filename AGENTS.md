# AGENTS.md

## 项目目标

LightweightToolset 是基于 Tauri 2 + Rust + React + TypeScript 的 Windows 轻量化桌面工具集：固定主窗口、单实例、单托盘，工具编译期注册、可独立启用/停用，所有数据本地存储。现有工具：剪贴板、软件使用统计、按键使用统计、计时器、窗口置顶、快捷键提示。当前稳定版 `v0.6.3`。旧 Electron 版 LightweightWindowsToolset 只作迁移参考，新功能不回写。

## 技术栈

- 前端：React 19、TypeScript 5.8（`strict`、`noUnusedLocals`、`noUnusedParameters`）、Vite 7、framer-motion、lucide-react。
- 后端：Tauri 2、Rust 2021，直接调用 `windows` / `windows-sys` 的 Win32 API。
- 打包：NSIS（`src-tauri/target/release/bundle/nsis/`）。
- 本机工具链：Node 24.12、cargo/rustc 1.96；npm 在 `E:\devtools\node\v24.12.0\`（Git Bash 的 PATH 里可能没有，必要时用 PowerShell 或绝对路径）。

## 目录结构

- `src/`：React 前端。`App.tsx` 为主窗口（体量大，优先局部修改）；`popup.tsx` / `free.tsx` 为快捷弹窗与自由窗口；`notification.tsx` 为快捷键提示浮窗页；`shortcutNotify.ts` 是页面按键上报桥；样式集中在 `App.css`、`popup.css`、`free.css`、`notification.css`。
- `src-tauri/src/`：Rust 后端。`lib.rs` 为应用装配与命令入口；`main.rs` 兼作采键子进程入口（`--shortcut-tip-helper`）；`autostart.rs` 开机自启（计划任务）；`tools.rs` 工具注册与生命周期；`clipboard.rs`、`shortcut_tip.rs`、`app_usage.rs`、`key_usage.rs`、`input_monitor.rs`、`timer.rs`、`window_pinner.rs`、`window_service.rs`、`windows_notification.rs`、`settings.rs` 按功能拆分。
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
- Rust 错误统一转 `String` 返回给前端；日志走 `push_debug_log`，不要 `println!` 留生产噪声。

## 硬性约束（禁止事项）

- 未经用户明确要求：不提交、不推送、不发布、不主动启动开发版。
- 不提交 `node_modules/`、`dist/`、`src-tauri/target/`、QA 截图、日志或缓存；`agent_memory/` 必须入库，不要加回 `.gitignore`。
- 不恢复用户主动删除的内容；不修改与本任务无关的既有工作树现场（如 `.codex/`、`window_service.rs` 的格式化改动）。
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
| 静默启动 | 默认关闭，可开关 | 默认**开启**，可开关 |
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
- **版本号单一来源**：前端版本号从 `../package.json` 直接 `import`（`src/App.tsx` 的 `APP_VERSION`）。禁止用 Vite `define` 注入全局常量——打包时会替换、dev 服务器不替换，会直接白屏。`package.json` / `src-tauri/Cargo.toml` / `src-tauri/tauri.conf.json` 三处版本号发版时同步修改。
- **采键通道与 Chromium 限制**：Chromium/WebView2 窗口在前台时，Windows 会间断地不给 `WH_KEYBOARD_LL` 派发按键；被系统或其他软件注册走的热键（`Win+D`、`Win+I`、`Win+Shift+S`、他软件全局热键）连 Raw Input 都收不到。现有的三条通道缺一不可：① 软件自身窗口的普通按键走页面 `keydown`（`src/shortcutNotify.ts` → `shortcut_tip_report`）；② 焦点在别处时走进程内钩子；③ 系统/他软件热键走独立采键子进程——复用同一个 exe，带 `--shortcut-tip-helper <port>` 启动，不初始化 Tauri/WebView（分叉要在进入 Tauri 之前），装钩子后经 `127.0.0.1` 回环 TCP 回传组合键；子进程继承提权、不加 UAC，连接断开或回传失败必须自行退出，工具停用时主程序负责结束它。三条通道共用节流与去重。
- **已排除方案：Raw Input 兜底**。实测 Raw Input（`RIDEV_INPUTSINK`）收得到普通键，但收不到被系统/他软件注册走的 Win 组合键；A/B 对照（禁用/启用）现象完全一致。不要再为「系统热键收不到」接 Raw Input。
- **浮窗提示节奏**：同一来源重复上报按 `0.5s` 节流（按住快捷键自动重复时逐 0.5s 累加），跨来源按 `400ms` 去重；应用内触发一律**立即**提示，不等剪贴板确认；松手后累加必须立即停止，不允许积压成队列。
- **浮窗尺寸与层级**：不要按字符估算宽度再动态改窗口（后续每次显示都会用估算值覆盖实测值，表现为闪一下又变窄）；用固定宽窗口 + 卡片 `width: max-content` 居中。只设一次 `always_on_top` 压不过同样置顶的窗口，显示时与显示期间（每 400ms）都要 `SetWindowPos(HWND_TOPMOST)` 重新压顶。
- **锁顺序与死锁**：`AppState.registry` 与工具自有存储锁不能嵌套持有；快照类接口必须「先拷贝数据、释放存储锁，再查注册表」。快捷键回调运行在主线程，**禁止**在其中做加锁 + 窗口操作，一律丢到独立线程，否则会与前端 2 秒轮询的快照形成环形等待、整窗卡死。
- **录制状态只跟录制框焦点绑定**：录制框聚焦才接管按键（钩子吞键 + 全局热键暂停），失焦立刻恢复。若绑定在「弹窗打开」上，在该弹窗的文案框打字会被当成录制内容回填。
- **条目 id 必须唯一**：不要用毫秒时间戳单独做 id（同一毫秒批量生成会撞车，React `key` 重复会导致列表重复渲染、编辑/删除打错条目）；统一用「时间戳-自增序号」，并在加载时修复历史重复 id。
- **弹窗退场状态要能复位**：带淡出动画的弹窗在提交失败（校验不过、后端拒绝）时必须把 `closing` 复位，否则遮罩停在透明却覆盖全屏的状态，界面表现为「卡死」。
- **禁止对仓库 Rust 文件直接跑 `rustfmt`**：仓库没有 `rustfmt.toml`，默认 100 列会把整文件重排，产生上百行无关噪声；误格式化后立即 `git checkout --` 还原。
- **本机网络**：`git` 走代理 `http://127.0.0.1:7897`；代理未启动时推送/gh 均失败，需先让用户开启代理，不要擅自清空代理配置直连。

## 分支、提交与发布

- 主分支 `master`，远程 `origin`（https://github.com/THE2580/LightweightToolset.git）。当前为单人开发，提交直接落在 `master`；是否引入功能分支/PR 待确认。
- 提交信息沿用 Conventional Commits：`feat:`、`fix:`、`chore:`、`docs:`、`refactor:`，发布提交在末尾追加版本号（如 `fix: release startup initialization v0.4.4`）。
- 提交前必须：`git status --short` 核对仅包含预期文件；`npm run build`；`cargo check`；涉及 Rust 逻辑时跑串行单测。
- 版本规则：新增工具调整第一/二位版本号；缺陷修复或既有工具扩展调整第三位。
- 发布 = 带注释 tag + GitHub Release + 安装包 SHA-256 记录，须用户明确授权；当前最新为 `v0.6.3`。
- **发版时必须同步更新 README 与项目简介（固定动作，不是可选项）**：每次发版都要把对外说明更新到当版，不允许只发代码、文档滞后。
  1. **README**：版本号、功能列表（新增 / 变更 / 移除）、下载与安装说明；过期的截图、命令或描述一并修正。
  2. **项目简介**：GitHub 仓库 About 描述随版本与功能更新（`gh repo edit --description "..."`）。
  3. 二者与 tag/Release 同批确认；发版完成后核对仓库首页展示的是当版信息。
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

- 前端/Build：`npm run build` 无 TS 错误与 Vite 构建错误。
- Rust：`cargo check` 通过；改动模块的单测通过，全量用 `--test-threads=1`（`input_monitor` 等测试共享全局按键状态，并行会偶发失败）。
- 运行时/UI 变更：需在 Windows 真机验证；应用为管理员权限，测试高完整性窗口按键时注意同 App ID 的已安装版干扰（开发版已隔离标识，两者可同时运行）。
- 开机自启改动：需在正式版上真实验证一次「注销/重启 → 登录后自动启动」；可用 `schtasks /Query /TN LightweightToolset /XML` 核对 `HighestAvailable`、`InteractiveToken`、`ExecutionTimeLimit=PT0S`。注意运行中的安装版不要随意结束进程。
- 开发版默认值改动：清空开发版数据目录后启动，核对 `settings.json` 中 `silentStart` / `mainWindowAlwaysOnTop` / `developerMode` 为 true、`autoStart` 为 false、`tools` 全部 false、`storagePath` 为空；窗口应隐藏、标题带「（开发版）」、托盘悬停为中文名。
- 静默启动改动：分别在「关闭/开启静默启动」下重启，确认窗口显示/隐藏正确，托盘、主窗口快捷键、再次启动三种方式都能唤起窗口，启动通知文案与当前快捷键一致。
- 冷启动类改动：以 release 构建多次冷启动统计为准（正常退出或留足资源释放间隔），不以开发热重启为样本。

## 项目记忆

- `agent_memory/context.md` 长期背景与约束；`progress.md` 阶段与待办；`bugs.md` 风险与修复线索；`working_state.md` 当前现场；`checkpoint.md` 恢复快照；`archive/` 旧摘要。
- 非简单任务开始前先读 `context.md`、`progress.md`、`bugs.md`、`working_state.md`；恢复/切线程时加读 `checkpoint.md`。
- 长任务、阶段切换、阻塞或会话结束前刷新 `progress.md`、`bugs.md`、`working_state.md`，必要时更新 `checkpoint.md`。记忆只保留当前有效信息，旧内容压缩进 `archive/`。
