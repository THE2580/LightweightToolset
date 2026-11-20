# Goal

复刻 PixPin 截图工具（LightweightToolset 新工具）。`v0.7.0`（2026-11-11）发布第一版；`v0.7.1`（2026-11-20）发布快捷键体系收口、主窗口唤出加固与截图设置页调整，均已发版。

# Current Focus（2026-11-20）

- **`v0.7.2` 已发布**（发布提交 `abe5441`，tag 与 master 均已推送）：修复「静默启动后整个软件像死的」——主线程嵌套建窗死锁。修复方式：`window_service::begin_window_build/window_build_busy` 占位 + 后到建窗让路（浮窗退避重试、弹窗线程排队），让路写 `logs/window.log`。
- **该卡死问题已结案**：现场用 `WM_NULL` 探活 + 主线程栈回溯（`StackWalk64`/dbghelp）定位到 `open_clipboard_popup` 与 `shortcut_tip::build_window` 嵌套建窗；复现修复后日志确证让路两次、两窗正常、进程持续响应。
- **UI 元素级检测：用户明确暂缓，不做**（`窗口和UI` 保持置灰）。
- 开发环境已按发版要求停掉并清理；安装版旧进程（含卡死的 0.7.1）已结束，用户升级到 0.7.2 后常驻。
- 工作树只剩既有三项：`M src-tauri/src/window_pinner.rs`（CRLF 差异）、`M src-tauri/src/window_service.rs`（rustfmt 重排）、`?? .codex/`——不处理、不提交。

# 最近验证（2026-11-20）

- `v0.7.2` 发版：`npm run tauri build` 成功（NSIS `轻量化工具集_0.7.2_x64-setup.exe` 33,851,424 字节）；资产 `LightweightToolset_0.7.2_x64-setup.exe` 名称完整、digest 与本地 SHA-256 一致；tag 与 Release 已发布。
- 死锁修复验证：开发版按剪贴板快捷键时 `window.log` 记录 `window_build.busy defer=shortcut_tip` 两次，剪贴板弹窗与提示浮窗同时出现、主窗口保持隐藏、`ping=OK`、进程响应正常。
- `cargo test --lib -- --test-threads=1` **52 passed / 0 failed / 2 ignored**。
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
