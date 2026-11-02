# 归档：2026-11-02 问题记录压缩

以下内容来自旧版 `bugs.md`，均已关闭或已成为历史背景；当前风险与经验见 `bugs.md`。

## 已关闭 P0（v0.4.2 冷启动竞态）

- AppState 在首次快照前注册，首次快照释放 registry 锁后才启动后台 runtime；`v0.4.3` 的错误字符串轮询已删除。20 次压力样本异常 `0/20`，最快真机冷启动 `775ms`。
- 失败尝试：`v0.4.3` 前端有限轮询只把「立即错误」变成「空壳后成功」，未修复状态注册顺序或启动性能；仅隐藏主窗口直到同步初始化完成会保留 1.2–4.6s 实际启动回归；仅增加普通权限低级键盘 Hook 无法覆盖 High 游戏输入。
- 修复线索（历史）：用 `[DEBUG-startup-*]` 单调时钟区分各启动阶段；watchdog、helper 与非首屏 worker 在状态可用后再启动；watchdog 的正式版异常清理能力不能因性能修复丢失。

## 管理员输入 helper（v0.5.0 已整体移除）

- 曾用回环 TCP + 会话令牌按需拉起 elevated helper 采集管理员窗口按键；v0.5.0 改为整体 `requireAdministrator` 后该 helper 与相关代码全部删除。
- 遗留文件：`%APPDATA%\com.the.lightweight-toolset\elevated_input_helper_*.exe` 不会自动删除，可手动清理；`window_pinner_watchdog_*.exe` 仍由看门狗管理。

## 通知机制（v0.5.x 演进，v0.6.0 已重构）

- 旧机制：复制/剪切需处于 800ms 意图窗口内且 `WM_CLIPBOARDUPDATE` 到达才提示，程序化写入不提示。v0.5.1 起应用内改为页面 `keydown` 直接提示；v0.6.0 起整体迁入「快捷键提示」工具，剪贴板只保留命令触发的文案提示。
- 长按积压：页面侧每个自动重复都上报，后端逐条处理形成队列，表现为松手后累加仍在继续；修法是同源 0.5s 节流 + 跨源 400ms 去重。
- 浮窗置顶：早期只在创建时设 `always_on_top`，压不过同样置顶的窗口；已改为每次显示 `SetWindowPos(HWND_TOPMOST)`，v0.6.0 起再加显示期间每 400ms 重压。

## Chromium 前台丢钩子（v0.6.0 已解决）

- 现象：Chromium/WebView2 窗口在前台时，Windows 会间断地不给 `WH_KEYBOARD_LL` 派发按键（实测 `C` 键到了、`Ctrl` 键一次没到）。
- 证据：A/B 对照（禁用/启用 Raw Input 通道）现象完全一致；Raw Input 能收到普通键，但一条 Win 组合键都收不到。
- 结论：根因不是权限，而是 Chromium 前台抑制 + 按键被抢先注册；解法是独立非 Chromium 采键子进程。参照 Keycastr（Tkinter 原生窗口、非提权）不受此限，但看不到管理员窗口。
