# Goal

LightweightToolset 持续迭代（Tauri 2 + Rust + React）：七个工具已上线，稳定版 `v0.7.2`；当前在准备 `0.8.0`（快捷键提示浮窗原生化的完整版）。

# Current Focus（2026-11-22，v0.8.0 已发布）

- **`0.8.0` 功能已收敛并全部提交**：快捷键提示浮窗改原生绘制（`67a2a54`）、键帽配色/字号/恢复默认（`677d109`）、热键兜底与生命周期修复（`f81a99e`/`91c7d3f`）、截图缺陷修复（`57a0566`）。
- **已验收能力**：原生浮窗（亚克力快照、圆角投影、深浅主题各一套模糊/不透明度、进入 200ms 缓入缓出、退出 300ms、弹动 230ms、内容 1.20 自适应）；外观设置弹窗（主题/动画/停留时长/位置/键帽三色/恢复默认）；显示走「最新意图 + 序号」状态机，`start()` 先 `paint()` 再 `ShowWindow`。
- **已完成**：`v0.8.0` 发布（tag/Release/About 均已更新），开发环境已关闭。
- **明确不做**：截图覆盖层辅助线掉帧（已回退并记入 `progress.md` 待办）；长快捷键超宽自动加宽。

# Remaining

- 预设图标完善（下一步）。
- 发版流程（用户授权后）。
- 历史候选见 `progress.md` 待办（标注拖动/文本再编辑、拖拽缩略图观感、贴图历史浏览、DragImage）。

# Verification

- `cargo test --lib -- --test-threads=1` → **55 passed / 3 ignored**；`npm run build` 通过；`cargo check` 仅 4 条既有 warning。
- 冒烟：`cargo test --lib native_tip -- --ignored`（显示→停用→再显示）；`NATIVE_TIP_DUMP` 可导出渲染帧人工核对。
- 开发版自检：1420 监听 + HTTP 200；exe 来自 `%TEMP%\lwt-dev`；页面正常；不动安装版进程。

# Resume Here

1. 读 `agent_memory/working_state.md` 与 `progress.md` 恢复现场。
2. 若继续功能：先为快捷键提示的常用预设补图标（`shortcut_tip.rs` 预设表 + `native_tip.rs` 的 `IconKind`/`draw_icon`，必要时给 `appearance`/payload 增加图标字段）。
3. 若发版：改三处版本号为 `0.8.0` → 更新 README 与仓库简介 → 清理开发版痕迹 → 构建 NSIS → 资产改名 ASCII + 记录 SHA-256 → 推送（确认代理）→ 发布 tag/Release。
