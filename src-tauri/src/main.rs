// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if lightweight_toolset_lib::run_window_pinner_watchdog_from_args() {
        return;
    }
    if lightweight_toolset_lib::run_elevated_input_helper_from_args() {
        return;
    }
    lightweight_toolset_lib::mark_process_start();
    lightweight_toolset_lib::run()
}
