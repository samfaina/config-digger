//! Task-level commands: each one is a thin async wrapper that runs the
//! synchronous core on a blocking thread.

use crate::session::{self, Session};
use config_digger_core::api::{self, Backup, Core, Inventory};
use config_digger_core::ports::Ports;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};
use std::time::Instant;
use tauri::{AppHandle, Manager};

/// The app's [`Session`], once the inventory was read.
#[derive(Default)]
pub struct SessionState(Mutex<Option<Session>>);

impl SessionState {
    fn lock(&self) -> MutexGuard<'_, Option<Session>> {
        // A panic in another command leaves the session as it was.
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// Reads every Agent home and resolves its Items for `project`, or for no
/// Project, leaving out the Agent homes and Projects the user hid.
#[tauri::command]
#[specta::specta]
pub async fn read_inventory(
    app: AppHandle,
    project: Option<PathBuf>,
) -> Result<Inventory, api::Error> {
    let started = Instant::now();
    let result = blocking(move || {
        let mut session = scan()?;
        let inventory = session.resolve(project);
        *app.state::<SessionState>().lock() = Some(session);
        Ok(inventory)
    })
    .await;

    let elapsed_ms = started.elapsed().as_millis();
    match &result {
        Ok(inventory) => tracing::info!(
            homes = inventory.homes.len(),
            items = inventory.items.len(),
            read_failures = inventory.read_failures.len(),
            elapsed_ms,
            "inventory read"
        ),
        Err(error) => tracing::error!(%error, elapsed_ms, "inventory failed"),
    }
    result
}

/// Resolves the last inventory read for `project`, or for no Project,
/// without reading the disk again. Reads it first if it was never read.
#[tauri::command]
#[specta::specta]
pub async fn switch_project(
    app: AppHandle,
    project: Option<PathBuf>,
) -> Result<Inventory, api::Error> {
    blocking(move || {
        let state = app.state::<SessionState>();
        let mut session = state.lock();
        if session.is_none() {
            *session = Some(scan()?);
        }
        Ok(session.as_mut().expect("read above").resolve(project))
    })
    .await
}

/// The backups History holds of `path`, a location the inventory shows,
/// newest first.
#[tauri::command]
#[specta::specta]
pub async fn backups_of(app: AppHandle, path: PathBuf) -> Result<Vec<Backup>, api::Error> {
    blocking(move || match app.state::<SessionState>().lock().as_mut() {
        Some(session) => session.backups_of(&path),
        None => Err(session::not_shown(&path)),
    })
    .await
}

/// Opens `path`, a location the inventory shows, in the app Windows
/// associates with it, or shows it selected in Explorer when it is a
/// folder, such as a skill's.
#[tauri::command]
#[specta::specta]
pub async fn open_location(app: AppHandle, path: PathBuf) -> Result<(), api::Error> {
    blocking(move || {
        ensure_shown(&app, &path)?;
        // A link, even a broken one, is shown selected rather than followed.
        let link = std::fs::symlink_metadata(&path).is_ok_and(|meta| meta.file_type().is_symlink());
        if link || path.is_dir() {
            select_in_explorer(&path)
        } else {
            open_file(&path)
        }
    })
    .await
}

/// Shows `path`, a location the inventory shows or a backup copy of one,
/// selected in Explorer.
#[tauri::command]
#[specta::specta]
pub async fn reveal_location(app: AppHandle, path: PathBuf) -> Result<(), api::Error> {
    blocking(move || {
        ensure_shown(&app, &path)?;
        select_in_explorer(&path)
    })
    .await
}

/// Reads every Agent home and Project on this machine.
fn scan() -> Result<Session, api::Error> {
    Session::scan(Core::new(Ports::real())?)
}

/// Runs `task` on a blocking thread.
async fn blocking<T: Send + 'static>(
    task: impl FnOnce() -> Result<T, api::Error> + Send + 'static,
) -> Result<T, api::Error> {
    tauri::async_runtime::spawn_blocking(task)
        .await
        .unwrap_or_else(|error| Err(api::Error::unexpected(error)))
}

/// Refuses a path the inventory never showed: the webview opens nothing else.
fn ensure_shown(app: &AppHandle, path: &Path) -> Result<(), api::Error> {
    let state = app.state::<SessionState>();
    match state.lock().as_ref() {
        Some(session) if session.can_open(path) => Ok(()),
        _ => Err(session::not_shown(path)),
    }
}

/// Opens `path` in the app Windows associates with it.
fn open_file(path: &Path) -> Result<(), api::Error> {
    let mut explorer = Command::new("explorer.exe");
    explorer.arg(path);
    start(explorer)
}

/// Opens Explorer on the folder holding `path`, with `path` selected.
#[cfg(windows)]
fn select_in_explorer(path: &Path) -> Result<(), api::Error> {
    use std::os::windows::process::CommandExt;
    // Explorer reads `/select,` and the quoted path as one argument, which
    // the usual quoting of a whole argument breaks. No Windows path holds `"`.
    let mut explorer = Command::new("explorer.exe");
    explorer.raw_arg(format!("/select,\"{}\"", path.display()));
    start(explorer)
}

#[cfg(not(windows))]
fn select_in_explorer(_path: &Path) -> Result<(), api::Error> {
    Err(api::Error::unexpected("Explorer is only on Windows"))
}

/// Starts Explorer. It exits with 1 even when it succeeds, so only a
/// failure to start it is one.
fn start(mut explorer: Command) -> Result<(), api::Error> {
    explorer
        .spawn()
        .map(drop)
        .map_err(|error| api::Error::unexpected(format!("cannot start Explorer: {error}")))
}
