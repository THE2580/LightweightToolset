use tauri::{
    AppHandle, Manager, PhysicalPosition, PhysicalSize, Position, Size, WebviewUrl,
    WebviewWindowBuilder,
};

use crate::{clipboard, push_debug_log, AppState};

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
}

pub fn open_clipboard_popup(app: &AppHandle) -> Result<(), String> {
    log_popup(app, "info", "clipboard popup: open requested");
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

    let window = WebviewWindowBuilder::new(app, label, WebviewUrl::App("index.html".into()))
        .title("Clipboard")
        .inner_size(width as f64, height as f64)
        .position(x as f64, y as f64)
        .decorations(false)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .always_on_top(false)
        .skip_taskbar(true)
        .focused(true)
        .visible(true)
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

    Ok(())
}

pub fn close_clipboard_popup(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("tool-clipboard-popup") {
        log_popup(app, "info", "clipboard popup: close requested");
        let _ = window.close();
    }
}

fn log_popup(app: &AppHandle, level: &'static str, message: impl Into<String>) {
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
