# Scope

- 当前范围：从 Codex 工作目录迁移到 DSH 工作区后的接手、项目规范与记忆治理；业务代码未改动。
- 项目处于 `v0.4.4` 维护状态，P0 已关闭，等待用户下一项任务。

# Git / Working Tree

- 工作目录：`E:\DeepSeek_Harness\Workspace\LightweightToolset`（2026-10-23 自 `E:\codex_agent_project\LightweightToolset` 完整复制，含 `.git`、`node_modules`、`src-tauri/target`）。
- 分支：`master`；HEAD：`7f9d66d`（`v0.4.4`），与 `origin/master` 同步。
- 既有现场：`M src-tauri/src/window_service.rs`（已核实为纯 rustfmt 换行噪声，73/24 行）、`?? .codex/`（Codex 遗留配置）。两者均不处理、不回滚、不提交。
- 本次变更：新增 `AGENTS.md`；`.gitignore` 移除 `/agent_memory/`；`agent_memory/` 五个文件按新现场刷新。均已在本地提交（`docs: add agents guide and track project memory`），未推送；`master` 领先 `origin/master` 1 个提交。

# Active Files

- `AGENTS.md`：项目目标、技术栈、目录、命令、风格、禁止事项、分支/提交/发布规则。
- `.gitignore`：记忆不再忽略。
- `agent_memory/context.md`、`progress.md`、`bugs.md`、`working_state.md`、`checkpoint.md`。

# Verification

- 迁移一致性：新工作区 `git log` 与原仓库一致（HEAD `7f9d66d`），`git status` 同为 `M window_service.rs` + `?? .codex/`；体积 `node_modules=136M`、`target=2.5G`、`dist=639K`、`agent_memory=41K`。
- 2026-10-23 `npx tsc --noEmit` 通过；`npm run build` 在放宽沙箱后通过（`built in 7.27s`）。
- 2026-10-23 `cargo check --manifest-path src-tauri/Cargo.toml` 通过（`Finished dev profile in 3m 48s`，复用迁移前的 target 缓存）。
- 2026-10-23 `cargo test --lib -- --test-threads=1`：12 passed / 0 failed（编译 2m59s，测试 0.02s）。
- 沙箱限制：`vite build` 在默认沙箱下报 `spawn EPERM`，需放宽权限或改用 `tsc` 验证。

# Blockers

- 无。

# Next Action

- 等待用户下一项需求；首次真实开发前先读 `AGENTS.md` 与 `context.md`。
- 推送由用户明确授权后再执行；提交后工作树应只剩既有 `window_service.rs` 与 `.codex/` 现场。
