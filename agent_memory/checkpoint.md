# Goal

复刻 PixPin 截图工具（LightweightToolset 新工具）。`v0.7.0`（2026-11-11）发布第一版；`v0.7.1`（2026-11-20）发布快捷键体系收口、主窗口唤出加固与截图设置页调整，均已发版。

# Current Focus（2026-11-20）

- **`v0.7.1` 已发布**（发布提交 `c1359bc`，tag 与 master 均已推送）：快捷键外显统一 `Win → Ctrl → Alt → Shift → 主键`、`SUPER` 不再外露；提示预设扩充（29 条系统 + 9 条绑定，含截图工具 6 条）；贴图组快捷键恢复提示；主窗口最小化可唤出 + 唤出事件落盘 `logs/window.log`；截图设置页「UI 检测」独立、辅助线改折叠分组。
- **用户进入日常使用阶段**，重点观察「静默启动后整个软件像死的」是否复现；该问题已冻结，取证流程见 `docs/troubleshooting-main-window.md`（`window.log` + 任务管理器 + 托盘菜单能否弹出），复现后再定方案。
- **UI 元素级检测：用户明确暂缓，不做**（`窗口和UI` 保持置灰）。
- 发版前已按固定清理项删除开发版数据与 `%TEMP%\lwt-dev`；重新开发需按「验证方式」重建环境。
- 工作树只剩既有三项：`M src-tauri/src/window_pinner.rs`（CRLF 差异）、`M src-tauri/src/window_service.rs`（rustfmt 重排）、`?? .codex/`——不处理、不提交。

# 最近验证（2026-11-20）

- `v0.7.1` 发版：`npm run tauri build` 成功（release exe 41,460,224 字节，NSIS 33,856,072 字节）；资产 `LightweightToolset_0.7.1_x64-setup.exe` 名称完整、digest 与本地 SHA-256 一致；tag `v0.7.1` 与 Release 已发布。
- 截图设置页：UI 检测独立、辅助线整行折叠（默认收起、切不显示自动收起、切标签重置、无模式重置按钮）均逐项截图核对。
- `npm run build` 通过；`cargo check` 通过（4 条既有 warning）；`cargo test --lib -- --test-threads=1` **52 passed / 0 failed / 2 ignored**。
- 实机：最小化 `SW_MINIMIZE` 后按 `CTRL+ALT+.` → `visible=True iconic=False`（修复前 `iconic=True`）；隐藏态唤出、`window.log` 落盘均正常。
- 实机：`Ctrl+Alt+P` / `Ctrl+Q` / `Ctrl+Alt+X` / `Ctrl+Alt+H` 按下后 `Shortcut tip` 窗口在 100ms 内可见（此前只有截图会提示）。
- 实机：设置页 `Ctrl+Alt+.`、截图页 `Ctrl+Alt+F9` 等、剪贴板弹窗 `Win+Ctrl+/`、提示列表 `Win+Ctrl+/`、`Win+G`、`Win+T`、`Ctrl+Alt+P 贴图` 均按新口径显示。

# 下一批候选（用户未定）

1. 标注拖动 / 文本再编辑（三档：选中+拖动平移 → 文字双击再编辑 → 控制点缩放），两处 `overlay.rs`、`pin.rs` 都要接。
2. 拖拽缩略图观感优化（真高斯阴影、长边可配、偏移微调）。
3. 贴图历史浏览界面（用户暂缓）。
4. 系统 DragImage 崩溃根因（`778e0a4`，当前自绘 ghost 稳定，不动也可）。
5. 静默启动「整个软件像死的」——等正式版实机证据。

# 不做（用户已确认）

- 截图查看、GIF 动态截图、多语言、主题/托盘图标自定义、托盘点击动作、UI 元素级检测（暂缓）。
- 未测但用户「直接通过」：双击动作=复制、关闭前确认弹窗、无覆盖层时按贴图键贴「最近一次截图」。

# Resume Here

0. 复现/开发：`npm run dev`（确认 :1420 返回 200）→ 改过 Rust 先 `cargo build` → 复制到 `%TEMP%\lwt-dev\` → **直接** `powershell -Command "Start-Process powershell -Verb RunAs -ArgumentList ... -File %TEMP%\lwt-dev-launch.ps1"`（不要用 `cmd start` 包一层）。
1. 等用户指定下一批功能；UI 元素检测已暂缓，不要主动开工。
2. 发版已完成（`v0.7.1`，tag 与 master 均已推送）；后续推送走代理 `127.0.0.1:7897`。
3. 坑速查：
   - 改 `settings.json` 用 node（PS 5.1 会按 ANSI 解码并写 BOM）；改正式版配置要「先停进程」。
   - 注入测试：`.ps1` 必须 ASCII-only；`c:` 绝对坐标 / `r:` 窗口相对坐标；热键注入必须从**提权**进程发（普通权限的 `keybd_event` 打不到提权应用的 RegisterHotKey）。
   - 弹窗类验证别依赖截图（易错过 1 秒窗口），改用窗口可见性轮询。
   - 提交时排除 `window_service.rs`、`window_pinner.rs`、`.codex/`。
