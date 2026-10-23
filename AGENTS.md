# AGENTS.md

## 项目目标

LightweightToolset 是基于 Tauri 2 + Rust + React + TypeScript 的 Windows 轻量化桌面工具集：固定主窗口、单实例、单托盘，工具编译期注册、可独立启用/停用，所有数据本地存储。现有工具：剪贴板、软件使用统计、按键使用统计、计时器、窗口置顶。当前稳定版 `v0.4.4`（`7f9d66d`）。旧 Electron 版 LightweightWindowsToolset 只作迁移参考，新功能不回写。

## 技术栈

- 前端：React 19、TypeScript 5.8（`strict`、`noUnusedLocals`、`noUnusedParameters`）、Vite 7、framer-motion、lucide-react。
- 后端：Tauri 2、Rust 2021，直接调用 `windows` / `windows-sys` 的 Win32 API。
- 打包：NSIS（`src-tauri/target/release/bundle/nsis/`）。
- 本机工具链：Node 24.12、cargo/rustc 1.96；npm 在 `E:\devtools\node\v24.12.0\`（Git Bash 的 PATH 里可能没有，必要时用 PowerShell 或绝对路径）。

## 目录结构

- `src/`：React 前端。`App.tsx` 为主窗口（体量大，优先局部修改）；`popup.tsx` / `free.tsx` 为快捷弹窗与自由窗口；样式集中在 `App.css`、`popup.css`、`free.css`。
- `src-tauri/src/`：Rust 后端。`lib.rs` 为应用装配与命令入口；`tools.rs` 工具注册与生命周期；`clipboard.rs`、`app_usage.rs`、`key_usage.rs`、`input_monitor.rs`、`timer.rs`、`window_pinner.rs`、`window_service.rs`、`settings.rs` 按功能拆分。
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

全仓 `cargo fmt --check` 与严格 clippy 目前有既有告警/格式问题，不作为准入条件；改动的 Rust 文件用 `rustfmt <file>` 局部处理，不要顺带全仓格式化。

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
- 主程序保持 Medium 权限；按键统计仅在启用时按需启动最小 High 输入 helper，停用/退出/IPC 断开必须退出。
- 不绕过 DSH 沙箱或审批；权限不足时说明并等待用户处理。
- 保留首次 release 构建产物；`src-tauri/target` 膨胀时只清理可再生的 `target/debug`，禁止 `cargo clean`（会连带删除 release）。

## 分支、提交与发布

- 主分支 `master`，远程 `origin`（https://github.com/THE2580/LightweightToolset.git）。当前为单人开发，提交直接落在 `master`；是否引入功能分支/PR 待确认。
- 提交信息沿用 Conventional Commits：`feat:`、`fix:`、`chore:`、`docs:`、`refactor:`，发布提交在末尾追加版本号（如 `fix: release startup initialization v0.4.4`）。
- 提交前必须：`git status --short` 核对仅包含预期文件；`npm run build`；`cargo check`；涉及 Rust 逻辑时跑串行单测。
- 版本规则：新增工具调整第一/二位版本号；缺陷修复或既有工具扩展调整第三位。
- 发布 = 带注释 tag + GitHub Release + 安装包 SHA-256 记录，须用户明确授权；当前最新为 `v0.4.4`。
- 推送前确认本地工作树无未预期的其他改动。

## 验证方式

- 前端/Build：`npm run build` 无 TS 错误与 Vite 构建错误。
- Rust：`cargo check` 通过；改动模块的单测通过，全量用 `--test-threads=1`（`input_monitor` 等测试共享全局按键状态，并行会偶发失败）。
- 运行时/UI 变更：需在 Windows 真机验证；按键统计涉及管理员 helper 时，核对进程完整路径，避免同 App ID 的已安装版干扰。
- 冷启动类改动：以 release 构建多次冷启动统计为准（正常退出或留足资源释放间隔），不以开发热重启为样本。

## 项目记忆

- `agent_memory/context.md` 长期背景与约束；`progress.md` 阶段与待办；`bugs.md` 风险与修复线索；`working_state.md` 当前现场；`checkpoint.md` 恢复快照；`archive/` 旧摘要。
- 非简单任务开始前先读 `context.md`、`progress.md`、`bugs.md`、`working_state.md`；恢复/切线程时加读 `checkpoint.md`。
- 长任务、阶段切换、阻塞或会话结束前刷新 `progress.md`、`bugs.md`、`working_state.md`，必要时更新 `checkpoint.md`。记忆只保留当前有效信息，旧内容压缩进 `archive/`。
