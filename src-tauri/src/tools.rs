use std::{collections::BTreeMap, sync::mpsc, thread};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

use crate::{
    app_usage, clipboard, hotkey_listener, key_usage, push_debug_log, screenshot, settings::AppSettings,
    shortcut_tip, timer, window_pinner, window_service, AppState,
};

/// 快捷键触发类型：`RegisterHotKey` 独占的全局热键。
pub const HOTKEY_KIND_GLOBAL: &str = "global";
/// 快捷键触发类型：低级键鼠钩子监听（不吞键、按下即触发）。
pub const HOTKEY_KIND_LISTEN: &str = "listen";

/// 触发类型归一：只接受 `global` / `listen`，其它值视为未知类型。
pub fn normalize_hotkey_kind(value: &str) -> Option<&'static str> {
    match value.trim().to_ascii_lowercase().as_str() {
        HOTKEY_KIND_GLOBAL => Some(HOTKEY_KIND_GLOBAL),
        HOTKEY_KIND_LISTEN => Some(HOTKEY_KIND_LISTEN),
        _ => None,
    }
}

struct ToolDefinition {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    default_hotkey: Option<&'static str>,
    default_enabled: bool,
    implemented: bool,
}

struct AppHotkeyDefinition {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    default_hotkey: &'static str,
}

const APP_HOTKEYS: [AppHotkeyDefinition; 6] = [
    AppHotkeyDefinition {
        id: "main_window",
        name: "呼出主窗口",
        description: "按下快捷键显示并聚焦主窗口",
        default_hotkey: "CTRL+ALT+M",
    },
    AppHotkeyDefinition {
        id: "screenshot_pin",
        name: "贴图",
        description: "在截图里钉住当前选区，不在截图时钉住最近一次截图",
        default_hotkey: "CTRL+ALT+P",
    },
    AppHotkeyDefinition {
        id: "pin_restore",
        name: "恢复上次关闭的贴图",
        description: "把上一次关闭的贴图重新贴出来",
        default_hotkey: "CTRL+ALT+R",
    },
    AppHotkeyDefinition {
        id: "pin_close_all",
        name: "关闭所有贴图窗口",
        description: "关闭屏幕上所有贴图",
        default_hotkey: "CTRL+ALT+X",
    },
    AppHotkeyDefinition {
        id: "pin_toggle_visible",
        name: "隐藏/显示所有贴图窗口",
        description: "在隐藏与显示之间切换所有贴图",
        default_hotkey: "CTRL+ALT+H",
    },
    AppHotkeyDefinition {
        id: "pin_passthrough",
        name: "鼠标穿透",
        description: "切换所有贴图窗口的鼠标穿透，默认不占用快捷键",
        default_hotkey: "",
    },
];

const TOOLS: [ToolDefinition; 7] = [
    ToolDefinition {
        id: "screenshot",
        name: "截图",
        description: "区域截图、标注与贴图，复制到剪贴板或保存为 PNG",
        default_hotkey: Some("CTRL+ALT+A"),
        default_enabled: false,
        implemented: true,
    },
    ToolDefinition {
        id: "clipboard",
        name: "剪贴板",
        description: "本地纯文本剪贴板历史与快捷弹窗",
        default_hotkey: Some("CTRL+ALT+V"),
        default_enabled: true,
        implemented: true,
    },
    ToolDefinition {
        id: "app_usage",
        name: "软件使用统计",
        description: "统计本机应用使用时长与活跃窗口",
        default_hotkey: None,
        default_enabled: false,
        implemented: true,
    },
    ToolDefinition {
        id: "key_usage",
        name: "按键使用统计",
        description: "统计物理按键次数、趋势与高频按键",
        default_hotkey: None,
        default_enabled: false,
        implemented: true,
    },
    ToolDefinition {
        id: "timer",
        name: "计时器",
        description: "正计时、倒计时与提醒管理",
        default_hotkey: None,
        default_enabled: false,
        implemented: true,
    },
    ToolDefinition {
        id: "window_pinner",
        name: "窗口置顶",
        description: "使用快捷键置顶当前外部窗口并集中管理",
        default_hotkey: Some("CTRL+ALT+T"),
        default_enabled: false,
        implemented: true,
    },
    ToolDefinition {
        id: "shortcut_tip",
        name: "快捷键提示",
        description: "按下自己配置的快捷键时，在任务栏上方浮窗提示本次按键",
        default_hotkey: None,
        default_enabled: true,
        implemented: true,
    },
];

struct RunningWorker {
    stop: mpsc::Sender<()>,
    thread: thread::JoinHandle<()>,
}

pub struct ToolRegistry {
    enabled: BTreeMap<String, bool>,
    hotkeys: BTreeMap<String, String>,
    /// 最近一次注册尝试的失败原因（快捷键 id → 友好文案）；注册成功时清除。
    /// 用于非录制阶段在快捷键行上显示红色状态标记。
    hotkey_errors: BTreeMap<String, String>,
    workers: BTreeMap<String, RunningWorker>,
    shortcuts_suspended: bool,
    settings: AppSettings,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolSnapshot {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    hotkey: String,
    /// 触发类型：`global` / `listen`。
    hotkey_kind: &'static str,
    /// 非空表示该快捷键当前不可用：被其它程序占用、解析失败或与其它功能配置冲突。
    hotkey_error: Option<String>,
    enabled: bool,
    implemented: bool,
    supports_hotkey: bool,
    worker_running: bool,
}

impl ToolRegistry {
    pub fn new(mut settings: AppSettings) -> Self {
        let known_tool_ids: Vec<&str> = TOOLS.iter().map(|tool| tool.id).collect();
        let known_hotkey_ids: Vec<&str> = known_tool_ids
            .iter()
            .copied()
            .chain(APP_HOTKEYS.iter().map(|hotkey| hotkey.id))
            .collect();
        settings
            .tools
            .retain(|tool_id, _| known_tool_ids.contains(&tool_id.as_str()));
        settings
            .hotkeys
            .retain(|hotkey_id, _| known_hotkey_ids.contains(&hotkey_id.as_str()));
        // 只保留已知条目且类型合法的记录；缺省（旧数据）不写入，读取时按 global 处理。
        settings.hotkey_kinds.retain(|hotkey_id, kind| {
            known_hotkey_ids.contains(&hotkey_id.as_str())
                && normalize_hotkey_kind(kind).is_some()
        });

        for tool in TOOLS.iter() {
            if tool.implemented {
                settings
                    .tools
                    .entry(tool.id.to_owned())
                    .or_insert(tool.default_enabled);
            } else {
                settings.tools.insert(tool.id.to_owned(), false);
            }
            if let Some(default_hotkey) = tool.default_hotkey {
                settings
                    .hotkeys
                    .entry(tool.id.to_owned())
                    .or_insert_with(|| default_hotkey.to_owned());
            } else {
                settings.hotkeys.remove(tool.id);
            }
        }
        for hotkey in APP_HOTKEYS.iter() {
            settings
                .hotkeys
                .entry(hotkey.id.to_owned())
                .or_insert_with(|| hotkey.default_hotkey.to_owned());
        }
        Self {
            enabled: settings.tools.clone(),
            hotkeys: settings.hotkeys.clone(),
            hotkey_errors: BTreeMap::new(),
            workers: BTreeMap::new(),
            shortcuts_suspended: false,
            settings,
        }
    }

    pub fn settings(&self) -> &AppSettings {
        &self.settings
    }

    pub fn settings_mut(&mut self) -> &mut AppSettings {
        &mut self.settings
    }

    /// 供启动通知等文案使用：返回当前设置的软件级快捷键文本（含默认值回退）。
    pub fn app_hotkey_text(&self, hotkey_id: &str) -> String {
        format_hotkey_display(&self.hotkey_for(hotkey_id))
    }

    pub fn start_enabled(&mut self, app: &AppHandle) -> Result<(), String> {
        for tool in TOOLS.iter() {
            if tool.implemented && self.is_enabled(tool.id) {
                self.start(app, tool)?;
            }
        }
        Ok(())
    }

    /// 归属某工具的软件级快捷键 id（复用快捷键提示的归属表，避免两份映射漂移）。
    fn app_hotkeys_owned_by(&self, tool_id: &str) -> Vec<&'static str> {
        APP_HOTKEYS
            .iter()
            .filter(|hotkey| crate::shortcut_tip::hotkey_owner_tool(hotkey.id) == Some(tool_id))
            .map(|hotkey| hotkey.id)
            .collect()
    }

    pub fn register_app_hotkeys(&mut self, app: &AppHandle) -> Result<(), String> {
        if self.shortcuts_suspended {
            return Ok(());
        }
        // 单个快捷键被别的软件占用时不能让其余快捷键一起失效，逐条注册并记录失败项。
        let mut failures: Vec<String> = Vec::new();
        for hotkey in APP_HOTKEYS.iter() {
            // 归属工具已停用：它的软件级快捷键也必须处于注销状态（工具生命周期完整性）。
            if let Some(owner) = crate::shortcut_tip::hotkey_owner_tool(hotkey.id) {
                if !self.is_enabled(owner) {
                    continue;
                }
            }
            let value = self.hotkey_for(hotkey.id);
            if value.is_empty() {
                continue;
            }
            let kind = self.hotkey_kind(hotkey.id);
            if let Err(error) = self.try_register_hotkey(app, hotkey.id, kind, &value) {
                failures.push(format!("{}: {error}", hotkey.id));
            }
        }
        if failures.is_empty() {
            return Ok(());
        }
        let message = format!("注册软件快捷键失败: {}", failures.join("; "));
        if let Some(state) = app.try_state::<AppState>() {
            push_debug_log(state.inner(), "hotkey", message.clone());
        } else {
            eprintln!("[hotkey] {message}");
        }
        Err(message)
    }

    pub fn set_enabled(
        &mut self,
        app: &AppHandle,
        tool_id: &str,
        enabled: bool,
    ) -> Result<(), String> {
        let tool = TOOLS
            .iter()
            .find(|tool| tool.id == tool_id)
            .ok_or("未知工具")?;
        if self.is_enabled(tool_id) == enabled {
            return Ok(());
        }
        if enabled && !tool.implemented {
            return Err("工具尚未实现".to_owned());
        }
        if enabled {
            self.start(app, tool)?;
        } else {
            self.stop(app, tool)?;
        }
        self.enabled.insert(tool_id.to_owned(), enabled);
        self.settings.tools.insert(tool_id.to_owned(), enabled);
        Ok(())
    }

    pub fn snapshot(&self) -> Vec<ToolSnapshot> {
        let conflicts = self.hotkey_conflicts();
        TOOLS
            .iter()
            .map(|tool| ToolSnapshot {
                id: tool.id,
                name: tool.name,
                description: tool.description,
                hotkey: self.hotkey_for(tool.id),
                hotkey_kind: self.hotkey_kind(tool.id),
                hotkey_error: self.hotkey_error(tool.id, &conflicts),
                enabled: self.is_enabled(tool.id),
                implemented: tool.implemented,
                supports_hotkey: tool.default_hotkey.is_some(),
                worker_running: self.workers.contains_key(tool.id),
            })
            .collect()
    }

    /// 软件级快捷键快照：`hotkey_error` 与工具行同一口径（配置冲突优先，其次注册失败）。
    pub fn app_hotkey_snapshots(&self) -> Vec<ToolSnapshot> {
        let conflicts = self.hotkey_conflicts();
        APP_HOTKEYS
            .iter()
            .map(|hotkey| ToolSnapshot {
                id: hotkey.id,
                name: hotkey.name,
                description: hotkey.description,
                hotkey: self.hotkey_for(hotkey.id),
                hotkey_kind: self.hotkey_kind(hotkey.id),
                hotkey_error: self.hotkey_error(hotkey.id, &conflicts),
                enabled: true,
                implemented: true,
                supports_hotkey: true,
                worker_running: false,
            })
            .collect()
    }

    pub fn set_hotkey(
        &mut self,
        app: &AppHandle,
        hotkey_id: &str,
        hotkey: String,
    ) -> Result<(), String> {
        let tool = TOOLS.iter().find(|tool| tool.id == hotkey_id);
        if tool.is_none() && !APP_HOTKEYS.iter().any(|hotkey| hotkey.id == hotkey_id) {
            return Err("未知快捷键".to_owned());
        }
        if tool.is_some_and(|tool| tool.default_hotkey.is_none()) {
            return Err("该工具不需要快捷键".to_owned());
        }
        let kind = self.hotkey_kind(hotkey_id);
        let normalized = normalize_hotkey_for_kind(kind, &hotkey)?;
        self.ensure_hotkey_available(hotkey_id, kind, &normalized)?;
        let previous = self.hotkey_for(hotkey_id);
        if previous == normalized {
            return Ok(());
        }

        let should_register = self.hotkey_should_register(hotkey_id);
        if should_register && self.shortcuts_suspended {
            // 录制期间快捷键已全部注销：全局热键先试注册一次再放掉，尽早暴露「键被占用」；
            // 监听型快捷键不独占按键，没有等价的可占用性检查。
            if kind == HOTKEY_KIND_GLOBAL {
                register_hotkey(app, hotkey_id, kind, &normalized)?;
                unregister_hotkey(app, hotkey_id, kind, &normalized);
            }
        } else if should_register {
            if !previous.is_empty() {
                unregister_hotkey(app, hotkey_id, kind, &previous);
            }
            if let Err(error) = self.try_register_hotkey(app, hotkey_id, kind, &normalized) {
                if !previous.is_empty() {
                    let _ = self.try_register_hotkey(app, hotkey_id, kind, &previous);
                }
                return Err(error);
            }
        }

        self.hotkeys
            .insert(hotkey_id.to_owned(), normalized.clone());
        self.settings
            .hotkeys
            .insert(hotkey_id.to_owned(), normalized);
        self.hotkey_errors.remove(hotkey_id);
        Ok(())
    }

    /// 切换快捷键的触发类型。
    ///
    /// **顺序固定为「先清空键值，再切类型」**：旧键值多半不符合新类型的格式
    /// （组合键 ↔ 单键），若先按新类型校验旧键就会误报「切换失败」。清空后保持
    /// 录制态由前端负责，这里只落库与解绑。
    pub fn set_hotkey_kind(
        &mut self,
        app: &AppHandle,
        hotkey_id: &str,
        kind: &str,
    ) -> Result<(), String> {
        let known = TOOLS
            .iter()
            .any(|tool| tool.id == hotkey_id && tool.default_hotkey.is_some())
            || APP_HOTKEYS.iter().any(|hotkey| hotkey.id == hotkey_id);
        if !known {
            return Err("未知快捷键".to_owned());
        }
        let kind = normalize_hotkey_kind(kind).ok_or("未知的触发类型")?;
        if self.hotkey_kind(hotkey_id) == kind {
            return Ok(());
        }
        self.unregister_hotkey_if_registered(app, hotkey_id);
        self.hotkeys.insert(hotkey_id.to_owned(), String::new());
        self.settings.hotkeys.insert(hotkey_id.to_owned(), String::new());
        self.settings
            .hotkey_kinds
            .insert(hotkey_id.to_owned(), kind.to_owned());
        self.hotkey_errors.remove(hotkey_id);
        Ok(())
    }

    pub fn clear_hotkey(&mut self, app: &AppHandle, hotkey_id: &str) -> Result<(), String> {
        let tool = TOOLS.iter().find(|tool| tool.id == hotkey_id);
        if tool.is_none() && !APP_HOTKEYS.iter().any(|hotkey| hotkey.id == hotkey_id) {
            return Err("未知快捷键".to_owned());
        }
        if tool.is_some_and(|tool| tool.default_hotkey.is_none()) {
            return Err("该工具不需要快捷键".to_owned());
        }
        self.unregister_hotkey_if_registered(app, hotkey_id);
        self.hotkeys.insert(hotkey_id.to_owned(), String::new());
        self.settings
            .hotkeys
            .insert(hotkey_id.to_owned(), String::new());
        self.hotkey_errors.remove(hotkey_id);
        Ok(())
    }

    /// 当前触发的规范化标识（跨类型判重用）：全局热键用 `normalize_hotkey` 的写法，
    /// 监听型用单键标识，两者共享同一套按键名，因此 `F12` 与监听 `F12` 会判为同一键。
    fn ensure_hotkey_available(
        &self,
        hotkey_id: &str,
        kind: &str,
        hotkey: &str,
    ) -> Result<(), String> {
        let Some(candidate) = trigger_identity(kind, hotkey) else {
            return Ok(());
        };
        let mut conflict = false;
        for tool in TOOLS.iter() {
            if tool.id == hotkey_id {
                continue;
            }
            let other_kind = self.hotkey_kind(tool.id);
            let other = self.hotkey_for(tool.id);
            if trigger_identity(other_kind, &other).as_deref() == Some(candidate.as_str()) {
                conflict = true;
                break;
            }
        }
        if !conflict {
            for app_hotkey in APP_HOTKEYS.iter() {
                if app_hotkey.id == hotkey_id {
                    continue;
                }
                let other_kind = self.hotkey_kind(app_hotkey.id);
                let other = self.hotkey_for(app_hotkey.id);
                if trigger_identity(other_kind, &other).as_deref() == Some(candidate.as_str()) {
                    conflict = true;
                    break;
                }
            }
        }
        if conflict {
            return Err("快捷键已被其他功能占用".to_owned());
        }
        Ok(())
    }

    pub fn tool_for_shortcut(&self, shortcut: &str) -> Option<String> {
        let normalized = canonical_hotkey(shortcut)?;
        TOOLS
            .iter()
            .find(|tool| {
                self.is_enabled(tool.id)
                    && self.hotkey_kind(tool.id) == HOTKEY_KIND_GLOBAL
                    && canonical_hotkey(&self.hotkey_for(tool.id)).as_deref()
                        == Some(normalized.as_str())
            })
            .map(|tool| tool.id.to_owned())
    }

    pub fn app_hotkey_for_shortcut(&self, shortcut: &str) -> Option<String> {
        let normalized = canonical_hotkey(shortcut)?;
        APP_HOTKEYS
            .iter()
            .find(|hotkey| {
                self.hotkey_kind(hotkey.id) == HOTKEY_KIND_GLOBAL
                    && canonical_hotkey(&self.hotkey_for(hotkey.id)).as_deref()
                        == Some(normalized.as_str())
            })
            .map(|hotkey| hotkey.id.to_owned())
    }

    /// 该 id 是否是软件级快捷键（而非工具 id）；派发时据此分流。
    pub fn is_app_hotkey(&self, hotkey_id: &str) -> bool {
        APP_HOTKEYS.iter().any(|hotkey| hotkey.id == hotkey_id)
    }

    pub fn only_enabled_tool(&self) -> Option<String> {
        let mut enabled_tools = TOOLS.iter().filter(|tool| self.is_enabled(tool.id));
        let tool = enabled_tools.next()?;
        if enabled_tools.next().is_none() {
            Some(tool.id.to_owned())
        } else {
            None
        }
    }

    pub fn suspend_shortcuts(&mut self, app: &AppHandle) -> Result<(), String> {
        if self.shortcuts_suspended {
            return Ok(());
        }
        for tool in TOOLS.iter() {
            if self.is_enabled(tool.id) && self.supports_hotkey(tool.id) {
                self.unregister_hotkey_if_registered(app, tool.id);
            }
        }
        for hotkey in APP_HOTKEYS.iter() {
            self.unregister_hotkey_if_registered(app, hotkey.id);
        }
        self.shortcuts_suspended = true;
        Ok(())
    }

    pub fn resume_shortcuts(&mut self, app: &AppHandle) -> Result<(), String> {
        if !self.shortcuts_suspended {
            return Ok(());
        }
        // 恢复注册按「逐条尽力」处理：单个键被占用不应让其余快捷键一起失效。
        let mut failures: Vec<String> = Vec::new();
        for tool in TOOLS.iter() {
            if self.is_enabled(tool.id) && self.supports_hotkey(tool.id) {
                let kind = self.hotkey_kind(tool.id);
                let value = self.hotkey_for(tool.id);
                if value.is_empty() {
                    continue;
                }
                if let Err(error) = self.try_register_hotkey(app, tool.id, kind, &value) {
                    failures.push(format!(
                        "{}={}: {error}",
                        tool.id,
                        format_hotkey_display(&value)
                    ));
                }
            }
        }
        for hotkey in APP_HOTKEYS.iter() {
            // 与 `register_app_hotkeys` 同口径：归属工具已停用时不再恢复注册。
            if let Some(owner) = crate::shortcut_tip::hotkey_owner_tool(hotkey.id) {
                if !self.is_enabled(owner) {
                    continue;
                }
            }
            let kind = self.hotkey_kind(hotkey.id);
            let value = self.hotkey_for(hotkey.id);
            if value.is_empty() {
                continue;
            }
            if let Err(error) = self.try_register_hotkey(app, hotkey.id, kind, &value) {
                failures.push(format!(
                    "{}={}: {error}",
                    hotkey.id,
                    format_hotkey_display(&value)
                ));
            }
        }
        self.shortcuts_suspended = false;
        if !failures.is_empty() {
            if let Some(state) = app.try_state::<AppState>() {
                push_debug_log(
                    &state,
                    "hotkey",
                    format!("tool.hotkeys.resume_failed {}", failures.join("; ")),
                );
            }
        }
        Ok(())
    }

    pub fn is_enabled(&self, tool_id: &str) -> bool {
        self.enabled.get(tool_id).copied().unwrap_or(false)
    }

    fn hotkey_for(&self, tool_id: &str) -> String {
        self.hotkeys
            .get(tool_id)
            .cloned()
            .or_else(|| {
                TOOLS
                    .iter()
                    .find(|tool| tool.id == tool_id)
                    .and_then(|tool| tool.default_hotkey.map(str::to_owned))
            })
            .or_else(|| {
                APP_HOTKEYS
                    .iter()
                    .find(|hotkey| hotkey.id == tool_id)
                    .map(|hotkey| hotkey.default_hotkey.to_owned())
            })
            .unwrap_or_default()
    }

    fn supports_hotkey(&self, tool_id: &str) -> bool {
        TOOLS
            .iter()
            .find(|tool| tool.id == tool_id)
            .is_some_and(|tool| tool.default_hotkey.is_some())
    }

    /// 触发类型：缺省（旧数据 / 未显式切换）一律按全局热键处理。
    fn hotkey_kind(&self, hotkey_id: &str) -> &'static str {
        match self
            .settings
            .hotkey_kinds
            .get(hotkey_id)
            .map(String::as_str)
        {
            Some(HOTKEY_KIND_LISTEN) => HOTKEY_KIND_LISTEN,
            _ => HOTKEY_KIND_GLOBAL,
        }
    }

    fn hotkey_should_register(&self, hotkey_id: &str) -> bool {
        if let Some(owner) = crate::shortcut_tip::hotkey_owner_tool(hotkey_id) {
            return self.is_enabled(owner);
        }
        APP_HOTKEYS.iter().any(|hotkey| hotkey.id == hotkey_id)
            || (self.is_enabled(hotkey_id) && self.supports_hotkey(hotkey_id))
    }

    fn unregister_hotkey_if_registered(&self, app: &AppHandle, hotkey_id: &str) {
        let kind = self.hotkey_kind(hotkey_id);
        let value = self.hotkey_for(hotkey_id);
        if value.is_empty() {
            return;
        }
        unregister_hotkey(app, hotkey_id, kind, &value);
    }

    /// 非录制阶段该快捷键是否要显示红色状态：配置冲突优先于“注册失败”。
    fn hotkey_error(
        &self,
        hotkey_id: &str,
        conflicts: &BTreeMap<String, String>,
    ) -> Option<String> {
        // 没有键值的行不可能是“被占用/冲突”，避免任何陈旧错误标记残留。
        if self.hotkey_for(hotkey_id).is_empty() {
            return None;
        }
        if let Some(reason) = conflicts.get(hotkey_id) {
            return Some(reason.clone());
        }
        self.hotkey_errors.get(hotkey_id).cloned()
    }

    /// 找出“同一个物理键被多个功能配置”的情况；正常保存不会产生，旧数据/手改配置可能有。
    fn hotkey_conflicts(&self) -> BTreeMap<String, String> {
        let mut groups: BTreeMap<String, Vec<&'static str>> = BTreeMap::new();
        for tool in TOOLS.iter() {
            let value = self.hotkey_for(tool.id);
            if let Some(identity) = trigger_identity(self.hotkey_kind(tool.id), &value) {
                groups.entry(identity).or_default().push(tool.id);
            }
        }
        for hotkey in APP_HOTKEYS.iter() {
            let value = self.hotkey_for(hotkey.id);
            if let Some(identity) = trigger_identity(self.hotkey_kind(hotkey.id), &value) {
                groups.entry(identity).or_default().push(hotkey.id);
            }
        }
        let mut conflicts = BTreeMap::new();
        for ids in groups.values() {
            if ids.len() < 2 {
                continue;
            }
            for id in ids {
                let other = ids
                    .iter()
                    .find(|candidate| *candidate != id)
                    .copied()
                    .unwrap_or(*id);
                conflicts.insert(
                    (*id).to_owned(),
                    format!(
                        "与「{}」使用了同一个按键，只能生效一个",
                        hotkey_display_name(other)
                    ),
                );
            }
        }
        conflicts
    }

    /// 注册并记录结果：成功清除该快捷键的错误标记，失败记录友好原因。
    fn try_register_hotkey(
        &mut self,
        app: &AppHandle,
        hotkey_id: &str,
        kind: &str,
        value: &str,
    ) -> Result<(), String> {
        match register_hotkey(app, hotkey_id, kind, value) {
            Ok(()) => {
                self.hotkey_errors.remove(hotkey_id);
                Ok(())
            }
            Err(error) => {
                self.hotkey_errors
                    .insert(hotkey_id.to_owned(), error.clone());
                Err(error)
            }
        }
    }

    fn start(&mut self, app: &AppHandle, tool: &ToolDefinition) -> Result<(), String> {
        if self.workers.contains_key(tool.id) {
            log_tool_lifecycle(app, tool.id, "tool.lifecycle.start_skipped already_running=true");
            return Ok(());
        }
        log_tool_lifecycle(app, tool.id, "tool.lifecycle.start_requested");
        if !self.shortcuts_suspended {
            // 归属该工具的软件级快捷键随工具启用一起注册（与停用时的注销成对）。
            let mut failures: Vec<String> = Vec::new();
            for hotkey_id in self.app_hotkeys_owned_by(tool.id) {
                let kind = self.hotkey_kind(hotkey_id);
                let value = self.hotkey_for(hotkey_id);
                if value.is_empty() {
                    continue;
                }
                if let Err(error) = self.try_register_hotkey(app, hotkey_id, kind, &value) {
                    failures.push(format!("{hotkey_id}: {error}"));
                }
            }
            if !failures.is_empty() {
                log_tool_lifecycle(
                    app,
                    tool.id,
                    format!("tool.hotkey.register_failed {}", failures.join("; ")),
                );
            }
        }
        if !self.shortcuts_suspended && tool.default_hotkey.is_some() {
            let kind = self.hotkey_kind(tool.id);
            let value = self.hotkey_for(tool.id);
            if !value.is_empty() {
                // 快捷键注册失败只提示、不阻止工具启用：键被其它程序占用不应该让整块工具打不开。
                if let Err(error) = self.try_register_hotkey(app, tool.id, kind, &value) {
                    let display = format_hotkey_display(&value);
                    log_tool_lifecycle(
                        app,
                        tool.id,
                        format!("tool.hotkey.register_failed key={display} error={error}"),
                    );
                    // Toast 只说明“存在冲突/占用”，不展开具体键名与原因（详情留日志与行内悬停）。
                    let _ = app.emit(
                        "tool-hotkey-warning",
                        format!("「{}」存在快捷键冲突或被其它程序占用", tool.name),
                    );
                }
            }
        }
        if tool.id == "clipboard" {
            clipboard::start()?;
        }
        if tool.id == "app_usage" {
            app_usage::start()?;
        }
        if tool.id == "key_usage" {
            key_usage::start()?;
        }
        if tool.id == "timer" {
            timer::start()?;
        }
        if tool.id == "shortcut_tip" {
            shortcut_tip::start()?;
        }
        let (stop, receiver) = mpsc::channel();
        let name = tool.id.to_owned();
        let thread = thread::Builder::new()
            .name(format!("tool-{name}-lifecycle"))
            .spawn(move || {
                let _ = receiver.recv();
            })
            .map_err(|error| format!("启动后台 worker 失败: {error}"))?;
        self.workers
            .insert(tool.id.to_owned(), RunningWorker { stop, thread });
        log_tool_lifecycle(app, tool.id, "tool.lifecycle.started");
        Ok(())
    }

    fn stop(&mut self, app: &AppHandle, tool: &ToolDefinition) -> Result<(), String> {
        log_tool_lifecycle(app, tool.id, "tool.lifecycle.stop_requested");
        if !self.shortcuts_suspended && tool.default_hotkey.is_some() {
            self.unregister_hotkey_if_registered(app, tool.id);
        }
        if !self.shortcuts_suspended {
            // 工具生命周期结束：归属它的软件级快捷键一并注销，别继续占用系统按键。
            for hotkey_id in self.app_hotkeys_owned_by(tool.id) {
                self.unregister_hotkey_if_registered(app, hotkey_id);
            }
        }
        // 停用后这些快捷键不再注册，旧的“不可用”标记随之清除（配置冲突仍由快照实时计算）。
        self.hotkey_errors.remove(tool.id);
        for hotkey_id in self.app_hotkeys_owned_by(tool.id) {
            self.hotkey_errors.remove(hotkey_id);
        }
        window_service::close_tool_window(app, tool.id);
        if tool.id == "clipboard" {
            clipboard::stop();
        }
        if tool.id == "app_usage" {
            app_usage::stop();
        }
        if tool.id == "key_usage" {
            key_usage::stop();
        }
        if tool.id == "timer" {
            let paused_count = timer::stop();
            log_tool_lifecycle(
                app,
                tool.id,
                format!("timer.lifecycle.stop_paused_running count={paused_count}"),
            );
        }
        if tool.id == "window_pinner" {
            window_pinner::unpin_all();
        }
        if tool.id == "shortcut_tip" {
            shortcut_tip::stop();
            // 快捷键提示的浮窗已由原生实现接管，停用工具时一并收起。
            crate::native_tip::stop();
        }
        if tool.id == "screenshot" {
            screenshot::stop();
        }
        if let Some(worker) = self.workers.remove(tool.id) {
            let _ = worker.stop.send(());
            let _ = worker.thread.join();
        }
        log_tool_lifecycle(app, tool.id, "tool.lifecycle.stopped");
        Ok(())
    }
}

/// 按触发类型注册快捷键：全局热键走 `RegisterHotKey`（宿主独占），监听型走低级键鼠钩子。
fn register_hotkey(
    app: &AppHandle,
    hotkey_id: &str,
    kind: &str,
    hotkey: &str,
) -> Result<(), String> {
    if hotkey.is_empty() {
        return Ok(());
    }
    if kind == HOTKEY_KIND_LISTEN {
        hotkey_listener::register(app, hotkey_id, hotkey).map_err(|error| {
            if let Some(state) = app.try_state::<AppState>() {
                push_debug_log(
                    &state,
                    "hotkey",
                    format!("hotkey.listen.register_failed id={hotkey_id} key={hotkey} error={error}"),
                );
            }
            error
        })?;
        if let Some(state) = app.try_state::<AppState>() {
            push_debug_log(
                &state,
                "hotkey",
                format!("hotkey.listen.registered id={hotkey_id} key={hotkey}"),
            );
        }
        return Ok(());
    }
    let shortcut = parse_shortcut(hotkey)?;
    app.global_shortcut()
        .register(shortcut)
        .map_err(friendly_register_error)
}

/// 按触发类型注销快捷键；不存在的按键静默忽略（停用工具时可能本来就没注册）。
fn unregister_hotkey(app: &AppHandle, hotkey_id: &str, kind: &str, hotkey: &str) {
    if hotkey.is_empty() {
        return;
    }
    if kind == HOTKEY_KIND_LISTEN {
        hotkey_listener::unregister(hotkey_id);
        return;
    }
    let Ok(shortcut) = parse_shortcut(hotkey) else {
        return;
    };
    let _ = app.global_shortcut().unregister(shortcut);
}

fn parse_shortcut(value: &str) -> Result<Shortcut, String> {
    value
        .parse()
        .map_err(|error| format!("解析快捷键失败: {error}"))
}

/// 注册失败统一转成用户能看懂的中文：键被占用时不把系统原文抛给用户。
fn friendly_register_error(error: impl std::fmt::Display) -> String {
    let text = error.to_string();
    let lower = text.to_ascii_lowercase();
    if lower.contains("already registered") || text.contains("1409") {
        "该键已被其它程序占用，请换一个键".to_owned()
    } else {
        format!("快捷键注册失败: {text}")
    }
}

/// 一次触发的规范化标识：全局热键用 `normalize_hotkey` 的写法，监听型用单键标识。
/// 两者共用同一套按键名，所以「全局 F12」与「监听 F12」能判成同一个键。
fn trigger_identity(kind: &str, hotkey: &str) -> Option<String> {
    if hotkey.trim().is_empty() {
        return None;
    }
    if kind == HOTKEY_KIND_LISTEN {
        return hotkey_listener::listen_key_token(hotkey);
    }
    canonical_hotkey(hotkey)
}

/// 按触发类型校验并规范化键值：监听型只接受单个按键 / 鼠标侧键。
fn normalize_hotkey_for_kind(kind: &str, value: &str) -> Result<String, String> {
    if kind == HOTKEY_KIND_LISTEN {
        return hotkey_listener::listen_key_token(value)
            .ok_or_else(|| "键鼠监听只支持单个按键或鼠标侧键".to_owned());
    }
    normalize_hotkey(value)
}

fn log_tool_lifecycle(app: &AppHandle, tool_id: &str, message: impl Into<String>) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let level = match tool_id {
        "app_usage" => "app_usage",
        "clipboard" => "clipboard",
        "key_usage" => "key_usage",
        "timer" => "timer",
        "window_pinner" => "window_pinner",
        "shortcut_tip" => "shortcut_tip",
        "screenshot" => "screenshot",
        _ => "settings",
    };
    push_debug_log(&state, level, message);
}

/// 开发版首次启动（数据目录全新）时的默认值：便于测试各类工具。
/// 仅由 `cfg!(debug_assertions)` 分支调用，不影响正式版。
pub fn apply_development_defaults(settings: &mut AppSettings) {
    settings.auto_start = false;
    settings.silent_start = false;
    settings.main_window_always_on_top = true;
    settings.developer_mode = true;
    settings.experimental_enabled = true;
    for tool in TOOLS.iter() {
        settings.tools.insert(tool.id.to_owned(), false);
    }
}

/// 快捷键 id → 展示名：配置冲突提示里要能说清“和谁撞了”。
fn hotkey_display_name(hotkey_id: &str) -> &str {
    TOOLS
        .iter()
        .find(|tool| tool.id == hotkey_id)
        .map(|tool| tool.name)
        .or_else(|| {
            APP_HOTKEYS
                .iter()
                .find(|hotkey| hotkey.id == hotkey_id)
                .map(|hotkey| hotkey.name)
        })
        .unwrap_or(hotkey_id)
}

fn normalize_hotkey(value: &str) -> Result<String, String> {
    let parts: Vec<String> = value
        .split('+')
        .map(|part| normalize_hotkey_part(part.trim()))
        .filter(|part| !part.is_empty())
        .collect();
    // 全局热键允许「单个功能键」：RegisterHotKey 可以只注册 F1–F24（其它单键仍要求修饰键）。
    if parts.len() < 2 {
        if let Some(single) = parts.first() {
            if is_function_key_token(single) {
                return Ok(single.clone());
            }
        }
        return Err("快捷键必须包含修饰键和一个按键".to_owned());
    }

    let key_count = parts
        .iter()
        .filter(|part| {
            !matches!(
                part.as_str(),
                "CTRL" | "CONTROL" | "ALT" | "SHIFT" | "META" | "SUPER" | "CMD" | "COMMAND" | "WIN"
            )
        })
        .count();
    if key_count == 0 {
        return Err("快捷键缺少主按键".to_owned());
    }
    if key_count > 1 {
        return Err("快捷键只能包含一个普通按键".to_owned());
    }
    if !matches!(
        parts.first().map(String::as_str),
        Some("CTRL" | "CONTROL" | "ALT" | "SHIFT" | "META" | "SUPER" | "CMD" | "COMMAND" | "WIN")
    ) {
        return Err("快捷键必须以修饰键开头".to_owned());
    }
    if matches!(
        parts.last().map(String::as_str),
        Some("CTRL" | "CONTROL" | "ALT" | "SHIFT" | "META" | "SUPER" | "CMD" | "COMMAND" | "WIN")
    ) {
        return Err("快捷键必须以普通按键结尾".to_owned());
    }

    let mut normalized = Vec::new();
    if parts
        .iter()
        .any(|part| matches!(part.as_str(), "CTRL" | "CONTROL"))
    {
        normalized.push("CTRL".to_owned());
    }
    if parts.iter().any(|part| part == "ALT") {
        normalized.push("ALT".to_owned());
    }
    if parts.iter().any(|part| part == "SHIFT") {
        normalized.push("SHIFT".to_owned());
    }
    if parts
        .iter()
        .any(|part| matches!(part.as_str(), "META" | "SUPER" | "CMD" | "COMMAND" | "WIN"))
    {
        normalized.push("SUPER".to_owned());
    }
    let key = parts
        .into_iter()
        .find(|part| {
            !matches!(
                part.as_str(),
                "CTRL" | "CONTROL" | "ALT" | "SHIFT" | "META" | "SUPER" | "CMD" | "COMMAND" | "WIN"
            )
        })
        .ok_or_else(|| "快捷键缺少主按键".to_owned())?;
    normalized.push(key);
    Ok(normalized.join("+"))
}

/// 把「存储写法」和「快捷键回调文本」统一成同一个按键标识，只用于匹配与判重。
///
/// 两边的按键名来源不同：存储值来自前端 `KeyboardEvent.key`（符号保持原样，如 `/`、`.`），
/// 回调文本来自 `Shortcut::to_string()`，用的是 DOM code 名（`Slash`、`Period`）。
/// 不统一就会匹配失败——回调落进兜底分支，表现为「按这个键却呼出主窗口」。
/// 显示仍走 `format_hotkey_display`，存储值不变。
fn canonical_hotkey(value: &str) -> Option<String> {
    let normalized = normalize_hotkey(value).ok()?;
    Some(match normalized.rsplit_once('+') {
        Some((modifiers, key)) => format!("{modifiers}+{}", canonical_key_token(key)),
        // 单个功能键（F1–F24）没有修饰键段。
        None => canonical_key_token(&normalized),
    })
}

/// 是否是单个功能键（`F1`–`F24`），全局热键允许不带修饰键。
fn is_function_key_token(token: &str) -> bool {
    token
        .strip_prefix('F')
        .and_then(|rest| rest.parse::<u32>().ok())
        .is_some_and(|number| (1..=24).contains(&number))
}

/// 把同一个物理按键的各种写法收敛到一个标识（符号 → DOM code 名，短别名 → 全名）。
fn canonical_key_token(part: &str) -> String {
    let upper = part.trim().to_uppercase();
    let token = match upper.as_str() {
        "/" | "SLASH" => "SLASH",
        "." | "PERIOD" => "PERIOD",
        "," | "COMMA" => "COMMA",
        ";" | "SEMICOLON" => "SEMICOLON",
        "'" | "QUOTE" => "QUOTE",
        "[" | "BRACKETLEFT" => "BRACKETLEFT",
        "]" | "BRACKETRIGHT" => "BRACKETRIGHT",
        "=" | "EQUAL" => "EQUAL",
        "-" | "MINUS" => "MINUS",
        "`" | "BACKQUOTE" => "BACKQUOTE",
        "\\" | "BACKSLASH" => "BACKSLASH",
        "ESC" | "ESCAPE" => "ESCAPE",
        "DEL" | "DELETE" => "DELETE",
        "INS" | "INSERT" => "INSERT",
        "RETURN" | "ENTER" => "ENTER",
        "SPACEBAR" | "SPACE" => "SPACE",
        "PGUP" | "PAGEUP" => "PAGEUP",
        "PGDN" | "PAGEDOWN" => "PAGEDOWN",
        "UP" | "ARROWUP" => "ARROWUP",
        "DOWN" | "ARROWDOWN" => "ARROWDOWN",
        "LEFT" | "ARROWLEFT" => "ARROWLEFT",
        "RIGHT" | "ARROWRIGHT" => "ARROWRIGHT",
        other => other,
    };
    token.to_owned()
}

/// 把存储写法（`CTRL+SUPER+/`）转成给用户看的文案（`Win+Ctrl+/`）。
///
/// 修饰键顺序固定 `Win` → `Ctrl` → `Alt` → `Shift` → 主键。只影响显示：存储值、冲突判重、
/// 注册与匹配仍用 `normalize_hotkey` 的大写形式。
pub fn format_hotkey_display(value: &str) -> String {
    let mut win = false;
    let mut ctrl = false;
    let mut alt = false;
    let mut shift = false;
    let mut keys: Vec<String> = Vec::new();
    for part in value.split('+').map(str::trim).filter(|part| !part.is_empty()) {
        match part.to_uppercase().as_str() {
            "WIN" | "WINDOWS" | "META" | "SUPER" | "CMD" | "COMMAND" => win = true,
            "CTRL" | "CONTROL" => ctrl = true,
            "ALT" => alt = true,
            "SHIFT" => shift = true,
            _ => keys.push(format_hotkey_display_part(part)),
        }
    }
    let mut parts: Vec<&str> = Vec::new();
    if win {
        parts.push("Win");
    }
    if ctrl {
        parts.push("Ctrl");
    }
    if alt {
        parts.push("Alt");
    }
    if shift {
        parts.push("Shift");
    }
    let mut text = parts.join("+");
    for key in keys {
        if !text.is_empty() {
            text.push('+');
        }
        text.push_str(&key);
    }
    text
}

fn format_hotkey_display_part(part: &str) -> String {
    let upper = part.to_uppercase();
    match upper.as_str() {
        "SPACE" | "SPACEBAR" => "Space".to_owned(),
        "TAB" => "Tab".to_owned(),
        "ENTER" | "RETURN" => "Enter".to_owned(),
        "ESC" | "ESCAPE" => "Esc".to_owned(),
        "BACKSPACE" => "Backspace".to_owned(),
        "DELETE" | "DEL" => "Delete".to_owned(),
        "INSERT" | "INS" => "Insert".to_owned(),
        "HOME" => "Home".to_owned(),
        "END" => "End".to_owned(),
        "PGUP" | "PAGEUP" => "PageUp".to_owned(),
        "PGDN" | "PAGEDOWN" => "PageDown".to_owned(),
        "UP" | "ARROWUP" => "Up".to_owned(),
        "DOWN" | "ARROWDOWN" => "Down".to_owned(),
        "LEFT" | "ARROWLEFT" => "Left".to_owned(),
        "RIGHT" | "ARROWRIGHT" => "Right".to_owned(),
        "CAPSLOCK" => "CapsLock".to_owned(),
        "NUMLOCK" => "NumLock".to_owned(),
        "SCROLLLOCK" => "ScrollLock".to_owned(),
        "PRINTSCREEN" => "PrintScreen".to_owned(),
        "PAUSE" => "Pause".to_owned(),
        // 键鼠监听型快捷键的单键标识：符号键与鼠标侧键要显示成用户认识的写法。
        "SLASH" | "/" => "/".to_owned(),
        "PERIOD" | "." => ".".to_owned(),
        "COMMA" | "," => ",".to_owned(),
        "SEMICOLON" | ";" => ";".to_owned(),
        "QUOTE" | "'" => "'".to_owned(),
        "BRACKETLEFT" | "[" => "[".to_owned(),
        "BRACKETRIGHT" | "]" => "]".to_owned(),
        "EQUAL" | "=" => "=".to_owned(),
        "MINUS" | "-" => "-".to_owned(),
        "BACKQUOTE" | "`" => "`".to_owned(),
        "BACKSLASH" | "\\" => "\\".to_owned(),
        "MOUSE4" => "鼠标侧键1".to_owned(),
        "MOUSE5" => "鼠标侧键2".to_owned(),
        _ => upper,
    }
}

fn normalize_hotkey_part(part: &str) -> String {
    let upper = part.trim().to_uppercase();
    if upper.len() == 4 && upper.starts_with("KEY") {
        return upper[3..].to_owned();
    }
    if upper.len() == 6 && upper.starts_with("DIGIT") {
        return upper[5..].to_owned();
    }
    upper
}

#[cfg(test)]
mod tests {
    use super::{canonical_hotkey, format_hotkey_display, normalize_hotkey, ToolRegistry};
    use tauri_plugin_global_shortcut::Shortcut;

    /// 工具停用必须把它名下的软件级快捷键一起注销：归属表是这条规则的唯一依据，
    /// 这里锁住截图组 5 条的归属，避免以后新增快捷键漏登记（漏登记就会在工具停用后继续占键）。
    #[test]
    fn app_hotkeys_owner_table_covers_screenshot_group() {
        let owner = |id: &str| crate::shortcut_tip::hotkey_owner_tool(id);
        assert_eq!(owner("main_window"), None);
        assert_eq!(owner("clipboard"), Some("clipboard"));
        assert_eq!(owner("window_pinner"), Some("window_pinner"));
        for id in [
            "screenshot",
            "screenshot_pin",
            "pin_restore",
            "pin_close_all",
            "pin_toggle_visible",
            "pin_passthrough",
        ] {
            assert_eq!(owner(id), Some("screenshot"), "{id} 必须归属 screenshot");
        }
    }

    #[test]
    fn formats_hotkey_for_display_with_titlecase_modifiers() {
        assert_eq!(format_hotkey_display("CTRL+ALT+F9"), "Ctrl+Alt+F9");
        assert_eq!(format_hotkey_display("CTRL+SUPER+/"), "Win+Ctrl+/");
        assert_eq!(format_hotkey_display("SUPER+CTRL+ALT+M"), "Win+Ctrl+Alt+M");
        assert_eq!(format_hotkey_display("WIN+SHIFT+S"), "Win+Shift+S");
        assert_eq!(format_hotkey_display("CTRL+SPACE"), "Ctrl+Space");
        assert_eq!(format_hotkey_display("Ctrl+Alt+T"), "Ctrl+Alt+T");
        assert_eq!(format_hotkey_display(""), "");
    }

    #[test]
    fn normalizes_tauri_shortcut_callback_text() {
        assert_eq!(normalize_hotkey("control+alt+KeyZ").unwrap(), "CTRL+ALT+Z");
        assert_eq!(normalize_hotkey("control+alt+KeyV").unwrap(), "CTRL+ALT+V");
        assert_eq!(
            normalize_hotkey("control+shift+Digit1").unwrap(),
            "CTRL+SHIFT+1"
        );
    }

    /// 符号键的存储写法（`KeyboardEvent.key`）与回调文本（DOM code 名）不同，
    /// 匹配前必须收敛到同一个标识，否则会落进「呼出主窗口」的兜底分支。
    #[test]
    fn canonical_hotkey_matches_symbol_keys_with_code_names() {
        assert_eq!(
            canonical_hotkey("CTRL+SUPER+/").as_deref(),
            Some("CTRL+SUPER+SLASH")
        );
        assert_eq!(
            canonical_hotkey("control+super+Slash").as_deref(),
            Some("CTRL+SUPER+SLASH")
        );
        assert_eq!(
            canonical_hotkey("CTRL+ALT+.").as_deref(),
            canonical_hotkey("control+alt+Period").as_deref()
        );
        assert_eq!(
            canonical_hotkey("CTRL+ALT+,").as_deref(),
            canonical_hotkey("control+alt+Comma").as_deref()
        );
        assert_eq!(
            canonical_hotkey("CTRL+[").as_deref(),
            canonical_hotkey("control+bracketLeft").as_deref()
        );
        assert_eq!(
            canonical_hotkey("CTRL+ESC").as_deref(),
            canonical_hotkey("control+Escape").as_deref()
        );
        assert_ne!(canonical_hotkey("CTRL+/"), canonical_hotkey("CTRL+."));
        assert_ne!(
            canonical_hotkey("CTRL+ALT+M"),
            canonical_hotkey("CTRL+SUPER+M")
        );
    }

    /// 记录根因：同一个快捷键注册成功，回调拿到的文本却是 `Slash` 而不是 `/`。
    #[test]
    fn shortcut_callback_text_uses_code_names_for_symbol_keys() {
        let slash: Shortcut = "CTRL+SUPER+/".parse().expect("符号键应可解析");
        assert_eq!(slash.to_string(), "control+super+Slash");
        let period: Shortcut = "CTRL+ALT+.".parse().expect("符号键应可解析");
        assert_eq!(period.to_string(), "control+alt+Period");
        // 单个功能键注册后的回调文本必须还能匹配回存储值（全局 F12 派发链路）。
        let function_key: Shortcut = "F12".parse().expect("单个功能键应可解析");
        assert_eq!(function_key.to_string(), "F12");
    }

    /// 全局热键放开单个功能键（F1–F24），但普通单键仍必须带修饰键。
    #[test]
    fn global_hotkey_accepts_single_function_keys_only() {
        assert_eq!(normalize_hotkey("f12").unwrap(), "F12");
        assert_eq!(canonical_hotkey("F12").as_deref(), Some("F12"));
        assert_eq!(canonical_hotkey("control+alt+F12").as_deref(), Some("CTRL+ALT+F12"));
        assert!(normalize_hotkey("A").is_err());
        assert!(normalize_hotkey("F25").is_err());
    }

    /// 监听型快捷键的键值要显示成用户认识的写法（符号与鼠标侧键）。
    #[test]
    fn formats_listen_hotkeys_for_display() {
        assert_eq!(format_hotkey_display("MOUSE4"), "鼠标侧键1");
        assert_eq!(format_hotkey_display("MOUSE5"), "鼠标侧键2");
        assert_eq!(format_hotkey_display("SLASH"), "/");
        assert_eq!(format_hotkey_display("ARROWUP"), "Up");
        assert_eq!(format_hotkey_display("F12"), "F12");
    }

    /// 旧数据没有 `hotkeyKinds`：缺省按全局热键处理，显式切换过的保持监听。
    #[test]
    fn hotkey_kind_defaults_to_global_and_keeps_listen() {
        use crate::settings::AppSettings;
        let mut settings = AppSettings::default();
        settings
            .hotkey_kinds
            .insert("clipboard".to_owned(), "listen".to_owned());
        let registry = ToolRegistry::new(settings);
        let kind_of = |id: &str| {
            registry
                .snapshot()
                .into_iter()
                .find(|tool| tool.id == id)
                .map(|tool| tool.hotkey_kind)
                .unwrap()
        };
        assert_eq!(kind_of("clipboard"), "listen");
        assert_eq!(kind_of("screenshot"), "global");
    }

    /// 监听型快捷键不参与全局热键的文本派发（否则单键会被误当成组合键匹配）。
    #[test]
    fn listen_hotkeys_do_not_match_global_dispatch() {
        use crate::settings::AppSettings;
        let mut settings = AppSettings::default();
        settings.tools.insert("clipboard".to_owned(), true);
        settings.hotkeys.insert("clipboard".to_owned(), "F12".to_owned());
        settings
            .hotkey_kinds
            .insert("clipboard".to_owned(), "listen".to_owned());
        let registry = ToolRegistry::new(settings);
        assert_eq!(registry.tool_for_shortcut("F12"), None);
        assert_eq!(registry.tool_for_shortcut("control+alt+F12"), None);
    }

    /// 跨类型冲突：全局 `F12` 与监听 `F12` 是同一个物理键。
    #[test]
    fn conflicts_are_detected_across_trigger_kinds() {
        use crate::settings::AppSettings;
        let mut settings = AppSettings::default();
        settings
            .hotkeys
            .insert("pin_passthrough".to_owned(), "F12".to_owned());
        let registry = ToolRegistry::new(settings);
        assert!(registry
            .ensure_hotkey_available("clipboard", "listen", "F12")
            .is_err());
        assert!(registry
            .ensure_hotkey_available("clipboard", "listen", "F11")
            .is_ok());
        assert!(registry
            .ensure_hotkey_available("clipboard", "global", "CTRL+ALT+F12")
            .is_ok());
    }

    /// 旧数据/手改配置里两个功能同键：两行都要带红标记（冲突原因指向对方）。
    #[test]
    fn duplicate_configured_hotkeys_are_reported_as_conflicts() {
        use crate::settings::AppSettings;
        let mut settings = AppSettings::default();
        settings
            .hotkeys
            .insert("clipboard".to_owned(), "CTRL+ALT+T".to_owned());
        let registry = ToolRegistry::new(settings);
        let tools = registry.snapshot();
        let clipboard = tools.iter().find(|tool| tool.id == "clipboard").unwrap();
        let pinner = tools.iter().find(|tool| tool.id == "window_pinner").unwrap();
        let reason = clipboard.hotkey_error.as_deref().unwrap_or_default();
        assert!(reason.contains("窗口置顶"), "{reason}");
        assert!(pinner.hotkey_error.is_some());
    }

    /// 跨类型同键在快照里也按冲突处理（全局 F12 ↔ 监听 F12）。
    #[test]
    fn cross_kind_duplicates_are_reported_as_conflicts() {
        use crate::settings::AppSettings;
        let mut settings = AppSettings::default();
        settings
            .hotkeys
            .insert("clipboard".to_owned(), "F12".to_owned());
        settings
            .hotkey_kinds
            .insert("clipboard".to_owned(), "listen".to_owned());
        settings
            .hotkeys
            .insert("pin_passthrough".to_owned(), "F12".to_owned());
        let registry = ToolRegistry::new(settings);
        let app_hotkeys = registry.app_hotkey_snapshots();
        let pin = app_hotkeys
            .iter()
            .find(|hotkey| hotkey.id == "pin_passthrough")
            .unwrap();
        assert!(pin
            .hotkey_error
            .as_deref()
            .unwrap_or_default()
            .contains("剪贴板"));
    }

    /// 注册失败只标对应快捷键，不牵连其它行。
    #[test]
    fn registration_failure_marks_only_that_hotkey() {
        use crate::settings::AppSettings;
        let mut registry = ToolRegistry::new(AppSettings::default());
        registry.hotkey_errors.insert(
            "window_pinner".to_owned(),
            "该键已被其它程序占用，请换一个键".to_owned(),
        );
        let tools = registry.snapshot();
        let pinner = tools.iter().find(|tool| tool.id == "window_pinner").unwrap();
        assert_eq!(
            pinner.hotkey_error.as_deref(),
            Some("该键已被其它程序占用，请换一个键")
        );
        let clipboard = tools.iter().find(|tool| tool.id == "clipboard").unwrap();
        assert!(clipboard.hotkey_error.is_none());
    }
}
