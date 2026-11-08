# Goal
复刻 PixPin 截图工具（LightweightToolset 新工具），当前阶段：截图工具的设置/辅助线批次已提交 → 下一步 S3 贴图。

# Current Focus
上一批（工具页标签 + 近期配置项 + 辅助线重构 + 选区翻转 + 设置层级）已验收并提交 `727dff0`，记忆补记 `cc66594`。工作树干净（只剩既有的 `window_pinner.rs`、`window_service.rs`、`.codex/`）。开发版与 Vite 正在运行。

# Remaining
1. S3 贴图：贴图窗口（置顶、拖动/缩放/滚轮）→ 贴图历史（`<存储>/screenshot/` 下 `.meta` + 索引）→ 填「贴图」标签
2. 贴图阶段设置项：锁定类型、双击动作、关闭确认、窗口阴影、历史数量、贴图快捷键组（恢复上次关闭/关闭所有/隐藏显示/鼠标穿透）
3. **待办（本工具完成后）**：其他有快捷键的工具逐个迁到各自工具页的「快捷键」标签
4. 不做：截图查看、动态截图（GIF）、UI 检测、多语言、主题/托盘图标自定义等（详见 `progress.md` 的「不做」清单）

# Verification
- `cargo test --lib screenshot::` 17/17；`cargo build`、`npm run build` 通过
- 用户已逐项验收：标签页、配置项、辅助线三态/颜色/宽度/虚线/无残留/跟手、选区翻转、设置层级
- 最近提交：`727dff0`（本批 feat）、`cc66594`（docs 补记）、`2183a2c`

# Resume Here
1. 环境：Vite `:1420` 与开发版 `%TEMP%\lwt-dev` 应在运行；重启流程：`npm run dev` → `cargo build` → 复制 `src-tauri/target/debug/lightweight-toolset.exe` 到 `%TEMP%\lwt-dev` → `Start-Process -Verb RunAs` → 核对进程路径。
2. 向用户复述 S3 贴图范围并等放行，再动手；每完成一块按固定动作重启开发版给用户验收。
3. 提交时排除 `window_service.rs`、`window_pinner.rs`、`.codex/`。
