use std::{
    collections::{BTreeMap, BTreeSet, HashSet},
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicU64, Ordering},
        mpsc, Arc, Mutex, OnceLock,
    },
    thread,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};

const APP_USAGE_DIR: &str = "app_usage";
const APP_USAGE_FILE: &str = "app_usage.json";
const SAMPLE_INTERVAL_MS: u64 = 1_000;
const SAVE_INTERVAL_MS: u64 = 10_000;
const DEFAULT_AFK_THRESHOLD_SEC: u32 = 300;

static APP_USAGE: OnceLock<Mutex<AppUsageManager>> = OnceLock::new();
static LAST_PHYSICAL_INPUT_MS: AtomicU64 = AtomicU64::new(0);

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUsageSettings {
    #[serde(default = "default_afk_threshold_sec")]
    pub afk_threshold_sec: u32,
}

impl Default for AppUsageSettings {
    fn default() -> Self {
        Self {
            afk_threshold_sec: DEFAULT_AFK_THRESHOLD_SEC,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct AppUsageStore {
    #[serde(default = "store_version")]
    version: u32,
    #[serde(default)]
    settings: AppUsageSettings,
    #[serde(default)]
    aliases: BTreeMap<String, String>,
    #[serde(default)]
    disabled_processes: BTreeSet<String>,
    #[serde(default)]
    days: BTreeMap<String, BTreeMap<String, f64>>,
    #[serde(default)]
    hours: BTreeMap<String, BTreeMap<String, BTreeMap<String, f64>>>,
    #[serde(skip)]
    dirty: bool,
}

impl Default for AppUsageStore {
    fn default() -> Self {
        Self {
            version: store_version(),
            settings: AppUsageSettings::default(),
            aliases: BTreeMap::new(),
            disabled_processes: BTreeSet::new(),
            days: BTreeMap::new(),
            hours: BTreeMap::new(),
            dirty: false,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUsageSettingsPatch {
    pub afk_threshold_sec: Option<u32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUsageProcessPatch {
    pub process_name: String,
    pub alias: Option<String>,
    pub monitored: Option<bool>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppUsageSnapshot {
    pub today: String,
    pub active_process: Option<String>,
    pub is_afk: bool,
    pub running: bool,
    pub storage_bytes: u64,
    pub settings: AppUsageSettings,
    pub aliases: BTreeMap<String, String>,
    pub disabled_processes: Vec<String>,
    pub days: BTreeMap<String, BTreeMap<String, f64>>,
    pub hours: BTreeMap<String, BTreeMap<String, BTreeMap<String, f64>>>,
}

struct AppUsageRuntime {
    active_process: Option<String>,
    is_afk: bool,
}

struct AppUsageWorker {
    stop: mpsc::Sender<()>,
    thread: thread::JoinHandle<()>,
    input_monitor: PhysicalInputMonitor,
}

#[cfg(target_os = "windows")]
struct PhysicalInputMonitor {
    thread_id: u32,
    thread: thread::JoinHandle<()>,
}

#[cfg(not(target_os = "windows"))]
struct PhysicalInputMonitor;

struct AppUsageManager {
    path: PathBuf,
    store: Arc<Mutex<AppUsageStore>>,
    runtime: Arc<Mutex<AppUsageRuntime>>,
    worker: Option<AppUsageWorker>,
}

pub fn init(config_dir: &Path) -> Result<(), String> {
    let dir = config_dir.join(APP_USAGE_DIR);
    fs::create_dir_all(&dir).map_err(|error| format!("创建软件统计目录失败: {error}"))?;
    let path = dir.join(APP_USAGE_FILE);
    let store = load_store(&path)?;
    let manager = AppUsageManager {
        path,
        store: Arc::new(Mutex::new(store)),
        runtime: Arc::new(Mutex::new(AppUsageRuntime {
            active_process: None,
            is_afk: false,
        })),
        worker: None,
    };
    APP_USAGE
        .set(Mutex::new(manager))
        .map_err(|_| "软件使用统计服务已初始化".to_owned())
}

pub fn relocate(config_dir: &Path) -> Result<(), String> {
    let manager_lock = APP_USAGE.get().ok_or("软件使用统计服务未初始化")?;
    let mut manager = manager_lock.lock().map_err(|_| "软件使用统计服务不可用")?;
    let was_running = manager.worker.is_some();
    if let Some(worker) = manager.worker.take() {
        stop_worker(worker);
    }

    let dir = config_dir.join(APP_USAGE_DIR);
    fs::create_dir_all(&dir).map_err(|error| format!("创建软件统计目录失败: {error}"))?;
    manager.path = dir.join(APP_USAGE_FILE);
    save_store(&manager.path, &manager.store, true)?;
    drop(manager);

    if was_running {
        start()?;
    }
    Ok(())
}

pub fn start() -> Result<(), String> {
    let manager_lock = APP_USAGE.get().ok_or("软件使用统计服务未初始化")?;
    let mut manager = manager_lock.lock().map_err(|_| "软件使用统计服务不可用")?;
    if manager.worker.is_some() {
        return Ok(());
    }

    let store = Arc::clone(&manager.store);
    let runtime = Arc::clone(&manager.runtime);
    let path = manager.path.clone();
    let ignored = ignored_processes();
    let input_monitor = start_physical_input_monitor()?;
    let (stop, receiver) = mpsc::channel();
    let thread = thread::Builder::new()
        .name("tool-app-usage".to_owned())
        .spawn(move || {
            let mut last_sample_at = Instant::now();
            let mut last_save_at = Instant::now();
            loop {
                if receiver
                    .recv_timeout(Duration::from_millis(SAMPLE_INTERVAL_MS))
                    .is_ok()
                {
                    let _ = save_store(&path, &store, true);
                    break;
                }

                let now = Instant::now();
                let elapsed_sec = now.duration_since(last_sample_at).as_secs_f64();
                last_sample_at = now;

                let active_process = get_foreground_process_name()
                    .filter(|name| !ignored.contains(&name.to_ascii_lowercase()))
                    .filter(|name| {
                        store
                            .lock()
                            .map(|guard| !guard.disabled_processes.contains(name))
                            .unwrap_or(true)
                    });
                let afk_threshold_sec = store
                    .lock()
                    .map(|guard| guard.settings.afk_threshold_sec)
                    .unwrap_or(DEFAULT_AFK_THRESHOLD_SEC);
                let is_afk = physical_idle_seconds() >= afk_threshold_sec;

                if let Ok(mut state) = runtime.lock() {
                    state.active_process = active_process.clone();
                    state.is_afk = is_afk;
                }

                if elapsed_sec > 0.0 && elapsed_sec <= 5.0 && !is_afk {
                    if let Some(process_name) = active_process {
                        if let Ok(mut guard) = store.lock() {
                            add_usage(&mut guard, process_name, elapsed_sec);
                        }
                    }
                }

                if last_save_at.elapsed() >= Duration::from_millis(SAVE_INTERVAL_MS) {
                    let _ = save_store(&path, &store, false);
                    last_save_at = Instant::now();
                }
            }
        })
        .map_err(|error| format!("启动软件使用统计失败: {error}"))?;

    manager.worker = Some(AppUsageWorker {
        stop,
        thread,
        input_monitor,
    });
    Ok(())
}

pub fn stop() {
    let Some(manager_lock) = APP_USAGE.get() else {
        return;
    };
    let Ok(mut manager) = manager_lock.lock() else {
        return;
    };
    if let Some(worker) = manager.worker.take() {
        stop_worker(worker);
    }
    if let Ok(mut state) = manager.runtime.lock() {
        state.active_process = None;
        state.is_afk = false;
    };
}

fn stop_worker(worker: AppUsageWorker) {
    let _ = worker.stop.send(());
    let _ = worker.thread.join();
    worker.input_monitor.stop();
}

pub fn snapshot() -> Result<AppUsageSnapshot, String> {
    let manager_lock = APP_USAGE.get().ok_or("软件使用统计服务未初始化")?;
    let manager = manager_lock.lock().map_err(|_| "软件使用统计服务不可用")?;
    let store = manager.store.lock().map_err(|_| "软件使用统计数据不可用")?;
    let runtime = manager
        .runtime
        .lock()
        .map_err(|_| "软件使用统计状态不可用")?;
    let storage_bytes = fs::metadata(&manager.path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    Ok(AppUsageSnapshot {
        today: local_day_string(),
        active_process: runtime.active_process.clone(),
        is_afk: runtime.is_afk,
        running: manager.worker.is_some(),
        storage_bytes,
        settings: store.settings.clone(),
        aliases: store.aliases.clone(),
        disabled_processes: store.disabled_processes.iter().cloned().collect(),
        days: store.days.clone(),
        hours: store.hours.clone(),
    })
}

pub fn update_settings(patch: AppUsageSettingsPatch) -> Result<AppUsageSnapshot, String> {
    let manager_lock = APP_USAGE.get().ok_or("软件使用统计服务未初始化")?;
    let manager = manager_lock.lock().map_err(|_| "软件使用统计服务不可用")?;
    {
        let mut store = manager.store.lock().map_err(|_| "软件使用统计数据不可用")?;
        if let Some(afk_threshold_sec) = patch.afk_threshold_sec {
            store.settings.afk_threshold_sec = afk_threshold_sec.clamp(30, 3600);
            store.dirty = true;
        }
    }
    save_store(&manager.path, &manager.store, true)?;
    drop(manager);
    snapshot()
}

pub fn update_process(patch: AppUsageProcessPatch) -> Result<AppUsageSnapshot, String> {
    let process_name = patch.process_name.trim().to_owned();
    if process_name.is_empty() {
        return Err("process name cannot be empty".to_owned());
    }

    let manager_lock = APP_USAGE
        .get()
        .ok_or("app usage service is not initialized")?;
    let manager = manager_lock
        .lock()
        .map_err(|_| "app usage service is unavailable")?;
    {
        let mut store = manager
            .store
            .lock()
            .map_err(|_| "app usage data is unavailable")?;
        if let Some(alias) = patch.alias {
            let alias = alias.trim();
            if alias.is_empty() {
                store.aliases.remove(&process_name);
            } else {
                store.aliases.insert(process_name.clone(), alias.to_owned());
            }
            store.dirty = true;
        }
        if let Some(monitored) = patch.monitored {
            if monitored {
                store.disabled_processes.remove(&process_name);
            } else {
                store.disabled_processes.insert(process_name.clone());
            }
            store.dirty = true;
        }
    }
    if patch.monitored == Some(false) {
        if let Ok(mut state) = manager.runtime.lock() {
            if state.active_process.as_deref() == Some(process_name.as_str()) {
                state.active_process = None;
            }
        }
    }
    save_store(&manager.path, &manager.store, true)?;
    drop(manager);
    snapshot()
}

pub fn clear() -> Result<AppUsageSnapshot, String> {
    let manager_lock = APP_USAGE.get().ok_or("软件使用统计服务未初始化")?;
    let manager = manager_lock.lock().map_err(|_| "软件使用统计服务不可用")?;
    {
        let mut store = manager.store.lock().map_err(|_| "软件使用统计数据不可用")?;
        store.days.clear();
        store.hours.clear();
        store.dirty = true;
    }
    save_store(&manager.path, &manager.store, true)?;
    drop(manager);
    snapshot()
}

fn add_usage(store: &mut AppUsageStore, process_name: String, seconds: f64) {
    if seconds <= 0.0 {
        return;
    }
    let (day, hour) = local_day_hour();
    let apps = store.days.entry(day.clone()).or_default();
    let next = apps.get(&process_name).copied().unwrap_or(0.0) + seconds;
    apps.insert(process_name.clone(), round_seconds(next));
    let hour_apps = store.hours.entry(day).or_default().entry(hour).or_default();
    let next_hour = hour_apps.get(&process_name).copied().unwrap_or(0.0) + seconds;
    hour_apps.insert(process_name, round_seconds(next_hour));
    store.dirty = true;
}

fn save_store(path: &Path, store: &Arc<Mutex<AppUsageStore>>, force: bool) -> Result<(), String> {
    let mut store = store.lock().map_err(|_| "软件使用统计数据不可用")?;
    if !force && !store.dirty {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建软件统计目录失败: {error}"))?;
    }
    let raw = serde_json::to_string_pretty(&*store)
        .map_err(|error| format!("序列化软件统计数据失败: {error}"))?;
    fs::write(path, raw).map_err(|error| format!("保存软件统计数据失败: {error}"))?;
    store.dirty = false;
    Ok(())
}

fn load_store(path: &Path) -> Result<AppUsageStore, String> {
    if !path.exists() {
        return Ok(AppUsageStore::default());
    }
    let raw = fs::read_to_string(path).map_err(|error| format!("读取软件统计数据失败: {error}"))?;
    let mut store: AppUsageStore = serde_json::from_str(&raw).unwrap_or_default();
    store.version = store_version();
    store.dirty = false;
    Ok(store)
}

fn ignored_processes() -> HashSet<String> {
    let mut ignored = HashSet::from(["lockapp.exe".to_owned(), "logonui.exe".to_owned()]);
    if let Ok(exe) = std::env::current_exe() {
        if let Some(name) = exe.file_name().and_then(|name| name.to_str()) {
            ignored.insert(name.to_ascii_lowercase());
        }
    }
    ignored
}

fn round_seconds(value: f64) -> f64 {
    (value * 10.0).round() / 10.0
}

fn default_afk_threshold_sec() -> u32 {
    DEFAULT_AFK_THRESHOLD_SEC
}

fn store_version() -> u32 {
    2
}

#[cfg(target_os = "windows")]
fn local_day_string() -> String {
    local_day_hour().0
}

#[cfg(target_os = "windows")]
fn local_day_hour() -> (String, String) {
    use windows_sys::Win32::{Foundation::SYSTEMTIME, System::SystemInformation::GetLocalTime};
    unsafe {
        let mut time = std::mem::zeroed::<SYSTEMTIME>();
        GetLocalTime(&mut time);
        (
            format!("{:04}-{:02}-{:02}", time.wYear, time.wMonth, time.wDay),
            format!("{:02}", time.wHour),
        )
    }
}

#[cfg(not(target_os = "windows"))]
fn local_day_string() -> String {
    local_day_hour().0
}

#[cfg(not(target_os = "windows"))]
fn local_day_hour() -> (String, String) {
    ("1970-01-01".to_owned(), "00".to_owned())
}

#[cfg(target_os = "windows")]
fn start_physical_input_monitor() -> Result<PhysicalInputMonitor, String> {
    use windows_sys::Win32::{
        System::Threading::GetCurrentThreadId,
        System::SystemInformation::GetTickCount64,
        UI::WindowsAndMessaging::{
            GetMessageW, PeekMessageW, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx,
            DispatchMessageW, MSG, PM_NOREMOVE, WH_KEYBOARD_LL, WH_MOUSE_LL,
        },
    };

    let (ready_tx, ready_rx) = mpsc::sync_channel(1);
    let thread = thread::Builder::new()
        .name("tool-app-usage-input".to_owned())
        .spawn(move || unsafe {
            let mut message = std::mem::zeroed::<MSG>();
            PeekMessageW(&mut message, std::ptr::null_mut(), 0, 0, PM_NOREMOVE);

            let keyboard_hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(physical_keyboard_hook), std::ptr::null_mut(), 0);
            if keyboard_hook.is_null() {
                let _ = ready_tx.send(Err("启动键盘空闲监听失败".to_owned()));
                return;
            }
            let mouse_hook = SetWindowsHookExW(WH_MOUSE_LL, Some(physical_mouse_hook), std::ptr::null_mut(), 0);
            if mouse_hook.is_null() {
                UnhookWindowsHookEx(keyboard_hook);
                let _ = ready_tx.send(Err("启动鼠标空闲监听失败".to_owned()));
                return;
            }

            LAST_PHYSICAL_INPUT_MS.store(GetTickCount64(), Ordering::Release);
            let _ = ready_tx.send(Ok(GetCurrentThreadId()));
            while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
            UnhookWindowsHookEx(mouse_hook);
            UnhookWindowsHookEx(keyboard_hook);
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
    Ok(PhysicalInputMonitor { thread_id, thread })
}

#[cfg(not(target_os = "windows"))]
fn start_physical_input_monitor() -> Result<PhysicalInputMonitor, String> {
    Ok(PhysicalInputMonitor)
}

#[cfg(target_os = "windows")]
impl PhysicalInputMonitor {
    fn stop(self) {
        use windows_sys::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT};
        unsafe {
            PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0);
        }
        let _ = self.thread.join();
    }
}

#[cfg(not(target_os = "windows"))]
impl PhysicalInputMonitor {
    fn stop(self) {}
}

#[cfg(target_os = "windows")]
fn physical_idle_seconds() -> u32 {
    use windows_sys::Win32::System::SystemInformation::GetTickCount64;
    let last_input_ms = LAST_PHYSICAL_INPUT_MS.load(Ordering::Acquire);
    if last_input_ms == 0 {
        return 0;
    }
    ((unsafe { GetTickCount64() }).saturating_sub(last_input_ms) / 1000).min(u32::MAX as u64) as u32
}

#[cfg(not(target_os = "windows"))]
fn physical_idle_seconds() -> u32 {
    0
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn physical_keyboard_hook(
    code: i32,
    w_param: windows_sys::Win32::Foundation::WPARAM,
    l_param: windows_sys::Win32::Foundation::LPARAM,
) -> windows_sys::Win32::Foundation::LRESULT {
    use windows_sys::Win32::{
        System::SystemInformation::GetTickCount64,
        UI::WindowsAndMessaging::{CallNextHookEx, KBDLLHOOKSTRUCT, LLKHF_INJECTED, WM_KEYDOWN, WM_SYSKEYDOWN},
    };
    if code == 0 && (w_param as u32 == WM_KEYDOWN || w_param as u32 == WM_SYSKEYDOWN) && l_param != 0 {
        let input = &*(l_param as *const KBDLLHOOKSTRUCT);
        if input.flags & LLKHF_INJECTED == 0 {
            LAST_PHYSICAL_INPUT_MS.store(GetTickCount64(), Ordering::Release);
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, w_param, l_param)
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
            CallNextHookEx, MSLLHOOKSTRUCT, LLMHF_INJECTED, WM_LBUTTONDOWN, WM_MBUTTONDOWN,
            WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_RBUTTONDOWN, WM_XBUTTONDOWN,
        },
    };
    let message = w_param as u32;
    if code == 0
        && matches!(message, WM_MOUSEMOVE | WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_MOUSEWHEEL | WM_XBUTTONDOWN)
        && l_param != 0
    {
        let input = &*(l_param as *const MSLLHOOKSTRUCT);
        if input.flags & LLMHF_INJECTED == 0 {
            LAST_PHYSICAL_INPUT_MS.store(GetTickCount64(), Ordering::Release);
        }
    }
    CallNextHookEx(std::ptr::null_mut(), code, w_param, l_param)
}

#[cfg(target_os = "windows")]
fn get_foreground_process_name() -> Option<String> {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, MAX_PATH},
        System::Threading::{
            GetCurrentProcessId, OpenProcess, QueryFullProcessImageNameW,
            PROCESS_QUERY_LIMITED_INFORMATION,
        },
        UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId},
    };

    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.is_null() {
            return None;
        }
        let mut process_id = 0u32;
        GetWindowThreadProcessId(hwnd, &mut process_id);
        if process_id == 0 || process_id == GetCurrentProcessId() {
            return None;
        }
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, process_id);
        if handle.is_null() {
            return None;
        }
        let mut buffer = [0u16; MAX_PATH as usize];
        let mut size = buffer.len() as u32;
        let ok = QueryFullProcessImageNameW(handle, 0, buffer.as_mut_ptr(), &mut size);
        CloseHandle(handle);
        if ok == 0 || size == 0 {
            return None;
        }
        let path = String::from_utf16_lossy(&buffer[..size as usize]);
        Path::new(&path)
            .file_name()
            .and_then(|name| name.to_str())
            .map(str::to_owned)
    }
}

#[cfg(not(target_os = "windows"))]
fn get_foreground_process_name() -> Option<String> {
    None
}
