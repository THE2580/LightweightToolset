use std::{
    collections::HashSet,
    fs,
    path::{Path, PathBuf},
    sync::{
        atomic::{AtomicBool, Ordering},
        mpsc, Mutex, OnceLock,
    },
    thread,
    time::{Duration, Instant},
};

use serde::{Deserialize, Serialize};
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, PhysicalSize, Position, Size, WebviewUrl,
    WebviewWindow, WebviewWindowBuilder,
};

use crate::AppState;

const NOTIFY_LABEL: &str = "tool-shortcut-tip";
const STORE_DIR: &str = "shortcut_tip";
const STORE_FILE: &str = "shortcut_tip.json";
const NOTIFY_WIDTH: f64 = 560.0;
const NOTIFY_HEIGHT: f64 = 56.0;
const NOTIFY_GAP: f64 = 16.0;
const DISPLAY_MS: u64 = 2000;
const SLIDE_MS: u64 = 300;
const KEEP_TOP_INTERVAL: Duration = Duration::from_millis(400);
const DUPLICATE_MS: u128 = 400;
const REPEAT_MS: u128 = 500;
const ZONE_RATIO: f64 = 0.15;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Origin {
    Hook,
    Helper,
    Ui,
    Command,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutTipEntry {
    pub id: String,
    pub keys: String,
    pub label: String,
    #[serde(default = "default_true")]
    pub enabled: bool,
    #[serde(default)]
    pub preset: bool,
    #[serde(default)]
    pub created_at: u128,
    #[serde(default)]
    pub triggered_count: u64,
    /// 绑定本软件全局快捷键时记录其 id（main_window / clipboard / window_pinner）。
    #[serde(default)]
    pub app_hotkey: Option<String>,
    /// 快照附加：对应工具是否启用，仅用于前端置灰展示。
    #[serde(default)]
    pub available: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ShortcutTipStore {
    #[serde(default)]
    tips: Vec<ShortcutTipEntry>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutTipSnapshot {
    pub tips: Vec<ShortcutTipEntry>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutTipInput {
    pub id: Option<String>,
    pub keys: String,
    pub label: String,
    pub enabled: Option<bool>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct TipPayload {
    title: String,
    keys: Option<String>,
    preset: bool,
    count: u32,
    side: &'static str,
}

enum Command {
    Show { title: String, keys: Option<String>, preset: bool },
    Reset,
}

struct Preset {
    keys: &'static str,
    label: &'static str,
}

const APP_HOTKEY_PRESETS: [(&str, &str); 3] = [
    ("main_window", "呼出主窗口"),
    ("clipboard", "打开剪贴板弹窗"),
    ("window_pinner", "窗口置顶"),
];

const PRESETS: [Preset; 24] = [
    Preset { keys: "Ctrl+C", label: "复制" },
    Preset { keys: "Ctrl+V", label: "粘贴" },
    Preset { keys: "Ctrl+X", label: "剪切" },
    Preset { keys: "Ctrl+A", label: "全选" },
    Preset { keys: "Ctrl+Z", label: "撤销" },
    Preset { keys: "Ctrl+Y", label: "重做" },
    Preset { keys: "Ctrl+S", label: "保存" },
    Preset { keys: "Ctrl+F", label: "查找" },
    Preset { keys: "Ctrl+N", label: "新建" },
    Preset { keys: "Ctrl+W", label: "关闭标签页" },
    Preset { keys: "Ctrl+Shift+T", label: "恢复标签页" },
    Preset { keys: "Alt+Tab", label: "切换窗口" },
    Preset { keys: "Win+D", label: "显示桌面" },
    Preset { keys: "Win+E", label: "打开资源管理器" },
    Preset { keys: "Win+I", label: "打开设置" },
    Preset { keys: "Win+S", label: "搜索" },
    Preset { keys: "Win+V", label: "剪贴板历史" },
    Preset { keys: "Win+Tab", label: "任务视图" },
    Preset { keys: "Win+Shift+S", label: "区域截图" },
    Preset { keys: "Win+Left", label: "窗口左分屏" },
    Preset { keys: "Win+Right", label: "窗口右分屏" },
    Preset { keys: "Win+Up", label: "窗口最大化" },
    Preset { keys: "Win+Down", label: "窗口还原/最小化" },
    Preset { keys: "Ctrl+Shift+Esc", label: "任务管理器" },
];

static STORE: OnceLock<Mutex<StoreHandle>> = OnceLock::new();
static APP: OnceLock<AppHandle> = OnceLock::new();
static TX: OnceLock<mpsc::Sender<Command>> = OnceLock::new();
static READY: AtomicBool = AtomicBool::new(false);
static WORKER_STARTED: AtomicBool = AtomicBool::new(false);
static LISTENER: Mutex<Option<InputListener>> = Mutex::new(None);
static LAST_REPORT: Mutex<Option<(String, Origin, Instant)>> = Mutex::new(None);
static SUPPRESS_UNTIL: Mutex<Option<Instant>> = Mutex::new(None);
static CAPTURING: AtomicBool = AtomicBool::new(false);
static HELPER: Mutex<Option<std::process::Child>> = Mutex::new(None);
static HELPER_STARTED: AtomicBool = AtomicBool::new(false);
#[cfg(target_os = "windows")]
static HELPER_STREAM: OnceLock<Mutex<std::net::TcpStream>> = OnceLock::new();

struct StoreHandle {
    path: PathBuf,
    store: ShortcutTipStore,
}

fn default_true() -> bool {
    true
}

fn now_ms() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn now_id() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let millis = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    format!("tip-{millis}-{seq}")
}

fn default_presets() -> Vec<ShortcutTipEntry> {
    PRESETS
        .iter()
        .map(|preset| ShortcutTipEntry {
            id: now_id(),
            keys: preset.keys.to_owned(),
            label: preset.label.to_owned(),
            enabled: true,
            preset: true,
            created_at: now_ms(),
            triggered_count: 0,
            app_hotkey: None,
            available: true,
        })
        .collect()
}

fn app_hotkey_presets() -> Vec<ShortcutTipEntry> {
    APP_HOTKEY_PRESETS
        .iter()
        .map(|(id, label)| ShortcutTipEntry {
            id: now_id(),
            keys: String::new(),
            label: (*label).to_owned(),
            enabled: true,
            preset: true,
            created_at: now_ms(),
            triggered_count: 0,
            app_hotkey: Some((*id).to_owned()),
            available: true,
        })
        .collect()
}

pub fn init(config_dir: &Path, app: &AppHandle) -> Result<(), String> {
    let _ = APP.set(app.clone());
    let dir = config_dir.join(STORE_DIR);
    fs::create_dir_all(&dir).map_err(|error| format!("创建快捷键提示目录失败: {error}"))?;
    let path = dir.join(STORE_FILE);
    let mut store = load_store(&path)?;
    let mut seeded = false;
    for preset in PRESETS.iter() {
        let exists = store
            .tips
            .iter()
            .any(|tip| tip.preset && tip.app_hotkey.is_none() && tip.keys == preset.keys);
        if !exists {
            store.tips.push(ShortcutTipEntry {
                id: now_id(),
                keys: preset.keys.to_owned(),
                label: preset.label.to_owned(),
                enabled: true,
                preset: true,
                created_at: now_ms(),
                triggered_count: 0,
                app_hotkey: None,
                available: true,
            });
            seeded = true;
        }
    }
    for (id, label) in APP_HOTKEY_PRESETS.iter() {
        if !store.tips.iter().any(|tip| tip.app_hotkey.as_deref() == Some(*id)) {
            store.tips.push(ShortcutTipEntry {
                id: now_id(),
                keys: String::new(),
                label: (*label).to_owned(),
                enabled: true,
                preset: true,
                created_at: now_ms(),
                triggered_count: 0,
                app_hotkey: Some((*id).to_owned()),
                available: true,
            });
            seeded = true;
        }
    }
    // 旧版本按毫秒生成 id，同一毫秒内的预设 id 全相同；这里补齐成唯一 id。
    let mut seen = std::collections::HashSet::new();
    let mut repaired = false;
    for tip in store.tips.iter_mut() {
        if tip.id.is_empty() || !seen.insert(tip.id.clone()) {
            tip.id = now_id();
            seen.insert(tip.id.clone());
            repaired = true;
        }
    }
    if repaired || seeded {
        save_store(&path, &store)?;
    }
    if !store.tips.iter().any(|tip| tip.preset) {
        // 旧数据没有 preset 标记：按键与预设重合的视为预设，其余保留为自定义。
        for tip in store.tips.iter_mut() {
            if PRESETS.iter().any(|preset| preset.keys == tip.keys) {
                tip.preset = true;
            }
        }
        if !store.tips.iter().any(|tip| tip.preset) {
            let mut tips = default_presets();
            tips.extend(store.tips);
            store.tips = tips;
        }
        save_store(&path, &store)?;
    }
    STORE
        .set(Mutex::new(StoreHandle { path, store }))
        .map_err(|_| "快捷键提示服务已初始化".to_owned())
}

pub fn relocate(config_dir: &Path) -> Result<(), String> {
    let Some(lock) = STORE.get() else {
        return Err("快捷键提示服务未初始化".to_owned());
    };
    let mut handle = lock.lock().map_err(|_| "快捷键提示数据不可用")?;
    let dir = config_dir.join(STORE_DIR);
    fs::create_dir_all(&dir).map_err(|error| format!("创建快捷键提示目录失败: {error}"))?;
    handle.path = dir.join(STORE_FILE);
    save_store(&handle.path, &handle.store)
}

fn load_store(path: &Path) -> Result<ShortcutTipStore, String> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(serde_json::from_str(&text).unwrap_or_else(|_| ShortcutTipStore { tips: Vec::new() })),
        Err(_) => Ok(ShortcutTipStore { tips: Vec::new() }),
    }
}

fn save_store(path: &Path, store: &ShortcutTipStore) -> Result<(), String> {
    let text = serde_json::to_string_pretty(store).map_err(|error| format!("序列化快捷键提示失败: {error}"))?;
    fs::write(path, text).map_err(|error| format!("写入快捷键提示失败: {error}"))
}

pub fn snapshot() -> Result<ShortcutTipSnapshot, String> {
    let tips = {
        let handle = store_handle()?;
        handle.store.tips.clone()
    };
    Ok(build_snapshot(tips))
}

/// 快照里的「本软件快捷键」条目按当前设置解析键值与可用状态。
fn build_snapshot(tips: Vec<ShortcutTipEntry>) -> ShortcutTipSnapshot {
    let state = APP.get().and_then(|app| app.try_state::<AppState>());
    let mut hotkeys: Option<std::collections::BTreeMap<String, String>> = None;
    let mut enabled: Vec<(String, bool)> = Vec::new();
    if let Some(state) = state {
        if let Ok(registry) = state.registry.lock() {
            hotkeys = Some(registry.settings().hotkeys.clone());
            enabled = APP_HOTKEY_PRESETS
                .iter()
                .map(|(id, _)| ((*id).to_owned(), *id == "main_window" || registry.is_enabled(id)))
                .collect();
        }
    }
    let tips = tips
        .into_iter()
        .map(|mut tip| {
            if let Some(id) = tip.app_hotkey.clone() {
                let raw = hotkeys
                    .as_ref()
                    .and_then(|map| map.get(&id).cloned())
                    .unwrap_or_default();
                tip.keys = canonical_chord(&raw).unwrap_or(raw);
                tip.available = enabled
                    .iter()
                    .find(|(candidate, _)| *candidate == id)
                    .map(|(_, value)| *value)
                    .unwrap_or(true);
            } else {
                tip.available = true;
            }
            tip
        })
        .collect();
    ShortcutTipSnapshot { tips }
}

/// 本软件全局快捷键被触发时通知提示工具（由快捷键回调调用，不依赖键盘钩子）。
pub fn notify_app_hotkey(hotkey_id: &str) {
    if CAPTURING.load(Ordering::SeqCst) {
        return;
    }
    let target = {
        let Ok(handle) = store_handle() else {
            return;
        };
        let Some(tip) = handle
            .store
            .tips
            .iter()
            .find(|tip| tip.enabled && tip.app_hotkey.as_deref() == Some(hotkey_id))
            .cloned()
        else {
            return;
        };
        tip
    };
    let Some(app) = APP.get() else {
        return;
    };
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let Ok(registry) = state.registry.lock() else {
        return;
    };
    if hotkey_id != "main_window" && !registry.is_enabled(hotkey_id) {
        return;
    }
    let raw = registry.settings().hotkeys.get(hotkey_id).cloned().unwrap_or_default();
    drop(registry);
    let keys = canonical_chord(&raw).unwrap_or(raw);
    if keys.is_empty() {
        return;
    }
    report(
        keys.clone(),
        Origin::Command,
        target.label,
        Some(keys),
        true,
        Some(target.id),
    );
}

fn store_handle() -> Result<std::sync::MutexGuard<'static, StoreHandle>, String> {
    let lock = STORE.get().ok_or("快捷键提示服务未初始化")?;
    lock.lock().map_err(|_| "快捷键提示数据不可用".to_owned())
}

pub fn upsert(input: ShortcutTipInput) -> Result<ShortcutTipSnapshot, String> {
    let keys = canonical_chord(&input.keys).ok_or("快捷键格式无法识别")?;
    let label = input.label.trim();
    if label.is_empty() {
        return Err("提示文案不能为空".to_owned());
    }
    let mut handle = store_handle()?;
    match input.id {
        Some(id) => {
            let entry = handle
                .store
                .tips
                .iter_mut()
                .find(|entry| entry.id == id)
                .ok_or("提示不存在")?;
            // 预设条目只允许改文案与启停，快捷键固定。
            if !entry.preset {
                entry.keys = keys;
            }
            entry.label = label.to_owned();
            if let Some(enabled) = input.enabled {
                entry.enabled = enabled;
            }
        }
        None => {
            handle.store.tips.push(ShortcutTipEntry {
                id: now_id(),
                keys,
                label: label.to_owned(),
                enabled: input.enabled.unwrap_or(true),
                preset: false,
                created_at: now_ms(),
                triggered_count: 0,
                app_hotkey: None,
                available: true,
            });
        }
    }
    save_store(&handle.path, &handle.store)?;
    let tips = handle.store.tips.clone();
    drop(handle);
    Ok(build_snapshot(tips))
}

pub fn delete(id: String) -> Result<ShortcutTipSnapshot, String> {
    let mut handle = store_handle()?;
    if let Some(entry) = handle.store.tips.iter().find(|entry| entry.id == id) {
        if entry.preset {
            return Err("预设提示不能删除，可以关闭它或修改文案".to_owned());
        }
    }
    handle.store.tips.retain(|entry| entry.id != id);
    save_store(&handle.path, &handle.store)?;
    let tips = handle.store.tips.clone();
    drop(handle);
    Ok(build_snapshot(tips))
}

pub fn restore_presets() -> Result<ShortcutTipSnapshot, String> {
    let mut handle = store_handle()?;
    let custom = handle
        .store
        .tips
        .iter()
        .filter(|tip| !tip.preset)
        .cloned()
        .collect::<Vec<_>>();
    let mut tips = default_presets();
    tips.extend(app_hotkey_presets());
    tips.extend(custom);
    handle.store.tips = tips;
    save_store(&handle.path, &handle.store)?;
    let tips = handle.store.tips.clone();
    drop(handle);
    Ok(build_snapshot(tips))
}

fn notifications_enabled() -> bool {
    let Some(app) = APP.get() else {
        return false;
    };
    let Some(state) = app.try_state::<AppState>() else {
        return false;
    };
    state
        .registry
        .lock()
        .map(|registry| registry.is_enabled("shortcut_tip"))
        .unwrap_or(false)
}

/// 应用内窗口与剪贴板命令触发的自定义提示（不匹配快捷键，直接显示文案）。
pub fn notify_text(title: &str, origin: Origin) {
    let title = title.trim();
    if title.is_empty() {
        return;
    }
    report(format!("text:{title}"), origin, title.to_owned(), None, false, None);
}

/// 进入录制：钩子接管并吞掉按键，避免系统或应用热键把按键吃掉/触发。
pub fn begin_capture() {
    CAPTURING.store(true, Ordering::SeqCst);
    cancel_all();
}

pub fn end_capture() {
    CAPTURING.store(false, Ordering::SeqCst);
}

/// 程序模拟粘贴会注入一次 Ctrl+V，短暂屏蔽快捷键匹配，避免多弹一条提示。
pub fn suppress_shortcuts(millis: u64) {
    if let Ok(mut until) = SUPPRESS_UNTIL.lock() {
        *until = Some(Instant::now() + Duration::from_millis(millis));
    }
}

fn shortcuts_suppressed() -> bool {
    SUPPRESS_UNTIL
        .lock()
        .map(|until| until.map(|at| Instant::now() < at).unwrap_or(false))
        .unwrap_or(false)
}

/// 快捷键触发：匹配用户配置的提示后弹浮窗。
pub fn notify_shortcut(keys: &str, origin: Origin) {
    if shortcuts_suppressed() {
        return;
    }
    let Some(chord) = canonical_chord(keys) else {
        return;
    };
    let Some((id, label, tip_keys, preset)) = lookup(&chord) else {
        return;
    };
    report(chord, origin, label, Some(tip_keys), preset, Some(id));
}

fn lookup(chord: &str) -> Option<(String, String, String, bool)> {
    let handle = STORE.get()?.lock().ok()?;
    handle
        .store
        .tips
        .iter()
        .find(|tip| tip.enabled && tip.app_hotkey.is_none() && tip.keys == chord)
        .map(|tip| (tip.id.clone(), tip.label.clone(), tip.keys.clone(), tip.preset))
}

/// 记录一次提示次数；落盘最多每 5 秒一次，避免高频写盘。
fn bump_triggered(id: &str) {
    static LAST_SAVE: Mutex<Option<Instant>> = Mutex::new(None);
    let Ok(mut handle) = store_handle() else {
        return;
    };
    let Some(tip) = handle.store.tips.iter_mut().find(|tip| tip.id == id) else {
        return;
    };
    tip.triggered_count = tip.triggered_count.saturating_add(1);
    let should_save = LAST_SAVE
        .try_lock()
        .map(|mut last| match *last {
            Some(at) if at.elapsed() < Duration::from_secs(5) => false,
            _ => {
                *last = Some(Instant::now());
                true
            }
        })
        .unwrap_or(false);
    if should_save {
        let _ = save_store(&handle.path, &handle.store);
    }
}

fn report(
    dedupe_key: String,
    origin: Origin,
    title: String,
    keys: Option<String>,
    preset: bool,
    bump_id: Option<String>,
) {
    if !notifications_enabled() {
        return;
    }
    if !claim_report(&dedupe_key, origin) {
        return;
    }
    if let Some(id) = bump_id {
        bump_triggered(&id);
    }
    send(Command::Show { title, keys, preset });
}

fn claim_report(key: &str, origin: Origin) -> bool {
    let Ok(mut last) = LAST_REPORT.lock() else {
        return true;
    };
    if let Some((last_key, last_origin, at)) = last.as_ref() {
        if last_key == key {
            let elapsed = at.elapsed().as_millis();
            if *last_origin == origin && elapsed < REPEAT_MS {
                return false;
            }
            if *last_origin != origin && elapsed < DUPLICATE_MS {
                return false;
            }
        }
    }
    *last = Some((key.to_owned(), origin, Instant::now()));
    true
}

fn send(command: Command) {
    if let Some(tx) = TX.get() {
        let _ = tx.send(command);
    }
}

pub fn cancel_all() {
    send(Command::Reset);
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
        .name("tool-shortcut-tip".to_owned())
        .spawn(move || {
            let mut visible = false;
            let mut hiding = false;
            let mut deadline: Option<Instant> = None;
            let mut last: Option<(String, Option<String>)> = None;
            let mut count = 0u32;
            loop {
                let wait = deadline
                    .map(|at| at.saturating_duration_since(Instant::now()).max(Duration::from_millis(5)))
                    // 显示期间每 400ms 醒一次重新压顶，防止被其他置顶窗口盖住。
                    .map(|wait| if visible { wait.min(KEEP_TOP_INTERVAL) } else { wait })
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
                        last = None;
                    }
                    Ok(Command::Show { title, keys, preset }) => {
                        if !notifications_enabled() {
                            continue;
                        }
                        let Some(app) = APP.get().cloned() else {
                            continue;
                        };
                        let Some(window) = ensure_window(&app) else {
                            continue;
                        };
                        let same = matches!(&last, Some((last_title, last_keys)) if *last_title == title && *last_keys == keys);
                        if !visible {
                            if !wait_ready(Duration::from_millis(1500)) {
                                let _ = window.hide();
                                continue;
                            }
                            count = 1;
                            last = Some((title.clone(), keys.clone()));
                        } else if same {
                            count = count.saturating_add(1);
                        } else {
                            count = 1;
                            last = Some((title.clone(), keys.clone()));
                        }
                        let side = position_window(&window, &app);

                        if !visible {
                            let _ = window.show();
                            visible = true;
                        }
                        raise_to_top(&window);
                        hiding = false;
                        let _ = window.emit(
                            "shortcut-tip-show",
                            TipPayload { title, keys, preset, count, side },
                        );
                        deadline = Some(Instant::now() + Duration::from_millis(DISPLAY_MS));
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => {
                        let Some(at) = deadline else {
                            continue;
                        };
                        if Instant::now() < at {
                            if visible && !hiding {
                                if let Some(app) = APP.get() {
                                    if let Some(window) = app.get_webview_window(NOTIFY_LABEL) {
                                        raise_to_top(&window);
                                    }
                                }
                            }
                            continue;
                        }
                        if !hiding {
                            if let Some(app) = APP.get() {
                                if let Some(window) = app.get_webview_window(NOTIFY_LABEL) {
                                    let _ = window.emit("shortcut-tip-hide", ());
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
                            last = None;
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
    let window = WebviewWindowBuilder::new(app, NOTIFY_LABEL, WebviewUrl::App("notification.html".into()))
        .title("Shortcut tip")
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



fn position_window(window: &WebviewWindow, app: &AppHandle) -> &'static str {
    let Ok(Some(monitor)) = app.primary_monitor() else {
        return "bottom";
    };
    let scale = monitor.scale_factor();
    let work_area = monitor.work_area();
    let width = (NOTIFY_WIDTH * scale).round() as u32;
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

#[cfg(target_os = "windows")]
fn raise_to_top(window: &WebviewWindow) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
    };
    let Ok(hwnd) = window.hwnd() else {
        return;
    };
    unsafe {
        // 每次显示都重新压到置顶层，避免被同样置顶的窗口（含本软件）盖住。
        SetWindowPos(
            hwnd.0,
            HWND_TOPMOST,
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        );
    }
}

#[cfg(not(target_os = "windows"))]
fn raise_to_top(_window: &WebviewWindow) {}

struct InputListener {
    thread_id: u32,
    thread: thread::JoinHandle<()>,
}

pub fn start() -> Result<(), String> {
    start_worker();
    start_helper_bridge();
    let Ok(mut guard) = LISTENER.lock() else {
        return Err("快捷键提示监听不可用".to_owned());
    };
    if guard.is_some() {
        return Ok(());
    }
    let (ready_tx, ready_rx) = mpsc::channel();
    let thread = thread::Builder::new()
        .name("tool-shortcut-tip-input".to_owned())
        .spawn(move || unsafe {
            install_input_listeners(ready_tx);
        })
        .map_err(|error| format!("启动快捷键提示监听失败: {error}"))?;
    match ready_rx.recv_timeout(Duration::from_secs(3)) {
        Ok(Some(thread_id)) => {
            *guard = Some(InputListener { thread_id, thread });
            Ok(())
        }
        _ => {
            let _ = thread.join();
            Err("快捷键提示监听启动超时".to_owned())
        }
    }
}

pub fn stop() {
    flush_store();
    stop_helper();
    if let Ok(mut guard) = LISTENER.lock() {
        if let Some(listener) = guard.take() {
            post_quit(listener.thread_id);
            let _ = listener.thread.join();
        }
    }
    cancel_all();
}

/// 独立采键子进程：复用本 exe，带参启动后只跑键盘钩子并通过回环 TCP 回传组合键。
/// 它不是 Chromium 进程，因此本软件窗口在前台时也能收到系统/他软件热键。
#[cfg(target_os = "windows")]
fn start_helper_bridge() {
    use std::{
        net::TcpListener,
        process::Command,
        sync::atomic::Ordering,
    };

    if HELPER_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let _ = thread::Builder::new()
        .name("tool-shortcut-tip-helper".to_owned())
        .spawn(move || {
            let Ok(listener) = TcpListener::bind(("127.0.0.1", 0)) else {
                return;
            };
            let Ok(port) = listener.local_addr().map(|addr| addr.port()) else {
                return;
            };
            let Ok(exe) = std::env::current_exe() else {
                return;
            };
            let child = {
                use std::os::windows::process::CommandExt;
                const CREATE_NO_WINDOW: u32 = 0x0800_0000;
                Command::new(exe)
                    .arg("--shortcut-tip-helper")
                    .arg(port.to_string())
                    .creation_flags(CREATE_NO_WINDOW)
                    .spawn()
            };
            let Ok(child) = child else {
                HELPER_STARTED.store(false, Ordering::SeqCst);
                return;
            };
            if let Ok(mut slot) = HELPER.lock() {
                *slot = Some(child);
            }
            let Ok((stream, _)) = listener.accept() else {
                return;
            };
            read_helper_stream(stream);
        });
}

#[cfg(target_os = "windows")]
fn read_helper_stream(stream: std::net::TcpStream) {
    use std::io::{BufRead, BufReader};
    for line in BufReader::new(stream).lines() {
        let Ok(chord) = line else {
            break;
        };
        let chord = chord.trim();
        if chord.is_empty() {
            continue;
        }
        notify_shortcut(chord, Origin::Helper);
    }
}

#[cfg(not(target_os = "windows"))]
fn start_helper_bridge() {}

#[cfg(target_os = "windows")]
fn stop_helper() {
    HELPER_STARTED.store(false, std::sync::atomic::Ordering::SeqCst);
    if let Ok(mut slot) = HELPER.lock() {
        if let Some(mut child) = slot.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn stop_helper() {}

/// 子进程入口：连接回主程序并安装键盘钩子，只回传组合键。
#[cfg(target_os = "windows")]
pub fn run_shortcut_helper(port: u16) -> ! {
    use std::net::TcpStream;
    use windows_sys::Win32::{
        System::LibraryLoader::GetModuleHandleW,
        UI::WindowsAndMessaging::{
            DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx,
            WH_KEYBOARD_LL,
        },
    };

    let stream = TcpStream::connect(("127.0.0.1", port)).unwrap_or_else(|_| {
        std::process::exit(1);
    });
    let _ = stream.set_nodelay(true);
    let _ = HELPER_STREAM.set(Mutex::new(stream));

    unsafe {
        let module = GetModuleHandleW(std::ptr::null());
        let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(helper_keyboard_hook), module, 0);
        let mut message = std::mem::zeroed();
        while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&message);
            DispatchMessageW(&message);
        }
        if !hook.is_null() {
            UnhookWindowsHookEx(hook);
        }
    }
    std::process::exit(0);
}

#[cfg(target_os = "windows")]
unsafe extern "system" fn helper_keyboard_hook(
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
            if let Ok(mut pressed) = PRESSED.get_or_init(|| Mutex::new(HashSet::new())).lock() {
                match message {
                    WM_KEYDOWN | WM_SYSKEYDOWN => {
                        let first_press = pressed.insert(vk);
                        if first_press && !is_modifier(vk) {
                            if let Some(chord) = chord_from_pressed(&pressed, vk) {
                                drop(pressed);
                                if let Some(stream) = HELPER_STREAM.get() {
                                    let written = stream
                                        .lock()
                                        .map(|mut stream| {
                                            use std::io::Write;
                                            writeln!(stream, "{chord}").and_then(|_| stream.flush()).is_ok()
                                        })
                                        .unwrap_or(false);
                                    if !written {
                                        // 主程序已退出或连接断开，子进程自行结束，避免成孤儿。
                                        std::process::exit(0);
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

fn flush_store() {
    if let Ok(handle) = store_handle() {
        let _ = save_store(&handle.path, &handle.store);
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
            DispatchMessageW, GetMessageW, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx,
            WH_KEYBOARD_LL,
        },
    };

    let module = GetModuleHandleW(std::ptr::null());
    let hook = SetWindowsHookExW(WH_KEYBOARD_LL, Some(low_level_keyboard_hook), module, 0);
    let thread_id = windows_sys::Win32::System::Threading::GetCurrentThreadId();
    let _ = ready_tx.send(Some(thread_id));

    let mut message = std::mem::zeroed();
    while GetMessageW(&mut message, std::ptr::null_mut(), 0, 0) > 0 {
        TranslateMessage(&message);
        DispatchMessageW(&message);
    }

    if !hook.is_null() {
        UnhookWindowsHookEx(hook);
    }
}

#[cfg(not(target_os = "windows"))]
unsafe fn install_input_listeners(ready_tx: mpsc::Sender<Option<u32>>) {
    let _ = ready_tx.send(None);
}

const MOD_CTRL: u32 = 0x11;
const MOD_ALT: u32 = 0x12;
const MOD_SHIFT: u32 = 0x10;
const MOD_WIN: u32 = 0x5B;
const VK_LCTRL: u32 = 0xA2;
const VK_RCTRL: u32 = 0xA3;
const VK_LALT: u32 = 0xA4;
const VK_RALT: u32 = 0xA5;
const VK_LSHIFT: u32 = 0xA0;
const VK_RSHIFT: u32 = 0xA1;
const VK_LWIN: u32 = 0x5C;
const VK_ESCAPE: u32 = 0x1B;

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
            if CAPTURING.load(Ordering::SeqCst) && vk != VK_ESCAPE {
                // 录制中：吞掉按键并回传组合，Esc 放行以便关闭弹窗。
                if let Ok(mut pressed) = PRESSED.get_or_init(|| Mutex::new(HashSet::new())).lock() {
                    match message {
                        WM_KEYDOWN | WM_SYSKEYDOWN => {
                            let first_press = pressed.insert(vk);
                            if !is_modifier(vk) && first_press {
                                if let Some(chord) = chord_from_pressed(&pressed, vk) {
                                    if let Some(app) = APP.get() {
                                        let _ = app.emit("shortcut-tip-captured", chord);
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
                return 1;
            }
            if let Ok(mut pressed) = PRESSED.get_or_init(|| Mutex::new(HashSet::new())).lock() {
                match message {
                    WM_KEYDOWN | WM_SYSKEYDOWN => {
                        let first_press = pressed.insert(vk);
                        if !is_modifier(vk) {
                            let chord = chord_from_pressed(&pressed, vk);
                            if let Some(chord) = chord {
                                if first_press {
                                    notify_shortcut(&chord, Origin::Hook);
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

fn is_modifier(vk: u32) -> bool {
    matches!(
        vk,
        MOD_CTRL | MOD_ALT | MOD_SHIFT | MOD_WIN
            | VK_LCTRL | VK_RCTRL | VK_LALT | VK_RALT | VK_LSHIFT | VK_RSHIFT | VK_LWIN
    )
}

fn chord_from_pressed(pressed: &HashSet<u32>, vk: u32) -> Option<String> {
    let key = key_name(vk)?;
    let ctrl = pressed.contains(&MOD_CTRL) || pressed.contains(&VK_LCTRL) || pressed.contains(&VK_RCTRL);
    let alt = pressed.contains(&MOD_ALT) || pressed.contains(&VK_LALT) || pressed.contains(&VK_RALT);
    let shift = pressed.contains(&MOD_SHIFT) || pressed.contains(&VK_LSHIFT) || pressed.contains(&VK_RSHIFT);
    let win = pressed.contains(&MOD_WIN) || pressed.contains(&VK_LWIN);
    let mut parts: Vec<&str> = Vec::new();
    if win {
        parts.push("Win");
    }
    if ctrl {
        parts.push("Ctrl");
    }
    if alt {
        parts.push("Alt");
    }
    if shift {
        parts.push("Shift");
    }
    parts.push(&key);
    Some(parts.join("+"))
}

fn key_name(vk: u32) -> Option<String> {
    let name = match vk {
        0x30..=0x39 => return Some(((b'0' + (vk - 0x30) as u8) as char).to_string()),
        0x41..=0x5A => return Some(((b'A' + (vk - 0x41) as u8) as char).to_string()),
        0x70..=0x87 => format!("F{}", vk - 0x70 + 1),
        0x08 => "Backspace".to_owned(),
        0x09 => "Tab".to_owned(),
        0x0D => "Enter".to_owned(),
        0x1B => "Esc".to_owned(),
        0x20 => "Space".to_owned(),
        0x21 => "PageUp".to_owned(),
        0x22 => "PageDown".to_owned(),
        0x23 => "End".to_owned(),
        0x24 => "Home".to_owned(),
        0x25 => "Left".to_owned(),
        0x26 => "Up".to_owned(),
        0x27 => "Right".to_owned(),
        0x28 => "Down".to_owned(),
        0x2D => "Insert".to_owned(),
        0x2E => "Delete".to_owned(),
        _ => return None,
    };
    Some(name)
}

/// 把用户输入或页面上报的快捷键文本规范化为 `Ctrl+Shift+F` 形式。
pub fn canonical_chord(raw: &str) -> Option<String> {
    let mut ctrl = false;
    let mut alt = false;
    let mut shift = false;
    let mut win = false;
    let mut key: Option<String> = None;
    for part in raw.split('+') {
        let token = part.trim();
        if token.is_empty() {
            continue;
        }
        let lower = token.to_ascii_lowercase();
        match lower.as_str() {
            "ctrl" | "control" => ctrl = true,
            "alt" => alt = true,
            "shift" => shift = true,
            "win" | "windows" | "meta" | "cmd" => win = true,
            _ => {
                if key.is_some() {
                    return None;
                }
                key = Some(normalize_key_name(token)?);
            }
        }
    }
    let mut parts: Vec<&str> = Vec::new();
    if win {
        parts.push("Win");
    }
    if ctrl {
        parts.push("Ctrl");
    }
    if alt {
        parts.push("Alt");
    }
    if shift {
        parts.push("Shift");
    }
    let key = key?;
    parts.push(&key);
    Some(parts.join("+"))
}

fn normalize_key_name(token: &str) -> Option<String> {
    let lower = token.to_ascii_lowercase();
    let name = match lower.as_str() {
        "arrowup" | "up" => "Up".to_owned(),
        "arrowdown" | "down" => "Down".to_owned(),
        "arrowleft" | "left" => "Left".to_owned(),
        "arrowright" | "right" => "Right".to_owned(),
        "escape" | "esc" => "Esc".to_owned(),
        " " | "space" | "spacebar" => "Space".to_owned(),
        "enter" | "return" => "Enter".to_owned(),
        "tab" => "Tab".to_owned(),
        "backspace" => "Backspace".to_owned(),
        "delete" | "del" => "Delete".to_owned(),
        "insert" | "ins" => "Insert".to_owned(),
        "home" => "Home".to_owned(),
        "end" => "End".to_owned(),
        "pageup" | "pgup" => "PageUp".to_owned(),
        "pagedown" | "pgdn" => "PageDown".to_owned(),
        _ => {
            let upper = token.to_ascii_uppercase();
            if upper.chars().count() == 1 && upper.chars().all(|ch| ch.is_ascii_graphic()) {
                upper
            } else if upper.starts_with('F') && upper[1..].parse::<u8>().is_ok() {
                upper
            } else {
                return None;
            }
        }
    };
    Some(name)
}

