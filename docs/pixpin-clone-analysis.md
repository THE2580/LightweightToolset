# PixPin 复刻分析（REA 取证版）

> 目标：把 `D:\PixPin` 的截图软件拆解到"可复刻"的粒度，并给出在 LightweightToolset 内注册为新工具的实现方案。
> 本文只记录**从产物取证得到的结论**；推断与未知项显式标注，不把猜测写成事实。

---

## 0. 结论摘要

1. PixPin 是 **Qt 5.15.2 + C++ 的原生 Win32 桌面程序**（非 Electron/非 .NET），主程序 `PixPin.exe` 约 26 MB，自研能力拆成 10 个 DLL，图像算法静态链接了 **OpenCV 4.8.0** 与 **ncnn** 推理引擎。
2. 核心机制不是"截屏 API 直出"，而是：**先冻结整块虚拟桌面为位图 → 全屏覆盖层显示这张冻结图 → 在覆盖层上做选区/标注 → 按选区裁切导出**。抓屏用 `QScreen::grabWindow`（GDI BitBlt 路径），鼠标指针**单独取、不烧录**。
3. 产品能力分四层：截图（区域/窗口/长截图/GIF）→ 标注（文字/几何/箭头/折线/铅笔/荧光笔/马赛克/橡皮/序号）→ 贴图（置顶、缩放、旋转、透明度、翻转、鼠标穿透）→ 文本（内置 OCR + 文本选择层）。
4. 复刻建议：在工具集内注册 `screenshot` 工具，**Rust 负责抓屏/裁切/编码/窗口，WebView 覆盖层负责选区与标注**（MVP 路线）；若覆盖层性能/DPI 不达标，再切原生 GDI 分层窗口。分 M1–M4 四期交付。
5. **明确不复刻**：账号登录、统计上报（`*.viewdepth.cn`）、自动更新、云服务；也不复制其字体/图标/OCR 模型等资产（合规，见 §7.9）。

---

## 1. 取证范围与方法

| 项 | 值 |
| --- | --- |
| 目标目录 | `D:\PixPin`（安装形态，非安装包） |
| 主程序 | `PixPin.exe`，26,024,448 字节，SHA-256 `d2bbbb72f4c31ed519239fc38c72708b6f7c3be33727ecfc8fec99f6a4441abf` |
| 文件时间 | 2023-11-29（主程序与自研 DLL） |
| 构建信息 | Qt 5.15.2、MSVC 2022 运行库、xmake（`C:/Users/runneradmin/AppData/Local/.xmake/...`）、OpenCV 4.8.0 静态、ncnn 静态 |
| 厂商 | 深圳深度图景科技有限公司（`Copyright 2022-2023 Shenzhen Shendu Tujing Technology Co., Ltd.`） |

取证手段：PE 导入/导出表、字符串与 RTTI 类型名、Qt 资源路径、SQLite 表结构、二进制元数据文件、Ghidra 12.1.4 反编译、运行期窗口枚举。**未**动态驱动其 UI（未点击、未注入、未改其配置）。

### 1.1 主要证据

| 证据 ID | 操作 | 证明了什么 |
| --- | --- | --- |
| `ev_7bca3b05…` | `inspect_pe_imports` PixPin.exe | 37 个导入 DLL；自研 DLL 的完整 C++ 导出符号；无 GDI32/DXGI/DWM 直接依赖 |
| `ev_c5fdd521…` | `inspect_pe_imports` UiSpy.dll | `AccessibleObjectFromWindow`(UIA)、`EnumDesktopWindows`、`DwmGetWindowAttribute`、`ChildWindowFromPoint` |
| `ev_7d3ec316…` | `inspect_pe_imports` QScreenshot.dll | `QScreen::grabWindow`、`QtWin::toHBITMAP`、`GetCursorInfo`/`DrawIconEx`、GDI32 `CreateCompatibleDC` |
| `ev_af6938b8…` | `procedure_pseudo_code` `QScreenshot::shotScreen` | 多屏并集→逐屏交集→抓取→按偏移拼接；光标矩形/位图单独保存 |
| `ev_d55c8836…` | `inspect_pe_imports` QLongScreenShot.dll | 3.5 MB，仅依赖 Qt/CRT/OpenGL32；图像算法静态链接 |
| `ev_c7e66f9f…` | `procedure_pseudo_code` `QLongScreenShot::addImage` | 首帧存储；后续帧匹配出 `overlap`；按方向扩画布后合成 |
| `ev_25925de8…` | `inspect_pe_imports` OwlKeyLog.dll | `SetWindowsHookExA`（低级键鼠钩子），无 Raw Input |
| `ev_9375b24c…` | `inspect_sqlite_database` PinWindowd.sqlite | 贴图历史表 `PinItem` / `ImageItem` 的完整字段 |
| `ev_e867808a…` | `binary_overview` | 14,232 个函数、9,235 个已定义字符串；9 个段 |

> 说明：Ghidra 内的自研类**没有符号**（无 PDB），按类名检索函数会返回 0 条；内部函数以 `FUN_xxx` 呈现。因此类级结论主要依据 RTTI/字符串/Qt 元对象，函数级结论只引用有导出符号的少数入口（`shotScreen`、`addImage`）。

---

## 2. 目标形态

### 2.1 模块构成

| 文件 | 职责（依据） |
| --- | --- |
| `PixPin.exe` | 主程序：`MainApplication`、`PixPin` 控制器、`ScreenShotWindow/View`、标注 `EditItem*`、`PinWindow*` 全家桶、`OcrRunnable`、`GifShot*`、`LongShot*`、`ZoomWidget`、`PointInfoWidget`、`ActionsBar`、`ShortcutManage`、`CloudManage`、`SingleApplication` |
| `QScreenshot.dll` | 抓屏：导出 `QScreenshot::shotScreen(QList<QScreen*>, QRect, bool, bool)` |
| `QLongScreenShot.dll` | 长截图拼接：`QLongScreenShot(ImgCatDirection, QObject*)`、`addImage(QImage)`、信号 `sigNewMatchResult(ImgCatResult)` |
| `UiSpy.dll` | 窗口/控件探测：`GetWinRectByPoint`、`GetChildWinRect`、`GetParantWinRect`、`SnapshotAllWinRect` |
| `OwlKeyLog.dll` | 全局键鼠钩子：`OwlKeyHook`，信号 `SigKeyDown/SigKeyUp/SigMouseMove/SigMouseDown/SigMouseUp/SigWheelRoll`，含 `setKeyHookEnable`/`setMouseHookEnable` |
| `SalmonActions.dll` | 截图工具栏：`ActionsBar`（加按钮/分隔/层，`sigWheelEvent`）、`MoveControl` |
| `PandaConfiguration.dll` | 配置框架：`PdConf::PdConfManage`、`ConfItem`、`ColorSelectButton`，支持 bool/int/string/color/快捷键/语言，JSON 序列化 |
| `QParametricCurve.dll` | 手绘/曲线拟合：`appendQPointF`、`setMaxError`、`activePath`/`storePath`（`QPainterPath`） |
| `KoiAnimation.dll` | GIF 编码/播放：`KoiAnimation::init/addFrameTimestamp/finish`、`KoiMovie` |
| `QIconfont.dll` | 图标字体初始化 |
| `RobinLog.dll` | spdlog 封装与全局 logger |
| `qxtglobalshortcut.dll` | 全局热键（`QxtGlobalShortcut::setShortcut/activated`） |
| Qt 5.15.2 + `qwindows` + `qsqlite` + `qwebp` 等插件 | 窗口/控件、SQLite、PNG/WebP/JPEG/TIFF/GIF 编解码 |

### 2.2 进程与窗口

- 单实例（`SingleApplication`，共享内存 + 锁，见其错误文案）。
- 常驻托盘（`QTrayIconMessageWindow`），主窗口可隐藏。
- 运行期枚举（当前实机）：仅剩隐藏的主窗口 `Qt5152QWindowPopupSaveBits`（标题 `PixPin`）与托盘消息窗口；说明其常态是"托盘常驻 + 按热键唤起"。
- 可自提权：日志出现 `restartAsAdmin`，配置项 `System.Run After Boot.subConf.RunAsAdmin`，托盘菜单含 `PixPin (Run As Admin)`。

---

## 3. 功能面（用户可见）

> 来源：字符串表、Qt 资源、INI/JSON 配置、SQLite 字段、RTTI 类名。标注"已确认"的为直接证据。

| 功能 | 证据 | 复刻优先级 |
| --- | --- | --- |
| 区域截图（鼠标拖选、多屏、DPI） | `shotScreen` 反编译 + `ScreenShotWindow/ScreenShotView/RectSelection` | M1 |
| 窗口/控件截图（悬停高亮） | `UiSpy` 导入 + `ShapeRectSelection` + `Only Window` | M2 |
| 形状自动识别/自动描边 | `Autotrace call`、`ShapeRectSelection`、`Detection Elements`、`UI Detection` | M2 |
| 标注：矩形/椭圆/箭头/折线/铅笔/荧光笔/马赛克/模糊/橡皮/文字/序号 | `markConfig.ini` 全量分组 + `EditItem*` 类族 | M1（文字与马赛克可后置） |
| 撤销/重做 | `TrigerEditItemUnredo`、`undo/redo : can not find item with id` | M1 |
| 放大镜 + 网格 + 坐标/颜色 | `ZoomWidget`、`PointInfoWidget`、`MagnifyingGridLines`、`RGB:/HEX:/C:/CMYK:` 复制提示 | M2 |
| 贴图（Pin To Screen） | `PinWindow*` 类族 + `Pin to screen` | M1 |
| 贴图变换：缩放/旋转/透明度/翻转/保持比例 | `.meta` 键 `WinScaleValue/WinRotateAngle/WinOpacity/WinKeepRatio/WinFlip` + `Opacity: %1%`、`Scale: %1%`、`Rotate to the Left/Right`、`Horizontal/Vertical Flip` | M1 |
| 贴图鼠标穿透/阴影/锁定 | `Mouse Penetration Enabled/Disabled`、`Show Window Shadow`、`Lock Pin Color`、`WS_EX_TRANSPARENT` 推断 | M2 |
| 贴图历史/恢复上次关闭 | `PinItem` 表 + `Restore Last Closed` + `Number of historical pin retained` | M1 |
| 长截图（纵向/横向） | `QLongScreenShot::addImage` + `LongShotWidget/LongImagePreview/LongShotDirCtrl` + `Long screenshot` | M3 |
| GIF 截图（调色板/抖动） | `GifShotRecord/View/Widget`、`KoiAnimation`、`Gif Encoder Pattle Type/Dithering` | M3 |
| 动态截图 | `Dynamic Screenshot`、`DynamicShotWidget` | 语义未确认，M4 再评估 |
| OCR 文字识别 + 文本选择层 | ncnn 字符串、`:/model/det_param|det_bin|rec_param|rec_bin`、`OcrRunnable`、`OcrTextItemLayer`、`ImageItem.ocrResult` | M3 |
| 文本选择/复制模式 | `Text Select Mode`、`Copy Selected Text`、`Selected All Text` | M3 |
| 快速保存（模板命名/冲突策略/通知） | `Save.QuickSave` 配置、`PixPin_$yyyy-MM-dd_HH-mm-ss$.png`、`whenConflict`、`Save Success` | M1 |
| 自动保存目录 | `/PixPinAutoSave/`、`ConfWidgetAutoSave` | M2 |
| 全局快捷键（截图/贴图/关闭全部贴图） | `qxtglobalshortcut`、`Shortcuts.Global Shortcuts.CloseAllPinShortcut = Alt+X` | M1 |
| 托盘菜单/开机启动 | `Taskbar Tray`、`Start automatically after boot`、`Run After Boot` | 复用工具集既有能力 |
| 多语言 | `language/zh-cn.qm` + `PdConf` 语言项 | 不单独做，跟随工具集 |

---

## 4. 关键实现（复刻重点）

### 4.1 抓屏：整块虚拟桌面 + 分区拼接

`QScreenshot::shotScreen(QList<QScreen*>, QRect rect, bool withCursor, bool)` 的反编译结论：

```text
若 rect 为空：
    rect = 所有屏幕 geometry 的并集
pixmap = QPixmap(rect.size())
painter = QPainter(pixmap)
for screen in screens:
    inter = rect ∩ screen.geometry()
    if inter 非空:
        tmp = screen.grabWindow(0, inter.x - rect.x, inter.y - rect.y, inter.width, inter.height)
        painter.drawPixmap(inter.topLeft - rect.topLeft, tmp)
painter.end()
若 withCursor:
    cursor = GetCursorInfo 系列 → 光标矩形 + 光标位图（单独保存到 this+0x10..0x30，不画进位图）
```

要点：

1. **坐标基准是虚拟桌面**（多屏可含负坐标），不是主屏。
2. `grabWindow` 的 `window=0` + 屏幕内偏移——Qt 内部走 GDI `BitBlt`（`QScreenshot.dll` 只导入 GDI32 的 `CreateCompatibleDC/SelectObject/GetObjectA/DeleteDC/DeleteObject` 与 `QtWin::toHBITMAP/fromHBITMAP`）。
3. **没有 DXGI Desktop Duplication、没有 Windows Graphics Capture、没有 PrintWindow**（主程序与 QScreenshot 的导入表均无）。这决定了：性能足够、实现简单，但 HDR/DRM 内容、部分 GPU 全屏窗口可能抓不到——复刻时同样取舍。
4. 光标是"数据 + 位图"两段，覆盖层自己画在鼠标位置，松手/移动时无需重抓屏幕。
5. 物理像素：Qt5 下 `grabWindow` 返回的是设备像素，`devicePixelRatio` 需用于换算逻辑坐标（类内有 `resetPixelRatio`）。复刻时必须显式定义"物理像素工作区 + 逻辑像素 UI"。

### 4.2 覆盖层与选区

类结构：`ScreenShotWindow`（顶层全屏）→ `ScreenShotView`（QGraphicsView）→ `TrigerGraphicScene` → `RectSelection`/`ShapeRectSelection`；配套 `ZoomWidget`、`PointInfoWidget`、`MouseIndicateItem`、`KeyBoardIndicateItem`、`NoRectSelectionMouseIndicatorLine`。

观察到的行为（字符串/类名支撑，未动态验证）：

- 覆盖层显示冻结图 + 选区外变暗（`Unselected Area Color When Screenshot` 可配）。
- 辅助线：`Show Auxiliary Lines`；窗口高亮：`Only Window`；悬停显示窗口/控件矩形（UiSpy）。
- 放大镜显示网格（可配线色）与像素坐标/颜色；`Shift` 切换 RGB/HEX。
- 覆盖层互斥：`ScreenShotWindow is already exist`。

### 4.3 标注模型与参数

`markConfig.ini` 是标注工具的**默认参数真相**（与 `EditItem*` 类一一对应）：

| 工具 | 关键参数 | 默认值（INI 实读） |
| --- | --- | --- |
| Text | 粗体/斜体/描边/描边色/字体/字号 | Bold=true, Outline=true, Arial 30 |
| Geometry（矩形/椭圆） | 线宽/线型/填充/PathMode | 12 / 1 / false / 2 |
| Arrow | 线宽/线型/PathMode/LineShape | 12 / 1 / 6 / 1 |
| BrokenLine（折线） | 线宽/线型/PathMode/LineShape | 12 / 1 / 4 / 0 |
| Pencil | 线宽/PathMode | 12 / 1 |
| Marker（荧光笔） | 颜色/线宽/填充/PathMode | 半透明绿 / 12 / true / 5 |
| Mosaic | 线宽/填充/PathMode/MosaicMode/强度区间 | 12 / true / 1 / 0 / 2–28（默认 5），模糊 5–30（默认 10） |
| Eraser | 线宽/填充/PathMode | 12 / true / 1 |
| Serial（序号） | 线宽/描边/描边宽/描边色/文本色 | 16 / false / 2 / 白 / 白 |

结构含义（推断自 `QParametricCurve` + `EditItemCustom/CustomShape`）：

- 手绘类工具先把鼠标点序列交给 `QParametricCurve::appendQPointF` 做**误差限内的曲线简化**（`setMaxError`），再生成 `QPainterPath`；`PathMode` 选择"直线/平滑曲线"，`LineShape` 选择箭头形状（直线/双边）。
- 标注是**对象化**的：每个标注是一个 item（可选中、改参数、撤销/重做），不是一次性画在图上。这决定了复刻必须做"标注对象模型 + 命令栈"，而不是直接往 canvas 上涂。
- 形状识别（`Autotrace call`/`ShapeRectSelection`）是可选增强：手画矩形/椭圆后自动吸附为标准形状，M2 再做。

### 4.4 贴图窗口（PinWindow）

- 类族：`PinWindowBase` + `PinWindowPixmap/File/Text/Color/MarkEdit/Route/Transfer/Menu/Animation` —— 说明贴图不只贴图片，还支持**文件、文本、颜色**四种内容类型，以及"拖拽到其他应用"的 transfer 路径。
- 持久化：`Data/*.webp` + 同名 `.meta`（Qt `QDataStream` 二进制），字段：`WinScaleVal`、`WinRotateAngle`、`WinOpacity`、`WinKeepRatio`、`WinFlip`、`SrcGeometry`、`PinWindowDataRelateFiles`、`OcrTextResults`。
- 历史库 `PinWindowd.sqlite`：

```sql
PinItem(id, type, createTime, closeTime, device, subItem, posX, posY,
        mimeText, markText, pinOnScreen, title, collect, saveBasename)
ImageItem(id, pinItem, width, height, srcWinClass, srcWinTitle, ocrResult, mainColor)
```

- 查询语句证明"历史贴图"语义：`pinOnScreen = true` 为当前屏上贴图；`pinOnScreen = 0 ORDER BY closeTime DESC LIMIT 200` 为历史；"恢复上次关闭"取 `MAX(closeTime)`。
- 交互：拖动、缩放、旋转、透明度、翻转、保持比例、鼠标穿透、阴影、锁定；`Close All Pin`（默认 `Alt+X`）。
- 复刻要点：贴图窗口必须是**无边框 + 置顶 + 不抢焦点 + 不进任务栏**的顶层窗口；若要"穿透"，用 `WS_EX_TRANSPARENT`（Tauri 用 `set_ignore_cursor_events(true)`），但穿透状态下无法再用该窗口接收拖拽——需要"锁定/解锁"切换。

### 4.5 长截图

`QLongScreenShot::addImage` 反编译结论：

```text
若首帧：保存为新图，初始化匹配引擎（方向、尺寸）
否则：
    (result, overlap, region) = engine.match(newImage)
    若 result 表示"匹配成功":
        若 direction == 0（纵向）: 新画布 = (旧宽, 旧高 + overlap)
        否则（横向）:              新画布 = (旧宽 + overlap, 旧高)
        engine.compose(新画布, 新图, 旧图, overlap)
    发出 sigNewMatchResult(result)
```

- `ImgCatDirection` 为构造参数（纵向/横向滚动）；`ImgCatResult` 表示匹配状态（成功/未匹配/到边界，具体枚举值未逐一确认）。
- 重叠量 `overlap` 由匹配引擎给出，画布增长量即重叠量——说明是"**增量拼接**"，不是整图金字塔。
- 引擎静态链接了 OpenCV/OpenGL（`wglGetProcAddress`、`CONCRT` 并行），匹配算法具体指标（NCC/SQDIFF/相位相关）**未确认**；复刻时用"窄条带上的归一化互相关 + 亚像素细化"即可达到同类效果。

### 4.6 OCR

- 模型以 Qt 资源内嵌：`:/model/det_param`、`:/model/det_bin`（检测，DBNet 类）、`:/model/rec_param`、`:/model/rec_bin`（识别，CRNN 类）、`:/model/ocr_text_dict`（字符集）。
- 推理：静态链接 ncnn（`ncnn::Net`/`Extractor`/`Layer` 全家桶）。
- 源码痕迹：`ThirdParty\QPixVision\src\ocr\TextRec.cpp`（第三方 OCR 组件）。
- 线程模型：`OcrRunnable`（可跑线程池）+ `OcrTextItemLayer`/`OcrGraphicsTextItem`（识别结果作为可选中文本图层），结果写回 `ImageItem.ocrResult`。
- 复刻建议：先用 Windows 自带 `Windows.Media.Ocr`（零模型体积、系统级中英文），做"识别→文本层→选择复制"；若需要版式/竖排/更高精度，再评估内置 PP-OCR ONNX（体积换精度）。

### 4.7 GIF 截图

- `GifShotRecord`（采集）+ `GifShotWidget/View`（录制 UI）+ `KoiAnimation`（编码，`addFrameTimestamp`/`finish`）+ `KoiMovie`（预览）。
- 编码参数暴露：`Gif Encoder Pattle Type`（调色板算法）、`Gif Encoder Dithering`（抖动）。
- 复刻：固定区域按 10–15 fps 抓帧，帧间做整帧比对去重，GIF 调色板量化 + 抖动；先做"停止录制后一次编码"。

### 4.8 窗口/元素识别

`UiSpy.dll` 的导入即为实现路径：

- UIA：`AccessibleObjectFromWindow` + `CoCreateInstance`（CLSID_CUIAutomation）→ 取元素边界与层级；
- Win32：`EnumDesktopWindows` + `IsWindowVisible` + `GetWindowLongW` + `GetClassNameW` + `ChildWindowFromPoint`；
- DWM：`DwmGetWindowAttribute(DWMWA_EXTENDED_FRAME_BOUNDS)` 去掉 Win10 的不可见边框，保证高亮矩形贴边。

### 4.9 全局热键与钩子

- 热键：`qxtglobalshortcut`（`QxtGlobalShortcut`，Windows 下 `RegisterHotKey` + 隐藏消息窗口）。
- 键鼠钩子：`OwlKeyLog` 用 `SetWindowsHookExA` 装 `WH_KEYBOARD_LL`/`WH_MOUSE_LL`，**未用 Raw Input**。这也解释了它为什么需要提权（低完整性进程收不到高完整性窗口的输入）。
- 复刻映射：工具集已有 `tauri-plugin-global-shortcut` 与管理员权限，可直接复用；键鼠钩子仅在"录制快捷键/演示标注"时需要。

### 4.10 配置与数据格式

| 文件 | 格式 | 用途 |
| --- | --- | --- |
| `Config/config.json` | JSON，键值 + `subConf` | 主配置：保存模板、冲突策略、通知、快捷键、开机自启 |
| `Config/markConfig.ini` | INI + Qt `@Variant` 编码 | 标注工具默认参数 |
| `LocalStorage.data` | INI + Qt `@Variant` | 更新检查时间、上次保存路径等"本地状态" |
| `Data/*.webp` + `*.meta` | WebP + Qt `QDataStream` | 贴图内容与窗口状态 |
| `Data/PinWindowd.sqlite` | SQLite | 贴图历史索引 |
| `language/zh-cn.qm` | Qt 翻译 | 多语言 |

---

## 5. 映射到 LightweightToolset 现状

| 现状 | 位置 | 对复刻的影响 |
| --- | --- | --- |
| 工具注册表 `TOOLS: [ToolDefinition; 6]` | `src-tauri/src/tools.rs` | 新增 `screenshot` 项即可获得启停/热键/侧栏入口 |
| 启停 `start/stop` + worker 线程 | `tools.rs` | 截图工具需要常驻 worker 吗？不需要；热键触发即可，`worker_running` 可保持 false |
| 多页 Vite（main/popup/free/notification） | `vite.config.ts` | 新增 `screenshot.html` + `src/screenshot.tsx`（覆盖层）、可复用 `free.html` 做贴图页 |
| 窗口白名单 `["main","tool-*"]` | `src-tauri/capabilities/default.json` | `tool-screenshot-*` 自动匹配；无需改 capability 前缀 |
| 全局热键插件 | `tauri-plugin-global-shortcut` | 直接复用；默认建议 `CTRL+ALT+A`（与 PixPin 无关，取工具集未占用组合） |
| 管理员清单 | `windows-app-manifest.xml` | 已满足"抓高完整性窗口 / SendInput 到管理员窗口" |
| `windows-sys` 已开 `Win32_Graphics_Gdi`、`Win32_UI_WindowsAndMessaging` | `Cargo.toml` | BitBlt/GetCursorInfo/SetWindowPos 可用；**缺** `Win32_Graphics_Dwm`、`Win32_UI_Accessibility`，做窗口检测时再补 |
| 剪贴板仅文本（CF_UNICODETEXT） | `clipboard.rs` | 截图复制需要新增 `CF_DIB`/`PNG` 写入；建议独立 `clipboard_image.rs`，不改文本路径 |
| 无图像编解码 crate | `Cargo.toml` | 需新增 `image`（PNG/JPEG/WebP）或走 WIC；推荐 `image`，与 `gif`/`png` 生态一致 |
| 存储：JSON 文件 + 应用数据目录 | `clipboard.rs`/`settings.rs` | 截图历史沿用 JSON 索引 + 文件目录，避免过早引入 SQLite |

---

## 6. 复刻方案（推荐）

### 6.1 架构决策：覆盖层用 WebView 还是原生窗口

| 维度 | A. 原生覆盖层（Rust + GDI） | B. WebView 覆盖层（Tauri 透明窗口 + Canvas） |
| --- | --- | --- |
| 抓屏/冻结图显示 | 直接 `SetDIBitsToDevice`/`UpdateLayeredWindow`，零拷贝路径最短 | 位图交给 WebView（HTTP/asset），多一次解码与 GPU 上传 |
| 首次显示延迟 | 实测 44–55 ms | 实测 323–403 ms（含 WebView2 冷启动） |
| 图片解码耗时 | 无（DIB 直接画） | 实测 134–141 ms（2560×1600 BMP） |
| 宿主进程峰值私有内存 | 实测 17.3 MB | 实测 20.0 MB |
| 进程树峰值私有内存 | 实测 17.3 MB | 实测 375 MB（含图）/ 262 MB（空白页基线） |
| 空闲私有内存 | 实测 0.88 MB | 实测 1.4 MB（宿主，不含渲染进程） |
| 选区/标注 UI | 需自绘（GDI/D2D），文字编辑与撤销栈成本高 | HTML/CSS + Canvas，工具栏/文字/撤销复用前端能力 |
| 多屏异 DPI | 需自己做物理↔逻辑换算 | 同样要换算，且 WebView 的 DPI 跟随窗口 |
| 维护成本 | 高（自绘控件、命中测试、文本输入法） | 低（React 生态） |
| 风险 | 工期长 | 透明全屏 WebView2 的稳定性/性能、合成层闪烁 |

**测量说明（2026-11-05，临时测量程序，未写入仓库）**：单屏 2560×1600 @150%（物理像素 4.1 MP）；抓屏为 GDI `BitBlt` + `GetDIBits`；WebView 侧用本机 HTTP 提供 15.6 MB BMP；内存为私有字节，40 ms 采样；两轮数据稳定（空白 262/268 MB、含图 375/375 MB）。**未覆盖**：多屏异 DPI、PNG 编码开销、WebView2 已预热时的增量（工具集主窗口已常驻 WebView2，覆盖层只会新增一个渲染进程，而非整套容器）。

**推荐改为 A（原生覆盖层）为主。** 实测依据：

1. 原生路径首帧 44–55 ms、进程树峰值 17.3 MB，与 PixPin 的 Qt 自绘同一量级，满足"按下热键即出画面"。
2. WebView 路径同一张图要多付 **约 113 MB** 进程树内存和 **约 270 ms** 首次显示；空白页基线 262 MB 说明大头是 WebView2 容器（工具集已常驻，可摊薄，但单个覆盖层仍会新增一个渲染进程）。
3. 贴图是多实例场景：若每个贴图窗口都挂 WebView，内存随贴图数量线性增长；原生分层窗口每张只需位图本身。

**折中路线（若标注 UI 不想全手写）**：覆盖层与选区用原生（保住 17 MB / 50 ms），选区完成后进入"编辑态"时再开一个**普通** Tauri 窗口承载标注界面（非全屏覆盖、按裁切后的图工作），这样既避开全屏 WebView 的延迟，也保留 React 的开发效率；贴图窗口保持原生。

> 注意：覆盖层必须在抓屏**之后**才显示，否则会抓到自己；贴图窗口与覆盖层都要在截图时排除。

### 6.2 模块划分

Rust（`src-tauri/src/`）：

```text
screenshot/
  mod.rs          # 对外命令、生命周期
  capture.rs      # 虚拟桌面枚举、逐屏 BitBlt、光标合成、物理/逻辑坐标换算
  overlay.rs      # 原生覆盖层：冻结图、选区、标注渲染、放大镜（实测 17 MB / 50 ms）
  compose.rs      # 按选区裁切、标注扁平化、编码
  clipboard_image.rs  # CF_DIB / PNG 写剪贴板
  pin.rs          # 贴图窗口（原生分层窗口）：置顶、无边框、穿透、变换状态
  history.rs      # 截图/贴图历史索引（JSON）
  monitor.rs      # 显示器拓扑、DPI、工作区
```

前端（可选，折中路线才需要）：

```text
editor.html + src/editor.tsx           # 选区完成后开普通窗口做标注编辑
src/editor/                            # 标注对象模型（item/命令栈/命中测试/渲染）
  model.ts  items.ts  history.ts  render.ts
```

数据（`%APPDATA%/<identifier>/screenshot/`）：

```text
index.json          # 历史：id, kind, createdAt, closedAt, width, height, path, ocr?, srcWin?
pins/<id>.png       # 贴图内容（或复用截图原图）
pins/<id>.json      # 贴图状态：scale/rotate/opacity/flip/keepRatio/pos
shots/2026-11-05_19-30-00.png
```

### 6.3 与 PixPin 的差异取舍

| PixPin 做法 | 我们的做法 | 原因 |
| --- | --- | --- |
| Qt Widgets 全自绘 | WebView 覆盖层 + Canvas | 复用工具集技术栈，工期可控 |
| OpenCV 静态链接 | `image` + 手写相关/卷积 | 避免 40 MB 级二进制膨胀 |
| ncnn + 内嵌 OCR 模型 | Windows.Media.Ocr → 可选 ONNX | 先零模型，后续按需 |
| SQLite 贴图库 | JSON 索引 | 数据量小，与现有工具一致 |
| 云账号/统计/更新 | 不做 | 隐私与范围；工具集已无此类入口 |
| 自绘 GIF 编码器 | `gif` crate | 成熟、体积小 |

### 6.4 分阶段与验收

**M1（MVP，建议 1 个迭代）**

- 热键 → 抓全虚拟桌面 → 覆盖层显示冻结图 → 拖选 → 工具栏（矩形/椭圆/箭头/铅笔/荧光笔/文字/序号）→ 撤销/重做 → 复制/保存/取消 → 贴图（置顶、拖动、缩放、透明度、关闭、关闭全部）。
- 验收：双屏异 DPI 选区不错位；复制到微信/画图正常；保存命名可用模板；ESC/右键取消；贴图置顶且在任务栏不出现；连续截图无内存增长。

**M2（体验）**

- 窗口/控件高亮与"仅窗口"截图、辅助线、放大镜 + 网格 + RGB/HEX/C 复制、马赛克/模糊、贴图旋转/翻转/穿透/阴影、历史与恢复上次关闭、自动保存。

**M3（进阶）**

- 长截图（纵向/横向，滚动采样 + 条带互相关 + 预览）、GIF 录制、OCR + 文本选择层。

**M4（可选）**

- 动态截图（需先确认其真实语义：推测为"定时/连拍/自动滚动"之一，未确认）、形状自动识别、贴图文件/文本/颜色类型、多语言。

---

## 7. 风险与坑（按优先级）

1. **多屏 + 异 DPI 坐标系**：必须统一到"物理像素虚拟桌面"，Tauri 窗口尺寸/位置用逻辑像素，二者换算只能有一个入口函数；否则选区偏移 1/3 屏宽。PixPin 用 `devicePixelRatio` 单独处理，值得照做。
2. **WebView2 透明全屏窗口**：`transparent: true` + `decorations: false` + `alwaysOnTop` + `skipTaskbar` 可行，但 4K 双屏下合成开销明显；必须实测首帧与拖动帧率，兜底原生覆盖层。
3. **抓屏时机**：先抓再显示覆盖层；连续截图要清缓存；`WS_EX_LAYERED` 窗口本身不会被 BitBlt 抓到（好），但贴图窗口会。
4. **图像剪贴板**：`CF_DIB` 需要自下而上 BGR + 4 字节行对齐，另加 `PNG` 格式备份给现代应用；不要只写 `CF_BITMAP`。
5. **长截图滚动**：`SendInput` 滚轮步长受系统"每次滚动行数"影响，必须按内容自适应（每次滚动 60% 可视高度 + 重叠 30%），并检测"两帧相同 → 到底"。
6. **GIF 体积**：调色板 + 帧间差分 + 只编码变化区域；否则 10 秒 1080p 可达几十 MB。
7. **内存**：一张 4K×2 屏 RGBA ≈ 66 MB；标注图层再翻倍；要限制历史缓存帧数。
8. **权限**：工具集已是管理员，抓屏/发送输入无障碍；但管理员进程的覆盖层可能与 UAC 安全桌面不同会话，属预期限制。
9. **合规**：只做**行为复刻**，不复制其二进制、图标、字体、OCR 模型与翻译文件；OCR 若用第三方模型需遵守其许可证（PaddleOCR Apache-2.0 / ncnn BSD 可行，但需自带模型与声明）。Qt 本身为 LGPL/GPL，我们不复用其代码，无连带义务。
10. **热键冲突**：避开工具集已用组合（`CTRL+ALT+M/V/T`）；截图默认建议 `CTRL+ALT+A`。

---

## 8. 明确不做

- 账号/登录/VIP（`accounts.viewdepth.cn`、`deepviewapi`）。
- 使用统计上报（`statistics.viewdepth.cn`）。
- 软件自更新（`PixPin Update`/下载/替换流程）——与本工具集的发布方式冲突。
- 翻译、AI 对话、资源捕获（工具集已有硬性约束禁止此类入口）。

---

## 9. 未确认项（需要时再取证）

| 问题 | 现状 | 下一步 |
| --- | --- | --- |
| 覆盖层/贴图窗口的精确 Win32 风格（是否 `WS_EX_LAYERED`、是否 DWM 阴影） | 只有类名与配置项，未取窗口标志 | 运行期 `GetWindowLong` 抓一次贴图窗口即可确认 |
| 长截图匹配的具体指标与亚像素策略 | 只见引擎边界，未见算法 | 反编译 `FUN_180003460` 或实测重叠误差 |
| OCR 模型来源/字典编码 | 资源名与第三方路径已确认，模型内容未解析 | 如需高精度再解析 param/bin |
| `Dynamic Screenshot` 的真实语义 | 仅类名与文案 | 观察一次真实操作或查文档 |
| 贴图 `.meta` 的完整字段类型 | 只读到部分键名 | 用 Qt `QDataStream` 规则完整解析一个样本 |

---

## 10. 落地待办

1. 在 `src-tauri/src/tools.rs` 注册 `screenshot` 工具（默认关闭、热键 `CTRL+ALT+A`）。
2. 新增 `screenshot/` Rust 模块，**原生全屏覆盖层**（冻结图 + 选区 + 标注渲染），跑通 M1 闭环。
3. 标注编辑界面二选一（§6.1 折中路线）：原生自绘，或选区完成后开普通 Tauri 编辑窗口；两者都不要全屏 WebView。
4. 新增 `image`/`gif` 依赖与 `clipboard_image.rs`，补 capability（原生窗口无需 capability，编辑窗口需要 `tool-*`）。
5. 真机验收多屏异 DPI、复制粘贴、贴图置顶、连续截图内存；补测多屏与 PNG 编码开销。
6. M2/M3 按 §6.4 推进；每期结束更新本文"未确认项"。

> 本文所有结论均来自 §1.1 的 Evidence；未标注的推断已写入 §9，未把静态观察当成运行行为。
