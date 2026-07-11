use std::{
    collections::{BTreeMap, HashSet},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, Mutex, OnceLock,
    },
    thread,
};

static MONITOR: OnceLock<Mutex<MonitorState>> = OnceLock::new();
static BUTTON_SUBSCRIBERS: OnceLock<Mutex<BTreeMap<u64, mpsc::Sender<ButtonEvent>>>> =
    OnceLock::new();
static PRESSED_KEYS: OnceLock<Mutex<HashSet<u32>>> = OnceLock::new();
static LAST_PHYSICAL_INPUT_MS: AtomicU64 = AtomicU64::new(0);

struct MonitorState {
    worker: Option<InputWorker>,
    activity_consumers: usize,
    key_consumers: usize,
    next_subscriber_id: u64,
}

impl Default for MonitorState {
    fn default() -> Self {
        Self {
            worker: None,
            activity_consumers: 0,
            key_consumers: 0,
            next_subscriber_id: 1,
        }
    }
}

#[cfg(target_os = "windows")]
struct InputWorker {
    thread_id: u32,
    thread: thread::JoinHandle<()>,
}

#[cfg(not(target_os = "windows"))]
struct InputWorker;

pub struct ActivityMonitor;

pub struct ButtonInputMonitor {
    subscriber_id: u64,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ButtonEvent {
    Keyboard(u32),
    Mouse(MouseButton),
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MouseButton {
    Left,
    Right,
    Middle,
    X1,
    X2,
}

fn is_valid_raw_keyboard_virtual_key(virtual_key: u32) -> bool {
    virtual_key != 0xFF
}

fn monitor() -> &'static Mutex<MonitorState> {
    MONITOR.get_or_init(|| Mutex::new(MonitorState::default()))
}

fn button_subscribers() -> &'static Mutex<BTreeMap<u64, mpsc::Sender<ButtonEvent>>> {
    BUTTON_SUBSCRIBERS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

pub fn start_activity_monitor() -> Result<ActivityMonitor, String> {
    let mut state = monitor().lock().map_err(|_| "物理输入监听服务不可用")?;
    ensure_worker(&mut state)?;
    state.activity_consumers += 1;
    Ok(ActivityMonitor)
}

pub fn stop_activity_monitor(_monitor: ActivityMonitor) {
    let worker = {
        let Ok(mut state) = monitor().lock() else {
            return;
        };
        state.activity_consumers = state.activity_consumers.saturating_sub(1);
        take_unused_worker(&mut state)
    };
    if let Some(worker) = worker {
        worker.stop();
    }
}

pub fn subscribe_button_down() -> Result<(ButtonInputMonitor, mpsc::Receiver<ButtonEvent>), String>
{
    let mut state = monitor().lock().map_err(|_| "物理输入监听服务不可用")?;
    ensure_worker(&mut state)?;
    let subscriber_id = state.next_subscriber_id;
    state.next_subscriber_id = state.next_subscriber_id.saturating_add(1);
    let (sender, receiver) = mpsc::channel();
    button_subscribers()
        .lock()
        .map_err(|_| "按键监听订阅服务不可用")?
        .insert(subscriber_id, sender);
    state.key_consumers += 1;
    Ok((ButtonInputMonitor { subscriber_id }, receiver))
}

pub fn unsubscribe_button_down(subscription: ButtonInputMonitor) {
    let worker = {
        let Ok(mut state) = monitor().lock() else {
            return;
        };
        if let Ok(mut subscribers) = button_subscribers().lock() {
            subscribers.remove(&subscription.subscriber_id);
        }
        state.key_consumers = state.key_consumers.saturating_sub(1);
        take_unused_worker(&mut state)
    };
    if let Some(worker) = worker {
        worker.stop();
    }
}

fn ensure_worker(state: &mut MonitorState) -> Result<(), String> {
    if state.worker.is_none() {
        state.worker = Some(InputWorker::start()?);
    }
    Ok(())
}

fn take_unused_worker(state: &mut MonitorState) -> Option<InputWorker> {
    if state.activity_consumers == 0 && state.key_consumers == 0 {
        state.worker.take()
    } else {
        None
    }
}

#[cfg(target_os = "windows")]
impl InputWorker {
    fn start() -> Result<Self, String> {
        use windows_sys::Win32::{
            System::{
                LibraryLoader::GetModuleHandleW, SystemInformation::GetTickCount64,
                Threading::GetCurrentThreadId,
            },
            UI::Input::{RegisterRawInputDevices, RAWINPUTDEVICE, RIDEV_INPUTSINK},
            UI::WindowsAndMessaging::{
                CreateWindowExW, DestroyWindow, DispatchMessageW, GetMessageW, RegisterClassW,
                SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx, UnregisterClassW,
                HWND_MESSAGE, MSG, WH_MOUSE_LL, WNDCLASSW,
            },
        };

        let (ready_tx, ready_rx) = mpsc::sync_channel(1);
        let thread = thread::Builder::new()
            .name("shared-physical-input".to_owned())
            .spawn(move || unsafe {
                let module = GetModuleHandleW(std::ptr::null());
                if module.is_null() {
                    let _ = ready_tx.send(Err("读取当前程序模块句柄失败".to_owned()));
                    return;
                }

                let window_class = WNDCLASSW {
                    lpfnWndProc: Some(raw_input_window_proc),
                    hInstance: module,
                    lpszClassName: RAW_INPUT_WINDOW_CLASS.as_ptr(),
                    ..std::mem::zeroed()
                };
                if RegisterClassW(&window_class) == 0 {
                    let _ = ready_tx.send(Err("注册原始输入窗口失败".to_owned()));
                    return;
                }
                let input_window = CreateWindowExW(
                    0,
                    RAW_INPUT_WINDOW_CLASS.as_ptr(),
                    RAW_INPUT_WINDOW_CLASS.as_ptr(),
                    0,
                    0,
                    0,
                    0,
                    0,
                    HWND_MESSAGE,
                    std::ptr::null_mut(),
                    module,
                    std::ptr::null(),
                );
                if input_window.is_null() {
                    UnregisterClassW(RAW_INPUT_WINDOW_CLASS.as_ptr(), module);
                    let _ = ready_tx.send(Err("创建原始输入窗口失败".to_owned()));
                    return;
                }
                let keyboard = RAWINPUTDEVICE {
                    usUsagePage: 0x01,
                    usUsage: 0x06,
                    dwFlags: RIDEV_INPUTSINK,
                    hwndTarget: input_window,
                };
                if RegisterRawInputDevices(
                    &keyboard,
                    1,
                    std::mem::size_of::<RAWINPUTDEVICE>() as u32,
                ) == 0
                {
                    DestroyWindow(input_window);
                    UnregisterClassW(RAW_INPUT_WINDOW_CLASS.as_ptr(), module);
                    let _ = ready_tx.send(Err("启动键盘原始输入监听失败".to_owned()));
                    return;
                }
                let mouse_hook =
                    SetWindowsHookExW(WH_MOUSE_LL, Some(physical_mouse_hook), module, 0);
                if mouse_hook.is_null() {
                    DestroyWindow(input_window);
                    UnregisterClassW(RAW_INPUT_WINDOW_CLASS.as_ptr(), module);
                    let _ = ready_tx.send(Err("启动鼠标输入监听失败".to_owned()));
                    return;
                }

                LAST_PHYSICAL_INPUT_MS.store(GetTickCount64(), Ordering::Release);
                let _ = ready_tx.send(Ok(GetCurrentThreadId()));
                let mut message = std::mem::zeroed::<MSG>();
                while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
                    TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
                UnhookWindowsHookEx(mouse_hook);
                DestroyWindow(input_window);
                UnregisterClassW(RAW_INPUT_WINDOW_CLASS.as_ptr(), module);
            })
            .map_err(|error| format!("启动物理输入监听线程失败: {error}"))?;

        let thread_id = match ready_rx.recv() {
            Ok(Ok(thread_id)) => thread_id,
            Ok(Err(error)) => {
                let _ = thread.join();
                return Err(error);
            }
            Err(_) => {
                let _ = thread.join();
                return Err("物理输入监听线程未能初始化".to_owned());
            }
        };
        Ok(Self { thread_id, thread })
    }

    fn stop(self) {
        use windows_sys::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT};
        unsafe {
            PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0);
        }
        let _ = self.thread.join();
        if let Ok(mut pressed) = PRESSED_KEYS
            .get_or_init(|| Mutex::new(HashSet::new()))
            .lock()
        {
            pressed.clear();
        }
    }
}

#[cfg(target_os = "windows")]
const RAW_INPUT_WINDOW_CLASS: [u16; 36] = [
    76, 105, 103, 104, 116, 119, 101, 105, 103, 104, 116, 84, 111, 111, 108, 115, 101, 116, 82, 97,
    119, 73, 110, 112, 117, 116, 87, 105, 110, 100, 111, 119, 67, 108, 115, 0,
];

#[cfg(target_os = "windows")]
unsafe extern "system" fn raw_input_window_proc(
    window: windows_sys::Win32::Foundation::HWND,
    message: u32,
    w_param: windows_sys::Win32::Foundation::WPARAM,
    l_param: windows_sys::Win32::Foundation::LPARAM,
) -> windows_sys::Win32::Foundation::LRESULT {
    use windows_sys::Win32::{
        System::SystemInformation::GetTickCount64,
        UI::{
            Input::{
                GetCurrentInputMessageSource, GetRawInputData, IMO_INJECTED, INPUT_MESSAGE_SOURCE,
                RAWINPUT, RAWINPUTHEADER, RID_INPUT, RIM_TYPEKEYBOARD,
            },
            WindowsAndMessaging::{DefWindowProcW, RI_KEY_BREAK, WM_INPUT},
        },
    };

    if message == WM_INPUT {
        let mut input = std::mem::zeroed::<RAWINPUT>();
        let mut input_size = std::mem::size_of::<RAWINPUT>() as u32;
        let read_size = GetRawInputData(
            l_param as _,
            RID_INPUT,
            (&mut input as *mut RAWINPUT).cast(),
            &mut input_size,
            std::mem::size_of::<RAWINPUTHEADER>() as u32,
        );
        if read_size != u32::MAX && input.header.dwType == RIM_TYPEKEYBOARD {
            let keyboard = input.data.keyboard;
            let virtual_key = keyboard.VKey as u32;
            if !is_valid_raw_keyboard_virtual_key(virtual_key) {
                return DefWindowProcW(window, message, w_param, l_param);
            }
            let is_key_up = keyboard.Flags as u32 & RI_KEY_BREAK != 0;
            let mut source = std::mem::zeroed::<INPUT_MESSAGE_SOURCE>();
            let is_injected =
                GetCurrentInputMessageSource(&mut source) != 0 && source.originId == IMO_INJECTED;
            if !is_injected && is_key_up {
                if let Ok(mut pressed) = PRESSED_KEYS
                    .get_or_init(|| Mutex::new(HashSet::new()))
                    .lock()
                {
                    pressed.remove(&virtual_key);
                }
            } else if !is_injected {
                LAST_PHYSICAL_INPUT_MS.store(GetTickCount64(), Ordering::Release);
                let first_press = PRESSED_KEYS
                    .get_or_init(|| Mutex::new(HashSet::new()))
                    .lock()
                    .map(|mut pressed| pressed.insert(virtual_key))
                    .unwrap_or(false);
                if first_press {
                    dispatch_button_down(ButtonEvent::Keyboard(virtual_key));
                }
            }
        }
    }

    DefWindowProcW(window, message, w_param, l_param)
}

#[cfg(not(target_os = "windows"))]
impl InputWorker {
    fn start() -> Result<Self, String> {
        Ok(Self)
    }

    fn stop(self) {}
}

#[cfg(target_os = "windows")]
pub fn physical_idle_seconds() -> u32 {
    use windows_sys::Win32::System::SystemInformation::GetTickCount64;
    let last_input_ms = LAST_PHYSICAL_INPUT_MS.load(Ordering::Acquire);
    if last_input_ms == 0 {
        return 0;
    }
    ((unsafe { GetTickCount64() }).saturating_sub(last_input_ms) / 1000).min(u32::MAX as u64) as u32
}

#[cfg(not(target_os = "windows"))]
pub fn physical_idle_seconds() -> u32 {
    0
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn physical_mouse_hook(
    code: i32,
    w_param: windows_sys::Win32::Foundation::WPARAM,
    l_param: windows_sys::Win32::Foundation::LPARAM,
) -> windows_sys::Win32::Foundation::LRESULT {
    use windows_sys::Win32::{
        System::SystemInformation::GetTickCount64,
        UI::WindowsAndMessaging::{
            CallNextHookEx, LLMHF_INJECTED, MSLLHOOKSTRUCT, WM_LBUTTONDOWN, WM_MBUTTONDOWN,
            WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_RBUTTONDOWN, WM_XBUTTONDOWN, XBUTTON1,
        },
    };
    let message = w_param as u32;
    if code == 0
        && matches!(
            message,
            WM_MOUSEMOVE
                | WM_LBUTTONDOWN
                | WM_RBUTTONDOWN
                | WM_MBUTTONDOWN
                | WM_MOUSEWHEEL
                | WM_XBUTTONDOWN
        )
        && l_param != 0
    {
        let input = &*(l_param as *const MSLLHOOKSTRUCT);
        if input.flags & LLMHF_INJECTED == 0 {
            LAST_PHYSICAL_INPUT_MS.store(GetTickCount64(), Ordering::Release);
            let button = match message {
                WM_LBUTTONDOWN => Some(MouseButton::Left),
                WM_RBUTTONDOWN => Some(MouseButton::Right),
                WM_MBUTTONDOWN => Some(MouseButton::Middle),
                WM_XBUTTONDOWN => Some(if (input.mouseData >> 16) as u16 == XBUTTON1 {
                    MouseButton::X1
                } else {
                    MouseButton::X2
                }),
                _ => None,
            };
            if let Some(button) = button {
                dispatch_button_down(ButtonEvent::Mouse(button));
            }
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, w_param, l_param)
}

#[cfg(target_os = "windows")]
fn dispatch_button_down(event: ButtonEvent) {
    if let Ok(mut subscribers) = button_subscribers().lock() {
        subscribers.retain(|_, sender| sender.send(event).is_ok());
    }
}

#[cfg(test)]
mod tests {
    use super::is_valid_raw_keyboard_virtual_key;

    #[test]
    fn rejects_raw_input_sentinel_virtual_key() {
        assert!(!is_valid_raw_keyboard_virtual_key(0xFF));
        assert!(is_valid_raw_keyboard_virtual_key(0x41));
    }
}
