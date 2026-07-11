use std::{
    fs,
    path::{Path, PathBuf},
    sync::{Mutex, OnceLock},
};

use serde::{Deserialize, Serialize};
use windows_sys::Win32::{
    Foundation::{GetLastError, HWND},
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
    Ok(())
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
