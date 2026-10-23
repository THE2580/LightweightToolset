# Goal

- 完成项目从 Codex 到 DSH 的接管：把 `LightweightToolset` 完整迁入 `E:\DeepSeek_Harness\Workspace\LightweightToolset`，补齐项目级 `AGENTS.md`，并把 `agent_memory/` 从本机忽略状态改为随仓库版本控制。

# Current Focus

- 状态：迁移与治理完成，已本地提交（`docs: add agents guide and track project memory`），未推送，`master` 领先远程 1 个提交。
- 2026-10-23 完成：完整复制（含 Git 历史、`node_modules`、`target`）；新增 `AGENTS.md`；`.gitignore` 移除 `/agent_memory/`；五个记忆文件按新现场刷新。
- 迁移后验证通过：`git log`/`git status` 与原仓库一致；`tsc`、`npm run build`、`cargo check` 均通过；串行库测试 12/12 通过。
- 保留现场：`M src-tauri/src/window_service.rs`（纯格式化噪声）、`?? .codex/`，不处理、不提交。

# Remaining

- 推送 `master` 需用户明确授权；后续版本发布同样需授权。

# Verification

- `git log --oneline -n 3`：`7f9d66d`（v0.4.4）、`f3e47e6`、`2a464cc`，与原仓库一致。
- `npx tsc --noEmit` 退出码 0；`npm run build` 成功（7.27s）；`cargo check` 成功（3m48s）。
- 成功标准：项目可在新路径直接开发；记忆缺失/丢失风险消除（入库）；规范可被下一个线程直接读取。

# Resume Here

- 先执行 `git status --short` 与 `git log --oneline -n 3`，确认提交存在且工作树只剩既有现场（`M src-tauri/src/window_service.rs`、`?? .codex/`）。
- 进入等待状态；用户给出新任务时，先读 `AGENTS.md`、`context.md`、`progress.md`、`bugs.md`，再读相关源码。
- 需要推送时先取得用户明确授权。
