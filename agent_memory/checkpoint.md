# Goal

- 在 LightweightToolset 内新增「截图」工具（PixPin 行为复刻）：抓屏 → 覆盖层选区 → 标注 → 复制/保存 → 贴图。
- 当前已提交：`c0ef236 feat: add screenshot tool with native capture, magnifier and color picker`（S1 + S2-a）。

# Current Focus

- 覆盖层：原生 Win32 窗口（非 WebView），三张 DIB 段（原图/压暗/后备），按「旧帧 ∪ 新帧」脏区重建。
- 已实现：区域截图、裁剪保存（`图片\LightweightToolset\Screenshots`）、CF_DIB+PNG 剪贴板、放大镜（240px、网格、蓝色十字线、坐标、HEX/RGB、`C` 取色退出、`Shift` 切换）、选区标签（起点坐标 + 尺寸）。
- 关键常量：`MAG_CELL=16`、`PANEL_WIDTH=MAG_SIZE+2`、字体 17 px；`alpha=115` 压暗。

# Remaining

1. ~~S2-b~S2-e~~ 已完成并提交：标注底座、六种形状、正圆、马赛克/模糊/荧光笔、文字/序号（内联编辑、中文输入、输入框样式）。下一步 S2-f 浮动工具栏/图层。
2. S2-c 形状（椭圆/箭头/折线/铅笔）、S2-d 马赛克/模糊/荧光笔、S2-e 文字/序号、S2-f 工具栏与图层。
3. S3 贴图窗口（置顶/缩放/透明度/穿透）与历史。
4. 多屏已由用户决定移出范围，不再测试或优化。

# Verification

- `cargo test --lib` 23/23（测试二进制需复制到 `%TEMP%` 运行）。
- 热键→覆盖层：单屏中位 101 ms（修复前 526 ms）。
- 折线拖动 56 步后「选区/面板之外的强调蓝像素」= 0。
- 端到端：拖选 600×350 → PNG 精确 600×350、剪贴板 CF_DIB+PNG。

# Resume Here

- `git log -1` 应为 `c0ef236`；工作树预期只剩 `M src-tauri/src/window_pinner.rs`（换行差异）、`M src-tauri/src/window_service.rs`（既有 rustfmt 噪声）与 `?? .codex/`。
- 启动开发版：`npm run dev` 起 Vite → 复制 `src-tauri/target/debug/lightweight-toolset.exe` 到 `%TEMP%\lwt-dev\` → `Start-Process -Verb RunAs`（控制台 `-WindowStyle Hidden`）。
- 开发版数据已预置为只启用 `screenshot`；`%APPDATA%\com.the.lightweight-toolset.dev\settings.json`。
- S2-b 起点：`src-tauri/src/screenshot/overlay.rs` 的绘制链路与 `mod.rs` 的 `run()`；新增 `annotation.rs`（对象模型/命令栈）与工具栏绘制。
