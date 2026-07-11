use std::{collections::BTreeMap, sync::mpsc, thread};

use serde::Serialize;
use tauri::{AppHandle, Manager};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

use crate::{
    app_usage, clipboard, key_usage, push_debug_log, settings::AppSettings, timer, window_pinner, window_service,
    AppState,
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

const APP_HOTKEYS: [AppHotkeyDefinition; 1] = [AppHotkeyDefinition {
    id: "main_window",
    name: "呼出主窗口",
    description: "按下快捷键显示并聚焦主窗口",
    default_hotkey: "CTRL+ALT+M",
}];

const TOOLS: [ToolDefinition; 5] = [
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
        for hotkey in APP_HOTKEYS.iter() {
            let Some(shortcut) = self.shortcut_for(hotkey.id)? else {
                continue;
            };
            app.global_shortcut()
                .register(shortcut)
                .map_err(|error| format!("注册软件快捷键失败: {error}"))?;
        }
        Ok(())
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
        let normalized = normalize_hotkey(shortcut).ok()?;
        TOOLS
            .iter()
            .find(|tool| {
                self.is_enabled(tool.id)
                    && self.hotkey_for(tool.id).eq_ignore_ascii_case(&normalized)
            })
            .map(|tool| tool.id.to_owned())
    }

    pub fn app_hotkey_for_shortcut(&self, shortcut: &str) -> Option<String> {
        let normalized = normalize_hotkey(shortcut).ok()?;
        APP_HOTKEYS
            .iter()
            .find(|hotkey| self.hotkey_for(hotkey.id).eq_ignore_ascii_case(&normalized))
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
        if TOOLS
            .iter()
            .any(|tool| tool.id != tool_id && self.hotkey_for(tool.id).eq_ignore_ascii_case(hotkey))
            || APP_HOTKEYS.iter().any(|app_hotkey| {
                app_hotkey.id != tool_id
                    && self.hotkey_for(app_hotkey.id).eq_ignore_ascii_case(hotkey)
            })
        {
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
        _ => "settings",
    };
    push_debug_log(&state, level, message);
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
    use super::normalize_hotkey;

    #[test]
    fn normalizes_tauri_shortcut_callback_text() {
        assert_eq!(normalize_hotkey("control+alt+KeyZ").unwrap(), "CTRL+ALT+Z");
        assert_eq!(normalize_hotkey("control+alt+KeyV").unwrap(), "CTRL+ALT+V");
        assert_eq!(
            normalize_hotkey("control+shift+Digit1").unwrap(),
            "CTRL+SHIFT+1"
        );
    }
}
