# 当前现场

- 2026-11-27：**`v0.8.4` 已发布**（提交 `5c3986d`、tag 已推、Release 已建、仓库 About 已更新）：快捷键触发类型、工具启用不被快捷键占用阻塞、非录制阶段红色虚线边框、快捷键提示自动识别 + 鼠标侧键 + 命名键补齐。
- **开发环境已关闭**：开发版进程、Vite 1420、`%TEMP%/lwt-dev`、开发版数据目录与开发版快捷方式均已清理；安装版 `v0.7.2` 仍在 `D:\轻量化工具集\` 运行，未动。
- 工作树干净（本版改动已提交）；`src-tauri/src/window_pinner.rs` 换行差异与 `.codex/` 仍未动。

# Scope

- 工具集：截图与贴图、剪贴板、软件使用统计、按键使用统计、计时器、窗口置顶、快捷键提示（共 7 个）。
- 本阶段：`v0.8.4` 已发布；下一轮需求先读 `progress.md` 的长期待办与 `bugs.md`。
- 暂不修：截图覆盖层开启辅助线时的拖拽掉帧（方案见 `progress.md`）。

# Git / Working Tree

- 分支 `master`；最新提交 `5c3986d feat: add hotkey trigger types with key/mouse listening v0.8.4`；已推送 `origin/master`。
- 最新 tag `v0.8.4`；Release https://github.com/THE2580/LightweightToolset/releases/tag/v0.8.4 。
- 安装包 `LightweightToolset_0.8.4_x64-setup.exe`：33,984,141 字节，SHA-256 `3e1968f7d92c74aca80cb587ee2daaa1955aa67070f8dc6d11d03af23e5b9762`。

# Active Files

- `src-tauri/src/hotkey_listener.rs`：键鼠监听服务（低级键鼠钩子线程、路由表、去抖、鼠标侧键录制通道）。
- `src-tauri/src/tools.rs`：快捷键注册/注销按类型分流、触发类型存储、跨类型冲突、非录制阶段状态标记。
- `src-tauri/src/lib.rs`：`dispatch_hotkey` / `dispatch_hotkey_id` 派发层；`set_tool_hotkey_kind` / `begin_hotkey_capture` / `end_hotkey_capture` 命令。
- `src-tauri/src/shortcut_tip.rs` / `native_tip.rs`：提示条目自动识别键形态、鼠标侧键提示、命名键白名单、浮窗键帽外显。
- `src/App.tsx` / `App.css`：`HotkeyRowEditor` 触发类型、红色虚线状态、提示弹窗自动识别与侧键录制。
- `AGENTS.md` / `README.md` / `agent_memory/*`：约束、对外说明与记忆。

# Verification

- `cargo test --lib -- --test-threads=1` → **69 passed / 3 ignored**。
- `npm run build`、`cargo check` 通过（仅 4 条既有 warning）；`npm run tauri build` 产出 NSIS 0.8.4。
- 用户已验收全部 4 批改动（触发类型、工具启用修复、批次 A、批次 B）。
- Release 资产名与 SHA-256 已用 `gh release view` 核对（资产名完整、digest 一致）。

# Blockers

- 无。

# Next Action

1. 下一轮需求先读 `progress.md` 长期待办（辅助线掉帧、标注拖动/文本再编辑、拖拽缩略图、贴图历史、自定义图标）与 `bugs.md`。
2. 再次改动前按 AGENTS「验证方式」重新起开发环境（`npm run dev` + 复制 exe 到 `%TEMP%/lwt-dev` + 管理员启动）。
3. 若监听型触发异常，先看设置 → 日志里的 `hotkey.listen.registered` / `tool.hotkey.triggered id=` 条目。
