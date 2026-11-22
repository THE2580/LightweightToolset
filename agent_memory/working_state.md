# 当前现场

- 2026-11-22：`v0.8.0` **已发布**（发布提交 `8d83b88`、tag `v0.8.0` 已推、Release 已建、仓库 About 已更新）；开发环境已关闭（开发版进程、Vite 1420、开发版数据目录、`%TEMP%\lwt-dev` 全部清理）。
- 安装版 `v0.7.2` 仍在 `D:\轻量化工具集\` 运行（本次未动它）。
- 工作树干净：只有既有的 `src-tauri/src/window_pinner.rs` 换行差异与 `.codex/`（都不要提交）。
- 下一步：等待新需求；候选见 `progress.md` 待办（辅助线掉帧、标注拖动/文本再编辑、拖拽缩略图观感等）。

# Scope

- 工具集：截图与贴图、剪贴板、软件使用统计、按键使用统计、计时器、窗口置顶、快捷键提示（共 7 个）。
- 本阶段：`0.8.0` 已交付（快捷键提示浮窗原生化 + 外观 DIY + 线性语义图标 + 实验性图标绘制 + 截图/热键修复）。
- 暂不修：截图覆盖层开启辅助线时的拖拽掉帧（见 `progress.md` 待办）。

# Git / Working Tree

- 分支 `master`；最新提交 `8b5c44e docs: record v0.8.0 release and close dev environment`；已推送 `origin/master`。
- tag `v0.8.0` 已推送；Release https://github.com/THE2580/LightweightToolset/releases/tag/v0.8.0 。
- 安装包 `LightweightToolset_0.8.0_x64-setup.exe`：33,873,147 字节，SHA-256 `a1ba487dfc3aaeae49c364163723321c4c22adcbed3cf41fe16a9d269695e108`。

# Active Files

- `src-tauri/src/native_tip.rs`：原生提示浮窗（分层窗口 + GDI 自绘、帧缓存、真实时间动画、线性图标与路径数据绘制、键帽配色覆盖）。
- `src-tauri/src/shortcut_tip.rs`：快捷键提示业务（预设/自定义、节流去重、`appearance`、软件级快捷键图标映射、实验性图标路径注入）。
- `src-tauri/src/settings.rs`：`AppSettings`（含 `experimental_enabled` / `experimental_icons`）。
- `src/App.tsx`：快捷键提示页与外观设置弹窗、设置页「实验性功能」标签与图标绘制编辑器。
- `AGENTS.md` / `agent_memory/*`：约束与记忆。

# Verification

- `cargo test --lib -- --test-threads=1` → **55 passed / 3 ignored**；`npm run build` 通过；`cargo check` 仅 4 条既有 warning。
- 发版：`npm run tauri build` 成功产出 NSIS 0.8.0；资产名完整、`sha256sum` 与 Release 记录一致。

# Blockers

- 无。

# Next Action

1. 下一轮需求来时先读 `progress.md` 待办与 `bugs.md` 经验。
2. 若继续 0.8.x：优先处理「辅助线掉帧」（方案见 `progress.md`：标注图层跨帧缓存）。
3. 开发调试时按 AGENTS「验证方式」启动开发版（`npm run dev` + 复制 exe 到 `%TEMP%\lwt-dev` + `Start-Process -Verb RunAs`）。
