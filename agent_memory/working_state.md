# Scope

- 工具集已含七个工具：**截图与贴图**（第一版，`v0.7.0` 发布）、剪贴板、软件使用统计、按键使用统计、计时器、窗口置顶、快捷键提示。
- 截图与贴图范围：区域截图（原生覆盖层/放大镜取色/辅助线/压暗）、12 种标注与二级栏、贴图窗口（拖动/缩放/锁定/穿透/阴影/右键图像处理）、贴图标注（窗口外伴生工具栏、文字与序号）、贴图文本选择（本地 OCR）、超级拖放与跟踪缩略图。
- 明确不在本版：标注拖动/文本再编辑、贴图历史浏览界面（暂缓）、系统级 DragImage 缩略图。

# Git / Working Tree

- 工作目录：`E:\DeepSeek_Harness\Workspace\LightweightToolset`；分支 `master`；HEAD **`b5775dc`**（`feat: release screenshot tool v0.7.0`），tag **`v0.7.0`** 已推送，Release 已发布；远程与本地一致，工作树只剩既有三项。
- 既有现场（不处理、不提交）：`M src-tauri/src/window_service.rs`（纯 rustfmt 噪声）、`M src-tauri/src/window_pinner.rs`（仅换行差异）、`?? .codex/`。
- 本机 git 走代理 `http://127.0.0.1:7897`；代理未启动时推送/gh 失败，需先让用户开启。

# Active Files

- `src-tauri/src/screenshot/mod.rs`：模块装配、`capture_config`（返回 `PinOptions` + 历史数量）、`pin_from_hotkey` / `restore_pin` / `close_all_pins` / `toggle_pins_visible` / `toggle_pins_passthrough`、`pin_hotkey_text()`、`stop()`；`set_pin_auto_copy_text` / `set_pin_auto_copy_persist` 转发。
- `src-tauri/src/screenshot/overlay.rs`：全屏覆盖层（三 DIB 段、脏区增量重绘、辅助线、放大镜、选区翻转、标注阶段与工具栏 `Mode::Screenshot`）；`OverlayOutcome.pin` 与 `request_pin()`。
- `src-tauri/src/screenshot/annotation.rs`：`Item`/`Document`/撤销重做（快照式 `begin_change`）/`hit_test`/绘制；`Item.bounds()` 已就绪，**尚无平移/换文本 API**。
- `src-tauri/src/screenshot/toolbar.rs`：`Mode{Screenshot,Pin}` 两套布局与命中、`Action::Done`/`Pin`、二级栏与下拉。
- `src-tauri/src/screenshot/pin.rs`（约 4400 行）：`screenshot-pin` 线程与指令通道；贴图 wndproc（绘制/拖动/缩放/滚轮/锁定/穿透/双击/关闭确认/右键菜单）；`PinShadow` 三态阴影（灰/蓝/金，`ShadowTone`）；旋转翻转；贴图标注（伴生工具栏小窗 `LightweightToolsetPinToolbar`）；文本选择（OCR 状态机、字符单元、离屏高亮层、`选中后自动复制`）；拖放（自绘 ghost + `ghost_thumbnail` 缩略图 + 自实现 `IDataObject`/`IDropSource`）；单测 9 条。
- `src-tauri/src/screenshot/ocr.rs`：PP-OCRv6 small + `pure-onnx-ocr`（tract 纯 Rust）；单工作线程串行识别、按需加载、用完 drop 引擎并 `trim_working_set`；模型从 exe 同级 `ocr/`（`#[cfg(debug_assertions)]` 才回落仓库）。
- `src-tauri/src/screenshot/{dib,save,clipboard_image}.rs`：DIB 段、PNG 编码与命名模板、剪贴板（图像 + 文本）。
- `src-tauri/src/settings.rs` / `lib.rs`：截图/贴图设置项（含 `screenshot_pin_auto_copy_text`，默认 false）、patch 校验、`set_pin_auto_copy_persist` 注入。
- `src-tauri/src/tools.rs`：`APP_HOTKEYS` 6 条（截图、贴图、恢复、关闭全部、隐藏显示、穿透），逐条注册并汇总失败。
- `src/App.tsx`：截图工具页五个标签（截图/贴图/保存/标注/快捷键）、`PIN_HOTKEY_IDS`、`选中后自动复制` 开关与 `screenshot-pin-auto-copy-changed` 事件同步。
- `src-tauri/resources/ocr/ppocr_v6_small/`：det/rec 的 `inference.onnx` + `inference.yml`（约 30MB，随安装包分发）。
- `src-tauri/tauri.conf.json`：`bundle.resources` 把 `resources/ocr` 映射到安装目录 `ocr/`。

# Verification

- 2026-11-11 `v0.7.0` 发版（已发布）：
  - 发布前：`npm run build` / `cargo check` / `cargo test --lib -- --test-threads=1`（48 passed / 2 ignored）；`npm run tauri build` 成功。
  - 安装验证：静默安装覆盖 `D:\轻量化工具集`；`ocr/ppocr_v6_small/{det,rec}/inference.onnx` 落位、exe 0.7.0（41,409,536 字节）；`Ctrl+Alt+A` 截图 → `Ctrl+Alt+P` 贴图 → OCR `ocr.ready chars=255 lines=15`、菜单恢复「选择所有文本」，证明 release 从安装目录加载模型成功。
  - 验证后已还原正式版 `settings.json`（截图工具保持关闭）并重启安装版；测试产生的 `screenshot/` 数据已删除。
  - 发布：推送 `master`、tag `v0.7.0`、Release 资产 `LightweightToolset_0.7.0_x64-setup.exe`（名称完整、digest 与本地 SHA-256 一致）、README 与 About 更新到 0.7.0。
- 截图功能逐轮验证记录（S1/S2/S3、贴图标注、文本选择、缩略图、金色阴影）见 `archive/2026-11-11-screenshot-progress.md`。

# Blockers

- 无阻塞。项目处于已发版稳定状态（v0.7.0）。

# Next Action

- 当前无待办；等用户指定下一批。候选见 `progress.md`「待办」：标注拖动/文本再编辑（三档）、缩略图观感优化、其他工具快捷键迁移、贴图历史浏览（暂缓）。
- 恢复开发：`npm run dev`（:1420）→ `cargo build` → 用 `%TEMP%\lwt-elevate.ps1`（内部 `lwt-dev-launch.ps1` 重建 `%TEMP%\lwt-dev` 并复制 `resources\ocr`）→ `%TEMP%\lwt-dev-shot8.ps1` 截图核对页面。
