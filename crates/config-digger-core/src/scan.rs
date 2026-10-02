//! `scan()`: reads every location of every found Agent home into an immutable
//! snapshot of Entries and Read failures. Nothing here decides a State.

use crate::definitions::AgentDefinition;
use crate::definitions::blocks::{Anchored, Locate, Parse};
use crate::ports::Ports;
use std::collections::BTreeMap;
use std::io;
use std::path::PathBuf;
use std::time::SystemTime;

pub(crate) struct Snapshot {
    pub read_at: SystemTime,
    pub homes: Vec<ScannedHome>,
    pub entries: Vec<ScannedEntry>,
    pub read_failures: Vec<ScannedReadFailure>,
}

/// An Agent home found on this machine.
pub(crate) struct ScannedHome {
    /// Index into the definitions the snapshot was scanned with.
    pub agent: usize,
    pub home: String,
}

/// One declaration at one concrete location.
pub(crate) struct ScannedEntry {
    pub agent: usize,
    pub home: String,
    pub location: String,
    pub path: PathBuf,
    pub key_path: Vec<String>,
    pub name: String,
}

pub(crate) struct ScannedReadFailure {
    pub agent: usize,
    pub home: String,
    pub path: PathBuf,
    pub kind: ReadFailureKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ReadFailureKind {
    Parse,
    Decode,
    Permission,
    Io,
}

/// Why a scan could not run at all.
#[derive(Debug, thiserror::Error)]
pub(crate) enum ScanError {
    #[error("cannot determine where {agent} home `{home}` is: the user's home folder is unknown")]
    HomeUnknown { agent: String, home: String },
}

pub(crate) fn scan(definitions: &[AgentDefinition], ports: &Ports) -> Result<Snapshot, ScanError> {
    let mut snapshot = Snapshot {
        read_at: ports.clock.now(),
        homes: Vec::new(),
        entries: Vec::new(),
        read_failures: Vec::new(),
    };

    for (agent, definition) in definitions.iter().enumerate() {
        for home in &definition.homes {
            let roots = home
                .roots
                .iter()
                .map(|(name, locator)| Some((name.clone(), locator.locate(ports.env.as_ref())?)))
                .collect::<Option<BTreeMap<_, _>>>()
                .ok_or_else(|| ScanError::HomeUnknown {
                    agent: definition.agent.name.clone(),
                    home: home.id.clone(),
                })?;
            if !roots.values().any(|root| ports.fs.exists(root)) {
                tracing::debug!(agent = %definition.agent.id, home = %home.id, "home not found");
                continue;
            }
            tracing::debug!(agent = %definition.agent.id, home = %home.id, "home found");
            snapshot.homes.push(ScannedHome {
                agent,
                home: home.id.clone(),
            });

            for location in &definition.locations {
                let Some(path) = location.anchor.path(&roots) else {
                    continue;
                };
                let document = match read(ports, &path, &location.format) {
                    Ok(Some(document)) => document,
                    Ok(None) => continue,
                    Err(kind) => {
                        tracing::warn!(
                            agent = %definition.agent.id,
                            path = %path.display(),
                            ?kind,
                            "read failure"
                        );
                        snapshot.read_failures.push(ScannedReadFailure {
                            agent,
                            home: home.id.clone(),
                            path,
                            kind,
                        });
                        continue;
                    }
                };
                let root = location
                    .root
                    .iter()
                    .try_fold(&document, |value, key| value.get(key));
                let Some(serde_json::Value::Object(entries)) = root else {
                    continue;
                };
                tracing::debug!(
                    agent = %definition.agent.id,
                    location = %location.id,
                    path = %path.display(),
                    entries = entries.len(),
                    "read location"
                );
                for name in entries.keys() {
                    let mut key_path = location.root.clone();
                    key_path.push(name.clone());
                    snapshot.entries.push(ScannedEntry {
                        agent,
                        home: home.id.clone(),
                        location: location.id.clone(),
                        path: path.clone(),
                        key_path,
                        name: name.clone(),
                    });
                }
            }
        }
    }
    Ok(snapshot)
}

/// A location's parsed document, `None` when it is missing.
fn read(
    ports: &Ports,
    path: &std::path::Path,
    format: &impl Parse,
) -> Result<Option<serde_json::Value>, ReadFailureKind> {
    let bytes = match ports.fs.read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            return Err(ReadFailureKind::Permission);
        }
        Err(_) => return Err(ReadFailureKind::Io),
    };
    let text = String::from_utf8(bytes).map_err(|_| ReadFailureKind::Decode)?;
    format
        .parse(&text)
        .map(Some)
        .map_err(|_| ReadFailureKind::Parse)
}
