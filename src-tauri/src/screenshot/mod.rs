mod annotation;
mod capture;
mod clipboard_image;
mod dib;
mod overlay;
mod toolbar;
mod save;

use std::sync::atomic::{AtomicBool, Ordering};

use tauri::{AppHandle, Manager};

static ACTIVE: AtomicBool = AtomicBool::new(false);

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
    let (vx, vy, width, height) = capture::virtual_rect()?;
    log(
        app,
        format!("screenshot.capture_begin size={width}x{height} origin=({vx}, {vy})"),
    );

    let outcome = overlay::select(width, height).ok_or_else(|| "cancelled".to_owned())?;
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
    let path = save::write_file(&png)?;

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
