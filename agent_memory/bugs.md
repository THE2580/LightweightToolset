# 已知问题 / 风险

- **P0 已关闭：** AppState 在首次快照前注册，首次快照释放 registry 锁后才启动后台 runtime；`v0.4.3` 的错误字符串轮询已删除。20 次压力样本异常 `0/20`，用户多次真机启动最快 `775ms`；管理员游戏输入、helper 生命周期及窗口置顶托盘退出清理全部通过。
- 同一 App ID 同时存在已安装版与测试 EXE 时，Computer Use 可能连接到已安装版；必须核对进程完整路径，由 shell 精确启动待测 EXE，并把控制层限制为窗口观察。错误夹具产生的恒定 `1995ms/2190ms` 样本已作废。
- watchdog EXE 首次复制/扫描与管理员输入 helper 启动耗时波动较大，但现已后台运行，不再允许其阻塞首屏；后续重点验证生命周期和失败状态上报。
- 管理员输入 helper 使用回环 TCP 与会话令牌防误连接，但同一用户可读取进程命令行，不作为跨用户安全边界；UAC 拒绝或 helper 失败应回退普通权限采集。
- 窗口置顶管理员目标受 UIPI 限制可能返回 Win32 5；这是已接受的第一版边界。
- 窗口置顶监护不保证开发终端关闭、系统同时杀死监护进程或关机场景；正式安装版任务栏结束任务和托盘退出已验收。
- 全仓 `cargo fmt --check` 与严格 clippy 仍受既有格式/lint 问题影响；不得为当前任务扩大范围。针对修改文件做局部 rustfmt 和匹配检查。
- `input_monitor` 多个测试共享全局按键状态，并行 `cargo test --lib` 曾偶发一次修饰符断言失败；目标单测与 `--test-threads=1` 串行 12/12 均通过。未确认产品缺陷，不在本 P0 中扩展修复。
- 工作树长期存在 `.codex/` 未跟踪目录与 `src-tauri/src/window_service.rs` modified/行尾噪声；除非用户另行授权，不处理、不回滚、不提交。
- `agent_memory/` 已于 2026-10-23 从 `.gitignore` 移除并纳入版本控制；此前 Codex 时期的记忆从未入库，迁移前只存在于本机，这是记忆易丢失的根因，现已消除。
- `.codex/config.toml`（`sandbox_mode = "danger-full-access"`）是 Codex 遗留本地配置，DSH 不读取；保持未跟踪，不提交、不复制到其他环境。
- Git Bash 的 PATH 中没有 `npm`（仅 `node`）；构建命令需在 PowerShell 执行，否则会把环境缺失误判为构建失败。
- 本机 DSH Windows 沙箱下 `vite build` 会因 esbuild 子进程管道返回 `spawn EPERM`；这是沙箱限制，不是项目故障。验证时可用 `npx tsc --noEmit`，或对 `npm run build` 一次性放宽沙箱权限。
- 2026-10-31 发现：系统 `%TEMP%`（`C:\\Users\\THE\\AppData\\Local\\Temp`）对测试子进程返回 `Os { code: 5, PermissionDenied }`，导致 `window_pinner` 3 个使用 `env::temp_dir()` 的测试失败；PowerShell 本身可正常写该目录。将 `TEMP`/`TMP` 指向 `src-tauri/target/test-tmp` 后 12/12 全部通过。属环境问题，不是代码回归；后续跑测试若再遇此错误，先改临时目录。
- 2026-10-31 发现（重要）：**在工作区内编译出的 EXE 会被 DSH 执行沙箱限制为只能写工作区**。直接 `npm run tauri dev` 会在 setup 阶段 panic：`保存存储定位文件失败: 拒绝访问 (os error 5)`；同一 EXE 复制到 `%TEMP%` 或 `E:\devtools` 运行正常。可用启动方式：先 `npm run dev` 起 Vite，再把 `src-tauri/target/debug/lightweight-toolset.exe` 复制到 `%TEMP%` 下运行。
- 2026-10-31 提权后的已知尾巴：此前生成的管理员输入 helper 副本（`%APPDATA%\com.the.lightweight-toolset\elevated_input_helper_*.exe`）不会自动删除，可在确认新版本稳定后手动清理；`window_pinner_watchdog_*.exe` 仍由看门狗机制管理，不受影响。
- 分词已定稿但仍有已知不完美（2026-10-31 与用户确认接受）：① 序号 `1.` `2.` 因通用字符类包含 `.`，会带句点成词；② `×N` 中 `×` 被当作非字母符号过滤，只剩 `N`；③ `Intl.Segmenter` 保留“的/和/就”等单字虚词。若后续要优化，优先去掉序号末尾标点、把 `×N` 合并为整词，再考虑虚词过滤。
- 2026-10-31 布局陷阱：本机显示缩放 135%，主窗口 912×598 的 CSS 视口只有 676×443；给固定高度弹窗写死大尺寸会溢出窗口。弹窗高度需用 `100vh` 兜底，不能只按逻辑像素估算。
- 2026-11-02 **已解决（helper 采键进程）**：上述限制通过独立采键子进程绕开——同 exe 带 `--shortcut-tip-helper <port>` 启动、不初始化 WebView、装钩子后经回环 TCP 回传组合键；实测本软件前台时 `Win+D` / `Win+I` / `Win+Shift+S` / PixPin `Ctrl+1` 都能触发提示，录制也正常。下文保留作为问题背景与证据。
- 2026-11-02 **（历史背景）Chromium 前台 + 被抢按键**：Chromium/WebView2 窗口占据前台时，低级键盘钩子不派发；而 `Win+D`、`Win+I`、`Win+Shift+S` 等系统热键，以及其他软件注册的全局热键（如 PixPin 的 `Ctrl+1`），会被系统/他进程先消费，页面 `keydown` 也收不到。实测证据：A/B 对照（禁用/启用 Raw Input 通道）结果完全一致；Raw 通道能收到普通键（`Ctrl+C`/`Ctrl+V`/`Ctrl+Shift+T`），但**一条 Win 组合键都收不到**。故「本软件窗口为前台时，被注册走的快捷键不提示」是确定的限制；根治需要把采键放进非 Chromium 的独立进程（helper），代价是多常驻进程与 UAC/IPC 复杂度，用户决定先接受、暂不做。参照：Keycastr（Tkinter 原生窗口、非提权）不受该限制，但看不到管理员窗口。
- 2026-10-31 通知限制（已随后续提权更新）：当前主程序整体以 High 完整性运行，同级及更低完整性窗口的 `Ctrl+C / Ctrl+X / Ctrl+V` 都能触发提示；只有 UAC 安全桌面与 System 完整性进程不可见。浮窗仍只出现在主显示器。
- 2026-10-31 通知确认机制：复制/剪切必须处于 800ms 意图窗口内且 `WM_CLIPBOARDUPDATE` 到达才提示；程序化写入不提示。**2026-11-01 修订**：应用内复制按钮（`clipboard_copy` / `clipboard_copy_text` / `clipboard_copy_derived_text`）经 `notify_copied_now()` 直接提示“已复制”；应用内 `Ctrl+C/X/V` 由页面上报后直接提示，只有其他程序仍走 800ms 确认。
- 2026-11-01 正式版「关于」页版号写死为常量 `0.4.4`，`v0.5.0` 发版后仍显示旧版号并误报更新（用户截图确认）。已改为 `src/App.tsx` 直接 `import packageJson from "../package.json"` 取 `version`；**不要**用 Vite `define` 注入全局常量——打包时会替换、dev 服务器不替换，会导致开发版白屏。
- 2026-11-01 重要：**WebView2（Chromium）窗口聚焦时，Windows 会间断地不给低级键盘钩子派发按键**（实测：`C` 键到了，`Ctrl` 键一次都没到，导致意图判定不成立）。全站已有公开记录（<https://dev.to/wudaming00/windows-silently-stops-delivering-whkeyboardll-hook-events-when-a-chromium-window-has-focus-and-bi8>）。因此软件自身窗口的快捷键改由页面 `keydown` 上报（`src/shortcutNotify.ts` → `clipboard_notify_shortcut` 命令），全局钩子只负责其他程序。
- 2026-11-01 通知积压现象：长按快捷键时页面侧每个自动重复都上报一次，后端逐条处理（每条都要重设窗口位置），形成队列，表现为**松手后累加仍在继续**。修法：页面上报与钩子共用 0.5s 重复节流 + 400ms 跨来源去重；应用内复制/剪切/粘贴改为上报后立即提示，不再等 800ms 剪贴板确认（WebView2 剪切写剪贴板慢，会超时丢提示）。
- 2026-11-01 通知浮窗置顶：只在创建时设过 `always_on_top`，压不过同样置顶的窗口（含本软件自己）。已改为每次显示都调用 `SetWindowPos(HWND_TOPMOST)`。
- 2026-11-01 `rustfmt <file>` 在本仓库会按默认 100 列重排整文件（仓库代码按更宽行宽书写，且无 `rustfmt.toml`），随手格式化会把无关文件改出上百行噪声；只允许手工保持风格，误格式化后立即 `git checkout --` 还原。
- 开发版与已安装版共享同一 App ID 与数据目录（当前定位文件指向 `D:\轻量化工具集`），本机测试会读写真实剪贴板数据；执行“全部清空”等破坏性操作前要确认数据归属。

# 失败尝试 / 限制

- `v0.4.3` 前端有限轮询只把“立即错误”变成“空壳后成功”，未修复状态注册顺序或启动性能。
- 仅增加普通权限低级键盘 Hook 无法覆盖 High 游戏输入；已采用按需管理员 helper，不能回退到继续堆叠普通输入 API。
- 仅隐藏主窗口直到全部同步初始化完成可以遮住空壳，但会保留 1.2–4.6s 实际启动回归，不是合格修复。

# 已知修复线索

- 先用 `[DEBUG-startup-*]` 单调时钟区分：存储/工具 init、watchdog 清理/复制/启动、每个 enabled tool、管理员 helper、快捷键、`app.manage`、首次快照。
- 基础设置与工具定义加载后应尽早注册可查询 AppState；watchdog、helper 和非首屏 worker 应在状态可用后启动，并通过 runtime ready 状态/事件更新。
- helper 仍需保持主程序 Medium、按需 UAC、IPC 断开/停用/退出时可靠结束；watchdog 的正式版异常清理能力不能因性能修复丢失。

# 已确认经验

- 修饰符双计数根因是 Raw Input 通用 VK 与 Hook 分侧 VK 不一致；共享入口规范化 Shift/Ctrl/Alt 后去重，已通过测试和真机验收。
- High 游戏输入根因是 Windows 完整性隔离；按需 `runas` helper 已验证 Minecraft Java、原神、绝区零、米哈游启动器和 PCL2。
- 窗口置顶强制终止后无法靠主进程回调清理；独立 watchdog 是必要能力，但其启动不能阻塞基础页面状态。
