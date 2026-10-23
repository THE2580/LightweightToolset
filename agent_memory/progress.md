# 当前阶段

- **当前无活跃 P0：** `v0.4.4` 已发布并完成真机验收，项目进入维护与功能迭代阶段，等待用户下一项任务。
- **P0 已完成并发布（历史）：** 修复 `v0.4.2` 引入、`v0.4.3` 仅以轮询掩盖的 AppState 冷启动竞态与首屏长尾；根因修复、release 界面验收、20 次冷启动压力统计及用户专项真机回归全部通过。

# 已确认事实

- 用户同机正式安装版 A/B/C 对照：
  - `v0.4.1`：冷启动 `804ms`，窗口出现时页面几乎完整。
  - `v0.4.2`：100% 在空壳页报 `state not managed for field 'state' on command 'get_app_snapshot'`，显示 `0/0`。
  - `v0.4.3`：最终可加载，但窗口先显示空壳约 `1.2–4.6s`；截图样本 `2297ms`。
- 主窗口在 `tauri.conf.json` 中默认可见；`app.manage(AppState)` 位于 `.setup()` 尾部，排在存储迁移、各工具 init、`window_pinner::start_watchdog()`、`registry.start_enabled()` 和快捷键注册之后。
- 页面“冷启动 N ms”在首次成功生成 `AppSnapshot` 时取 `process_started_at.elapsed()`；`2297ms` 代表快照可用时间，不是窗口创建时间。
- `v0.4.0` 只新增窗口置顶状态初始化；`v0.4.1` 主要是主题/UI；`v0.4.2` 在状态注册前新增 watchdog 辅助 EXE 清理/必要复制/启动，并新增管理员输入链。
- 2026-07-13 release 单次基线：setup `2761ms`，watchdog `680ms`，管理员 helper `1925ms`，首次快照 `3467ms`。
- 单变量样本：跳过 watchdog 时 setup `518ms`/首次快照 `1202ms`；跳过 helper 时 setup `183ms`/首次快照 `1043ms`。已确认两条非首屏必需进程启动链同步阻塞 AppState 注册，其中 helper 是主要波动源，watchdog 首次复制/扫描形成额外长尾。

# 已完成里程碑

- 五个工具已实现并发布；窗口置顶第一版、深浅主题、跨窗口主题同步、通知、管理员游戏输入和修饰符去重均已通过相应自动或真机验收。
- `v0.4.3` 已发布，但其 `200ms × 150` 精确字符串匹配轮询被确认只是绕过错误，不是有效根因修复。
- 项目记忆于 2026-07-13 完成压缩；旧阶段摘要见 `archive/2026-07-13-memory-compaction.md`。
- AppState 现于基础设置/registry 建立后立即注册；首次 `get_app_snapshot` 完成并释放 registry 锁后，以原子标记只启动一次后台 runtime。watchdog、开机自启、enabled tools/helper 与快捷键不再阻塞首屏。
- 前端已删除 `200ms × 150` state-not-managed 轮询，改为立即单次快照 + `app-runtime-state-changed` 完成事件；首页显示 runtime starting/ready/error。
- 清理后最终 release 界面验收：冷启动 `1208ms`、`5/5` 工具运行、基础服务运行中，无 state-not-managed、无 `0/0` 空壳。
- 指定 release EXE 的 20 次压力冷启动：最小 `1073ms`、中位数 `1145.5ms`、平均 `1160.1ms`、P95 `1284ms`、最大 `1313ms`；竞态、空壳和启动异常 `0/20`。样本以强制结束后等待 3 秒释放资源执行，不替代托盘正常退出专项验收。
- 用户于 2026-07-13 完成多次连续启动和真机专项验收：最快冷启动 `775ms`，截图显示基础服务运行中、`5/5` 工具运行；管理员游戏输入计数持续增加、停用按键统计后 elevated helper 退出、再次启用后新 helper 出现、窗口置顶后托盘退出能取消目标窗口置顶，全部通过。
- `v0.4.4` 已提交 `7f9d66d`、推送 `master` 与带注释标签，并发布 GitHub Release：`https://github.com/THE2580/LightweightToolset/releases/tag/v0.4.4`。资产 `LightweightToolset_0.4.4_x64-setup.exe`，SHA-256 `8570AED8E3E34B156F33BA9A1349AC14A20F51216B6DEB846277F59162C738B9`。
- 2026-10-23：项目从 Codex 工作目录完整迁移至 DSH 工作区 `E:\DeepSeek_Harness\Workspace\LightweightToolset`，Git 历史、`v0.4.4` 现场与构建缓存保持一致；新增项目级 `AGENTS.md`，`agent_memory/` 纳入版本控制。

# 待办

1. 无进行中的 P0；等待用户下一项需求。
2. 待确认：后续是否继续 `master` 直推，或引入功能分支/PR 流程。

# 最近维护

- 2026-10-23：DSH 接管迁移完成；新增 `AGENTS.md`，`.gitignore` 移除 `/agent_memory/`，记忆文件按新工作区现场刷新；迁移后 `npm run build` 与 `cargo check` 验证结果见 `working_state.md`。
- 2026-07-13：确认 `src-tauri/target` 共 `38.298 GiB`，其中累计的 `target/debug` 占 `35.815 GiB`；已仅清理 debug 并保留 release，清理后 target 为 `2.482 GiB`。

# 下一步

- 新线程从 `checkpoint.md` 的 `Resume Here` 开始。未经用户明确要求，不提交、推送或发布。
- 后续定期测量 `src-tauri/target` 占用；只在明显膨胀时按需清理 debug，不清理 release。
