# 当前现场

- 2026-11-27：待办 1（窗口置顶启用被快捷键注册失败阻塞）**已修，待用户验证**：工具热键注册失败不再中断启用（改发 `tool-hotkey-warning` 轻提示），`resume_shortcuts` 改为逐条尽力恢复。实测 `Ctrl+Alt+T` 仍被占用（1409，大概率安装版 v0.7.2），启用后会提示快捷键不可用。
- 2026-11-27：批次 B「快捷键提示的触发类型」**已实现并通过验收**：自动识别键形态（无类型按钮）；补齐 CapsLock/NumLock/ScrollLock/PrintScreen/Pause 与 OEM 符号键（钩子/后端/页面三处命名表）；鼠标钩子支持侧键提示与录制回传。
- 2026-11-27：批次 A「非录制阶段快捷键冲突提示」**已实现并通过验收**：快照新增 `hotkeyError`（配置冲突优先，其次注册失败），按用户要求改为**键值框红色边框**（不再加额外图标），非录制态生效；修复“改键/清空后仍显示”的陈旧数据问题（前端 null 兜底 + 空键值双重兜底）。
- 2026-11-26：**快捷键「触发类型」已完成并通过用户验收**（未提交）。全局热键（组合键/单个 F1–F24）+ 新增「键鼠监听」类型（键盘单键/鼠标侧键，低级钩子不吞键、按下即触发）；单按钮切换类型（已有键值先确认清空）、跨类型冲突检测、键被占用友好提示、鼠标侧键录制通道、命名键与方向键录制修复均已落地。细节见 `progress.md`。
- 2026-11-23：`v0.8.3` **已发布**（`86381f9`）：提示窗位置越界夹回、位置不再写死、重置清回默认、滑块不再显示 0,0。开发环境已关闭。
- 2026-11-23：`v0.8.2` **已发布**（`b778fa4`）：修复静默启动后功能全失效的回归（运行时初始化改由后端 setup 触发）。开发环境已关闭。
- 2026-11-23：`v0.8.1` **已发布**（提交 `1361100`、tag 已推、Release 已建、About 已更新）：主窗口按需创建 + 关窗即销毁，托盘态 ≈10MB。开发环境已关闭。
- 2026-11-22：`v0.8.0` **已发布**（发布提交 `8d83b88`、tag `v0.8.0` 已推、Release 已建、仓库 About 已更新）；开发环境已关闭（开发版进程、Vite 1420、开发版数据目录、`%TEMP%\lwt-dev` 全部清理）。
- 安装版 `v0.7.2` 仍在 `D:\轻量化工具集\` 运行（本次未动它）。
- 工作树：新增 `src-tauri/src/hotkey_listener.rs`；改动 `tools.rs` / `lib.rs` / `settings.rs` / `App.tsx` / `App.css`；`window_pinner.rs` 换行差异与 `.codex/` 保持不动。
- 2026-11-27 下午：**开发环境已重启**（Vite 1420 监听 + `%TEMP%/lwt-dev` 最新 debug 构建，管理员启动，窗口正常渲染）；开发版数据保留；安装版 v0.7.2 同时运行、未动。
- 验收后又发现两个待办（见 `progress.md` 顶部）：① 窗口置顶启用被快捷键注册失败阻塞；② 快捷键提示「新增提示」录制框没有触发类型（口径待确认）。

# Scope

- 工具集：截图与贴图、剪贴板、软件使用统计、按键使用统计、计时器、窗口置顶、快捷键提示（共 7 个）。
- 本阶段：`v0.8.3` 已发布；「快捷键触发类型」已验收但未提交；两个新待办见 `progress.md` 顶部（窗口置顶启用阻塞、快捷键提示的类型切换）。
- 暂不修：截图覆盖层开启辅助线时的拖拽掉帧（方案见 `progress.md`）。

# Git / Working Tree

- 分支 `master`；最新提交 `bc73579 docs: queue hotkey trigger-type extension...`；已推送 `origin/master`。
- 最新 tag `v0.8.3`；Release https://github.com/THE2580/LightweightToolset/releases/tag/v0.8.3 。
- 安装包 `LightweightToolset_0.8.3_x64-setup.exe`：33,891,232 字节，SHA-256 `14fb2e62…`。
- 未提交改动：`src-tauri/src/{hotkey_listener.rs(新),lib.rs,tools.rs,settings.rs}`、`src/{App.tsx,App.css}`、`agent_memory/*`。

# Active Files

- `src-tauri/src/hotkey_listener.rs`：键鼠监听服务（低级键盘/鼠标钩子线程、路由表、去抖、鼠标侧键录制通道）。
- `src-tauri/src/tools.rs`：快捷键注册/注销按类型分流、触发类型存储、跨类型冲突、显示格式化。
- `src-tauri/src/lib.rs`：`dispatch_hotkey` / `dispatch_hotkey_id` 派发层、`set_tool_hotkey_kind` / `begin_hotkey_capture` / `end_hotkey_capture` 命令。
- `src-tauri/src/settings.rs`：`AppSettings.hotkey_kinds`。
- `src/App.tsx`：`HotkeyRowEditor` 触发类型切换、确认框、按类型录制、鼠标侧键事件订阅。
- `AGENTS.md` / `agent_memory/*`：约束与记忆。

# Verification

- `cargo test --lib -- --test-threads=1`（TEMP/TMP 指向 `src-tauri/target/test-tmp`）→ **64 passed / 3 ignored**。
- `npm run build` 通过；`cargo check` 仅 4 条既有 warning（`cargo build` 同理）。
- 开发版：Vite 1420 返回 200、exe 已复制到开发临时目录并管理员启动、窗口正常渲染（截图核对）。
- 用户已验收（2026-11-26）：「快捷键触发类型」全部交互与监听触发通过；验收中修复的两处 bug（命名键录制、方向键被导航吞）也已复验通过。
- 2026-11-27：待办 1 修完后重新跑了 `cargo test --lib`（64 passed / 3 ignored）、`npm run build`、`cargo build` 并重启开发版；该修复待用户点「窗口置顶」验证。
- 未提交（等用户明确要求）：`src-tauri/src/{hotkey_listener.rs(新),lib.rs,tools.rs,settings.rs}`、`src/{App.tsx,App.css}`、`agent_memory/*`。

# Blockers

- 无。

# Next Action

1. 等用户验证待办 1：启用「窗口置顶」应成功打开（默认 `Ctrl+Alt+T` 被安装版占用时只弹轻提示，不再红条挡住）。
2. 待办 2（快捷键提示的类型切换）口径确认后再动手。
3. 全部完成后，用户明确要求才按 Conventional Commits 提交/推送。
4. 若监听型触发异常，先看设置 → 日志里的 `hotkey.listen.registered` / `tool.hotkey.triggered id=` 条目。