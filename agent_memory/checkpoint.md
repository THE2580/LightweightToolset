# Goal
复刻 PixPin 截图工具（LightweightToolset 新工具）。贴图标注的工具栏、批 A（绘制/交互）、批 B（性能）、文字与序号均已验收并提交（`d32b36f`、`27c669d`、`4785eea`、`450c122`）。下一步是贴图文本选择（OCR）与贴图历史浏览。

# Current Focus

- **贴图文本选择（本地 OCR + 内存/交互/配置项，2026-11-10 第三轮，已实现待验收，未提交）**：
  - 引擎：`pure-onnx-ocr` 0.3.2 + PP-OCRv6 small（det 9.9MB / rec 21.2MB），纯 Rust tract 推理。
  - **内存策略（用户口径：识别完回到没有 OCR 时的水平）**：模型外置到 exe 同级 `ocr/ppocr_v6_small/`（不再 `include_bytes!`，否则映射页常驻）；单条 `lwt-ocr` 工作线程串行识别，**每识别完立刻 drop 引擎**，再调 `SetProcessWorkingSetSize` 把已释放页还给系统。实测：测试进程 idle 7.6MB → 峰值 110–118MB → 释放后 1.6–2.2MB；三张开/关循环无累积。
  - 文字层：按行识别，行内按字符权重（CJK 1.0 / ASCII 0.55）拆字符单元，选区和复制都按字符下标。
  - 交互：①落图即预识别，识别中右键菜单用灰色「正在检测文本」顶替两项；②新贴图默认开启「文本可选择」；③只有文字区域划词，空白/图片区域拖动窗口、边缘 6px 优先缩放；④选区画进离屏高亮层、只刷新「旧选区 ∪ 新选区」；⑤点贴图内非文字区域清空选区。
  - 配置项：`screenshotPinAutoCopyText`（默认 **false**），文案「**选中后自动复制**」——右键菜单与工具页「贴图」标签各一项、状态同步并持久化；**同时作用于划词和「选择所有文本」**：关闭时两条路径都只选中不写剪贴板，用户自己 `Ctrl+C`。
  - 性能：`[profile.dev.package."*"] opt-level = 2`（debug 依赖也优化）；540×220 稳态约 0.9s（含每次重新加载模型）。

- **贴图文本选择（本地 OCR，2026-11-10，已实现待验收，未提交）**：
  - 引擎：`pure-onnx-ocr` 0.3.2 + PP-OCRv6 small（det 9.9MB / rec 21.2MB），纯 Rust tract 推理、无 C/C++ 运行时；模型 `include_bytes!` 进二进制（开发版从 `%TEMP%` 运行，外部资源目录会失效）。
  - 模型来源：`huggingface.co/PaddlePaddle/PP-OCRv6_small_{det,rec}_onnx` 的 `inference.onnx` + `inference.yml`（yml 内含字典），放在 `src-tauri/resources/ocr/ppocr_v6_small/{det,rec}/`。
  - 文字层结构：识别按「行」返回，行内按字符权重（CJK 1.0 / ASCII 0.55）均分成字符单元，选区按字符下标范围；复制用字符拼接，跨行换行。
  - 交互四项（用户口径）：①贴图落图即预识别，识别中右键菜单用灰色「正在检测文本」顶替两项；②新贴图默认开启「文本可选择」；③只有点在文字上才划词，空白/图片区域拖动窗口，窗口边缘（6px）优先缩放；④选区画进离屏高亮层、只刷新「旧选区 ∪ 新选区」修闪烁。
  - 性能：`[profile.dev.package."*"] opt-level = 2`（否则 debug 推理慢一个数量级）；引擎加载 178ms，540×220 首轮 1.27s / 稳态 0.43s，1888×897 首轮 4.5s。
  - 体验上限：复杂版面/极小字号仍可能出错；首次识别有 1–2s 等待（有「正在检测文本」占位）。

- **贴图文本选择（系统 OCR，2026-11-10，已实现待验收，未提交）**：
  - 方案：`Windows.Media.Ocr`（无第三方依赖、包体不增），新增 `screenshot/ocr.rs`；`SoftwareBitmap` 由 BGRA 缓冲构造（alpha 统一置 255），小图先 2× HALFTONE 放大再识别，包围盒按倍数缩回。
  - 接入：右键「选择所有文本」「文本可选择」解禁；「文本可选择」为勾选项，勾上时后台线程识别（`WM_APP+3` 回传结果）；拖选按阅读顺序命中词、蓝色半透明高亮、松手自动复制并提示「已复制 N 字」；Ctrl+C 复制选区；「选择所有文本」全选并复制；识别中/失败/无文字都有提示；与标注模式互斥；旋转/翻转后 OCR 结果作废。
  - 验证：单测 46 passed（含 OCR 真实识别探针 1 条 ignored）；注入实测拖选、全选、菜单状态、高亮对齐均通过。
  - 体验上限：12px 界面小字仍有错字，复杂版面一般——系统 OCR 的固有上限，已和用户确认先看效果。

- **批 A：贴图标注绘制问题（2026-11-09，已修，已验收，提交 `27c669d`）**：
  - 根因：pin 的草稿构造参数口径不对——`line_style(true)` 恒取形状档（线条被画成虚线）、`arrow_style()` 不分工具（直线也带箭头）、马赛克强度恒传 `strength(false)`（用了模糊档）、路径型工具只传 `[起点] + 光标`（铅笔/荧光笔/折线都退化成两点，铅笔荧光笔甚至不成图）。
  - 修法：新增 `draft_item(&PinState)` / `finish_draft(&mut PinState)` 统一草稿构造与收尾，口径与 `overlay.rs` 一致；`PinState` 增加 `draft_cursor`（图像坐标的预览光标点）。
  - 折线改为与截图一致的交互：单击逐点落下折点，双击/右键收尾（画到一半时右键先收尾、不弹菜单）。
  - 实测通过：直线/箭头拖拽预览为实线、箭头只在箭头工具出现；折线三点成形；铅笔、荧光笔可涂画；马赛克拖拽预览与松手结果一致。
- **批 A 补充（2026-11-09，已修，已验收，提交 `27c669d`）**：
  - 折线右键/双击收尾不再把收尾点补成折点（`finish_draft(state, with_cursor)`，右键、双击传 `false`）。
  - 选中任意标注工具时禁用右键菜单与滚轮缩放（未选工具时保持原行为）。
  - 工具栏提示条：未展开二级栏时窗口底部预留 `TB_TOOLTIP=30`，一级栏悬停提示不再被裁掉。
  - 支持 `Ctrl+Z` / `Ctrl+Y` 撤销/重做标注（标注模式下拦截，不关闭窗口）。
  - 实测：`menu_after_tool=0`、`menu_no_tool=1`、滚轮后贴图矩形不变、铅笔提示条可见、undo 后线条消失 / redo 恢复、折线两点 + 右键只出一段。

- **贴图标注：文字与序号（2026-11-10，已验收，提交 `450c122`）**：
  - 序号：单击落号，`doc.next_serial()` 自增，样式沿用截图（圆底白字）。
  - 文字：贴图内的内联编辑（不建子窗口）——点一下出现输入框与光标，接管 IME（`ImmGetContext`/组合串/候选窗定位），回车或右键提交、Esc 取消、退格删字；提交后按 `doc.font_size()` 生成文字项，留存在图上。
  - 编辑期间临时 `SetForegroundWindow` + `SetFocus` 拿焦点（贴图平时不抢焦点）；退出标注/关闭窗口会取消编辑并销毁光标。
  - 验收过程中修掉两处：①只有第一个文字框能用输入法——提交/取消会解绑 IME 且只在创建时绑过一次，改为窗口创建时记下 `ime_context`、每次编辑重新挂回；②编辑中文字没有白描边——改为与 `annotation::draw` 相同的「四方向白 + 本色覆盖」。
  - 实测：序号连点出 1/2/3；连续多个文字框中英文输入与切换正常（用户实机验收通过）；编辑中与提交后的描边一致。

- **批 B：贴图标注性能（2026-11-09，已优化，已验收，提交 `4785eea`）**：
  - 测量（debug 构建，1200×800 贴图）：优化前 `paint` 平均 7.06ms / 最大 9.65ms，工具栏重绘平均 9.51ms / 最大 12.89ms（两者叠加接近 16.7ms 帧预算 → 掉帧）。
  - 改动：①工具栏位图缓存复用（不再每次新建 DIB）、哨兵处理改 u32 批量；②只有「悬停目标变化」才重绘工具栏（同一按钮内移动不重绘）；③标注分两层缓存——已落图标注 `items_surface` 只在内容变化时重画，草稿合成 `composite` 复用；④绘制时只刷新「旧草稿 ∪ 新草稿」脏区并按脏区换算 StretchBlt 源矩形，不再整窗重贴；⑤退出标注释放标注相关位图。
  - 优化后：`paint` 平均 **2.67ms**（最大 8.78，为首次整图合成）、工具栏重绘平均 **3.05ms**（且次数由 20 降到 14）。内存（1200×800 贴图）：创建贴图 +11.5MB、进标注 +4.3MB、画完第 1 个矩形 +3.6MB 后保持不变、退出标注 −4.1MB。

- **贴图标注工具栏（2026-11-09，已验收，未提交）**：
  - `pin.rs` 删掉临时迷你工具栏（`PinAction`/`pin_toolbar_*`/`draw_pin_toolbar`），改为 `toolbar::layout/draw/hit` + `Mode::Pin`，动作分发照抄 `overlay.rs`。
  - 用户否决「画在贴图窗口内」（窗口比工具栏窄时被裁切，实测 320×240 只剩前 6 个按钮、看不见对勾），选定**贴图窗口外的独立伴生小窗**（像 PixPin）。
  - 小窗：`LightweightToolsetPinToolbar`，`WS_EX_LAYERED|TOOLWINDOW|TOPMOST|NOACTIVATE`，Sentinel 抠透明 + `UpdateLayeredWindow`；主栏贴贴图下沿 8px、二级栏与下拉在其下方生长；下方放不下整体翻到贴图上方；宽度按 `2×内容宽 − 主栏宽` 计算（二级栏比主栏宽，约 1017px）。
  - 交互：Esc 标注时只退出标注；无工具时拖动仍是移动窗口；删除模式点画布删除；滚轮落在数值项调数值、其余在画布上缩放。
  - 标注合成缓存（图像分辨率 + `annot_dirty`），鼠标悬停只重贴不重算。
- 拖放：`8c97d46` 已回退到 ghost 版（能拖、无缩略图）；`778e0a4` 崩溃事故见 `bugs.md`，根因待查。

# Remaining
1. 贴图文本选择已实现待验收；验收后提交（`Cargo.toml`、`screenshot/mod.rs`、`screenshot/pin.rs` + 新增 `screenshot/ocr.rs`，以及 `agent_memory/`）。
2. 贴图标注补：马赛克/模糊在贴图内还没有实现（工具栏能选中）。
3. 贴图历史浏览界面（用户已明确暂缓）。
4. 其他有快捷键的工具逐个迁到各自工具页的「快捷键」标签。
5. 不做：截图查看、GIF、UI 检测、多语言、主题/托盘图标自定义（见 `progress.md`）。

# Verification
- 构建：`cargo check` / `cargo build` / `npm run build` 全通过（4 条既有告警）；`cargo test --lib -- --test-threads=1` **45 passed / 0 failed / 2 ignored**。
- 内存：OCR 测试进程 idle 7.6MB → 峰值 110–118MB → 释放后 1.6–2.2MB；App 三张开/关：开 74–82MB WS、关 71–72MB WS，无累积。
- 行为：默认不自动复制（剪贴板保持 MARKER）；开关打开后划词与「选择所有文本」都会复制（后者实测 `Select All Test ABC 123`）；点空白清选区；工具页开关与 settings.json 双向同步；菜单文案「选中后自动复制」。
- 截图：`%TEMP%\lwt-t7-menu.png`、`lwt-t7-all-off.png`、`lwt-ui-pin-tab.png`、`lwt-ocr-t5-selected.png`、`lwt-ocr-t5-cleared.png`。
- 未验证：旋转/翻转后重新识别、超大图耗时、安装包实际体积（模型 30MB 外置）。

# Resume Here

0. **开发环境已重启（2026-11-10 22:48）**：HEAD `fde2d58`；工作树 = 既有三项 + 贴图文本选择（含第四轮）改动，**未提交**；dev pid 18692（`%TEMP%\lwt-dev`，模型在 `%TEMP%\lwt-dev\ocr`），Vite `:1420` 在跑；`screenshotPinAutoCopyText=false`（默认）。
1. **下一步：等用户验收第四轮后提交**（全选与划词共用「选中后自动复制」开关；文案已改名）。
2. 提交 `feat: select and copy text in pin window via local OCR`，含 `src-tauri/resources/ocr/`（30MB）与 `agent_memory/`；提交前 `npm run build` + `cargo check` + `cargo test --lib -- --test-threads=1`。
3. 坑（重要）：**不要用 PowerShell 改 `settings.json`**——PS 5.1 读无 BOM 的 UTF-8 会按 ANSI 解码，写回时带 BOM 且把中文变乱码（`windowTitle` 已被坑过一次，用 node 改回来）。要用脚本改配置就用 node。
4. 重启环境固定流程：`npm run dev` → `cargo build` → `cmd /c start` 调 `%TEMP%\lwt-elevate.ps1`（内部 `lwt-dev-launch.ps1`：停旧实例、复制 exe 与 `resources\ocr` 到 `%TEMP%\lwt-dev`、提权启动）→ `%TEMP%\lwt-dev-shot8.ps1` 截图核对。
3. 已交付并验收的贴图能力：工具栏（复用截图工具栏 + 伴生小窗，`d32b36f`）、绘制与交互（`27c669d`）、性能（`4785eea`）、文字与序号（`450c122`）。
4. 用户已明确暂缓：贴图历史浏览界面。待办：其他有快捷键的工具逐个迁到各自工具页的「快捷键」标签。
5. 注入测试注意：临时改开发版热键用 `%TEMP%\lwt-hk-patch.js unique|restore`，测完必须 restore 并重启；输入注入里 `SetCursorPos` 后等 ≥200ms 再 `mouse_event`；**所有 .ps1 测试脚本必须 ASCII-only 或带 BOM**，否则 PowerShell 5.1 解析失败。
6. 提交时排除 `window_service.rs`、`window_pinner.rs`、`.codex/`；提交前 `npm run build` + `cargo check` + `cargo test --lib screenshot:: -- --test-threads=1`。
