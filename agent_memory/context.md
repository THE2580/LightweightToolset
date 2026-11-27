# 项目背景

- LightweightToolset 是旧版 LightweightWindowsToolset 的独立 Tauri 2 / Rust / React / TypeScript 路线；旧 Electron 项目只作稳定参考与迁移对照，新功能不回写旧项目。
- 已实现工具：截图与贴图（原生覆盖层 + 贴图窗口 + 标注 + 本地 OCR 文本选择，行为参照 `docs/pixpin-clone-analysis.md`）、剪贴板、软件使用统计、按键使用统计、计时器、窗口置顶、快捷键提示。后续候选：快捷翻译、桌宠。
- 当前稳定发布为 `v0.8.4`，发布提交 `5c3986d`，仓库 `https://github.com/THE2580/LightweightToolset`；用户已于 2026-11-27 安装该版本并在日常体验（安装目录 `D:\轻量化工具集\`）。

# 协作环境

- 2026-10-23 起由 DSH 接管，主工作目录 `E:\DeepSeek_Harness\Workspace\LightweightToolset`，自原 Codex 目录 `E:\codex_agent_project\LightweightToolset` 完整迁移（含 Git 历史、依赖与构建缓存）。
- 项目级规范见根目录 `AGENTS.md`；`agent_memory/` 已从 `.gitignore` 移除并纳入版本控制，记忆随项目提交。
- Windows 下 `npm` 不在 Git Bash 的 PATH；构建/安装命令在 PowerShell 执行，或使用 `E:\devtools\node\v24.12.0\npm.cmd`。

# 长期工程约束

- 默认中文沟通与项目记忆；代码标识保持仓库风格。默认最小改动，优先共享入口、现有实现、标准库、平台能力和已安装依赖。
- 非简单任务先读项目记忆和相关源码；运行时代码、构建、类型、公共接口或关键 UI 变化必须执行匹配验证。
- 未经用户明确要求，不提交、不推送、不发布；开发版按 `AGENTS.md`「验证方式」的固定动作，在改动完成后主动启动供测试。
- 源码/文档通过文件编辑工具写入，优先局部编辑而非整文件重写；不恢复用户主动删除内容；不提交 QA 截图、日志、缓存或构建副产物。
- 工具禁用必须停止快捷键、worker、监听、计时器、关联窗口和 helper，不能只隐藏 UI。
- 只做编译期内部工具注册；不做插件市场、远程插件、脚本扩展或运行时安装工具。
- 不恢复 AI 聊天、API 设置、DeepSeek 或资源捕获入口；未列入迁移队列的旧工具视为废弃。
- 版本规则：新增工具调整第一/二位版本号；缺陷修复或现有工具扩展调整第三位版本号。
- 保留首次 release 构建产物，不清理 `src-tauri/target/release`。定期评估 `src-tauri/target` 占用；明显膨胀时先核对构建进程与子目录大小，仅按需清理可再生的 `target/debug`，禁止直接使用会同时删除 release 的 `cargo clean`。
- 开发版与正式版的隔离方式、默认值对照、常驻行为差异、并存限制与发版前清理，统一见 `AGENTS.md`「开发版与正式版（差异与注意事项）」；此处不再重复，避免两处规则漂移。

# 权限与生命周期边界

- 主程序以管理员权限运行（`requireAdministrator`，清单见 `src-tauri/windows-app-manifest.xml`），每次启动弹 UAC。按键统计与快捷键提示依赖低级键盘钩子，可覆盖同级及更低完整性窗口；快捷键提示另有一个继承提权的采键子进程（非 Chromium），用于 Chromium 前台时仍能收到系统/他软件热键。UAC 安全桌面与 System 完整性进程不可见。
- 原“管理员输入 helper”的职责（主程序 Medium 时捕获 High 窗口）已随整体提权消失；若未来退回 Medium，需要恢复等效方案。
- 已验证 High 输入样本：Minecraft Java、原神、绝区零、米哈游启动器、PCL2；未测试游戏不泛化保证。
- 窗口置顶第一版只支持同级或更低完整性窗口；管理员窗口可能因 UIPI 返回 Win32 5，不引入置顶管理员 helper。
- 窗口置顶异常退出清理以正式安装版为验收范围：任务栏“结束任务”和托盘正常退出应释放责任清单；开发热重启/关闭开发终端不在该功能保证范围。

# 产品与 UI 基线

- 主窗口逻辑尺寸约 `912 × 598`，固定不可调整，使用应用内标题栏。
- 深色主题采用中性黑灰层级（背景约 `#171717`、面板约 `#212121`）；普通按钮不使用整块实心蓝，蓝色用于选中、链接、图表、热力和状态强调。浅色主题保持既有基线。
- 主窗口保存 `light` / `dark` / `system` 并广播 `app-theme-changed`；快捷弹窗和自由窗口监听该事件，`system` 模式监听 `prefers-color-scheme`。
- 通用分段输入复用 `SegmentedControl`：等分真实轨道、点击和拖动、吸附、未选中 hover；初始化/异步恢复直接定位，只有用户操作播放动画。剪贴板“历史/固定”是导航标签，不属于分段输入。
- 全局弹窗使用 body portal、顶层遮罩与统一动画；全局 Toast 只由 App 外壳维护单实例并替换旧提示；Tooltip 使用顶层 portal/fixed。
- 工具主页面默认隐藏页面级滚动条；固定工作台只允许明确列表/内容面板内部滚动。
- 快捷键设置跟随工具走：每个工具的快捷键配置入口放在它自己的页面/设置页里，全局设置只保留软件级动作（呼出主窗口）；录制统一用 `HotkeyRowEditor`，不要为单个工具另写一套。
- 快捷键外显统一口径：修饰键顺序 `Win → Ctrl → Alt → Shift → 主键`，`SUPER` 是 `Win` 的存储别名、对外一律显示 `Win`；前端走 `formatHotkeyForDisplay`，后端走 `tools::format_hotkey_display`，存储值与判重规则不变。快捷键提示的预设同样跟随工具：新增快捷键要同时登记到 `APP_HOTKEY_PRESETS` 与 `hotkey_owner_tool`。
- 快捷键触发类型（v0.8.4 起）：每条快捷键可选 `global`（组合键或单个 F1–F24，`RegisterHotKey` 独占）或 `listen`（任意键盘单键 / 鼠标侧键，`hotkey_listener` 低级钩子，不吞键、按下即触发、250ms 去抖、忽略注入事件）；存储在 `AppSettings.hotkey_kinds`（缺省 global）。注册/注销走 `tools::register_hotkey`/`unregister_hotkey` 分流，派发走 `dispatch_hotkey`/`dispatch_hotkey_id`。非录制阶段不可用状态用键值框红色虚线边框（`hotkey_error`）。工具热键注册失败不阻止工具启用。快捷键提示的自定义条目则按录到的键形态自动识别，只监听不注册。
- 截图进入即预选：覆盖层一出现就预选全屏；「仅窗口」检测下鼠标移动把预选切换成光标下窗口（只排除覆盖层自身窗口，本软件其它窗口与任务栏都可选中）；单击预选进入标注，拖拽则自定义选区松手确认。UI 元素级检测已明确暂缓。
- 键盘导航：上下用于应用级目标，左右用于页内线性对象；输入框、可编辑区和模态框不抢占。快捷键录制框带 `data-hotkey-capture`，录制态下所有方向键导航都要让路（否则方向键录不进去）。

# 项目记忆规则

- `context.md`：长期背景和约束；`progress.md`：当前阶段、里程碑、待办；`bugs.md`：活跃问题、限制、修复线索；`working_state.md`：实时现场；`checkpoint.md`：当前任务恢复包。
- 核心文件只保留当前有效信息；旧阶段摘要移入 `archive/`。实时工作树高于 checkpoint。

# 快捷键提示浮窗（原生实现）

- 「快捷键提示」的浮窗**已由原生实现接管**：`src-tauri/src/native_tip.rs` 用纯 Win32 分层窗口 + GDI 自绘，独立线程消息循环；原 WebView 浮窗（`notification.tsx` / `notification.html`）已删除。工具 id 仍是 `shortcut_tip`，预设、自定义提示、排序、启停状态全部沿用，无需迁移。
- 显示链路：`shortcut_tip` 的 worker 收到 `Command::Show` 后只维护「同内容累加 ×N」与可见窗口，然后调用 `native_tip::show`；主题、模糊、不透明度、动画、停留时长、位置、**键帽配色（浅/暗各一套底色/描边/文字，`#RRGGBB`）**来自 `shortcut_tip` 设置里的 `appearance`（`shortcut_tip_set_appearance` 命令与快照一起下发，配置了实色就按 1.0 不透明度绘制）。
- 外观设置入口：「快捷键提示」页右上「设置」按钮 → 标准弹窗（`createPortal` + 整窗模糊遮罩 + 进退场动画），底栏左侧「恢复默认配置」（二次确认后整体复位）。
- 既定设计口径：**窗口尺寸固定**（不提供缩放配置），内容按固定 1.20 比例自适应；亚克力为「显示那一刻抓屏快照 → 盒式模糊 → 主题色覆盖」；进入 200ms 缓入缓出、退出 300ms、图标/徽标弹动 230ms；停留时长可配 0.8–10s（默认 2s）。
- 定位模式：X/Y 两个滑块（屏幕物理像素），进入后显示**常驻静态预览**（标题「示例提示」、键盘图标、不播任何动画），X/Y 实时移动、重置立即生效；期间后端全局压制普通弹出，退出即失效并收起。
- 机制：显示/隐藏/预览走「最新意图 + 单调序号」状态机；`start()` 先 `paint()` 再 `ShowWindow`；预览态不启动动画定时器。

# 发布索引

- `v0.8.4` / `5c3986d`：快捷键新增「触发类型」（组合键/单个 F1–F24 的全局热键 + 任意单键/鼠标侧键的键鼠监听，单按钮切换、跨类型冲突检测、键被占用友好提示）；工具启用不再被快捷键注册失败阻塞，非录制阶段不可用键用红色虚线边框标记；快捷键提示改为按键形态自动识别，支持鼠标侧键提示/录制，补齐 CapsLock/NumLock/ScrollLock/PrintScreen/Pause 与 OEM 符号键。安装包 `LightweightToolset_0.8.4_x64-setup.exe`（33,984,141 字节），SHA-256 `3e1968f7d92c74aca80cb587ee2daaa1955aa67070f8dc6d11d03af23e5b9762`，Release `https://github.com/THE2580/LightweightToolset/releases/tag/v0.8.4`。
- `v0.8.3` / `86381f9`：提示窗自定义位置越界自动夹回工作区（完全不在屏幕则回落默认）；位置不再被“进入编辑”写死、旧值启动时迁移、位置「重置」清回默认、设置弹窗预取度量。安装包 `LightweightToolset_0.8.3_x64-setup.exe`（33,891,232 字节），SHA-256 `14fb2e62b40cdaefe09edf2f76fc5527a64ee6265d949a46eff8187d3780e784`，Release `https://github.com/THE2580/LightweightToolset/releases/tag/v0.8.3`。
- `v0.8.2` / `b778fa4`：修复 v0.8.1 静默启动回归（运行时初始化从 `get_app_snapshot` 移到 `setup` 自行启动 + 幂等），静默启动即可用全部后台功能并正常发启动通知。安装包 `LightweightToolset_0.8.2_x64-setup.exe`（33,885,497 字节），SHA-256 `769ea8bab05ba22adc8da864d98f44fa4dfc86aca9ab75e29e0c040f255bc9dd`，Release `https://github.com/THE2580/LightweightToolset/releases/tag/v0.8.2`。
- `v0.8.1` / `1361100`：主窗口改为按需创建 + 关窗立即销毁（`EmptyWorkingSet`），托盘态专用工作集 ≈153MB → **≈10MB**；采键 helper 启动即回收（≈16MB → 1MB）；`run()` 拦截 `ExitRequested` 防「窗口归零即退出」。安装包 `LightweightToolset_0.8.1_x64-setup.exe`（33,879,183 字节），SHA-256 `ad7b66332499724c72b6dd1c6d86849546b158909bf20bfb466440cc44a9dd6c`，Release `https://github.com/The2580/LightweightToolset/releases/tag/v0.8.1`。
- `v0.8.0` / `8d83b88`：快捷键提示浮窗改为原生绘制（省掉常驻 WebView2 渲染进程约 58MB）+ 外观 DIY（主题/动画/停留/位置/模糊/不透明度/键帽配色/恢复默认）+ 线性语义图标 + 「实验性功能」开关与图标绘制标签页（路径数据 M/L/H/V/C/Z）；修复符号键快捷键误判、停用工具后快捷键仍生效、贴图快捷键作用域/关闭确认/穿透状态残留、选区偶发漏压暗。安装包 `LightweightToolset_0.8.0_x64-setup.exe`（33,873,147 字节），SHA-256 `a1ba487dfc3aaeae49c364163723321c4c22adcbed3cf41fe16a9d269695e108`，Release `https://github.com/THE2580/LightweightToolset/releases/tag/v0.8.0`。
- `v0.7.2` / `abe5441`：修复主线程嵌套建窗导致的整机卡死（剪贴板弹窗与快捷键提示浮窗同时首次创建时互相等锁 → 主线程永久死锁，表现为托盘菜单/快捷键/所有窗口无响应）。新增 `window_service::begin_window_build/window_build_busy` 占位，后到建窗让路并写 `logs/window.log`。安装包 `LightweightToolset_0.7.2_x64-setup.exe`（33,851,424 字节），SHA-256 `2C24A3B5CCB13795DF60414AB7803E161A1D2732CCAF1C01C01BCF4C7D43130E`，Release `https://github.com/THE2580/LightweightToolset/releases/tag/v0.7.2`。
- `v0.7.1` / `c1359bc`：新增截图「UI 检测」（不检测 / 仅窗口，`窗口和UI` 置灰）与默认全屏预选（仅窗口下预选跟随光标窗口）；快捷键入口归位（剪贴板/窗口置顶迁入各自页面）与录制框改版；数量档位选择器；快捷键文案统一（`Win → Ctrl → Alt → Shift → 主键`、`SUPER` 不再外露）；快捷键提示预设扩充（新增 `Win+P/Win+R/Win+T/Win+G/Alt+F4`，截图 6 个快捷键纳入绑定预设）；贴图组快捷键恢复提示；主窗口最小化可唤出 + 唤出事件落盘 `logs/window.log`；截图设置页「UI 检测」独立与辅助线折叠分组。安装包 `LightweightToolset_0.7.1_x64-setup.exe`（33,856,072 字节），SHA-256 `5E875F6C6ECFE0EE1E32281888023AFD83C2DCB5535627110B220ED9FF00ECDE`，Release `https://github.com/THE2580/LightweightToolset/releases/tag/v0.7.1`。

- `v0.7.0` / `b5775dc`：新增工具「截图与贴图」——区域截图（覆盖层、放大镜取色、辅助线、可配置压暗）、12 种标注与二级栏、贴图窗口（拖动/缩放/锁定/穿透/阴影/右键图像处理）、贴图标注（窗口外伴生工具栏）、本地 OCR 文本选择（PP-OCRv6，离线、用后释放内存、「选中后自动复制」可关）、超级拖放与跟随缩略图、锁定态金色阴影；安装包含 30MB 离线模型，体积增至 33,846,940 字节。资产 `LightweightToolset_0.7.0_x64-setup.exe`，SHA-256 `705D550D7330C54F6BAB7EC15C49F6294DF66FD007D87FF07706D9C0645AB4EE`，Release `https://github.com/THE2580/LightweightToolset/releases/tag/v0.7.0`。
- `v0.6.4` / `fa580e8`：「快捷键提示」改用专用图标（键盘 + 提示气泡），修复侧栏兜底扳手与页头键盘不一致；侧栏、工具页头部、编辑弹窗、空状态统一。资产 `轻量化工具集_0.6.4_x64-setup.exe`（2,677,923 字节），SHA-256 `34C167F700B288B40D0384718804265C2387F83981632FEB90EB0D4898FAB5CD`，Release `https://github.com/THE2580/LightweightToolset/releases/tag/v0.6.4`。
- `v0.6.3` / `ed31f94`：静默启动时的启动通知（含实际快捷键、注册失败降级）；开发版专属默认值（静默启动/置顶/开发者模式默认开、工具全关、自启关并锁定、存储路径不可改）；开发版标题追加「（开发版）」；托盘与通知应用名改中文产品名（开发版带标识）。资产 `轻量化工具集_0.6.3_x64-setup.exe`（2,677,488 字节），SHA-256 `3A02BC5B13FECEC35D7AB68732F860AEE3B180676E24D4444AE5A7C37848C6C4`，Release `https://github.com/THE2580/LightweightToolset/releases/tag/v0.6.3`。
- `v0.6.2` / `9b5257e`：开机自启改计划任务（登录触发 + HighestAvailable + InteractiveToken + PT0S，清理旧 Run 键）；开发版 identifier 完全隔离（数据/WebView2/单实例/AUMID/开始菜单快捷方式/自启项）；开发版自启开关固定关闭并置灰；新增「静默启动」设置（主窗口 `visible:false` + setup 条件 show）。资产 `轻量化工具集_0.6.2_x64-setup.exe`（2,675,727 字节），SHA-256 `F3ADC3CFFF9807A490580C51074021E3EF210E27D864062E1A184173C88AB601`，Release `https://github.com/THE2580/LightweightToolset/releases/tag/v0.6.2`。
- `v0.6.0` / `5f12c12`：新增工具「快捷键提示」（预设 24 条 + 本软件快捷键 + 自定义录制）、独立采键子进程（`--shortcut-tip-helper` + 回环 TCP）、浮窗内容自适应与显示期间持续压顶；剪贴板移除真实按键通知与剪贴板确认，条目点击提示改调新工具。资产 `LightweightToolset_0.6.0_x64-setup.exe`，SHA-256 `93D2315A30BDEDF01C7AAF0817EA102245DAC4734456776A4CB3E64C9F447499`，Release `https://github.com/THE2580/LightweightToolset/releases/tag/v0.6.0`。

- `v0.5.1` / `1d4e65d`：装机回归修复——「关于」页版号改读 `package.json`；软件内 `Ctrl+C / Ctrl+X / Ctrl+V` 改页面上报（WebView2 聚焦时全局钩子漏派发），0.5s 节流 + 跨来源去重；应用内复制/剪切立即提示；通知浮窗每次显示重压置顶层。资产 `LightweightToolset_0.5.1_x64-setup.exe`，SHA-256 `0D37D40604CC46D5EC09CABFB97C0E1F1A7A3F57EB2E3FA2C34B230279AEAC53`，Release `https://github.com/THE2580/LightweightToolset/releases/tag/v0.5.1`。

- `v0.5.0` / `8c68e2b`：剪贴板七项（回收站全部清空、Ctrl+A、快捷弹窗分段/全局搜索/过渡/焦点驻留、分词重构、系统通知）、整体提权（`requireAdministrator`）、移除管理员输入 helper。资产 `LightweightToolset_0.5.0_x64-setup.exe`，SHA-256 `4E1CE15BD8836E2332032732AAE7F2EF2DA5936E184C84A86F98DDD3EA36CD6F`，Release `https://github.com/THE2580/LightweightToolset/releases/tag/v0.5.0`。

- `v0.4.4` / `7f9d66d`：AppState 提前注册、首次快照后后台初始化 watchdog/helper/tools/hotkeys、移除 state-not-managed 轮询并增加 runtime 状态反馈。安装包 SHA-256 `8570AED8E3E34B156F33BA9A1349AC14A20F51216B6DEB846277F59162C738B9`。
- `v0.4.3` / `f3e47e6`：AppState 首次快照有限重试；该方案现确认未解决 P0 根因。安装包 SHA-256 `15CD38C33C121DD468849BABB1AD484C40AC68B4CDB712297F17F1207E1800E0`。
- `v0.4.2` / `2a464cc`：主题实时同步、窗口置顶 watchdog/通知声音、管理员输入 helper、修饰符去重。SHA-256 `78C287260E1FB3796A60F9D69F26CAA0B8742A60E3A0A9A3FF91772527FB7D73`。
- `v0.4.1` / `1096e21`：全局深色主题、跨窗口主题、窗口置顶通知。SHA-256 `4894EC1DD583BBD1FD05FC960A3C0D96E0F1B5766E7BAF6FA06251BC2F4C7EC7`。
- `v0.4.0` / `a3b625b`：窗口置顶第一版与分段控件统一。SHA-256 `E26729492908A0F1C4B2F941C61F3C1DAD3D759EDF2878C4B56568607DBADCC6`。
