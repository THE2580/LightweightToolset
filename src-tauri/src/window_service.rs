use std::{
    sync::atomic::{AtomicBool, Ordering},
    thread,
    time::Duration,
};

use tauri::{
    AppHandle, Manager, PhysicalPosition, PhysicalSize, Position, Size, WebviewUrl,
    WebviewWindowBuilder, WindowEvent,
};

use crate::{clipboard, push_debug_log, AppState};

static CLIPBOARD_POPUP_PINNED: AtomicBool = AtomicBool::new(false);
static CLIPBOARD_POPUP_DRAGGING: AtomicBool = AtomicBool::new(false);
static CLIPBOARD_POPUP_AUTO_CLOSE_SUSPENDED: AtomicBool = AtomicBool::new(false);

pub enum ToolWindowKind {
    QuickPopup,
    FreeWindow,
    FloatingWindow,
    TransparentOverlay,
}

pub fn close_tool_window(app: &AppHandle, tool_id: &str) {
    let label = format!("tool-{tool_id}");
    if let Some(window) = app.get_webview_window(&label) {
        let _ = window.close();
    }
    if tool_id == "clipboard" {
        close_clipboard_popup(app);
    }
    if tool_id == "timer" {
        close_timer_free_windows(app);
    }
}

pub fn open_timer_free_window(app: &AppHandle, timer_id: &str) -> Result<(), String> {
    let label = timer_free_window_label(timer_id);
    if let Some(window) = app.get_webview_window(&label) {
        focus_window(&window);
        log_window(app, "timer", format!("timer.free_window.focused id={timer_id}"));
        return Ok(());
    }

    let url = WebviewUrl::App(format!("free.html?kind=timer&id={timer_id}").into());
    let window = WebviewWindowBuilder::new(app, &label, url)
        .title("计时器")
        .inner_size(300.0, 186.0)
        .min_inner_size(260.0, 160.0)
        .resizable(true)
        .maximizable(false)
        .always_on_top(true)
        .visible(false)
        .build()
        .map_err(|error| format!("打开计时器自由窗口失败: {error}"))?;
    window.show().map_err(|error| format!("显示计时器自由窗口失败: {error}"))?;
    focus_window(&window);
    log_window(app, "timer", format!("timer.free_window.opened id={timer_id}"));
    Ok(())
}

pub fn open_timer_clock_window(app: &AppHandle) -> Result<(), String> {
    let label = "tool-timer-clock";
    if let Some(window) = app.get_webview_window(label) {
        focus_window(&window);
        log_window(app, "timer", "timer.clock_window.focused");
        return Ok(());
    }

    let window = WebviewWindowBuilder::new(app, label, WebviewUrl::App("free.html?kind=clock".into()))
        .title("本地时间")
        .inner_size(300.0, 156.0)
        .min_inner_size(260.0, 136.0)
        .resizable(true)
        .maximizable(false)
        .always_on_top(true)
        .visible(false)
        .build()
        .map_err(|error| format!("打开本地时间自由窗口失败: {error}"))?;
    window.show().map_err(|error| format!("显示本地时间自由窗口失败: {error}"))?;
    focus_window(&window);
    log_window(app, "timer", "timer.clock_window.opened");
    Ok(())
}

pub fn timer_free_window_count(app: &AppHandle) -> usize {
    app.webview_windows()
        .values()
        .filter(|window| {
            window.label() == "tool-timer-clock" || window.label().starts_with("tool-timer-free-")
        })
        .count()
}

fn close_timer_free_windows(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("tool-timer-clock") {
        let _ = window.close();
    }
    for window in app.webview_windows().into_values() {
        if window.label().starts_with("tool-timer-free-") {
            let _ = window.close();
        }
    }
}

fn timer_free_window_label(timer_id: &str) -> String {
    let safe_id: String = timer_id
        .chars()
        .map(|character| if character.is_ascii_alphanumeric() || character == '-' { character } else { '_' })
        .collect();
    format!("tool-timer-free-{safe_id}")
}

fn focus_window(window: &tauri::WebviewWindow) {
    let _ = window.unminimize();
    let _ = window.show();
    let _ = window.set_focus();
}

pub fn open_clipboard_popup(app: &AppHandle) -> Result<(), String> {
    log_popup(app, "info", "clipboard popup: open requested");
    CLIPBOARD_POPUP_PINNED.store(false, Ordering::Relaxed);
    clipboard::remember_paste_target_window();
    let result = open_clipboard_popup_inner(app);
    match &result {
        Ok(()) => log_popup(app, "info", "clipboard popup: open flow finished"),
        Err(error) => log_popup(app, "error", format!("clipboard popup: open failed: {error}")),
    }
    result
}

fn open_clipboard_popup_inner(app: &AppHandle) -> Result<(), String> {
    let label = "tool-clipboard-popup";
    let (width, height) = clipboard::panel_size();

    if let Some(window) = app.get_webview_window(label) {
        log_popup(app, "info", "clipboard popup: old window found, closing first");
        let _ = window.close();
    }

    let cursor = app
        .cursor_position()
        .map_err(|error| format!("read cursor position failed: {error}"))?;
    let monitor = app
        .monitor_from_point(cursor.x, cursor.y)
        .map_err(|error| format!("read monitor info failed: {error}"))?
        .or_else(|| app.available_monitors().ok().and_then(|monitors| monitors.into_iter().next()))
        .ok_or_else(|| "no available monitor".to_owned())?;
    let work_area = monitor.work_area();
    let margin = 10;
    let min_x = work_area.position.x + margin;
    let min_y = work_area.position.y + margin;
    let max_x = work_area.position.x + work_area.size.width as i32 - width as i32 - margin;
    let max_y = work_area.position.y + work_area.size.height as i32 - height as i32 - margin;
    let x = (cursor.x.round() as i32 - width as i32 / 2).clamp(min_x, max_x.max(min_x));
    let y = (cursor.y.round() as i32).clamp(min_y, max_y.max(min_y));
    log_popup(
        app,
        "info",
        format!(
            "clipboard popup: position cursor=({:.0},{:.0}) size={}x{} work_area=({},{} {}x{}) target=({},{})",
            cursor.x,
            cursor.y,
            width,
            height,
            work_area.position.x,
            work_area.position.y,
            work_area.size.width,
            work_area.size.height,
            x,
            y
        ),
    );

    let window = WebviewWindowBuilder::new(app, label, WebviewUrl::App("popup.html".into()))
        .title("Clipboard")
        .inner_size(width as f64, height as f64)
        .decorations(false)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(false)
        .visible(false)
        .build()
        .map_err(|error| format!("build clipboard popup failed: {error}"))?;
    log_popup(app, "info", "clipboard popup: webview created");

    window
        .set_size(Size::Physical(PhysicalSize { width, height }))
        .map_err(|error| format!("set clipboard popup size failed: {error}"))?;
    window
        .set_position(Position::Physical(PhysicalPosition { x, y }))
        .map_err(|error| format!("set clipboard popup position failed: {error}"))?;
    log_popup(app, "info", "clipboard popup: size and position applied");

    window.show().map_err(|error| format!("show clipboard popup failed: {error}"))?;
    window.set_focus().map_err(|error| format!("focus clipboard popup failed: {error}"))?;
    log_popup(app, "info", "clipboard popup: shown and focused");

    let app_for_focus = app.clone();
    window.on_window_event(move |event| {
        if matches!(event, WindowEvent::Focused(false)) && !CLIPBOARD_POPUP_DRAGGING.load(Ordering::Relaxed) {
            let app_for_close = app_for_focus.clone();
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(100));
                if CLIPBOARD_POPUP_PINNED.load(Ordering::Relaxed) {
                    if !CLIPBOARD_POPUP_DRAGGING.load(Ordering::Relaxed)
                        && !CLIPBOARD_POPUP_AUTO_CLOSE_SUSPENDED.load(Ordering::Relaxed)
                    {
                        clipboard::remember_paste_target_window();
                        log_popup(&app_for_close, "info", "clipboard popup: paste target updated after pinned focus loss");
                    }
                } else if !CLIPBOARD_POPUP_DRAGGING.load(Ordering::Relaxed)
                    && !CLIPBOARD_POPUP_AUTO_CLOSE_SUSPENDED.load(Ordering::Relaxed)
                {
                    log_popup(&app_for_close, "info", "clipboard popup: lost focus, closing");
                    close_clipboard_popup(&app_for_close);
                }
            });
        }
    });

    Ok(())
}

pub fn close_clipboard_popup(app: &AppHandle) {
    CLIPBOARD_POPUP_PINNED.store(false, Ordering::Relaxed);
    CLIPBOARD_POPUP_DRAGGING.store(false, Ordering::Relaxed);
    CLIPBOARD_POPUP_AUTO_CLOSE_SUSPENDED.store(false, Ordering::Relaxed);
    if let Some(window) = app.get_webview_window("tool-clipboard-popup") {
        log_popup(app, "info", "clipboard popup: close requested");
        let _ = window.close();
    }
}

pub fn set_clipboard_popup_pinned(pinned: bool) {
    CLIPBOARD_POPUP_PINNED.store(pinned, Ordering::Relaxed);
}

pub fn is_clipboard_popup_pinned() -> bool {
    CLIPBOARD_POPUP_PINNED.load(Ordering::Relaxed)
}

pub fn set_clipboard_popup_dragging(dragging: bool) {
    CLIPBOARD_POPUP_DRAGGING.store(dragging, Ordering::Relaxed);
}

pub fn start_clipboard_popup_drag(app: &AppHandle) {
    CLIPBOARD_POPUP_DRAGGING.store(true, Ordering::Relaxed);
    if let Some(window) = app.get_webview_window("tool-clipboard-popup") {
        let _ = window.start_dragging();
    }
    thread::spawn(|| {
        thread::sleep(Duration::from_millis(900));
        CLIPBOARD_POPUP_DRAGGING.store(false, Ordering::Relaxed);
    });
}

pub fn refocus_clipboard_popup_after_paste(app: &AppHandle) {
    CLIPBOARD_POPUP_AUTO_CLOSE_SUSPENDED.store(true, Ordering::Relaxed);
    let app = app.clone();
    thread::spawn(move || {
        thread::sleep(Duration::from_millis(260));
        if let Some(window) = app.get_webview_window("tool-clipboard-popup") {
            let _ = window.set_focus();
        }
        thread::sleep(Duration::from_millis(160));
        CLIPBOARD_POPUP_AUTO_CLOSE_SUSPENDED.store(false, Ordering::Relaxed);
    });
}

fn log_popup(app: &AppHandle, level: &'static str, message: impl Into<String>) {
    if let Some(state) = app.try_state::<AppState>() {
        let level = if level == "error" { "error" } else { "clipboard" };
        push_debug_log(&state, level, message);
    }
}

fn log_window(app: &AppHandle, level: &'static str, message: impl Into<String>) {
    if let Some(state) = app.try_state::<AppState>() {
        push_debug_log(&state, level, message);
    }
}

// Future window creation must route through this module so visibility protection,
// placement, focus behavior, and click-through rules stay centralized.
pub fn reserved_window_kinds() -> [ToolWindowKind; 4] {
    [
        ToolWindowKind::QuickPopup,
        ToolWindowKind::FreeWindow,
        ToolWindowKind::FloatingWindow,
        ToolWindowKind::TransparentOverlay,
    ]
}
