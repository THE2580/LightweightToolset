mod annotation;
mod capture;
mod clipboard_image;
mod dib;
mod overlay;
mod toolbar;
mod save;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Manager};

static ACTIVE: AtomicBool = AtomicBool::new(false);

/// 截图工具数据目录：`<存储目录>/screenshot`，与 clipboard、timer 等工具目录同级。
pub(crate) fn data_dir(state: &crate::AppState) -> PathBuf {
    state.storage_dir().join("screenshot")
}

/// 默认截图保存目录：`<存储目录>/screenshot/Screenshots`。
pub(crate) fn default_save_dir(state: &crate::AppState) -> PathBuf {
    data_dir(state).join("Screenshots")
}

/// 把设置里的目录解析为实际路径：空串表示默认目录。
pub(crate) fn resolve_save_dir(state: &crate::AppState, configured: &str) -> PathBuf {
    let trimmed = configured.trim();
    if trimmed.is_empty() {
        default_save_dir(state)
    } else {
        PathBuf::from(trimmed)
    }
}

/// 内置默认调色板的 `#RRGGBB` 形式（设置默认值用）。
pub(crate) fn default_palette_hex() -> Vec<String> {
    annotation::DEFAULT_PALETTE.iter().map(|color| annotation::hex_color(*color)).collect()
}

pub(crate) const PALETTE_LEN: usize = annotation::PALETTE_LEN;

pub(crate) fn is_valid_hex(value: &str) -> bool {
    annotation::parse_hex_color(value).is_some()
}

fn parse_palette(values: &[String]) -> [u32; annotation::PALETTE_LEN] {
    let mut colors = annotation::DEFAULT_PALETTE;
    for (index, value) in values.iter().take(annotation::PALETTE_LEN).enumerate() {
        if let Some(color) = annotation::parse_hex_color(value) {
            colors[index] = color;
        }
    }
    colors
}

/// 无 AppState 时的兜底目录（正常流程不会走到）。
fn fallback_save_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("Pictures")
        .join("LightweightToolset")
        .join("Screenshots")
}

/// 读取覆盖层需要的截图设置：手动保存初始目录、快捷保存目录、调色板。
fn capture_config(app: &AppHandle) -> (PathBuf, PathBuf, [u32; annotation::PALETTE_LEN]) {
    let default_palette = annotation::DEFAULT_PALETTE;
    let Some(state) = app.try_state::<crate::AppState>() else {
        let dir = fallback_save_dir();
        return (dir.clone(), dir, default_palette);
    };
    let Ok(registry) = state.registry.lock() else {
        let dir = default_save_dir(&state);
        return (dir.clone(), dir, default_palette);
    };
    // 只拷贝设置值，释放注册表锁后再解析目录（避免持锁做文件 IO）。
    let (manual_raw, quick_raw, palette_raw) = {
        let settings = registry.settings();
        (
            settings.screenshot_manual_save_dir.clone(),
            settings.screenshot_quick_save_dir.clone(),
            settings.screenshot_palette.clone(),
        )
    };
    drop(registry);
    let manual = resolve_save_dir(&state, &manual_raw);
    let quick = resolve_save_dir(&state, &quick_raw);
    (manual, quick, parse_palette(&palette_raw))
}

/// 由全局热键调用：抓屏 → 原生覆盖层选区 → 复制到剪贴板并保存 PNG。
///
/// 热键回调运行在主线程，这里只做守卫并另起线程，避免阻塞 UI。
pub fn capture_from_hotkey(app: &AppHandle) {
    if ACTIVE.swap(true, Ordering::SeqCst) {
        return;
    }
    let app = app.clone();
    std::thread::Builder::new()
        .name("screenshot-capture".to_owned())
        .spawn(move || {
            let outcome = run(&app);
            ACTIVE.store(false, Ordering::SeqCst);
            if let Some(state) = app.try_state::<crate::AppState>() {
                match outcome {
                    Ok(message) => crate::push_debug_log(&state, "screenshot", message),
                    Err(error) => {
                        crate::push_debug_log(&state, "screenshot", format!("screenshot.abort {error}"))
                    }
                }
            }
        })
        .ok();
}

fn run(app: &AppHandle) -> Result<String, String> {
    let (manual_dir, quick_dir, palette) = capture_config(app);
    annotation::set_palette(palette);
    let (vx, vy, width, height) = capture::virtual_rect()?;
    log(
        app,
        format!("screenshot.capture_begin size={width}x{height} origin=({vx}, {vy})"),
    );

    let outcome = overlay::select(width, height, manual_dir, quick_dir.clone())
        .ok_or_else(|| "cancelled".to_owned())?;
    let selection = outcome.selection;

    let png = save::encode_png(&outcome.top_down, selection.width as u32, selection.height as u32)?;
    clipboard_image::copy_image(
        selection.width,
        selection.height,
        &outcome.bottom_up,
        Some(&png),
    )?;
    if outcome.copy_only {
        return Ok(format!(
            "screenshot.copied size={}x{}",
            selection.width, selection.height
        ));
    }
    if outcome.saved {
        return Ok(format!(
            "screenshot.saved_manually size={}x{}",
            selection.width, selection.height
        ));
    }
    let path = save::write_png_to(&quick_dir, &png)?;

    Ok(format!(
        "screenshot.saved path={} size={}x{}",
        path.display(),
        selection.width,
        selection.height
    ))
}

fn log(app: &AppHandle, message: String) {
    if let Some(state) = app.try_state::<crate::AppState>() {
        crate::push_debug_log(&state, "screenshot", message);
    }
}

/// 工具被停用时调用：关闭可能仍打开的覆盖层。
pub fn stop() {
    overlay::abort();
}
