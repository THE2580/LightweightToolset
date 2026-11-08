# Goal
复刻 PixPin 截图工具（LightweightToolset 新工具）。S3 贴图两轮已完成并提交，用户已验收；下一步是贴图标注与文本选择。

# Current Focus
- `cd1052a`：贴图窗口/历史/快捷键组/「贴图」标签。
- `9c72761`：修上下颠倒、缩放改左上角固定、压延迟、PixPin 同款三态阴影、右键菜单（图像处理/复制/另存为/阴影/锁定/关闭）。
- `7fc6049`：右键菜单改 owner-draw 紧凑版式（左右留白收紧、雅黑字体、置灰项不可高亮），用户验收通过。
- `394291c`：去掉自绘子菜单箭头（系统会画，重叠成两个箭头）。
- `67044c6`：贴图内存压缩（DIB 作唯一像素源、bottom_up 按需、LAST_CLOSED 上限 3）+ 超级拖放（Ctrl+拖动把图以临时 PNG 拖给其它程序）。
- 开发版已按固定流程重启（`%TEMP%\lwt-dev`），Vite `:1420` 在跑；开发版设置已恢复：`screenshot=CTRL+ALT+A`、`pin_restore=CTRL+ALT+R`、`pin_passthrough` 空、`screenshotPinConfirmClose=false`、`screenshotPinLock=none`、`screenshotPinDoubleClick=none`、`screenshotPinHistory=20`。

# Remaining
1. **贴图标注**：右键「标注」进入标注模式 → 只含贴图相关项的标注工具栏（外形参照截图工具栏）→ 工具栏跟随贴图移动 → 最右对勾结束标注退出；「隐藏标注」同步实现；标注内容随贴图缩放/旋转。
2. **贴图文本选择**：「选择所有文本」「文本可选择」两项（含 OCR 文本）。
3. 贴图历史浏览界面（目前只有「恢复上一次」+ 保留数量）。
4. **待办（本工具完成后）**：其他有快捷键的工具逐个迁到各自工具页的「快捷键」标签。
5. 不做：截图查看、GIF、UI 检测、多语言、主题/托盘图标自定义等（见 `progress.md`）。

# Verification
- `npm run build`、`cargo check`、`cargo test --lib screenshot::` 23/23 通过。
- 注入实测（提权 PowerShell）：
  - 贴图不再上下颠倒（贴图区域内文字正读）；
  - 按下贴图到窗口可见约 80–125 ms（含合成按键固定开销）；
  - 滚轮缩放 `600,400` 起点不变、400×296 → 440×326；
  - 阴影三态截图确认：出现淡蓝泛光 / 稳定失焦灰 / 点击后焦点淡蓝；
  - 右键菜单结构正确（文本选择与标注置灰），`WM_COMMAND` 11/12/14/40/20/50 实测：右旋 400×296→296×400 且左上角不动、重置回 400×296、阴影开关 1→0→1、复制图像进剪贴板 400×296、菜单关闭销毁窗口；
  - 关闭全部与恢复上一次关闭均正常（恢复曾用 `CTRL+ALT+J` 验证，因本机 `CTRL+ALT+R` 被其它软件占用）。
- 菜单紧凑版截图确认：主菜单与「图像处理」二级菜单左右留白收紧、勾选与箭头贴近边缘、灰项正确置灰；`WM_COMMAND` 11/14/40/50 在 owner-draw 版下仍正常（旋转 400×296→296×400、重置、阴影开关、关闭）。
- **未测**：图像另存为的系统对话框、双击动作=复制、锁定勾选菜单项、贴图拖动后阴影跟随（拖动逻辑已调用 `shadow_sync`，未单独截图）。

# Resume Here
1. 环境：Vite `:1420` 与开发版 `%TEMP%\lwt-dev`（pid 6816）在运行；重启流程：`npm run dev` → `cargo build` → 复制 `src-tauri/target/debug/lightweight-toolset.exe` 到 `%TEMP%\lwt-dev` → `powershell -File <脚本> -Verb RunAs -WindowStyle Hidden` → 核对进程路径与页面加载。
2. 下一块是「贴图标注 + 文本选择」，开工前先按约定复述范围等放行。
3. 提交时排除 `window_service.rs`、`window_pinner.rs`、`.codex/`。
4. 测试注意：本机 `CTRL+ALT+A` 常被安装版占用、`CTRL+ALT+R` 被其它软件占用，注入测试时可临时换键（`CTRL+ALT+SHIFT+A` / `CTRL+ALT+J`），测完必须改回并重启；开发版「关闭前确认」若为开，关闭会弹模态框并卡住贴图线程。
