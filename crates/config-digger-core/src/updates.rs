//! What the app keeps of its update checks: when it last checked, in
//! `%LOCALAPPDATA%\config-digger\update-check.json`, since the time is this
//! machine's. The check itself is the app's: the core makes no network call.
//! Like the settings, the file is written by atomic replace without the
//! global lock, and the CLI never reads it.

use crate::api::Error;
use crate::ports::Ports;
use crate::{history, user_list};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The version of the file's format.
const VERSION: u32 = 1;

/// The name of the file in the local data folder.
const FILE: &str = "update-check.json";

/// The file as it is on disk.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CheckFile {
    version: u32,
    /// When the app last checked, in RFC 3339.
    checked_at: String,
}

fn file(ports: &Ports) -> Result<PathBuf, Error> {
    Ok(history::data_folder(ports.env.as_ref())?.join(FILE))
}

/// When the app last checked for updates, `None` when it never did.
pub(crate) fn last(ports: &Ports) -> Result<Option<String>, Error> {
    let file = file(ports)?;
    let Some(saved) = user_list::read_json::<CheckFile>(ports, &file, "an update check record")?
    else {
        return Ok(None);
    };
    user_list::check_version(&file, saved.version, VERSION)?;
    Ok(Some(saved.checked_at))
}

/// Records that the app checks for updates now, and returns the time.
pub(crate) fn record(ports: &Ports) -> Result<String, Error> {
    let checked_at = humantime::format_rfc3339_seconds(ports.clock.now()).to_string();
    let saved = CheckFile {
        version: VERSION,
        checked_at: checked_at.clone(),
    };
    user_list::write_json(ports, &file(ports)?, &saved)?;
    Ok(checked_at)
}
