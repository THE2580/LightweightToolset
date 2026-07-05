use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    sync::{mpsc, Arc, Mutex, OnceLock},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

const TIMER_DIR: &str = "timer";
const TIMER_FILE: &str = "timer.json";
const TICK_INTERVAL_MS: u64 = 500;
const MAX_TIMERS: usize = 20;
const DEFAULT_COUNTDOWN_SECONDS: u64 = 25 * 60;
const MAX_COUNTDOWN_SECONDS: u64 = 99 * 60 * 60 + 59 * 60 + 59;

static TIMER: OnceLock<Mutex<TimerManager>> = OnceLock::new();

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TimerKind {
    Stopwatch,
    Countdown,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub enum TimerStatus {
    Paused,
    Running,
    Finished,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimerEntry {
    pub id: String,
    pub name: String,
    pub note: String,
    pub kind: TimerKind,
    pub status: TimerStatus,
    #[serde(default)]
    pub elapsed_ms: u128,
    #[serde(default)]
    pub duration_ms: Option<u128>,
    #[serde(default)]
    pub started_at_ms: Option<u128>,
    #[serde(default)]
    pub finished_at_ms: Option<u128>,
    #[serde(default = "default_notifications_enabled")]
    pub notifications_enabled: bool,
    #[serde(default)]
    pub order: u32,
    #[serde(default)]
    pub window: TimerWindowState,
    #[serde(default)]
    notified_at_ms: Option<u128>,
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimerWindowState {
    #[serde(default)]
    pub compact_open: bool,
    #[serde(default)]
    pub detached_open: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct TimerStore {
    #[serde(default = "store_version")]
    version: u32,
    #[serde(default)]
    timers: Vec<TimerEntry>,
    #[serde(skip)]
    dirty: bool,
}

impl Default for TimerStore {
    fn default() -> Self {
        Self {
            version: store_version(),
            timers: Vec::new(),
            dirty: false,
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimerCreateInput {
    pub kind: TimerKind,
    pub name: Option<String>,
    pub note: Option<String>,
    pub duration_seconds: Option<u64>,
    pub notifications_enabled: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimerUpdateInput {
    pub id: String,
    pub name: Option<String>,
    pub note: Option<String>,
    pub duration_seconds: Option<u64>,
    pub notifications_enabled: Option<bool>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TimerReorderInput {
    pub ids: Vec<String>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimerSnapshot {
    pub running: bool,
    pub storage_bytes: u64,
    pub timers: Vec<TimerView>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TimerView {
    pub id: String,
    pub name: String,
    pub note: String,
    pub kind: TimerKind,
    pub status: TimerStatus,
    pub elapsed_ms: u128,
    pub duration_ms: Option<u128>,
    pub remaining_ms: Option<u128>,
    pub progress: f64,
    pub notifications_enabled: bool,
    pub order: u32,
    pub finished_at_ms: Option<u128>,
}

struct TimerWorker {
    stop: mpsc::Sender<()>,
    thread: thread::JoinHandle<()>,
}

struct TimerManager {
    path: PathBuf,
    store: Arc<Mutex<TimerStore>>,
    worker: Option<TimerWorker>,
}

pub fn init(config_dir: &Path) -> Result<(), String> {
    let dir = config_dir.join(TIMER_DIR);
    fs::create_dir_all(&dir).map_err(|error| format!("创建计时器目录失败: {error}"))?;
    let path = dir.join(TIMER_FILE);
    let store = load_store(&path)?;
    let manager = TimerManager {
        path,
        store: Arc::new(Mutex::new(store)),
        worker: None,
    };
    TIMER
        .set(Mutex::new(manager))
        .map_err(|_| "计时器服务已初始化".to_owned())
}

pub fn relocate(config_dir: &Path) -> Result<(), String> {
    let manager_lock = TIMER.get().ok_or("计时器服务未初始化")?;
    let mut manager = manager_lock.lock().map_err(|_| "计时器服务不可用")?;
    let was_running = manager.worker.is_some();
    if let Some(worker) = manager.worker.take() {
        let _ = worker.stop.send(());
        let _ = worker.thread.join();
    }

    let dir = config_dir.join(TIMER_DIR);
    fs::create_dir_all(&dir).map_err(|error| format!("创建计时器目录失败: {error}"))?;
    manager.path = dir.join(TIMER_FILE);
    save_store(&manager.path, &manager.store, true)?;
    drop(manager);

    if was_running {
        start()?;
    }
    Ok(())
}

pub fn start() -> Result<(), String> {
    let manager_lock = TIMER.get().ok_or("计时器服务未初始化")?;
    let mut manager = manager_lock.lock().map_err(|_| "计时器服务不可用")?;
    if manager.worker.is_some() {
        return Ok(());
    }

    let store = Arc::clone(&manager.store);
    let path = manager.path.clone();
    let (stop, receiver) = mpsc::channel();
    let thread = thread::Builder::new()
        .name("tool-timer".to_owned())
        .spawn(move || loop {
            if receiver
                .recv_timeout(Duration::from_millis(TICK_INTERVAL_MS))
                .is_ok()
            {
                let _ = save_store(&path, &store, true);
                break;
            }
            let mut finished = Vec::new();
            if let Ok(mut guard) = store.lock() {
                let now = now_ms();
                let mut dirty = false;
                for timer in guard.timers.iter_mut() {
                    if timer.status == TimerStatus::Running
                        && matches!(timer.kind, TimerKind::Countdown)
                    {
                        let elapsed = timer_elapsed_ms(timer, now);
                        if timer.duration_ms.is_some_and(|duration| elapsed >= duration) {
                            timer.elapsed_ms = timer.duration_ms.unwrap_or(elapsed);
                            timer.started_at_ms = None;
                            timer.status = TimerStatus::Finished;
                            timer.finished_at_ms = Some(now);
                            dirty = true;
                            if timer.notifications_enabled && timer.notified_at_ms.is_none() {
                                timer.notified_at_ms = Some(now);
                                finished.push(timer.name.clone());
                            }
                        }
                    }
                }
                guard.dirty = guard.dirty || dirty;
            }
            for name in finished {
                notify_timer_finished(name);
            }
            let _ = save_store(&path, &store, false);
        })
        .map_err(|error| format!("启动计时器失败: {error}"))?;

    manager.worker = Some(TimerWorker { stop, thread });
    Ok(())
}

pub fn stop() -> usize {
    let Some(manager_lock) = TIMER.get() else {
        return 0;
    };
    let Ok(mut manager) = manager_lock.lock() else {
        return 0;
    };
    let path = manager.path.clone();
    let store = Arc::clone(&manager.store);
    let mut paused_count = 0;
    if let Ok(mut store) = store.lock() {
        paused_count = pause_running_entries(&mut store);
    }
    if let Some(worker) = manager.worker.take() {
        let _ = worker.stop.send(());
        let _ = worker.thread.join();
    }
    let _ = save_store(&path, &store, true);
    paused_count
}

pub fn snapshot() -> Result<TimerSnapshot, String> {
    let manager_lock = TIMER.get().ok_or("计时器服务未初始化")?;
    let manager = manager_lock.lock().map_err(|_| "计时器服务不可用")?;
    let store = manager.store.lock().map_err(|_| "计时器数据不可用")?;
    let storage_bytes = fs::metadata(&manager.path)
        .map(|metadata| metadata.len())
        .unwrap_or(0);
    Ok(TimerSnapshot {
        running: manager.worker.is_some(),
        storage_bytes,
        timers: timer_views(&store.timers),
    })
}

pub fn create(input: TimerCreateInput) -> Result<TimerSnapshot, String> {
    let manager_lock = TIMER.get().ok_or("计时器服务未初始化")?;
    let manager = manager_lock.lock().map_err(|_| "计时器服务不可用")?;
    {
        let mut store = manager.store.lock().map_err(|_| "计时器数据不可用")?;
        if store.timers.len() >= MAX_TIMERS {
            return Err(format!("最多只能创建 {MAX_TIMERS} 个计时器"));
        }
        let order = store
            .timers
            .iter()
            .map(|timer| timer.order)
            .max()
            .unwrap_or(0)
            .saturating_add(1);
        let now = now_ms();
        let kind = input.kind;
        let duration_ms = match kind {
            TimerKind::Stopwatch => None,
            TimerKind::Countdown => Some(
                input
                    .duration_seconds
                    .unwrap_or(DEFAULT_COUNTDOWN_SECONDS)
                    .clamp(1, MAX_COUNTDOWN_SECONDS) as u128
                    * 1000,
            ),
        };
        let default_name = match kind {
            TimerKind::Stopwatch => "正计时",
            TimerKind::Countdown => "倒计时",
        };
        let notifications_enabled =
            matches!(kind, TimerKind::Countdown) && input.notifications_enabled.unwrap_or(true);
        store.timers.push(TimerEntry {
            id: format!("{now:x}-{order:x}"),
            name: clean_text(input.name).unwrap_or_else(|| default_name.to_owned()),
            note: clean_text(input.note).unwrap_or_default(),
            kind,
            status: TimerStatus::Paused,
            elapsed_ms: 0,
            duration_ms,
            started_at_ms: None,
            finished_at_ms: None,
            notifications_enabled,
            order,
            window: TimerWindowState::default(),
            notified_at_ms: None,
        });
        store.dirty = true;
    }
    save_store(&manager.path, &manager.store, true)?;
    drop(manager);
    snapshot()
}

pub fn update(input: TimerUpdateInput) -> Result<TimerSnapshot, String> {
    let manager_lock = TIMER.get().ok_or("计时器服务未初始化")?;
    let manager = manager_lock.lock().map_err(|_| "计时器服务不可用")?;
    {
        let mut store = manager.store.lock().map_err(|_| "计时器数据不可用")?;
        let timer = store
            .timers
            .iter_mut()
            .find(|timer| timer.id == input.id)
            .ok_or("计时器不存在")?;
        if let Some(name) = input.name {
            if let Some(name) = clean_text(Some(name)) {
                timer.name = name;
            }
        }
        if input.note.is_some() {
            timer.note = clean_text(input.note).unwrap_or_default();
        }
        if let Some(notifications_enabled) = input.notifications_enabled {
            timer.notifications_enabled =
                matches!(timer.kind, TimerKind::Countdown) && notifications_enabled;
        }
        if matches!(timer.kind, TimerKind::Stopwatch) {
            timer.notifications_enabled = false;
        }
        if let Some(duration_seconds) = input.duration_seconds {
            if matches!(timer.kind, TimerKind::Countdown) {
                let duration_ms = duration_seconds.clamp(1, MAX_COUNTDOWN_SECONDS) as u128 * 1000;
                timer.duration_ms = Some(duration_ms);
                timer.elapsed_ms = timer.elapsed_ms.min(duration_ms);
                if timer.status == TimerStatus::Finished && timer.elapsed_ms < duration_ms {
                    timer.status = TimerStatus::Paused;
                    timer.finished_at_ms = None;
                    timer.notified_at_ms = None;
                }
            }
        }
        store.dirty = true;
    }
    save_store(&manager.path, &manager.store, true)?;
    drop(manager);
    snapshot()
}

pub fn start_timer(id: String) -> Result<TimerSnapshot, String> {
    let manager_lock = TIMER.get().ok_or("计时器服务未初始化")?;
    let manager = manager_lock.lock().map_err(|_| "计时器服务不可用")?;
    {
        let mut store = manager.store.lock().map_err(|_| "计时器数据不可用")?;
        let timer = store
            .timers
            .iter_mut()
            .find(|timer| timer.id == id)
            .ok_or("计时器不存在")?;
        if timer.status != TimerStatus::Running {
            let now = now_ms();
            if timer.status == TimerStatus::Finished {
                timer.elapsed_ms = 0;
                timer.finished_at_ms = None;
                timer.notified_at_ms = None;
            }
            timer.started_at_ms = Some(now);
            timer.status = TimerStatus::Running;
            store.dirty = true;
        }
    }
    save_store(&manager.path, &manager.store, true)?;
    drop(manager);
    snapshot()
}

pub fn pause_timer(id: String) -> Result<TimerSnapshot, String> {
    let manager_lock = TIMER.get().ok_or("计时器服务未初始化")?;
    let manager = manager_lock.lock().map_err(|_| "计时器服务不可用")?;
    {
        let mut store = manager.store.lock().map_err(|_| "计时器数据不可用")?;
        let timer = store
            .timers
            .iter_mut()
            .find(|timer| timer.id == id)
            .ok_or("计时器不存在")?;
        if timer.status == TimerStatus::Running {
            timer.elapsed_ms = timer_elapsed_ms(timer, now_ms());
            timer.started_at_ms = None;
            timer.status = TimerStatus::Paused;
            store.dirty = true;
        }
    }
    save_store(&manager.path, &manager.store, true)?;
    drop(manager);
    snapshot()
}

pub fn pause_running_timers() -> Result<TimerSnapshot, String> {
    let manager_lock = TIMER.get().ok_or("计时器服务未初始化")?;
    let manager = manager_lock.lock().map_err(|_| "计时器服务不可用")?;
    {
        let mut store = manager.store.lock().map_err(|_| "计时器数据不可用")?;
        pause_running_entries(&mut store);
    }
    save_store(&manager.path, &manager.store, true)?;
    drop(manager);
    snapshot()
}

pub fn reset_timer(id: String) -> Result<TimerSnapshot, String> {
    let manager_lock = TIMER.get().ok_or("计时器服务未初始化")?;
    let manager = manager_lock.lock().map_err(|_| "计时器服务不可用")?;
    {
        let mut store = manager.store.lock().map_err(|_| "计时器数据不可用")?;
        let timer = store
            .timers
            .iter_mut()
            .find(|timer| timer.id == id)
            .ok_or("计时器不存在")?;
        timer.elapsed_ms = 0;
        timer.started_at_ms = None;
        timer.status = TimerStatus::Paused;
        timer.finished_at_ms = None;
        timer.notified_at_ms = None;
        store.dirty = true;
    }
    save_store(&manager.path, &manager.store, true)?;
    drop(manager);
    snapshot()
}

pub fn reset_active_timers() -> Result<TimerSnapshot, String> {
    let manager_lock = TIMER.get().ok_or("计时器服务未初始化")?;
    let manager = manager_lock.lock().map_err(|_| "计时器服务不可用")?;
    {
        let mut store = manager.store.lock().map_err(|_| "计时器数据不可用")?;
        let mut changed = false;
        for timer in store.timers.iter_mut() {
            if timer.status == TimerStatus::Running
                || timer.status == TimerStatus::Finished
                || timer.elapsed_ms > 0
            {
                timer.elapsed_ms = 0;
                timer.started_at_ms = None;
                timer.status = TimerStatus::Paused;
                timer.finished_at_ms = None;
                timer.notified_at_ms = None;
                changed = true;
            }
        }
        if changed {
            store.dirty = true;
        }
    }
    save_store(&manager.path, &manager.store, true)?;
    drop(manager);
    snapshot()
}

pub fn reorder_timers(input: TimerReorderInput) -> Result<TimerSnapshot, String> {
    let manager_lock = TIMER.get().ok_or("Timer service is not initialized")?;
    let manager = manager_lock
        .lock()
        .map_err(|_| "Timer service is unavailable")?;
    {
        let mut store = manager
            .store
            .lock()
            .map_err(|_| "Timer data is unavailable")?;
        if input.ids.len() != store.timers.len() {
            return Err("Timer reorder list is incomplete".to_owned());
        }

        let mut seen = HashSet::new();
        if input.ids.iter().any(|id| !seen.insert(id.as_str())) {
            return Err("Timer reorder list contains duplicate ids".to_owned());
        }
        if store
            .timers
            .iter()
            .any(|timer| !seen.contains(timer.id.as_str()))
        {
            return Err("Timer reorder list contains invalid ids".to_owned());
        }

        for timer in store.timers.iter_mut() {
            if let Some(index) = input.ids.iter().position(|id| id == &timer.id) {
                timer.order = index as u32 + 1;
            }
        }
        store.dirty = true;
    }
    save_store(&manager.path, &manager.store, true)?;
    drop(manager);
    snapshot()
}

pub fn delete_timer(id: String) -> Result<TimerSnapshot, String> {
    let manager_lock = TIMER.get().ok_or("计时器服务未初始化")?;
    let manager = manager_lock.lock().map_err(|_| "计时器服务不可用")?;
    {
        let mut store = manager.store.lock().map_err(|_| "计时器数据不可用")?;
        let len = store.timers.len();
        store.timers.retain(|timer| timer.id != id);
        if store.timers.len() == len {
            return Err("计时器不存在".to_owned());
        }
        store.dirty = true;
    }
    save_store(&manager.path, &manager.store, true)?;
    drop(manager);
    snapshot()
}

fn timer_views(timers: &[TimerEntry]) -> Vec<TimerView> {
    let now = now_ms();
    let mut views: Vec<TimerView> = timers
        .iter()
        .map(|timer| {
            let elapsed_ms = timer_elapsed_ms(timer, now);
            let remaining_ms = timer
                .duration_ms
                .map(|duration| duration.saturating_sub(elapsed_ms));
            let progress = timer
                .duration_ms
                .map(|duration| {
                    if duration == 0 {
                        0.0
                    } else {
                        (elapsed_ms.min(duration) as f64 / duration as f64).clamp(0.0, 1.0)
                    }
                })
                .unwrap_or(0.0);
            TimerView {
                id: timer.id.clone(),
                name: timer.name.clone(),
                note: timer.note.clone(),
                kind: timer.kind.clone(),
                status: timer.status.clone(),
                elapsed_ms,
                duration_ms: timer.duration_ms,
                remaining_ms,
                progress,
                notifications_enabled: matches!(timer.kind, TimerKind::Countdown)
                    && timer.notifications_enabled,
                order: timer.order,
                finished_at_ms: timer.finished_at_ms,
            }
        })
        .collect();
    views.sort_by_key(|timer| timer.order);
    views
}

fn timer_elapsed_ms(timer: &TimerEntry, now: u128) -> u128 {
    if timer.status == TimerStatus::Running {
        timer
            .started_at_ms
            .map(|started| timer.elapsed_ms.saturating_add(now.saturating_sub(started)))
            .unwrap_or(timer.elapsed_ms)
    } else {
        timer.elapsed_ms
    }
}

fn pause_running_entries(store: &mut TimerStore) -> usize {
    let now = now_ms();
    let mut paused_count = 0;
    for timer in store.timers.iter_mut() {
        if timer.status == TimerStatus::Running {
            timer.elapsed_ms = timer_elapsed_ms(timer, now);
            timer.started_at_ms = None;
            timer.status = TimerStatus::Paused;
            paused_count += 1;
        }
    }
    if paused_count > 0 {
        store.dirty = true;
    }
    paused_count
}

fn save_store(path: &Path, store: &Arc<Mutex<TimerStore>>, force: bool) -> Result<(), String> {
    let mut store = store.lock().map_err(|_| "计时器数据不可用")?;
    if !force && !store.dirty {
        return Ok(());
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建计时器目录失败: {error}"))?;
    }
    let raw = serde_json::to_string_pretty(&*store)
        .map_err(|error| format!("序列化计时器数据失败: {error}"))?;
    fs::write(path, raw).map_err(|error| format!("保存计时器数据失败: {error}"))?;
    store.dirty = false;
    Ok(())
}

fn load_store(path: &Path) -> Result<TimerStore, String> {
    if !path.exists() {
        return Ok(TimerStore::default());
    }
    let raw = fs::read_to_string(path).map_err(|error| format!("读取计时器数据失败: {error}"))?;
    let mut store: TimerStore = serde_json::from_str(&raw).unwrap_or_default();
    store.version = store_version();
    for timer in store.timers.iter_mut() {
        if timer.status == TimerStatus::Running {
            timer.elapsed_ms = timer_elapsed_ms(timer, now_ms());
            timer.status = TimerStatus::Paused;
            timer.started_at_ms = None;
        }
        if matches!(timer.kind, TimerKind::Countdown) && timer.duration_ms.is_none() {
            timer.duration_ms = Some(DEFAULT_COUNTDOWN_SECONDS as u128 * 1000);
        }
        if matches!(timer.kind, TimerKind::Stopwatch) {
            timer.notifications_enabled = false;
        }
    }
    store.dirty = false;
    Ok(store)
}

fn clean_text(value: Option<String>) -> Option<String> {
    let value = value?.trim().to_owned();
    if value.is_empty() {
        None
    } else {
        Some(value.chars().take(80).collect())
    }
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn default_notifications_enabled() -> bool {
    true
}

fn store_version() -> u32 {
    1
}

#[cfg(target_os = "windows")]
fn notify_timer_finished(name: String) {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, MB_ICONINFORMATION, MB_OK, MB_SETFOREGROUND, MB_TOPMOST,
    };

    let title: Vec<u16> = std::ffi::OsStr::new("计时器结束")
        .encode_wide()
        .chain(Some(0))
        .collect();
    let body = format!("{name} 已结束");
    let body: Vec<u16> = std::ffi::OsStr::new(&body)
        .encode_wide()
        .chain(Some(0))
        .collect();
    thread::spawn(move || unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            body.as_ptr(),
            title.as_ptr(),
            MB_OK | MB_ICONINFORMATION | MB_TOPMOST | MB_SETFOREGROUND,
        );
    });
}

#[cfg(not(target_os = "windows"))]
fn notify_timer_finished(_name: String) {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pause_running_entries_pauses_and_keeps_elapsed_time() {
        let now = now_ms();
        let mut store = TimerStore {
            timers: vec![TimerEntry {
                id: "timer-1".to_owned(),
                name: "Timer".to_owned(),
                note: String::new(),
                kind: TimerKind::Stopwatch,
                status: TimerStatus::Running,
                elapsed_ms: 1_000,
                duration_ms: None,
                started_at_ms: Some(now.saturating_sub(2_000)),
                finished_at_ms: None,
                notifications_enabled: false,
                order: 1,
                window: TimerWindowState::default(),
                notified_at_ms: None,
            }],
            ..TimerStore::default()
        };

        let paused_count = pause_running_entries(&mut store);

        let timer = &store.timers[0];
        assert_eq!(timer.status, TimerStatus::Paused);
        assert!(timer.started_at_ms.is_none());
        assert!(timer.elapsed_ms >= 3_000);
        assert_eq!(paused_count, 1);
        assert!(store.dirty);
    }
}
