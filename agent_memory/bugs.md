# 活跃风险与约束

- **工作树常态**：`M src-tauri/src/window_service.rs`（既有 rustfmt 噪声）与 `?? .codex/`（Codex 遗留本地配置，DSH 不读取）长期存在；除非用户授权，不处理、不回滚、不提交、不复制到别处。
- **同一 App ID 冲突（已解决，待发版验证）**：此前开发版与安装版共用 identifier 与数据目录（当前指向 `D:\轻量化工具集`），会抢单实例、读写同一份真实数据。现 debug 构建在 `run()` 中把 identifier 改为 `com.the.lightweight-toolset.dev`，数据/WebView2/单实例/通知 AUMID/开始菜单快捷方式全部隔离；开发版自启固定关闭且不写设置；正式版标识保持不变以保留用户数据。
- **窗口置顶边界**：管理员目标受 UIPI 限制可能返回 Win32 5；监护不覆盖开发终端关闭、系统同时杀死监护进程、关机等场景（正式版任务栏结束与托盘退出已验收）。
- **测试串行**：`input_monitor` 等测试共享全局按键状态，并行 `cargo test --lib` 会偶发修饰符断言失败；统一用 `--test-threads=1`。
- **格式化**：全仓 `cargo fmt --check` 与严格 clippy 有既有告警，不作为准入条件；也不要对仓库 Rust 文件跑 `rustfmt`（无 `rustfmt.toml`，默认 100 列会整文件重排）。

# 已知限制（已接受）

- 分词提取：序号 `1.` `2.` 会带句点成词、`×N` 只剩 `N`、单字虚词（的/和/就）保留；用户已确认接受，不再改动。
- `Ctrl+Alt+Del`、`Win+L` 等内核/安全层处理的热键无法捕获或拦截。
- 浮窗只出现在主显示器；UAC 安全桌面与 System 完整性进程的按键不可见。
- **双版本并存时全局快捷键冲突（已接受，用户选 D）**：Windows `RegisterHotKey` 系统独占，安装版与开发版同用默认 `CTRL+ALT+M`（软件级「呼出主窗口」）时，后启动的一方注册失败并在控制台打印 `app hotkey registration skipped ... HotKey already registered`。影响仅该快捷键；托盘、再次启动图标仍可唤起。用户决定不处理，仅需知晓。`register_app_hotkeys` 目前「一条失败即整批中止」，当前只有 1 条软件快捷键，暂无连带影响；若将来增加多条，应改为逐条注册并汇总失败。

# 环境坑（本机）

- **npm 路径**：Git Bash 的 PATH 里没有 `npm`（只有 `node`）；用 `E:\devtools\node\v24.12.0\node.exe ...\npm-cli.js` 或 PowerShell 执行，否则会把环境缺失误判成构建失败。
- **沙箱内 `vite build`**：DSH Windows 沙箱下 esbuild 子进程管道会 `spawn EPERM`；属沙箱限制，放宽策略或改用 `tsc --noEmit` 即可。
- **测试临时目录**：系统 `%TEMP%` 对测试子进程返回 `PermissionDenied`；跑测试前把 `TEMP`/`TMP` 指向 `src-tauri/target/test-tmp`。
- **`npm run tauri dev` 不可用（直接原因）**：cargo 用 CreateProcess 启动 `target\debug\lightweight-toolset.exe`，`requireAdministrator` 清单返回 `os error 740`（never executed），Vite 子进程随之退出。正确做法：先 `npm run dev`，把 debug exe 复制到 `%TEMP%` 后 `Start-Process -Verb RunAs` 启动（复制到 `%TEMP%` 同时规避工作区沙箱写限制，之前表现为 `保存存储定位文件失败: 拒绝访问 (os error 5)`）。
- **布局陷阱**：本机缩放 135%，主窗口 912×598 的 CSS 视口只有 676×443；固定高度弹窗要用 `100vh` 兜底，不能按逻辑像素硬算。
- **Release 资产中文名被截断**：直接用中文文件名上传（`gh release create` 路径含中文）时，GitHub 资产名会丢掉中文段，变成 `_0.6.x_x64-setup.exe`；0.6.2–0.6.4 均复现。做法：上传前复制为 `LightweightToolset_<版本>_x64-setup.exe` 英文名再上传，上传后用 `gh release view` / `gh api` 核对；已上传的可 `gh api -X PATCH repos/<owner>/<repo>/releases/assets/<id> -f name=...` 修正（内容与 SHA-256 不变）。

# 已确认经验

- 2026-11-11：**OCR 模型路径与内存**。release 里不能保留「仓库资源目录」兜底，否则在开发机上安装测试会假通过（实际资源没进安装包也照样能识别）；模型要外置到 `<install>/ocr` 并按需加载，`include_bytes!` 会让 30MB 映射页常驻，达不到「识别完回落基线」。
- 2026-11-11：**PowerShell 5.1 改 `settings.json` 会毁配置**。PS 读无 BOM UTF-8 时按 ANSI 解码、写回还加 BOM；开发版 `windowTitle` 与正式版配置各踩一次。脚本改配置一律用 node。
- 2026-11-11：**注入测试的 `Ctrl+Esc`**。按住 Ctrl 时按 Esc 等于 `Ctrl+Esc`，会弹开始菜单；取消拖放前必须先松开 Ctrl。
- 修饰符双计数根因是 Raw Input 通用 VK 与 Hook 分侧 VK 不一致；共享入口规范化后再去重。
- High 游戏输入根因是完整性隔离；`v0.5.0` 起整体提权解决，原有 elevated helper 已删除。
- 窗口置顶被强制终止后无法靠主进程回调清理，独立 watchdog 是必要能力，但其启动不能阻塞基础页面状态。
- 2026-11-02：「系统/他软件热键在本软件前台时不提示」根因不是权限，而是 Chromium 前台抑制 + 按键被抢先注册；解法是独立非 Chromium 采键子进程，Raw Input 不能替代（已实测）。
- 2026-11-02：快捷键回调运行在主线程，若在其中加锁并操作窗口，会与前端 2 秒快照轮询形成环形等待、整窗卡死；回调一律丢独立线程，快照接口先拷贝再查注册表。
- 2026-11-02：浮窗不要按字符估宽再动态改窗口（会被后续显示的估算值覆盖，表现为闪一下又变窄）；固定宽窗口 + `width: max-content` 才稳。
- 2026-11-02：条目 id 用毫秒时间戳会撞车，React `key` 重复会导致列表重复渲染、编辑/删除打错条目；改用「时间戳-自增序号」并在加载时修复历史数据。
- 2026-11-02：带淡出动画的弹窗在提交失败时必须复位 `closing`，否则透明遮罩覆盖全窗、看起来像卡死。
- 2026-11-02：录制状态若绑定「弹窗打开」而非「录制框聚焦」，在文案框打字会被当成录制内容回填键值。

- 2026-11-04：「开机自启开关开着但开机不启动」根因是提权清单与 Run 键机制冲突——Explorer 处理 Run 条目只做 CreateProcess，`requireAdministrator` 的 exe 返回 `ERROR_ELEVATION_REQUIRED` 后被静默丢弃（Shell-Core 事件 9708 PID 0 + UAC Operational 事件 1 可复现）。Run 键条目本身、StartupApproved 启用态、UAC 静默提权策略均正常。修法：改用「登录触发 + HighestAvailable + InteractiveToken」计划任务，`ExecutionTimeLimit=PT0S`，并清理旧 Run 值。
- 2026-11-04：开发版隔离不要靠改数据路径或换端口，直接改 `identifier` 即可——Tauri 的配置/数据目录、WebView2 数据、单实例互斥量与窗口类、我们自己的 AUMID 与开始菜单快捷方式都从 identifier 派生。

- 2026-11-06：截图覆盖层拖影根因是「只贴脏矩形 + `InvalidateRect` 整窗」——未被绘制的区域会被 `EndPaint` 标记为已校验，旧像素永久留在屏幕上；另外十字线/白框曾越出面板 1 px，把脏像素留在后备缓冲。修法：按「旧帧 ∪ 新帧」重建脏区、所有绘制收在 `frame_rect` 内。整帧重建虽然绝不残留，但 2560×1600 每帧两次 16 MB BitBlt 在 144 Hz 上明显掉手感，只作备选。
- 2026-11-06：覆盖层按键会被中文输入法吞掉——按过 Shift 后，`C` 到达的是 `VK_PROCESSKEY(229)`。解法：窗口创建后 `ImmAssociateContext(hwnd, NULL)` 解绑 IME。**做文字标注时需要在文本输入控件里重新接回 IME**，不能沿用全局解绑。
- 2026-11-06：截屏首帧延迟优化经验——抓屏直接 `BitBlt` 进 DIB 段（免 `GetDIBits` + 整图拷贝）、压暗用一次 `AlphaBlend`（黑源 + 常量 alpha）替代逐像素循环（debug 构建下逐像素极慢）。优化前 526 ms，优化后约 101 ms。
- 2026-11-06：本会话里测试二进制放在工作区 `target` 下运行时无法写 `%TEMP%`（沙箱拒绝），复制到 `%TEMP%` 再运行即正常；与既有「测试临时目录」条目不冲突，是同一限制的另一种表现。

# 历史记录

- 已关闭的 P0、已移除的 helper、旧通知机制与 Chromium 丢钩子的完整证据链，见 `archive/2026-11-02-bugs-compaction.md`。
- 2026-11-08：截图配置项最终形态：`screenshotManualSaveDir`（覆盖层「保存」按钮初始目录）+ `screenshotQuickSaveDir`（`Ctrl+S` 目录）+ `screenshotPalette`（8 色 `#RRGGBB`）。默认目录是 `<存储目录>/screenshot/Screenshots`，**不再回落图片文件夹**；旧键 `screenshotSaveDir` 已被 serde 忽略，下次保存设置后自然消失。
- 2026-11-08：工具页设置项要跟着工具走，不要塞进全局设置；「截图查看」已取消，不要再加回来。
- 2026-11-08：`SHBrowseForFolderW` 想指定初始目录必须给 `BROWSEINFOW.lpfn` 回调，在 `BFFM_INITIALIZED` 里 `SendMessageW(hwnd, BFFM_SETSELECTIONW, 1, 字符串指针)`；`lParam` 只带指针，字符串要在调用期间保持存活。
- 2026-11-08：开发版测试前必须确认 Vite 1420 返回 200 且进程来自 `%TEMP%\lwt-dev`，否则会出现“页面空白还盲测热键”的假象。

## 贴图/COM：手写 vtable 释放接口会崩溃（2026-11-08）

- 现象：`SHCreateShellItemArrayFromIDLists` 拿到 `IShellItemArray*` 后，用自写函数按
  `vtable[2]` 调 `Release`，进程必定在超级拖放结束时报 `0xc0000005`（事件日志 offset 固定）。
- 修法：改用 `windows::core::Interface::from_raw` 构造类型化接口再 `drop`，由生成代码释放。
- 教训：windows-sys 只有原始指针，跨 FFI 的对象生命周期交给 `windows` crate 的类型化包装，
  不要自己算 vtable 槽位。

## 贴图超级拖放：两个必踩的坑（2026-11-08）

- `SHDoDragDrop` 在未做 OLE 初始化的线程上直接返回 `0x800401F0 CO_E_NOTINITIALIZED`。
  需要 `OleInitialize`，只调 `CoInitializeEx` 不够。
- `SHDoDragDrop(hwnd, pdata, ...)` 的 `pdata` 必须是 `IDataObject*`；把 `IShellItemArray*` 传进去会返回
  `0x80004001 E_NOTIMPL`，表现为「只生成了临时文件、拖放根本没开始」。
  正确做法：`IShellItemArray::BindToHandler(None, &BHID_DataObject)` 取出数据对象再交给 `SHDoDragDrop`。
- 判断是否真的开始拖放：`SHDoDragDrop` 返回 `0x00040100`（`DRAGDROP_S_DROP`）且 `effect` 非 0。

## 贴图超级拖放：拖拽图像与目标拒收（2026-11-08）

- 现象：能发起拖放，但跟随光标的是系统文件缩略图，网页上传区什么都不接收。
- 原因：数据对象只有 `CF_HDROP`（文件）；很多接收方只吃位图格式。
- 修法：自定义 `IDataObject` 同时给 `CF_DIB` + `CF_HDROP`；`IDragSourceHelper::InitializeFromBitmap` 设拖拽图像。
- 关键细节：`InitializeFromBitmap` 会通过 `IDataObject::SetData` 把 `DragImageBits` 写回，
  `SetData` 返回 `E_NOTIMPL` 时整个调用会失败（`InitializeFromBitmap` 直接返回 `E_NOTIMPL`）。
  必须存下这块 HGLOBAL，并在 `GetData`/`QueryGetData`/`EnumFormatEtc` 里透出，拖放结束后释放。

## 贴图超级拖放：缩略图看不见（2026-11-08）

- 现象：拖放数据正常被接收，但拖动过程中没有跟随光标的缩略图。
- 原因：`SHDRAGIMAGE.hbmpDragImage` 用的是截屏 DIB 直拷，alpha 字节全为 0；
  系统按 alpha 合成拖动图像 → 整张完全透明。
- 修法：拷进拖动位图后把每个像素的 alpha 置 255；拖动图像不需要透明。

## 贴图超级拖放：系统 DragImage 不显示（2026-11-08）

- 现象：`IDragSourceHelper::InitializeFromBitmap` 成功（SetData 收到了 DragImageBits），
  但拖动时始终没有缩略图；日志显示 OLE **从不调用** `GetData(DragImageBits)`。
- 结论：不要依赖系统 DragImage。改为自己建一个 `WS_EX_LAYERED|WS_EX_TRANSPARENT|WS_EX_TOPMOST|WS_EX_NOACTIVATE`
  的跟随窗口，`IDropSource::GiveFeedback` 里按光标位置移动它，拖放结束销毁。
- 同时把 `SHDoDragDrop` 换成自实现 `IDropSource` + `DoDragDrop`，控制 `QueryContinueDrag`
  （Esc 取消 / 松开左键放下）。

## 贴图标注工具栏不能画在贴图窗口内（2026-11-09）

- 现象：把截图那套工具栏按 `Mode::Pin` 直接画进贴图窗口，主栏宽 627px；
  贴图窗口就是图本身，图比 627px 窄时右半边被裁掉（320×240 只能看到前 6 个按钮，对勾不可见）。
- 结论：用户选定「贴图窗口外的独立伴生小窗」（与 PixPin 一致）。实现要点：
  - 窗口类 `LightweightToolsetPinToolbar`，`WS_EX_LAYERED|TOOLWINDOW|TOPMOST|NOACTIVATE`；
  - 分层窗口直接用 `UpdateLayeredWindow` 呈现，不用 `WM_PAINT`；圆角与留白靠「哨兵色抠透明」：
    先把 DIB 填成哨兵 `B=3,G=2,R=1`，`toolbar::draw` 后仍等于哨兵即 alpha=0，其余 alpha=255；
  - 宽度不能只取主栏：二级栏（线型+线宽+填充+形状+8 色）约 808px 且与主栏左对齐居中，
    所以窗口宽必须满足 `W >= 2×内容宽 − 主栏宽`（实测 1017px）；
  - 定位：主栏贴贴图下沿 +8px，二级栏/下拉向下生长（主栏不跳位）；下方放不下整体翻到贴图上方；
  - 跟随：贴图窗口 `WM_WINDOWPOSCHANGED` 时重算位置与内容。
- 调试坑：合成输入脚本里 `SetCursorPos` 后紧接着 `mouse_event` 会偶发点击仍落在旧位置，
  等待要 ≥200ms；否则会误判成「点击没生效」。
- 补充（2026-11-09）：右键菜单「标注/隐藏标注」一直是灰的，原因是 `MenuEntry::item(text, grayed, checked)`
  的第二个参数仍传了 `true`（参数顺序是「置灰、勾选」，不是「可用、勾选」）。改为
  `item("标注", false, state.annotating)`、`item("隐藏标注", false, state.annotations_hidden)`。

## 事故：贴图超级拖放时崩溃，连带接收方（DSH）进程结束（2026-11-08）

- 版本：`778e0a4`（把拖拽图像重新交给系统 `IDragSourceHelper` 的那一版）。
- 现象：用户从贴图按住 Ctrl 拖出，还没看清落点就崩；报错不止我们，连目标程序（DSH）也被结束。
- 对照：上一版 `96655a6`/`0c221a9`（自绘 ghost 跟随窗口、**不启用** helper）能正常拖放，只是看不见缩略图。
- 处置：`8c97d46` 回退 `778e0a4`，回到 ghost 版；**拖放暂时保留这一版**（可拖、无缩略图）。
- 首要嫌疑（未验证）：`IDragSourceHelper` 写回的 `DragImageBits` HGLOBAL 生命周期——我们在拖放结束 `GlobalFree`，若目标也释放同一块即双重释放；其次是 `CF_DIB` 内容的兼容性。
- 待办（下一轮排查，必须带证据）：① 查 Windows 事件日志两侧的崩溃模块与异常码（我方 + DSH）；② 若重现，先用“只给 CF_HDROP、不给 CF_DIB”和“只给 CF_DIB”两组隔离；③ 拖拽图像 HGLOBAL 改为不再由我们释放（交给 OLE 的 `ReleaseStgMedium` 语义）。

## REA 结论：PixPin 的拖放实现（2026-11-08）

- `D:\PixPin\PixPin.exe`（SHA-256 `d2bbbb72…1abf`）**自己不做拖放**：ole32 仅导入 `CoInitializeEx/CoInitialize/CoUninitialize/CoCreateInstance/CreateStreamOnHGlobal/GetHGlobalFromStream`，无 `DoDragDrop`，无 `RegisterDragDrop`。
- 真正的拖放在 `D:\PixPin\platforms\qwindows.dll`（SHA-256 `3333ba24…2b85`，1.4 MB，Qt 平台插件）：ole32 导入完整——`DoDragDrop`、`OleInitialize/OleUninitialize`、`RegisterDragDrop/RevokeDragDrop`、`ReleaseStgMedium`、`OleSetClipboard/OleGetClipboard/OleFlushClipboard/OleIsCurrentClipboard`、`CoLockObjectExternal`、`CoCreateGuid`；Shell32 侧有 `SHCreateItemFromParsingName`/`SHCreateItemFromIDList`。
- 即：PixPin 走的是标准 OLE 拖放（Qt 的 `IDataObject` + `IDropSource` + `DoDragDrop`，拖拽图像由 `CoCreateInstance(CLSID_DragDropHelper)` → `IDragSourceHelper::InitializeFromBitmap`），**不是**自绘跟随窗口。
- 重要推论：拖拽缩略图由**目标端**绘制（`IDropTargetHelper` 去取 `DragImageBits`）。实测我们的数据对象在整次拖动中只被问过 `EnumFormatEtc(GET)`、`QueryGetData(CF_HDROP)`、`GetData(CF_HDROP)`，**从未被问 `DragImageBits`**——所以把图像交给 helper 也不能保证浏览器类目标显示，这解释了“机制相同但没有缩略图”。
