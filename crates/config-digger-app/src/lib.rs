//! The config-digger desktop app: a thin Tauri 2 shell that calls the core
//! in-process. The webview reaches the core only through the task-level
//! commands in [`commands`], granted by the single `main` capability.
//!
//! The React/TypeScript UI lives in `ui/`. Run the app with `npm run tauri dev`
//! from there; after changing a command or an api DTO, run `cargo xtask bindings`.

mod commands;
mod logging;

use specta_typescript::Typescript;
use std::path::Path;

/// The header of the generated TypeScript bindings.
const BINDINGS_HEADER: &str =
    "// Generated from the api DTOs by `cargo xtask bindings`. Do not edit.";

/// The commands the webview can call. `build.rs` lists the same names.
fn specta_builder() -> tauri_specta::Builder<tauri::Wry> {
    tauri_specta::Builder::new().commands(tauri_specta::collect_commands![commands::read_inventory])
}

/// Writes the TypeScript types and typed command wrappers to `path`.
pub fn export_bindings(path: &Path) -> Result<(), specta_typescript::Error> {
    specta_builder().export(Typescript::default().header(BINDINGS_HEADER), path)
}

/// Runs the app until its window closes.
pub fn run(context: tauri::Context<tauri::Wry>) {
    // Dropping the guard flushes the log, so it lives until the app exits.
    let _log = logging::init();
    tracing::info!(version = env!("CARGO_PKG_VERSION"), "app started");

    tauri::Builder::default()
        .invoke_handler(specta_builder().invoke_handler())
        .run(context)
        .expect("the Tauri app runs");
}
