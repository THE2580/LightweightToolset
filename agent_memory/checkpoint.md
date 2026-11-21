# Goal

LightweightToolset 持续迭代（Tauri 2 + Rust + React）：七个工具已全部上线，当前稳定版 `v0.7.2`（2026-11-20 发布，主线程嵌套建窗死锁修复）。当前短任务：修复「符号键快捷键被当成呼出主窗口」并等用户真机确认。

# Current Focus（2026-11-21）

- **当前**：`native_tip`「原生提示实验」工具已完成三轮打磨（视觉/动画、功能、收尾回归），**代码全部未提交**；HEAD 仍是 `3e9fde2`。
- **已验收的能力**：亚克力自绘（快照）、圆角+投影、深浅主题各一套模糊/不透明度、主题跟随/浅/暗、进入 200ms 缓入缓出 + 退出 300ms、弹动 230ms、窗口尺寸固定 1.00（内容 1.20 自适应）、停留时间 0.8–10s 且联动 ×N 累加窗口、X/Y 定位滑块 + 定位模式（静态预览「示例提示」+键盘图标、常驻、实时、重置即时、期间后端全局压制弹出）。
- **机制**：显示/隐藏/预览走「最新意图 + 单调序号」状态机；`start()` 先 `paint()` 再 `ShowWindow`；预览不开动画定时器。
- **入正已完成**（用户验收通过）：快捷键提示浮窗由 `native_tip` 接管，工具 id 仍为 `shortcut_tip`（预设/自定义零迁移），外观设置在后端 `appearance` 字段并由「设置」弹窗编辑；WebView 浮窗页与实验工具条目已删除。
- **待决定**：① 是否提交这批入正改动；② 版本号是否按规则升到 `0.8.0`（发版时统一定）；③ 长快捷键超宽自动加宽（已评估：暂不做）。
- **现场**：工作树新增 `src-tauri/src/native_tip.rs`，修改 `Cargo.toml`/`lib.rs`/`tools.rs`/`App.tsx`/`App.css`/`main.tsx`/`agent_memory/*`；`window_pinner.rs` 与 `.codex/` 是既有现场，不要动。

# 最近验证（2026-11-21）

- 符号键快捷键修复：`cargo test --lib -- --test-threads=1` 54 passed / 2 ignored、`cargo check` 通过；开发版注入 `Ctrl+Win+/` → `Clipboard` 窗口出现、Esc 关闭；`CTRL+ALT+.` 作主窗口快捷键时，窗口隐藏后按下可重新唤出。
- 开发版启动自检：`:1420` 返回 200、进程路径来自 `%TEMP%\lwt-dev`、主窗口标题带「（开发版）」、页面正常加载。

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
5. ~~静默启动「整个软件像死的」~~ 已随 `v0.7.2` 修复结案。
6. 观察项（未定，非本轮引入）：`shortcut_tip` 工具停用时，本软件快捷键仍会弹提示浮窗。

# 不做（用户已确认）

- 截图查看、GIF 动态截图、多语言、主题/托盘图标自定义、托盘点击动作、UI 元素级检测（暂缓）。
- 未测但用户「直接通过」：双击动作=复制、关闭前确认弹窗、无覆盖层时按贴图键贴「最近一次截图」。

# Resume Here

0. 本轮修复已提交 `f81a99e`、**未推送**。要推送/发版必须先拿明确指令，推送前先让用户开代理 `127.0.0.1:7897`；发版按 `AGENTS.md`「分支、提交与发布」走（含清理开发版痕迹、README/About 同步、ASCII 资产名）。
1. 复现/开发：`npm run dev`（确认 `:1420` 返回 200）→ 改过 Rust 先 `cargo build` → 复制到 `%TEMP%\lwt-dev\` → **直接** `powershell -Command "Start-Process powershell -Verb RunAs -ArgumentList ... -File %TEMP%\lwt-dev-launch.ps1"`（不要用 `cmd start` 包一层）。
2. 热键注入脚本放 `%TEMP%\lwt-dev\`、**ASCII-only**；按键注入必须从提权进程发。
3. 坑速查：
   - 改 `settings.json` 用 node（PS 5.1 会按 ANSI 解码并写 BOM）；改正式版配置要「先停进程」。
   - 注入测试：`.ps1` 必须 ASCII-only；`c:` 绝对坐标 / `r:` 窗口相对坐标。
   - 弹窗类验证别依赖截图（易错过 1 秒窗口），改用窗口可见性轮询（`EnumWindows` + `IsWindowVisible`）。
   - 提交时排除 `window_pinner.rs`、`.codex/`。

