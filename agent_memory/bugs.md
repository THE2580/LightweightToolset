# 活跃风险与约束

- ~~静默启动后主窗口「唤不出 / 整个软件像死的」~~ **已结案（`v0.7.2` 发布，`abe5441`）**：用户装完 `v0.7.1` 后第一次开机即复现——进程 `Responding=False`、所有窗口 `WM_NULL` 探测 HUNG、托盘菜单弹不出。抓主线程栈定位到**主线程嵌套建窗死锁**：
  - 栈底 `window_service::open_clipboard_popup` → `WebviewWindowBuilder::build` → tauri 的 `send_user_message` 在主线程**就地执行** `handle_user_message` → 窗口消息派发期间把排队的 `shortcut_tip::build_window` 也执行了 → 内层建窗再去拿同一把窗口锁 → `std::sync::Mutex::lock_contended` 永久阻塞。
  - 复现条件：剪贴板弹窗与快捷键提示浮窗**同时首次创建**（首次安装后第一次用剪贴板快捷键最易撞上）。
  - 修复：`window_service::begin_window_build()` / `window_build_busy()` 占位，后到建窗让路（浮窗退避重试 5 次、弹窗交给独立线程排队），覆盖剪贴板弹窗、计时器自由/时钟窗口、提示浮窗；让路写 `logs/window.log`（`window_build.busy defer=...`）。开发版复现时日志确证让路两次、两窗正常出现、进程持续响应。
  - 取证方法沉淀：`SendMessageTimeout(WM_NULL)` 探活 + `StackWalk64`/dbghelp 栈回溯（本机无 procdump/cdb，用 PowerShell 内嵌 C# 实现）。

- **工作树常态**：`M src-tauri/src/window_service.rs`（既有 rustfmt 噪声）、`M src-tauri/src/window_pinner.rs`（仅换行差异）与 `?? .codex/`（Codex 遗留本地配置，DSH 不读取）长期存在；除非用户授权，不处理、不回滚、不提交。
- **窗口置顶边界**：管理员目标受 UIPI 限制可能返回 Win32 5；监护不覆盖开发终端关闭、系统同时杀死监护进程、关机等场景。
- **测试串行**：`input_monitor` 等测试共享全局按键状态，并行 `cargo test --lib` 会偶发失败；统一 `--test-threads=1`，并把 `TEMP`/`TMP` 指向 `src-tauri/target/test-tmp`（否则 `window_pinner` 三条测试因沙箱写 `%TEMP%` 失败）。
- **格式化**：全仓 `cargo fmt --check` 与严格 clippy 有既有告警，不作为准入条件；不要对仓库 Rust 文件跑 `rustfmt`。
- **OCR 模型目录**：运行时只认 exe 同级 `ocr/ppocr_v6_small`；release 无仓库兜底（否则安装测试假通过）。模型缺失时贴图菜单会提示「找不到 OCR 模型」，不影响其他功能。
- **系统级拖拽缩略图未解**：`778e0a4`（走 `IDragSourceHelper`）曾导致拖放崩溃并带崩接收方，根因未查；当前用自绘 ghost + 缩略图，四目标实测稳定，**不要贸然改回 helper 路径**。

# 已知限制（已接受）

- 分词提取：序号 `1.` `2.` 会带句点成词、`×N` 只剩 `N`、单字虚词保留；用户已确认接受。
- `Ctrl+Alt+Del`、`Win+L` 等内核/安全层热键无法捕获或拦截。
- 浮窗只出现在主显示器；UAC 安全桌面与 System 完整性进程的按键不可见。
- 双版本并存时全局快捷键系统级独占，后启动一方可能注册失败（预期行为，不处理）。
- 截图：多屏不作为验收/优化目标（代码按虚拟桌面并集实现）；马赛克/模糊是矩形区域，不是 PixPin 的笔刷涂抹。
- 贴图文本选择：复杂版面/极小字号仍可能识别出错；每次识别重新加载模型（约 0.1–0.3s，含在「正在检测文本」里）。

# 环境坑（本机）

- **npm 路径**：Git Bash 的 PATH 里没有 `npm`；用 `E:\devtools\node\v24.12.0\npm.cmd` 或 PowerShell 执行。
- **`npm run tauri dev` 不可用**：`requireAdministrator` 清单在 cargo 拉起 debug exe 时报 `os error 740`；按 `AGENTS.md` 的「启动开发版」流程走。
- **测试临时目录**：见上文「测试串行」。
- **布局**：本机缩放 135%，主窗口 CSS 视口比物理尺寸小，固定高度弹窗用 `100vh` 兜底。
- **Release 资产中文名被截断**：必须上传 ASCII 英文名副本，再用 `gh release view` 核对。
- **PowerShell 5.1 读写 `settings.json` 会毁配置**：读无 BOM UTF-8 按 ANSI 解码、写回加 BOM；改配置一律用 node。

# 已确认经验

- 2026-11-20：截图工具里「贴图 / 恢复 / 关闭全部 / 隐藏显示 / 鼠标穿透」这几个快捷键不弹提示，根因是全局快捷键回调里贴图组命令分支执行完直接 `return`，而「派发快捷键提示」那段排在它后面，永远轮不到（截图、呼出主窗口、窗口置顶排在后面处理，所以只有它们会提示）。修法：把 `notify_app_hotkey` 的派发提前到贴图组命令之前。教训：快捷键回调里任何新增的「早返回」分支之前，都要确认提示派发已经发生。
- 2026-11-20：窗口预选最初按「同进程」排除，会连带把本软件主窗口排除掉；用户要求本软件窗口也能被预选，因此改为只跳过截图覆盖层自身的 HWND（`OVERLAY_HWND`），其余同进程窗口正常参与命中。
- 2026-11-20：预选态「整屏压暗 + 放大镜拖出亮痕」的根因：首帧走 `band_needs_full_paint` 快速路径只画了压暗图；全屏窗口的预选矩形与初始全屏预选相等时不会触发“预选变化→整屏重绘”，于是永远停在压暗层。修复：脏区与 `selecting_rect()` 相交就强制完整绘制（原图 + 边框），不再只依赖预选变化这一个触发点。
- 2026-11-19：覆盖层加整屏预选后出现「鼠标/放大镜像 60 帧」的掉帧：`frame_rect()` 把整块预选算进每帧脏区，导致每次鼠标移动都整屏重绘（压暗 + 原图两次 16MB BitBlt）。修法是预选态只把放大镜面板计入脏区，预选切换 / 拖拽开始 / 进入标注时再强制整屏重绘（`painted_once = false`）。以后往选区阶段加常驻装饰时，先确认它不会把 `frame_rect` 撑成整屏。

- 2026-11-19：注入测试的「点击录制框 → 发送组合键 → 点保存」必须在**同一次脚本运行**里连续完成；分两次运行会因为新进程的 `SetForegroundWindow` 让录制框失焦、触发 `onBlur` 收起，表现为「保存后说未填写/没变化」。
- 2026-11-19：工具页里的 `tool` 来自 `activeTool`（导航时捕获的快照），只有 `setToolEnabled` 会刷新它；因此在工具页内改快捷键后，`tool.hotkey` 会滞后。读实时 `tools.find(...)?.hotkey` 才能即时刷新（截图页原本就这么做，剪贴板页与窗口置顶页本轮补上）。
- 2026-11-19：提权启动开发版**不要**用 `cmd /c start ... powershell -Command "Start-Process -Verb RunAs ..."` 包一层——实测静默失败、`%TEMP%\lwt-dev` 都不生成；应从当前 shell 直接 `powershell -Command "Start-Process powershell -Verb RunAs -ArgumentList ..."`。启动后必须核对三件事：`%TEMP%\lwt-dev` 已生成、进程 `StartTime` 是新的、`kill` 旧实例目标来自该目录，否则容易把上一次的旧截图当成新结果（本轮曾误读 11-11 的旧日志与截图）。
- 2026-11-19：固定高度弹窗（`.clipboard-settings-dialog`）里增内容会挤成滚动条并切掉底部说明；加内容时同步调 `height`（本轮 400→452px）。
- 2026-11-11：OCR 模型必须外置并按需加载（`include_bytes!` 会让 30MB 映射页常驻，达不到「识别完回落基线」）；单工作线程串行识别，用完 drop 引擎 + `SetProcessWorkingSetSize` 归还工作集（实测测试进程 idle 7.6MB → 峰值 110–118MB → 释放后 1.6–2.2MB）。
- 2026-11-11：改正式版配置做验证必须「先停进程 → 改文件 → 启动验证 → 再停 → 还原 → 启动」；运行中的实例退出时会回写内存里的设置，覆盖手工还原。验证产生的 `screenshot/` 数据也要清掉。
- 2026-11-11：注入测试里按住 Ctrl 按 Esc 等于 `Ctrl+Esc`，会弹出开始菜单；取消拖放前先松开 Ctrl。`SetCursorPos` 后要等 ≥200ms 再 `mouse_event`。
- 2026-11-11：拖放用自绘缩略图（`ghost_thumbnail`）比系统 `IDragSourceHelper` 可控；系统 DragImage 在很多接收方根本不被请求（实测 OLE 从不调用 `GetData(DragImageBits)`）。
- 修饰符双计数根因是 Raw Input 通用 VK 与 Hook 分侧 VK 不一致；共享入口规范化后再去重。
- High 游戏输入根因是完整性隔离；`v0.5.0` 起整体提权解决。
- 窗口置顶被强制终止后无法靠主进程回调清理，独立 watchdog 是必要能力，但启动不能阻塞基础页面状态。
- 2026-11-02：「系统/他软件热键在本软件前台时不提示」根因是 Chromium 前台抑制 + 按键被抢先注册；解法是独立非 Chromium 采键子进程，Raw Input 不能替代。
- 2026-11-02：快捷键回调运行在主线程时，加锁 + 窗口操作会与前端 2 秒快照轮询形成环形等待、整窗卡死；回调一律丢独立线程，快照先拷贝再查注册表。
- 2026-11-02：浮窗不要按字符估宽再动态改窗口；固定宽窗口 + `width: max-content` 才稳。
- 2026-11-02：条目 id 用毫秒时间戳会撞车，改「时间戳-自增序号」。
- 2026-11-02：带淡出动画的弹窗在提交失败时必须复位 `closing`，否则遮罩留在全屏不可见状态。
- 2026-11-02：录制状态要绑定「录制框聚焦」而不是「弹窗打开」。
- 2026-11-04：开机自启必须用计划任务（登录触发 + HighestAvailable + InteractiveToken + `ExecutionTimeLimit=PT0S`），Run 键对提权 exe 无效。
- 2026-11-04：开发版隔离直接改 `identifier`，不要靠改路径或端口。
- 2026-11-06：覆盖层拖影根因是「只贴脏矩形 + `InvalidateRect` 整窗」；按「旧帧 ∪ 新帧」重建脏区，所有绘制收在 `frame_rect` 内。
- 2026-11-06：覆盖层按键会被中文输入法吞成 `VK_PROCESSKEY`；创建窗口后 `ImmAssociateContext(NULL)` 解绑，文字编辑时再挂回。
- 2026-11-06：截屏首帧优化——直接 `BitBlt` 进 DIB 段、压暗用一次 `AlphaBlend`（526ms → 约 101ms）。
- 2026-11-08：`SHDoDragDrop` 需要 `OleInitialize` 且第二参数必须是 `IDataObject*`；返回 `0x00040100` 才算真正开始拖放。
- 2026-11-08：手写 vtable 释放 COM 接口会崩，跨 FFI 的对象生命周期交给 `windows` crate 的类型化包装。
- 2026-11-08：工具页设置项要跟着工具走；「截图查看」已取消，不要再加回。
- 2026-11-08：`SHBrowseForFolderW` 指定初始目录必须给 `BFFM_INITIALIZED` 回调。
- 2026-11-09：贴图标注工具栏不能画在贴图窗口内（窄图会被裁），用窗口外伴生小窗；宽度按 `2×内容宽 − 主栏宽`。
- 2026-11-09：`MenuEntry::item(text, grayed, checked)` 第二个参数是「置灰」，不是「可用」。
- 2026-11-09：预览/最终合成必须走同一套绘制（草稿构造口径与 overlay 一致）。

# 历史记录

- 贴图 COM 与拖放、贴图标注工具栏、拖放崩溃事故的完整记录（含 REA 取证）见 `archive/2026-11-11-bugs-history.md`。
- 更早的 P0/helper/通知/Chromium 证据链见 `archive/2026-11-02-bugs-compaction.md`。
