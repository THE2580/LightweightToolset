//! 「键鼠监听」型快捷键：进程内低级键盘/鼠标钩子（`WH_KEYBOARD_LL` / `WH_MOUSE_LL`）。
//!
//! 口径（与「全局热键」区分）：
//! - 只监听、不吞键：钩子始终 `CallNextHookEx`，按键照常传给系统与其它程序；
//! - 按下即触发：不做长按判定，键盘自动重复只认第一次按下；
//! - 同键 250ms 去抖，忽略 `LLKHF_INJECTED` / `LLMHF_INJECTED` 注入事件；
//! - 鼠标侧键（XBUTTON1/2）WebView 收不到，录制监听型快捷键时由 `begin_capture`
//!   临时把侧键通过 `hotkey-captured` 事件回传前端；录制期间吞掉侧键，避免 WebView
//!   触发前进/后退导航。
//!
//! 生命周期由 `tools.rs` 的 `register_hotkey` / `unregister_hotkey` 驱动：
//! 有监听型快捷键（或正在录制）时才启动钩子线程，全部注销后线程退出、钩子卸载。

use std::{
    collections::{BTreeMap, HashSet},
    sync::{Mutex, OnceLock},
    thread,
    time::{Duration, Instant},
};

use tauri::AppHandle;

/// 同键去抖窗口：按住键盘触发的自动重复、连点都在此窗口内合并成一次。
const DEBOUNCE: Duration = Duration::from_millis(250);

/// 录制通道的安全上限：前端正常会主动结束录制，这里只兜「界面异常退出/崩溃」，
/// 避免侧键被永久吞掉（录制态会吞掉 XBUTTON，见 `swallow_for_capture`）。
const CAPTURE_TIMEOUT: Duration = Duration::from_secs(120);

const CAPTURE_EVENT: &str = "hotkey-captured";

struct CaptureSession {
    app: AppHandle,
    until: Instant,
}

struct HookState {
    /// 派发用的 AppHandle（首次注册时写入；监听触发时克隆给独立线程）。
    app: Option<AppHandle>,
    /// 规范化按键 → 快捷键 id 列表。
    routes: BTreeMap<String, Vec<String>>,
    /// 录制通道：有值时把鼠标侧键回传前端，不派发快捷键。
    capture: Option<CaptureSession>,
    /// 当前按下的键盘虚拟键，用于过滤自动重复。
    keys_down: HashSet<u32>,
    /// 每个按键最近一次触发时间，用于 250ms 去抖。
    last_fired: BTreeMap<String, Instant>,
}

impl Default for HookState {
    fn default() -> Self {
        Self {
            app: None,
            routes: BTreeMap::new(),
            capture: None,
            keys_down: HashSet::new(),
            last_fired: BTreeMap::new(),
        }
    }
}

struct Worker {
    thread_id: u32,
    thread: thread::JoinHandle<()>,
}

static HOOK_STATE: OnceLock<Mutex<HookState>> = OnceLock::new();
static WORKER: OnceLock<Mutex<Option<Worker>>> = OnceLock::new();

fn hook_state() -> &'static Mutex<HookState> {
    HOOK_STATE.get_or_init(|| Mutex::new(HookState::default()))
}

fn worker_slot() -> &'static Mutex<Option<Worker>> {
    WORKER.get_or_init(|| Mutex::new(None))
}

/// 把存储值/录制值收敛成监听用的规范化按键标识（`A` / `F12` / `ARROWUP` / `MOUSE4`）。
///
/// 返回 `None` 表示该键不支持监听（修饰键本身、未登记的符号等）。
pub fn listen_key_token(value: &str) -> Option<String> {
    let upper = value.trim().to_uppercase();
    if upper.is_empty() {
        return None;
    }
    if let Some(token) = named_token(&upper) {
        return Some(token.to_owned());
    }
    if let Some(number) = upper.strip_prefix('F').and_then(|rest| rest.parse::<u32>().ok()) {
        if (1..=24).contains(&number) {
            return Some(format!("F{number}"));
        }
    }
    let mut chars = upper.chars();
    if let (Some(ch), None) = (chars.next(), chars.next()) {
        if ch.is_ascii_alphanumeric() {
            return Some(upper);
        }
    }
    None
}

fn named_token(name: &str) -> Option<&'static str> {
    let token = match name {
        "UP" | "ARROWUP" => "ARROWUP",
        "DOWN" | "ARROWDOWN" => "ARROWDOWN",
        "LEFT" | "ARROWLEFT" => "ARROWLEFT",
        "RIGHT" | "ARROWRIGHT" => "ARROWRIGHT",
        "PGUP" | "PAGEUP" => "PAGEUP",
        "PGDN" | "PAGEDOWN" => "PAGEDOWN",
        "TAB" => "TAB",
        "SPACE" | "SPACEBAR" => "SPACE",
        "ENTER" | "RETURN" => "ENTER",
        "ESC" | "ESCAPE" => "ESCAPE",
        "BACKSPACE" => "BACKSPACE",
        "INS" | "INSERT" => "INSERT",
        "DEL" | "DELETE" => "DELETE",
        "HOME" => "HOME",
        "END" => "END",
        "CAPSLOCK" => "CAPSLOCK",
        "NUMLOCK" => "NUMLOCK",
        "SCROLLLOCK" => "SCROLLLOCK",
        "PRINTSCREEN" => "PRINTSCREEN",
        "PAUSE" => "PAUSE",
        "MOUSE4" => "MOUSE4",
        "MOUSE5" => "MOUSE5",
        "/" | "SLASH" => "SLASH",
        "." | "PERIOD" => "PERIOD",
        "," | "COMMA" => "COMMA",
        ";" | "SEMICOLON" => "SEMICOLON",
        "'" | "QUOTE" => "QUOTE",
        "[" | "BRACKETLEFT" => "BRACKETLEFT",
        "]" | "BRACKETRIGHT" => "BRACKETRIGHT",
        "=" | "EQUAL" => "EQUAL",
        "-" | "MINUS" => "MINUS",
        "`" | "BACKQUOTE" => "BACKQUOTE",
        "\\" | "BACKSLASH" => "BACKSLASH",
        _ => return None,
    };
    Some(token)
}

/// 虚拟键码 → 规范化按键标识（与 `listen_key_token` 同一套标识）。
fn key_token(virtual_key: u32) -> Option<String> {
    let token = match virtual_key {
        0x08 => "BACKSPACE",
        0x09 => "TAB",
        0x0D => "ENTER",
        0x1B => "ESCAPE",
        0x20 => "SPACE",
        0x21 => "PAGEUP",
        0x22 => "PAGEDOWN",
        0x23 => "END",
        0x24 => "HOME",
        0x25 => "ARROWLEFT",
        0x26 => "ARROWUP",
        0x27 => "ARROWRIGHT",
        0x28 => "ARROWDOWN",
        0x2C => "PRINTSCREEN",
        0x2D => "INSERT",
        0x2E => "DELETE",
        0x13 => "PAUSE",
        0x14 => "CAPSLOCK",
        0x90 => "NUMLOCK",
        0x91 => "SCROLLLOCK",
        0x30..=0x39 | 0x41..=0x5A => {
            return char::from_u32(virtual_key).map(|ch| ch.to_string())
        }
        0x70..=0x87 => return Some(format!("F{}", virtual_key - 0x6F)),
        0xBA => "SEMICOLON",
        0xBB => "EQUAL",
        0xBC => "COMMA",
        0xBD => "MINUS",
        0xBE => "PERIOD",
        0xBF => "SLASH",
        0xC0 => "BACKQUOTE",
        0xDB => "BRACKETLEFT",
        0xDC => "BACKSLASH",
        0xDD => "BRACKETRIGHT",
        0xDE => "QUOTE",
        _ => return None,
    };
    Some(token.to_owned())
}

/// 注册一个监听型快捷键：同一按键只保留一条 id，重复注册幂等。
pub fn register(app: &AppHandle, hotkey_id: &str, key: &str) -> Result<(), String> {
    let token = listen_key_token(key).ok_or_else(|| "该键不支持键鼠监听".to_owned())?;
    {
        let mut state = hook_state().lock().map_err(|_| "键鼠监听服务不可用")?;
        state.app = Some(app.clone());
        let ids = state.routes.entry(token).or_default();
        if !ids.iter().any(|id| id == hotkey_id) {
            ids.push(hotkey_id.to_owned());
        }
    }
    if let Err(error) = ensure_worker() {
        unregister(hotkey_id);
        return Err(error);
    }
    Ok(())
}

/// 注销一个监听型快捷键（按 id 移除；该按键没有其它归属时一并删除路由）。
pub fn unregister(hotkey_id: &str) {
    {
        let Ok(mut state) = hook_state().lock() else {
            return;
        };
        for ids in state.routes.values_mut() {
            ids.retain(|id| id != hotkey_id);
        }
        state.routes.retain(|_, ids| !ids.is_empty());
        // 钩子线程可能在按键仍按下时被停止，残留的按下状态会吞掉下一次按键。
        state.keys_down.clear();
        state.last_fired.clear();
    }
    stop_worker_if_idle();
}

/// 进入「录制一次」：把鼠标侧键回传给前端（快捷键本身已在录制期间挂起）。
pub fn begin_capture(app: &AppHandle) -> Result<(), String> {
    {
        let mut state = hook_state().lock().map_err(|_| "键鼠监听服务不可用")?;
        state.app = Some(app.clone());
        state.capture = Some(CaptureSession {
            app: app.clone(),
            until: Instant::now() + CAPTURE_TIMEOUT,
        });
        state.keys_down.clear();
        state.last_fired.clear();
    }
    if let Err(error) = ensure_worker() {
        end_capture();
        return Err(error);
    }
    Ok(())
}

pub fn end_capture() {
    if let Ok(mut state) = hook_state().lock() {
        state.capture = None;
    }
    stop_worker_if_idle();
}

fn ensure_worker() -> Result<(), String> {
    let mut slot = worker_slot().lock().map_err(|_| "键鼠监听服务不可用")?;
    if slot.is_none() {
        *slot = Some(Worker::start()?);
    }
    Ok(())
}

fn stop_worker_if_idle() {
    let idle = match hook_state().lock() {
        Ok(state) => state.routes.is_empty() && state.capture.is_none(),
        Err(_) => false,
    };
    if !idle {
        return;
    }
    let worker = worker_slot().lock().ok().and_then(|mut slot| slot.take());
    if let Some(worker) = worker {
        worker.stop();
    }
}

fn allow_fire(state: &mut HookState, token: &str) -> bool {
    let now = Instant::now();
    if let Some(last) = state.last_fired.get(token) {
        if now.duration_since(*last) < DEBOUNCE {
            return false;
        }
    }
    state.last_fired.insert(token.to_owned(), now);
    true
}

/// 触发派发放到独立线程：钩子回调必须立刻返回，否则系统会丢弃/忽略这个低级钩子。
fn dispatch_async(app: &AppHandle, hotkey_id: &str) {
    let app = app.clone();
    let hotkey_id = hotkey_id.to_owned();
    std::thread::spawn(move || crate::dispatch_hotkey_id(&app, &hotkey_id));
}

#[cfg(target_os = "windows")]
mod platform {
    use super::{
        allow_fire, dispatch_async, hook_state, key_token, HookState, Worker, CAPTURE_EVENT,
    };
    use std::{mem, ptr, sync::mpsc, thread, time::Instant};
    use tauri::{AppHandle, Emitter};
    use windows_sys::Win32::{
        Foundation::{LPARAM, LRESULT, WPARAM},
        System::{LibraryLoader::GetModuleHandleW, Threading::GetCurrentThreadId},
        UI::WindowsAndMessaging::{
            CallNextHookEx, GetMessageW, KBDLLHOOKSTRUCT, LLKHF_INJECTED, LLMHF_INJECTED,
            MSG, MSLLHOOKSTRUCT, PM_NOREMOVE, PeekMessageW, PostThreadMessageW, SetWindowsHookExW,
            UnhookWindowsHookEx, WH_KEYBOARD_LL, WH_MOUSE_LL, WM_KEYDOWN, WM_KEYUP, WM_QUIT,
            WM_SYSKEYDOWN, WM_SYSKEYUP, WM_XBUTTONDOWN, WM_XBUTTONUP, XBUTTON1,
        },
    };

    impl Worker {
        pub(super) fn start() -> Result<Self, String> {
            let (ready_tx, ready_rx) = mpsc::sync_channel(1);
            let thread = thread::Builder::new()
                .name("hotkey-listen-hooks".to_owned())
                .spawn(move || unsafe {
                    let module = GetModuleHandleW(ptr::null());
                    if module.is_null() {
                        let _ = ready_tx.send(Err("读取当前程序模块句柄失败".to_owned()));
                        return;
                    }
                    let keyboard_hook =
                        SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), module, 0);
                    if keyboard_hook.is_null() {
                        let _ = ready_tx.send(Err("启动键盘监听失败".to_owned()));
                        return;
                    }
                    let mouse_hook = SetWindowsHookExW(WH_MOUSE_LL, Some(mouse_hook), module, 0);
                    if mouse_hook.is_null() {
                        UnhookWindowsHookEx(keyboard_hook);
                        let _ = ready_tx.send(Err("启动鼠标监听失败".to_owned()));
                        return;
                    }
                    // 先建好线程消息队列：PostThreadMessageW(WM_QUIT) 在队列不存在时会失败，
                    // 那样 stop() 就等不到 WM_QUIT，join 也会卡住。
                    let mut message = mem::zeroed::<MSG>();
                    PeekMessageW(&mut message, ptr::null_mut(), 0, 0, PM_NOREMOVE);
                    let _ = ready_tx.send(Ok(GetCurrentThreadId()));
                    while GetMessageW(&mut message, ptr::null_mut(), 0, 0) > 0 {}
                    UnhookWindowsHookEx(keyboard_hook);
                    UnhookWindowsHookEx(mouse_hook);
                })
                .map_err(|error| format!("启动键鼠监听线程失败: {error}"))?;

            match ready_rx.recv() {
                Ok(Ok(thread_id)) => Ok(Self { thread_id, thread }),
                Ok(Err(error)) => {
                    let _ = thread.join();
                    Err(error)
                }
                Err(_) => {
                    let _ = thread.join();
                    Err("键鼠监听线程未能初始化".to_owned())
                }
            }
        }

        pub(super) fn stop(self) {
            unsafe {
                PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0);
            }
            let _ = self.thread.join();
        }
    }

    /// 低级键盘钩子：只认第一次按下（自动重复走 `keys_down` 过滤），任何时候都放行。
    unsafe extern "system" fn keyboard_hook(code: i32, w_param: WPARAM, l_param: LPARAM) -> LRESULT {
        if code == 0 && l_param != 0 {
            let input = &*(l_param as *const KBDLLHOOKSTRUCT);
            if input.flags & LLKHF_INJECTED == 0 {
                handle_keyboard(w_param as u32, input.vkCode);
            }
        }
        CallNextHookEx(ptr::null_mut(), code, w_param, l_param)
    }

    fn handle_keyboard(message: u32, virtual_key: u32) {
        let is_key_up = matches!(message, WM_KEYUP | WM_SYSKEYUP);
        if !is_key_up && !matches!(message, WM_KEYDOWN | WM_SYSKEYDOWN) {
            return;
        }
        let Ok(mut state) = hook_state().lock() else {
            return;
        };
        if is_key_up {
            state.keys_down.remove(&virtual_key);
            return;
        }
        // 按住不放时系统会持续派发 keydown：只认第一次按下，不做长按逻辑。
        if !state.keys_down.insert(virtual_key) {
            return;
        }
        // 录制态：键盘由页面 keydown 负责，监听侧不重复处理。
        if capture_app(&mut state).is_some() {
            return;
        }
        let Some(token) = key_token(virtual_key) else {
            return;
        };
        let targets = take_targets(&mut state, &token);
        drop(state);
        dispatch_targets(targets);
    }

    /// 低级鼠标钩子：只处理 XBUTTON1/2（侧键），录制态吞掉侧键避免 WebView 前进/后退。
    unsafe extern "system" fn mouse_hook(code: i32, w_param: WPARAM, l_param: LPARAM) -> LRESULT {
        let message = w_param as u32;
        if code == 0
            && l_param != 0
            && matches!(message, WM_XBUTTONDOWN | WM_XBUTTONUP)
        {
            let input = &*(l_param as *const MSLLHOOKSTRUCT);
            if input.flags & LLMHF_INJECTED == 0 && message == WM_XBUTTONDOWN {
                let which = (input.mouseData >> 16) as u16;
                let token = if which == XBUTTON1 { "MOUSE4" } else { "MOUSE5" };
                if swallow_for_capture(token) {
                    return 1;
                }
                if let Ok(mut state) = hook_state().lock() {
                    let targets = take_targets(&mut state, token);
                    drop(state);
                    dispatch_targets(targets);
                }
            }
        }
        CallNextHookEx(ptr::null_mut(), code, w_param, l_param)
    }

    /// 录制态：把侧键回传前端并吞掉这次点击；返回 true 表示钩子要返回 1。
    fn swallow_for_capture(token: &str) -> bool {
        let app = {
            let Ok(mut state) = hook_state().lock() else {
                return false;
            };
            capture_app(&mut state)
        };
        let Some(app) = app else {
            return false;
        };
        let _ = app.emit(CAPTURE_EVENT, token.to_owned());
        true
    }

    /// 取录制回传用 AppHandle；超时未结束的录制在这里自动解除（兜底，避免永久吞侧键）。
    fn capture_app(state: &mut HookState) -> Option<AppHandle> {
        let active = match state.capture.as_ref() {
            Some(session) => Instant::now() < session.until,
            None => false,
        };
        if !active {
            state.capture = None;
            return None;
        }
        state.capture.as_ref().map(|session| session.app.clone())
    }

    /// 去抖 + 取派发目标（AppHandle 与快捷键 id），调用方负责释放锁后再拉线程。
    fn take_targets(state: &mut HookState, token: &str) -> Option<(AppHandle, Vec<String>)> {
        if !allow_fire(state, token) {
            return None;
        }
        let ids = state.routes.get(token).cloned().unwrap_or_default();
        if ids.is_empty() {
            return None;
        }
        let app = state.app.clone()?;
        Some((app, ids))
    }

    fn dispatch_targets(targets: Option<(AppHandle, Vec<String>)>) {
        let Some((app, ids)) = targets else {
            return;
        };
        for hotkey_id in ids {
            dispatch_async(&app, &hotkey_id);
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod platform {
    use super::Worker;

    impl Worker {
        pub(super) fn start() -> Result<Self, String> {
            Err("键鼠监听仅支持 Windows".to_owned())
        }

        pub(super) fn stop(self) {}
    }
}

#[cfg(test)]
mod tests {
    use super::{key_token, listen_key_token};

    #[test]
    fn accepts_single_keys_and_side_buttons() {
        assert_eq!(listen_key_token("a").as_deref(), Some("A"));
        assert_eq!(listen_key_token("F12").as_deref(), Some("F12"));
        assert_eq!(listen_key_token("f24").as_deref(), Some("F24"));
        assert_eq!(listen_key_token("ArrowUp").as_deref(), Some("ARROWUP"));
        assert_eq!(listen_key_token("/").as_deref(), Some("SLASH"));
        assert_eq!(listen_key_token("MOUSE4").as_deref(), Some("MOUSE4"));
        assert_eq!(listen_key_token("MOUSE5").as_deref(), Some("MOUSE5"));
    }

    #[test]
    fn rejects_modifiers_combos_and_unknown_keys() {
        assert_eq!(listen_key_token(""), None);
        assert_eq!(listen_key_token("CTRL"), None);
        assert_eq!(listen_key_token("CTRL+A"), None);
        assert_eq!(listen_key_token("F25"), None);
        assert_eq!(listen_key_token("0"), Some("0".to_owned()));
        // 带修饰键的组合键写法不能被当成单个按键。
        assert_eq!(listen_key_token("CTRL+ALT+M"), None);
    }

    #[test]
    fn virtual_keys_map_to_the_same_tokens() {
        assert_eq!(key_token(0x41).as_deref(), Some("A"));
        assert_eq!(key_token(0x5A).as_deref(), Some("Z"));
        assert_eq!(key_token(0x31).as_deref(), Some("1"));
        assert_eq!(key_token(0x70).as_deref(), Some("F1"));
        assert_eq!(key_token(0x87).as_deref(), Some("F24"));
        assert_eq!(key_token(0x26).as_deref(), Some("ARROWUP"));
        assert_eq!(key_token(0xBF).as_deref(), Some("SLASH"));
        // 修饰键本身不参与匹配（监听只认单键）。
        assert_eq!(key_token(0x11), None);
        assert_eq!(key_token(0xA2), None);
    }
}
