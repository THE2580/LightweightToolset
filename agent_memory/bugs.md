# 活跃风险与约束

- **「符号键快捷键被当成呼出主窗口」已定位并修复（2026-11-21，提交 `f81a99e`，未推送；开发版实测通过、用户已确认）**：把剪贴板快捷键设成 `Ctrl+Win+/` 后按下去会弹出主窗口。根因是同一物理按键在两侧的名字不统一：
  - 存储值来自前端 `KeyboardEvent.key`（符号原样保留 → `CTRL+SUPER+/`），`Shortcut::from_str` 能解析（`/` → `Code::Slash`），所以键**注册成功、回调也会来**；
  - 回调文本来自 `Shortcut::to_string()`（`global-hotkey` 的 `into_string`，顺序 shift/control/alt/super + `Code` 的 Display），符号键输出 DOM code 名 → `control+super+Slash`，`normalize_hotkey` 后是 `CTRL+SUPER+SLASH`，与存储值 `CTRL+SUPER+/` 对不上；
  - 匹配失败 → `handled` 保持 false → 旧兜底分支 `else { show_main_window(app) }` 直接呼出主窗口。
  - 修复（共享入口）：`tools.rs` 新增 `canonical_hotkey` / `canonical_key_token`，`tool_for_shortcut` / `app_hotkey_for_shortcut` / `ensure_hotkey_available` 全部按规范化标识比较（符号 ↔ DOM code 名、`ESC/ESCAPE`、`UP/ARROWUP`、`PGUP/PAGEUP` 等），存储值与显示文案不变；`lib.rs` 兜底改为只在真正匹配到 `main_window` 时唤出主窗口，其余记 `hotkey.unmatched` 且不再弹窗。
  - 影响面比报告的大：所有符号/别名键快捷键（`/ . , ; ' [ ] = - \``）此前都匹配不上；`Ctrl+Alt+.` 当主窗口快捷键时也是靠兜底「碰巧」生效，现在走显式匹配。
  - 验证：`tools.rs` 两条新单测（含 `Shortcut::to_string()` 输出 `control+super+Slash` 的根因复现）；开发版注入 `Ctrl+Win+/` → `Clipboard` 弹窗出现、Esc 关闭；把主窗口快捷键设为 `CTRL+ALT+.`、关闭主窗口（隐藏到托盘）后按 `Ctrl+Alt+.` → 主窗口重新可见。

- ~~静默启动后主窗口「唤不出 / 整个软件像死的」~~ **已结案（`v0.7.2` 发布，`abe5441`）**：用户装完 `v0.7.1` 后第一次开机即复现——进程 `Responding=False`、所有窗口 `WM_NULL` 探测 HUNG、托盘菜单弹不出。抓主线程栈定位到**主线程嵌套建窗死锁**：
  - 栈底 `window_service::open_clipboard_popup` → `WebviewWindowBuilder::build` → tauri 的 `send_user_message` 在主线程**就地执行** `handle_user_message` → 窗口消息派发期间把排队的 `shortcut_tip::build_window` 也执行了 → 内层建窗再去拿同一把窗口锁 → `std::sync::Mutex::lock_contended` 永久阻塞。
  - 复现条件：剪贴板弹窗与快捷键提示浮窗**同时首次创建**（首次安装后第一次用剪贴板快捷键最易撞上）。
  - 修复：`window_service::begin_window_build()` / `window_build_busy()` 占位，后到建窗让路（浮窗退避重试 5 次、弹窗交给独立线程排队），覆盖剪贴板弹窗、计时器自由/时钟窗口、提示浮窗；让路写 `logs/window.log`（`window_build.busy defer=...`）。开发版复现时日志确证让路两次、两窗正常出现、进程持续响应。
  - 取证方法沉淀：`SendMessageTimeout(WM_NULL)` 探活 + `StackWalk64`/dbghelp 栈回溯（本机无 procdump/cdb，用 PowerShell 内嵌 C# 实现）。

- **工作树常态**：`M src-tauri/src/window_pinner.rs`（仅换行差异）与 `?? .codex/`（Codex 遗留本地配置，DSH 不读取）长期存在；除非用户授权，不处理、不回滚、不提交。（`window_service.rs` 已随 0.7.2 提交，不再是现场。）
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

- 2026-11-22（截图选区偶发"漏压暗一块"）：**根因在脏区记账与"亮块回退"**。`selecting_rect()` 在拖拽起步帧（`current_rect()` 为 0 大小）会**回退到预选矩形**，而预选可能仍是全屏；此帧又正好触发"强制整屏重绘"（`painted_once=false`），于是整屏被画成亮块（未压暗）。下一帧的脏区只按 `union(last_frame, frame)` 计算，`last_frame` 记的是小框而非实际画过的整屏，多画的亮区再没人擦 → 残留一块跟着选区走。修法：① `dragging` 时 `selecting_rect()` 不再回退预选（只认当前拖拽框）；② 拖拽期间的强制整屏重绘，把 `last_frame` 记为整屏，让下一帧擦回压暗（只多一次整屏重绘，之后自动回落到小脏区）。排查靠的是临时埋点（debug 构建写 `%TEMP%\lwt-overlay-paint.log`，记录 `painted_once/dragging/bright/preselect/last/frame/base/rects`），定位后已删除。

- 2026-11-22（贴图偶发"无焦点/穿透"）：根因是**全局状态泄漏**——`PASSTHROUGH` / `HIDDEN` 是进程级标志，「全部穿逐/隐藏」只翻转不复位，最后一张贴图销毁后仍保留，之后新建的贴图会继承（穿透=点不动，隐藏=看不见）。修法：① 最后一张贴图销毁（`PINS` 为空）时把两个标志复位；② 没有贴图时 `ToggleVisible` / `TogglePassthrough` 不再翻转标志。注意贴图平时**故意不抢焦点**（`SW_SHOWNOACTIVATE`），只有文字编辑才 `SetForegroundWindow`，所以"看起来没焦点"本身可能是设计行为，先排查全局标志再怀疑焦点。

- 2026-11-22（已安装 v0.7.2 的兜底缺陷，用户已复现确认）：`v0.7.2` 热键回调末尾是 `if handled { ... } else { show_main_window(app) }`，任何"按下但未处理"的快捷键都会呼出主窗口。停用「截图」后，贴图组快捷键属于 **APP_HOTKEYS（软件级）**，注册不看工具启用状态，仍然进回调；而回调里贴图分支要求 `is_enabled("screenshot")` 才置 `handled`，于是全部落到兜底 → 弹主窗口。截图自身的工具级快捷键在停用时被注销，故只有它"正常"。**该兜底已在 master 的 `f81a99e` 改为只记 `hotkey.unmatched`**，因此开发版不复现，安装版仍带缺陷。残留两项待办：① 停用工具时注销其名下 APP_HOTKEYS（现仍占用全局键）；② 未执行时不要派发 `notify_app_hotkey`（工具停用仍弹提示窗）。

- 2026-11-21（前端弹窗层级）：应用内弹窗必须 `createPortal(..., document.body)` + `.dialog-backdrop`（`position: fixed; inset: 0; z-index: 9999`、`backdrop-filter: blur(3px)`、`dialog-fade/dialog-pop` 进退场动画）。直接渲染在工具页 DOM 里时，遮罩只覆盖内容区、侧栏不被遮住、也没有淡入淡出——层级就不对。关闭要走 `closing` 状态 + 170ms 延迟卸载，才有退场动画。

- 2026-11-21（分层窗口 / 显示顺序）：`UpdateLayeredWindow` 在窗口隐藏时也会更新内容，所以**必须先画再 `ShowWindow`**。先显示再画，用户会先看到上一次留在图层里的旧画面（本项目表现为"退出定位后第一次弹出闪一下示例"）。
- 2026-11-21（原生提示实验 / 静态预览）：`preview` 必须同时关掉**动画定时器**和 `tick()` 里的弹动推进；只在 `start()` 里把 `pop` 置 1 是不够的，`Hold` 阶段会把 `pop` 重新从 0 推到 1（图标徽标照样弹）。

- 2026-11-21（React effect cleanup 坑）：**不要写"无条件注册 cleanup、在 cleanup 里读 ref 判断状态"的写法**。`useEffect(() => () => { if (ref.current) ... }, [state])` 在 state 由 false→true 时，会先执行上一次的 cleanup，而 ref 此时已是新值 true，于是清理动作立刻发生——本项目表现为"进入定位态后示例窗刚弹出就被自己收掉"。正解：`if (!state) return;` 只在状态成立时注册 cleanup，进入时不会再误触发。

- 2026-11-21（原生提示实验 / 显示状态机）：**不要用"排队消息 + 缓存 visible"管理显示/隐藏**。`hide` 与 `show` 各发一条消息时，先发的 hide 可能后到并作用在刚弹出的窗口上（"瞬间出现又消失"）；缓存状态还会与实际窗口不同步（"该显示不显示"）。正解：命令线程只写一个"最新意图槽"并递增序号（显示 / 隐藏），窗口线程每被唤醒只取最新一条，序号不比已执行的更新就丢弃；`start()` 无条件 `ShowWindow`，隐藏用 `hide_now()` 统一复位。

- 2026-11-21（原生提示实验 / 定位态收尾）：编辑态的生命周期必须**两端都闭合**——进入时只接受预览、退出时**立刻拒绝迟到的预览请求**（后端判 `preview && !positioning` 直接丢弃），否则滑块拖动产生的在途请求会在退出后又把示例弹回来，表现为"闪一下示例再弹正常窗"。前端同时用 ref 标记编辑态，让在途的滑块请求自己作废；退出顺序固定为「关压制 → 等 hide 落地 → 解除 UI」。

- 2026-11-21（原生提示实验 / 定位模式）：**「定位期间不弹窗」必须由后端统一压制**（`POSITIONING` 原子标志，`show()` 对非 preview 请求直接返回），不能只在前端禁用按钮——将来提示会由快捷键/后台事件触发，前端按钮只是其中一条入口。
- 2026-11-21（原生提示实验 / 重复动画）：淡入过程中又收到 show 请求时，**不能重设 `phase_started`/`alpha`**，否则会重放整段进入动画（表现为"弹两次"）。判据：`visible && phase == FadeIn` → 沿用当前进度；定位预览直接 `alpha=1 / phase=Hold`（不播进入动画）。

- 2026-11-21（原生提示实验 / 进入动画卡顿）：**淡入阶段不要推进弹动动画**。弹动会改变内容指纹 → 触发整卡片重绘（填色 + 文字 + 逐像素遮罩），在 300ms 里重画十几次就会出现"慢且卡"；退出阶段内容不变，只换透明度，所以一直顺。做法：淡入期间冻结内容（pop 固定 0），进入 Hold 后再从 0 播到 1。
- 2026-11-21（原生提示实验 / 重复触发）：`start()` 判断"已显示"不能只看 `visible && alpha>0`——淡出途中满足这一条会走"只更新内容"分支，导致窗口卡在退出过程的半透明状态。判据改成 `phase == Hold && alpha >= 0.999`，其余情况从当前 alpha 续上淡入（`fade_from`）。

- 2026-11-21（原生提示实验 / 动画手感）：`WM_TIMER` 的实际粒度约 15.6ms 且会被合并，用它驱动淡入会出现「一顿一顿」。做法：`timeBeginPeriod(1)` 把系统计时精度提到 1ms，再把动画定时器设成 8ms；退出时 `timeEndPeriod(1)`。另外**缓出曲线不适合做进入动画**（前 1/4 时间就到 58% 透明度，肉眼像瞬入），进入用缓入缓出 `t²(3-2t)`，退出保持原曲线。

- 2026-11-21（原生提示实验 / 性能）：**分层窗口不能每帧重画整张卡片**。原来每个 tick 都 `CreateDIBSection` + 重画 + `apply_alpha_mask`（逐像素 sqrt）→ 开销随窗口面积线性涨，表现为「缩放越大动画越卡、0.8 最顺」。改法：缓存已渲染帧（内容指纹不变就只换 `SourceConstantAlpha` 重新 `UpdateLayeredWindow`），淡入淡出阶段只做合成、不再重画；跳动动画按 12 档量化，避免每 tick 都重画。

- 2026-11-21（前端白屏）：`setState((current) => ...)` 的 updater 里**不能读合成事件**（`event.currentTarget.value/checked`）。React 会延迟执行 updater，执行时事件的 `currentTarget` 已被置空 → 渲染期抛 `TypeError: Cannot read properties of null (reading 'value')` → 整棵树卸载成白屏。正确写法是先在事件回调里取值（`const value = Number(event.currentTarget.value)`），再在 updater 里用这个局部变量。排查手段：错误边界 + `logs/frontend.log`（`frontend_error_log` 命令）。

- 2026-11-21（原生提示实验 / 测试链）：测试二进制里一旦出现 `tauri::AppHandle` + `emit`，链接器会把 rfd 的 `TaskDialogIndirect` 拉进来；test exe 没有 v6 公共控件清单，加载阶段直接 `STATUS_ENTRYPOINT_NOT_FOUND (0xc0000139)`。定位方法：`dumpbin /imports` + 对每个导入 `LoadLibrary/GetProcAddress` 找缺失项。结论：需要 AppHandle 的生产逻辑用 `#[cfg(not(test))]` 挡在测试构建之外。
- 2026-11-21（原生提示实验）：拖动定位时必须在每帧重绘前把窗口新坐标写回 payload（paint 每帧都按 payload 重新定位），否则窗口会被弹回原位；拖动模式要临时去掉 `WS_EX_TRANSPARENT`，否则收不到鼠标消息。
- 2026-11-21（原生提示实验）：实时背景刷新必须先 `SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE)`，否则抓屏会把自己抓进去形成拖影。

- 2026-11-21（原生提示实验）：**窗口类名在进程内只能注册一次**。停用工具销毁工作线程后再次启用，`RegisterClassExW` 会返回 0 并置 `ERROR_CLASS_ALREADY_EXISTS`(1410)；把它当失败就会「停用后再启用永久弹不出来」。同类问题也会出现在任何「懒创建 + 可销毁」的 Win32 窗口上。
- 2026-11-21（原生提示实验）：分层窗口自绘阴影时，距离衰减必须写成 `1.0 - dist/spread`；写成 `1.0 + dist/spread` 再 clamp，卡片外会恒等于最大不透明度，表现为「卡片四周有一整块黑底」。
- 2026-11-21（原生提示实验）：图标分类别用 `contains("ctrl+a")` 这类子串匹配——`Win+Ctrl+Alt+…` 会误命中 `ctrl+a`，所有带 Alt 的组合都被画成复制图标；必须整串比较。
- 2026-11-21（原生提示实验）：窗口类名是每个进程各自注册的，`FindWindowW(类名, null)` 会连到**别的进程**的同类名窗口（开发版实例里就有一个）；测试里要按 `GetWindowThreadProcessId` 过滤本进程。

- 2026-11-21：**WebView2 挂起不是省内存的手段**。`ICoreWebView2_3::TrySuspend` 接在主窗口隐藏路径后，实测渲染进程 WS 只从 94.1MB 掉到 91.0MB（Private 49.4→45.6），CPU 无可测变化——不可见渲染器本来就被 Chromium 节流，挂起主要省的是「页面还在跑的定时器/动画」。同时挂起期间页面不处理事件，隐藏窗口上 `emit` 会丢。已回退，不要再把它当优化项重提。
- 2026-11-21：量占用先认窗口尺寸再下结论。截图里那个「隐藏却常驻 58MB 的渲染进程」是**快捷键提示浮窗**（560×56 的 Tauri Window），剪贴板弹窗是关窗即销毁、不常驻；之前按名字猜成弹窗，结论会带偏优化方向。
- 2026-11-21：验证脚本两条硬约束：① `.ps1` 必须 ASCII-only（PS 5.1 无 BOM 时按 ANSI 读，中文标签会变乱码且可能破坏解析）；② `List[string].Add(...)` 不能直接塞 `-replace`/`-split` 产生的数组，要先 `[string]::Join()`，否则报「找不到 Add 的重载」并让整个脚本静默失败。另外 `keybd_event` 注入 Win 组合键不稳定（前台是提权控制台时偶尔吞掉），验证窗口唤出优先用「再启动一次走单实例」或 `WM_CLOSE` + `EnumWindows` 轮询，别把注入失败当成功能坏了。
- 2026-11-21：快捷键匹配**必须比规范化后的按键标识**，不能拿存储原值直接和回调文本比：存储侧是前端 `KeyboardEvent.key`（`/`、`.` 原样），回调侧是 `Shortcut::to_string()` 的 `Code` 名（`Slash`、`Period`）。同时**快捷键回调的兜底分支绝不能默认「呼出主窗口」**——未匹配就只记录，否则任何匹配失败都会表现成「按了没设过的键却弹出主窗口」。新增按键别名时改 `canonical_key_token` 一处即可。
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
