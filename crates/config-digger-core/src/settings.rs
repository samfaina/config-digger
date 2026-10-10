//! The app's settings, kept beside the user's lists in the roaming data
//! folder, `%APPDATA%\config-digger\settings.json`. Like the lists, the file
//! is written by atomic replace without the global lock, since it holds no
//! Agent configuration. The CLI never reads it.

use crate::api::{Error, Settings};
use crate::ports::{Environment, Ports};
use crate::user_list;
use serde::{Deserialize, Serialize};
use std::io;
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
    let bytes = match ports.fs.read(&file) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(Settings::from(SettingsFile::default()));
        }
        Err(error) => {
            return Err(Error::unexpected(format!(
                "cannot read {}: {error}",
                file.display()
            )));
        }
    };
    let saved: SettingsFile = serde_json::from_slice(&bytes).map_err(|error| {
        Error::unexpected(format!(
            "{} is not a settings file config-digger can read ({error}); fix or delete it",
            file.display()
        ))
    })?;
    if saved.version != VERSION {
        return Err(Error::unexpected(format!(
            "{} has version {}, which this config-digger cannot read",
            file.display(),
            saved.version
        )));
    }
    Ok(Settings::from(saved))
}

/// Replaces the file with `settings`, through a temporary file and a rename.
pub(crate) fn save(ports: &Ports, settings: &Settings) -> Result<(), Error> {
    let file = file(ports.env.as_ref())?;
    let cannot =
        |error: io::Error| Error::unexpected(format!("cannot write {}: {error}", file.display()));
    if let Some(folder) = file.parent() {
        ports.fs.create_dir_all(folder).map_err(cannot)?;
    }
    let mut json = serde_json::to_vec_pretty(&SettingsFile {
        version: VERSION,
        check_updates_on_startup: settings.check_updates_on_startup,
    })
    .expect("the settings serialise");
    json.push(b'\n');
    ports.fs.write_atomic(&file, &json).map_err(cannot)
}

impl From<SettingsFile> for Settings {
    fn from(file: SettingsFile) -> Self {
        Settings {
            check_updates_on_startup: file.check_updates_on_startup,
        }
    }
}
