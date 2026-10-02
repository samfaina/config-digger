//! The `config-digger-app` executable. Everything but the embedded frontend
//! lives in the library, so `xtask` can generate the bindings without it.

// Release builds run without a console window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    config_digger_app::run(tauri::generate_context!());
}
