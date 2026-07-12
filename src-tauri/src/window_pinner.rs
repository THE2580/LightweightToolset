use std::{
    env, fs,
    os::windows::process::CommandExt,
    path::{Path, PathBuf},
    process::Command,
    sync::{Mutex, OnceLock},
};

use serde::{Deserialize, Serialize};
use windows_sys::Win32::{
    Foundation::{CloseHandle, GetLastError, HWND, WAIT_FAILED, WAIT_OBJECT_0},
    System::Threading::{
        OpenProcess, WaitForSingleObject, CREATE_BREAKAWAY_FROM_JOB, CREATE_NEW_PROCESS_GROUP,
        CREATE_NO_WINDOW,
    },
    UI::WindowsAndMessaging::{
        GetForegroundWindow, GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW,
        GetWindowThreadProcessId, IsWindow, IsWindowVisible, SetWindowPos, GWL_EXSTYLE,
        HWND_NOTOPMOST, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, WS_EX_TOPMOST,
    },
};

#[derive(Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PinnedWindow {
    hwnd: isize,
    title: String,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowPinnerSnapshot {
    pub windows: Vec<PinnedWindow>,
    pub max_pins: usize,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct WindowPinnerAction {
    pub snapshot: WindowPinnerSnapshot,
    pub message: String,
}

struct State {
    windows: Vec<PinnedWindow>,
    max_pins: usize,
    path: Option<PathBuf>,
}

static STATE: OnceLock<Mutex<State>> = OnceLock::new();
const WATCHDOG_ARG: &str = "--window-pinner-watchdog";
const WATCHDOG_PREFIX: &str = "window_pinner_watchdog_";
const SYNCHRONIZE_ACCESS: u32 = 0x0010_0000;

fn state() -> &'static Mutex<State> {
    STATE.get_or_init(|| {
        Mutex::new(State {
            windows: Vec::new(),
            max_pins: 10,
            path: None,
        })
    })
}

pub fn init(storage_dir: &Path, max_pins: usize) -> Result<(), String> {
    if let Ok(mut state) = state().lock() {
        state.max_pins = normalize_max_pins(max_pins);
        state.path = Some(storage_dir.join("window_pinner_state.json"));
        state.windows = state
            .path
            .as_ref()
            .and_then(|path| fs::read_to_string(path).ok())
            .and_then(|raw| serde_json::from_str::<Vec<PinnedWindow>>(&raw).ok())
            .unwrap_or_default();
        state.windows.retain(|item| {
            let hwnd = item.hwnd as HWND;
            (unsafe { IsWindow(hwnd) }) != 0 && window_title(hwnd) == item.title
        });
        persist(&state)?;
    }
    Ok(())
}

pub fn start_watchdog(storage_dir: &Path) -> Result<(), String> {
    let parent_pid = std::process::id();
    let state_path = storage_dir.join("window_pinner_state.json");
    let current_exe =
        env::current_exe().map_err(|error| format!("定位窗口置顶监护程序失败: {error}"))?;
    let metadata = fs::metadata(&current_exe)
        .map_err(|error| format!("读取窗口置顶监护程序信息失败: {error}"))?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let helper_path = storage_dir.join(format!(
        "{WATCHDOG_PREFIX}{}_{}.exe",
        metadata.len(),
        modified
    ));
    cleanup_stale_watchdogs(storage_dir, Some(&helper_path));
    if !helper_path.exists() {
        fs::copy(&current_exe, &helper_path)
            .map_err(|error| format!("准备窗口置顶监护程序失败: {error}"))?;
    }

    let spawn = |flags| {
        let mut command = Command::new(&helper_path);
        command
            .arg(WATCHDOG_ARG)
            .arg(parent_pid.to_string())
            .arg(&state_path);
        command.creation_flags(flags).spawn()
    };
    let base_flags = CREATE_NO_WINDOW | CREATE_NEW_PROCESS_GROUP;
    spawn(base_flags | CREATE_BREAKAWAY_FROM_JOB)
        .or_else(|_| spawn(base_flags))
        .map(|_| ())
        .map_err(|error| format!("启动窗口置顶监护程序失败: {error}"))
}

pub fn run_watchdog_from_args() -> bool {
    let mut args = env::args_os();
    let _ = args.next();
    if args.next().as_deref() != Some(std::ffi::OsStr::new(WATCHDOG_ARG)) {
        return false;
    }
    let Some(parent_pid) = args
        .next()
        .and_then(|value| value.to_string_lossy().parse::<u32>().ok())
    else {
        return true;
    };
    let Some(state_path) = args.next().map(PathBuf::from) else {
        return true;
    };
    run_watchdog(parent_pid, &state_path);
    true
}

fn run_watchdog(parent_pid: u32, state_path: &Path) {
    let process = unsafe { OpenProcess(SYNCHRONIZE_ACCESS, 0, parent_pid) };
    if process.is_null() {
        cleanup_after_parent_exit(state_path, &[]);
        return;
    }
    let mut cached = Vec::new();
    loop {
        if let Some(windows) = read_responsibility_list(state_path) {
            cached = windows;
        }
        let wait = unsafe { WaitForSingleObject(process, 100) };
        if wait == WAIT_OBJECT_0 || wait == WAIT_FAILED {
            break;
        }
    }
    unsafe { CloseHandle(process) };
    cleanup_after_parent_exit(state_path, &cached);
}

fn cleanup_after_parent_exit(state_path: &Path, cached: &[PinnedWindow]) {
    if !state_path.exists() {
        return;
    }
    let windows = read_responsibility_list(state_path).unwrap_or_else(|| cached.to_vec());
    for item in windows {
        let hwnd = item.hwnd as HWND;
        if unsafe { IsWindow(hwnd) } != 0 && window_title(hwnd) == item.title {
            let _ = set_topmost(hwnd, false);
        }
    }
    let _ = fs::remove_file(state_path);
}

fn read_responsibility_list(path: &Path) -> Option<Vec<PinnedWindow>> {
    fs::read_to_string(path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
}

fn cleanup_stale_watchdogs(storage_dir: &Path, keep: Option<&Path>) {
    let Ok(entries) = fs::read_dir(storage_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_watchdog = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with(WATCHDOG_PREFIX) && name.ends_with(".exe"));
        if is_watchdog && keep != Some(path.as_path()) {
            let _ = fs::remove_file(path);
        }
    }
}

pub fn relocate(storage_dir: &Path) -> Result<(), String> {
    let mut state = state().lock().map_err(|_| "窗口置顶状态不可用")?;
    let old_path = state
        .path
        .replace(storage_dir.join("window_pinner_state.json"));
    persist(&state)?;
    if let Some(old_path) = old_path {
        if state.path.as_ref() != Some(&old_path) && old_path.exists() {
            let _ = fs::remove_file(old_path);
        }
    }
    drop(state);
    start_watchdog(storage_dir)
}

pub fn snapshot() -> Result<WindowPinnerSnapshot, String> {
    let mut state = state().lock().map_err(|_| "窗口置顶状态不可用")?;
    cleanup(&mut state);
    persist(&state)?;
    Ok(snapshot_of(&state))
}

pub fn set_max_pins(max_pins: usize) -> Result<WindowPinnerSnapshot, String> {
    let mut state = state().lock().map_err(|_| "窗口置顶状态不可用")?;
    let max_pins = normalize_max_pins(max_pins);
    if state.windows.len() > max_pins {
        return Err(format!(
            "当前已有 {} 个置顶窗口，请先取消部分窗口",
            state.windows.len()
        ));
    }
    state.max_pins = max_pins;
    Ok(snapshot_of(&state))
}

pub fn toggle_foreground(own_process_id: u32) -> Result<WindowPinnerAction, String> {
    let hwnd = unsafe { GetForegroundWindow() };
    validate_target(hwnd, own_process_id)?;
    let mut state = state().lock().map_err(|_| "窗口置顶状态不可用")?;
    cleanup(&mut state);
    let hwnd_value = hwnd as isize;
    if let Some(index) = state
        .windows
        .iter()
        .position(|item| item.hwnd == hwnd_value)
    {
        let item = state.windows.remove(index);
        set_topmost(hwnd, false)?;
        persist(&state)?;
        return Ok(WindowPinnerAction {
            message: format!("已取消置顶：{}", item.title),
            snapshot: snapshot_of(&state),
        });
    }
    if unsafe { GetWindowLongPtrW(hwnd, GWL_EXSTYLE) } as u32 & WS_EX_TOPMOST != 0 {
        let title = window_title(hwnd);
        set_topmost(hwnd, false)?;
        return Ok(WindowPinnerAction {
            message: format!("已取消遗留置顶：{title}"),
            snapshot: snapshot_of(&state),
        });
    }
    if state.windows.len() >= state.max_pins {
        return Err(format!("已达到最大置顶数量 {}", state.max_pins));
    }
    let title = window_title(hwnd);
    set_topmost(hwnd, true)?;
    state.windows.push(PinnedWindow {
        hwnd: hwnd_value,
        title: title.clone(),
    });
    persist(&state)?;
    Ok(WindowPinnerAction {
        message: format!("已置顶：{title}"),
        snapshot: snapshot_of(&state),
    })
}

pub fn unpin(hwnd: isize) -> Result<WindowPinnerSnapshot, String> {
    let mut state = state().lock().map_err(|_| "窗口置顶状态不可用")?;
    if let Some(index) = state.windows.iter().position(|item| item.hwnd == hwnd) {
        let item = state.windows.remove(index);
        let native_hwnd = hwnd as HWND;
        if unsafe { IsWindow(native_hwnd) } != 0 {
            set_topmost(native_hwnd, false).map_err(|error| {
                state.windows.insert(index, item);
                error
            })?;
        }
    }
    cleanup(&mut state);
    persist(&state)?;
    Ok(snapshot_of(&state))
}

pub fn unpin_all() -> WindowPinnerSnapshot {
    let mut state = state().lock().unwrap_or_else(|error| error.into_inner());
    for item in state.windows.drain(..) {
        let hwnd = item.hwnd as HWND;
        if unsafe { IsWindow(hwnd) } != 0 {
            let _ = set_topmost(hwnd, false);
        }
    }
    let _ = persist(&state);
    snapshot_of(&state)
}

fn validate_target(hwnd: HWND, own_process_id: u32) -> Result<(), String> {
    if hwnd.is_null() || unsafe { IsWindow(hwnd) } == 0 || unsafe { IsWindowVisible(hwnd) } == 0 {
        return Err("当前没有可置顶的窗口".to_owned());
    }
    let mut process_id = 0;
    unsafe { GetWindowThreadProcessId(hwnd, &mut process_id) };
    if process_id == own_process_id {
        return Err("本软件窗口请在软件设置中控制置顶".to_owned());
    }
    Ok(())
}

fn set_topmost(hwnd: HWND, enabled: bool) -> Result<(), String> {
    let after = if enabled {
        HWND_TOPMOST
    } else {
        HWND_NOTOPMOST
    };
    if unsafe {
        SetWindowPos(
            hwnd,
            after,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        )
    } == 0
    {
        return Err(format!("更新窗口置顶失败（Win32 {}）", unsafe {
            GetLastError()
        }));
    }
    Ok(())
}

fn cleanup(state: &mut State) {
    state
        .windows
        .retain(|item| unsafe { IsWindow(item.hwnd as HWND) } != 0);
}
fn normalize_max_pins(value: usize) -> usize {
    [1usize, 5, 10, 15, 20]
        .into_iter()
        .min_by_key(|candidate| (*candidate).abs_diff(value))
        .unwrap_or(10)
}
fn persist(state: &State) -> Result<(), String> {
    let Some(path) = &state.path else {
        return Ok(());
    };
    if state.windows.is_empty() {
        if path.exists() {
            fs::remove_file(path).map_err(|error| format!("清理窗口置顶状态失败: {error}"))?;
        }
        return Ok(());
    }
    let raw = serde_json::to_string(&state.windows)
        .map_err(|error| format!("序列化窗口置顶状态失败: {error}"))?;
    fs::write(path, raw).map_err(|error| format!("保存窗口置顶状态失败: {error}"))
}
fn snapshot_of(state: &State) -> WindowPinnerSnapshot {
    WindowPinnerSnapshot {
        windows: state.windows.clone(),
        max_pins: state.max_pins,
    }
}
fn window_title(hwnd: HWND) -> String {
    let length = unsafe { GetWindowTextLengthW(hwnd) };
    if length <= 0 {
        return format!("窗口 0x{:X}", hwnd as usize);
    }
    let mut buffer = vec![0u16; length as usize + 1];
    let read = unsafe { GetWindowTextW(hwnd, buffer.as_mut_ptr(), buffer.len() as i32) };
    let title = String::from_utf16_lossy(&buffer[..read.max(0) as usize]);
    if title.trim().is_empty() {
        format!("窗口 0x{:X}", hwnd as usize)
    } else {
        title
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_dir(name: &str) -> PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        env::temp_dir().join(format!("lightweight-toolset-{name}-{unique}"))
    }

    #[test]
    fn watchdog_treats_missing_state_as_normal_cleanup() {
        let directory = test_dir("watchdog-normal-exit");
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("window_pinner_state.json");

        cleanup_after_parent_exit(
            &path,
            &[PinnedWindow {
                hwnd: 1,
                title: "cached".to_owned(),
            }],
        );

        assert!(!path.exists());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn watchdog_removes_empty_responsibility_file_after_abnormal_exit() {
        let directory = test_dir("watchdog-abnormal-exit");
        fs::create_dir_all(&directory).unwrap();
        let path = directory.join("window_pinner_state.json");
        fs::write(&path, "[]").unwrap();

        cleanup_after_parent_exit(&path, &[]);

        assert!(!path.exists());
        fs::remove_dir_all(directory).unwrap();
    }

    #[test]
    fn startup_removes_stale_watchdog_copies_only() {
        let directory = test_dir("watchdog-stale-copy");
        fs::create_dir_all(&directory).unwrap();
        let stale = directory.join("window_pinner_watchdog_123.exe");
        let current = directory.join("window_pinner_watchdog_current.exe");
        let unrelated = directory.join("keep.exe");
        fs::write(&stale, b"stale").unwrap();
        fs::write(&current, b"current").unwrap();
        fs::write(&unrelated, b"keep").unwrap();

        cleanup_stale_watchdogs(&directory, Some(&current));

        assert!(!stale.exists());
        assert!(current.exists());
        assert!(unrelated.exists());
        fs::remove_dir_all(directory).unwrap();
    }
}
