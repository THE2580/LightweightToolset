# Goal

复刻 PixPin 截图工具（LightweightToolset 新工具）。**第一版已于 2026-11-11 随 `v0.7.0` 发布**：区域截图、12 种标注与二级栏、贴图窗口、贴图标注、本地 OCR 文本选择、超级拖放缩略图、锁定态金色阴影。后续候选见 `progress.md`「待办」。

# Current Focus

- **v0.7.0 已发布（2026-11-11）**：安装包 `LightweightToolset_0.7.0_x64-setup.exe`（33,846,940 字节，SHA-256 `705D550D7330C54F6BAB7EC15C49F6294DF66FD007D87FF07706D9C0645AB4EE`），Release `https://github.com/THE2580/LightweightToolset/releases/tag/v0.7.0`，发布提交 `b5775dc`；安装验证通过（模型落在 `<install>/ocr/ppocr_v6_small`，安装版 OCR 正常）。
- 当前无进行中的开发任务；工作树只剩既有三项（`window_service.rs`、`window_pinner.rs`、`.codex/`）。

# 下一批候选（用户未定）

1. 标注拖动 / 文本再编辑（后续扩展项）：建议三档——①选中 + 拖动平移；②文字双击再编辑（复用内联编辑器、提交替换原 item）；③控制点缩放（多点图形需重采样，最后做）。两处（`overlay.rs`、`pin.rs`）都要接；拖动只在标注模式下生效，避免与划词/拖窗口抢左键。
2. 拖拽缩略图观感优化（真高斯阴影、长边可配、偏移微调）。
3. 其他有快捷键的工具迁到各自工具页「快捷键」标签。
4. 贴图历史浏览界面（用户暂缓）。
5. 系统 DragImage 崩溃根因（`778e0a4`，当前走自绘 ghost，不动也可）。

# Remaining / 不做

- 不做：截图查看、GIF、UI 检测、多语言、主题/托盘图标自定义、托盘点击动作（详见 `progress.md`）。
- 未测但用户已「直接通过」：双击动作=复制、关闭前确认弹窗、无覆盖层时按贴图键贴「最近一次截图」。

# Verification（最近一次）

- `npm run build` / `cargo check` / `cargo test --lib -- --test-threads=1`：**48 passed / 0 failed / 2 ignored**（ignored 为 OCR 端到端探针）。
- `npm run tauri build` 成功；安装包 33,846,940 字节；安装后 `ocr/` 落位正确、`Ctrl+Alt+A` → `Ctrl+Alt+P` → `ocr.ready chars=255 lines=15`。
- 逐轮历史验证记录见 `archive/2026-11-11-screenshot-progress.md`。

# Resume Here

0. **已发版（2026-11-11）**：HEAD `b5775dc`（工作树干净，仅既有三项）；tag `v0.7.0` 已推送、Release 已发布；开发环境已关闭（Vite 停、`%TEMP%\lwt-dev` 已清理），安装版 0.7.0 正常运行。
1. 下一批做什么等用户指定；候选见上「下一批候选」。
2. 重启开发环境：`npm run dev`（:1420）→ `cargo build` → `cmd /c start` 调 `%TEMP%\lwt-elevate.ps1`（内部 `lwt-dev-launch.ps1` 会重建 `%TEMP%\lwt-dev` 并复制 `resources\ocr`）→ `%TEMP%\lwt-dev-shot8.ps1` 截图核对页面。
3. 坑速查：
   - 改 `settings.json` 用 node（PS 5.1 会按 ANSI 解码并写 BOM）；改正式版配置要「先停进程」。
   - 注入测试取消拖放前先松 Ctrl（`Ctrl+Esc` 会弹开始菜单）；`SetCursorPos` 后等 ≥200ms 再 `mouse_event`；`.ps1` 必须 ASCII-only 或带 BOM。
   - `Start-Process` 启动 GUI 会挂住 DSH 管道，用 `cmd /c start` 解耦。
   - 提交时排除 `window_service.rs`、`window_pinner.rs`、`.codex/`。
