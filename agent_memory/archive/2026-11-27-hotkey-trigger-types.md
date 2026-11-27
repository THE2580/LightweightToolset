# v0.8.4 快捷键触发类型 —— 实现明细（归档于 2026-11-27）

> 本文件是 v0.8.4 开发期间的待办/验收记录原文，发布后从 `progress.md` 顶部移入归档。
> 结论见 `progress.md` 的「当前阶段：v0.8.4 已发布」。

# 待办

> **批次划分（2026-11-27）**：用户把原「快捷键提示的类型切换」拆成**单独一批（批次 B，口径待确认，未开工）**；当前批次 A 先做「非录制阶段的快捷键冲突提示」——方案由用户从候选里选定后再动手。

1. ~~窗口置顶启用被快捷键注册失败阻塞~~ **已修（2026-11-27，用户已验证通过）**。
   - 修法：`ToolRegistry::start()` 里工具自身热键注册失败不再 `?` 中断，改为记日志 + 发 `tool-hotkey-warning` 事件（前端 App 监听后用轻提示展示），工具照常启用；`resume_shortcuts` 同步改为“逐条尽力恢复”，单个键被占用不会让其余快捷键一起失效。
   - 实测本机 `Ctrl+Alt+T` 仍被占用（`RegisterHotKey` 返回 1409，大概率是同时运行的安装版 v0.7.2），所以启用窗口置顶会成功，但会提示“快捷键不可用”，需要换键或停掉占用方。

2. **（批次 B）快捷键提示的触发类型** —— 最终按用户要求：**自动识别键形态，不手动切换**（2026-11-27，**已通过用户验收**）。
   - 提示始终只监听、不注册、不独占；不引入类型字段，录到什么形态就按什么形态匹配（组合键 / 单个 F1–F24 / 单键 / 鼠标侧键）。
   - 录入弹窗不显示类型按钮，正文只有录制框 + 文案输入。
   - **补齐按键白名单**（CapsLock 录不进去的根因）：提示工具自己有独立按键命名表，之前只覆盖字母/数字/F1–24/方向键/Tab/Enter/Space/Esc/Backspace/Insert/Delete/Home/End。已补 `CapsLock`/`NumLock`/`ScrollLock`/`PrintScreen`/`Pause` 与 OEM 符号键（`; = , - . / \` [ ] ' \\`），三处同步：钩子 `key_name`、后端 `normalize_key_name`、页面 `shortcutNotify.ts` 的 `KEY_NAMES`；并加了单测锁定。
   - 鼠标侧键：主进程提示钩子新增 `WH_MOUSE_LL`，平时按侧键弹提示（不吞键），录制时回传 `Mouse4/Mouse5` 并吞掉该次点击（防 WebView 前进/后退）；`canonical_chord` 支持 `Mouse4/Mouse5`；列表与原生浮窗键帽显示「鼠标侧键1/2」。
   - 预设提示（绑定本软件快捷键）键固定，不参与自动识别。
3. **（批次 A）非录制阶段的快捷键冲突提示**：用户选定 **C 方案（行内红色标记），且只要“被占用 / 配置冲突”时显示红标，不显示绿/灰状态**。
   - **已实现并通过用户验收（2026-11-27，“边框红色态”）**：`ToolSnapshot.hotkey_error`（工具与软件级快捷键都带）由后端计算——配置冲突优先（同一物理键被多个功能配置，提示“与「XXX」使用了同一个按键”），其次为最近一次注册失败的友好原因；前端**不再加额外图标标记**，改为非录制态把 `.hotkey-box` 的 2px 边框转红**且改虚线**（`--danger`，深色主题单独覆盖），hover title 显示原因；注册成功/清空/切类型/停用工具时清除。启用工具时的 `tool-hotkey-warning` toast 收敛为“「工具名」存在快捷键冲突或被其它程序占用”，不再展开键名与原因。
   - **修复“改键/清空后仍显示占用”**：根因是前端 `item.hotkeyError ?? tool.hotkeyError` 的 `??` 把后端返回的 `null`（无错误）当成空值、回退到导航时旧快照的陈旧错误；改为只读实时 `tools` 的 `hotkeyError`，并对空键值强制不显示红（前端 + 后端 `hotkey_error()` 双重兜底）。
   - 检测边界：监听型不注册，无法感知外部占用；占用方退出后需要一次重试（重新保存、重进编辑或重启应用）标记才会消失。
   - 数据来源：`register_app_hotkeys` / `start` / `resume_shortcuts` 统一走 `try_register_hotkey` 记录；配置冲突由 `hotkey_conflicts()` 在快照时实时计算。

# 已验收：快捷键「触发类型」（2026-11-26，v0.8.3 之后）

**目标**：给快捷键加「触发类型」，只借 BetterGI 的类型与可录键位能力；**生效时机沿用本软件规则**。

1. **全局热键**：组合键 **和单个功能键**（F1–F24）——`RegisterHotKey` 独占。
2. **键鼠监听（新增类型）**：任意键盘单键 + 鼠标侧键；低级钩子；**不吞键**；**按下即触发、不做长按**。
3. **生效时机不变**：软件级（「呼出主窗口」等）= 进程启动即注册；工具级 = 随工具启停注册/注销。
4. **类型显式可切换**（选 B 方案，不做"按键形状自动判定"）。
5. **交互口径（用户明确要求）**：
   - 已有键值时切换类型 → 先弹**确认框**说明"会清空当前键值"，确认后切类型 + 清空键值并保持录制态；取消则不变；**不要**弹"切换失败"。
   - **类型按钮只在编辑态可点**（非编辑态显示但置灰），且点它**不能让录制框失焦**（否则触发"失焦即保存并退出编辑"）。
6. 注册失败给**友好提示**（键被占用 → "该键已被其它程序占用…请换一个键"），不要抛系统原文。

**本轮实现（已落地，未提交）**
- 存储：`AppSettings.hotkey_kinds`（`global`/`listen`，缺省 global，旧数据零迁移；启动时清理未知 id/非法值）。
- 监听服务：新模块 `hotkey_listener.rs`——`WH_KEYBOARD_LL` + `WH_MOUSE_LL`、独立线程 + `GetMessage` 循环（`PostThreadMessage(WM_QUIT)` 停止）、始终 `CallNextHookEx` 不吞键、`keys_down` 过滤自动重复、250ms 同键去抖、忽略注入事件；按 token → 多个快捷键 id 路由，命中后在线程里派发 `dispatch_hotkey_id`。
- 录制通道：`begin_hotkey_capture` / `end_hotkey_capture` + `hotkey-captured` 事件把鼠标侧键回传给前端；**只在「监听类型 + 录制态」开启**，录制期间吞掉 XBUTTON 防止 WebView 前进/后退；带 120s 超时兜底（界面崩溃也不会永久吞侧键）。
- 注册分派：`register_hotkey` / `unregister_hotkey` 按类型分流，替换所有 `global_shortcut().register/unregister` 调用点（`register_app_hotkeys`、`start`、`stop`、`resume_shortcuts`、`suspend_shortcuts`、`set_hotkey`、`clear_hotkey`、`set_hotkey_kind`）。
- 派发重构：`dispatch_hotkey(app, text)`（文本→id）+ `dispatch_hotkey_id(app, id)`（动作），全局热键与监听共用动作层。
- 校验：`global` 用 `normalize_hotkey`（放开单 F 键）；`listen` 用 `listen_key_token` 单键规范化（字母/数字/方向键/Tab/F1–24/常用符号/鼠标 4-5/CapsLock 等）。
- 跨类型冲突：`trigger_identity(kind, hotkey)` 统一标识，全局 `F12` 与监听 `F12` 判为同一键；前端冲突检查同口径（`hotkeyTriggerIdentity`）。
- 快照：`ToolSnapshot` 增加 `hotkey_kind`，前端类型按钮据此显示；`app_hotkey_snapshots(settings)` 读 `settings.hotkey_kinds`。
- 命令：`set_tool_hotkey_kind(tool_id, kind)` **单命令原子完成**「解绑 → 清空键值 → 切类型」（比"clear + set"两步少一次中间态、不会因旧键格式误报失败）。
- 前端：`HotkeyRowEditor` 增加**单个触发类型切换按钮**（显示当前类型、点击切到另一种；非编辑态禁用且完全静态，无任何 hover 反馈；`onMouseDown preventDefault` 保证录制框不失焦）、`HotkeyKindConfirmDialog`（portal + 标准 `.update-dialog`，整块遮罩与按钮 `onMouseDown preventDefault`）、按类型的录制/校验/保存、监听类型下订阅 `hotkey-captured`；录制框空态文案统一为「按下快捷键」。

**验证**
- `cargo test --lib -- --test-threads=1` → **64 passed / 3 ignored**（新增 7 条：单 F 键、监听显示、缺省类型、监听不参与文本派发、跨类型冲突、监听键 token、虚拟键映射、回调文本）。
- `npm run build` 通过；`cargo check` 仅 4 条既有 warning。
- 开发版已用最新 debug 构建启动（`%TEMP%\lwt-dev`，窗口正常渲染）。**用户已验收（2026-11-26）**：单按钮类型切换/置灰静态、确认框、录制框不失焦、鼠标侧键录制、命名键（Space/Tab/Enter/方向键等）录制、方向键不被导航吞、监听触发均通过。验收中发现并修复两处：① 监听映射表缺规范化标识自映射导致命名键无法录制；② 页面方向键导航（window 捕获阶段）抢走录制框的方向键。

**踩坑（保留，后续改这块必看）**
- 录制态点类型按钮/确认框按钮必须 `onMouseDown={e => e.preventDefault()}`，否则录制框失焦 → `saveHotkey()` → 退出编辑。
- `set_tool_hotkey_kind` 不能先按新类型校验旧键，顺序必须是**先清空键值再切类型**。
- `canonical_hotkey()` 依赖 `normalize_hotkey()`，单 F 键已放开；监听型仍要用 `listen_key_token`，不能复用组合键校验。
- 派发若按"快捷键文本"解析 id，监听侧的单键解析不到；监听必须直接带 **id**。
- `app_hotkey_snapshots(settings)` 是自由函数，取类型只能读 `settings.hotkey_kinds`。
- 实测：`F12` 被本机其它程序占用（`RegisterHotKey` 返回 1409，已转友好文案）；单键全局注册会**独占**，所以普通单键只走监听。
- 低级钩子回调里不能加锁做重活：先在钩子线程取路由快照并释放锁，再拉线程派发；线程消息队列要先 `PeekMessageW(PM_NOREMOVE)` 建好，否则 `PostThreadMessage(WM_QUIT)` 可能失败、`join` 卡住。
