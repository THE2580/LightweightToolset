# AGENTS.md

## 项目目标

LightweightToolset 是基于 Tauri 2 + Rust + React + TypeScript 的 Windows 轻量化桌面工具集：固定主窗口、单实例、单托盘，工具编译期注册、可独立启用/停用，所有数据本地存储。现有工具：剪贴板、软件使用统计、按键使用统计、计时器、窗口置顶、快捷键提示。当前稳定版 `v0.6.0`。旧 Electron 版 LightweightWindowsToolset 只作迁移参考，新功能不回写。

## 技术栈

- 前端：React 19、TypeScript 5.8（`strict`、`noUnusedLocals`、`noUnusedParameters`）、Vite 7、framer-motion、lucide-react。
- 后端：Tauri 2、Rust 2021，直接调用 `windows` / `windows-sys` 的 Win32 API。
- 打包：NSIS（`src-tauri/target/release/bundle/nsis/`）。
- 本机工具链：Node 24.12、cargo/rustc 1.96；npm 在 `E:\devtools\node\v24.12.0\`（Git Bash 的 PATH 里可能没有，必要时用 PowerShell 或绝对路径）。

## 目录结构

- `src/`：React 前端。`App.tsx` 为主窗口（体量大，优先局部修改）；`popup.tsx` / `free.tsx` 为快捷弹窗与自由窗口；`notification.tsx` 为快捷键提示浮窗页；`shortcutNotify.ts` 是页面按键上报桥；样式集中在 `App.css`、`popup.css`、`free.css`、`notification.css`。
- `src-tauri/src/`：Rust 后端。`lib.rs` 为应用装配与命令入口；`main.rs` 兼作采键子进程入口（`--shortcut-tip-helper`）；`tools.rs` 工具注册与生命周期；`clipboard.rs`、`shortcut_tip.rs`、`app_usage.rs`、`key_usage.rs`、`input_monitor.rs`、`timer.rs`、`window_pinner.rs`、`window_service.rs`、`settings.rs` 按功能拆分。
- `src-tauri/capabilities/default.json`：Tauri 权限清单；`src-tauri/tauri.conf.json`：窗口/打包配置。
- `agent_memory/`：项目记忆（权威状态源，随仓库提交）；`archive/` 存旧摘要。

## 常用命令

```powershell
npm install                          # 首次/依赖变更后
npm run dev                          # 仅前端 Vite
npm run tauri dev                    # 开发版（含 Rust）
npm run build                        # tsc + vite build（前端类型与产物验证）
npm run tauri build                  # release + NSIS 安装包
cargo check --manifest-path src-tauri/Cargo.toml
cargo test --lib --manifest-path src-tauri/Cargo.toml -- --test-threads=1
```

全仓 `cargo fmt --check` 与严格 clippy 目前有既有告警/格式问题，不作为准入条件；也不要对仓库 Rust 文件直接跑 `rustfmt`（原因见下方平台约束），按既有行宽手工对齐即可。

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
- 主程序以管理员权限运行（`requireAdministrator` 清单 `src-tauri/windows-app-manifest.xml`）；按键捕获依赖进程内低级键盘钩子，覆盖同级及更低完整性窗口，UAC 安全桌面与 System 完整性进程不可见。每次启动会弹 UAC，开机自启需接受该体验。
- 不绕过 DSH 沙箱或审批；权限不足时说明并等待用户处理。
- 保留首次 release 构建产物；`src-tauri/target` 膨胀时只清理可再生的 `target/debug`，禁止 `cargo clean`（会连带删除 release）。

## 平台约束与踩坑经验（必须遵守）

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
- 发布 = 带注释 tag + GitHub Release + 安装包 SHA-256 记录，须用户明确授权；当前最新为 `v0.6.0`。
- **发版完工后必须做一次沉淀（固定动作，不等用户提醒）**：确认发布完成后，① 通读本次改动，把可复用的经验、新约束、已排除方案补进本文件「平台约束与踩坑经验」；② 刷新 `agent_memory` 五件套（`context` 发布索引、`progress` 里程碑、`bugs` 风险与经验、`working_state` 现场、`checkpoint` 恢复快照），过时内容压缩进 `archive/`；③ 核对全库是否残留「未发版 / 旧版本号 / 已删除模块」等过时描述。
- 推送前确认本地工作树无未预期的其他改动。

## 验证方式

- 前端/Build：`npm run build` 无 TS 错误与 Vite 构建错误。
- Rust：`cargo check` 通过；改动模块的单测通过，全量用 `--test-threads=1`（`input_monitor` 等测试共享全局按键状态，并行会偶发失败）。
- 运行时/UI 变更：需在 Windows 真机验证；应用为管理员权限，测试高完整性窗口按键时注意同 App ID 的已安装版干扰。
- 冷启动类改动：以 release 构建多次冷启动统计为准（正常退出或留足资源释放间隔），不以开发热重启为样本。

## 项目记忆

- `agent_memory/context.md` 长期背景与约束；`progress.md` 阶段与待办；`bugs.md` 风险与修复线索；`working_state.md` 当前现场；`checkpoint.md` 恢复快照；`archive/` 旧摘要。
- 非简单任务开始前先读 `context.md`、`progress.md`、`bugs.md`、`working_state.md`；恢复/切线程时加读 `checkpoint.md`。
- 长任务、阶段切换、阻塞或会话结束前刷新 `progress.md`、`bugs.md`、`working_state.md`，必要时更新 `checkpoint.md`。记忆只保留当前有效信息，旧内容压缩进 `archive/`。
