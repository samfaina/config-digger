//! The app's settings, kept beside the user's lists in the roaming data
//! folder, `%APPDATA%\config-digger\settings.json`. Like the lists, the file
//! is written by atomic replace without the global lock, since it holds no
//! Agent configuration. The CLI never reads it.

use crate::api::{Error, Settings};
use crate::ports::{Environment, Ports};
use crate::user_list;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The version of the file's format.
const VERSION: u32 = 1;

/// The name of the file in the roaming data folder.
const FILE: &str = "settings.json";

/// The file as it is on disk. A setting it lacks has its default.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SettingsFile {
    #[serde(default = "version")]
    version: u32,
    #[serde(default)]
    check_updates_on_startup: bool,
}

fn version() -> u32 {
    VERSION
}

/// The file the settings are kept in.
pub(crate) fn file(env: &dyn Environment) -> Result<PathBuf, Error> {
    Ok(user_list::data_folder(env)?.join(FILE))
}

/// The settings as they are on disk: the defaults when the file is missing.
pub(crate) fn load(ports: &Ports) -> Result<Settings, Error> {
    let file = file(ports.env.as_ref())?;
    let Some(saved) = user_list::read_json::<SettingsFile>(ports, &file, "a settings file")? else {
        return Ok(Settings::from(SettingsFile::default()));
    };
    user_list::check_version(&file, saved.version, VERSION)?;
    Ok(Settings::from(saved))
}

/// Replaces the file with `settings`, through a temporary file and a rename.
pub(crate) fn save(ports: &Ports, settings: &Settings) -> Result<(), Error> {
    let file = file(ports.env.as_ref())?;
    let saved = SettingsFile {
        version: VERSION,
        check_updates_on_startup: settings.check_updates_on_startup,
    };
    user_list::write_json(ports, &file, &saved)
}

impl From<SettingsFile> for Settings {
    fn from(file: SettingsFile) -> Self {
        Settings {
            check_updates_on_startup: file.check_updates_on_startup,
        }
    }
}
