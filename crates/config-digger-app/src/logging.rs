//! The app's rotating log in `%LOCALAPPDATA%\config-digger\logs\`. Like every
//! config-digger log it carries only paths, key paths, block ids, reason codes
//! and errors, never configuration values.

use std::path::PathBuf;
use tracing_appender::non_blocking::WorkerGuard;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::EnvFilter;

/// One file per day; older ones are deleted beyond this count.
const KEPT_LOG_FILES: usize = 14;

/// Only config-digger's own events, and no way to widen them: the webview
/// stack logs URLs and page content we have no control over.
const FILTER: &str = "warn,config_digger_app=info,config_digger_core=info";

/// Starts logging to the rotating file. Without `LOCALAPPDATA`, or when the
/// folder cannot be created, the app runs without a log.
pub(crate) fn init() -> Option<WorkerGuard> {
    let appender = RollingFileAppender::builder()
        .rotation(Rotation::DAILY)
        .filename_prefix("config-digger-app")
        .filename_suffix("log")
        .max_log_files(KEPT_LOG_FILES)
        .build(logs_dir()?)
        .ok()?;
    let (writer, guard) = tracing_appender::non_blocking(appender);
    tracing_subscriber::fmt()
        .with_writer(writer)
        .with_ansi(false)
        .with_env_filter(EnvFilter::new(FILTER))
        .try_init()
        .ok()?;
    Some(guard)
}

fn logs_dir() -> Option<PathBuf> {
    let local_app_data = std::env::var_os("LOCALAPPDATA").filter(|value| !value.is_empty())?;
    Some(
        PathBuf::from(local_app_data)
            .join("config-digger")
            .join("logs"),
    )
}
