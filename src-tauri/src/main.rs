// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Some(port) = shortcut_helper_port() {
        lightweight_toolset_lib::run_shortcut_helper(port);
    }
    if lightweight_toolset_lib::run_window_pinner_watchdog_from_args() {
        return;
    }
    lightweight_toolset_lib::mark_process_start();
    lightweight_toolset_lib::run()
}

fn shortcut_helper_port() -> Option<u16> {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--shortcut-tip-helper" {
            return args.next().and_then(|value| value.parse::<u16>().ok());
        }
    }
    None
}
