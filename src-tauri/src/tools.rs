use std::{collections::BTreeMap, sync::mpsc, thread};

use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

use crate::{clipboard, settings::AppSettings, window_service};

struct ToolDefinition {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    default_hotkey: &'static str,
    default_enabled: bool,
}

const TOOLS: [ToolDefinition; 1] = [ToolDefinition {
    id: "clipboard",
    name: "剪贴板",
    description: "本地纯文本剪贴板历史与快捷弹窗",
    default_hotkey: "CTRL+ALT+V",
    default_enabled: true,
}];

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
    worker_running: bool,
}

impl ToolRegistry {
    pub fn new(mut settings: AppSettings) -> Self {
        let known_tool_ids: Vec<&str> = TOOLS.iter().map(|tool| tool.id).collect();
        settings
            .tools
            .retain(|tool_id, _| known_tool_ids.contains(&tool_id.as_str()));
        settings
            .hotkeys
            .retain(|tool_id, _| known_tool_ids.contains(&tool_id.as_str()));

        for tool in TOOLS.iter() {
            settings
                .tools
                .entry(tool.id.to_owned())
                .or_insert(tool.default_enabled);
            settings
                .hotkeys
                .entry(tool.id.to_owned())
                .or_insert_with(|| tool.default_hotkey.to_owned());
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
            if self.is_enabled(tool.id) {
                self.start(app, tool)?;
            }
        }
        Ok(())
    }

    pub fn set_enabled(&mut self, app: &AppHandle, tool_id: &str, enabled: bool) -> Result<(), String> {
        let tool = TOOLS.iter().find(|tool| tool.id == tool_id).ok_or("未知工具")?;
        if self.is_enabled(tool_id) == enabled {
            return Ok(());
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
                worker_running: self.workers.contains_key(tool.id),
            })
            .collect()
    }

    pub fn set_hotkey(&mut self, app: &AppHandle, tool_id: &str, hotkey: String) -> Result<(), String> {
        let tool = TOOLS.iter().find(|tool| tool.id == tool_id).ok_or("未知工具")?;
        let normalized = normalize_hotkey(&hotkey)?;
        self.ensure_hotkey_available(tool_id, &normalized)?;
        let previous = self.hotkey_for(tool_id);
        if previous == normalized {
            return Ok(());
        }

        if self.is_enabled(tool_id) && self.shortcuts_suspended {
            let next_shortcut: Shortcut = normalized.parse().map_err(|error| format!("解析新快捷键失败: {error}"))?;
            app.global_shortcut()
                .register(next_shortcut)
                .map_err(|error| format!("注册新快捷键失败: {error}"))?;
            let registered_shortcut: Shortcut = normalized.parse().map_err(|error| format!("解析新快捷键失败: {error}"))?;
            app.global_shortcut()
                .unregister(registered_shortcut)
                .map_err(|error| format!("释放新快捷键检查失败: {error}"))?;
        } else if self.is_enabled(tool_id) {
            let previous_shortcut: Shortcut = previous.parse().map_err(|error| format!("解析原快捷键失败: {error}"))?;
            app.global_shortcut()
                .unregister(previous_shortcut)
                .map_err(|error| format!("注销原快捷键失败: {error}"))?;
            let next_shortcut: Shortcut = normalized.parse().map_err(|error| format!("解析新快捷键失败: {error}"))?;
            if let Err(error) = app.global_shortcut().register(next_shortcut) {
                let restore_shortcut: Shortcut = previous.parse().map_err(|parse_error| format!("恢复原快捷键失败: {parse_error}"))?;
                let _ = app.global_shortcut().register(restore_shortcut);
                return Err(format!("注册新快捷键失败: {error}"));
            }
        }

        self.hotkeys.insert(tool.id.to_owned(), normalized.clone());
        self.settings.hotkeys.insert(tool.id.to_owned(), normalized);
        Ok(())
    }

    pub fn tool_for_shortcut(&self, shortcut: &str) -> Option<String> {
        let normalized = normalize_hotkey(shortcut).ok()?;
        TOOLS
            .iter()
            .find(|tool| self.is_enabled(tool.id) && self.hotkey_for(tool.id).eq_ignore_ascii_case(&normalized))
            .map(|tool| tool.id.to_owned())
    }

    pub fn suspend_shortcuts(&mut self, app: &AppHandle) -> Result<(), String> {
        if self.shortcuts_suspended {
            return Ok(());
        }
        for tool in TOOLS.iter() {
            if self.is_enabled(tool.id) {
                let shortcut: Shortcut = self.hotkey_for(tool.id).parse().map_err(|error| format!("解析快捷键失败: {error}"))?;
                app.global_shortcut()
                    .unregister(shortcut)
                    .map_err(|error| format!("暂停快捷键失败: {error}"))?;
            }
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
            if self.is_enabled(tool.id) {
                let shortcut: Shortcut = self.hotkey_for(tool.id).parse().map_err(|error| format!("解析快捷键失败: {error}"))?;
                if let Err(error) = app.global_shortcut().register(shortcut) {
                    for registered_shortcut in registered {
                        let _ = app.global_shortcut().unregister(registered_shortcut);
                    }
                    return Err(format!("恢复快捷键失败: {error}"));
                }
                registered.push(shortcut);
            }
        }
        self.shortcuts_suspended = false;
        Ok(())
    }

    fn is_enabled(&self, tool_id: &str) -> bool {
        self.enabled.get(tool_id).copied().unwrap_or(false)
    }

    fn hotkey_for(&self, tool_id: &str) -> String {
        self.hotkeys
            .get(tool_id)
            .cloned()
            .or_else(|| TOOLS.iter().find(|tool| tool.id == tool_id).map(|tool| tool.default_hotkey.to_owned()))
            .unwrap_or_default()
    }

    fn ensure_hotkey_available(&self, tool_id: &str, hotkey: &str) -> Result<(), String> {
        if TOOLS
            .iter()
            .any(|tool| tool.id != tool_id && self.hotkey_for(tool.id).eq_ignore_ascii_case(hotkey))
        {
            return Err("快捷键已被其他工具占用".to_owned());
        }
        Ok(())
    }

    fn start(&mut self, app: &AppHandle, tool: &ToolDefinition) -> Result<(), String> {
        if self.workers.contains_key(tool.id) {
            return Ok(());
        }
        if !self.shortcuts_suspended {
            let shortcut: Shortcut = self.hotkey_for(tool.id).parse().map_err(|error| format!("解析快捷键失败: {error}"))?;
            app.global_shortcut()
                .register(shortcut)
                .map_err(|error| format!("注册快捷键失败: {error}"))?;
        }
        if tool.id == "clipboard" {
            clipboard::start()?;
        }
        let (stop, receiver) = mpsc::channel();
        let name = tool.id.to_owned();
        let thread = thread::Builder::new()
            .name(format!("tool-{name}-lifecycle"))
            .spawn(move || {
                let _ = receiver.recv();
            })
            .map_err(|error| format!("启动后台 worker 失败: {error}"))?;
        self.workers.insert(tool.id.to_owned(), RunningWorker { stop, thread });
        Ok(())
    }

    fn stop(&mut self, app: &AppHandle, tool: &ToolDefinition) -> Result<(), String> {
        if !self.shortcuts_suspended {
            let shortcut: Shortcut = self.hotkey_for(tool.id).parse().map_err(|error| format!("解析快捷键失败: {error}"))?;
            app.global_shortcut()
                .unregister(shortcut)
                .map_err(|error| format!("注销快捷键失败: {error}"))?;
        }
        window_service::close_tool_window(app, tool.id);
        if tool.id == "clipboard" {
            clipboard::stop();
        }
        if let Some(worker) = self.workers.remove(tool.id) {
            let _ = worker.stop.send(());
            let _ = worker.thread.join();
        }
        Ok(())
    }
}

fn normalize_hotkey(value: &str) -> Result<String, String> {
    let parts: Vec<String> = value
        .split('+')
        .map(|part| part.trim().to_uppercase())
        .filter(|part| !part.is_empty())
        .collect();
    if parts.len() < 2 {
        return Err("快捷键必须包含修饰键和一个按键".to_owned());
    }

    let key_count = parts
        .iter()
        .filter(|part| !matches!(part.as_str(), "CTRL" | "CONTROL" | "ALT" | "SHIFT" | "META" | "SUPER" | "CMD" | "COMMAND" | "WIN"))
        .count();
    if key_count == 0 {
        return Err("快捷键缺少主按键".to_owned());
    }
    if key_count > 1 {
        return Err("快捷键只能包含一个普通按键".to_owned());
    }
    if !matches!(parts.first().map(String::as_str), Some("CTRL" | "CONTROL" | "ALT" | "SHIFT" | "META" | "SUPER" | "CMD" | "COMMAND" | "WIN")) {
        return Err("快捷键必须以修饰键开头".to_owned());
    }
    if matches!(parts.last().map(String::as_str), Some("CTRL" | "CONTROL" | "ALT" | "SHIFT" | "META" | "SUPER" | "CMD" | "COMMAND" | "WIN")) {
        return Err("快捷键必须以普通按键结尾".to_owned());
    }

    let mut normalized = Vec::new();
    if parts.iter().any(|part| matches!(part.as_str(), "CTRL" | "CONTROL")) {
        normalized.push("CTRL".to_owned());
    }
    if parts.iter().any(|part| part == "ALT") {
        normalized.push("ALT".to_owned());
    }
    if parts.iter().any(|part| part == "SHIFT") {
        normalized.push("SHIFT".to_owned());
    }
    if parts.iter().any(|part| matches!(part.as_str(), "META" | "SUPER" | "CMD" | "COMMAND" | "WIN")) {
        normalized.push("SUPER".to_owned());
    }
    let key = parts
        .into_iter()
        .find(|part| !matches!(part.as_str(), "CTRL" | "CONTROL" | "ALT" | "SHIFT" | "META" | "SUPER" | "CMD" | "COMMAND" | "WIN"))
        .ok_or_else(|| "快捷键缺少主按键".to_owned())?;
    normalized.push(key);
    Ok(normalized.join("+"))
}
