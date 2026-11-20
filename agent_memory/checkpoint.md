# Goal

复刻 PixPin 截图工具（LightweightToolset 新工具）。`v0.7.0`（2026-11-11）已发布第一版；此后完成「设置入口归位」「截图 UI 检测第一阶段」「主窗口唤出加固与快捷键提示完善」三条线，均未发版。

# Current Focus（2026-11-20）

- **快捷键体系收口（已提交 `7fd2878`，未推送）**：外显统一 `Win → Ctrl → Alt → Shift → 主键`，`SUPER` 不再外露；快捷键提示预设扩充到 9 条绑定条目（呼出主窗口、剪贴板、窗口置顶 + 截图工具 6 条）与 29 条系统预设（新增 `Win+P/Win+R/Win+T/Win+G/Alt+F4`）。
- **主窗口唤出加固（已提交 `c4d3207`，未推送）**：最小化时先 `unminimize()`；窗口记录缺失时有界重试 + `FindWindowExW` 兜底；`show()` 后回读状态；唤出事件写 `<存储目录>/logs/window.log`；贴图组快捷键先派发提示再执行动作。
- **冻结中**：「静默启动后整个软件像死的（托盘菜单能弹、其余全失效）」本机 9 次冷启动未复现，用户决定等正式版实机复现后按 `docs/troubleshooting-main-window.md` 取证再查，本轮不动。
- **UI 元素级检测：用户明确暂缓，不做**（`窗口和UI` 保持置灰）。
- 开发环境：Vite `:1420`（HTTP 200）；开发版实例来自 `%TEMP%\lwt-dev`，数据已复位为基线（剪贴板/截图/窗口置顶启用 + 快捷键提示启用，`silentStart=false`）。
- 工作树只剩既有三项：`M src-tauri/src/window_pinner.rs`（CRLF 差异）、`M src-tauri/src/window_service.rs`（rustfmt 重排）、`?? .codex/`——不处理、不提交。

# 最近验证（2026-11-20）

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
2. 记忆与约束已随 `docs:` 提交同步；推送走代理 `127.0.0.1:7897`。
3. 坑速查：
   - 改 `settings.json` 用 node（PS 5.1 会按 ANSI 解码并写 BOM）；改正式版配置要「先停进程」。
   - 注入测试：`.ps1` 必须 ASCII-only；`c:` 绝对坐标 / `r:` 窗口相对坐标；热键注入必须从**提权**进程发（普通权限的 `keybd_event` 打不到提权应用的 RegisterHotKey）。
   - 弹窗类验证别依赖截图（易错过 1 秒窗口），改用窗口可见性轮询。
   - 提交时排除 `window_service.rs`、`window_pinner.rs`、`.codex/`。
