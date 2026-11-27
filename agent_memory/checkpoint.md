# Goal

LightweightToolset 持续迭代（Tauri 2 + Rust + React）：七个工具已上线，稳定版 `v0.8.3`；「快捷键触发类型」扩展已通过用户验收（未提交）。

# Current Focus（2026-11-27）

- **快捷键触发类型**已实现并验收（未提交）：`global`（组合键 / 单个 F1–F24，`RegisterHotKey`）与新增 `listen`（任意键盘单键 + 鼠标侧键，`WH_KEYBOARD_LL` / `WH_MOUSE_LL`，不吞键、按下即触发、250ms 去抖、忽略注入事件）。
- **交互**：类型用**单个切换按钮**显式切换（显示当前类型，仅编辑态可点；非编辑态静态置灰、无 hover）；已有键值时先弹标准确认框（说明清空、整块遮罩与按钮 `onMouseDown preventDefault` 保证录制框不失焦），确认后切类型 + 清空并保持录制态；录制框空态文案统一「按下快捷键」。
- **其它**：跨类型冲突检测（全局 `F12` ↔ 监听 `F12`）、键被占用友好文案、鼠标侧键录制通道（`begin/end_hotkey_capture` + `hotkey-captured` 事件，录制期吞 XBUTTON 防导航，120s 兜底）。
- **2026-11-27 修复（已验证）**：工具热键注册失败不再阻止工具启用（`tool-hotkey-warning` 轻提示）；`resume_shortcuts` 改为逐条尽力恢复。
- **2026-11-27 批次 B（已验收）**：快捷键提示**自动识别键形态**（无类型按钮）+ 鼠标侧键 `Mouse4/5` 录制与触发；补齐 CapsLock 等命名键与 OEM 符号键；提示只监听不注册。
- **2026-11-27 批次 A（已验收）**：`ToolSnapshot.hotkeyError` + **键值框红色边框**（仅非录制态；配置冲突优先，其次注册失败；成功/清空/切类型/停用即清除；已修“改键/清空后仍显示”的陈旧数据问题）。开发环境在跑（Vite 1420 + `%TEMP%/lwt-dev`），安装版 v0.7.2 同时运行（占用 `Ctrl+Alt+T` / `Ctrl+Alt+M`，属双版本共存预期）。

# Remaining

- **待办 1（已修，已验证通过）**：窗口置顶启用不再被快捷键注册失败阻塞。
- ~~批次 A~~ **已验收**：非录制阶段快捷键边框红色态（虚线、仅被占用/配置冲突时显示；不加额外图标）。
- ~~批次 B~~ **已验收**：快捷键提示自动识别键形态（无类型按钮）；补齐 CapsLock 等命名键与 OEM 符号键；鼠标侧键可录制并触发提示。
- 用户要求时按 Conventional Commits 提交（建议 `feat: add hotkey trigger types with key/mouse listening`）。
- 历史候选见 `progress.md`（辅助线掉帧、标注拖动/文本再编辑、拖拽缩略图观感、贴图历史浏览）。

# Verification

- `cargo test --lib -- --test-threads=1`（TEMP/TMP 指向 `src-tauri/target/test-tmp`）→ 64 passed / 3 ignored。
- `npm run build` 通过；`cargo check` 仅 4 条既有 warning。
- 开发版自检：1420 监听且 HTTP 200；exe 来自开发临时目录；窗口正常渲染（截图核对）；不动安装版进程。

# Resume Here

1. 读 `agent_memory/working_state.md` 与 `progress.md` 顶部恢复现场（开发环境在跑：Vite 1420 + `%TEMP%/lwt-dev`）。
2. 批次 B 等用户验证；有问题按反馈改（提示类型/侧键提示/旧数据迁移）。
3. 若监听型不触发：让用户在「设置 → 日志」看 `hotkey.listen.registered` 与 `tool.hotkey.triggered id=`。
4. 全部完成后按用户要求提交（`git status --short` 核对不含 `window_pinner.rs` 换行差异与 `.codex/`）。
