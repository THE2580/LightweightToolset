use std::{collections::BTreeMap, sync::mpsc, thread};

use serde::Serialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

use crate::{
    app_usage, clipboard, key_usage, push_debug_log, screenshot, settings::AppSettings, shortcut_tip,
    timer, window_pinner, window_service, AppState,
};

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

    pub fn register_app_hotkeys(&self, app: &AppHandle) -> Result<(), String> {
        if self.shortcuts_suspended {
            return Ok(());
        }
        // 单个快捷键被别的软件占用时不能让其余快捷键一起失效，逐条注册并记录失败项。
        let mut failures: Vec<String> = Vec::new();
        for hotkey in APP_HOTKEYS.iter() {
            let shortcut = match self.shortcut_for(hotkey.id) {
                Ok(Some(shortcut)) => shortcut,
                Ok(None) => continue,
                Err(error) => {
                    failures.push(format!("{}: {error}", hotkey.id));
                    continue;
                }
            };
            if let Err(error) = app.global_shortcut().register(shortcut) {
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
        TOOLS
            .iter()
            .map(|tool| ToolSnapshot {
                id: tool.id,
                name: tool.name,
                description: tool.description,
                hotkey: self.hotkey_for(tool.id),
                enabled: self.is_enabled(tool.id),
                implemented: tool.implemented,
                supports_hotkey: tool.default_hotkey.is_some(),
                worker_running: self.workers.contains_key(tool.id),
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
        let normalized = normalize_hotkey(&hotkey)?;
        self.ensure_hotkey_available(hotkey_id, &normalized)?;
        let previous = self.hotkey_for(hotkey_id);
        if previous == normalized {
            return Ok(());
        }

        let should_register = self.hotkey_should_register(hotkey_id);
        if should_register && self.shortcuts_suspended {
            let next_shortcut: Shortcut = normalized
                .parse()
                .map_err(|error| format!("解析新快捷键失败: {error}"))?;
            app.global_shortcut()
                .register(next_shortcut)
                .map_err(|error| format!("注册新快捷键失败: {error}"))?;
            let registered_shortcut: Shortcut = normalized
                .parse()
                .map_err(|error| format!("解析新快捷键失败: {error}"))?;
            app.global_shortcut()
                .unregister(registered_shortcut)
                .map_err(|error| format!("释放新快捷键检查失败: {error}"))?;
        } else if should_register {
            if !previous.is_empty() {
                let previous_shortcut: Shortcut = previous
                    .parse()
                    .map_err(|error| format!("解析原快捷键失败: {error}"))?;
                app.global_shortcut()
                    .unregister(previous_shortcut)
                    .map_err(|error| format!("注销原快捷键失败: {error}"))?;
            }
            let next_shortcut: Shortcut = normalized
                .parse()
                .map_err(|error| format!("解析新快捷键失败: {error}"))?;
            if let Err(error) = app.global_shortcut().register(next_shortcut) {
                if !previous.is_empty() {
                    let restore_shortcut: Shortcut = previous
                        .parse()
                        .map_err(|parse_error| format!("恢复原快捷键失败: {parse_error}"))?;
                    let _ = app.global_shortcut().register(restore_shortcut);
                }
                return Err(format!("注册新快捷键失败: {error}"));
            }
        }

        self.hotkeys
            .insert(hotkey_id.to_owned(), normalized.clone());
        self.settings
            .hotkeys
            .insert(hotkey_id.to_owned(), normalized);
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
        if self.hotkey_registered(hotkey_id) && !self.shortcuts_suspended {
            self.unregister_hotkey_if_registered(app, hotkey_id);
        }
        self.hotkeys.insert(hotkey_id.to_owned(), String::new());
        self.settings
            .hotkeys
            .insert(hotkey_id.to_owned(), String::new());
        Ok(())
    }

    pub fn tool_for_shortcut(&self, shortcut: &str) -> Option<String> {
        let normalized = canonical_hotkey(shortcut)?;
        TOOLS
            .iter()
            .find(|tool| {
                self.is_enabled(tool.id)
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
                canonical_hotkey(&self.hotkey_for(hotkey.id)).as_deref()
                    == Some(normalized.as_str())
            })
            .map(|hotkey| hotkey.id.to_owned())
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
        let mut registered = Vec::new();
        for tool in TOOLS.iter() {
            if self.is_enabled(tool.id) && self.supports_hotkey(tool.id) {
                let Some(shortcut) = self.shortcut_for(tool.id)? else {
                    continue;
                };
                if let Err(error) = app.global_shortcut().register(shortcut) {
                    for registered_shortcut in registered {
                        let _ = app.global_shortcut().unregister(registered_shortcut);
                    }
                    return Err(format!("恢复快捷键失败: {error}"));
                }
                registered.push(shortcut);
            }
        }
        for hotkey in APP_HOTKEYS.iter() {
            let Some(shortcut) = self.shortcut_for(hotkey.id)? else {
                continue;
            };
            if let Err(error) = app.global_shortcut().register(shortcut) {
                eprintln!("[hotkey] app hotkey restore skipped: {error}");
                continue;
            }
            registered.push(shortcut);
        }
        self.shortcuts_suspended = false;
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

    fn ensure_hotkey_available(&self, tool_id: &str, hotkey: &str) -> Result<(), String> {
        let Some(candidate) = canonical_hotkey(hotkey) else {
            return Ok(());
        };
        let conflict = TOOLS
            .iter()
            .any(|tool| {
                tool.id != tool_id
                    && canonical_hotkey(&self.hotkey_for(tool.id)).as_deref()
                        == Some(candidate.as_str())
            })
            || APP_HOTKEYS.iter().any(|app_hotkey| {
                app_hotkey.id != tool_id
                    && canonical_hotkey(&self.hotkey_for(app_hotkey.id)).as_deref()
                        == Some(candidate.as_str())
            });
        if conflict {
            return Err("快捷键已被其他功能占用".to_owned());
        }
        Ok(())
    }

    fn hotkey_registered(&self, hotkey_id: &str) -> bool {
        !self.hotkey_for(hotkey_id).is_empty() && self.hotkey_should_register(hotkey_id)
    }

    fn hotkey_should_register(&self, hotkey_id: &str) -> bool {
        APP_HOTKEYS.iter().any(|hotkey| hotkey.id == hotkey_id)
            || (self.is_enabled(hotkey_id) && self.supports_hotkey(hotkey_id))
    }

    fn shortcut_for(&self, hotkey_id: &str) -> Result<Option<Shortcut>, String> {
        let hotkey = self.hotkey_for(hotkey_id);
        if hotkey.is_empty() {
            return Ok(None);
        }
        hotkey
            .parse()
            .map(Some)
            .map_err(|error| format!("解析快捷键失败: {error}"))
    }

    fn unregister_hotkey_if_registered(&self, app: &AppHandle, hotkey_id: &str) {
        let Ok(shortcut) = self.hotkey_for(hotkey_id).parse::<Shortcut>() else {
            return;
        };
        let _ = app.global_shortcut().unregister(shortcut);
    }

    fn start(&mut self, app: &AppHandle, tool: &ToolDefinition) -> Result<(), String> {
        if self.workers.contains_key(tool.id) {
            log_tool_lifecycle(app, tool.id, "tool.lifecycle.start_skipped already_running=true");
            return Ok(());
        }
        log_tool_lifecycle(app, tool.id, "tool.lifecycle.start_requested");
        if !self.shortcuts_suspended && tool.default_hotkey.is_some() {
            if let Some(shortcut) = self.shortcut_for(tool.id)? {
                app.global_shortcut()
                    .register(shortcut)
                    .map_err(|error| format!("注册快捷键失败: {error}"))?;
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
    for tool in TOOLS.iter() {
        settings.tools.insert(tool.id.to_owned(), false);
    }
}

pub fn app_hotkey_snapshots(settings: &AppSettings) -> Vec<ToolSnapshot> {
    APP_HOTKEYS
        .iter()
        .map(|hotkey| ToolSnapshot {
            id: hotkey.id,
            name: hotkey.name,
            description: hotkey.description,
            hotkey: settings
                .hotkeys
                .get(hotkey.id)
                .cloned()
                .unwrap_or_else(|| hotkey.default_hotkey.to_owned()),
            enabled: true,
            implemented: true,
            supports_hotkey: true,
            worker_running: false,
        })
        .collect()
}

fn normalize_hotkey(value: &str) -> Result<String, String> {
    let parts: Vec<String> = value
        .split('+')
        .map(|part| normalize_hotkey_part(part.trim()))
        .filter(|part| !part.is_empty())
        .collect();
    if parts.len() < 2 {
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
    let (modifiers, key) = normalized.rsplit_once('+')?;
    Some(format!("{modifiers}+{}", canonical_key_token(key)))
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
    use super::{canonical_hotkey, format_hotkey_display, normalize_hotkey};
    use tauri_plugin_global_shortcut::Shortcut;

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
    }
}
