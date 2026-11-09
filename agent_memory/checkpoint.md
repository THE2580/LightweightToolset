# Goal
复刻 PixPin 截图工具（LightweightToolset 新工具）。贴图标注工具栏（复用截图工具栏 + 独立伴生小窗）与右键菜单「标注/隐藏标注」已由用户验收通过，待提交；之后是文字/序号/马赛克等工具，再是贴图文本选择与历史浏览。

# Current Focus

- **贴图标注工具栏（2026-11-09，已验收，未提交）**：
  - `pin.rs` 删掉临时迷你工具栏（`PinAction`/`pin_toolbar_*`/`draw_pin_toolbar`），改为 `toolbar::layout/draw/hit` + `Mode::Pin`，动作分发照抄 `overlay.rs`。
  - 用户否决「画在贴图窗口内」（窗口比工具栏窄时被裁切，实测 320×240 只剩前 6 个按钮、看不见对勾），选定**贴图窗口外的独立伴生小窗**（像 PixPin）。
  - 小窗：`LightweightToolsetPinToolbar`，`WS_EX_LAYERED|TOOLWINDOW|TOPMOST|NOACTIVATE`，Sentinel 抠透明 + `UpdateLayeredWindow`；主栏贴贴图下沿 8px、二级栏与下拉在其下方生长；下方放不下整体翻到贴图上方；宽度按 `2×内容宽 − 主栏宽` 计算（二级栏比主栏宽，约 1017px）。
  - 交互：Esc 标注时只退出标注；无工具时拖动仍是移动窗口；删除模式点画布删除；滚轮落在数值项调数值、其余在画布上缩放。
  - 标注合成缓存（图像分辨率 + `annot_dirty`），鼠标悬停只重贴不重算。
- 拖放：`8c97d46` 已回退到 ghost 版（能拖、无缩略图）；`778e0a4` 崩溃事故见 `bugs.md`，根因待查。

# Remaining
1. 本次改动尚未提交（`pin.rs`、`toolbar.rs`、`agent_memory/`）；用户验收已通过。
2. 贴图标注补：文字、序号、马赛克/模糊（工具栏能选中，但贴图内还没有文字内联编辑等实现）。
3. 贴图文本选择（「选择所有文本」「文本可选择」+ OCR）。
4. 贴图历史浏览界面。
5. 其他有快捷键的工具逐个迁到各自工具页的「快捷键」标签。
6. 不做：截图查看、GIF、UI 检测、多语言、主题/托盘图标自定义（见 `progress.md`）。

# Verification
- `cargo build` 通过（4 条既有告警）；`cargo test --lib screenshot::` **23/23**。
- 注入实测（提权 PowerShell，热键临时改为 `CTRL+ALT+SHIFT+A/P` 避开安装版）：
  - 900×600 贴图：进标注 → 点形状组 → 小窗 830×112（二级栏完整，8 色全见）→ 拖红框（`items=1`）→ 对勾退出 → 工具栏销毁、红框留存；
  - 320×240 贴图：工具栏完整显示在贴图下方（不再被裁切），绘制正常；
  - 贴屏幕底部（pin 700,1380–1000,1540）：工具栏翻到贴图上方 ✔；
  - 截图见 `%TEMP%\lwt-pin-tb8-c.png`、`lwt-pin-tb6-c.png`、`lwt-pin-tb10-a.png`。
- 未验证：文字/序号/马赛克工具绘制、下拉菜单（线型/箭头）在小窗里的展开、滚轮调数值、删除模式、窗口拖动时小窗跟随（`WM_WINDOWPOSCHANGED` 已接）。

# Resume Here

0. 当前 HEAD `d32b36f`（`feat: reuse screenshot annotation toolbar for pin`）；工作树只剩既有的 `window_pinner.rs`、`window_service.rs`、`.codex/`，不要提交。
1. **开发已暂停（2026-11-09）**：开发版进程与 Vite `:1420` 均已关闭（只剩安装版 `D:\轻量化工具集` 在跑）。下次开工按固定流程重启：`npm run dev`（Vite 1420）→ `cargo build` → 复制 `src-tauri/target/debug/lightweight-toolset.exe` 到 `%TEMP%\lwt-dev` → 提权启动 → 核对进程路径与页面加载。
2. 下一步补贴图标注的文字/序号/马赛克：`toolbar` 的 `Action::Tool(Shape::Text/Serial)` 已能切工具，缺的是贴图内的文字内联编辑（覆盖层那套 `start_text_edit`/IME 逻辑在 `overlay.rs`，需要按贴图窗口改造）。
3. 之后：文本选择（「选择所有文本」「文本可选择」+ OCR）与贴图历史浏览；`MENU_HIDE_ANNOTATION` 已验证可切换。
4. 注入测试注意：安装版与开发版热键冲突，临时改开发版热键用 `%TEMP%\lwt-hk-patch.js unique|restore`，测完必须 restore 并重启；系统输入注入里 `SetCursorPos` 后要等 ≥200ms 再 `mouse_event`。
5. 提交时排除 `window_service.rs`、`window_pinner.rs`、`.codex/`；提交前 `npm run build` + `cargo check` + `cargo test --lib screenshot:: -- --test-threads=1`。
