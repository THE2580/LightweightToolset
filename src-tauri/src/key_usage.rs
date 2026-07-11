use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
    sync::{mpsc, Arc, Mutex, OnceLock},
    thread,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};

use crate::input_monitor;

const KEY_USAGE_DIR: &str = "key_usage";
const KEY_USAGE_FILE: &str = "key_usage.json";
const SAVE_INTERVAL_MS: u64 = 10_000;

static KEY_USAGE: OnceLock<Mutex<KeyUsageManager>> = OnceLock::new();

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct KeyUsageStore {
    #[serde(default = "store_version")]
    version: u32,
    #[serde(default)]
    days: BTreeMap<String, BTreeMap<String, u64>>,
    #[serde(default)]
    hours: BTreeMap<String, BTreeMap<String, BTreeMap<String, u64>>>,
    #[serde(skip)]
    dirty: bool,
}

impl Default for KeyUsageStore {
    fn default() -> Self {
        Self {
            version: store_version(),
            days: BTreeMap::new(),
            hours: BTreeMap::new(),
            dirty: false,
        }
    }
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyUsageSnapshot {
    pub today: String,
    pub running: bool,
    pub storage_bytes: u64,
    pub days: BTreeMap<String, BTreeMap<String, u64>>,
    pub hours: BTreeMap<String, BTreeMap<String, BTreeMap<String, u64>>>,
}

struct KeyUsageWorker {
    stop: mpsc::Sender<()>,
    thread: thread::JoinHandle<()>,
    input_monitor: input_monitor::ButtonInputMonitor,
}

struct KeyUsageManager {
    path: PathBuf,
    store: Arc<Mutex<KeyUsageStore>>,
    worker: Option<KeyUsageWorker>,
}

pub fn init(config_dir: &Path) -> Result<(), String> {
    let dir = config_dir.join(KEY_USAGE_DIR);
    fs::create_dir_all(&dir).map_err(|error| format!("创建按键统计目录失败: {error}"))?;
    let path = dir.join(KEY_USAGE_FILE);
    let store = load_store(&path)?;
    KEY_USAGE
        .set(Mutex::new(KeyUsageManager {
            path,
            store: Arc::new(Mutex::new(store)),
            worker: None,
        }))
        .map_err(|_| "按键使用统计服务已初始化".to_owned())
}

pub fn relocate(config_dir: &Path) -> Result<(), String> {
    let manager_lock = KEY_USAGE.get().ok_or("按键使用统计服务未初始化")?;
    let mut manager = manager_lock.lock().map_err(|_| "按键使用统计服务不可用")?;
    let was_running = manager.worker.is_some();
    if let Some(worker) = manager.worker.take() {
        stop_worker(worker);
    }
    let dir = config_dir.join(KEY_USAGE_DIR);
    fs::create_dir_all(&dir).map_err(|error| format!("创建按键统计目录失败: {error}"))?;
    manager.path = dir.join(KEY_USAGE_FILE);
    save_store(&manager.path, &manager.store, true)?;
    drop(manager);
    if was_running {
        start()?;
    }
    Ok(())
}

pub fn start() -> Result<(), String> {
    let manager_lock = KEY_USAGE.get().ok_or("按键使用统计服务未初始化")?;
    let mut manager = manager_lock.lock().map_err(|_| "按键使用统计服务不可用")?;
    if manager.worker.is_some() {
        return Ok(());
    }

    let (input_monitor, button_receiver) = input_monitor::subscribe_button_down()?;
    let store = Arc::clone(&manager.store);
    let path = manager.path.clone();
    let (stop, stop_receiver) = mpsc::channel();
    let thread = match thread::Builder::new()
        .name("tool-key-usage".to_owned())
        .spawn(move || {
            let mut last_save_at = Instant::now();
            loop {
                if stop_receiver.try_recv().is_ok() {
                    let _ = save_store(&path, &store, true);
                    break;
                }
                match button_receiver.recv_timeout(Duration::from_millis(250)) {
                    Ok(event) => {
                        if let Ok(mut guard) = store.lock() {
                            add_button_press(&mut guard, event);
                        }
                    }
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    Err(mpsc::RecvTimeoutError::Timeout) => {}
                }
                if last_save_at.elapsed() >= Duration::from_millis(SAVE_INTERVAL_MS) {
                    let _ = save_store(&path, &store, false);
                    last_save_at = Instant::now();
                }
            }
        }) {
        Ok(thread) => thread,
        Err(error) => {
            input_monitor::unsubscribe_button_down(input_monitor);
            return Err(format!("启动按键使用统计失败: {error}"));
        }
    };

    manager.worker = Some(KeyUsageWorker {
        stop,
        thread,
        input_monitor,
    });
    Ok(())
}

pub fn stop() {
    let Some(manager_lock) = KEY_USAGE.get() else {
        return;
    };
    let Ok(mut manager) = manager_lock.lock() else {
        return;
    };
    if let Some(worker) = manager.worker.take() {
        stop_worker(worker);
    }
}

fn stop_worker(worker: KeyUsageWorker) {
    let _ = worker.stop.send(());
    let _ = worker.thread.join();
    input_monitor::unsubscribe_button_down(worker.input_monitor);
}

pub fn snapshot() -> Result<KeyUsageSnapshot, String> {
    let manager_lock = KEY_USAGE.get().ok_or("按键使用统计服务未初始化")?;
    let manager = manager_lock.lock().map_err(|_| "按键使用统计服务不可用")?;
    let store = manager.store.lock().map_err(|_| "按键使用统计数据不可用")?;
    Ok(KeyUsageSnapshot {
        today: local_day_hour().0,
        running: manager.worker.is_some(),
        storage_bytes: fs::metadata(&manager.path)
            .map(|metadata| metadata.len())
            .unwrap_or(0),
        days: store.days.clone(),
        hours: store.hours.clone(),
    })
}

pub fn clear() -> Result<KeyUsageSnapshot, String> {
    let manager_lock = KEY_USAGE.get().ok_or("按键使用统计服务未初始化")?;
    let manager = manager_lock.lock().map_err(|_| "按键使用统计服务不可用")?;
    {
        let mut store = manager.store.lock().map_err(|_| "按键使用统计数据不可用")?;
        store.days.clear();
        store.hours.clear();
        store.dirty = true;
    }
    save_store(&manager.path, &manager.store, true)?;
    drop(manager);
    snapshot()
}

fn add_button_press(store: &mut KeyUsageStore, event: input_monitor::ButtonEvent) {
    let key = button_name(event);
    let (day, hour) = local_day_hour();
    *store
        .days
        .entry(day.clone())
        .or_default()
        .entry(key.clone())
        .or_default() += 1;
    *store
        .hours
        .entry(day)
        .or_default()
        .entry(hour)
        .or_default()
        .entry(key)
        .or_default() += 1;
    store.dirty = true;
}

fn button_name(event: input_monitor::ButtonEvent) -> String {
    match event {
        input_monitor::ButtonEvent::Keyboard(virtual_key) => key_name(virtual_key),
        input_monitor::ButtonEvent::Mouse(button) => match button {
            input_monitor::MouseButton::Left => "MouseLeft",
            input_monitor::MouseButton::Right => "MouseRight",
            input_monitor::MouseButton::Middle => "MouseMiddle",
            input_monitor::MouseButton::X1 => "MouseX1",
            input_monitor::MouseButton::X2 => "MouseX2",
        }
        .to_owned(),
    }
}

fn key_name(virtual_key: u32) -> String {
    match virtual_key {
        0x30..=0x39 | 0x41..=0x5A => {
            return char::from_u32(virtual_key)
                .map(|value| value.to_string())
                .unwrap_or_else(|| format!("VK_{virtual_key:02X}"));
        }
        0x70..=0x87 => return format!("F{}", virtual_key - 0x6F),
        0x60..=0x69 => return format!("Num{}", virtual_key - 0x60),
        0x08 => "Backspace",
        0x09 => "Tab",
        0x0D => "Enter",
        0x10 | 0xA0 | 0xA1 => "Shift",
        0x11 | 0xA2 | 0xA3 => "Ctrl",
        0x12 | 0xA4 | 0xA5 => "Alt",
        0x14 => "CapsLock",
        0x1B => "Escape",
        0x20 => "Space",
        0x21 => "PageUp",
        0x22 => "PageDown",
        0x23 => "End",
        0x24 => "Home",
        0x25 => "ArrowLeft",
        0x26 => "ArrowUp",
        0x27 => "ArrowRight",
        0x28 => "ArrowDown",
        0x2C => "PrintScreen",
        0x2D => "Insert",
        0x2E => "Delete",
        0x5B | 0x5C => "Win",
        0x6A => "NumMultiply",
        0x6B => "NumAdd",
        0x6D => "NumSubtract",
        0x6E => "NumDecimal",
        0x6F => "NumDivide",
        0x90 => "NumLock",
        0x91 => "ScrollLock",
        0xBA => "Semicolon",
        0xBB => "Equal",
        0xBC => "Comma",
        0xBD => "Minus",
        0xBE => "Period",
        0xBF => "Slash",
        0xC0 => "Backquote",
        0xDB => "BracketLeft",
        0xDC => "Backslash",
        0xDD => "BracketRight",
        0xDE => "Quote",
        _ => return format!("VK_{virtual_key:02X}"),
    }
    .to_owned()
}

fn load_store(path: &Path) -> Result<KeyUsageStore, String> {
    if !path.exists() {
        return Ok(KeyUsageStore::default());
    }
    let raw = fs::read_to_string(path).map_err(|error| format!("读取按键统计失败: {error}"))?;
    serde_json::from_str(&raw).map_err(|error| format!("解析按键统计失败: {error}"))
}

fn save_store(path: &Path, store: &Arc<Mutex<KeyUsageStore>>, force: bool) -> Result<(), String> {
    let mut guard = store.lock().map_err(|_| "按键使用统计数据不可用")?;
    if !force && !guard.dirty {
        return Ok(());
    }
    guard.version = store_version();
    let content = serde_json::to_string_pretty(&*guard)
        .map_err(|error| format!("序列化按键统计失败: {error}"))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建按键统计目录失败: {error}"))?;
    }
    fs::write(path, content).map_err(|error| format!("保存按键统计失败: {error}"))?;
    guard.dirty = false;
    Ok(())
}

fn store_version() -> u32 {
    1
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
fn local_day_hour() -> (String, String) {
    ("1970-01-01".to_owned(), "00".to_owned())
}

#[cfg(test)]
mod tests {
    use super::{add_button_press, button_name, key_name, KeyUsageStore};
    use crate::input_monitor::{ButtonEvent, MouseButton};

    #[test]
    fn normalizes_virtual_keys_without_recording_text() {
        assert_eq!(key_name(0x41), "A");
        assert_eq!(key_name(0x31), "1");
        assert_eq!(key_name(0x61), "Num1");
        assert_eq!(key_name(0xA0), "Shift");
        assert_eq!(key_name(0x70), "F1");
        assert_eq!(key_name(0xFE), "VK_FE");
    }

    #[test]
    fn aggregates_key_presses_without_storing_sequences() {
        let mut store = KeyUsageStore::default();
        add_button_press(&mut store, ButtonEvent::Keyboard(0x41));
        add_button_press(&mut store, ButtonEvent::Keyboard(0x41));
        add_button_press(&mut store, ButtonEvent::Mouse(MouseButton::Left));
        assert_eq!(store.days.values().next().unwrap().get("A"), Some(&2));
        assert_eq!(
            store.days.values().next().unwrap().get("MouseLeft"),
            Some(&1),
        );
        assert_eq!(
            store
                .hours
                .values()
                .next()
                .unwrap()
                .values()
                .next()
                .unwrap()
                .get("A"),
            Some(&2),
        );
    }

    #[test]
    fn names_mouse_buttons_separately_from_keyboard_keys() {
        assert_eq!(
            button_name(ButtonEvent::Mouse(MouseButton::Right)),
            "MouseRight",
        );
        assert_eq!(button_name(ButtonEvent::Keyboard(0x41)), "A");
    }
}
