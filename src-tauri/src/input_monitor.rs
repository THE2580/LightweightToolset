use std::{
    collections::{BTreeMap, HashSet},
    io::{BufRead, BufReader, Read, Write},
    net::{TcpListener, TcpStream},
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, AtomicU64, Ordering},
        mpsc, Mutex, OnceLock,
    },
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

static MONITOR: OnceLock<Mutex<MonitorState>> = OnceLock::new();
static BUTTON_SUBSCRIBERS: OnceLock<Mutex<BTreeMap<u64, mpsc::Sender<ButtonEvent>>>> =
    OnceLock::new();
static PRESSED_KEYS: OnceLock<Mutex<HashSet<u32>>> = OnceLock::new();
static LAST_PHYSICAL_INPUT_MS: AtomicU64 = AtomicU64::new(0);
static ELEVATED_HELPER_DIR: OnceLock<PathBuf> = OnceLock::new();
static ELEVATED_HELPER_STREAM: OnceLock<Mutex<Option<TcpStream>>> = OnceLock::new();
static ELEVATED_INPUT_ACTIVE: AtomicBool = AtomicBool::new(false);
const ELEVATED_HELPER_ARG: &str = "--elevated-input-helper";
const ELEVATED_HELPER_PREFIX: &str = "elevated_input_helper_";

struct MonitorState {
    worker: Option<InputWorker>,
    elevated_worker: Option<ElevatedInputWorker>,
    activity_consumers: usize,
    key_consumers: usize,
    next_subscriber_id: u64,
}

impl Default for MonitorState {
    fn default() -> Self {
        Self {
            worker: None,
            elevated_worker: None,
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

struct ElevatedInputWorker {
    stop: mpsc::Sender<()>,
    thread: thread::JoinHandle<()>,
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

fn normalize_modifier_virtual_key(virtual_key: u32) -> u32 {
    match virtual_key {
        0xA0 | 0xA1 => 0x10,
        0xA2 | 0xA3 => 0x11,
        0xA4 | 0xA5 => 0x12,
        _ => virtual_key,
    }
}

fn update_pressed_key(virtual_key: u32, is_key_up: bool) -> bool {
    let virtual_key = normalize_modifier_virtual_key(virtual_key);
    let Ok(mut pressed) = PRESSED_KEYS
        .get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
    else {
        return false;
    };
    if is_key_up {
        pressed.remove(&virtual_key);
        false
    } else {
        pressed.insert(virtual_key)
    }
}

fn monitor() -> &'static Mutex<MonitorState> {
    MONITOR.get_or_init(|| Mutex::new(MonitorState::default()))
}

fn button_subscribers() -> &'static Mutex<BTreeMap<u64, mpsc::Sender<ButtonEvent>>> {
    BUTTON_SUBSCRIBERS.get_or_init(|| Mutex::new(BTreeMap::new()))
}

pub fn configure_elevated_helper_dir(config_dir: &Path) -> Result<(), String> {
    ELEVATED_HELPER_DIR
        .set(config_dir.to_path_buf())
        .map_err(|_| "管理员输入助手目录已配置".to_owned())
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
    if state.key_consumers == 0 && state.elevated_worker.is_none() {
        state.elevated_worker = ElevatedInputWorker::start().ok();
    }
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
    let (worker, elevated_worker) = {
        let Ok(mut state) = monitor().lock() else {
            return;
        };
        if let Ok(mut subscribers) = button_subscribers().lock() {
            subscribers.remove(&subscription.subscriber_id);
        }
        state.key_consumers = state.key_consumers.saturating_sub(1);
        let elevated_worker = if state.key_consumers == 0 {
            state.elevated_worker.take()
        } else {
            None
        };
        (take_unused_worker(&mut state), elevated_worker)
    };
    if let Some(worker) = elevated_worker {
        worker.stop();
    }
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

impl ElevatedInputWorker {
    fn start() -> Result<Self, String> {
        let helper_dir = ELEVATED_HELPER_DIR
            .get()
            .ok_or("管理员输入助手目录未配置")?;
        let listener = TcpListener::bind("127.0.0.1:0")
            .map_err(|error| format!("启动管理员输入通道失败: {error}"))?;
        listener
            .set_nonblocking(true)
            .map_err(|error| format!("配置管理员输入通道失败: {error}"))?;
        let address = listener
            .local_addr()
            .map_err(|error| format!("读取管理员输入通道失败: {error}"))?;
        let token = format!(
            "{:x}{:x}{:x}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or_default(),
            address.port()
        );
        let (stop, stop_receiver) = mpsc::channel();
        let expected_token = token.clone();
        let thread = thread::Builder::new()
            .name("elevated-input-ipc".to_owned())
            .spawn(move || elevated_ipc_loop(listener, expected_token, stop_receiver))
            .map_err(|error| format!("启动管理员输入接收线程失败: {error}"))?;

        if let Err(error) = launch_elevated_helper(helper_dir, &address.to_string(), &token) {
            let _ = stop.send(());
            let _ = thread.join();
            return Err(error);
        }
        Ok(Self { stop, thread })
    }

    fn stop(self) {
        let _ = self.stop.send(());
        let _ = self.thread.join();
        ELEVATED_INPUT_ACTIVE.store(false, Ordering::Release);
        clear_pressed_keys();
    }
}

fn elevated_ipc_loop(listener: TcpListener, expected_token: String, stop: mpsc::Receiver<()>) {
    while stop.try_recv().is_err() {
        match listener.accept() {
            Ok((stream, _)) => {
                if receive_elevated_events(stream, &expected_token, &stop) {
                    break;
                }
                ELEVATED_INPUT_ACTIVE.store(false, Ordering::Release);
                clear_pressed_keys();
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(Duration::from_millis(50));
            }
            Err(_) => break,
        }
    }
    ELEVATED_INPUT_ACTIVE.store(false, Ordering::Release);
    clear_pressed_keys();
}

fn receive_elevated_events(
    stream: TcpStream,
    expected_token: &str,
    stop: &mpsc::Receiver<()>,
) -> bool {
    let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    loop {
        if stop.try_recv().is_ok() {
            return true;
        }
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => return false,
            Ok(_) => break,
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => return false,
        }
    }
    if line.trim_end() != expected_token {
        return false;
    }
    clear_pressed_keys();
    ELEVATED_INPUT_ACTIVE.store(true, Ordering::Release);
    loop {
        if stop.try_recv().is_ok() {
            return true;
        }
        line.clear();
        match reader.read_line(&mut line) {
            Ok(0) => return false,
            Ok(_) => {
                if let Some(event) = parse_elevated_event(line.trim_end()) {
                    LAST_PHYSICAL_INPUT_MS.store(tick_count_ms(), Ordering::Release);
                    dispatch_button_down(event);
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => return false,
        }
    }
}

fn parse_elevated_event(value: &str) -> Option<ButtonEvent> {
    let (kind, raw) = value.split_once(' ')?;
    let code = raw.parse::<u32>().ok()?;
    match kind {
        "K" => Some(ButtonEvent::Keyboard(code)),
        "M" => match code {
            1 => Some(ButtonEvent::Mouse(MouseButton::Left)),
            2 => Some(ButtonEvent::Mouse(MouseButton::Right)),
            3 => Some(ButtonEvent::Mouse(MouseButton::Middle)),
            4 => Some(ButtonEvent::Mouse(MouseButton::X1)),
            5 => Some(ButtonEvent::Mouse(MouseButton::X2)),
            _ => None,
        },
        _ => None,
    }
}

fn tick_count_ms() -> u64 {
    #[cfg(target_os = "windows")]
    unsafe {
        windows_sys::Win32::System::SystemInformation::GetTickCount64()
    }
    #[cfg(not(target_os = "windows"))]
    {
        0
    }
}

fn clear_pressed_keys() {
    if let Ok(mut pressed) = PRESSED_KEYS
        .get_or_init(|| Mutex::new(HashSet::new()))
        .lock()
    {
        pressed.clear();
    }
}

#[cfg(target_os = "windows")]
fn launch_elevated_helper(helper_dir: &Path, address: &str, token: &str) -> Result<(), String> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::{Shell::ShellExecuteW, WindowsAndMessaging::SW_HIDE};
    let current_exe =
        std::env::current_exe().map_err(|error| format!("定位管理员输入助手失败: {error}"))?;
    let metadata = std::fs::metadata(&current_exe)
        .map_err(|error| format!("读取管理员输入助手信息失败: {error}"))?;
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
        .map(|duration| duration.as_nanos())
        .unwrap_or_default();
    let helper_path = helper_dir.join(format!(
        "{ELEVATED_HELPER_PREFIX}{}_{}.exe",
        metadata.len(),
        modified
    ));
    cleanup_stale_elevated_helpers(helper_dir, &helper_path);
    if !helper_path.exists() {
        std::fs::copy(&current_exe, &helper_path)
            .map_err(|error| format!("准备管理员输入助手失败: {error}"))?;
    }
    let operation = wide("runas");
    let file = helper_path
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect::<Vec<_>>();
    let parameters = wide(&format!("{ELEVATED_HELPER_ARG} {address} {token}"));
    let result = unsafe {
        ShellExecuteW(
            std::ptr::null_mut(),
            operation.as_ptr(),
            file.as_ptr(),
            parameters.as_ptr(),
            std::ptr::null(),
            SW_HIDE,
        )
    };
    if result as isize <= 32 {
        return Err(format!(
            "管理员输入助手未启动（ShellExecute {}）",
            result as isize
        ));
    }
    Ok(())
}

#[cfg(not(target_os = "windows"))]
fn launch_elevated_helper(_helper_dir: &Path, _address: &str, _token: &str) -> Result<(), String> {
    Err("当前平台不支持管理员输入助手".to_owned())
}

#[cfg(target_os = "windows")]
fn wide(value: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(value)
        .encode_wide()
        .chain(Some(0))
        .collect()
}

fn cleanup_stale_elevated_helpers(helper_dir: &Path, keep: &Path) {
    let Ok(entries) = std::fs::read_dir(helper_dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let is_helper = path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with(ELEVATED_HELPER_PREFIX) && name.ends_with(".exe"));
        if is_helper && path != keep {
            let _ = std::fs::remove_file(path);
        }
    }
}

pub fn run_elevated_helper_from_args() -> bool {
    let mut args = std::env::args();
    let _ = args.next();
    if args.next().as_deref() != Some(ELEVATED_HELPER_ARG) {
        return false;
    }
    let Some(address) = args.next() else {
        return true;
    };
    let Some(token) = args.next() else {
        return true;
    };
    let Ok(mut control) = TcpStream::connect(address) else {
        return true;
    };
    if writeln!(control, "{token}").is_err() {
        return true;
    }
    let Ok(events) = control.try_clone() else {
        return true;
    };
    if let Ok(mut stream) = ELEVATED_HELPER_STREAM
        .get_or_init(|| Mutex::new(None))
        .lock()
    {
        *stream = Some(events);
    }
    let Ok(worker) = InputWorker::start() else {
        return true;
    };
    let _ = control.set_read_timeout(Some(Duration::from_millis(250)));
    let mut buffer = [0u8; 1];
    loop {
        match control.read(&mut buffer) {
            Ok(0) => break,
            Ok(_) => {}
            Err(error)
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(_) => break,
        }
    }
    worker.stop();
    if let Ok(mut stream) = ELEVATED_HELPER_STREAM
        .get_or_init(|| Mutex::new(None))
        .lock()
    {
        *stream = None;
    }
    true
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
                HWND_MESSAGE, MSG, WH_KEYBOARD_LL, WH_MOUSE_LL, WNDCLASSW,
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
                let keyboard_hook =
                    SetWindowsHookExW(WH_KEYBOARD_LL, Some(physical_keyboard_hook), module, 0);

                LAST_PHYSICAL_INPUT_MS.store(GetTickCount64(), Ordering::Release);
                let _ = ready_tx.send(Ok(GetCurrentThreadId()));
                let mut message = std::mem::zeroed::<MSG>();
                while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
                    TranslateMessage(&message);
                    DispatchMessageW(&message);
                }
                if !keyboard_hook.is_null() {
                    UnhookWindowsHookEx(keyboard_hook);
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
            if ELEVATED_INPUT_ACTIVE.load(Ordering::Acquire) {
                return DefWindowProcW(window, message, w_param, l_param);
            }
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
                update_pressed_key(virtual_key, true);
            } else if !is_injected {
                LAST_PHYSICAL_INPUT_MS.store(GetTickCount64(), Ordering::Release);
                if update_pressed_key(virtual_key, false) {
                    dispatch_button_down(ButtonEvent::Keyboard(virtual_key));
                }
            }
        }
    }

    DefWindowProcW(window, message, w_param, l_param)
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn physical_keyboard_hook(
    code: i32,
    w_param: windows_sys::Win32::Foundation::WPARAM,
    l_param: windows_sys::Win32::Foundation::LPARAM,
) -> windows_sys::Win32::Foundation::LRESULT {
    use windows_sys::Win32::{
        System::SystemInformation::GetTickCount64,
        UI::WindowsAndMessaging::{
            CallNextHookEx, KBDLLHOOKSTRUCT, LLKHF_INJECTED, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN,
            WM_SYSKEYUP,
        },
    };
    let message = w_param as u32;
    if code == 0
        && matches!(message, WM_KEYDOWN | WM_SYSKEYDOWN | WM_KEYUP | WM_SYSKEYUP)
        && l_param != 0
    {
        let input = &*(l_param as *const KBDLLHOOKSTRUCT);
        if input.flags & LLKHF_INJECTED == 0 && !ELEVATED_INPUT_ACTIVE.load(Ordering::Acquire) {
            let is_key_up = matches!(message, WM_KEYUP | WM_SYSKEYUP);
            if is_key_up {
                update_pressed_key(input.vkCode, true);
            } else {
                LAST_PHYSICAL_INPUT_MS.store(GetTickCount64(), Ordering::Release);
                if update_pressed_key(input.vkCode, false) {
                    dispatch_button_down(ButtonEvent::Keyboard(input.vkCode));
                }
            }
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, w_param, l_param)
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
            if !ELEVATED_INPUT_ACTIVE.load(Ordering::Acquire) {
                if let Some(button) = button {
                    dispatch_button_down(ButtonEvent::Mouse(button));
                }
            }
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, w_param, l_param)
}

#[cfg(target_os = "windows")]
fn dispatch_button_down(event: ButtonEvent) {
    if let Ok(mut helper) = ELEVATED_HELPER_STREAM
        .get_or_init(|| Mutex::new(None))
        .lock()
    {
        if let Some(stream) = helper.as_mut() {
            let value = match event {
                ButtonEvent::Keyboard(code) => format!("K {code}\n"),
                ButtonEvent::Mouse(MouseButton::Left) => "M 1\n".to_owned(),
                ButtonEvent::Mouse(MouseButton::Right) => "M 2\n".to_owned(),
                ButtonEvent::Mouse(MouseButton::Middle) => "M 3\n".to_owned(),
                ButtonEvent::Mouse(MouseButton::X1) => "M 4\n".to_owned(),
                ButtonEvent::Mouse(MouseButton::X2) => "M 5\n".to_owned(),
            };
            let _ = stream.write_all(value.as_bytes());
            return;
        }
    }
    if let Ok(mut subscribers) = button_subscribers().lock() {
        subscribers.retain(|_, sender| sender.send(event).is_ok());
    }
}

#[cfg(test)]
mod tests {
    use super::{
        is_valid_raw_keyboard_virtual_key, normalize_modifier_virtual_key, parse_elevated_event,
        update_pressed_key, ButtonEvent, MouseButton, PRESSED_KEYS,
    };

    #[test]
    fn rejects_raw_input_sentinel_virtual_key() {
        assert!(!is_valid_raw_keyboard_virtual_key(0xFF));
        assert!(is_valid_raw_keyboard_virtual_key(0x41));
    }

    #[test]
    fn deduplicates_keyboard_down_across_raw_input_and_hook() {
        let pressed =
            PRESSED_KEYS.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()));
        pressed.lock().unwrap().clear();
        assert!(update_pressed_key(0x41, false));
        assert!(!update_pressed_key(0x41, false));
        assert!(!update_pressed_key(0x41, true));
        assert!(update_pressed_key(0x41, false));
        pressed.lock().unwrap().clear();
    }

    #[test]
    fn normalizes_sided_modifiers_before_cross_source_deduplication() {
        let pressed =
            PRESSED_KEYS.get_or_init(|| std::sync::Mutex::new(std::collections::HashSet::new()));
        pressed.lock().unwrap().clear();
        assert_eq!(normalize_modifier_virtual_key(0xA0), 0x10);
        assert_eq!(normalize_modifier_virtual_key(0xA3), 0x11);
        assert_eq!(normalize_modifier_virtual_key(0xA5), 0x12);
        assert!(update_pressed_key(0xA0, false));
        assert!(!update_pressed_key(0x10, false));
        assert!(!update_pressed_key(0xA1, true));
        assert!(update_pressed_key(0x10, false));
        pressed.lock().unwrap().clear();
    }

    #[test]
    fn parses_only_known_elevated_input_events() {
        assert_eq!(
            parse_elevated_event("K 65"),
            Some(ButtonEvent::Keyboard(65))
        );
        assert_eq!(
            parse_elevated_event("M 4"),
            Some(ButtonEvent::Mouse(MouseButton::X1))
        );
        assert_eq!(parse_elevated_event("M 9"), None);
        assert_eq!(parse_elevated_event("unknown"), None);
    }
}
