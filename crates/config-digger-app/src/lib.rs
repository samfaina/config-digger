//! The config-digger desktop app: a thin Tauri 2 shell that calls the core
//! in-process. The webview reaches the core only through the task-level
//! commands in [`commands`], granted by the single `main` capability.
//!
//! The React/TypeScript UI lives in `ui/`. Run the app with `npm run tauri dev`
//! from there; after changing a command or an api DTO, run `cargo xtask bindings`.

mod actions;
mod commands;
mod logging;
mod session;

use config_digger_core::api::Core;
use config_digger_core::ports::Ports;
use specta_typescript::Typescript;
use std::path::Path;

/// The header of the generated TypeScript bindings.
const BINDINGS_HEADER: &str =
    "// Generated from the api DTOs by `cargo xtask bindings`. Do not edit.";

/// The commands the webview can call. `build.rs` lists the same names.
fn specta_builder() -> tauri_specta::Builder<tauri::Wry> {
    tauri_specta::Builder::new().commands(tauri_specta::collect_commands![
        commands::read_inventory,
        commands::switch_project,
        commands::backups_of,
        commands::open_location,
        commands::reveal_location,
        commands::plan_action,
        commands::execute_plan,
        commands::cancel_plan,
        commands::read_history,
        commands::history_detail,
        commands::dismiss_operation,
    ])
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
    // History is pruned at startup, off the UI thread. It is best-effort: a
    // held lock or a failure leaves it for the next start.
    std::thread::spawn(|| {
        if let Err(error) = Core::new(Ports::real()).and_then(|core| core.prune_history()) {
            tracing::warn!(%error, "history not pruned");
        }
    });

    tauri::Builder::default()
        .manage(commands::SessionState::default())
        .manage(commands::ActionsState::default())
        .invoke_handler(specta_builder().invoke_handler())
        .run(context)
        .expect("the Tauri app runs");
}
