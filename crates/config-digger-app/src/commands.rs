//! Task-level commands: each one is a thin async wrapper that runs the
//! synchronous core on a blocking thread.

use config_digger_core::api::{self, Context, Core, Inventory, ScanOptions};
use config_digger_core::ports::Ports;
use std::time::Instant;

/// Reads every Agent home and resolves its Items with no Project, leaving
/// out the Agent homes and Projects the user hid.
#[tauri::command]
#[specta::specta]
pub async fn read_inventory() -> Result<Inventory, api::Error> {
    let started = Instant::now();
    let result = tauri::async_runtime::spawn_blocking(|| {
        let core = Core::new(Ports::real())?;
        let snapshot = core.scan(&ScanOptions::default())?;
        Ok(core
            .resolve(&snapshot, &Context::default())
            .without_hidden())
    })
    .await
    .unwrap_or_else(|error| Err(api::Error::unexpected(error)));

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
