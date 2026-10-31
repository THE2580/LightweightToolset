use std::{
    collections::HashSet,
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Mutex, OnceLock,
    },
    thread,
    time::{Duration, Instant},
};

use serde::Serialize;
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, Position, Size, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder,
};

use crate::{clipboard, AppState};

const NOTIFY_LABEL: &str = "tool-clipboard-notify";
const NOTIFY_WIDTH: f64 = 168.0;
const NOTIFY_HEIGHT: f64 = 50.0;
const NOTIFY_GAP: f64 = 16.0;
const DISPLAY_MS: u64 = 2000;
const SLIDE_MS: u64 = 300;
const CLIPBOARD_CONFIRM_MS: u128 = 800;
const ZONE_RATIO: f64 = 0.15;

#[derive(Clone, Copy, PartialEq, Eq)]
enum TipKind {
    Copied,
    Cut,
    Pasted,
}

impl TipKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Copied => "copied",
            Self::Cut => "cut",
            Self::Pasted => "pasted",
        }
    }
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TipPayload {
    kind: &'static str,
    count: u32,
    side: &'static str,
}

enum Command {
    Show(TipKind),
    Reset,
}

static APP: OnceLock<AppHandle> = OnceLock::new();
static TX: OnceLock<mpsc::Sender<Command>> = OnceLock::new();
static PENDING: Mutex<Option<(TipKind, Instant)>> = Mutex::new(None);
static READY: AtomicBool = AtomicBool::new(false);
static WORKER_STARTED: AtomicBool = AtomicBool::new(false);
static LISTENER: Mutex<Option<CopyListener>> = Mutex::new(None);

pub fn init(app: &AppHandle) {
    let _ = APP.set(app.clone());
    start_worker();
}

pub fn notify_copied() {
    set_pending(TipKind::Copied);
}

pub fn notify_cut() {
    set_pending(TipKind::Cut);
}

pub fn notify_pasted() {
    send(Command::Show(TipKind::Pasted));
}

fn set_pending(kind: TipKind) {
    if let Ok(mut pending) = PENDING.lock() {
        *pending = Some((kind, Instant::now()));
    }
}

fn send(command: Command) {
    if let Some(tx) = TX.get() {
        let _ = tx.send(command);
    }
}

/// 剪贴板真正更新时调用：处于复制/剪切意图窗口内才提示，避免终端 Ctrl+C 等误报。
pub fn on_clipboard_changed() {
    let Ok(mut pending) = PENDING.lock() else {
        return;
    };
    let Some((kind, started_at)) = *pending else {
        return;
    };
    *pending = None;
    if started_at.elapsed().as_millis() <= CLIPBOARD_CONFIRM_MS {
        send(Command::Show(kind));
    }
}

fn notifications_enabled(app: &AppHandle) -> bool {
    let Some(state) = app.try_state::<AppState>() else {
        return false;
    };
    let tool_enabled = state
        .registry
        .lock()
        .map(|registry| registry.is_enabled("clipboard"))
        .unwrap_or(false);
    tool_enabled && clipboard::notifications_enabled()
}

pub fn cancel_all() {
    if let Ok(mut pending) = PENDING.lock() {
        *pending = None;
    }
    send(Command::Reset);
}

pub fn refresh() {
    match APP.get() {
        Some(app) if notifications_enabled(app) => {}
        _ => cancel_all(),
    }
}

pub fn mark_ready() {
    READY.store(true, Ordering::SeqCst);
}

fn wait_ready(timeout: Duration) -> bool {
    let started = Instant::now();
    while started.elapsed() < timeout {
        if READY.load(Ordering::SeqCst) {
            return true;
        }
        thread::sleep(Duration::from_millis(20));
    }
    READY.load(Ordering::SeqCst)
}

fn start_worker() {
    if WORKER_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let (tx, rx) = mpsc::channel();
    let _ = TX.set(tx);
    let _ = thread::Builder::new()
        .name("tool-clipboard-notify".to_owned())
        .spawn(move || {
            let mut visible = false;
            let mut hiding = false;
            let mut deadline: Option<Instant> = None;
            let mut last_kind: Option<TipKind> = None;
            let mut count = 0u32;
            loop {
                let wait = deadline
                    .map(|at| at.saturating_duration_since(Instant::now()).max(Duration::from_millis(5)))
                    .unwrap_or(Duration::from_secs(3600));
                match rx.recv_timeout(wait) {
                    Ok(Command::Reset) => {
                        if let Some(app) = APP.get() {
                            if let Some(window) = app.get_webview_window(NOTIFY_LABEL) {
                                let _ = window.hide();
                            }
                        }
                        visible = false;
                        hiding = false;
                        deadline = None;
                        count = 0;
                        last_kind = None;
                    }
                    Ok(Command::Show(kind)) => {
                        let Some(app) = APP.get().cloned() else {
                            continue;
                        };
                        if !notifications_enabled(&app) {
                            continue;
                        }
                        let Some(window) = ensure_window(&app) else {
                            continue;
                        };
                        if !visible {
                            if !wait_ready(Duration::from_millis(1500)) {
                                let _ = window.hide();
                                continue;
                            }
                            count = 1;
                            last_kind = Some(kind);
                        } else if last_kind == Some(kind) {
                            count = count.saturating_add(1);
                        } else {
                            count = 1;
                            last_kind = Some(kind);
                        }
                        let side = position_window(&window, &app, tip_width(count));
                        if !visible {
                            let _ = window.show();
                            visible = true;
                        }
                        hiding = false;
                        let _ = window.emit(
                            "clipboard-notify-show",
                            TipPayload { kind: kind.as_str(), count, side },
                        );
                        deadline = Some(Instant::now() + Duration::from_millis(DISPLAY_MS));
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        let Some(at) = deadline else {
                            continue;
                        };
                        if Instant::now() < at {
                            continue;
                        }
                        if !hiding {
                            if let Some(app) = APP.get() {
                                if let Some(window) = app.get_webview_window(NOTIFY_LABEL) {
                                    let _ = window.emit("clipboard-notify-hide", ());
                                }
                            }
                            hiding = true;
                            deadline = Some(Instant::now() + Duration::from_millis(SLIDE_MS));
                        } else {
                            if let Some(app) = APP.get() {
                                if let Some(window) = app.get_webview_window(NOTIFY_LABEL) {
                                    let _ = window.hide();
                                }
                            }
                            visible = false;
                            hiding = false;
                            deadline = None;
                            count = 0;
                            last_kind = None;
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
        });
}

fn ensure_window(app: &AppHandle) -> Option<WebviewWindow> {
    if let Some(window) = app.get_webview_window(NOTIFY_LABEL) {
        return Some(window);
    }
    READY.store(false, Ordering::SeqCst);
    let (tx, rx) = mpsc::channel();
    let app_for_main = app.clone();
    if app
        .run_on_main_thread(move || {
            let _ = tx.send(build_window(&app_for_main));
        })
        .is_err()
    {
        return None;
    }
    rx.recv_timeout(Duration::from_secs(3)).ok().flatten()
}

fn build_window(app: &AppHandle) -> Option<WebviewWindow> {
    let window = WebviewWindowBuilder::new(
        app,
        NOTIFY_LABEL,
        WebviewUrl::App("notification.html".into()),
    )
    .title("Clipboard tip")
    .inner_size(NOTIFY_WIDTH, NOTIFY_HEIGHT)
    .decorations(false)
    .transparent(true)
    .resizable(false)
    .maximizable(false)
    .minimizable(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .focused(false)
    .shadow(false)
    .visible(false)
    .build()
    .ok()?;
    let _ = window.set_ignore_cursor_events(true);
    Some(window)
}

fn tip_width(count: u32) -> f64 {
    let digits = count.to_string().len().max(1);
    NOTIFY_WIDTH + (digits.saturating_sub(1) as f64) * 10.0
}

fn position_window(window: &WebviewWindow, app: &AppHandle, logical_width: f64) -> &'static str {
    let Ok(Some(monitor)) = app.primary_monitor() else {
        return "bottom";
    };
    let scale = monitor.scale_factor();
    let work_area = monitor.work_area();
    let width = (logical_width * scale).round() as u32;
    let height = (NOTIFY_HEIGHT * scale).round() as u32;
    let gap = (NOTIFY_GAP * scale).round() as i32;
    let x = work_area.position.x + (work_area.size.width as i32 - width as i32) / 2;
    let y = work_area.position.y + work_area.size.height as i32 - height as i32 - gap;
    let _ = window.set_size(Size::Physical(PhysicalSize { width, height }));
    let _ = window.set_position(Position::Physical(PhysicalPosition { x, y }));

    let screen = monitor.size();
    let center_x = x as f64 + width as f64 / 2.0;
    let center_y = y as f64 + height as f64 / 2.0;
    let zone_w = screen.width as f64 * ZONE_RATIO;
    let zone_h = screen.height as f64 * ZONE_RATIO;
    if center_x < zone_w {
        "left"
    } else if center_x > screen.width as f64 - zone_w {
        "right"
    } else if center_y < zone_h {
        "top"
    } else if center_y > screen.height as f64 - zone_h {
        "bottom"
    } else {
        "bottom"
    }
}

struct CopyListener {
    thread_id: u32,
    thread: thread::JoinHandle<()>,
}

pub fn start_copy_listener() {
    let Ok(mut guard) = LISTENER.lock() else {
        return;
    };
    if guard.is_some() {
        return;
    }
    let (ready_tx, ready_rx) = mpsc::channel();
    let thread = thread::Builder::new()
        .name("tool-clipboard-notify-input".to_owned())
        .spawn(move || unsafe {
            install_input_listeners(ready_tx);
        });
    let Ok(thread) = thread else {
        return;
    };
    match ready_rx.recv_timeout(Duration::from_secs(3)) {
        Ok(Some(thread_id)) => {
            *guard = Some(CopyListener { thread_id, thread });
        }
        _ => {
            let _ = thread.join();
        }
    }
}

pub fn stop_copy_listener() {
    let Ok(mut guard) = LISTENER.lock() else {
        return;
    };
    if let Some(listener) = guard.take() {
        post_quit(listener.thread_id);
        let _ = listener.thread.join();
    }
}

#[cfg(target_os = "windows")]
fn post_quit(thread_id: u32) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT};
    unsafe {
        PostThreadMessageW(thread_id, WM_QUIT, 0, 0);
    }
}

#[cfg(not(target_os = "windows"))]
fn post_quit(_thread_id: u32) {}

#[cfg(target_os = "windows")]
unsafe fn install_input_listeners(ready_tx: mpsc::Sender<Option<u32>>) {
    use windows_sys::Win32::{
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            CreateWindowExW, DispatchMessageW, GetMessageW, RegisterClassW, SetWindowsHookExW,
            TranslateMessage, UnhookWindowsHookEx, DestroyWindow, HWND_MESSAGE, MSG,
            WH_KEYBOARD_LL, WNDCLASSW,
        },
    };
    use windows_sys::Win32::System::DataExchange::{
        AddClipboardFormatListener, RemoveClipboardFormatListener,
    };

    let module = GetModuleHandleW(std::ptr::null());
    let class_name: Vec<u16> = "LightweightToolsetClipboardTip\0".encode_utf16().collect();
    let mut class = std::mem::zeroed::<WNDCLASSW>();
    class.lpfnWndProc = Some(tip_window_proc);
    class.hInstance = module;
    class.lpszClassName = class_name.as_ptr();
    RegisterClassW(&class);

    let window = CreateWindowExW(
        0,
        class_name.as_ptr(),
        std::ptr::null(),
        0,
        0,
        0,
        0,
        0,
        HWND_MESSAGE,
        std::ptr::null_mut(),
        module,
        std::ptr::null_mut(),
    );
    if !window.is_null() {
        AddClipboardFormatListener(window);
    }

    let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(low_level_keyboard_hook), module, 0);
    let thread_id = windows_sys::Win32::System::Threading::GetCurrentThreadId();
    let _ = ready_tx.send(Some(thread_id));

    let mut message = std::mem::zeroed::<MSG>();
    while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
        TranslateMessage(&message);
        DispatchMessageW(&message);
    }

    if !hook.is_null() {
        UnhookWindowsHookEx(hook);
    }
    if !window.is_null() {
        RemoveClipboardFormatListener(window);
        DestroyWindow(window);
    }
}

#[cfg(not(target_os = "windows"))]
unsafe fn install_input_listeners(ready_tx: mpsc::Sender<Option<u32>>) {
    let _ = ready_tx.send(None);
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn tip_window_proc(
    window: windows_sys::Win32::Foundation::HWND,
    message: u32,
    w_param: windows_sys::Win32::Foundation::WPARAM,
    l_param: windows_sys::Win32::Foundation::LPARAM,
) -> windows_sys::Win32::Foundation::LRESULT {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        DefWindowProcW, WM_CLIPBOARDUPDATE,
    };
    if message == WM_CLIPBOARDUPDATE {
        on_clipboard_changed();
    }
    DefWindowProcW(window, message, w_param, l_param)
}

#[cfg(target_os = "windows")]
fn allow_shortcut(vk: u32, first_press: bool) -> bool {
    use std::collections::HashMap;
    static REPEAT_AT: OnceLock<Mutex<HashMap<u32, Instant>>> = OnceLock::new();
    let Ok(mut times) = REPEAT_AT
        .get_or_init(|| Mutex::new(HashMap::new()))
        .lock()
    else {
        return false;
    };
    let now = Instant::now();
    if !first_press {
        if let Some(last) = times.get(&vk) {
            if now.duration_since(*last) < Duration::from_millis(500) {
                return false;
            }
        }
    }
    times.insert(vk, now);
    true
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn low_level_keyboard_hook(
    code: i32,
    w_param: windows_sys::Win32::Foundation::WPARAM,
    l_param: windows_sys::Win32::Foundation::LPARAM,
) -> windows_sys::Win32::Foundation::LRESULT {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CallNextHookEx, KBDLLHOOKSTRUCT, LLKHF_INJECTED, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN,
        WM_SYSKEYUP,
    };
    static PRESSED: OnceLock<Mutex<HashSet<u32>>> = OnceLock::new();
    if code >= 0 && l_param != 0 {
        let input = &*(l_param as *const KBDLLHOOKSTRUCT);
        if input.flags & LLKHF_INJECTED == 0 {
            let message = w_param as u32;
            let vk = input.vkCode;
            if let Ok(mut pressed) = PRESSED
                .get_or_init(|| Mutex::new(HashSet::new()))
                .lock()
            {
                match message {
                    WM_KEYDOWN | WM_SYSKEYDOWN => {
                        let first_press = pressed.insert(vk);
                        let ctrl = pressed.contains(&0x11)
                            || pressed.contains(&0xA2)
                            || pressed.contains(&0xA3);
                        if ctrl {
                            let action = match vk {
                                0x43 => Some(TipKind::Copied),
                                0x58 => Some(TipKind::Cut),
                                0x56 => Some(TipKind::Pasted),
                                _ => None,
                            };
                            if let Some(kind) = action {
                                if allow_shortcut(vk, first_press) {
                                    match kind {
                                        TipKind::Copied => notify_copied(),
                                        TipKind::Cut => notify_cut(),
                                        TipKind::Pasted => notify_pasted(),
                                    }
                                }
                            }
                        }
                    }
                    WM_KEYUP | WM_SYSKEYUP => {
                        pressed.remove(&vk);
                    }
                    _ => {}
                }
            }
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, w_param, l_param)
}
