# 2026-11-11 bugs 历史归档

本文件收存 `bugs.md` 中已关闭/已沉淀的详细事故记录（贴图 COM 与拖放、贴图标注工具栏等）。
主文件只保留当前有效风险、限制与结论摘要。

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
