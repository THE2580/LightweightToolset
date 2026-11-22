# 当前现场

- 2026-11-22：`0.8.0` 功能开发已收敛（快捷键提示原生浮窗 + 键帽 DIY + 截图缺陷修复），**代码全部已提交**，`origin/master` 尚未推送新提交。
- 开发版在跑：`%TEMP%\lwt-dev\lightweight-toolset.exe`（Vite 1420 常驻）；安装版 `v0.7.2` 在 `D:\轻量化工具集\` 运行中，**不要结束**。
- 本批（未提交）：键盘图标保留原实心样式，其余图标线性描边（眼睛瞳孔也改描边、无线型填充）；`Ctrl+A`/`Ctrl+X` 等无专属图标的快捷键一律退回键盘图标；（键盘/复制/粘贴重画），新增 save/find/undo/redo/pin/pin_top/close_all/eye/pointer/window/clipboard/camera 共 15 种；软件级快捷键按 id 映射图标（`icon_for_hotkey`），自定义快捷键按快捷键内容推断（含 Ctrl+S/F/Z/Y 等）。

# Scope

- 工具集：截图与贴图、剪贴板、软件使用统计、按键使用统计、计时器、窗口置顶、快捷键提示（共 7 个）。
- 本阶段目标：`0.8.0`（快捷键提示浮窗改为原生绘制 + 键帽/外观 DIY + 截图与热键缺陷修复）。
- 暂不修：截图覆盖层开启辅助线时的拖拽掉帧（见 `progress.md` 待办）。

# Git / Working Tree

- 分支 `master`；最近提交：`677d109`（键帽配色/字号/恢复默认）、`57a0566`（截图贴图快捷键与关闭确认等修复）、`91c7d3f`（工具生命周期热键注销）、`67a2a54`（原生浮窗入正）。
- 工作树只剩既有现场：`src-tauri/src/window_pinner.rs` 换行差异、`.codex/`（都不要提交）。
- 未推送：`57a0566`、`677d109`（推送前需确认代理 `127.0.0.1:7897`）。

# Active Files

- `src-tauri/src/native_tip.rs`：原生提示浮窗（分层窗口 + GDI 自绘、帧缓存、真实时间动画、键帽配色覆盖、图标种类）。
- `src-tauri/src/shortcut_tip.rs`：快捷键提示业务（预设/自定义、节流去重、`appearance` 设置、worker 调 `native_tip::show`）。
- `src-tauri/src/screenshot/{mod,overlay,pin}.rs`：本轮截图修复（贴图快捷键作用域、关闭所有确认、穿透/隐藏状态复位、选区漏压暗）。
- `src/App.tsx`：快捷键提示页与「提示外观」设置弹窗（`createPortal` 标准弹窗、颜色选择、恢复默认）。
- `AGENTS.md` / `agent_memory/*`：约束与记忆。

# Verification

- `cargo test --lib -- --test-threads=1` → **55 passed / 3 ignored**；`npm run build` 通过；`cargo check` 仅剩仓库原有 4 条 warning。
- 冒烟：`cargo test --lib native_tip -- --ignored`（显示→停用→再显示），渲染帧可用 `NATIVE_TIP_DUMP` 导出人工核对。
- 开发版自检：1420 监听 + HTTP 200；exe 从 `%TEMP%\lwt-dev` 启动；页面正常加载；安装版进程不动。

# Blockers

- 无阻塞。发版前需用户授权并开代理。

# Next Action

1. 为快捷键提示的常用预设补充/配置图标（先复述口径，再实现）。
2. 之后再走 `0.8.0` 发版流程（版本号三处、README + 仓库简介、清理开发版痕迹、ASCII 资产名 + SHA-256）。
