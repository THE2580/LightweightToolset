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
    /// 静默启动：启动后只显示托盘图标，不弹出主窗口。
    #[serde(default)]
    pub silent_start: bool,
    #[serde(default)]
    pub main_window_always_on_top: bool,
    #[serde(default = "default_window_pinner_max_pins")]
    pub window_pinner_max_pins: usize,
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
    /// 覆盖层「保存」按钮默认打开的目录；空串表示用 `screenshot/Screenshots`。
    #[serde(default)]
    pub screenshot_manual_save_dir: String,
    /// 保存快捷键（Ctrl+S）直接写入的目录；空串表示用 `screenshot/Screenshots`。
    #[serde(default)]
    pub screenshot_quick_save_dir: String,
    /// 覆盖层标注调色板（8 个 `#RRGGBB`）。
    #[serde(default = "default_screenshot_palette")]
    pub screenshot_palette: Vec<String>,
    /// 保存文件名模板，支持 `$yyyy$` 等占位符。
    #[serde(default = "default_screenshot_template")]
    pub screenshot_file_template: String,
    /// 选区外压暗层颜色（`#RRGGBB`）。
    #[serde(default = "default_screenshot_dim_color")]
    pub screenshot_dim_color: String,
    /// 压暗层不透明度（0–100）。
    #[serde(default = "default_screenshot_dim_opacity")]
    pub screenshot_dim_opacity: u8,
    /// 放大镜是否显示网格线。
    #[serde(default = "default_screenshot_grid")]
    pub screenshot_magnifier_grid: bool,
    /// 放大镜网格线颜色（`#RRGGBB`）。
    #[serde(default = "default_screenshot_grid_color")]
    pub screenshot_magnifier_grid_color: String,
    /// 辅助线显示模式：`always` / `alt` / `off`。
    #[serde(default = "default_screenshot_guides_mode")]
    pub screenshot_guides_mode: String,
    /// 蓝色辅助线颜色（屏幕中心十字 + 选区系）。
    #[serde(default = "default_screenshot_guides_color")]
    pub screenshot_guides_color: String,
    /// 光标十字颜色（灰色实线）。
    #[serde(default = "default_screenshot_cursor_guide_color")]
    pub screenshot_cursor_guide_color: String,
    /// 蓝色辅助线线宽（1–5）。
    #[serde(default = "default_screenshot_guide_width")]
    pub screenshot_guides_width: u8,
    /// 光标十字线宽（1–5）。
    #[serde(default = "default_screenshot_guide_width")]
    pub screenshot_cursor_guide_width: u8,
}

fn default_screenshot_guide_width() -> u8 {
    1
}

fn default_screenshot_guides_mode() -> String {
    "alt".to_owned()
}

fn default_screenshot_guides_color() -> String {
    "#30B4FF".to_owned()
}

fn default_screenshot_cursor_guide_color() -> String {
    "#808080".to_owned()
}

fn default_screenshot_palette() -> Vec<String> {
    crate::screenshot::default_palette_hex()
}

fn default_screenshot_template() -> String {
    crate::screenshot::default_file_template()
}

fn default_screenshot_dim_color() -> String {
    "#000000".to_owned()
}

fn default_screenshot_dim_opacity() -> u8 {
    crate::screenshot::DEFAULT_DIM_OPACITY
}

fn default_screenshot_grid() -> bool {
    true
}

fn default_screenshot_grid_color() -> String {
    "#666666".to_owned()
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            tools: BTreeMap::new(),
            hotkeys: BTreeMap::new(),
            theme: default_theme(),
            auto_start: false,
            silent_start: false,
            main_window_always_on_top: false,
            window_pinner_max_pins: default_window_pinner_max_pins(),
            auto_check_updates: true,
            show_update_notification: true,
            window_title: default_window_title(),
            close_behavior: default_close_behavior(),
            developer_mode: false,
            storage_path: String::new(),
            screenshot_manual_save_dir: String::new(),
            screenshot_quick_save_dir: String::new(),
            screenshot_palette: default_screenshot_palette(),
            screenshot_file_template: default_screenshot_template(),
            screenshot_dim_color: default_screenshot_dim_color(),
            screenshot_dim_opacity: default_screenshot_dim_opacity(),
            screenshot_magnifier_grid: default_screenshot_grid(),
            screenshot_magnifier_grid_color: default_screenshot_grid_color(),
            screenshot_guides_mode: default_screenshot_guides_mode(),
            screenshot_guides_color: default_screenshot_guides_color(),
            screenshot_cursor_guide_color: default_screenshot_cursor_guide_color(),
            screenshot_guides_width: default_screenshot_guide_width(),
            screenshot_cursor_guide_width: default_screenshot_guide_width(),
        }
    }
}

fn default_window_pinner_max_pins() -> usize { 10 }

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn legacy_settings_without_silent_start_default_to_visible_window() {
        let legacy = r#"{"theme":"system","autoStart":true,"windowTitle":"旧配置"}"#;
        let settings: AppSettings = serde_json::from_str(legacy).expect("旧配置应能解析");
        assert!(!settings.silent_start);
        assert!(settings.auto_start);
    }

    #[test]
    fn silent_start_round_trips_through_json() {
        let settings: AppSettings = serde_json::from_str(r#"{"silentStart":true}"#).expect("解析");
        assert!(settings.silent_start);
        let serialized = serde_json::to_string(&settings).expect("序列化");
        assert!(serialized.contains("\"silentStart\":true"));
    }
}
