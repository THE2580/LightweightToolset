# Goal
复刻 PixPin 截图工具（LightweightToolset 新工具），当前阶段：本批已验收待提交 → S3 贴图。

# Current Focus
工具页标签页改造 + 近期配置项 + 辅助线重构（三组线 / 三态 / 双颜色 / 双线宽 / 自定义虚线 / 单次呈现无闪烁）+ 选区翻转 + 设置子项层级，全部已验收通过，正在提交本批。

# Remaining
1. 提交本批（排除 `window_service.rs`、`window_pinner.rs`、`.codex/`）
2. S3 贴图：贴图窗口（置顶、拖动/缩放）→ 贴图历史 → 填「贴图」标签
3. **待办（本工具完成后）**：其他有快捷键的工具逐个迁到各自工具页的「快捷键」标签

# Verification
- `cargo test --lib screenshot::` 17/17（含 3 个 resize 翻转单测）、`cargo build`、`npm run build` 通过
- 用户已逐项验收：标签页、配置项、辅助线三态/颜色/宽度/虚线、无残留、层级、选区翻转
- 最近提交：`727dff0`（本批：工具页标签 + 配置项 + 辅助线重构 + 选区翻转）、`2183a2c`、`4c88fe1`

# Resume Here
1. 提交后从 `progress.md` 的「贴图阶段」开始 S3，先复述需求等放行。
2. 贴图建议顺序：贴图窗口（置顶/拖动/缩放/滚轮）→ 贴图历史（`<存储>/screenshot/` 下 `.meta` + 索引）→ 「贴图」标签设置项。
3. 提交时排除 `window_service.rs`、`window_pinner.rs`、`.codex/`。
