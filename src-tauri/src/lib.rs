mod app_usage;
mod clipboard;
mod input_monitor;
mod key_usage;
mod settings;
mod timer;
mod tools;
mod windows_notification;
mod window_service;
mod window_pinner;

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
#[cfg(not(debug_assertions))]
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_global_shortcut::ShortcutState;

use app_usage::{AppUsageProcessPatch, AppUsageSettingsPatch, AppUsageSnapshot};
use clipboard::{
    ClipboardEntry, ClipboardEntryPatch, ClipboardPasteResult, ClipboardQueryInput,
    ClipboardQueryResult, ClipboardSettingsPatch, ClipboardSnapshot,
};
use key_usage::KeyUsageSnapshot;
use settings::{AppSettings, CloseBehavior, ThemeMode};
use timer::{TimerCreateInput, TimerReorderInput, TimerSnapshot, TimerUpdateInput};
use window_pinner::WindowPinnerSnapshot;
use tools::{app_hotkey_snapshots, ToolRegistry, ToolSnapshot};

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
    app_hotkeys: Vec<ToolSnapshot>,
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
    main_window_always_on_top: Option<bool>,
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
        app_hotkeys: app_hotkey_snapshots(registry.settings()),
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

fn write_storage_pointer(
    default_config_dir: &PathBuf,
    storage_dir: &PathBuf,
) -> Result<(), String> {
    let pointer = pointer_path(default_config_dir);
    if storage_dir == default_config_dir {
        if pointer.exists() {
            fs::remove_file(&pointer).map_err(|error| format!("删除存储定位文件失败: {error}"))?;
        }
        return Ok(());
    }
    fs::write(&pointer, storage_dir.display().to_string())
        .map_err(|error| format!("保存存储定位文件失败: {error}"))
}

fn migrate_storage_files(
    default_config_dir: &PathBuf,
    storage_dir: &PathBuf,
) -> Result<(), String> {
    if storage_dir == default_config_dir {
        return Ok(());
    }
    fs::create_dir_all(storage_dir).map_err(|error| format!("创建存储目录失败: {error}"))?;

    let source_settings = settings_path_for(default_config_dir);
    let target_settings = settings_path_for(storage_dir);
    if source_settings.exists() && !target_settings.exists() {
        fs::copy(&source_settings, &target_settings)
            .map_err(|error| format!("迁移设置文件失败: {error}"))?;
    }

    let source_clipboard = default_config_dir.join("clipboard").join("clipboard.json");
    let target_clipboard = storage_dir.join("clipboard").join("clipboard.json");
    if source_clipboard.exists() && !target_clipboard.exists() {
        if let Some(parent) = target_clipboard.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("创建剪贴板存储目录失败: {error}"))?;
        }
        fs::copy(&source_clipboard, &target_clipboard)
            .map_err(|error| format!("迁移剪贴板数据失败: {error}"))?;
    }

    let source_app_usage = default_config_dir.join("app_usage").join("app_usage.json");
    let target_app_usage = storage_dir.join("app_usage").join("app_usage.json");
    if source_app_usage.exists() && !target_app_usage.exists() {
        if let Some(parent) = target_app_usage.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("创建软件统计存储目录失败: {error}"))?;
        }
        fs::copy(&source_app_usage, &target_app_usage)
            .map_err(|error| format!("迁移软件统计数据失败: {error}"))?;
    }

    let source_timer = default_config_dir.join("timer").join("timer.json");
    let target_timer = storage_dir.join("timer").join("timer.json");
    if source_timer.exists() && !target_timer.exists() {
        if let Some(parent) = target_timer.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("创建计时器存储目录失败: {error}"))?;
        }
        fs::copy(&source_timer, &target_timer)
            .map_err(|error| format!("迁移计时器数据失败: {error}"))?;
    }

    let source_key_usage = default_config_dir.join("key_usage").join("key_usage.json");
    let target_key_usage = storage_dir.join("key_usage").join("key_usage.json");
    if source_key_usage.exists() && !target_key_usage.exists() {
        if let Some(parent) = target_key_usage.parent() {
            fs::create_dir_all(parent)
                .map_err(|error| format!("创建按键统计存储目录失败: {error}"))?;
        }
        fs::copy(&source_key_usage, &target_key_usage)
            .map_err(|error| format!("迁移按键统计数据失败: {error}"))?;
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
    #[cfg(debug_assertions)]
    {
        let _ = app;
        let _ = enabled;
        return Ok(());
    }

    #[cfg(not(debug_assertions))]
    {
    let autostart = app.autolaunch();
    if enabled {
        autostart
            .enable()
            .map_err(|error| format!("启用开机自启失败: {error}"))?;
    } else {
        autostart
            .disable()
            .map_err(|error| format!("关闭开机自启失败: {error}"))?;
    }
    Ok(())
    }
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
    let level = normalize_debug_log_level(level);
    push_debug_log(&state, level, message);
}

fn normalize_debug_log_level(level: String) -> &'static str {
    match level.trim().to_ascii_lowercase().as_str() {
        "app" => "app",
        "app_usage" => "app_usage",
        "clipboard" => "clipboard",
        "error" => "error",
        "frontend" => "frontend",
        "hotkey" => "hotkey",
        "key_usage" => "key_usage",
        "settings" => "settings",
        "storage" => "storage",
        "system" => "system",
        "timer" => "timer",
        "update" => "update",
        "window" => "window",
        _ => "frontend",
    }
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
    push_debug_log(
        &state,
        "settings",
        format!("tool.enabled id={tool_id} enabled={enabled}"),
    );
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
    push_debug_log(
        &state,
        "hotkey",
        format!("tool.hotkey.updated id={tool_id} hotkey={hotkey}"),
    );
    app_snapshot(&state, &registry)
}

#[tauri::command]
fn clear_tool_hotkey(
    app: AppHandle,
    state: State<'_, AppState>,
    tool_id: String,
) -> Result<AppSnapshot, String> {
    let mut registry = state.registry.lock().map_err(|_| "工具注册表不可用")?;
    registry.clear_hotkey(&app, &tool_id)?;
    save_app_settings(&state, registry.settings())?;
    push_debug_log(
        &state,
        "hotkey",
        format!("tool.hotkey.cleared id={tool_id}"),
    );
    app_snapshot(&state, &registry)
}

#[tauri::command]
fn suspend_tool_hotkeys(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let mut registry = state.registry.lock().map_err(|_| "工具注册表不可用")?;
    registry.suspend_shortcuts(&app)?;
    push_debug_log(&state, "hotkey", "tool.hotkeys.suspended");
    Ok(())
}

#[tauri::command]
fn resume_tool_hotkeys(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    let mut registry = state.registry.lock().map_err(|_| "工具注册表不可用")?;
    registry.resume_shortcuts(&app)?;
    push_debug_log(&state, "hotkey", "tool.hotkeys.resumed");
    Ok(())
}

fn ensure_tool_enabled(state: &State<'_, AppState>, tool_id: &str) -> Result<(), String> {
    let registry = state.registry.lock().map_err(|_| "工具注册表不可用")?;
    if registry.is_enabled(tool_id) {
        Ok(())
    } else {
        Err("工具已禁用".to_owned())
    }
}

fn ensure_clipboard_enabled(state: &State<'_, AppState>) -> Result<(), String> {
    ensure_tool_enabled(state, "clipboard")
}

fn ensure_app_usage_enabled(state: &State<'_, AppState>) -> Result<(), String> {
    ensure_tool_enabled(state, "app_usage")
}

fn ensure_key_usage_enabled(state: &State<'_, AppState>) -> Result<(), String> {
    ensure_tool_enabled(state, "key_usage")
}

fn ensure_timer_enabled(state: &State<'_, AppState>) -> Result<(), String> {
    ensure_tool_enabled(state, "timer")
}

fn ensure_window_pinner_enabled(state: &State<'_, AppState>) -> Result<(), String> {
    ensure_tool_enabled(state, "window_pinner")
}

#[tauri::command]
fn window_pinner_get_snapshot(state: State<'_, AppState>) -> Result<WindowPinnerSnapshot, String> {
    ensure_window_pinner_enabled(&state)?;
    window_pinner::snapshot()
}

#[tauri::command]
fn window_pinner_unpin(state: State<'_, AppState>, hwnd: isize) -> Result<WindowPinnerSnapshot, String> {
    ensure_window_pinner_enabled(&state)?;
    window_pinner::unpin(hwnd)
}

#[tauri::command]
fn window_pinner_unpin_all(state: State<'_, AppState>) -> Result<WindowPinnerSnapshot, String> {
    ensure_window_pinner_enabled(&state)?;
    Ok(window_pinner::unpin_all())
}

#[tauri::command]
fn window_pinner_set_max_pins(state: State<'_, AppState>, max_pins: usize) -> Result<WindowPinnerSnapshot, String> {
    ensure_window_pinner_enabled(&state)?;
    let snapshot = window_pinner::set_max_pins(max_pins)?;
    let mut registry = state.registry.lock().map_err(|_| "工具状态不可用")?;
    registry.settings_mut().window_pinner_max_pins = snapshot.max_pins;
    save_app_settings(&state, registry.settings())?;
    Ok(snapshot)
}

#[tauri::command]
fn app_usage_get_snapshot(state: State<'_, AppState>) -> Result<AppUsageSnapshot, String> {
    ensure_app_usage_enabled(&state)?;
    push_debug_log(&state, "app_usage", "app_usage.snapshot.requested");
    app_usage::snapshot()
}

#[tauri::command]
fn app_usage_update_settings(
    state: State<'_, AppState>,
    patch: AppUsageSettingsPatch,
) -> Result<AppUsageSnapshot, String> {
    ensure_app_usage_enabled(&state)?;
    push_debug_log(&state, "app_usage", "app_usage.settings.update_requested");
    app_usage::update_settings(patch)
}

#[tauri::command]
fn app_usage_update_process(
    state: State<'_, AppState>,
    patch: AppUsageProcessPatch,
) -> Result<AppUsageSnapshot, String> {
    ensure_app_usage_enabled(&state)?;
    push_debug_log(&state, "app_usage", "app_usage.process.update_requested");
    app_usage::update_process(patch)
}

#[tauri::command]
fn app_usage_clear(state: State<'_, AppState>) -> Result<AppUsageSnapshot, String> {
    ensure_app_usage_enabled(&state)?;
    push_debug_log(&state, "app_usage", "app_usage.clear_requested");
    app_usage::clear()
}

#[tauri::command]
fn key_usage_get_snapshot(state: State<'_, AppState>) -> Result<KeyUsageSnapshot, String> {
    ensure_key_usage_enabled(&state)?;
    push_debug_log(&state, "key_usage", "key_usage.snapshot.requested");
    key_usage::snapshot()
}

#[tauri::command]
fn key_usage_clear(state: State<'_, AppState>) -> Result<KeyUsageSnapshot, String> {
    ensure_key_usage_enabled(&state)?;
    push_debug_log(&state, "key_usage", "key_usage.clear_requested");
    key_usage::clear()
}

#[tauri::command]
fn timer_get_snapshot(state: State<'_, AppState>) -> Result<TimerSnapshot, String> {
    ensure_timer_enabled(&state)?;
    push_debug_log(&state, "timer", "timer.snapshot.requested");
    timer::snapshot()
}

#[tauri::command]
fn timer_create(
    state: State<'_, AppState>,
    input: TimerCreateInput,
) -> Result<TimerSnapshot, String> {
    ensure_timer_enabled(&state)?;
    push_debug_log(&state, "timer", "timer.create_requested");
    timer::create(input)
}

#[tauri::command]
fn timer_update(
    state: State<'_, AppState>,
    input: TimerUpdateInput,
) -> Result<TimerSnapshot, String> {
    ensure_timer_enabled(&state)?;
    push_debug_log(&state, "timer", "timer.update_requested");
    timer::update(input)
}

#[tauri::command]
fn timer_start(state: State<'_, AppState>, id: String) -> Result<TimerSnapshot, String> {
    ensure_timer_enabled(&state)?;
    push_debug_log(&state, "timer", format!("timer.start_requested id={id}"));
    timer::start_timer(id)
}

#[tauri::command]
fn timer_pause(state: State<'_, AppState>, id: String) -> Result<TimerSnapshot, String> {
    ensure_timer_enabled(&state)?;
    push_debug_log(&state, "timer", format!("timer.pause_requested id={id}"));
    timer::pause_timer(id)
}

#[tauri::command]
fn timer_pause_running(state: State<'_, AppState>) -> Result<TimerSnapshot, String> {
    ensure_timer_enabled(&state)?;
    push_debug_log(&state, "timer", "timer.pause_running_requested");
    timer::pause_running_timers()
}

#[tauri::command]
fn timer_reset(state: State<'_, AppState>, id: String) -> Result<TimerSnapshot, String> {
    ensure_timer_enabled(&state)?;
    push_debug_log(&state, "timer", format!("timer.reset_requested id={id}"));
    timer::reset_timer(id)
}

#[tauri::command]
fn timer_reset_active(state: State<'_, AppState>) -> Result<TimerSnapshot, String> {
    ensure_timer_enabled(&state)?;
    push_debug_log(&state, "timer", "timer.reset_active_requested");
    timer::reset_active_timers()
}

#[tauri::command]
fn timer_reorder(
    state: State<'_, AppState>,
    input: TimerReorderInput,
) -> Result<TimerSnapshot, String> {
    ensure_timer_enabled(&state)?;
    push_debug_log(&state, "timer", "timer.reorder_requested");
    timer::reorder_timers(input)
}

#[tauri::command]
fn timer_delete(state: State<'_, AppState>, id: String) -> Result<TimerSnapshot, String> {
    ensure_timer_enabled(&state)?;
    push_debug_log(&state, "timer", format!("timer.delete_requested id={id}"));
    timer::delete_timer(id)
}

#[tauri::command]
fn timer_open_free_window(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<(), String> {
    ensure_timer_enabled(&state)?;
    push_debug_log(&state, "timer", format!("timer.free_window.open_requested id={id}"));
    window_service::open_timer_free_window(&app, &id)
}

#[tauri::command]
fn timer_open_clock_window(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    ensure_timer_enabled(&state)?;
    push_debug_log(&state, "timer", "timer.clock_window.open_requested");
    window_service::open_timer_clock_window(&app)
}

#[tauri::command]
fn timer_get_free_window_count(app: AppHandle, state: State<'_, AppState>) -> Result<usize, String> {
    ensure_timer_enabled(&state)?;
    Ok(window_service::timer_free_window_count(&app))
}

#[tauri::command]
fn clipboard_get_snapshot(state: State<'_, AppState>) -> Result<ClipboardSnapshot, String> {
    ensure_clipboard_enabled(&state)?;
    push_debug_log(&state, "clipboard", "clipboard.snapshot.requested");
    clipboard::snapshot()
}

#[tauri::command]
fn clipboard_query(
    state: State<'_, AppState>,
    input: ClipboardQueryInput,
) -> Result<ClipboardQueryResult, String> {
    ensure_clipboard_enabled(&state)?;
    push_debug_log(
        &state,
        "clipboard",
        format!(
            "clipboard.query.requested scope={:?} search_len={} limit={:?} offset={:?}",
            input.scope,
            input.search.as_deref().unwrap_or("").len(),
            input.limit,
            input.offset
        ),
    );
    clipboard::query(input)
}

#[tauri::command]
fn clipboard_update_settings(
    state: State<'_, AppState>,
    patch: ClipboardSettingsPatch,
) -> Result<ClipboardSnapshot, String> {
    ensure_clipboard_enabled(&state)?;
    push_debug_log(&state, "clipboard", "clipboard.settings.update_requested");
    clipboard::update_settings(patch)
}

#[tauri::command]
fn clipboard_create_manual(
    state: State<'_, AppState>,
    title: String,
    text: String,
) -> Result<Option<ClipboardEntry>, String> {
    ensure_clipboard_enabled(&state)?;
    push_debug_log(
        &state,
        "clipboard",
        format!("clipboard.manual_create.requested title_len={} text_len={}", title.len(), text.len()),
    );
    clipboard::create_manual(title, text)
}

#[tauri::command]
fn clipboard_update_entry(
    state: State<'_, AppState>,
    id: String,
    patch: ClipboardEntryPatch,
) -> Result<Option<ClipboardEntry>, String> {
    ensure_clipboard_enabled(&state)?;
    push_debug_log(
        &state,
        "clipboard",
        format!("clipboard.entry.update_requested id={id}"),
    );
    clipboard::update_entry(id, patch)
}

#[tauri::command]
fn clipboard_copy(state: State<'_, AppState>, id: String) -> Result<ClipboardPasteResult, String> {
    ensure_clipboard_enabled(&state)?;
    push_debug_log(&state, "clipboard", format!("clipboard.copy.requested id={id}"));
    clipboard::copy_entry(id)
}

#[tauri::command]
fn clipboard_paste(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
) -> Result<ClipboardPasteResult, String> {
    ensure_clipboard_enabled(&state)?;
    push_debug_log(&state, "clipboard", format!("clipboard.paste.requested id={id}"));
    let result = clipboard::paste_entry(id)?;
    if result.copied {
        if window_service::is_clipboard_popup_pinned() {
            window_service::refocus_clipboard_popup_after_paste(&app);
        } else {
            window_service::close_clipboard_popup(&app);
        }
    }
    Ok(result)
}

#[tauri::command]
fn clipboard_copy_text(
    state: State<'_, AppState>,
    text: String,
) -> Result<ClipboardPasteResult, String> {
    ensure_clipboard_enabled(&state)?;
    push_debug_log(
        &state,
        "clipboard",
        format!("clipboard.copy_text.requested text_len={}", text.len()),
    );
    clipboard::copy_text(text)
}

#[tauri::command]
fn clipboard_copy_derived_text(
    state: State<'_, AppState>,
    text: String,
) -> Result<ClipboardPasteResult, String> {
    ensure_clipboard_enabled(&state)?;
    push_debug_log(
        &state,
        "clipboard",
        format!("clipboard.copy_derived_text.requested text_len={}", text.len()),
    );
    clipboard::copy_derived_text(text)
}

#[tauri::command]
fn clipboard_paste_text(
    app: AppHandle,
    state: State<'_, AppState>,
    text: String,
) -> Result<ClipboardPasteResult, String> {
    ensure_clipboard_enabled(&state)?;
    push_debug_log(
        &state,
        "clipboard",
        format!("clipboard.paste_text.requested text_len={}", text.len()),
    );
    let result = clipboard::paste_text(text)?;
    if result.copied {
        window_service::refocus_clipboard_popup_after_paste(&app);
    }
    Ok(result)
}

#[tauri::command]
fn clipboard_delete(state: State<'_, AppState>, ids: Vec<String>) -> Result<(), String> {
    ensure_clipboard_enabled(&state)?;
    push_debug_log(&state, "clipboard", format!("clipboard.delete.requested count={}", ids.len()));
    clipboard::delete_entries(ids)
}

#[tauri::command]
fn clipboard_restore(state: State<'_, AppState>, ids: Vec<String>) -> Result<(), String> {
    ensure_clipboard_enabled(&state)?;
    push_debug_log(&state, "clipboard", format!("clipboard.restore.requested count={}", ids.len()));
    clipboard::restore_entries(ids)
}

#[tauri::command]
fn clipboard_purge(state: State<'_, AppState>, ids: Vec<String>) -> Result<(), String> {
    ensure_clipboard_enabled(&state)?;
    push_debug_log(&state, "clipboard", format!("clipboard.purge.requested count={}", ids.len()));
    clipboard::purge_entries(ids)
}

#[tauri::command]
fn clipboard_clear_history(state: State<'_, AppState>) -> Result<(), String> {
    ensure_clipboard_enabled(&state)?;
    push_debug_log(&state, "clipboard", "clipboard.clear_history.requested");
    clipboard::clear_history()
}

#[tauri::command]
fn clipboard_open_panel(app: AppHandle, state: State<'_, AppState>) -> Result<(), String> {
    ensure_clipboard_enabled(&state)?;
    push_debug_log(&state, "clipboard", "clipboard.panel.open_requested");
    window_service::open_clipboard_popup(&app)
}

#[tauri::command]
fn clipboard_close_panel(app: AppHandle) {
    window_service::close_clipboard_popup(&app);
}

#[tauri::command]
fn clipboard_set_panel_pinned(pinned: bool) {
    window_service::set_clipboard_popup_pinned(pinned);
}

#[tauri::command]
fn clipboard_set_panel_dragging(dragging: bool) {
    window_service::set_clipboard_popup_dragging(dragging);
}

#[tauri::command]
fn clipboard_start_panel_drag(app: AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        push_debug_log(&state, "clipboard", "clipboard.panel.drag_started");
    }
    window_service::start_clipboard_popup_drag(&app);
}

#[tauri::command]
fn clipboard_open_management(app: AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        push_debug_log(&state, "clipboard", "clipboard.management.open_requested");
    }
    window_service::close_clipboard_popup(&app);
    show_main_window(&app);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.emit("navigate-tool", "clipboard");
    }
}

#[tauri::command]
fn set_auto_start_enabled(
    app: AppHandle,
    state: State<'_, AppState>,
    enabled: bool,
) -> Result<AppSnapshot, String> {
    set_auto_start_plugin(&app, enabled)?;
    let mut registry = state.registry.lock().map_err(|_| "工具注册表不可用")?;
    registry.settings_mut().auto_start = enabled;
    save_app_settings(&state, registry.settings())?;
    push_debug_log(
        &state,
        "settings",
        format!("app.autostart.updated enabled={enabled}"),
    );
    app_snapshot(&state, &registry)
}

#[tauri::command]
fn get_default_storage_path(state: State<'_, AppState>) -> Result<String, String> {
    Ok(default_storage_path(&state)?.display().to_string())
}

#[tauri::command]
fn open_storage_path(
    state: State<'_, AppState>,
    storage_path: Option<String>,
) -> Result<(), String> {
    let path = resolve_storage_path(&state, storage_path)?;
    fs::create_dir_all(&path).map_err(|error| format!("创建存储目录失败: {error}"))?;
    Command::new("explorer")
        .arg(&path)
        .spawn()
        .map_err(|error| format!("打开存储目录失败: {error}"))?;
    push_debug_log(
        &state,
        "storage",
        format!("storage.opened path={}", path.display()),
    );
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
    let updated_theme = patch.theme.clone();
    if let Some(theme) = patch.theme {
        settings.theme = theme;
    }
    if let Some(always_on_top) = patch.main_window_always_on_top {
        settings.main_window_always_on_top = always_on_top;
        if let Some(window) = app.get_webview_window("main") {
            window
                .set_always_on_top(always_on_top)
                .map_err(|error| format!("更新主窗口置顶失败: {error}"))?;
        }
        push_debug_log(
            &state,
            "window",
            format!("main_window.always_on_top.updated enabled={always_on_top}"),
        );
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
        fs::create_dir_all(&next_storage_dir)
            .map_err(|error| format!("创建存储目录失败: {error}"))?;
        settings.storage_path = storage_path;
        let next_settings_path = settings_path_for(&next_storage_dir);
        AppSettings::save(&next_settings_path, settings)?;
        {
            let mut active_settings_path =
                state.settings_path.lock().map_err(|_| "设置路径不可用")?;
            *active_settings_path = next_settings_path;
        }
        write_storage_pointer(&state.default_config_dir, &next_storage_dir)?;
        clipboard::relocate(&next_storage_dir)?;
        app_usage::relocate(&next_storage_dir)?;
        key_usage::relocate(&next_storage_dir)?;
        timer::relocate(&next_storage_dir)?;
        window_pinner::relocate(&next_storage_dir)?;
    }
    save_app_settings(&state, registry.settings())?;
    if let Some(theme) = updated_theme {
        let _ = app.emit("app-theme-changed", theme);
    }
    push_debug_log(&state, "settings", "app.settings.saved");
    app_snapshot(&state, &registry)
}

fn show_main_window(app: &AppHandle) {
    if let Some(state) = app.try_state::<AppState>() {
        push_debug_log(&state, "window", "main_window.show_requested");
    }
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
                if let Some(state) = app.try_state::<AppState>() {
                    push_debug_log(&state, "app", "tray.quit_requested");
                }
                window_service::close_clipboard_popup(app);
                clipboard::stop();
                app_usage::stop();
                key_usage::stop();
                timer::stop();
                window_pinner::unpin_all();
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
                if let Some(state) = tray.app_handle().try_state::<AppState>() {
                    push_debug_log(&state, "window", "tray.left_click_show_requested");
                }
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
        .plugin(
            tauri_plugin_autostart::Builder::new()
                .app_name("LightweightToolset")
                .build(),
        )
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        let mut handled = false;
                        let mut window_pinner_hotkey = false;
                        if let Some(state) = app.try_state::<AppState>() {
                            push_debug_log(
                                &state,
                                "hotkey",
                                format!("tool.hotkey.triggered shortcut={shortcut}"),
                            );
                            if let Ok(registry) = state.registry.lock() {
                                let shortcut_text = shortcut.to_string();
                                let app_hotkey = registry.app_hotkey_for_shortcut(&shortcut_text);
                                if app_hotkey.as_deref() == Some("main_window") {
                                    handled = false;
                                } else if registry.tool_for_shortcut(&shortcut_text).as_deref() == Some("window_pinner") {
                                    window_pinner_hotkey = true;
                                    handled = true;
                                } else if registry.tool_for_shortcut(&shortcut_text).as_deref()
                                    == Some("clipboard")
                                    || registry.only_enabled_tool().as_deref() == Some("clipboard")
                                {
                                    handled = true;
                                }
                            }
                            if window_pinner_hotkey {
                                match window_pinner::toggle_foreground(std::process::id()) {
                                    Ok(action) => {
                                        windows_notification::notify_window_pinner(&action.message);
                                        let _ = app.emit("window-pinner-changed", &action);
                                    }
                                    Err(error) => { let _ = app.emit("window-pinner-error", error); }
                                }
                            }
                        }
                        if window_pinner_hotkey {
                            return;
                        }
                        if handled {
                            if let Err(error) = window_service::open_clipboard_popup(app) {
                                if let Some(state) = app.try_state::<AppState>() {
                                    push_debug_log(
                                        &state,
                                        "error",
                                        format!("clipboard.popup.open_failed error={error}"),
                                    );
                                }
                            }
                        } else {
                            show_main_window(app);
                        }
                    }
                })
                .build(),
        )
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            show_main_window(app)
        }))
        .on_window_event(|window, event| {
            if window.label() == "main" {
                if let WindowEvent::CloseRequested { api, .. } = event {
                    let should_hide = window
                        .try_state::<AppState>()
                        .and_then(|state| {
                            state.registry.lock().ok().map(|registry| {
                                registry.settings().close_behavior == CloseBehavior::Tray
                            })
                        })
                        .unwrap_or(true);
                    if should_hide {
                        if let Some(state) = window.try_state::<AppState>() {
                            push_debug_log(&state, "window", "main_window.close_to_tray_requested");
                        }
                        api.prevent_close();
                        let _ = window.hide();
                    }
                }
            }
        })
        .setup(|app| {
            windows_notification::prepare_app_identity();
            let _supported_window_kinds = window_service::reserved_window_kinds();
            let config_dir = app.path().app_config_dir()?;
            std::fs::create_dir_all(&config_dir)?;
            let storage_dir = initial_storage_dir(&config_dir)?;
            migrate_storage_files(&config_dir, &storage_dir)?;
            write_storage_pointer(&config_dir, &storage_dir)?;
            clipboard::init(&storage_dir)?;
            app_usage::init(&storage_dir)?;
            key_usage::init(&storage_dir)?;
            timer::init(&storage_dir)?;
            let settings_path = settings_path_for(&storage_dir);
            let settings = AppSettings::load(&settings_path)?;
            window_pinner::init(&storage_dir, settings.window_pinner_max_pins)?;
            let mut registry = ToolRegistry::new(settings);
            if !registry.is_enabled("window_pinner") {
                window_pinner::unpin_all();
            }
            if registry.settings().auto_start {
                let _ = set_auto_start_plugin(app.handle(), true);
            }
            registry.start_enabled(app.handle())?;
            if let Err(error) = registry.register_app_hotkeys(app.handle()) {
                eprintln!("[hotkey] app hotkey registration skipped: {error}");
            }
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_title(&registry.settings().window_title);
                let _ = window.set_always_on_top(registry.settings().main_window_always_on_top);
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
                    level: "app",
                    message: "app.started profile=development".to_owned(),
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
            clear_tool_hotkey,
            suspend_tool_hotkeys,
            resume_tool_hotkeys,
            set_auto_start_enabled,
            app_usage_get_snapshot,
            app_usage_update_settings,
            app_usage_update_process,
            app_usage_clear,
            key_usage_get_snapshot,
            key_usage_clear,
            timer_get_snapshot,
            timer_create,
            timer_update,
            timer_start,
            timer_pause,
            timer_pause_running,
            timer_reset,
            timer_reset_active,
            timer_reorder,
            timer_delete,
            timer_open_free_window,
            timer_open_clock_window,
            timer_get_free_window_count,
            window_pinner_get_snapshot,
            window_pinner_unpin,
            window_pinner_unpin_all,
            window_pinner_set_max_pins,
            clipboard_get_snapshot,
            clipboard_query,
            clipboard_update_settings,
            clipboard_create_manual,
            clipboard_update_entry,
            clipboard_copy,
            clipboard_paste,
            clipboard_copy_text,
            clipboard_copy_derived_text,
            clipboard_paste_text,
            clipboard_delete,
            clipboard_restore,
            clipboard_purge,
            clipboard_clear_history,
            clipboard_open_panel,
            clipboard_close_panel,
            clipboard_set_panel_pinned,
            clipboard_set_panel_dragging,
            clipboard_start_panel_drag,
            clipboard_open_management,
            get_default_storage_path,
            open_storage_path,
            update_app_settings
        ])
        .run(tauri::generate_context!())
        .expect("启动 LightweightToolset 失败");
}
