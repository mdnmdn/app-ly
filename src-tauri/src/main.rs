// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    if let Some(code) = app_ly_lib::maybe_run_cli() {
        std::process::exit(code);
    }
    app_ly_lib::run()
}
