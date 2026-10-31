# 当前阶段

- **当前无活跃 P0：** `v0.4.4` 已发布并完成真机验收，项目进入维护与功能迭代阶段，等待用户下一项任务。
- **P0 已完成并发布（历史）：** 修复 `v0.4.2` 引入、`v0.4.3` 仅以轮询掩盖的 AppState 冷启动竞态与首屏长尾；根因修复、release 界面验收、20 次冷启动压力统计及用户专项真机回归全部通过。

# 当前迭代（剪贴板优化）

- 2026-10-31：回收站新增“全部清空”，按钮位于回收站标题栏右侧，空回收站时禁用；点击后弹出二次确认（“将永久删除回收站内全部记录，删除后无法恢复”），确认才永久删除，取消不改数据；Toast 回报删除条数。后端新增命令 `clipboard_purge_trash`（返回删除条数并写调试日志）。
- 2026-10-31：剪贴板界面支持 `Ctrl+A` 全选当前页（历史、固定、回收站三处）；范围只限当前分页，不跨页；焦点在输入框/搜索框时不抢占；已全选时再次按下沿用原“全选”按钮语义取消。前端改动经 `tsc` 与 `npm run build` 验证，HMR 已生效，未提交。
- 2026-10-31：快捷弹窗标题栏改为单行紧凑布局，左侧加入仅图标的“历史 / 固定”分段切换（复用 `FileText` 与 `Pin` 图标），默认历史；列表按分段单独查询，固定条目不再混排；搜索只作用于当前分段；搜索占位符随分段变化；空态文案区分“暂无固定内容”。面板宽高仍由设置控制。经 `tsc` 与 `npm run build` 验证，未提交。
- 2026-10-31：快捷弹窗搜索改为全局搜索，并随搜索状态切换标题栏布局——搜索为空保持分段 + 短搜索框；有内容时隐藏分段、搜索框铺满、保留图钉与管理图标。搜索结果合并固定与历史，固定置顶并恢复“固定 / 最近”分组标签，分页按合并结果计算；搜索期间固定/取消固定立即刷新重排。清空搜索恢复原布局与搜索前分段。经 `tsc` 与 `npm run build` 验证，未提交。
- 2026-10-31：快捷弹窗搜索态布局切换补上过渡动画——工具栏改弹性布局，分段宽度 220ms 收起/展开并淡出，搜索框宽度随剩余空间连续变化，不再硬切；触发条件与搜索逻辑不变。经 `tsc` 与 `npm run build` 验证，未提交。
- 2026-10-31：快捷弹窗焦点驻留——搜索框失焦时若焦点仍在面板内（或落到面板空白），异步把焦点还给搜索框；详情/提取弹窗打开或窗口本身失焦时不抢焦点，保证按钮点击与弹窗内操作不受影响。经 `tsc` 与 `npm run build` 验证，未提交。
- 2026-10-31：通知细节修正——停留改 2s；去掉 DWM 系统材质/拉伸（窗口先露矩形框的根因），改为窗口全透明 + 纯 CSS 卡片参与淡入淡出；浮窗宽度随 ×N 位数增长并重新居中；按住快捷键时按 0.5s 节流累加；图标随累加播放缩放动效。已通过 `cargo build`、`tsc`、`npm run build`；开发版已重启待验证。
- 验证：`tsc --noEmit`、`npm run build`、`cargo check`、串行 12/12 单测通过；二次确认经 `tsc` 与 `npm run build` 验证，开发版 HMR 已生效；尚未做真机 UI 验收，改动未提交。

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

- 2026-10-31：正式采用整体提权（`requireAdministrator`，`src-tauri/windows-app-manifest.xml`），并移除按键统计的管理员输入 helper（TCP IPC、自我复制 EXE、`--elevated-input-helper` 分支、`ELEVATED_INPUT_ACTIVE` 门控及相关测试）。主程序 High 后进程内钩子已覆盖同级及更低完整性窗口；UAC 安全桌面/System 进程仍不可见。构建与 11/11 单测通过，待真机复测按键统计。
- 2026-10-31：剪贴板迭代 7 项全部实现并经用户验收，已提交并推送 `origin/master`（`feat: improve clipboard workflows and notifications` + `fix: refine clipboard token extraction and settings dialog`，HEAD `76f438f`）。

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
2. 剪贴板迭代第 1 项“回收站全部清空”已完成，等待用户确认效果与后续条目；改动未提交。
3. 待确认：后续是否继续 `master` 直推，或引入功能分支/PR 流程。

# 最近维护

- 2026-10-31：完成剪贴板回收站“全部清空”功能（前后端）；期间发现系统 `%TEMP%` 对测试子进程拒绝写入，用工作区内临时目录跑通全量单测，已记入 `bugs.md`。
- 2026-10-23：DSH 接管迁移完成；新增 `AGENTS.md`，`.gitignore` 移除 `/agent_memory/`，记忆文件按新工作区现场刷新；迁移后 `npm run build` 与 `cargo check` 验证结果见 `working_state.md`。
- 2026-07-13：确认 `src-tauri/target` 共 `38.298 GiB`，其中累计的 `target/debug` 占 `35.815 GiB`；已仅清理 debug 并保留 release，清理后 target 为 `2.482 GiB`。

# 下一步

- 新线程从 `checkpoint.md` 的 `Resume Here` 开始。未经用户明确要求，不提交、推送或发布。
- 后续定期测量 `src-tauri/target` 占用；只在明显膨胀时按需清理 debug，不清理 release。
