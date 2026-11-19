# Goal

复刻 PixPin 截图工具（LightweightToolset 新工具）。**第一版已于 2026-11-11 随 `v0.7.0` 发布**：区域截图、12 种标注与二级栏、贴图窗口、贴图标注、本地 OCR 文本选择、超级拖放缩略图、锁定态金色阴影。后续候选见 `progress.md`「待办」。

# Current Focus

- **2026-11-19 快捷键入口迁移 + 录制 UI 改版（已验收，待提交）**：剪贴板与窗口置顶的快捷键都迁到各自设置页/工具页，全局设置只剩「呼出主窗口」；`HotkeyRowEditor` 共享组件（`hideTitle` 支持工具栏），录制框单框单按钮、失焦自动保存，2px 边框 / 宽度随内容 / 空态虚线 / 录制态主色 + 光晕。
- 开发版当前状态：`%TEMP%\lwt-dev` 副本运行中（pid 14776），**静默启动默认已改为关闭**；剪贴板/截图/窗口置顶三个工具启用，剪贴板快捷键为空（用户测试「停用」），窗口置顶为默认 `CTRL+ALT+T`。
- 已记账待办（本轮不做）：静默启动后托盘唤起主窗口偶发失效，正式版与开发版均可复现，见 `bugs.md`「活跃风险与约束」。
- 工作树：本次改动已提交（HEAD，未推送），工作树只剩既有三项（`window_service.rs`、`window_pinner.rs`、`.codex/`）。

# 下一批候选（用户未定）

1. 标注拖动 / 文本再编辑（后续扩展项）：建议三档——①选中 + 拖动平移；②文字双击再编辑（复用内联编辑器、提交替换原 item）；③控制点缩放（多点图形需重采样，最后做）。两处（`overlay.rs`、`pin.rs`）都要接；拖动只在标注模式下生效，避免与划词/拖窗口抢左键。
2. 拖拽缩略图观感优化（真高斯阴影、长边可配、偏移微调）。
3. ~~快捷键入口迁移~~ **已完成**：剪贴板与窗口置顶都已迁到各自工具页/设置页，全局设置只剩「呼出主窗口」。
4. 贴图历史浏览界面（用户暂缓）。
5. 系统 DragImage 崩溃根因（`778e0a4`，当前走自绘 ghost，不动也可）。

# Remaining / 不做

- 不做：截图查看、GIF、UI 检测、多语言、主题/托盘图标自定义、托盘点击动作（详见 `progress.md`）。
- 未测但用户已「直接通过」：双击动作=复制、关闭前确认弹窗、无覆盖层时按贴图键贴「最近一次截图」。

# Verification（最近一次）

- 2026-11-19 剪贴板迁移：`npm run build` 通过；开发版实机验证（弹窗内录制/保存/即时刷新、提示层级、全局设置无剪贴板分组、截图页快捷键标签无回归）。Rust 未改动。
- 上一次发版（2026-11-11）：`npm run build` / `cargo check` / `cargo test --lib -- --test-threads=1` **48 passed / 0 failed / 2 ignored**；安装包 33,846,940 字节、`ocr/` 落位正确、OCR 实测通过。

# Resume Here

0. **开发版在跑**：`npm run dev`(:1420) + `%TEMP%\lwt-dev` 副本（pid 14776）。改前端后 Vite 会热更，必要时 `Ctrl+R`。
1. 快捷键这批已验收并提交（HEAD，未推送）；是否推送等用户明确指令。
2. 重启开发环境：`npm run dev` →（改过 Rust 时）`cargo build` → **直接** `powershell -Command "Start-Process powershell -Verb RunAs -ArgumentList ... -File %TEMP%\lwt-dev-launch.ps1"`（不要用 `cmd start` 包一层）→ `%TEMP%\lwt-ui.ps1 -Steps 'c:x,y;s:ms' -Png ...` 注入点击并截图。
3. 坑速查：
   - 改 `settings.json` 用 node（PS 5.1 会按 ANSI 解码并写 BOM）；改正式版配置要「先停进程」。
   - 注入测试取消拖放前先松 Ctrl（`Ctrl+Esc` 会弹开始菜单）；`SetCursorPos` 后等 ≥200ms 再 `mouse_event`；`.ps1` 必须 ASCII-only 或带 BOM。
   - 提权启动 GUI：**别**用 `cmd /c start` 包一层（会静默失败）；从当前 shell 直接 `Start-Process -Verb RunAs`，启动后核对 `%TEMP%\lwt-dev` 已生成 + 进程 StartTime 是新的。
   - 提交时排除 `window_service.rs`、`window_pinner.rs`、`.codex/`。
