mod app_usage;
mod autostart;
mod clipboard;
mod input_monitor;
mod key_usage;
mod native_tip;
mod screenshot;
mod settings;
mod shortcut_tip;
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
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex, OnceLock,
    },
    time::{Instant, SystemTime, UNIX_EPOCH},
};

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Emitter, Manager, State, WindowEvent,
};
use tauri_plugin_global_shortcut::ShortcutState;

use app_usage::{AppUsageProcessPatch, AppUsageSettingsPatch, AppUsageSnapshot};
use clipboard::{
    ClipboardEntry, ClipboardEntryPatch, ClipboardPasteResult, ClipboardQueryInput,
    ClipboardQueryResult, ClipboardSettingsPatch, ClipboardSnapshot,
};
use key_usage::KeyUsageSnapshot;
use settings::{AppSettings, CloseBehavior, ThemeMode};
use shortcut_tip::{ShortcutTipInput, ShortcutTipSnapshot};
use timer::{TimerCreateInput, TimerReorderInput, TimerSnapshot, TimerUpdateInput};
use window_pinner::WindowPinnerSnapshot;
use tools::{app_hotkey_snapshots, ToolRegistry, ToolSnapshot};

const SETTINGS_FILE: &str = "settings.json";
const STORAGE_POINTER_FILE: &str = "storage_path.txt";

pub struct AppState {
    registry: Mutex<ToolRegistry>,
    default_config_dir: PathBuf,
    runtime_storage_dir: PathBuf,
    settings_path: Mutex<PathBuf>,
    process_started_at: Instant,
    cold_start_ms: Mutex<Option<u128>>,
    debug_logs: Mutex<VecDeque<DebugLogEntry>>,
    runtime_started: AtomicBool,
    runtime_ready: AtomicBool,
    runtime_error: Mutex<Option<String>>,
}

static PROCESS_STARTED_AT: OnceLock<Instant> = OnceLock::new();

impl AppState {
    /// 当前存储目录：优先读指针文件，保证运行中改存储路径后截图目录立刻跟上。
    pub(crate) fn storage_dir(&self) -> PathBuf {
        read_storage_pointer(&self.default_config_dir)
            .unwrap_or_else(|| self.runtime_storage_dir.clone())
    }
}

pub fn mark_process_start() {
    PROCESS_STARTED_AT.get_or_init(Instant::now);
}

pub fn run_window_pinner_watchdog_from_args() -> bool {
    window_pinner::run_watchdog_from_args()
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSnapshot {
    tools: Vec<ToolSnapshot>,
    app_hotkeys: Vec<ToolSnapshot>,
    cold_start_ms: u128,
    settings: AppSettings,
    runtime_ready: bool,
    runtime_error: Option<String>,
    /// 开发版（debug 构建）标记：自启等系统级能力在开发版中固定关闭。
    dev_build: bool,
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
    silent_start: Option<bool>,
    auto_check_updates: Option<bool>,
    show_update_notification: Option<bool>,
    window_title: Option<String>,
    close_behavior: Option<CloseBehavior>,
    developer_mode: Option<bool>,
    storage_path: Option<String>,
    screenshot_manual_save_dir: Option<String>,
    screenshot_quick_save_dir: Option<String>,
    screenshot_palette: Option<Vec<String>>,
    screenshot_file_template: Option<String>,
    screenshot_dim_color: Option<String>,
    screenshot_dim_opacity: Option<u8>,
    screenshot_magnifier_grid: Option<bool>,
    screenshot_magnifier_grid_color: Option<String>,
    screenshot_guides_mode: Option<String>,
    screenshot_guides_color: Option<String>,
    screenshot_cursor_guide_color: Option<String>,
    screenshot_guides_width: Option<u8>,
    screenshot_cursor_guide_width: Option<u8>,
    screenshot_pin_lock: Option<String>,
    screenshot_pin_double_click: Option<String>,
    screenshot_pin_confirm_close: Option<bool>,
    screenshot_pin_shadow: Option<bool>,
    screenshot_pin_history: Option<u8>,
    screenshot_pin_auto_copy_text: Option<bool>,
    screenshot_ui_detection: Option<String>,
}

fn cold_start_ms(state: &AppState) -> Result<u128, String> {
    let mut metric = state.cold_start_ms.lock().map_err(|_| "性能指标不可用")?;
    Ok(*metric.get_or_insert_with(|| state.process_started_at.elapsed().as_millis()))
}

fn app_snapshot(state: &AppState, registry: &ToolRegistry) -> Result<AppSnapshot, String> {
    let runtime_error = state
        .runtime_error
        .lock()
        .map_err(|_| "运行时状态不可用")?
        .clone();
    Ok(AppSnapshot {
        tools: registry.snapshot(),
        app_hotkeys: app_hotkey_snapshots(registry.settings()),
        cold_start_ms: cold_start_ms(state)?,
        settings: registry.settings().clone(),
        runtime_ready: state.runtime_ready.load(Ordering::Acquire),
        runtime_error,
        dev_build: cfg!(debug_assertions),
    })
}

fn start_runtime_initialization(app: AppHandle, storage_dir: PathBuf) {
    tauri::async_runtime::spawn_blocking(move || {
        let result = (|| -> Result<(), String> {
            window_pinner::start_watchdog(&storage_dir)?;

            let state = app.state::<AppState>();
            // 自启注册会启动 schtasks 子进程，放在持有注册表锁之前执行，避免阻塞前端快照轮询。
            let auto_start = {
                let registry = state.registry.lock().map_err(|_| "工具注册表不可用")?;
                registry.settings().auto_start
            };
            if auto_start {
                if let Err(error) = autostart::apply(true) {
                    push_debug_log(&state, "error", format!("app.autostart.apply_failed error={error}"));
                }
            }

            let mut registry = state.registry.lock().map_err(|_| "工具注册表不可用")?;
            registry.start_enabled(&app)?;
            let hotkeys_registered = match registry.register_app_hotkeys(&app) {
                Ok(()) => true,
                Err(error) => {
                    eprintln!("[hotkey] app hotkey registration skipped: {error}");
                    false
                }
            };
            // 静默启动时补一条启动提示：仅在实际隐藏了主窗口的那次启动发送。
            if registry.settings().silent_start {
                let hotkey = if hotkeys_registered {
                    Some(registry.app_hotkey_text("main_window"))
                } else {
                    None
                };
                std::thread::spawn(move || windows_notification::notify_silent_start(hotkey.as_deref()));
            }
            Ok(())
        })();

        let state = app.state::<AppState>();
        if let Err(error) = &result {
            if let Ok(mut runtime_error) = state.runtime_error.lock() {
                *runtime_error = Some(error.clone());
            }
            push_debug_log(&state, "error", format!("app.runtime.start_failed error={error}"));
        } else {
            push_debug_log(&state, "app", "app.runtime.started");
        }
        state.runtime_ready.store(true, Ordering::Release);
        let _ = app.emit("app-runtime-state-changed", ());
    });
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

#[tauri::command]
fn get_app_snapshot(app: AppHandle, state: State<'_, AppState>) -> Result<AppSnapshot, String> {
    let snapshot = {
        let registry = state.registry.lock().map_err(|_| "工具注册表不可用")?;
        app_snapshot(&state, &registry)?
    };
    if state
        .runtime_started
        .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
        .is_ok()
    {
        start_runtime_initialization(app, state.runtime_storage_dir.clone());
    }
    Ok(snapshot)
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

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct NativeTipMetrics {
    work_left: i32,
    work_top: i32,
    work_right: i32,
    work_bottom: i32,
    window_width: i32,
    window_height: i32,
    default_x: i32,
    default_y: i32,
}

/// 定位滑块需要的屏幕/窗口度量（物理像素）。
#[tauri::command]
fn native_tip_metrics(state: State<'_, AppState>) -> Result<NativeTipMetrics, String> {
    ensure_tool_enabled(&state, "shortcut_tip")?;
    let (work_left, work_top, work_right, work_bottom, window_width, window_height, default_x, default_y) =
        native_tip::metrics();
    Ok(NativeTipMetrics {
        work_left,
        work_top,
        work_right,
        work_bottom,
        window_width,
        window_height,
        default_x,
        default_y,
    })
}

/// 定位模式开关：开启期间后端直接丢弃普通弹出请求（不仅依赖前端禁用按钮）。
#[tauri::command]
fn native_tip_set_positioning(active: bool) -> Result<(), String> {
    native_tip::set_positioning(active);
    Ok(())
}

/// 收起提示窗（定位预览结束或关闭工具时）。
#[tauri::command]
fn native_tip_hide(state: State<'_, AppState>) -> Result<(), String> {
    ensure_tool_enabled(&state, "shortcut_tip")?;
    native_tip::hide();
    Ok(())
}

/// 前端运行时错误落盘：页面白屏后内存日志读不到，这里直接写
/// `<存储目录>/logs/frontend.log`，便于事后定位（只记录，不影响功能）。
#[tauri::command]
fn frontend_error_log(state: State<'_, AppState>, message: String) {
    let dir = state.runtime_storage_dir.join("logs");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let path = dir.join("frontend.log");
    if std::fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0) > 256 * 1024 {
        let _ = std::fs::remove_file(&path);
    }
    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        use std::io::Write;
        let _ = writeln!(file, "{timestamp_ms} {message}");
    }
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
    let snapshot = clipboard::update_settings(patch)?;
    Ok(snapshot)
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
    let result = clipboard::copy_entry(id)?;
    if result.copied {
        shortcut_tip::notify_text("已复制", shortcut_tip::Origin::Command);
    }
    Ok(result)
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
        shortcut_tip::notify_text("已粘贴", shortcut_tip::Origin::Command);
        // 随后会模拟一次 Ctrl+V，短暂屏蔽快捷键匹配，避免多弹一条。
        shortcut_tip::suppress_shortcuts(900);
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
    let result = clipboard::copy_text(text)?;
    if result.copied {
        shortcut_tip::notify_text("已复制", shortcut_tip::Origin::Command);
    }
    Ok(result)
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
    let result = clipboard::copy_derived_text(text)?;
    if result.copied {
        shortcut_tip::notify_text("已复制", shortcut_tip::Origin::Command);
    }
    Ok(result)
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
        shortcut_tip::notify_text("已粘贴", shortcut_tip::Origin::Command);
        shortcut_tip::suppress_shortcuts(900);
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
fn shortcut_tip_ready() {
    shortcut_tip::mark_ready();
}

/// 应用内窗口上报的快捷键（WebView2 聚焦时全局钩子可能收不到按键）。
#[tauri::command]
fn shortcut_tip_begin_capture() {
    shortcut_tip::begin_capture();
}

#[tauri::command]
fn shortcut_tip_end_capture() {
    shortcut_tip::end_capture();
}

#[tauri::command]
fn shortcut_tip_report(keys: String) {
    shortcut_tip::notify_shortcut(&keys, shortcut_tip::Origin::Ui);
}

#[tauri::command]
fn shortcut_tip_set_appearance(
    appearance: shortcut_tip::ShortcutTipAppearance,
) -> Result<ShortcutTipSnapshot, String> {
    shortcut_tip::set_appearance(appearance)
}

#[tauri::command]
fn shortcut_tip_snapshot() -> Result<ShortcutTipSnapshot, String> {
    shortcut_tip::snapshot()
}

#[tauri::command]
fn shortcut_tip_upsert(
    state: State<'_, AppState>,
    entry: ShortcutTipInput,
) -> Result<ShortcutTipSnapshot, String> {
    ensure_tool_enabled(&state, "shortcut_tip")?;
    shortcut_tip::upsert(entry)
}

#[tauri::command]
fn shortcut_tip_delete(
    state: State<'_, AppState>,
    id: String,
) -> Result<ShortcutTipSnapshot, String> {
    ensure_tool_enabled(&state, "shortcut_tip")?;
    shortcut_tip::delete(id)
}

#[tauri::command]
fn shortcut_tip_restore_presets(
    state: State<'_, AppState>,
) -> Result<ShortcutTipSnapshot, String> {
    ensure_tool_enabled(&state, "shortcut_tip")?;
    shortcut_tip::restore_presets()
}

/// 「原生提示实验」工具：在工具页点击测试按钮后弹出一次原生绘制的提示浮窗。
/// 与 `shortcut_tip` 完全隔离：不接它的按键链路，也不写它的设置。
#[tauri::command]
fn native_tip_show(
    app: tauri::AppHandle,
    state: State<'_, AppState>,
    payload: native_tip::NativeTipPayload,
) -> Result<(), String> {
    ensure_tool_enabled(&state, "shortcut_tip")?;
    log_main_window_event(
        &app,
        &format!(
            "native_tip.show_requested count={} theme={} persistent={}",
            payload.count,
            payload.theme,
            payload.persistent.unwrap_or(false)
        ),
    );
    match native_tip::show(payload) {
        Ok(()) => Ok(()),
        Err(error) => {
            log_main_window_event(&app, &format!("native_tip.show_failed error={error}"));
            Err(error)
        }
    }
}



#[tauri::command]
fn clipboard_purge(state: State<'_, AppState>, ids: Vec<String>) -> Result<(), String> {
    ensure_clipboard_enabled(&state)?;
    push_debug_log(&state, "clipboard", format!("clipboard.purge.requested count={}", ids.len()));
    clipboard::purge_entries(ids)
}

#[tauri::command]
fn clipboard_purge_trash(state: State<'_, AppState>) -> Result<usize, String> {
    ensure_clipboard_enabled(&state)?;
    let removed = clipboard::purge_trash()?;
    push_debug_log(&state, "clipboard", format!("clipboard.purge_trash count={removed}"));
    Ok(removed)
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
    // 开发版固定关闭开机自启：不注册系统任务，也不把这个开关写进设置。
    if !autostart::is_supported() {
        let _ = app;
        push_debug_log(
            &state,
            "settings",
            "app.autostart.ignored reason=dev_build".to_owned(),
        );
        let registry = state.registry.lock().map_err(|_| "工具注册表不可用")?;
        return app_snapshot(&state, &registry);
    }

    autostart::apply(enabled)?;
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

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct ScreenshotDefaults {
    save_dir: String,
    palette: Vec<String>,
    file_template: String,
    dim_color: String,
    dim_opacity: u8,
    magnifier_grid: bool,
    magnifier_grid_color: String,
    guides_mode: String,
    guides_color: String,
    cursor_guide_color: String,
    guides_width: u8,
    cursor_guide_width: u8,
    pin_lock: String,
    pin_double_click: String,
    pin_confirm_close: bool,
    pin_shadow: bool,
    pin_history: u8,
}

/// 截图工具页「恢复默认」需要的全部内置默认值。
#[tauri::command]
fn get_screenshot_defaults(state: State<'_, AppState>) -> ScreenshotDefaults {
    ScreenshotDefaults {
        save_dir: screenshot::default_save_dir(&state).display().to_string(),
        palette: screenshot::default_palette_hex(),
        file_template: screenshot::default_file_template(),
        dim_color: "#000000".to_owned(),
        dim_opacity: screenshot::DEFAULT_DIM_OPACITY,
        magnifier_grid: true,
        magnifier_grid_color: "#666666".to_owned(),
        guides_mode: "alt".to_owned(),
        guides_color: "#30B4FF".to_owned(),
        cursor_guide_color: "#808080".to_owned(),
        guides_width: 1,
        cursor_guide_width: 1,
        pin_lock: "none".to_owned(),
        pin_double_click: "none".to_owned(),
        pin_confirm_close: false,
        pin_shadow: true,
        pin_history: 20,
    }
}

#[tauri::command]
fn open_screenshot_dir(state: State<'_, AppState>, save_dir: Option<String>) -> Result<(), String> {
    let path = screenshot::resolve_save_dir(&state, save_dir.as_deref().unwrap_or(""));
    fs::create_dir_all(&path).map_err(|error| format!("创建截图目录失败: {error}"))?;
    Command::new("explorer")
        .arg(&path)
        .spawn()
        .map_err(|error| format!("打开截图目录失败: {error}"))?;
    push_debug_log(
        &state,
        "screenshot",
        format!("screenshot.dir.opened path={}", path.display()),
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
    if let Some(dir) = patch.screenshot_manual_save_dir {
        settings.screenshot_manual_save_dir = dir;
    }
    if let Some(dir) = patch.screenshot_quick_save_dir {
        settings.screenshot_quick_save_dir = dir;
    }
    if let Some(palette) = patch.screenshot_palette {
        // 只接受能解析的 8 个色值，其余沿用默认，避免脏数据把调色板点坏。
        let defaults = settings.screenshot_palette.clone();
        let mut next = Vec::with_capacity(screenshot::PALETTE_LEN);
        for index in 0..screenshot::PALETTE_LEN {
            let candidate = palette
                .get(index)
                .filter(|value| screenshot::is_valid_hex(value))
                .cloned()
                .or_else(|| defaults.get(index).cloned())
                .unwrap_or_default();
            next.push(candidate);
        }
        settings.screenshot_palette = next;
    }
    if let Some(template) = patch.screenshot_file_template {
        // 空模板等于恢复默认，避免保存出空文件名。
        let trimmed = template.trim();
        settings.screenshot_file_template = if trimmed.is_empty() {
            screenshot::default_file_template()
        } else {
            trimmed.to_owned()
        };
    }
    if let Some(color) = patch.screenshot_dim_color {
        if screenshot::is_valid_hex(&color) {
            settings.screenshot_dim_color = color.to_uppercase();
        }
    }
    if let Some(opacity) = patch.screenshot_dim_opacity {
        settings.screenshot_dim_opacity = opacity.min(100);
    }
    if let Some(grid) = patch.screenshot_magnifier_grid {
        settings.screenshot_magnifier_grid = grid;
    }
    if let Some(color) = patch.screenshot_magnifier_grid_color {
        if screenshot::is_valid_hex(&color) {
            settings.screenshot_magnifier_grid_color = color.to_uppercase();
        }
    }
    if let Some(mode) = patch.screenshot_guides_mode {
        // 只接受三态里的值，其余保持原设置。
        if matches!(mode.as_str(), "always" | "alt" | "off") {
            settings.screenshot_guides_mode = mode;
        }
    }
    if let Some(color) = patch.screenshot_guides_color {
        if screenshot::is_valid_hex(&color) {
            settings.screenshot_guides_color = color.to_uppercase();
        }
    }
    if let Some(color) = patch.screenshot_cursor_guide_color {
        if screenshot::is_valid_hex(&color) {
            settings.screenshot_cursor_guide_color = color.to_uppercase();
        }
    }
    if let Some(width) = patch.screenshot_guides_width {
        settings.screenshot_guides_width = width.clamp(1, 5);
    }
    if let Some(width) = patch.screenshot_cursor_guide_width {
        settings.screenshot_cursor_guide_width = width.clamp(1, 5);
    }
    if let Some(lock) = patch.screenshot_pin_lock {
        if matches!(lock.as_str(), "none" | "position" | "all") {
            settings.screenshot_pin_lock = lock;
        }
    }
    if let Some(action) = patch.screenshot_pin_double_click {
        if matches!(action.as_str(), "none" | "close" | "copy") {
            settings.screenshot_pin_double_click = action;
        }
    }
    if let Some(confirm) = patch.screenshot_pin_confirm_close {
        settings.screenshot_pin_confirm_close = confirm;
    }
    if let Some(shadow) = patch.screenshot_pin_shadow {
        settings.screenshot_pin_shadow = shadow;
    }
    if let Some(history) = patch.screenshot_pin_history {
        settings.screenshot_pin_history = history.clamp(1, 100);
    }
    if let Some(auto_copy) = patch.screenshot_pin_auto_copy_text {
        settings.screenshot_pin_auto_copy_text = auto_copy;
        screenshot::set_pin_auto_copy_text(auto_copy);
    }
    if let Some(detection) = patch.screenshot_ui_detection {
        // 本轮只实现「仅窗口」；`window_ui` 先落库，运行时按「仅窗口」处理。
        if matches!(detection.as_str(), "none" | "window" | "window_ui") {
            settings.screenshot_ui_detection = detection;
        }
    }
    if let Some(window_title) = patch.window_title {
        let title = window_title.trim();
        if !title.is_empty() {
            settings.window_title = title.to_owned();
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_title(&display_window_title(title));
            }
        }
    }
    if let Some(close_behavior) = patch.close_behavior {
        settings.close_behavior = close_behavior;
    }
    if let Some(silent_start) = patch.silent_start {
        let was_enabled = settings.silent_start;
        settings.silent_start = silent_start;
        if silent_start && !was_enabled {
            // 首次开启时提示一次，避免下次启动后用户以为没启动成功。
            std::thread::spawn(windows_notification::notify_silent_start_enabled);
        }
    }
    if let Some(developer_mode) = patch.developer_mode {
        // 开发版固定开启开发者模式（界面开关也锁定），避免误关闭后看不到诊断信息。
        settings.developer_mode = if cfg!(debug_assertions) { true } else { developer_mode };
    }
    if patch.storage_path.is_some() && cfg!(debug_assertions) {
        // 开发版固定使用应用配置目录，避免测试数据写到别处、与安装版的判断混淆。
        push_debug_log(&state, "settings", "app.storage_path.ignored reason=dev_build");
    } else if let Some(storage_path) = patch.storage_path {
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
        shortcut_tip::relocate(&next_storage_dir)?;
        window_pinner::relocate(&next_storage_dir)?;
    }
    save_app_settings(&state, registry.settings())?;
    if let Some(theme) = updated_theme {
        let _ = app.emit("app-theme-changed", theme);
    }
    push_debug_log(&state, "settings", "app.settings.saved");
    app_snapshot(&state, &registry)
}

/// 主窗口的唤出入口（托盘、托盘菜单、快捷键、二次启动共用）。
///
/// 两条口径：
/// 1. 最小化到任务栏（未隐藏到托盘）时，`show()` 只做 `SW_SHOW`，不会把窗口还原——必须先
///    `unminimize()`，否则窗口会「显示但仍缩在任务栏」，表现为快捷键唤不出来。
/// 2. 隐藏窗口刚被 `SW_HIDE` 时，运行时对 `main` 的窗口记录可能还在重挂载，`get_webview_window`
///    会短暂返回 `None`；这里做有界重试，避免一次唤不出就彻底没反应。
fn show_main_window(app: &AppHandle) {
    #[cfg(windows)]
    log_main_window_event(
        app,
        &format!(
            "main_window.show_requested webview={}",
            main_window_webview_state(app)
        ),
    );
    #[cfg(not(windows))]
    log_main_window_event(app, "main_window.show_requested");
    let mut window = app.get_webview_window("main");
    let mut attempts = 0;
    while window.is_none() && attempts < 5 {
        attempts += 1;
        std::thread::sleep(std::time::Duration::from_millis(20));
        window = app.get_webview_window("main");
    }
    let Some(window) = window else {
        log_main_window_event(app, "main_window.show_failed reason=window_not_found");
        // 兜底：运行时记录缺失时按「本进程 + 窗口类 + 标题」找回 HWND 直接还原，避免只能重启软件。
        #[cfg(windows)]
        if force_show_main_window_native(app) {
            log_main_window_event(app, "main_window.show_recovered_by_hwnd");
        }
        return;
    };
    if window.is_minimized().unwrap_or(false) {
        if let Err(error) = window.unminimize() {
            log_main_window_event(app, &format!("main_window.unminimize_failed error={error}"));
        }
    }
    if let Err(error) = window.show() {
        log_main_window_event(app, &format!("main_window.show_failed error={error}"));
    }
    // show() 返回 Ok 不代表窗口真的出来了：隐藏窗口重新显示时可能被系统还原/焦点状态吞掉。
    // 这里回读真实状态（给跨线程派发留几帧余量），没出来就走原生兜底，避免「点了没反应」。
    let mut still_hidden = true;
    let mut still_minimized = false;
    for _ in 0..5 {
        std::thread::sleep(std::time::Duration::from_millis(20));
        still_hidden = !window.is_visible().unwrap_or(true);
        still_minimized = window.is_minimized().unwrap_or(false);
        if !still_hidden && !still_minimized {
            break;
        }
    }
    if still_hidden || still_minimized {
        log_main_window_event(
            app,
            &format!(
                "main_window.show_ineffective visible={} minimized={}",
                !still_hidden, still_minimized
            ),
        );
        #[cfg(windows)]
        if force_show_main_window_native(app) {
            log_main_window_event(app, "main_window.show_recovered_by_hwnd");
        }
    }
    let _ = window.set_focus();
}

/// 主窗口唤出事件的落盘日志：内存日志在界面卡死时读不到，这里同时追加到存储目录下的
/// `logs/window.log`（超过 256KB 时重置），用于事后定位「托盘/快捷键唤不出主窗口」的现场。
pub(crate) fn log_main_window_event(app: &AppHandle, message: &str) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    push_debug_log(&state, "window", message);
    let dir = state.runtime_storage_dir.join("logs");
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let path = dir.join("window.log");
    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    let line = format!("{timestamp_ms} {message}\n");
    let too_large = std::fs::metadata(&path)
        .map(|metadata| metadata.len() > 256 * 1024)
        .unwrap_or(false);
    use std::io::Write;
    let _ = if too_large {
        std::fs::write(&path, line)
    } else {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .and_then(|mut file| file.write_all(line.as_bytes()))
    };
}

/// 窗口标题的展示值：开发版在用户设置的标题后追加标识，存储值保持原文。
fn display_window_title(title: &str) -> String {
    if cfg!(debug_assertions) {
        format!("{title}（开发版）")
    } else {
        title.to_owned()
    }
}

/// 按「本进程 + `Tauri Window` 类 + 期望标题」找回主窗口 HWND：Tauri 的窗口记录不可信时用它兜底。
#[cfg(windows)]
fn find_main_window_hwnd(app: &AppHandle) -> Option<windows_sys::Win32::Foundation::HWND> {
    use windows_sys::Win32::Foundation::{HWND, MAX_PATH};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowExW, GetWindowTextW, GetWindowThreadProcessId,
    };

    let expected_title = app
        .try_state::<AppState>()
        .and_then(|state| {
            state
                .registry
                .lock()
                .ok()
                .map(|registry| display_window_title(&registry.settings().window_title))
        })
        .unwrap_or_default();

    let class: Vec<u16> = "Tauri Window\0".encode_utf16().collect();
    let mut previous: HWND = std::ptr::null_mut();
    loop {
        let hwnd = unsafe {
            FindWindowExW(
                std::ptr::null_mut(),
                previous,
                class.as_ptr(),
                std::ptr::null(),
            )
        };
        if hwnd.is_null() {
            return None;
        }
        previous = hwnd;
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
        if pid != std::process::id() {
            continue;
        }
        if !expected_title.is_empty() {
            let mut buffer = [0u16; MAX_PATH as usize];
            let length = unsafe { GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32) };
            let title = String::from_utf16_lossy(&buffer[..length.max(0) as usize]);
            if title != expected_title {
                continue;
            }
        }
        return Some(hwnd);
    }
}

/// 兜底唤出：Tauri 的窗口记录缺失或 `show()` 没生效时，直接按 HWND 还原并置前。
#[cfg(windows)]
fn force_show_main_window_native(app: &AppHandle) -> bool {
    use windows_sys::Win32::UI::WindowsAndMessaging::{SetForegroundWindow, ShowWindow, SW_RESTORE};

    let Some(hwnd) = find_main_window_hwnd(app) else {
        return false;
    };
    unsafe {
        ShowWindow(hwnd, SW_RESTORE);
        SetForegroundWindow(hwnd);
    }
    true
}

/// 主窗口里的 WebView2 子窗口还在不在（`alive` / `missing` / `no_window`）。
///
/// 用来区分「窗口显示失败」和「整个 WebView 进程没了」：后者表现为托盘菜单还能弹，
/// 但窗口、快捷弹窗、所有界面功能都失效——正是「整个软件像死的」那种现场。
#[cfg(windows)]
fn main_window_webview_state(app: &AppHandle) -> &'static str {
    use windows_sys::Win32::Foundation::LPARAM;
    use windows_sys::Win32::UI::WindowsAndMessaging::EnumChildWindows;

    let Some(hwnd) = find_main_window_hwnd(app) else {
        return "no_window";
    };
    let mut found = false;
    unsafe {
        EnumChildWindows(
            hwnd,
            Some(probe_webview_child),
            &mut found as *mut bool as LPARAM,
        );
    }
    if found {
        "alive"
    } else {
        "missing"
    }
}

#[cfg(windows)]
unsafe extern "system" fn probe_webview_child(
    hwnd: windows_sys::Win32::Foundation::HWND,
    lparam: windows_sys::Win32::Foundation::LPARAM,
) -> i32 {
    use windows_sys::Win32::UI::WindowsAndMessaging::GetClassNameW;

    let mut buffer = [0u16; 128];
    let length = GetClassNameW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32);
    if length > 0 {
        let class = String::from_utf16_lossy(&buffer[..length as usize]);
        if class.starts_with("Chrome_") {
            *(lparam as *mut bool) = true;
            return 0;
        }
    }
    1
}

/// 托盘悬停等处的应用名：正式版用产品中文名，开发版追加标识。
fn display_app_name(app: &AppHandle) -> String {
    let base = app
        .config()
        .product_name
        .clone()
        .unwrap_or_else(|| "轻量化工具集".to_owned());
    if cfg!(debug_assertions) {
        format!("{base}（开发版）")
    } else {
        base
    }
}

fn build_tray(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "显示主窗口", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &quit])?;

    TrayIconBuilder::with_id("main-tray")
        .icon(app.default_window_icon().expect("缺少应用图标").clone())
        .tooltip(display_app_name(app))
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
/// 快捷键提示的独立采键子进程入口（不初始化 Tauri / WebView）。
pub fn run_shortcut_helper(port: u16) -> ! {
    shortcut_tip::run_shortcut_helper(port)
}

pub fn run() {
    // 截图工具按物理像素工作；显式声明 Per-Monitor V2，保证 GetSystemMetrics/BitBlt
    // 拿到的是真实像素而不是 150% 缩放后的逻辑坐标。Tauri 自身也会设置同一档，
    // 这里先设置可确保我们的原生覆盖层与之一致（重复设置会返回失败，忽略即可）。
    #[cfg(windows)]
    unsafe {
        let _ = windows_sys::Win32::UI::HiDpi::SetProcessDpiAwarenessContext(
            windows_sys::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
        );
    }
    // 正式版不修改 identifier，debug 分支不存在时 `mut` 无意义。
    #[allow(unused_mut)]
    let mut context = tauri::generate_context!();
    #[cfg(debug_assertions)]
    {
        // 开发版使用独立标识：配置/数据目录、WebView2 数据、单实例互斥量与窗口类、通知 AUMID
        // 都由 identifier 派生，改成 `...-dev` 后与正式安装版完全隔离，可同时运行。
        let identifier = context.config().identifier.clone();
        context.config_mut().identifier = format!("{identifier}.dev");
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(
            tauri_plugin_global_shortcut::Builder::new()
                .with_handler(|app, shortcut, event| {
                    if event.state() == ShortcutState::Pressed {
                        let mut handled = false;
                        let mut show_main_window_hotkey = false;
                        let mut window_pinner_hotkey = false;
                        let mut screenshot_hotkey = false;
                        let mut pin_command: Option<String> = None;
                        let mut triggered_hotkey_id: Option<String> = None;
                        if let Some(state) = app.try_state::<AppState>() {
                            push_debug_log(
                                &state,
                                "hotkey",
                                format!("tool.hotkey.triggered shortcut={shortcut}"),
                            );
                            if let Ok(registry) = state.registry.lock() {
                                let shortcut_text = shortcut.to_string();
                                let app_hotkey = registry.app_hotkey_for_shortcut(&shortcut_text);
                                triggered_hotkey_id = app_hotkey
                                    .clone()
                                    .or_else(|| registry.tool_for_shortcut(&shortcut_text));
                                if app_hotkey.as_deref() == Some("main_window") {
                                    show_main_window_hotkey = true;
                                } else if registry.tool_for_shortcut(&shortcut_text).as_deref() == Some("window_pinner") {
                                    window_pinner_hotkey = true;
                                    handled = true;
                                } else if registry.tool_for_shortcut(&shortcut_text).as_deref() == Some("screenshot") {
                                    screenshot_hotkey = true;
                                    handled = true;
                                } else if let Some(command) = app_hotkey.clone() {
                                    // 贴图组快捷键：截图工具停用时不再响应。
                                    if command != "main_window" && registry.is_enabled("screenshot") {
                                        pin_command = Some(command);
                                        handled = true;
                                    }
                                } else if registry.tool_for_shortcut(&shortcut_text).as_deref()
                                    == Some("clipboard")
                                    || registry.only_enabled_tool().as_deref() == Some("clipboard")
                                {
                                    handled = true;
                                }
                            }
                            // 先派发快捷键提示，再执行动作：贴图组命令分支会直接 return，
                            // 提示若排在后面就永远轮不到（截图/呼出主窗口等排在其后，因此此前只它们会提示）。
                            if let Some(hotkey_id) = triggered_hotkey_id.clone() {
                                // 快捷键回调在主线程上，提示放到独立线程，避免阻塞 UI 或与其他锁互相等待。
                                std::thread::spawn(move || shortcut_tip::notify_app_hotkey(&hotkey_id));
                            }
                            if let Some(command) = pin_command.clone() {
                                match command.as_str() {
                                    "screenshot_pin" => screenshot::pin_from_hotkey(app),
                                    "pin_restore" => screenshot::restore_pin(app),
                                    "pin_close_all" => screenshot::close_all_pins(),
                                    "pin_toggle_visible" => screenshot::toggle_pins_visible(),
                                    "pin_passthrough" => screenshot::toggle_pins_passthrough(),
                                    _ => {}
                                }
                                return;
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
                        if screenshot_hotkey {
                            // 抓屏与覆盖层都在独立线程里跑，热键回调立即返回。
                            screenshot::capture_from_hotkey(app);
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
                        } else if show_main_window_hotkey {
                            show_main_window(app);
                        } else if let Some(state) = app.try_state::<AppState>() {
                            // 兜底不再当成「呼出主窗口」：未匹配的键只记录，避免误触把主窗口弹出来
                            // （历史缺陷：符号键的存储写法与回调文本不一致，落到这里就会弹主窗口）。
                            push_debug_log(
                                &state,
                                "hotkey",
                                format!("hotkey.unmatched shortcut={shortcut}"),
                            );
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
            windows_notification::prepare_app_identity(&app.config().identifier);
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
            shortcut_tip::init(&storage_dir, app.handle())?;
            let settings_path = settings_path_for(&storage_dir);
            let settings_file_existed = settings_path.exists();
            let mut settings = AppSettings::load(&settings_path)?;
            if cfg!(debug_assertions) && !settings_file_existed {
                // 开发版首次启动（数据目录全新）采用测试友好的默认值：全部工具关闭、
                // 静默启动/主窗口置顶/开发者模式默认开启；正式版默认值不受影响。
                tools::apply_development_defaults(&mut settings);
                AppSettings::save(&settings_path, &settings)?;
            }
            window_pinner::init(&storage_dir, settings.window_pinner_max_pins)?;
            let registry = ToolRegistry::new(settings);
            if !registry.is_enabled("window_pinner") {
                window_pinner::unpin_all();
            }
            let silent_start = registry.settings().silent_start;
            if let Some(window) = app.get_webview_window("main") {
                let _ = window.set_title(&display_window_title(&registry.settings().window_title));
                let _ = window.set_always_on_top(registry.settings().main_window_always_on_top);
                // 配置里主窗口默认不可见；非静默启动时才显示，避免启动瞬间闪窗。
                if !silent_start {
                    let _ = window.show();
                    let _ = window.set_focus();
                }
            }
            app.manage(AppState {
                registry: Mutex::new(registry),
                default_config_dir: config_dir,
                runtime_storage_dir: storage_dir,
                settings_path: Mutex::new(settings_path),
                process_started_at: *PROCESS_STARTED_AT.get_or_init(Instant::now),
                cold_start_ms: Mutex::new(None),
                debug_logs: Mutex::new(VecDeque::from([DebugLogEntry {
                    timestamp_ms: SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|duration| duration.as_millis())
                        .unwrap_or_default(),
                    level: "app",
                    message: format!(
                        "app.started profile={}",
                        if cfg!(debug_assertions) { "development" } else { "release" }
                    ),
                }])),
                runtime_started: AtomicBool::new(false),
                runtime_ready: AtomicBool::new(false),
                runtime_error: Mutex::new(None),
            });
            if silent_start {
                push_debug_log(&app.state::<AppState>(), "window", "main_window.silent_start");
            }
            // 贴图右键菜单里的「划词即复制」要能写回设置，且与工具页同步。
            {
                let handle = app.handle().clone();
                screenshot::set_pin_auto_copy_persist(move |enabled| {
                    let Some(state) = handle.try_state::<AppState>() else {
                        return;
                    };
                    let saved = {
                        let Ok(mut registry) = state.registry.lock() else {
                            return;
                        };
                        registry.settings_mut().screenshot_pin_auto_copy_text = enabled;
                        registry.settings().clone()
                    };
                    if let Err(error) = save_app_settings(&handle.state::<AppState>(), &saved) {
                        push_debug_log(&handle.state::<AppState>(), "screenshot", error);
                    }
                    let _ = handle.emit("screenshot-pin-auto-copy-changed", enabled);
                });
            }
            build_tray(app.handle())?;
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            get_app_snapshot,
            get_debug_logs,
            clear_debug_logs,
            push_frontend_debug_log,
            frontend_error_log,
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
            clipboard_purge_trash,
            shortcut_tip_ready,
            shortcut_tip_begin_capture,
            shortcut_tip_end_capture,
            shortcut_tip_report,
            shortcut_tip_set_appearance,
            shortcut_tip_snapshot,
            shortcut_tip_upsert,
            shortcut_tip_delete,
            shortcut_tip_restore_presets,
            native_tip_show,
            native_tip_metrics,
            native_tip_set_positioning,
            native_tip_hide,
            clipboard_clear_history,
            clipboard_open_panel,
            clipboard_close_panel,
            clipboard_set_panel_pinned,
            clipboard_set_panel_dragging,
            clipboard_start_panel_drag,
            clipboard_open_management,
            get_default_storage_path,
            open_storage_path,
            get_screenshot_defaults,
            open_screenshot_dir,
            update_app_settings
        ])
        .run(context)
        .expect("启动 LightweightToolset 失败");
}
