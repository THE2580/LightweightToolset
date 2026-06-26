mod settings;
mod tools;
mod window_service;

use std::{path::PathBuf, sync::Mutex, time::Instant};

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, State,
};
use tauri_plugin_global_shortcut::ShortcutState;

use settings::AppSettings;
use tools::{ToolRegistry, ToolSnapshot};

pub struct AppState {
    registry: Mutex<ToolRegistry>,
    settings_path: PathBuf,
    started_at: Instant,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSnapshot {
    tools: Vec<ToolSnapshot>,
    startup_ms: u128,
}

#[tauri::command]
fn get_app_snapshot(state: State<'_, AppState>) -> Result<AppSnapshot, String> {
    let registry = state.registry.lock().map_err(|_| "工具注册表不可用")?;
    Ok(AppSnapshot {
        tools: registry.snapshot(),
        startup_ms: state.started_at.elapsed().as_millis(),
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
        startup_ms: state.started_at.elapsed().as_millis(),
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
        .setup(|app| {
            let _supported_window_kinds = window_service::reserved_window_kinds();
            let config_dir = app.path().app_config_dir()?;
            std::fs::create_dir_all(&config_dir)?;
            let settings_path = config_dir.join("settings.json");
            let settings = AppSettings::load(&settings_path)?;
            let mut registry = ToolRegistry::new(settings);
            registry.start_enabled(app.handle())?;
            app.manage(AppState {
                registry: Mutex::new(registry),
                settings_path,
                started_at: Instant::now(),
            });
            build_tray(app.handle())?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![get_app_snapshot, set_tool_enabled])
        .run(tauri::generate_context!())
        .expect("启动 LightweightToolset 失败");
}
