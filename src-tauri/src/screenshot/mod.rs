mod annotation;
mod capture;
mod clipboard_image;
mod dib;
mod overlay;
mod pin;
mod toolbar;
mod save;

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Manager};

static ACTIVE: AtomicBool = AtomicBool::new(false);
/// 最近一次截图产出的图，没有覆盖层时按贴图快捷键用它。
static LAST_SHOT: Mutex<Option<Arc<pin::PinImage>>> = Mutex::new(None);
/// 覆盖层「贴图」按钮/快捷键的提示文本（工具栏悬停用）。
static PIN_HOTKEY: Mutex<String> = Mutex::new(String::new());

/// 贴图快捷键的显示文本，工具栏提示与设置页共用。
pub(crate) fn pin_hotkey_text() -> String {
    let text = PIN_HOTKEY.lock().map(|value| value.clone()).unwrap_or_default();
    if text.is_empty() {
        "未设置".to_owned()
    } else {
        text
    }
}

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

/// 压暗层默认不透明度（百分比）。
pub(crate) const DEFAULT_DIM_OPACITY: u8 = 45;

pub(crate) fn default_file_template() -> String {
    save::DEFAULT_TEMPLATE.to_owned()
}

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

/// 百分比 → AlphaBlend 的 0–255 不透明度。
fn opacity_to_alpha(opacity: u8) -> u8 {
    let percent = opacity.min(100) as u32;
    ((percent * 255 + 50) / 100) as u8
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

/// 读取覆盖层需要的截图设置：目录、文件名模板、调色板、外观项与贴图选项。
fn capture_config(
    app: &AppHandle,
) -> (
    overlay::OverlayOptions,
    [u32; annotation::PALETTE_LEN],
    pin::PinOptions,
    usize,
) {
    let Some(state) = app.try_state::<crate::AppState>() else {
        let dir = fallback_save_dir();
        return (
            overlay::OverlayOptions::defaults(dir),
            annotation::DEFAULT_PALETTE,
            pin::PinOptions::default(),
            20,
        );
    };
    let Ok(registry) = state.registry.lock() else {
        let dir = default_save_dir(&state);
        return (
            overlay::OverlayOptions::defaults(dir),
            annotation::DEFAULT_PALETTE,
            pin::PinOptions::default(),
            20,
        );
    };
    // 只拷贝设置值，释放注册表锁后再解析目录（避免持锁做文件 IO）。
    let settings = registry.settings().clone();
    let pin_hotkey = registry.app_hotkey_text("screenshot_pin");
    drop(registry);

    if let Ok(mut text) = PIN_HOTKEY.lock() {
        *text = pin_hotkey;
    }

    let manual = resolve_save_dir(&state, &settings.screenshot_manual_save_dir);
    let quick = resolve_save_dir(&state, &settings.screenshot_quick_save_dir);
    let options = overlay::OverlayOptions {
        manual_dir: manual,
        quick_dir: quick,
        file_template: settings.screenshot_file_template.clone(),
        dim_color: annotation::parse_hex_color(&settings.screenshot_dim_color).unwrap_or(0),
        dim_alpha: opacity_to_alpha(settings.screenshot_dim_opacity),
        grid: settings.screenshot_magnifier_grid,
        grid_color: annotation::parse_hex_color(&settings.screenshot_magnifier_grid_color)
            .unwrap_or(annotation::rgb(0x66, 0x66, 0x66)),
        guides_mode: overlay::GuidesMode::parse(&settings.screenshot_guides_mode),
        guides_color: annotation::parse_hex_color(&settings.screenshot_guides_color)
            .unwrap_or(annotation::rgb(0x30, 0xB4, 0xFF)),
        cursor_guide_color: annotation::parse_hex_color(&settings.screenshot_cursor_guide_color)
            .unwrap_or(annotation::rgb(0x80, 0x80, 0x80)),
        guides_width: settings.screenshot_guides_width.clamp(1, 5) as i32,
        cursor_guide_width: settings.screenshot_cursor_guide_width.clamp(1, 5) as i32,
    };
    let pin_options = pin::PinOptions {
        shadow: settings.screenshot_pin_shadow,
        confirm_close: settings.screenshot_pin_confirm_close,
        double_click: pin::DoubleClick::parse(&settings.screenshot_pin_double_click),
        lock: pin::PinLock::parse(&settings.screenshot_pin_lock),
    };
    (
        options,
        parse_palette(&settings.screenshot_palette),
        pin_options,
        settings.screenshot_pin_history.clamp(1, 100) as usize,
    )
}

/// 只取贴图相关选项（不抓屏时用）。
fn pin_config(app: &AppHandle) -> (pin::PinOptions, usize, PathBuf) {
    let Some(state) = app.try_state::<crate::AppState>() else {
        return (pin::PinOptions::default(), 20, fallback_save_dir());
    };
    let Ok(registry) = state.registry.lock() else {
        return (pin::PinOptions::default(), 20, default_save_dir(&state));
    };
    let settings = registry.settings().clone();
    drop(registry);
    let options = pin::PinOptions {
        shadow: settings.screenshot_pin_shadow,
        confirm_close: settings.screenshot_pin_confirm_close,
        double_click: pin::DoubleClick::parse(&settings.screenshot_pin_double_click),
        lock: pin::PinLock::parse(&settings.screenshot_pin_lock),
    };
    (
        options,
        settings.screenshot_pin_history.clamp(1, 100) as usize,
        data_dir(&state),
    )
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
    let (options, palette, pin_options, pin_history) = capture_config(app);
    annotation::set_palette(palette);
    // 提前把贴图线程与窗口类准备好，贴图只走建窗口这一步。
    pin::warm_up();
    let quick_dir = options.quick_dir.clone();
    let template = options.file_template.clone();
    let (vx, vy, width, height) = capture::virtual_rect()?;
    log(
        app,
        format!("screenshot.capture_begin size={width}x{height} origin=({vx}, {vy})"),
    );

    let outcome =
        overlay::select(width, height, options).ok_or_else(|| "cancelled".to_owned())?;
    let selection = outcome.selection;

    let image = Arc::new(pin::PinImage {
        width: selection.width,
        height: selection.height,
        top_down: outcome.top_down,
    });
    // 记住这次截图，这样没有覆盖层时按贴图快捷键也能贴出来。
    if let Ok(mut last) = LAST_SHOT.lock() {
        *last = Some(Arc::clone(&image));
    }

    if outcome.pin {
        // 先把窗口贴出来，历史落盘放到后台，避免写盘拖慢首次呈现。
        let rect = windows_sys::Win32::Foundation::RECT {
            left: selection.x,
            top: selection.y,
            right: selection.x + selection.width,
            bottom: selection.y + selection.height,
        };
        pin::create(Arc::clone(&image), rect, pin_options);
        if let Some(state) = app.try_state::<crate::AppState>() {
            let dir = data_dir(&state);
            std::thread::spawn(move || {
                let _ = pin::record_history(&dir, &image, pin_history);
            });
        }
        return Ok(format!(
            "screenshot.pinned size={}x{}",
            selection.width, selection.height
        ));
    }

    let png = save::encode_png(&image.top_down, selection.width as u32, selection.height as u32)?;
    // CF_DIB 需要自下而上的行序，只在复制时临时翻转。
    let bottom_up = pin::flip_rows(&image.top_down, selection.width, selection.height);
    clipboard_image::copy_image(
        selection.width,
        selection.height,
        &bottom_up,
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
    let path = save::write_png_to(&quick_dir, &png, &template)?;

    Ok(format!(
        "screenshot.saved path={} size={}x{}",
        path.display(),
        selection.width,
        selection.height
    ))
}

/// 贴图快捷键：覆盖层打开时钉当前选区，否则钉最近一次截图。
pub fn pin_from_hotkey(app: &AppHandle) {
    if overlay::request_pin() {
        return;
    }
    let image = LAST_SHOT.lock().ok().and_then(|last| last.clone());
    let Some(image) = image else {
        log(app, "screenshot.pin_skipped reason=no_shot".to_owned());
        return;
    };
    let (options, history, dir) = pin_config(app);
    let rect = windows_sys::Win32::Foundation::RECT {
        left: 120,
        top: 120,
        right: 120 + image.width,
        bottom: 120 + image.height,
    };
    pin::record_history(&dir, &image, history);
    pin::create(image, rect, options);
    log(app, "screenshot.pin_from_last".to_owned());
}

/// 恢复上次关闭的贴图。
pub fn restore_pin(app: &AppHandle) {
    let (options, _, dir) = pin_config(app);
    let restored = pin::restore_last(&dir, options);
    log(
        app,
        format!("screenshot.pin_restore ok={restored}"),
    );
}

/// 关闭所有贴图窗口。
pub fn close_all_pins() {
    pin::close_all();
}

/// 隐藏 / 显示所有贴图窗口。
pub fn toggle_pins_visible() {
    pin::toggle_visible();
}

/// 切换所有贴图窗口的鼠标穿透。
pub fn toggle_pins_passthrough() {
    pin::toggle_passthrough();
}

fn log(app: &AppHandle, message: String) {
    if let Some(state) = app.try_state::<crate::AppState>() {
        crate::push_debug_log(&state, "screenshot", message);
    }
}

/// 工具被停用时调用：关闭可能仍打开的覆盖层与全部贴图窗口。
pub fn stop() {
    overlay::abort();
    pin::stop_all();
}
