pub const APP_ID: &str = "com.the.lightweight-toolset";
const APP_NAME: &str = "轻量化工具集";
const LEGACY_APP_NAME: &str = "LightweightToolset";

#[cfg(target_os = "windows")]
pub fn prepare_app_identity() {
    set_process_app_id();
    let _ = ensure_start_menu_shortcut();
}

#[cfg(not(target_os = "windows"))]
pub fn prepare_app_identity() {}

pub fn notify_timer_finished(name: &str) {
    #[cfg(target_os = "windows")]
    {
        let _ = notify_rust::Notification::new()
            .app_id(APP_ID)
            .summary("计时器提醒")
            .body(&format!("{name}\n倒计时已结束"))
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
            .app_id(APP_ID)
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
fn set_process_app_id() {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::Shell::SetCurrentProcessExplicitAppUserModelID;

    let app_id: Vec<u16> = std::ffi::OsStr::new(APP_ID)
        .encode_wide()
        .chain(Some(0))
        .collect();
    unsafe {
        let _ = SetCurrentProcessExplicitAppUserModelID(app_id.as_ptr());
    }
}

#[cfg(target_os = "windows")]
fn ensure_start_menu_shortcut() -> Result<(), String> {
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
    let legacy_shortcut_path = programs_dir.join(format!("{LEGACY_APP_NAME}.lnk"));
    if legacy_shortcut_path.exists() {
        let _ = fs::remove_file(&legacy_shortcut_path);
    }
    let shortcut_path = programs_dir.join(format!("{APP_NAME}.lnk"));

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
        let mut app_id = InitPropVariantFromStringAsVector(&HSTRING::from(APP_ID))
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
