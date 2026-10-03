//! Installations: the launchable copies of each Agent, found from their
//! definitions at every scan, each with the version read from its files.
//! A version is never read by running the Agent, and never cached.

pub(crate) mod pe;

use crate::api;
use crate::definitions::AgentDefinition;
use crate::definitions::blocks::{self, InstallationPaths, ReadVersion, VersionRead};
use crate::ports::Ports;
use crate::scan::{ScannedHome, Snapshot};
use crate::version::{Standing, Version};
use crate::{explain, paths};
use std::collections::BTreeSet;
use std::path::PathBuf;

/// An Installation found on this machine.
#[derive(Clone)]
pub(crate) struct ScannedInstallation {
    pub agent_index: usize,
    /// The id of its definition's installation.
    pub id: String,
    pub executable: PathBuf,
    /// `None` when no source of its version could be read.
    pub version: Option<VersionRead>,
}

/// The Installations among `installations` that read `home`, and what to
/// note about their versions there.
pub(crate) fn of_home(
    definition: &AgentDefinition,
    installations: &[ScannedInstallation],
    home: &ScannedHome,
) -> (Vec<api::Installation>, Vec<api::AgentNote>) {
    let agent_name = &definition.agent.name;
    let range = definition.verified.as_ref();
    let mut notes = Vec::new();
    let note = |code, installation: Option<&ScannedInstallation>, sentence| api::AgentNote {
        code,
        installation: installation.map(|installation| installation.executable.clone()),
        sentence,
    };
    if range.is_none() {
        notes.push(note(
            api::AgentNoteCode::DefinitionUntested,
            None,
            explain::definition_untested(agent_name),
        ));
    }
    let reading: Vec<&ScannedInstallation> = installations
        .iter()
        .filter(|installation| reads(definition, installation, home))
        .collect();
    if reading.is_empty() {
        notes.push(note(
            api::AgentNoteCode::NoInstallationFound,
            None,
            explain::no_installation_found(agent_name),
        ));
    }
    let mut listed = Vec::new();
    for installation in reading {
        let standing = installation
            .version
            .as_ref()
            .zip(range)
            .map(|(read, verified)| {
                let version = Version::parse(&read.text).expect("a version read parses");
                (read, verified, verified.standing(&version))
            });
        let out_of_range = match standing {
            Some((read, verified, standing)) if standing != Standing::Tested => {
                notes.push(note(
                    api::AgentNoteCode::VersionOutsideTestedRange,
                    Some(installation),
                    explain::version_outside_tested_range(
                        agent_name,
                        &read.text,
                        standing == Standing::Newer,
                        &verified.min,
                        &verified.max,
                    ),
                ));
                true
            }
            _ => false,
        };
        if installation.version.is_none() {
            notes.push(note(
                api::AgentNoteCode::VersionUnreadable,
                Some(installation),
                explain::version_unreadable(agent_name, &installation.executable),
            ));
        }
        listed.push(api::Installation {
            id: installation.id.clone(),
            path: installation.executable.clone(),
            version: installation.version.as_ref().map(|read| read.text.clone()),
            version_from: installation.version.as_ref().map(|read| read.from.clone()),
            out_of_range,
        });
    }
    (listed, notes)
}

/// The Agent homes of `snapshot`, by Agent index and id, that an
/// Installation it found reads: an Agent can run only for them.
pub(crate) fn installed_homes(
    definitions: &[AgentDefinition],
    snapshot: &Snapshot,
) -> BTreeSet<(usize, String)> {
    snapshot
        .homes
        .iter()
        .filter(|home| {
            snapshot
                .installations
                .iter()
                .any(|installation| reads(&definitions[home.agent_index], installation, home))
        })
        .map(|home| (home.agent_index, home.home.clone()))
        .collect()
}

/// Whether `installation` reads `home`: every home when its definition
/// names none, and a home the user added when it reads the first home.
fn reads(
    definition: &AgentDefinition,
    installation: &ScannedInstallation,
    home: &ScannedHome,
) -> bool {
    if installation.agent_index != home.agent_index {
        return false;
    }
    let declared = definition
        .installations
        .iter()
        .find(|declared| declared.id == installation.id)
        .expect("a found Installation comes from its definition");
    let Some(homes) = &declared.homes else {
        return true;
    };
    let laid_out_as = if definition.homes.iter().any(|known| known.id == home.home) {
        &home.home
    } else {
        &definition.homes[0].id
    };
    homes.contains(laid_out_as)
}

/// Every Installation of every Agent, in definition order, each found once
/// per folder its `folder` glob matches where its executable exists.
pub(crate) fn find(definitions: &[AgentDefinition], ports: &Ports) -> Vec<ScannedInstallation> {
    let mut found = Vec::new();
    for (agent_index, definition) in definitions.iter().enumerate() {
        for installation in &definition.installations {
            let folders =
                paths::expand_glob(&installation.folder, ports.fs.as_ref(), ports.env.as_ref());
            for folder in folders {
                let executable = blocks::join_relative(&folder, &installation.executable);
                if !ports.fs.exists(&executable) {
                    continue;
                }
                let at = InstallationPaths {
                    folder: &folder,
                    executable: &executable,
                };
                let version = installation
                    .versions
                    .iter()
                    .find_map(|source| source.read(&at, ports));
                tracing::debug!(
                    agent = %definition.agent.id,
                    installation = %installation.id,
                    executable = %executable.display(),
                    version = version.as_ref().map(|version| version.text.as_str()),
                    "installation found"
                );
                found.push(ScannedInstallation {
                    agent_index,
                    id: installation.id.clone(),
                    executable,
                    version,
                });
            }
        }
    }
    found
}
