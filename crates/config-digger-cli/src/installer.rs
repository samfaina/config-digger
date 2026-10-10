//! `installer`: the hidden commands the NSIS installer and uninstaller run.
//! They are no part of the CLI's public contract.

use super::{Cli, Failure, load_core};
use clap::Subcommand;
use std::path::{Path, PathBuf};

/// How many parked Entries `installer parked` names before it counts the
/// rest, and how long a line it prints, so that its output and the
/// uninstaller's warning around it fit NSIS's 1024-character strings.
const NAMED_AT_MOST: usize = 8;
const LINE_AT_MOST: usize = 60;

#[derive(Subcommand)]
pub(crate) enum InstallerCommand {
    /// Print the Entries in config-digger's parked-Entry store, one per
    /// line, for the uninstaller's "Delete config-digger data" warning: the
    /// first eight, each line cut to 60 characters, then how many more
    /// there are.
    Parked,
    /// Add a folder to the user's PATH, unless it is there already.
    AddToPath(PathEdit),
    /// Take every spelling of a folder out of the user's PATH.
    RemoveFromPath(PathEdit),
}

/// The folder a PATH edit adds or removes, and where the user's PATH is.
#[derive(clap::Args)]
pub(crate) struct PathEdit {
    /// The folder, as the installer spells it.
    folder: PathBuf,
    /// The key under `HKEY_CURRENT_USER` holding the user's environment,
    /// which tests point elsewhere.
    #[arg(long, value_name = "KEY", default_value = "Environment")]
    environment_key: String,
}

pub(crate) fn run(cli: &Cli, command: &InstallerCommand) -> Result<(), Failure> {
    match command {
        InstallerCommand::Parked => parked(cli),
        InstallerCommand::AddToPath(edit) => user_path::edit(edit, add),
        InstallerCommand::RemoveFromPath(edit) => user_path::edit(edit, remove),
    }
}

fn parked(cli: &Cli) -> Result<(), Failure> {
    let core = load_core(cli)?;
    let parked = core.stored_parked_entries()?;
    for entry in parked.iter().take(NAMED_AT_MOST) {
        println!(
            "{}",
            shortened(&format!("{} ({})", entry.name, entry.agent_name))
        );
    }
    if parked.len() > NAMED_AT_MOST {
        println!("and {} more", parked.len() - NAMED_AT_MOST);
    }
    Ok(())
}

/// `line`, cut to [`LINE_AT_MOST`] characters with `...` when longer.
fn shortened(line: &str) -> String {
    if line.chars().count() <= LINE_AT_MOST {
        return line.to_owned();
    }
    let kept: String = line.chars().take(LINE_AT_MOST - 3).collect();
    format!("{kept}...")
}

/// `entries` with `folder` at the end, or `None` when it is there already.
fn add(entries: &[&str], folder: &Path) -> Option<Vec<String>> {
    if entries.iter().any(|entry| names_folder(entry, folder)) {
        return None;
    }
    let mut added: Vec<String> = entries.iter().map(|entry| (*entry).to_owned()).collect();
    added.push(folder.display().to_string());
    Some(added)
}

/// `entries` without any spelling of `folder`, or `None` when it is not there.
fn remove(entries: &[&str], folder: &Path) -> Option<Vec<String>> {
    if !entries.iter().any(|entry| names_folder(entry, folder)) {
        return None;
    }
    Some(
        entries
            .iter()
            .filter(|entry| !names_folder(entry, folder))
            .map(|entry| (*entry).to_owned())
            .collect(),
    )
}

/// Whether a PATH entry names `folder`, ignoring case, quotes and a
/// trailing separator.
fn names_folder(entry: &str, folder: &Path) -> bool {
    let spelling = |text: &str| {
        text.trim()
            .trim_matches('"')
            .replace('/', "\\")
            .trim_end_matches('\\')
            .to_lowercase()
    };
    spelling(entry) == spelling(&folder.display().to_string())
}

/// The user's PATH: the `Path` value of `HKCU\Environment`, edited in Rust
/// rather than in NSIS, whose strings stop at 1024 characters. Its text
/// stays unexpanded and its type stays `REG_EXPAND_SZ` or `REG_SZ`, so that
/// entries such as `%USERPROFILE%\bin` keep working. The installer
/// broadcasts the change itself.
#[cfg(windows)]
mod user_path {
    use super::{Failure, PathEdit};
    use config_digger_core::api;
    use std::path::Path;
    use winreg::RegKey;
    use winreg::RegValue;
    use winreg::enums::{HKEY_CURRENT_USER, RegType};

    const VALUE: &str = "Path";

    /// Applies `change` to the user's PATH, writing only when it changed.
    pub(super) fn edit(
        edit: &PathEdit,
        change: fn(&[&str], &Path) -> Option<Vec<String>>,
    ) -> Result<(), Failure> {
        let fail = |error: std::io::Error| {
            Failure::from(api::Error::unexpected(format!(
                "cannot edit the user PATH: {error}"
            )))
        };
        let (key, _) = RegKey::predef(HKEY_CURRENT_USER)
            .create_subkey(&edit.environment_key)
            .map_err(fail)?;
        let (text, vtype) = match key.get_raw_value(VALUE) {
            Ok(value) => (decode(&value.bytes), value.vtype),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                (String::new(), RegType::REG_EXPAND_SZ)
            }
            Err(error) => return Err(fail(error)),
        };
        let entries: Vec<&str> = text.split(';').filter(|entry| !entry.is_empty()).collect();
        let Some(changed) = change(&entries, &edit.folder) else {
            return Ok(());
        };
        if changed.is_empty() {
            return key.delete_value(VALUE).map_err(fail);
        }
        let value = RegValue {
            bytes: encode(&changed.join(";")).into(),
            vtype,
        };
        key.set_raw_value(VALUE, &value).map_err(fail)
    }

    /// A registry string's UTF-16 bytes, up to its terminating nul.
    fn decode(bytes: &[u8]) -> String {
        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|pair| u16::from_le_bytes([pair[0], pair[1]]))
            .take_while(|unit| *unit != 0)
            .collect();
        String::from_utf16_lossy(&units)
    }

    /// `text` as a nul-terminated registry string.
    fn encode(text: &str) -> Vec<u8> {
        text.encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect()
    }
}

/// Only Windows has a user PATH in the registry.
#[cfg(not(windows))]
mod user_path {
    use super::{Failure, PathEdit};
    use config_digger_core::api;
    use std::path::Path;

    pub(super) fn edit(
        _edit: &PathEdit,
        _change: fn(&[&str], &Path) -> Option<Vec<String>>,
    ) -> Result<(), Failure> {
        Err(api::Error::unexpected("the user PATH is edited only on Windows").into())
    }
}
