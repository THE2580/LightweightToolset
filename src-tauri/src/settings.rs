use std::{collections::BTreeMap, fs, path::Path};

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ThemeMode {
    System,
    Light,
    Dark,
}

fn default_theme() -> ThemeMode {
    ThemeMode::System
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum CloseBehavior {
    Quit,
    Tray,
}

fn default_close_behavior() -> CloseBehavior {
    CloseBehavior::Tray
}

fn default_window_title() -> String {
    "轻量化工具集".to_owned()
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    #[serde(default)]
    pub tools: BTreeMap<String, bool>,
    #[serde(default)]
    pub hotkeys: BTreeMap<String, String>,
    #[serde(default = "default_theme")]
    pub theme: ThemeMode,
    #[serde(default)]
    pub auto_start: bool,
    #[serde(default)]
    pub main_window_always_on_top: bool,
    #[serde(default)]
    pub auto_check_updates: bool,
    #[serde(default)]
    pub show_update_notification: bool,
    #[serde(default = "default_window_title")]
    pub window_title: String,
    #[serde(default = "default_close_behavior")]
    pub close_behavior: CloseBehavior,
    #[serde(default)]
    pub developer_mode: bool,
    #[serde(default)]
    pub storage_path: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            tools: BTreeMap::new(),
            hotkeys: BTreeMap::new(),
            theme: default_theme(),
            auto_start: false,
            main_window_always_on_top: false,
            auto_check_updates: true,
            show_update_notification: true,
            window_title: default_window_title(),
            close_behavior: default_close_behavior(),
            developer_mode: false,
            storage_path: String::new(),
        }
    }
}

impl AppSettings {
    pub fn load(path: &Path) -> Result<Self, String> {
        if !path.exists() {
            return Ok(Self::default());
        }
        let raw = fs::read_to_string(path).map_err(|error| format!("读取设置失败: {error}"))?;
        serde_json::from_str(&raw).map_err(|error| format!("解析设置失败: {error}"))
    }

    pub fn save(path: &Path, settings: &Self) -> Result<(), String> {
        let content = serde_json::to_string_pretty(settings)
            .map_err(|error| format!("序列化设置失败: {error}"))?;
        fs::write(path, content).map_err(|error| format!("保存设置失败: {error}"))
    }
}
