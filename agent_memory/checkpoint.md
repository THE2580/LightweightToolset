# Goal
复刻 PixPin 截图工具（LightweightToolset 新工具），当前阶段：截图工具页配置（两个保存目录 + 8 色预设）待验收 → S3 贴图。

# Current Focus
工具页配置改造已完成并构建通过；**开发版与 Vite 已关闭（收工）**，工作树未提交。

# Remaining
1. 下次开工先重启 Vite 与开发版
2. 用户验收工具页配置（手测覆盖层保存按钮初始目录、Ctrl+S 目录、改色生效）
3. 验收后提交本批（排除 `window_service.rs`、`window_pinner.rs`、`.codex/`）
4. 之后按 `progress.md` 的「截图配置待办」推进：近期项（文件名模板、未选区域颜色、放大镜网格线、辅助线），再进 S3 贴图
5. 不做：「截图查看」、动态截图、UI 检测、多语言等（见 progress 的不做清单）

# Verification
- `cargo check` 通过（仅既有告警）；`npm run build` 通过；`cargo test --lib screenshot::` 11/11
- 工具页渲染已用 PrintWindow 截图确认
- **未验证**：覆盖层「保存」按钮初始目录、`Ctrl+S` 落盘（上次注入测试被提权控制台抢焦点，无效）
- 最近提交：`4c88fe1`、`1373ed8`、`010f025`

# Resume Here
1. 启动环境：`npm run dev` 起 Vite（确认 `http://localhost:1420` 返回 200）→ `cargo build`（若改过 Rust）→ 复制 debug exe 到 `%TEMP%\lwt-dev` → `Start-Process -Verb RunAs` 启动 → 核对进程路径来自 `%TEMP%\lwt-dev`。
2. 请用户验收工具页与覆盖层保存行为；有问题改 `src/App.tsx` 的 `ScreenshotToolPage`、`src/App.css` 的 `.screenshot-*`，或 `src-tauri/src/screenshot/{mod,overlay}.rs`。
3. 验收通过后 `git add` 明确路径提交；然后进入 S3 贴图或近期配置项。
