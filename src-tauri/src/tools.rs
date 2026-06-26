use std::{collections::BTreeMap, sync::mpsc, thread};

use serde::Serialize;
use tauri::AppHandle;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, Shortcut};

use crate::{settings::AppSettings, window_service};

struct ToolDefinition {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    hotkey: &'static str,
    default_enabled: bool,
}

const TOOLS: [ToolDefinition; 2] = [
    ToolDefinition {
        id: "lifecycle-probe-a",
        name: "生命周期工具 A",
        description: "验证统一快捷键、后台 worker 与启用状态持久化。",
        hotkey: "CTRL+ALT+SHIFT+1",
        default_enabled: true,
    },
    ToolDefinition {
        id: "lifecycle-probe-b",
        name: "生命周期工具 B",
        description: "验证禁用时释放快捷键、worker 和工具窗口。",
        hotkey: "CTRL+ALT+SHIFT+2",
        default_enabled: true,
    },
];

struct RunningWorker {
    stop: mpsc::Sender<()>,
    thread: thread::JoinHandle<()>,
}

pub struct ToolRegistry {
    enabled: BTreeMap<String, bool>,
    workers: BTreeMap<String, RunningWorker>,
    settings: AppSettings,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolSnapshot {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    hotkey: &'static str,
    enabled: bool,
    worker_running: bool,
}

impl ToolRegistry {
    pub fn new(mut settings: AppSettings) -> Self {
        for tool in TOOLS.iter() {
            settings.tools.entry(tool.id.to_owned()).or_insert(tool.default_enabled);
        }
        Self {
            enabled: settings.tools.clone(),
            workers: BTreeMap::new(),
            settings,
        }
    }

    pub fn settings(&self) -> &AppSettings {
        &self.settings
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
        TOOLS.iter().map(|tool| ToolSnapshot {
            id: tool.id,
            name: tool.name,
            description: tool.description,
            hotkey: tool.hotkey,
            enabled: self.is_enabled(tool.id),
            worker_running: self.workers.contains_key(tool.id),
        }).collect()
    }

    fn is_enabled(&self, tool_id: &str) -> bool {
        self.enabled.get(tool_id).copied().unwrap_or(false)
    }

    fn start(&mut self, app: &AppHandle, tool: &ToolDefinition) -> Result<(), String> {
        if self.workers.contains_key(tool.id) {
            return Ok(());
        }
        let shortcut: Shortcut = tool.hotkey.parse().map_err(|error| format!("解析快捷键失败: {error}"))?;
        app.global_shortcut().register(shortcut).map_err(|error| format!("注册快捷键失败: {error}"))?;
        let (stop, receiver) = mpsc::channel();
        let name = tool.id.to_owned();
        let thread = thread::Builder::new()
            .name(format!("tool-{name}"))
            .spawn(move || {
                let _ = receiver.recv();
            })
            .map_err(|error| format!("启动后台 worker 失败: {error}"))?;
        self.workers.insert(tool.id.to_owned(), RunningWorker { stop, thread });
        Ok(())
    }

    fn stop(&mut self, app: &AppHandle, tool: &ToolDefinition) -> Result<(), String> {
        let shortcut: Shortcut = tool.hotkey.parse().map_err(|error| format!("解析快捷键失败: {error}"))?;
        app.global_shortcut().unregister(shortcut).map_err(|error| format!("注销快捷键失败: {error}"))?;
        window_service::close_tool_window(app, tool.id);
        if let Some(worker) = self.workers.remove(tool.id) {
            let _ = worker.stop.send(());
            let _ = worker.thread.join();
        }
        Ok(())
    }
}
