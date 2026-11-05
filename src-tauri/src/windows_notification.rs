use std::sync::OnceLock;

const APP_NAME: &str = "轻量化工具集";
/// 开发版使用独立的开始菜单快捷方式名，避免覆盖安装版的同名快捷方式。
const DEV_APP_NAME: &str = "轻量化工具集 (开发版)";
/// 旧版本使用的快捷方式名；仅正式版负责清理。
#[cfg(not(debug_assertions))]
const LEGACY_APP_NAME: &str = "LightweightToolset";
/// 启动早期（setup 之前）的兜底 AUMID；setup 阶段会用实际 identifier 覆盖（开发版为 `...-dev`）。
const FALLBACK_APP_ID: &str = "com.the.lightweight-toolset";

static APP_ID: OnceLock<String> = OnceLock::new();

fn app_id() -> &'static str {
    APP_ID.get().map(String::as_str).unwrap_or(FALLBACK_APP_ID)
}

/// 初始化通知身份：AUMID 与开始菜单快捷方式都跟随应用 identifier，
/// 开发版因此使用独立 AUMID 与独立快捷方式，不与安装版互相覆盖。
#[cfg(target_os = "windows")]
pub fn prepare_app_identity(identifier: &str) {
    let _ = APP_ID.set(identifier.to_owned());
    set_process_app_id(identifier);
    let shortcut_name = if cfg!(debug_assertions) {
        DEV_APP_NAME
    } else {
        APP_NAME
    };
    let _ = ensure_start_menu_shortcut(identifier, shortcut_name);
}

#[cfg(not(target_os = "windows"))]
pub fn prepare_app_identity(_identifier: &str) {}

pub fn notify_silent_start_enabled() {
    #[cfg(target_os = "windows")]
    {
        let _ = notify_rust::Notification::new()
            .app_id(app_id())
            .summary("静默启动已开启")
            .body("下次启动只显示托盘图标；可从托盘、主窗口快捷键或再次双击图标打开主窗口。")
            .sound_name("Default")
            .auto_icon()
            .show();
    }
}

/// 静默启动真正发生时的提示：告诉用户程序已在后台运行，以及三种呼出方式。
pub fn notify_silent_start(hotkey: Option<&str>) {
    #[cfg(target_os = "windows")]
    {
        let body = silent_start_body(hotkey);
        let _ = notify_rust::Notification::new()
            .app_id(app_id())
            .summary("轻量化工具集已在后台启动")
            .body(&body)
            .sound_name("Default")
            .auto_icon()
            .show();
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = hotkey;
    }
}

/// 静默启动通知正文；`hotkey` 为当前实际生效的「呼出主窗口」快捷键，注册失败时传 None。
pub fn silent_start_body(hotkey: Option<&str>) -> String {
    match hotkey {
        Some(hotkey) if !hotkey.is_empty() => format!(
            "窗口已隐藏到托盘。点托盘图标、再次打开程序，或按 {hotkey} 即可显示主窗口。"
        ),
        _ => "窗口已隐藏到托盘。点托盘图标或再次打开程序即可显示主窗口。".to_owned(),
    }
}

pub fn notify_timer_finished(name: &str) {
    #[cfg(target_os = "windows")]
    {
        let _ = notify_rust::Notification::new()
            .app_id(app_id())
            .summary("计时器提醒")
            .body(&format!("{name}\\n倒计时已结束"))
            .sound_name("Default")
            .auto_icon()
            .show();
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = name;
    }
}

pub fn notify_window_pinner(message: &str) {
    #[cfg(target_os = "windows")]
    {
        let _ = notify_rust::Notification::new()
            .app_id(app_id())
            .summary("窗口置顶")
            .body(message)
            .sound_name("Default")
            .auto_icon()
            .show();
    }

    #[cfg(not(target_os = "windows"))]
    {
        let _ = message;
    }
}

#[cfg(target_os = "windows")]
fn set_process_app_id(identifier: &str) {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;

    let app_id: Vec<u16> = std::ffi::OsStr::new(identifier)
        .encode_wide()
        .chain(Some(0))
        .collect();
    unsafe {
        let _ = SetCurrentProcessExplicitAppUserModelID(app_id.as_ptr());
    }
}

#[cfg(target_os = "windows")]
fn ensure_start_menu_shortcut(identifier: &str, shortcut_name: &str) -> Result<(), String> {
    use std::{env, fs};
    use windows::{
        core::{Interface, HSTRING},
        Win32::{
            System::Com::StructuredStorage::{InitPropVariantFromStringAsVector, PropVariantClear},
            System::Com::{
                CoCreateInstance, CoInitializeEx, IPersistFile, CLSCTX_INPROC_SERVER,
                COINIT_APARTMENTTHREADED,
            },
            UI::Shell::{IShellLinkW, PropertiesSystem::IPropertyStore, ShellLink},
        },
    };

    let exe = env::current_exe().map_err(|error| error.to_string())?;
    let programs_dir = env::var_os("APPDATA")
        .map(std::path::PathBuf::from)
        .ok_or("APPDATA is not available")?
        .join("Microsoft")
        .join("Windows")
        .join("Start Menu")
        .join("Programs");
    fs::create_dir_all(&programs_dir).map_err(|error| error.to_string())?;

    // 旧版本的快捷方式名只在正式版清理；开发版不能改动安装版的通知身份。
    #[cfg(not(debug_assertions))]
    {
        let legacy_shortcut_path = programs_dir.join(format!("{LEGACY_APP_NAME}.lnk"));
        if legacy_shortcut_path.exists() {
            let _ = fs::remove_file(&legacy_shortcut_path);
        }
    }

    let shortcut_path = programs_dir.join(format!("{shortcut_name}.lnk"));

    unsafe {
        let _ = CoInitializeEx(None, COINIT_APARTMENTTHREADED);
        let shell_link: IShellLinkW = CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)
            .map_err(|error| error.to_string())?;
        shell_link
            .SetPath(&HSTRING::from(exe.display().to_string()))
            .map_err(|error| error.to_string())?;
        shell_link
            .SetIconLocation(&HSTRING::from(exe.display().to_string()), 0)
            .map_err(|error| error.to_string())?;

        let property_store: IPropertyStore =
            shell_link.cast().map_err(|error| error.to_string())?;
        let mut app_id = InitPropVariantFromStringAsVector(&HSTRING::from(identifier))
            .map_err(|error| error.to_string())?;
        property_store
            .SetValue(&PKEY_APP_USER_MODEL_ID, &app_id)
            .map_err(|error| error.to_string())?;
        property_store.Commit().map_err(|error| error.to_string())?;
        let _ = PropVariantClear(&mut app_id);

        let persist_file: IPersistFile = shell_link.cast().map_err(|error| error.to_string())?;
        persist_file
            .Save(&HSTRING::from(shortcut_path.display().to_string()), true)
            .map_err(|error| error.to_string())?;
    }

    Ok(())
}

#[cfg(target_os = "windows")]
const PKEY_APP_USER_MODEL_ID: windows::Win32::Foundation::PROPERTYKEY =
    windows::Win32::Foundation::PROPERTYKEY {
        fmtid: windows::core::GUID::from_u128(0x9f4c2855_9f79_4b39_a8d0_e1d42de1d5f3),
        pid: 5,
    };

#[cfg(test)]
mod tests {
    use super::silent_start_body;

    #[test]
    fn silent_start_body_mentions_actual_hotkey_when_available() {
        let body = silent_start_body(Some("CTRL+ALT+M"));
        assert!(body.contains("CTRL+ALT+M"));
        assert!(body.contains("托盘"));
        assert!(body.contains("再次打开程序"));
    }

    #[test]
    fn silent_start_body_omits_hotkey_hint_when_registration_failed() {
        let body = silent_start_body(None);
        assert!(!body.contains("按"));
        assert!(body.contains("托盘"));
    }
}
