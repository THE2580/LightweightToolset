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
use tauri::{AppHandle, Emitter, Manager};

use crate::AppState;

const STORE_DIR: &str = "shortcut_tip";
const STORE_FILE: &str = "shortcut_tip.json";
const DUPLICATE_MS: u128 = 400;
const REPEAT_MS: u128 = 500;

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

/// 原生浮窗外观（迁移自「原生提示实验」的 DIY 参数）。
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutTipAppearance {
    /// `auto` / `light` / `dark`
    pub mode: String,
    pub light_blur: u32,
    pub light_tint: f32,
    pub dark_blur: u32,
    pub dark_tint: f32,
    /// `fade` / `slide_up` / `slide_down`
    pub animation: String,
    pub duration_ms: u32,
    /// 键帽配色（`#RRGGBB`）：浅色主题一套。
    pub light_chip_bg: String,
    pub light_chip_border: String,
    pub light_chip_text: String,
    /// 键帽配色（`#RRGGBB`）：暗色主题一套。
    pub dark_chip_bg: String,
    pub dark_chip_border: String,
    pub dark_chip_text: String,
    /// 屏幕物理像素；None = 主屏底部居中。
    pub position: Option<[i32; 2]>,
}

impl Default for ShortcutTipAppearance {
    fn default() -> Self {
        Self {
            mode: "auto".to_owned(),
            light_blur: 4,
            light_tint: 0.92,
            dark_blur: 4,
            dark_tint: 0.78,
            animation: "fade".to_owned(),
            duration_ms: 2000,
            // 默认值＝内置主题键帽的等效实色（浅色：淡蓝灰底 + 灰蓝描边 + 深蓝灰字）。
            light_chip_bg: "#E3E6EF".to_owned(),
            light_chip_border: "#A8B3C9".to_owned(),
            light_chip_text: "#303E58".to_owned(),
            // 暗色：中性灰底 + 浅灰描边 + 浅蓝灰字。
            dark_chip_bg: "#404044".to_owned(),
            dark_chip_border: "#757578".to_owned(),
            dark_chip_text: "#D6E0F0".to_owned(),
            position: None,
        }
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct ShortcutTipStore {
    #[serde(default)]
    tips: Vec<ShortcutTipEntry>,
    #[serde(default)]
    appearance: ShortcutTipAppearance,
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutTipSnapshot {
    pub tips: Vec<ShortcutTipEntry>,
    pub appearance: ShortcutTipAppearance,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ShortcutTipInput {
    pub id: Option<String>,
    pub keys: String,
    pub label: String,
    pub enabled: Option<bool>,
}

struct InputListener {
    thread_id: u32,
    thread: thread::JoinHandle<()>,
}

enum Command {
    Show {
        title: String,
        keys: Option<String>,
        icon: Option<String>,
        icon_path: Option<String>,
    },
    Reset,
}

struct Preset {
    keys: &'static str,
    label: &'static str,
}

const APP_HOTKEY_PRESETS: &[(&str, &str)] = &[
    ("main_window", "呼出主窗口"),
    ("clipboard", "打开剪贴板弹窗"),
    ("window_pinner", "窗口置顶"),
    ("screenshot", "截图"),
    ("screenshot_pin", "贴图"),
    ("pin_restore", "恢复上次关闭的贴图"),
    ("pin_close_all", "关闭所有贴图窗口"),
    ("pin_toggle_visible", "隐藏/显示所有贴图窗口"),
    ("pin_passthrough", "鼠标穿透"),
];

/// 查实验性自定义图标路径（只在开发版生效；正式版一律返回 `None`）。
fn experimental_icon_path(icon: Option<&str>) -> Option<String> {
    if !cfg!(debug_assertions) {
        return None;
    }
    let name = icon?;
    let app = APP.get()?;
    let state = app.try_state::<AppState>()?;
    let registry = state.registry.lock().ok()?;
    registry.settings().experimental_icons.get(name).cloned()
}

/// 软件级快捷键 → 提示浮窗图标名（与 `native_tip` 的图标集合对应）。
fn icon_for_hotkey(hotkey_id: &str) -> Option<&'static str> {
    Some(match hotkey_id {
        "main_window" => "window",
        "clipboard" => "clipboard",
        "window_pinner" => "pin_top",
        "screenshot" => "camera",
        "screenshot_pin" => "pin",
        "pin_restore" => "undo",
        "pin_close_all" => "close_all",
        "pin_toggle_visible" => "eye",
        "pin_passthrough" => "pointer",
        _ => return None,
    })
}

/// 绑定设置类快捷键归属的工具；`None` 表示软件级入口（呼出主窗口），始终可用。
/// 软件级快捷键归属的工具；`None` 表示软件级入口（呼出主窗口），始终可用。
///
/// 工具停用时要按这份归属表注销/启用它的软件级快捷键，所以对外暴露给 `tools`。
pub(crate) fn hotkey_owner_tool(hotkey_id: &str) -> Option<&'static str> {
    match hotkey_id {
        "main_window" => None,
        "clipboard" => Some("clipboard"),
        "window_pinner" => Some("window_pinner"),
        "screenshot" | "screenshot_pin" | "pin_restore" | "pin_close_all"
        | "pin_toggle_visible" | "pin_passthrough" => Some("screenshot"),
        _ => None,
    }
}

const PRESETS: [Preset; 29] = [
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
    Preset { keys: "Alt+F4", label: "关闭当前窗口" },
    Preset { keys: "Win+D", label: "显示桌面" },
    Preset { keys: "Win+E", label: "打开资源管理器" },
    Preset { keys: "Win+I", label: "打开设置" },
    Preset { keys: "Win+S", label: "搜索" },
    Preset { keys: "Win+V", label: "剪贴板历史" },
    Preset { keys: "Win+Tab", label: "任务视图" },
    Preset { keys: "Win+P", label: "投影/切换显示模式" },
    Preset { keys: "Win+R", label: "运行" },
    Preset { keys: "Win+T", label: "任务栏预览" },
    Preset { keys: "Win+G", label: "打开 Game Bar" },
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
    // 旧版本在「进入位置编辑」时会把默认坐标写死成固定值（用户并未真正放置）。
    // 迁移：保存的坐标若与当前默认位置一致，就清回 None，恢复"跟随默认位置"，换显示器/缩放时自动对齐。
    let mut appearance_repaired = false;
    if let Some([x, y]) = store.appearance.position {
        let (_, _, _, _, _, _, default_x, default_y) = crate::native_tip::metrics();
        if x == default_x && y == default_y {
            store.appearance.position = None;
            appearance_repaired = true;
        }
    }
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
    if repaired || seeded || appearance_repaired {
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
        Ok(text) => Ok(serde_json::from_str(&text).unwrap_or_else(|_| ShortcutTipStore {
            tips: Vec::new(),
            appearance: ShortcutTipAppearance::default(),
        })),
        Err(_) => Ok(ShortcutTipStore {
            tips: Vec::new(),
            appearance: ShortcutTipAppearance::default(),
        }),
    }
}

fn save_store(path: &Path, store: &ShortcutTipStore) -> Result<(), String> {
    let text = serde_json::to_string_pretty(store).map_err(|error| format!("序列化快捷键提示失败: {error}"))?;
    fs::write(path, text).map_err(|error| format!("写入快捷键提示失败: {error}"))
}

/// 校验并规范化 `#RRGGBB`；非法值落回暗色默认（避免把无效颜色塞进绘制层）。
fn normalize_color(value: String) -> String {
    let trimmed = value.trim();
    let hex = trimmed.strip_prefix('#').unwrap_or(trimmed);
    if hex.len() == 6 && hex.chars().all(|c| c.is_ascii_hexdigit()) {
        format!("#{}", hex.to_ascii_uppercase())
    } else {
        "#808080".to_owned()
    }
}

/// `#RRGGBB` → 0xRRGGBB。
fn color_to_u32(value: &str) -> Option<u32> {
    let hex = value.trim().strip_prefix('#').unwrap_or(value.trim());
    u32::from_str_radix(hex, 16).ok()
}

/// 读取当前外观（不持锁跨调用）。
pub fn appearance() -> ShortcutTipAppearance {
    store_handle()
        .map(|handle| handle.store.appearance.clone())
        .unwrap_or_default()
}

/// 更新外观设置，返回最新快照。
pub fn set_appearance(input: ShortcutTipAppearance) -> Result<ShortcutTipSnapshot, String> {
    let tips = {
        let mut handle = store_handle()?;
        handle.store.appearance = ShortcutTipAppearance {
            mode: match input.mode.as_str() {
                "light" | "dark" => input.mode,
                _ => "auto".to_owned(),
            },
            light_blur: input.light_blur.min(12),
            light_tint: input.light_tint.clamp(0.4, 1.0),
            dark_blur: input.dark_blur.min(12),
            dark_tint: input.dark_tint.clamp(0.4, 1.0),
            animation: match input.animation.as_str() {
                "slide_up" | "slide_down" => input.animation,
                _ => "fade".to_owned(),
            },
            duration_ms: input.duration_ms.clamp(800, 10_000),
            light_chip_bg: normalize_color(input.light_chip_bg),
            light_chip_border: normalize_color(input.light_chip_border),
            light_chip_text: normalize_color(input.light_chip_text),
            dark_chip_bg: normalize_color(input.dark_chip_bg),
            dark_chip_border: normalize_color(input.dark_chip_border),
            dark_chip_text: normalize_color(input.dark_chip_text),
            position: input.position,
        };
        let _ = save_store(&handle.path, &handle.store);
        handle.store.tips.clone()
    };
    Ok(build_snapshot(tips))
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
                .map(|(id, _)| {
                    (
                        (*id).to_owned(),
                        hotkey_owner_tool(id).map_or(true, |tool| registry.is_enabled(tool)),
                    )
                })
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
                tip.keys = crate::tools::format_hotkey_display(&raw);
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
    ShortcutTipSnapshot {
        tips,
        appearance: appearance(),
    }
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
    if hotkey_owner_tool(hotkey_id).is_some_and(|tool| !registry.is_enabled(tool)) {
        return;
    }
    let raw = registry.settings().hotkeys.get(hotkey_id).cloned().unwrap_or_default();
    drop(registry);
    let keys = crate::tools::format_hotkey_display(&raw);
    if keys.is_empty() {
        return;
    }
    let icon = icon_for_hotkey(hotkey_id).map(str::to_owned);
    let icon_path = experimental_icon_path(icon.as_deref());
    report(
        keys.clone(),
        Origin::Command,
        target.label,
        Some(keys),
        true,
        Some(target.id),
        icon,
        icon_path,
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
    report(
        format!("text:{title}"),
        origin,
        title.to_owned(),
        None,
        false,
        None,
        None,
        None,
    );
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
    let icon = Some(crate::native_tip::icon_name_for_keys(&chord).to_owned());
    let icon_path = experimental_icon_path(icon.as_deref());
    report(
        chord, origin, label, Some(tip_keys), preset, Some(id), icon, icon_path,
    );
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
    icon: Option<String>,
    icon_path: Option<String>,
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
    let _ = preset;
    send(Command::Show {
        title,
        keys,
        icon,
        icon_path,
    });
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

fn start_worker() {
    if WORKER_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    let (tx, rx) = mpsc::channel();
    let _ = TX.set(tx);
    let _ = thread::Builder::new()
        .name("tool-shortcut-tip".to_owned())
        .spawn(move || {
            // 显示改由原生浮窗负责（`native_tip`），这里只维护"同一内容累加 ×N"的状态。
            let mut last: Option<(String, Option<String>)> = None;
            let mut count = 0u32;
            let mut visible_until: Option<Instant> = None;
            while let Ok(command) = rx.recv() {
                match command {
                    Command::Reset => {
                        crate::native_tip::hide();
                        last = None;
                        count = 0;
                        visible_until = None;
                    }
                    Command::Show {
                        title,
                        keys,
                        icon,
                        icon_path,
                    } => {
                        if !notifications_enabled() {
                            continue;
                        }
                        let now = Instant::now();
                        let same = matches!(&last, Some((last_title, last_keys)) if *last_title == title && *last_keys == keys);
                        let still_visible = visible_until.is_some_and(|at| now < at);
                        if same && still_visible {
                            count = count.saturating_add(1);
                        } else {
                            count = 1;
                        }
                        last = Some((title.clone(), keys.clone()));
                        let appearance = appearance();
                        let theme = resolve_theme(&appearance.mode);
                        let (blur, tint) = if theme == "dark" {
                            (appearance.dark_blur, appearance.dark_tint)
                        } else {
                            (appearance.light_blur, appearance.light_tint)
                        };
                        visible_until = Some(now + Duration::from_millis(u64::from(appearance.duration_ms)));
                        let _ = crate::native_tip::show(crate::native_tip::NativeTipPayload {
                            title,
                            keys,
                            count,
                            theme: theme.to_owned(),
                            blur_radius: Some(blur),
                            tint_alpha: Some(tint),
                            animation: Some(appearance.animation.clone()),
                            position: appearance.position,
                            persistent: None,
                            duration_ms: Some(appearance.duration_ms),
                            preview: None,
                            icon: icon.clone(),
                            icon_path: icon_path.clone(),
                            chip_bg: color_to_u32(&(if theme == "dark" {
                                &appearance.dark_chip_bg
                            } else {
                                &appearance.light_chip_bg
                            })),
                            chip_border: color_to_u32(&(if theme == "dark" {
                                &appearance.dark_chip_border
                            } else {
                                &appearance.light_chip_border
                            })),
                            chip_text: color_to_u32(&(if theme == "dark" {
                                &appearance.dark_chip_text
                            } else {
                                &appearance.light_chip_text
                            })),
                        });
                    }
                }
            }
        });
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 解析外观主题：`auto` 读系统「应用使用浅色主题」设置。
fn resolve_theme(mode: &str) -> &'static str {
    match mode {
        "light" => "light",
        "dark" => "dark",
        _ => {
            use windows_sys::Win32::System::Registry::{
                RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY_CURRENT_USER, KEY_READ,
                REG_DWORD,
            };
            let mut value: u32 = 1;
            let mut size = std::mem::size_of::<u32>() as u32;
            let mut key = std::ptr::null_mut();
            let path = wide("Software\\Microsoft\\Windows\\CurrentVersion\\Themes\\Personalize");
            let opened = unsafe {
                RegOpenKeyExW(
                    HKEY_CURRENT_USER,
                    path.as_ptr(),
                    0,
                    KEY_READ,
                    &mut key,
                )
            };
            if opened == 0 {
                let mut kind = 0;
                let value_name = wide("AppsUseLightTheme");
                let read = unsafe {
                    RegQueryValueExW(
                        key,
                        value_name.as_ptr(),
                        std::ptr::null_mut(),
                        &mut kind,
                        &mut value as *mut u32 as *mut u8,
                        &mut size,
                    )
                };
                unsafe {
                    let _ = RegCloseKey(key);
                }
                if read == 0 && kind == REG_DWORD {
                    return if value == 0 { "dark" } else { "light" };
                }
            }
            "light"
        }
    }
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

    // 采键子进程也要按「托盘低内存」口径归还工作集：装完钩子就没什么驻留需求。
    #[cfg(windows)]
    unsafe {
        use windows_sys::Win32::System::ProcessStatus::EmptyWorkingSet;
        use windows_sys::Win32::System::Threading::GetCurrentProcess;
        let _ = EmptyWorkingSet(GetCurrentProcess());
    }

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
            "win" | "windows" | "meta" | "super" | "cmd" | "command" => win = true,
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


#[cfg(test)]
mod tests {
    use super::{canonical_chord, hotkey_owner_tool};

    #[test]
    fn canonical_chord_accepts_super_alias() {
        assert_eq!(canonical_chord("CTRL+SUPER+/").as_deref(), Some("Win+Ctrl+/"));
        assert_eq!(canonical_chord("SUPER+D").as_deref(), Some("Win+D"));
        assert_eq!(canonical_chord("META+Shift+S").as_deref(), Some("Win+Shift+S"));
    }

    #[test]
    fn app_hotkey_presets_map_to_owning_tools() {
        assert_eq!(hotkey_owner_tool("main_window"), None);
        assert_eq!(hotkey_owner_tool("clipboard"), Some("clipboard"));
        assert_eq!(hotkey_owner_tool("window_pinner"), Some("window_pinner"));
        assert_eq!(hotkey_owner_tool("screenshot"), Some("screenshot"));
        assert_eq!(hotkey_owner_tool("screenshot_pin"), Some("screenshot"));
        assert_eq!(hotkey_owner_tool("pin_passthrough"), Some("screenshot"));
    }
}
