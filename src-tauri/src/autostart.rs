//! 开机自启（Windows 计划任务实现）。
//!
//! 主程序清单固定为 `requireAdministrator`（`src-tauri/windows-app-manifest.xml`），
//! Explorer 处理 `HKCU\...\Run` 里的条目时是直接 CreateProcess，提权程序会以
//! `ERROR_ELEVATION_REQUIRED` 失败并被静默丢弃（事件日志 Shell-Core 9708 / UAC Operational）。
//! 因此 Run 键方式在本清单下永远无法在登录时拉起程序。
//!
//! 这里改用「登录触发 + 最高权限」的计划任务：由任务计划程序以完整令牌直接启动，不弹 UAC；
//! 同时清理旧版本遗留的 Run 键条目，避免每次登录重复产生失败记录。

#[cfg(any(not(debug_assertions), test))]
use std::path::Path;

#[cfg(any(not(debug_assertions), test))]
const TASK_NAME: &str = "LightweightToolset";
/// 旧版本（<= v0.6.0）通过 tauri-plugin-autostart 写入的 Run 值名。
#[cfg(not(debug_assertions))]
const LEGACY_RUN_VALUE: &str = "LightweightToolset";
#[cfg(all(windows, not(debug_assertions)))]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// 开发版不注册、也不改动系统自启（避免和安装版抢自启项）。
pub fn is_supported() -> bool {
    cfg!(not(debug_assertions))
}

/// 按设置应用开机自启。开发版是空操作，且不会改动安装版的自启配置。
pub fn apply(enabled: bool) -> Result<(), String> {
    #[cfg(debug_assertions)]
    {
        let _ = enabled;
        Ok(())
    }

    #[cfg(not(debug_assertions))]
    {
        let outcome = if enabled { enable_task() } else { disable_task() };
        remove_legacy_run_value();
        outcome
    }
}

#[cfg(not(debug_assertions))]
fn enable_task() -> Result<(), String> {
    let exe = std::env::current_exe().map_err(|error| format!("获取程序路径失败: {error}"))?;
    let user = current_user_name()?;
    let xml = task_xml(&exe, &user)?;
    let xml_path = std::env::temp_dir().join(format!(
        "lightweight-toolset-autostart-{}.xml",
        std::process::id()
    ));
    write_utf16_file(&xml_path, &xml)?;
    let xml_arg = xml_path.to_string_lossy().into_owned();
    let result = run_schtasks(&["/Create", "/TN", TASK_NAME, "/XML", &xml_arg, "/F"]);
    let _ = std::fs::remove_file(&xml_path);
    result
}

#[cfg(not(debug_assertions))]
fn disable_task() -> Result<(), String> {
    if !task_exists()? {
        return Ok(());
    }
    run_schtasks(&["/Delete", "/TN", TASK_NAME, "/F"])
}

#[cfg(not(debug_assertions))]
fn task_exists() -> Result<bool, String> {
    let output = schtasks_command(&["/Query", "/TN", TASK_NAME])
        .output()
        .map_err(|error| format!("调用计划任务命令失败: {error}"))?;
    Ok(output.status.success())
}

#[cfg(not(debug_assertions))]
fn run_schtasks(args: &[&str]) -> Result<(), String> {
    let output = schtasks_command(args)
        .output()
        .map_err(|error| format!("调用计划任务命令失败: {error}"))?;
    if output.status.success() {
        return Ok(());
    }
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    let detail = if stderr.is_empty() { stdout } else { stderr };
    Err(format!("开机自启注册失败: {detail}"))
}

#[cfg(not(debug_assertions))]
fn schtasks_command(args: &[&str]) -> std::process::Command {
    let mut command = std::process::Command::new("schtasks");
    command.args(args);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    command
}

#[cfg(not(debug_assertions))]
fn current_user_name() -> Result<String, String> {
    let user = std::env::var("USERNAME").map_err(|_| "无法确定当前用户名".to_owned())?;
    if user.trim().is_empty() {
        return Err("无法确定当前用户名".to_owned());
    }
    match std::env::var("USERDOMAIN") {
        Ok(domain) if !domain.trim().is_empty() => Ok(format!("{domain}\\{user}")),
        _ => Ok(user),
    }
}

/// 生成计划任务 XML：登录触发、交互式令牌、最高权限、无执行时限、电池供电也启动。
#[cfg(any(not(debug_assertions), test))]
fn task_xml(exe: &Path, user: &str) -> Result<String, String> {
    let exe_text = exe.to_str().ok_or("程序路径包含非 Unicode 字符")?;
    let workdir = exe
        .parent()
        .and_then(Path::to_str)
        .ok_or("程序目录路径包含非 Unicode 字符")?;
    let exe_text = escape_xml(exe_text);
    let workdir = escape_xml(workdir);
    let user = escape_xml(user);
    Ok(format!(
        r#"<?xml version="1.0" encoding="UTF-16"?>
<Task version="1.4" xmlns="http://schemas.microsoft.com/windows/2004/02/mit/task">
  <RegistrationInfo>
    <Description>轻量化工具集开机自启（以管理员身份运行，不弹 UAC）</Description>
    <URI>\{TASK_NAME}</URI>
  </RegistrationInfo>
  <Triggers>
    <LogonTrigger>
      <Enabled>true</Enabled>
      <UserId>{user}</UserId>
    </LogonTrigger>
  </Triggers>
  <Principals>
    <Principal id="Author">
      <UserId>{user}</UserId>
      <LogonType>InteractiveToken</LogonType>
      <RunLevel>HighestAvailable</RunLevel>
    </Principal>
  </Principals>
  <Settings>
    <MultipleInstancesPolicy>IgnoreNew</MultipleInstancesPolicy>
    <DisallowStartIfOnBatteries>false</DisallowStartIfOnBatteries>
    <StopIfGoingOnBatteries>false</StopIfGoingOnBatteries>
    <AllowHardTerminate>true</AllowHardTerminate>
    <StartWhenAvailable>true</StartWhenAvailable>
    <RunOnlyIfNetworkAvailable>false</RunOnlyIfNetworkAvailable>
    <AllowStartOnDemand>true</AllowStartOnDemand>
    <Enabled>true</Enabled>
    <Hidden>false</Hidden>
    <RunOnlyIfIdle>false</RunOnlyIfIdle>
    <WakeToRun>false</WakeToRun>
    <ExecutionTimeLimit>PT0S</ExecutionTimeLimit>
    <Priority>7</Priority>
  </Settings>
  <Actions Context="Author">
    <Exec>
      <Command>{exe_text}</Command>
      <WorkingDirectory>{workdir}</WorkingDirectory>
    </Exec>
  </Actions>
</Task>
"#
    ))
}

#[cfg(any(not(debug_assertions), test))]
fn escape_xml(value: &str) -> String {
    value
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(any(not(debug_assertions), test))]
fn write_utf16_file(path: &Path, content: &str) -> Result<(), String> {
    let mut bytes = Vec::with_capacity(content.len() * 2 + 2);
    bytes.extend_from_slice(&[0xFF, 0xFE]);
    for unit in content.encode_utf16() {
        bytes.extend_from_slice(&unit.to_le_bytes());
    }
    std::fs::write(path, bytes).map_err(|error| format!("写入计划任务描述失败: {error}"))
}

/// 删除旧版本写入的 Run 键值；不存在时静默跳过。仅在正式版调用，开发版不碰安装版配置。
#[cfg(all(windows, not(debug_assertions)))]
fn remove_legacy_run_value() {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegDeleteValueW, RegOpenKeyExW, HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE,
    };

    let subkey: Vec<u16> = std::ffi::OsStr::new("Software\\Microsoft\\Windows\\CurrentVersion\\Run")
        .encode_wide()
        .chain(Some(0))
        .collect();
    let value: Vec<u16> = std::ffi::OsStr::new(LEGACY_RUN_VALUE)
        .encode_wide()
        .chain(Some(0))
        .collect();
    let mut key: HKEY = std::ptr::null_mut();
    unsafe {
        if RegOpenKeyExW(HKEY_CURRENT_USER, subkey.as_ptr(), 0, KEY_SET_VALUE, &mut key) != 0 {
            return;
        }
        let _ = RegDeleteValueW(key, value.as_ptr());
        let _ = RegCloseKey(key);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_xml_uses_logon_trigger_and_highest_privileges() {
        let xml = task_xml(
            Path::new("D:\\轻量化工具集\\lightweight-toolset.exe"),
            "THE2580\\THE",
        )
        .expect("xml");
        assert!(xml.contains("<LogonTrigger>"));
        assert!(xml.contains("<RunLevel>HighestAvailable</RunLevel>"));
        assert!(xml.contains("<LogonType>InteractiveToken</LogonType>"));
        assert!(xml.contains("<UserId>THE2580\\THE</UserId>"));
        assert!(xml.contains("<Command>D:\\轻量化工具集\\lightweight-toolset.exe</Command>"));
        assert!(xml.contains("<WorkingDirectory>D:\\轻量化工具集</WorkingDirectory>"));
        // 默认 72 小时执行时限会让常驻程序被计划任务杀掉，必须显式取消。
        assert!(xml.contains("<ExecutionTimeLimit>PT0S</ExecutionTimeLimit>"));
    }

    #[test]
    fn task_xml_escapes_special_characters() {
        let xml = task_xml(Path::new("C:\\a&b\\c<d>.exe"), "DOM\\user").expect("xml");
        assert!(xml.contains("<Command>C:\\a&amp;b\\c&lt;d&gt;.exe</Command>"));
    }

    #[test]
    fn debug_build_never_supports_autostart() {
        assert!(!is_supported());
    }
}
