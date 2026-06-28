use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};

use crate::clipboard;

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
    let label = "tool-clipboard-popup";
    let (width, height) = clipboard::panel_size();
    let cursor = app.cursor_position().map_err(|error| format!("读取鼠标位置失败: {error}"))?;
    let monitor = app
        .monitor_from_point(cursor.x, cursor.y)
        .map_err(|error| format!("读取屏幕信息失败: {error}"))?
        .or_else(|| app.available_monitors().ok().and_then(|monitors| monitors.into_iter().next()))
        .ok_or_else(|| "没有可用屏幕".to_owned())?;
    let work_area = monitor.work_area();
    let margin = 10.0;
    let max_x = work_area.position.x as f64 + work_area.size.width as f64 - width as f64 - margin;
    let max_y = work_area.position.y as f64 + work_area.size.height as f64 - height as f64 - margin;
    let x = (cursor.x - width as f64 / 2.0)
        .clamp(work_area.position.x as f64 + margin, max_x.max(work_area.position.x as f64 + margin));
    let y = cursor
        .y
        .clamp(work_area.position.y as f64 + margin, max_y.max(work_area.position.y as f64 + margin));

    if let Some(window) = app.get_webview_window(label) {
        let _ = window.set_position(tauri::Position::Physical(tauri::PhysicalPosition {
            x: x.round() as i32,
            y: y.round() as i32,
        }));
        let _ = window.show();
        let _ = window.set_focus();
        return Ok(());
    }

    WebviewWindowBuilder::new(app, label, WebviewUrl::App("index.html#/clipboard-popup".into()))
        .title("剪贴板")
        .inner_size(width as f64, height as f64)
        .position(x, y)
        .decorations(false)
        .resizable(false)
        .maximizable(false)
        .minimizable(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .focused(true)
        .visible(true)
        .build()
        .map_err(|error| format!("打开剪贴板弹窗失败: {error}"))?;
    Ok(())
}

pub fn close_clipboard_popup(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("tool-clipboard-popup") {
        let _ = window.close();
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
