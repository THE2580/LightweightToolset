# Scope

- 当前范围：截图工具 S1+S2-a 已提交 `c0ef236`；S2-b 标注底座待开工。
- 无进行中的代码任务。

# Git / Working Tree

- 工作目录：`E:\DeepSeek_Harness\Workspace\LightweightToolset`；分支 `master`；最新提交 **`c0ef236`**（截图工具 S1+S2-a，未推送）。
- 既有现场（不处理、不提交）：`M src-tauri/src/window_service.rs`（纯 rustfmt 噪声）、`M src-tauri/src/window_pinner.rs`（仅换行差异）、`?? .codex/`。
- 本机 git 走代理 `http://127.0.0.1:7897`；代理未启动时推送/gh 失败，需先让用户开启。

# Active Files

- `src-tauri/src/screenshot/`（新）：`mod.rs`（热键入口/编排）、`capture.rs`（BitBlt + 裁切 + 变暗，含 3 个单测）、`overlay.rs`（原生覆盖层窗口）、`clipboard_image.rs`（CF_DIB + PNG）、`save.rs`（PNG 编码 + 落盘）。
- `src-tauri/src/tools.rs`、`lib.rs`：注册 `screenshot`、热键分支、DPI 声明、停用时关闭覆盖层。
- `src/App.tsx`：`screenshot: Crop` 图标映射。
- `docs/pixpin-clone-analysis.md`：复刻分析与方案（本线程产出）。
- `src/shortcutTipIcon.tsx`：自定义「键盘 + 提示气泡」图标组件（24 网格、2px 描边、`currentColor`）。
- `src/App.tsx`：`toolIcons` 增加 `shortcut_tip` 映射；工具页头部、编辑弹窗、空状态统一使用该图标。
- `src-tauri/src/lib.rs`、`autostart.rs`、`tools.rs`、`windows_notification.rs`、`settings.rs`、`tauri.conf.json`：自启计划任务、开发版隔离/默认值/锁定、静默启动与通知（0.6.2–0.6.3 已交付）。
- `AGENTS.md`：含「开发版与正式版（差异与注意事项）」与发版固定动作（README/简介、清理开发版痕迹、事后沉淀）。
- `README.md`：已更新到 0.6.4。

# Verification

- 2026-11-06 S2-a.3：脏区重建版本，折线拖动后 stray_blue_pixels=0；测试 23/23。
- 2026-11-06 S2-a.2：整帧重建后压力拖动无拖影；延迟 101 ms 中位（95.7–103.8）；测试 22/22。
- 2026-11-06 S2-a.1 自检：放大镜拖动中常驻并跟随（`(900, 700)`）；`C` 复制 `#ffffff` 后覆盖层退出；`Shift` 切换为 `RGB(...)`；测试 23/23。
- 2026-11-06 S2-a 自检：放大镜面板显示 `(1200, 700)` + `#FFFFFF`；`C` 复制为 `#FFFFFF`，`Shift` 后 `C` 为 `RGB(255, 255, 255)`；拖动时标签显示 `600, 500 · 400 × 300`、无放大镜；`cargo test --lib` 23/23。
- 2026-11-06 扩展模式（副屏 2560x1600@150% 位于 (-138,-1600)，主屏 2240x1400@150%）自检：覆盖层窗口 = 虚拟桌面 `(-138,-1600,2560,3000)`；跨屏拖选 `600x700` 精确落盘，两侧像素非黑且均值不同；热键延迟中位 **156.9 ms**（单屏 102.7 ms，采集面积 4.1MP→7.7MP）。混合缩放未测（两屏同为 150%）。
- 2026-11-06 **用户验收 S1.2 通过**：无闪屏、响应近乎无感、比 PixPin 快（实测中位 102.7 ms，修复前 526.7 ms）。
- 2026-11-06 S1.2 自检：热键 → 覆盖层 → 拖 600×350 → PNG 精确 `600×350`、覆盖层关闭无残留；`cargo test --lib` 21/21。
- 2026-11-06 S1.1 闪屏修复后自检：覆盖层出现 → 拖 600×350 → PNG `600×350`（62,560 字节）落盘，覆盖层关闭，无残留。
- 2026-11-05 S1 **端到端自检通过**（管理员权限注入 Ctrl+Alt+A → 覆盖层出现 → 拖选 300,250→900,600 → 松开）：保存 `600×350` PNG（187,976 字节），剪贴板同时有 `CF_DIB` 与 `PNG`，覆盖层自动关闭。
- 2026-11-05 S1 构建：`cargo check` 无告警；`cargo build` 通过；`cargo test --lib` **21/21**（测试二进制复制到 `%TEMP%` 运行；工作区 `target` 下运行会因会话沙箱无法写 `%TEMP%`）。`npm run build` 通过。
- 开发版当前运行中（`%TEMP%\lwt-dev`），Vite `:1420` 在跑；开发版数据 `%APPDATA%\com.the.lightweight-toolset.dev\settings.json` 预置为只启用 screenshot、`silentStart=false`。
- 多屏已按用户决定移出范围（不再测试/优化；抓屏仍按虚拟桌面并集实现）。待做：覆盖层鼠标指针、连续截图回归、S2-a 放大镜。
- 2026-11-05 覆盖层测量（临时程序 `%TEMP%\pixpin-spike`）：原生 17.3 MB / 44–55 ms；WebView 375 MB（含图）/ 262 MB（空白）/ 323–403 ms。
- 2026-11-05 发版前：`cargo check` 无告警、`cargo test --lib --test-threads=1` 18/18、`npm run build`、`npm run tauri build`（NSIS）通过；exe 版本资源 `0.6.4`。
- 开发版真机确认：新图标在侧栏 15px、页头 18px 下可辨认，与页头/弹窗/空状态一致（用户已确认「通过」）。
- 本次发布资产：`轻量化工具集_0.6.4_x64-setup.exe`（2,677,923 字节，SHA-256 `34C167F700B288B40D0384718804265C2387F83981632FEB90EB0D4898FAB5CD`）。
- README 已更新到 0.6.4；仓库简介复核后仍准确（未改工具列表），未改动。

# Blockers

- 无。安装版进程全程未被结束或改动。

# Next Action

- 下一步 S2-b：在 `src-tauri/src/screenshot/` 下加 `annotation.rs`（对象模型/命令栈），覆盖层接矩形工具与撤销/重做；完成后按约定启动开发版供验收。
- 发版时按「三处版本号 + 更新 README/简介 + 清理开发版痕迹 + 提交推送 + 注释 tag + Release + 事后沉淀」流程；新增工具要调第一/二位版本号。
- 提交时排除 `window_service.rs` 与 `.codex/`。
