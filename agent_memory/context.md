# 项目背景

- LightweightToolset 是旧版 LightweightWindowsToolset 的独立 Tauri 2 / Rust / React / TypeScript 路线；旧 Electron 项目只作稳定参考与迁移对照，新功能不回写旧项目。
- 已实现工具：剪贴板、软件使用统计、按键使用统计、计时器、窗口置顶、快捷键提示。后续候选：快捷翻译、桌宠。
- 当前稳定发布为 `v0.6.0`，发布提交 `5f12c12`，仓库 `https://github.com/THE2580/LightweightToolset`。

# 协作环境

- 2026-10-23 起由 DSH 接管，主工作目录 `E:\DeepSeek_Harness\Workspace\LightweightToolset`，自原 Codex 目录 `E:\codex_agent_project\LightweightToolset` 完整迁移（含 Git 历史、依赖与构建缓存）。
- 项目级规范见根目录 `AGENTS.md`；`agent_memory/` 已从 `.gitignore` 移除并纳入版本控制，记忆随项目提交。
- Windows 下 `npm` 不在 Git Bash 的 PATH；构建/安装命令在 PowerShell 执行，或使用 `E:\devtools\node\v24.12.0\npm.cmd`。

# 长期工程约束

- 默认中文沟通与项目记忆；代码标识保持仓库风格。默认最小改动，优先共享入口、现有实现、标准库、平台能力和已安装依赖。
- 非简单任务先读项目记忆和相关源码；运行时代码、构建、类型、公共接口或关键 UI 变化必须执行匹配验证。
- 未经用户明确要求，不提交、不推送、不发布、不主动启动开发版。
- 源码/文档通过文件编辑工具写入，优先局部编辑而非整文件重写；不恢复用户主动删除内容；不提交 QA 截图、日志、缓存或构建副产物。
- 工具禁用必须停止快捷键、worker、监听、计时器、关联窗口和 helper，不能只隐藏 UI。
- 只做编译期内部工具注册；不做插件市场、远程插件、脚本扩展或运行时安装工具。
- 不恢复 AI 聊天、API 设置、DeepSeek 或资源捕获入口；未列入迁移队列的旧工具视为废弃。
- 版本规则：新增工具调整第一/二位版本号；缺陷修复或现有工具扩展调整第三位版本号。
- 保留首次 release 构建产物，不清理 `src-tauri/target/release`。定期评估 `src-tauri/target` 占用；明显膨胀时先核对构建进程与子目录大小，仅按需清理可再生的 `target/debug`，禁止直接使用会同时删除 release 的 `cargo clean`。

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
- 键盘导航：上下用于应用级目标，左右用于页内线性对象；输入框、可编辑区和模态框不抢占。

# 项目记忆规则

- `context.md`：长期背景和约束；`progress.md`：当前阶段、里程碑、待办；`bugs.md`：活跃问题、限制、修复线索；`working_state.md`：实时现场；`checkpoint.md`：当前任务恢复包。
- 核心文件只保留当前有效信息；旧阶段摘要移入 `archive/`。实时工作树高于 checkpoint。

# 发布索引

- `v0.6.0` / `5f12c12`：新增工具「快捷键提示」（预设 24 条 + 本软件快捷键 + 自定义录制）、独立采键子进程（`--shortcut-tip-helper` + 回环 TCP）、浮窗内容自适应与显示期间持续压顶；剪贴板移除真实按键通知与剪贴板确认，条目点击提示改调新工具。资产 `LightweightToolset_0.6.0_x64-setup.exe`，SHA-256 `93D2315A30BDEDF01C7AAF0817EA102245DAC4734456776A4CB3E64C9F447499`，Release `https://github.com/THE2580/LightweightToolset/releases/tag/v0.6.0`。

- `v0.5.1` / `1d4e65d`：装机回归修复——「关于」页版号改读 `package.json`；软件内 `Ctrl+C / Ctrl+X / Ctrl+V` 改页面上报（WebView2 聚焦时全局钩子漏派发），0.5s 节流 + 跨来源去重；应用内复制/剪切立即提示；通知浮窗每次显示重压置顶层。资产 `LightweightToolset_0.5.1_x64-setup.exe`，SHA-256 `0D37D40604CC46D5EC09CABFB97C0E1F1A7A3F57EB2E3FA2C34B230279AEAC53`，Release `https://github.com/THE2580/LightweightToolset/releases/tag/v0.5.1`。

- `v0.5.0` / `8c68e2b`：剪贴板七项（回收站全部清空、Ctrl+A、快捷弹窗分段/全局搜索/过渡/焦点驻留、分词重构、系统通知）、整体提权（`requireAdministrator`）、移除管理员输入 helper。资产 `LightweightToolset_0.5.0_x64-setup.exe`，SHA-256 `4E1CE15BD8836E2332032732AAE7F2EF2DA5936E184C84A86F98DDD3EA36CD6F`，Release `https://github.com/THE2580/LightweightToolset/releases/tag/v0.5.0`。

- `v0.4.4` / `7f9d66d`：AppState 提前注册、首次快照后后台初始化 watchdog/helper/tools/hotkeys、移除 state-not-managed 轮询并增加 runtime 状态反馈。安装包 SHA-256 `8570AED8E3E34B156F33BA9A1349AC14A20F51216B6DEB846277F59162C738B9`。
- `v0.4.3` / `f3e47e6`：AppState 首次快照有限重试；该方案现确认未解决 P0 根因。安装包 SHA-256 `15CD38C33C121DD468849BABB1AD484C40AC68B4CDB712297F17F1207E1800E0`。
- `v0.4.2` / `2a464cc`：主题实时同步、窗口置顶 watchdog/通知声音、管理员输入 helper、修饰符去重。SHA-256 `78C287260E1FB3796A60F9D69F26CAA0B8742A60E3A0A9A3FF91772527FB7D73`。
- `v0.4.1` / `1096e21`：全局深色主题、跨窗口主题、窗口置顶通知。SHA-256 `4894EC1DD583BBD1FD05FC960A3C0D96E0F1B5766E7BAF6FA06251BC2F4C7EC7`。
- `v0.4.0` / `a3b625b`：窗口置顶第一版与分段控件统一。SHA-256 `E26729492908A0F1C4B2F941C61F3C1DAD3D759EDF2878C4B56568607DBADCC6`。
