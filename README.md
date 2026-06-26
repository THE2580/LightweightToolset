# LightweightToolset

基于 Tauri 2、Rust、React 和 TypeScript 的 Windows 轻量化桌面工具集新路线。旧 Electron 项目仅作为稳定参考与迁移对照；新功能和工具只在本仓库开发。

## 当前阶段

第一阶段建立工具集基础框架：固定主窗口、唯一托盘、单实例、内部工具注册、工具生命周期、统一快捷键、设置持久化和性能基线入口。

当前注册的两个工具仅用于验证生命周期，不包含具体业务功能。

## 约束

- 仅支持编译期内部工具注册，不提供插件系统、远程代码加载、第三方工具市场或运行时安装外部工具。
- 禁用工具必须注销快捷键、停止后台 worker、关闭其窗口并移除入口。
- 当前只配置 Windows x64 NSIS 安装包目标；release 打包和远端推送需用户主动要求。

## 开发

环境：Windows、Rust、Node.js、npm、WebView2 Runtime。

```powershell
npm install
npm run tauri dev
```

基础静态检查：

```powershell
npm run build
cargo check --manifest-path src-tauri\Cargo.toml
```
