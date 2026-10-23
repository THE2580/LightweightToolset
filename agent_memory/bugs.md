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
