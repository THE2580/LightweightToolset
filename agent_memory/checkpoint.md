# Goal

LightweightToolset 持续迭代（Tauri 2 + Rust + React）：七个工具已上线，最新稳定版 `v0.8.4`（快捷键触发类型）。

# Current Focus（2026-11-27，v0.8.4 已发布）

- 本版内容：快捷键触发类型（`global` 组合键/单个 F1–F24 与 `listen` 任意单键/鼠标侧键，单按钮切换、跨类型冲突、键被占用提示）；工具启用不再被快捷键注册失败阻塞；非录制阶段不可用键用红色虚线边框；快捷键提示改为按键形态自动识别，支持鼠标侧键提示/录制，补齐 CapsLock 等命名键与 OEM 符号键。
- 发布：提交 `5c3986d`，tag `v0.8.4`，Release https://github.com/THE2580/LightweightToolset/releases/tag/v0.8.4 ；资产 `LightweightToolset_0.8.4_x64-setup.exe`（33,984,141 字节，SHA-256 `3e1968f7d92c74aca80cb587ee2daaa1955aa67070f8dc6d11d03af23e5b9762`）；README 与仓库 About 已更新。
- 开发环境已关闭（开发版进程、Vite、开发版数据/快捷方式、`%TEMP%/lwt-dev` 已清理）。安装版已更新为 `v0.8.4`（`D:\轻量化工具集\`），用户已安装并开始体验。

# Remaining

- 长期待办见 `progress.md`：截图辅助线掉帧、标注拖动/文本再编辑、拖拽缩略图观感、贴图历史浏览、自定义图标（方案 A，暂不做）。
- 无进行中的未提交任务。

# Verification

- `cargo test --lib -- --test-threads=1`（TEMP/TMP 指向 `src-tauri/target/test-tmp`）→ 69 passed / 3 ignored。
- `npm run build`、`cargo check`、`npm run tauri build` 通过；Release 资产名与 SHA-256 已用 `gh release view` 核对。
- 用户已验收本版全部 4 批改动。

# Resume Here

1. 读 `agent_memory/working_state.md` 与 `progress.md` 顶部恢复现场。
2. 下一轮需求按 AGENTS「分支、提交与发布」判断版本号（本版之后：既有工具扩展改第三位、新工具改第一/二位）。
3. 再改动前重新起开发环境：`npm run dev` + 复制 `src-tauri/target/debug/lightweight-toolset.exe` 到 `%TEMP%/lwt-dev` + `Start-Process -Verb RunAs`。
4. 若监听型快捷键异常：先看设置 → 日志里的 `hotkey.listen.registered` / `tool.hotkey.triggered id=`。
5. 发版前清理开发版痕迹（AGENTS 固定检查项），不要清理正式版目录。
