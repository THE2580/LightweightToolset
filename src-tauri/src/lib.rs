mod clipboard;
mod settings;
mod tools;
mod window_service;

use std::{
    collections::VecDeque,
    fs,
    path::PathBuf,
    process::Command,
    sync::{Mutex, OnceLock},
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, State, WindowEvent,
};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_global_shortcut::ShortcutState;

use clipboard::{
    ClipboardEntry, ClipboardEntryPatch, ClipboardPasteResult, ClipboardQueryInput, ClipboardQueryResult,
    ClipboardSettingsPatch, ClipboardSnapshot,
};
use settings::{AppSettings, CloseBehavior, ThemeMode};
use tools::{ToolRegistry, ToolSnapshot};

const SETTINGS_FILE: &str = "settings.json";
const STORAGE_POINTER_FILE: &str = "storage_path.txt";

pub struct AppState {
    registry: Mutex<ToolRegistry>,
    default_config_dir: PathBuf,
    settings_path: Mutex<PathBuf>,
    process_started_at: Instant,
    cold_start_ms: Mutex<Option<u128>>,
    debug_logs: Mutex<VecDeque<DebugLogEntry>>,
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

#[derive(Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct DebugLogEntry {
    timestamp_ms: u128,
    level: &'static str,
    message: String,
}

#[derive(serde::Deserialize)]
#[serde(rename_all = "camelCase")]
struct SettingsPatch {
    theme: Option<ThemeMode>,
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

fn app_snapshot(state: &AppState, registry: &ToolRegistry) -> Result<AppSnapshot, String> {
    Ok(AppSnapshot {
        tools: registry.snapshot(),
        cold_start_ms: cold_start_ms(state)?,
        settings: registry.settings().clone(),
    })
}

fn save_app_settings(state: &AppState, settings: &AppSettings) -> Result<(), String> {
    let settings_path = state.settings_path.lock().map_err(|_| "设置路径不可用")?;
    AppSettings::save(&settings_path, settings)
}

pub(crate) fn push_debug_log(state: &AppState, level: &'static str, message: impl Into<String>) {
    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    if let Ok(mut logs) = state.debug_logs.lock() {
        logs.push_back(DebugLogEntry {
            timestamp_ms,
            level,
            message: message.into(),
        });
        while logs.len() > 300 {
            logs.pop_front();
        }
    }
}

fn default_storage_path(state: &AppState) -> Result<PathBuf, String> {
    Ok(state.default_config_dir.clone())
}

fn resolve_storage_path(state: &AppState, storage_path: Option<String>) -> Result<PathBuf, String> {
    let value = storage_path.unwrap_or_default();
    let trimmed = value.trim();
    if trimmed.is_empty() {
        default_storage_path(state)
    } else {
        Ok(PathBuf::from(trimmed))
    }
}

fn pointer_path(default_config_dir: &PathBuf) -> PathBuf {
    default_config_dir.join(STORAGE_POINTER_FILE)
}

fn settings_path_for(storage_dir: &PathBuf) -> PathBuf {
    storage_dir.join(SETTINGS_FILE)
}

fn read_storage_pointer(default_config_dir: &PathBuf) -> Option<PathBuf> {
    let raw = fs::read_to_string(pointer_path(default_config_dir)).ok()?;
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        None
    } else {
        Some(PathBuf::from(trimmed))
    }
}

fn write_storage_pointer(default_config_dir: &PathBuf, storage_dir: &PathBuf) -> Result<(), String> {
    let pointer = pointer_path(default_config_dir);
    if storage_dir == default_config_dir {
        if pointer.exists() {
            fs::remove_file(&pointer).map_err(|error| format!("删除存储定位文件失败: {error}"))?;
        }
        return Ok(());
    }
    fs::write(&pointer, storage_dir.display().to_string()).map_err(|error| format!("保存存储定位文件失败: {error}"))
}

fn migrate_storage_files(default_config_dir: &PathBuf, storage_dir: &PathBuf) -> Result<(), String> {
    if storage_dir == default_config_dir {
        return Ok(());
    }
    fs::create_dir_all(storage_dir).map_err(|error| format!("创建存储目录失败: {error}"))?;

    let source_settings = settings_path_for(default_config_dir);
    let target_settings = settings_path_for(storage_dir);
    if source_settings.exists() && !target_settings.exists() {
        fs::copy(&source_settings, &target_settings).map_err(|error| format!("迁移设置文件失败: {error}"))?;
    }

    let source_clipboard = default_config_dir.join("clipboard").join("clipboard.json");
    let target_clipboard = storage_dir.join("clipboard").join("clipboard.json");
    if source_clipboard.exists() && !target_clipboard.exists() {
        if let Some(parent) = target_clipboard.parent() {
            fs::create_dir_all(parent).map_err(|error| format!("创建剪贴板存储目录失败: {error}"))?;
        }
        fs::copy(&source_clipboard, &target_clipboard).map_err(|error| format!("迁移剪贴板数据失败: {error}"))?;
    }
    Ok(())
}

fn initial_storage_dir(default_config_dir: &PathBuf) -> Result<PathBuf, String> {
    if let Some(storage_dir) = read_storage_pointer(default_config_dir) {
        return Ok(storage_dir);
    }
    let settings_path = settings_path_for(default_config_dir);
    let settings = AppSettings::load(&settings_path)?;
    let storage_path = settings.storage_path.trim();
    if storage_path.is_empty() {
        Ok(default_config_dir.clone())
    } else {
        Ok(PathBuf::from(storage_path))
    }
}

fn set_auto_start_plugin(app: &AppHandle, enabled: bool) -> Result<(), String> {
    let autostart = app.autolaunch();
    if enabled {
        autostart.enable().map_err(|error| format!("启用开机自启失败: {error}"))?;
    } else {
        autostart.disable().map_err(|error| format!("关闭开机自启失败: {error}"))?;
    }
    Ok(())
}

#[tauri::command]
fn get_app_snapshot(state: State<'_, AppState>) -> Result<AppSnapshot, String> {
    let registry = state.registry.lock().map_err(|_| "工具注册表不可用")?;
    app_snapshot(&state, &registry)
}

#[tauri::command]
fn get_debug_logs(state: State<'_, AppState>) -> Result<Vec<DebugLogEntry>, String> {
    let logs = state.debug_logs.lock().map_err(|_| "调试日志不可用")?;
    Ok(logs.iter().cloned().collect())
}

#[tauri::command]
fn clear_debug_logs(state: State<'_, AppState>) -> Result<(), String> {
    let mut logs = state.debug_logs.lock().map_err(|_| "调试日志不可用")?;
    logs.clear();
    Ok(())
}

#[tauri::command]
fn push_frontend_debug_log(state: State<'_, AppState>, level: String, message: String) {
    let level = if level == "error" { "error" } else { "info" };
    push_debug_log(&state, level, message);
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
    save_app_settings(&state, registry.settings())?;
    push_debug_log(&state, "info", format!("工具 {tool_id} {}", if enabled { "已启用" } else { "已禁用" }));
    app_snapshot(&state, &registry)
}

#[tauri::command]
fn set_tool_hotkey(
    app: AppHandle,
    state: State<'_, AppState>,
    tool_id: String,
    hotkey: String,
) -> Result<AppSnapshot, String> {
    let mut registry = state.registry.lock().map_err(|_| "工具注册表不可用")?;
    registry.set_hotkey(&app, &tool_id, hotkey.clone())?;
    save_app_settings(&state, registry.settings())?;
    push_debug_log(&state, "info", format!("工具 {tool_id} 快捷键已更新为 {hotkey}"));
    app_snapshot(&state, &registry)
}

#[tauri::command]
fn suspend_tool_hotkeys(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let mut registry = state.registry.lock().map_err(|_| "工具注册表不可用")?;
    registry.suspend_shortcuts(&app)?;
    push_debug_log(&state, "info", "工具快捷键监听期间已暂停");
    Ok(())
}

#[tauri::command]
fn resume_tool_hotkeys(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let mut registry = state.registry.lock().map_err(|_| "工具注册表不可用")?;
    registry.resume_shortcuts(&app)?;
    push_debug_log(&state, "info", "工具快捷键已恢复");
    Ok(())
}

#[tauri::command]
fn clipboard_get_snapshot() -> Result<ClipboardSnapshot, String> {
    clipboard::snapshot()
}

#[tauri::command]
fn clipboard_query(input: ClipboardQueryInput) -> Result<ClipboardQueryResult, String> {
    clipboard::query(input)
}

#[tauri::command]
fn clipboard_update_settings(patch: ClipboardSettingsPatch) -> Result<ClipboardSnapshot, String> {
    clipboard::update_settings(patch)
}

#[tauri::command]
fn clipboard_create_manual(title: String, text: String) -> Result<Option<ClipboardEntry>, String> {
    clipboard::create_manual(title, text)
}

#[tauri::command]
fn clipboard_update_entry(id: String, patch: ClipboardEntryPatch) -> Result<Option<ClipboardEntry>, String> {
    clipboard::update_entry(id, patch)
}

#[tauri::command]
fn clipboard_copy(id: String) -> Result<ClipboardPasteResult, String> {
    clipboard::copy_entry(id)
}

#[tauri::command]
fn clipboard_copy_text(text: String) -> Result<ClipboardPasteResult, String> {
    clipboard::copy_text(text)
}

#[tauri::command]
fn clipboard_delete(ids: Vec<String>) -> Result<(), String> {
    clipboard::delete_entries(ids)
}

#[tauri::command]
fn clipboard_restore(ids: Vec<String>) -> Result<(), String> {
    clipboard::restore_entries(ids)
}

#[tauri::command]
fn clipboard_purge(ids: Vec<String>) -> Result<(), String> {
    clipboard::purge_entries(ids)
}

#[tauri::command]
fn clipboard_clear_history() -> Result<(), String> {
    clipboard::clear_history()
}

#[tauri::command]
fn clipboard_open_panel(app: AppHandle) -> Result<(), String> {
    window_service::open_clipboard_popup(&app)
}

#[tauri::command]
fn clipboard_close_panel(app: AppHandle) {
    window_service::close_clipboard_popup(&app);
}

#[tauri::command]
fn clipboard_open_management(app: AppHandle) {
    window_service::close_clipboard_popup(&app);
    show_main_window(&app);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.emit("navigate-tool", "clipboard");
    }
}

#[tauri::command]
fn set_auto_start_enabled(app: AppHandle, state: State<'_, AppState>, enabled: bool) -> Result<AppSnapshot, String> {
    set_auto_start_plugin(&app, enabled)?;
    let mut registry = state.registry.lock().map_err(|_| "工具注册表不可用")?;
    registry.settings_mut().auto_start = enabled;
    save_app_settings(&state, registry.settings())?;
    push_debug_log(&state, "info", format!("开机自启{}", if enabled { "已开启" } else { "已关闭" }));
    app_snapshot(&state, &registry)
}

#[tauri::command]
fn get_default_storage_path(state: State<'_, AppState>) -> Result<String, String> {
    Ok(default_storage_path(&state)?.display().to_string())
}

#[tauri::command]
fn open_storage_path(state: State<'_, AppState>, storage_path: Option<String>) -> Result<(), String> {
    let path = resolve_storage_path(&state, storage_path)?;
    fs::create_dir_all(&path).map_err(|error| format!("创建存储目录失败: {error}"))?;
    Command::new("explorer")
        .arg(&path)
        .spawn()
        .map_err(|error| format!("打开存储目录失败: {error}"))?;
    push_debug_log(&state, "info", format!("打开存储目录：{}", path.display()));
    Ok(())
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
        let next_storage_dir = resolve_storage_path(&state, Some(storage_path.clone()))?;
        fs::create_dir_all(&next_storage_dir).map_err(|error| format!("创建存储目录失败: {error}"))?;
        settings.storage_path = storage_path;
        let next_settings_path = settings_path_for(&next_storage_dir);
        AppSettings::save(&next_settings_path, settings)?;
        {
            let mut active_settings_path = state.settings_path.lock().map_err(|_| "设置路径不可用")?;
            *active_settings_path = next_settings_path;
        }
        write_storage_pointer(&state.default_config_dir, &next_storage_dir)?;
        clipboard::relocate(&next_storage_dir)?;
    }
    save_app_settings(&state, registry.settings())?;
    push_debug_log(&state, "info", "应用设置已保存");
    app_snapshot(&state, &registry)
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
            "quit" => {
                window_service::close_clipboard_popup(app);
                clipboard::stop();
                app.exit(0);
            }
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
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_autostart::Builder::new().app_name("LightweightToolset").build())
        .plugin(tauri_plugin_global_shortcut::Builder::new().with_handler(|app, shortcut, event| {
            if event.state() == ShortcutState::Pressed {
                let mut handled = false;
                if let Some(state) = app.try_state::<AppState>() {
                    push_debug_log(&state, "info", format!("工具快捷键已触发：{shortcut}"));
                    if let Ok(registry) = state.registry.lock() {
                        if registry.tool_for_shortcut(&shortcut.to_string()).as_deref() == Some("clipboard") {
                            handled = true;
                        }
                    }
                }
                if handled {
                    if let Err(error) = window_service::open_clipboard_popup(app) {
                        if let Some(state) = app.try_state::<AppState>() {
                            push_debug_log(&state, "error", format!("剪贴板快捷窗口启动命令失败：{error}"));
                        }
                    }
                } else {
                    show_main_window(app);
                }
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
            let storage_dir = initial_storage_dir(&config_dir)?;
            migrate_storage_files(&config_dir, &storage_dir)?;
            write_storage_pointer(&config_dir, &storage_dir)?;
            clipboard::init(&storage_dir)?;
            let settings_path = settings_path_for(&storage_dir);
            let settings = AppSettings::load(&settings_path)?;
            let mut registry = ToolRegistry::new(settings);
            if registry.settings().auto_start {
                let _ = set_auto_start_plugin(app.handle(), true);
            }
            registry.start_enabled(app.handle())?;
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_title(&registry.settings().window_title);
            }
            app.manage(AppState {
                registry: Mutex::new(registry),
                default_config_dir: config_dir,
                settings_path: Mutex::new(settings_path),
                process_started_at: *PROCESS_STARTED_AT.get_or_init(Instant::now),
                cold_start_ms: Mutex::new(None),
                debug_logs: Mutex::new(VecDeque::from([DebugLogEntry {
                    timestamp_ms: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|duration| duration.as_millis())
                        .unwrap_or_default(),
                    level: "info",
                    message: "LightweightToolset 开发版日志已启动".to_owned(),
                }])),
            });
            build_tray(app.handle())?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_app_snapshot,
            get_debug_logs,
            clear_debug_logs,
            push_frontend_debug_log,
            set_tool_enabled,
            set_tool_hotkey,
            suspend_tool_hotkeys,
            resume_tool_hotkeys,
            set_auto_start_enabled,
            clipboard_get_snapshot,
            clipboard_query,
            clipboard_update_settings,
            clipboard_create_manual,
            clipboard_update_entry,
            clipboard_copy,
            clipboard_copy_text,
            clipboard_delete,
            clipboard_restore,
            clipboard_purge,
            clipboard_clear_history,
            clipboard_open_panel,
            clipboard_close_panel,
            clipboard_open_management,
            get_default_storage_path,
            open_storage_path,
            update_app_settings
        ])
        .run(tauri::generate_context!())
        .expect("启动 LightweightToolset 失败");
}
