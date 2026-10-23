//! The opt-in updater: Tauri's updater, called from Rust only, so the
//! webview gets no `updater:*` permission. It reaches the network only when
//! the user asks, with Check now, or once at startup when "Check for updates
//! on startup" is on, and installs only on Install and restart.

use crate::about::Page;
use crate::commands::blocking;
use config_digger_core::api::{self, Core};
use config_digger_core::ports::Ports;
use serde::Serialize;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, MutexGuard};
use tauri::AppHandle;
use tauri_plugin_updater::{Update, UpdaterExt};

/// An update a check found, as its notice shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
pub struct UpdateNotice {
    /// The version it installs.
    pub version: String,
    /// Its release notes on GitHub.
    pub release_notes: String,
}

impl UpdateNotice {
    /// The notice of an update to `version`.
    pub fn of(version: &str) -> Self {
        UpdateNotice {
            version: version.to_owned(),
            release_notes: Page::ReleaseNotes.url(env!("CARGO_PKG_REPOSITORY"), version),
        }
    }
}

/// What the app knows of updates, without checking.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
pub struct UpdateStatus {
    /// When the app last checked, in RFC 3339, `None` when it never did.
    pub last_checked: Option<String>,
    /// The update the last check since the app started found, `None` when
    /// it found none or there was no check.
    pub update: Option<UpdateNotice>,
}

/// The update found since the app started, and whether the startup check
/// already ran.
#[derive(Default)]
pub struct UpdatesState {
    startup_check_ran: AtomicBool,
    found: Mutex<Option<Update>>,
}

impl UpdatesState {
    fn found(&self) -> MutexGuard<'_, Option<Update>> {
        // A panic in another command leaves the update as it was.
        self.found
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// What the app knows of updates, read without a network call.
    pub async fn read_status(&self) -> Result<UpdateStatus, api::Error> {
        let last_checked = blocking(|| Core::new(Ports::real())?.last_update_check()).await?;
        Ok(UpdateStatus {
            last_checked,
            update: self.notice(),
        })
    }

    fn notice(&self) -> Option<UpdateNotice> {
        self.found()
            .as_ref()
            .map(|update| UpdateNotice::of(&update.version))
    }

    /// Checks for updates on the first call since the app started, when
    /// the user turned "Check for updates on startup" on. Otherwise it makes
    /// no network call and only reads what the app knows.
    pub async fn startup_check(&self, app: &AppHandle) -> Result<UpdateStatus, api::Error> {
        let first = !self.startup_check_ran.swap(true, Ordering::SeqCst);
        let on = first
            && blocking(|| {
                Ok(Core::new(Ports::real())?
                    .settings()?
                    .check_updates_on_startup)
            })
            .await?;
        if on {
            self.check(app).await
        } else {
            self.read_status().await
        }
    }

    /// The release notes of the update found, `None` when none was.
    pub fn release_notes(&self) -> Option<String> {
        self.notice().map(|notice| notice.release_notes)
    }

    /// Checks GitHub Releases for an update: the one network call, made only
    /// when the user asks or turned the startup check on. The time is
    /// recorded once the check succeeds, whatever it finds.
    pub async fn check(&self, app: &AppHandle) -> Result<UpdateStatus, api::Error> {
        let result = self.check_inner(app).await;
        if let Err(error) = &result {
            tracing::warn!(%error, "update check failed");
        }
        result
    }

    async fn check_inner(&self, app: &AppHandle) -> Result<UpdateStatus, api::Error> {
        let update = app
            .updater()
            .map_err(failed_check)?
            .check()
            .await
            .map_err(failed_check)?;
        let checked_at = blocking(|| Core::new(Ports::real())?.record_update_check()).await?;
        tracing::info!(
            found = update.as_ref().map(|update| update.version.as_str()),
            "updates checked"
        );
        *self.found() = update;
        Ok(UpdateStatus {
            last_checked: Some(checked_at),
            update: self.notice(),
        })
    }

    /// Downloads the update found, checks its signature, and runs its
    /// installer in passive mode, which replaces both executables and
    /// restarts the app. It fails at once while an Operation holds the
    /// global write lock or the CLI next to the app runs, and holds the lock
    /// while the update downloads, so no Operation starts then. On Windows
    /// the app exits once the installer started, which releases the lock.
    pub async fn install(&self) -> Result<(), api::Error> {
        let result = self.install_inner().await;
        if let Err(error) = &result {
            tracing::warn!(code = error.code.code(), "update not installed");
        }
        result
    }

    async fn install_inner(&self) -> Result<(), api::Error> {
        let Some(update) = self.found().clone() else {
            return Err(api::Error::unexpected(
                "no update was found: check for updates first",
            ));
        };
        let cli = std::env::current_exe()
            .map_err(api::Error::unexpected)?
            .with_file_name(CLI_EXE);
        let lock = blocking(move || Core::new(Ports::real())?.lock_for_update(&cli)).await?;
        tracing::info!(version = %update.version, "update installing");
        let installed = update.download_and_install(|_, _| {}, || {}).await;
        drop(lock);
        installed.map_err(failed_install)
    }
}

/// The CLI the installer puts next to the app, and an update replaces.
const CLI_EXE: &str = "config-digger.exe";

/// A check that failed, with only its cause: About says the check failed.
fn failed_check(error: tauri_plugin_updater::Error) -> api::Error {
    api::Error::unexpected(error)
}

/// A download or install that failed, with only its cause: About says the
/// update was not installed.
fn failed_install(error: tauri_plugin_updater::Error) -> api::Error {
    api::Error::unexpected(error)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_found_update_links_to_its_own_release_notes() {
        let notice = UpdateNotice::of("0.2.0");

        assert_eq!(
            notice,
            UpdateNotice {
                version: "0.2.0".to_owned(),
                release_notes: "https://github.com/samfaina/config-digger/releases/tag/v0.2.0"
                    .to_owned(),
            }
        );
    }

    #[test]
    fn a_failed_check_carries_only_its_cause() {
        let error = failed_check(tauri_plugin_updater::Error::ReleaseNotFound);

        assert_eq!(
            error.message,
            "Could not fetch a valid release JSON from the remote"
        );
    }

    #[test]
    fn a_failed_install_carries_only_its_cause() {
        let error = failed_install(tauri_plugin_updater::Error::InvalidUpdaterFormat);

        assert_eq!(error.message, "invalid updater binary format");
    }
}
