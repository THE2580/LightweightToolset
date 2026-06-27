mod settings;
mod tools;
mod window_service;

use std::{path::PathBuf, sync::{Mutex, OnceLock}, time::Instant};

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, State, WindowEvent,
};
use tauri_plugin_global_shortcut::ShortcutState;

use settings::{AppSettings, CloseBehavior, ThemeMode};
use tools::{ToolRegistry, ToolSnapshot};

pub struct AppState {
    registry: Mutex<ToolRegistry>,
    settings_path: PathBuf,
    process_started_at: Instant,
    cold_start_ms: Mutex<Option<u128>>,
}

static PROCESS_STARTED_AT: OnceLock<Instant> = OnceLock::new();

pub fn mark_process_start() {
    PROCESS_STARTED_AT.get_or_init(Instant::now);
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSnapshot {
    tools: Vec<ToolSnapshot>,
    cold_start_ms: u128,
    settings: AppSettings,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SettingsPatch {
    theme: Option<ThemeMode>,
    auto_start: Option<bool>,
    auto_check_updates: Option<bool>,
    show_update_notification: Option<bool>,
    window_title: Option<String>,
    close_behavior: Option<CloseBehavior>,
    developer_mode: Option<bool>,
    storage_path: Option<String>,
}

fn cold_start_ms(state: &AppState) -> Result<u128, String> {
    let mut metric = state.cold_start_ms.lock().map_err(|_| "性能指标不可用")?;
    Ok(*metric.get_or_insert_with(|| state.process_started_at.elapsed().as_millis()))
}

#[tauri::command]
fn get_app_snapshot(state: State<'_, AppState>) -> Result<AppSnapshot, String> {
    let registry = state.registry.lock().map_err(|_| "工具注册表不可用")?;
    Ok(AppSnapshot {
        tools: registry.snapshot(),
        cold_start_ms: cold_start_ms(&state)?,
        settings: registry.settings().clone(),
    })
}

#[tauri::command]
fn set_tool_enabled(
    app: AppHandle,
    state: State<'_, AppState>,
    tool_id: String,
    enabled: bool,
) -> Result<AppSnapshot, String> {
    let mut registry = state.registry.lock().map_err(|_| "工具注册表不可用")?;
    registry.set_enabled(&app, &tool_id, enabled)?;
    AppSettings::save(&state.settings_path, registry.settings())?;
    Ok(AppSnapshot {
        tools: registry.snapshot(),
        cold_start_ms: cold_start_ms(&state)?,
        settings: registry.settings().clone(),
    })
}

#[tauri::command]
fn update_app_settings(
    app: AppHandle,
    state: State<'_, AppState>,
    patch: SettingsPatch,
) -> Result<AppSnapshot, String> {
    let mut registry = state.registry.lock().map_err(|_| "工具注册表不可用")?;
    let settings = registry.settings_mut();
    if let Some(theme) = patch.theme {
        settings.theme = theme;
    }
    if let Some(auto_start) = patch.auto_start {
        settings.auto_start = auto_start;
    }
    if let Some(auto_check_updates) = patch.auto_check_updates {
        settings.auto_check_updates = auto_check_updates;
    }
    if let Some(show_update_notification) = patch.show_update_notification {
        settings.show_update_notification = show_update_notification;
    }
    if let Some(window_title) = patch.window_title {
        let title = window_title.trim();
        if !title.is_empty() {
            settings.window_title = title.to_owned();
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_title(title);
            }
        }
    }
    if let Some(close_behavior) = patch.close_behavior {
        settings.close_behavior = close_behavior;
    }
    if let Some(developer_mode) = patch.developer_mode {
        settings.developer_mode = developer_mode;
    }
    if let Some(storage_path) = patch.storage_path {
        settings.storage_path = storage_path;
    }
    AppSettings::save(&state.settings_path, registry.settings())?;
    Ok(AppSnapshot {
        tools: registry.snapshot(),
        cold_start_ms: cold_start_ms(&state)?,
        settings: registry.settings().clone(),
    })
}

fn show_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    TrayIconBuilder::with_id("main-tray")
        .icon(app.default_window_icon().expect("缺少应用图标").clone())
        .tooltip("LightweightToolset")
        .menu(&menu)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main_window(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_main_window(tray.app_handle());
            }
        })
        .build(app)?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_global_shortcut::Builder::new().with_handler(|app, _, event| {
            if event.state() == ShortcutState::Pressed {
                show_main_window(app);
            }
        }).build())
        .plugin(tauri_plugin_single_instance::init(|app, _, _| show_main_window(app)))
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    let should_hide = window
                        .try_state::<AppState>()
                        .and_then(|state| state.registry.lock().ok().map(|registry| registry.settings().close_behavior == CloseBehavior::Tray))
                        .unwrap_or(true);
                    if should_hide {
                        api.prevent_close();
                        let _ = window.hide();
                    }
                }
            }
        })
        .setup(|app| {
            let _supported_window_kinds = window_service::reserved_window_kinds();
            let config_dir = app.path().app_config_dir()?;
            std::fs::create_dir_all(&config_dir)?;
            let settings_path = config_dir.join("settings.json");
            let settings = AppSettings::load(&settings_path)?;
            let mut registry = ToolRegistry::new(settings);
            registry.start_enabled(app.handle())?;
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_title(&registry.settings().window_title);
            }
            app.manage(AppState {
                registry: Mutex::new(registry),
                settings_path,
                process_started_at: *PROCESS_STARTED_AT.get_or_init(Instant::now),
                cold_start_ms: Mutex::new(None),
            });
            build_tray(app.handle())?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![get_app_snapshot, set_tool_enabled, update_app_settings])
        .run(tauri::generate_context!())
        .expect("启动 LightweightToolset 失败");
}
