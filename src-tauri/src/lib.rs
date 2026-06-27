mod settings;
mod tools;
mod window_service;

use std::{fs, path::PathBuf, process::Command, sync::{mpsc, Mutex, OnceLock}, thread, time::Instant};

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, State, WindowEvent,
};
use tauri_plugin_global_shortcut::ShortcutState;
use windows::{
    core::HSTRING,
    Win32::{
        System::Com::{CoCreateInstance, CoInitializeEx, CoUninitialize, IBindCtx, CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED},
        UI::Shell::{
            FileOpenDialog, IFileOpenDialog, IShellItem, FOS_FORCEFILESYSTEM, FOS_PICKFOLDERS,
            SIGDN_FILESYSPATH, SHCreateItemFromParsingName,
        },
    },
};

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
    auto_check_updates: Option<bool>,
    show_update_notification: Option<bool>,
    window_title: Option<String>,
    close_behavior: Option<CloseBehavior>,
    developer_mode: Option<bool>,
    storage_path: Option<String>,
}

const AUTO_START_RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
const AUTO_START_RUN_VALUE: &str = "LightweightToolset";

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

fn default_storage_path(state: &AppState) -> Result<PathBuf, String> {
    state
        .settings_path
        .parent()
        .map(PathBuf::from)
        .ok_or_else(|| "默认存储目录不可用".to_owned())
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

fn set_auto_start_registry(enabled: bool) -> Result<(), String> {
    if enabled {
        let exe = std::env::current_exe().map_err(|error| format!("获取当前程序路径失败: {error}"))?;
        let target = format!("\"{}\"", exe.display());
        let status = Command::new("reg")
            .args(["add", AUTO_START_RUN_KEY, "/v", AUTO_START_RUN_VALUE, "/t", "REG_SZ", "/d"])
            .arg(target)
            .args(["/f"])
            .status()
            .map_err(|error| format!("写入开机自启注册表失败: {error}"))?;
        if !status.success() {
            return Err("写入开机自启注册表失败".to_owned());
        }
    } else {
        let _ = Command::new("reg")
            .args(["delete", AUTO_START_RUN_KEY, "/v", AUTO_START_RUN_VALUE, "/f"])
            .status();
    }
    Ok(())
}

#[tauri::command]
fn get_app_snapshot(state: State<'_, AppState>) -> Result<AppSnapshot, String> {
    let registry = state.registry.lock().map_err(|_| "工具注册表不可用")?;
    app_snapshot(&state, &registry)
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
    app_snapshot(&state, &registry)
}

#[tauri::command]
fn set_auto_start_enabled(state: State<'_, AppState>, enabled: bool) -> Result<AppSnapshot, String> {
    set_auto_start_registry(enabled)?;
    let mut registry = state.registry.lock().map_err(|_| "工具注册表不可用")?;
    registry.settings_mut().auto_start = enabled;
    AppSettings::save(&state.settings_path, registry.settings())?;
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
    Ok(())
}

fn show_folder_picker(initial_path: PathBuf) -> Result<Option<String>, String> {
    let (sender, receiver) = mpsc::channel();
    thread::spawn(move || {
        let result = unsafe {
            CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()
                .map_err(|error| format!("初始化目录选择器失败: {error}"))
                .and_then(|_| {
                    let dialog: IFileOpenDialog =
                        CoCreateInstance(&FileOpenDialog, None, CLSCTX_INPROC_SERVER)
                            .map_err(|error| format!("创建目录选择器失败: {error}"))?;
                    let options = dialog
                        .GetOptions()
                        .map_err(|error| format!("读取目录选择器配置失败: {error}"))?;
                    dialog
                        .SetOptions(options | FOS_PICKFOLDERS | FOS_FORCEFILESYSTEM)
                        .map_err(|error| format!("设置目录选择器失败: {error}"))?;
                    if initial_path.exists() {
                        if let Ok(folder) = SHCreateItemFromParsingName::<_, _, IShellItem>(
                            &HSTRING::from(initial_path.display().to_string()),
                            None::<&IBindCtx>,
                        ) {
                            let _ = dialog.SetFolder(&folder);
                        }
                    }
                    let picked = match dialog.Show(None) {
                        Ok(()) => {
                            let item = dialog
                                .GetResult()
                                .map_err(|error| format!("读取选择目录失败: {error}"))?;
                            let path = item
                                .GetDisplayName(SIGDN_FILESYSPATH)
                                .map_err(|error| format!("读取选择目录路径失败: {error}"))?;
                            Ok(Some(path.to_string().map_err(|error| {
                                format!("转换选择目录路径失败: {error}")
                            })?))
                        }
                        Err(error) if error.code().0 as u32 == 0x800704C7 => Ok(None),
                        Err(error) => Err(format!("打开目录选择器失败: {error}")),
                    };
                    CoUninitialize();
                    picked
                })
        };
        let _ = sender.send(result);
    });

    receiver
        .recv()
        .map_err(|error| format!("等待目录选择器失败: {error}"))?
}

#[tauri::command]
fn pick_storage_path(state: State<'_, AppState>, storage_path: Option<String>) -> Result<Option<String>, String> {
    let initial_path = resolve_storage_path(&state, storage_path)?;
    if std::env::var_os("LWT_USE_POWERSHELL_FOLDER_PICKER").is_none() {
        return show_folder_picker(initial_path.clone());
    }
    let script = r#"
Add-Type -AssemblyName System.Windows.Forms
$dialog = New-Object System.Windows.Forms.FolderBrowserDialog
$dialog.Description = '选择存储路径'
$dialog.ShowNewFolderButton = $true
$initial = $env:LWT_INITIAL_DIR
if ($initial -and [System.IO.Directory]::Exists($initial)) {
  $dialog.SelectedPath = $initial
}
if ($dialog.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {
  [Console]::OutputEncoding = New-Object System.Text.UTF8Encoding $false
  Write-Output $dialog.SelectedPath
}
"#;
    let output = Command::new("powershell")
        .args(["-NoProfile", "-STA", "-Command", script])
        .env("LWT_INITIAL_DIR", initial_path)
        .output()
        .map_err(|error| format!("打开目录选择器失败: {error}"))?;
    if !output.status.success() {
        return Err("打开目录选择器失败".to_owned());
    }
    let selected = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if selected.is_empty() {
        Ok(None)
    } else {
        Ok(Some(selected))
    }
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
        settings.storage_path = storage_path;
    }
    AppSettings::save(&state.settings_path, registry.settings())?;
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
            if registry.settings().auto_start {
                let _ = set_auto_start_registry(true);
            }
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
        .invoke_handler(tauri::generate_handler![
            get_app_snapshot,
            set_tool_enabled,
            set_auto_start_enabled,
            get_default_storage_path,
            open_storage_path,
            pick_storage_path,
            update_app_settings
        ])
        .run(tauri::generate_context!())
        .expect("启动 LightweightToolset 失败");
}
