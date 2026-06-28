use std::{
    collections::hash_map::DefaultHasher,
    fs,
    hash::{Hash, Hasher},
    path::{Path, PathBuf},
    sync::{mpsc, Arc, Mutex, OnceLock},
    thread,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};

const CLIPBOARD_DIR: &str = "clipboard";
const CLIPBOARD_FILE: &str = "clipboard.json";
const POLL_INTERVAL_MS: u64 = 800;
const DEFAULT_RETENTION_DAYS: u32 = 30;
const DEFAULT_MAX_TEXT_BYTES: usize = 100 * 1024;
pub const DEFAULT_PANEL_WIDTH: u32 = 320;
pub const DEFAULT_PANEL_HEIGHT: u32 = 360;

static CLIPBOARD: OnceLock<Mutex<ClipboardManager>> = OnceLock::new();

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardEntry {
    pub id: String,
    pub text: String,
    pub title: String,
    pub source: String,
    pub created_at: u128,
    pub last_copied_at: u128,
    pub last_used_at: Option<u128>,
    pub pinned_at: Option<u128>,
    pub deleted_at: Option<u128>,
    pub copy_count: u32,
    pub use_count: u32,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardSettings {
    #[serde(default = "default_listening")]
    pub listening: bool,
    #[serde(default = "default_retention_days")]
    pub retention_days: u32,
    #[serde(default = "default_max_text_bytes")]
    pub max_text_bytes: usize,
    #[serde(default = "default_panel_width")]
    pub panel_width: u32,
    #[serde(default = "default_panel_height")]
    pub panel_height: u32,
}

impl Default for ClipboardSettings {
    fn default() -> Self {
        Self {
            listening: default_listening(),
            retention_days: DEFAULT_RETENTION_DAYS,
            max_text_bytes: DEFAULT_MAX_TEXT_BYTES,
            panel_width: DEFAULT_PANEL_WIDTH,
            panel_height: DEFAULT_PANEL_HEIGHT,
        }
    }
}

#[derive(Clone, Debug, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ClipboardStore {
    #[serde(default)]
    settings: ClipboardSettings,
    #[serde(default)]
    entries: Vec<ClipboardEntry>,
    #[serde(default)]
    skipped_too_long: u32,
    #[serde(default)]
    last_cleanup_at: Option<u128>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardQueryInput {
    pub scope: String,
    pub search: Option<String>,
    pub offset: Option<usize>,
    pub limit: Option<usize>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardQueryResult {
    pub entries: Vec<ClipboardEntry>,
    pub total: usize,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardSettingsPatch {
    pub listening: Option<bool>,
    pub retention_days: Option<u32>,
    pub max_text_bytes: Option<usize>,
    pub panel_width: Option<u32>,
    pub panel_height: Option<u32>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardEntryPatch {
    pub title: Option<String>,
    pub pinned: Option<bool>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardSnapshot {
    pub settings: ClipboardSettings,
    pub stats: ClipboardStats,
    pub listening_active: bool,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardStats {
    pub history_count: usize,
    pub pinned_count: usize,
    pub trash_count: usize,
    pub skipped_too_long: u32,
    pub last_cleanup_at: Option<u128>,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipboardPasteResult {
    pub copied: bool,
    pub pasted: bool,
    pub message: String,
}

struct ClipboardWorker {
    stop: mpsc::Sender<()>,
    thread: thread::JoinHandle<()>,
}

struct ClipboardManager {
    path: PathBuf,
    store: Arc<Mutex<ClipboardStore>>,
    worker: Option<ClipboardWorker>,
}

pub fn init(config_dir: &Path) -> Result<(), String> {
    let dir = config_dir.join(CLIPBOARD_DIR);
    fs::create_dir_all(&dir).map_err(|error| format!("创建剪贴板目录失败: {error}"))?;
    let path = dir.join(CLIPBOARD_FILE);
    let store = if path.exists() {
        let raw = fs::read_to_string(&path).map_err(|error| format!("读取剪贴板数据失败: {error}"))?;
        serde_json::from_str(&raw).unwrap_or_default()
    } else {
        ClipboardStore::default()
    };
    let manager = ClipboardManager {
        path,
        store: Arc::new(Mutex::new(store)),
        worker: None,
    };
    CLIPBOARD
        .set(Mutex::new(manager))
        .map_err(|_| "剪贴板服务已初始化".to_owned())
}

pub fn start() -> Result<(), String> {
    let manager_lock = CLIPBOARD.get().ok_or("剪贴板服务未初始化")?;
    let mut manager = manager_lock.lock().map_err(|_| "剪贴板服务不可用")?;
    if manager.worker.is_some() {
        return Ok(());
    }

    let store = Arc::clone(&manager.store);
    let path = manager.path.clone();
    let (stop, receiver) = mpsc::channel();
    let thread = thread::Builder::new()
        .name("tool-clipboard".to_owned())
        .spawn(move || {
            let mut last_hash = read_clipboard_text().ok().map(|text| hash_text(&text));
            loop {
                if receiver.recv_timeout(Duration::from_millis(POLL_INTERVAL_MS)).is_ok() {
                    break;
                }
                let text = match read_clipboard_text() {
                    Ok(text) => text,
                    Err(_) => continue,
                };
                if text.trim().is_empty() {
                    continue;
                }
                let next_hash = hash_text(&text);
                if Some(next_hash) == last_hash {
                    continue;
                }
                last_hash = Some(next_hash);
                let changed = {
                    let mut guard = match store.lock() {
                        Ok(guard) => guard,
                        Err(_) => break,
                    };
                    if !guard.settings.listening {
                        false
                    } else {
                        add_text_to_store(&mut guard, text, "clipboard")
                    }
                };
                if changed {
                    let _ = save_store(&path, &store);
                }
            }
        })
        .map_err(|error| format!("启动剪贴板监听失败: {error}"))?;

    manager.worker = Some(ClipboardWorker { stop, thread });
    Ok(())
}

pub fn stop() {
    let Some(manager_lock) = CLIPBOARD.get() else {
        return;
    };
    let Ok(mut manager) = manager_lock.lock() else {
        return;
    };
    if let Some(worker) = manager.worker.take() {
        let _ = worker.stop.send(());
        let _ = worker.thread.join();
    }
}

pub fn snapshot() -> Result<ClipboardSnapshot, String> {
    let manager_lock = CLIPBOARD.get().ok_or("剪贴板服务未初始化")?;
    let manager = manager_lock.lock().map_err(|_| "剪贴板服务不可用")?;
    let store = manager.store.lock().map_err(|_| "剪贴板数据不可用")?;
    Ok(ClipboardSnapshot {
        settings: store.settings.clone(),
        stats: stats_for(&store),
        listening_active: manager.worker.is_some() && store.settings.listening,
    })
}

pub fn query(input: ClipboardQueryInput) -> Result<ClipboardQueryResult, String> {
    let manager_lock = CLIPBOARD.get().ok_or("剪贴板服务未初始化")?;
    let manager = manager_lock.lock().map_err(|_| "剪贴板服务不可用")?;
    let store = manager.store.lock().map_err(|_| "剪贴板数据不可用")?;
    let scope = input.scope.as_str();
    let search = input.search.unwrap_or_default().trim().to_lowercase();
    let offset = input.offset.unwrap_or(0);
    let limit = input.limit.unwrap_or(50).clamp(1, 100);
    let mut entries: Vec<ClipboardEntry> = store
        .entries
        .iter()
        .filter(|entry| match scope {
            "pinned" => entry.deleted_at.is_none() && entry.pinned_at.is_some(),
            "trash" => entry.deleted_at.is_some(),
            _ => entry.deleted_at.is_none() && entry.pinned_at.is_none(),
        })
        .filter(|entry| search.is_empty() || entry.text.to_lowercase().contains(&search) || entry.title.to_lowercase().contains(&search))
        .cloned()
        .collect();
    entries.sort_by(|a, b| entry_sort_key(b, scope).cmp(&entry_sort_key(a, scope)));
    let total = entries.len();
    let entries = entries.into_iter().skip(offset).take(limit).collect();
    Ok(ClipboardQueryResult { entries, total })
}

pub fn update_settings(patch: ClipboardSettingsPatch) -> Result<ClipboardSnapshot, String> {
    let manager_lock = CLIPBOARD.get().ok_or("剪贴板服务未初始化")?;
    let manager = manager_lock.lock().map_err(|_| "剪贴板服务不可用")?;
    {
        let mut store = manager.store.lock().map_err(|_| "剪贴板数据不可用")?;
        if let Some(listening) = patch.listening {
            store.settings.listening = listening;
        }
        if let Some(retention_days) = patch.retention_days {
            store.settings.retention_days = retention_days.clamp(1, 3650);
        }
        if let Some(max_text_bytes) = patch.max_text_bytes {
            store.settings.max_text_bytes = max_text_bytes.clamp(1024, 10 * 1024 * 1024);
        }
        if let Some(panel_width) = patch.panel_width {
            store.settings.panel_width = panel_width.clamp(280, 560);
        }
        if let Some(panel_height) = patch.panel_height {
            store.settings.panel_height = panel_height.clamp(300, 640);
        }
    }
    save_store(&manager.path, &manager.store)?;
    drop(manager);
    snapshot()
}

pub fn create_manual(title: String, text: String) -> Result<Option<ClipboardEntry>, String> {
    let manager_lock = CLIPBOARD.get().ok_or("剪贴板服务未初始化")?;
    let manager = manager_lock.lock().map_err(|_| "剪贴板服务不可用")?;
    let entry = {
        let mut store = manager.store.lock().map_err(|_| "剪贴板数据不可用")?;
        if !add_text_to_store(&mut store, text, "manual") {
            None
        } else {
            let now = now_ms();
            let entry = store.entries.first_mut();
            if let Some(entry) = entry {
                entry.title = title;
                entry.pinned_at = Some(now);
                Some(entry.clone())
            } else {
                None
            }
        }
    };
    save_store(&manager.path, &manager.store)?;
    Ok(entry)
}

pub fn update_entry(id: String, patch: ClipboardEntryPatch) -> Result<Option<ClipboardEntry>, String> {
    let manager_lock = CLIPBOARD.get().ok_or("剪贴板服务未初始化")?;
    let manager = manager_lock.lock().map_err(|_| "剪贴板服务不可用")?;
    let entry = {
        let mut store = manager.store.lock().map_err(|_| "剪贴板数据不可用")?;
        let now = now_ms();
        if let Some(entry) = store.entries.iter_mut().find(|entry| entry.id == id) {
            if let Some(title) = patch.title {
                entry.title = title;
            }
            if let Some(pinned) = patch.pinned {
                entry.pinned_at = pinned.then_some(now);
            }
            Some(entry.clone())
        } else {
            None
        }
    };
    save_store(&manager.path, &manager.store)?;
    Ok(entry)
}

pub fn copy_entry(id: String) -> Result<ClipboardPasteResult, String> {
    let manager_lock = CLIPBOARD.get().ok_or("剪贴板服务未初始化")?;
    let manager = manager_lock.lock().map_err(|_| "剪贴板服务不可用")?;
    let entry = {
        let mut store = manager.store.lock().map_err(|_| "剪贴板数据不可用")?;
        let now = now_ms();
        if let Some(entry) = store.entries.iter_mut().find(|entry| entry.id == id && entry.deleted_at.is_none()) {
            entry.last_used_at = Some(now);
            entry.last_copied_at = now;
            entry.use_count = entry.use_count.saturating_add(1);
            entry.copy_count = entry.copy_count.saturating_add(1);
            Some(entry.clone())
        } else {
            None
        }
    };
    let Some(entry) = entry else {
        return Ok(ClipboardPasteResult {
            copied: false,
            pasted: false,
            message: "条目不存在".to_owned(),
        });
    };
    write_clipboard_text(&entry.text)?;
    save_store(&manager.path, &manager.store)?;
    Ok(ClipboardPasteResult {
        copied: true,
        pasted: false,
        message: "已复制到剪贴板".to_owned(),
    })
}

pub fn copy_text(text: String) -> Result<ClipboardPasteResult, String> {
    if text.trim().is_empty() {
        return Ok(ClipboardPasteResult {
            copied: false,
            pasted: false,
            message: "没有可复制内容".to_owned(),
        });
    }
    write_clipboard_text(&text)?;
    Ok(ClipboardPasteResult {
        copied: true,
        pasted: false,
        message: "已复制到剪贴板".to_owned(),
    })
}

pub fn delete_entries(ids: Vec<String>) -> Result<(), String> {
    let manager_lock = CLIPBOARD.get().ok_or("剪贴板服务未初始化")?;
    let manager = manager_lock.lock().map_err(|_| "剪贴板服务不可用")?;
    {
        let mut store = manager.store.lock().map_err(|_| "剪贴板数据不可用")?;
        let now = now_ms();
        for entry in store.entries.iter_mut().filter(|entry| ids.contains(&entry.id)) {
            entry.deleted_at = Some(now);
        }
    }
    save_store(&manager.path, &manager.store)
}

pub fn clear_history() -> Result<(), String> {
    let manager_lock = CLIPBOARD.get().ok_or("剪贴板服务未初始化")?;
    let manager = manager_lock.lock().map_err(|_| "剪贴板服务不可用")?;
    {
        let mut store = manager.store.lock().map_err(|_| "剪贴板数据不可用")?;
        let now = now_ms();
        for entry in store.entries.iter_mut().filter(|entry| entry.deleted_at.is_none() && entry.pinned_at.is_none()) {
            entry.deleted_at = Some(now);
        }
    }
    save_store(&manager.path, &manager.store)
}

pub fn panel_size() -> (u32, u32) {
    snapshot()
        .map(|snapshot| (snapshot.settings.panel_width, snapshot.settings.panel_height))
        .unwrap_or((DEFAULT_PANEL_WIDTH, DEFAULT_PANEL_HEIGHT))
}

fn add_text_to_store(store: &mut ClipboardStore, text: String, source: &str) -> bool {
    let normalized = text.trim_matches('\0').to_owned();
    if normalized.trim().is_empty() {
        return false;
    }
    if normalized.len() > store.settings.max_text_bytes {
        store.skipped_too_long = store.skipped_too_long.saturating_add(1);
        return true;
    }
    let now = now_ms();
    if let Some(index) = store.entries.iter().position(|entry| entry.text == normalized) {
        let mut entry = store.entries.remove(index);
        entry.deleted_at = None;
        entry.last_copied_at = now;
        entry.copy_count = entry.copy_count.saturating_add(1);
        store.entries.insert(0, entry);
        cleanup_store(store);
        return true;
    }
    store.entries.insert(0, ClipboardEntry {
        id: format!("{now:x}-{:x}", hash_text(&normalized)),
        text: normalized,
        title: String::new(),
        source: source.to_owned(),
        created_at: now,
        last_copied_at: now,
        last_used_at: None,
        pinned_at: None,
        deleted_at: None,
        copy_count: 1,
        use_count: 0,
    });
    cleanup_store(store);
    true
}

fn cleanup_store(store: &mut ClipboardStore) {
    let now = now_ms();
    let cutoff = now.saturating_sub(store.settings.retention_days as u128 * 24 * 60 * 60 * 1000);
    let trash_cutoff = now.saturating_sub(30 * 24 * 60 * 60 * 1000);
    for entry in store.entries.iter_mut() {
        if entry.deleted_at.is_none() && entry.pinned_at.is_none() && entry.last_copied_at < cutoff {
            entry.deleted_at = Some(now);
        }
    }
    store.entries.retain(|entry| entry.deleted_at.map(|deleted| deleted >= trash_cutoff).unwrap_or(true));
    if store.entries.len() > 500 {
        let mut kept = Vec::with_capacity(500);
        for entry in store.entries.drain(..) {
            if kept.len() < 500 || entry.pinned_at.is_some() {
                kept.push(entry);
            }
        }
        store.entries = kept;
    }
    store.last_cleanup_at = Some(now);
}

fn stats_for(store: &ClipboardStore) -> ClipboardStats {
    ClipboardStats {
        history_count: store.entries.iter().filter(|entry| entry.deleted_at.is_none() && entry.pinned_at.is_none()).count(),
        pinned_count: store.entries.iter().filter(|entry| entry.deleted_at.is_none() && entry.pinned_at.is_some()).count(),
        trash_count: store.entries.iter().filter(|entry| entry.deleted_at.is_some()).count(),
        skipped_too_long: store.skipped_too_long,
        last_cleanup_at: store.last_cleanup_at,
    }
}

fn entry_sort_key(entry: &ClipboardEntry, scope: &str) -> u128 {
    match scope {
        "pinned" => entry.pinned_at.unwrap_or(0),
        "trash" => entry.deleted_at.unwrap_or(0),
        _ => entry.last_used_at.unwrap_or(entry.last_copied_at).max(entry.last_copied_at),
    }
}

fn save_store(path: &Path, store: &Arc<Mutex<ClipboardStore>>) -> Result<(), String> {
    let store = store.lock().map_err(|_| "剪贴板数据不可用")?;
    let raw = serde_json::to_string_pretty(&*store).map_err(|error| format!("序列化剪贴板数据失败: {error}"))?;
    fs::write(path, raw).map_err(|error| format!("保存剪贴板数据失败: {error}"))
}

fn hash_text(text: &str) -> u64 {
    let mut hasher = DefaultHasher::new();
    text.hash(&mut hasher);
    hasher.finish()
}

fn now_ms() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default()
}

fn default_listening() -> bool {
    true
}

fn default_retention_days() -> u32 {
    DEFAULT_RETENTION_DAYS
}

fn default_max_text_bytes() -> usize {
    DEFAULT_MAX_TEXT_BYTES
}

fn default_panel_width() -> u32 {
    DEFAULT_PANEL_WIDTH
}

fn default_panel_height() -> u32 {
    DEFAULT_PANEL_HEIGHT
}

#[cfg(target_os = "windows")]
fn read_clipboard_text() -> Result<String, String> {
    use std::{ptr, slice};
    use windows_sys::Win32::{
        Foundation::HWND,
        System::{
            DataExchange::{CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard},
            Memory::{GlobalLock, GlobalUnlock},
            Ole::CF_UNICODETEXT,
        },
    };

    unsafe {
        if IsClipboardFormatAvailable(CF_UNICODETEXT.into()) == 0 {
            return Ok(String::new());
        }
        if OpenClipboard(ptr::null_mut::<core::ffi::c_void>() as HWND) == 0 {
            return Err("打开系统剪贴板失败".to_owned());
        }
        let handle = GetClipboardData(CF_UNICODETEXT.into());
        if handle.is_null() {
            CloseClipboard();
            return Ok(String::new());
        }
        let locked = GlobalLock(handle);
        if locked.is_null() {
            CloseClipboard();
            return Err("读取系统剪贴板失败".to_owned());
        }
        let mut len = 0usize;
        let ptr = locked as *const u16;
        while *ptr.add(len) != 0 {
            len += 1;
        }
        let text = String::from_utf16_lossy(slice::from_raw_parts(ptr, len));
        GlobalUnlock(handle);
        CloseClipboard();
        Ok(text)
    }
}

#[cfg(target_os = "windows")]
fn write_clipboard_text(text: &str) -> Result<(), String> {
    use std::{mem, ptr};
    use windows_sys::Win32::{
        Foundation::{GlobalFree, HWND},
        System::{
            DataExchange::{CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData},
            Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE},
            Ole::CF_UNICODETEXT,
        },
    };

    unsafe {
        let mut wide: Vec<u16> = text.encode_utf16().collect();
        wide.push(0);
        let bytes = wide.len() * mem::size_of::<u16>();
        if OpenClipboard(ptr::null_mut::<core::ffi::c_void>() as HWND) == 0 {
            return Err("打开系统剪贴板失败".to_owned());
        }
        if EmptyClipboard() == 0 {
            CloseClipboard();
            return Err("清空系统剪贴板失败".to_owned());
        }
        let handle = GlobalAlloc(GMEM_MOVEABLE, bytes);
        if handle.is_null() {
            CloseClipboard();
            return Err("分配剪贴板内存失败".to_owned());
        }
        let locked = GlobalLock(handle);
        if locked.is_null() {
            GlobalFree(handle);
            CloseClipboard();
            return Err("写入系统剪贴板失败".to_owned());
        }
        ptr::copy_nonoverlapping(wide.as_ptr(), locked as *mut u16, wide.len());
        GlobalUnlock(handle);
        if SetClipboardData(CF_UNICODETEXT.into(), handle).is_null() {
            GlobalFree(handle);
            CloseClipboard();
            return Err("提交系统剪贴板失败".to_owned());
        }
        CloseClipboard();
        Ok(())
    }
}

#[cfg(not(target_os = "windows"))]
fn read_clipboard_text() -> Result<String, String> {
    Ok(String::new())
}

#[cfg(not(target_os = "windows"))]
fn write_clipboard_text(_text: &str) -> Result<(), String> {
    Err("当前平台暂不支持剪贴板写入".to_owned())
}
