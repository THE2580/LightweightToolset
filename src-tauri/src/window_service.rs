use tauri::{AppHandle, Manager};

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
