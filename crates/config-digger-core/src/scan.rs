//! `scan()`: reads every location of every found Agent home and Project into
//! an immutable snapshot of Projects, Entries and Read failures. Nothing here
//! decides a State.

use crate::api::ReadFailureKind;
use crate::definitions::blocks::{Anchored, Locate, Parse, Register};
use crate::definitions::{AgentDefinition, LocationDefinition, PROJECT_KEY};
use crate::paths;
use crate::ports::Ports;
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Clone)]
pub(crate) struct Snapshot {
    pub read_at: SystemTime,
    pub homes: Vec<ScannedHome>,
    /// Sorted by path.
    pub projects: Vec<ScannedProject>,
    pub entries: Vec<ScannedEntry>,
    pub read_failures: Vec<ScannedReadFailure>,
}

impl Snapshot {
    /// The snapshot as it would be read once the Entries at `removed`, each a
    /// path and a key path, are gone.
    pub fn without(&self, removed: &[(PathBuf, Vec<String>)]) -> Snapshot {
        let mut snapshot = self.clone();
        snapshot.entries.retain(|entry| {
            !removed
                .iter()
                .any(|(path, key_path)| entry.path == *path && entry.key_path == *key_path)
        });
        snapshot
    }
}

/// An Agent home found on this machine.
#[derive(Clone)]
pub(crate) struct ScannedHome {
    /// Index of the Agent definition in the definitions the snapshot was scanned with.
    pub agent_index: usize,
    pub home: String,
}

/// A Project, found in a registry or asked for.
#[derive(Clone)]
pub(crate) struct ScannedProject {
    /// The real path of an existing folder, the lexical normalisation otherwise.
    pub path: PathBuf,
    /// What Projects are compared by.
    pub key: String,
    pub exists: bool,
    /// The keys of every lexically normalised spelling that names this Project.
    pub spellings: BTreeSet<String>,
    /// The Agents whose registries record it.
    pub registered_by: BTreeSet<usize>,
}

/// One declaration at one concrete location.
#[derive(Clone)]
pub(crate) struct ScannedEntry {
    pub agent_index: usize,
    pub home: String,
    pub location: String,
    pub path: PathBuf,
    /// In the Agent's own spelling, Project keys included.
    pub key_path: Vec<String>,
    pub name: String,
    /// The key of the Project the Entry applies in, or `None` when it applies in every context.
    pub project: Option<String>,
}

#[derive(Clone)]
pub(crate) struct ScannedReadFailure {
    pub agent_index: usize,
    pub home: String,
    pub path: PathBuf,
    pub kind: ReadFailureKind,
}

/// Why a scan could not run at all.
#[derive(Debug, thiserror::Error)]
pub(crate) enum ScanError {
    #[error("cannot determine where {agent} home `{home}` is: the user's home folder is unknown")]
    HomeUnknown { agent: String, home: String },
}

/// A found Agent home with the paths of its roots.
struct FoundHome {
    agent_index: usize,
    home: String,
    roots: BTreeMap<String, PathBuf>,
}

/// `extra_projects` are evaluated whether or not a registry records them.
pub(crate) fn scan(
    definitions: &[AgentDefinition],
    ports: &Ports,
    extra_projects: &[PathBuf],
) -> Result<Snapshot, ScanError> {
    scan_as_if(definitions, ports, extra_projects, &[])
}

/// Scans as [`scan`] does, as if each file in `written` held the bytes paired
/// with it, so that a plan can predict what a scan will read once it is done.
pub(crate) fn scan_as_if(
    definitions: &[AgentDefinition],
    ports: &Ports,
    extra_projects: &[PathBuf],
    written: &[(PathBuf, Vec<u8>)],
) -> Result<Snapshot, ScanError> {
    let read_at = ports.clock.now();
    let found = find_homes(definitions, ports)?;
    let mut scanner = Scanner {
        reader: Reader::new(ports, written),
        projects: Projects::new(ports),
        entries: Vec::new(),
    };

    for home in &found {
        scanner.read_registries(home, &definitions[home.agent_index]);
    }
    for path in extra_projects {
        scanner.projects.add(path, None);
    }
    // Locations keyed by Project can name Projects no registry records, so
    // every one of them is read before any location in a Project folder.
    for home in &found {
        let definition = &definitions[home.agent_index];
        for location in definition.locations.iter() {
            if location.anchor.in_project() {
                continue;
            }
            if let Some(path) = location.anchor.path(&home.roots, None) {
                scanner.read_location(home, definition, location, path, None);
            }
        }
    }
    let folders: Vec<(PathBuf, String)> = scanner
        .projects
        .list
        .iter()
        .filter(|project| project.exists)
        .map(|project| (project.path.clone(), project.key.clone()))
        .collect();
    for home in &found {
        let definition = &definitions[home.agent_index];
        for location in definition.locations.iter() {
            if !location.anchor.in_project() {
                continue;
            }
            for (folder, key) in &folders {
                if let Some(path) = location.anchor.path(&home.roots, Some(folder)) {
                    scanner.read_location(home, definition, location, path, Some(key.clone()));
                }
            }
        }
    }

    let mut projects = scanner.projects.list;
    projects.sort_by(|a, b| a.key.cmp(&b.key));
    Ok(Snapshot {
        read_at,
        homes: found
            .into_iter()
            .map(|home| ScannedHome {
                agent_index: home.agent_index,
                home: home.home,
            })
            .collect(),
        projects,
        entries: scanner.entries,
        read_failures: scanner.reader.failures,
    })
}

fn find_homes(definitions: &[AgentDefinition], ports: &Ports) -> Result<Vec<FoundHome>, ScanError> {
    let mut found = Vec::new();
    for (agent_index, definition) in definitions.iter().enumerate() {
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
            found.push(FoundHome {
                agent_index,
                home: home.id.clone(),
                roots,
            });
        }
    }
    Ok(found)
}

/// What a scan has gathered so far.
struct Scanner<'p> {
    reader: Reader<'p>,
    projects: Projects<'p>,
    entries: Vec<ScannedEntry>,
}

impl Scanner<'_> {
    /// Adds the Projects an Agent home's registries record.
    fn read_registries(&mut self, home: &FoundHome, definition: &AgentDefinition) {
        for registry in &definition.projects.registries {
            let Some(path) = registry.anchor().path(&home.roots, None) else {
                continue;
            };
            let Some(text) = self.reader.text(home, &path) else {
                continue;
            };
            match registry.spellings(text) {
                Ok(spellings) => {
                    for spelling in spellings {
                        self.projects
                            .add(Path::new(&spelling), Some(home.agent_index));
                    }
                }
                Err(_) => self.reader.fail(home, &path, ReadFailureKind::Parse),
            }
        }
    }

    /// Adds the Entries of one location. `project` is the key of the Project
    /// whose folder holds the location, if it lies in one.
    fn read_location(
        &mut self,
        home: &FoundHome,
        definition: &AgentDefinition,
        location: &LocationDefinition,
        path: PathBuf,
        project: Option<String>,
    ) {
        let Some(text) = self.reader.text(home, &path) else {
            return;
        };
        let document = match location.format.parse(text) {
            Ok(document) => document,
            Err(_) => {
                self.reader.fail(home, &path, ReadFailureKind::Parse);
                return;
            }
        };
        let mut objects = Vec::new();
        find_objects(&document, &location.root, Vec::new(), None, &mut objects);
        for object in objects {
            tracing::debug!(
                agent = %definition.agent.id,
                location = %location.id,
                path = %path.display(),
                entries = object.entries.len(),
                "read location"
            );
            let project = match object.project {
                Some(spelling) => match self.projects.add(Path::new(&spelling), None) {
                    Some(key) => Some(key),
                    // Keyed to something that is not a Project path: no context reads it.
                    None => continue,
                },
                None => project.clone(),
            };
            for name in object.entries.keys() {
                let mut key_path = object.key_path.clone();
                key_path.push(name.clone());
                self.entries.push(ScannedEntry {
                    agent_index: home.agent_index,
                    home: home.home.clone(),
                    location: location.id.clone(),
                    path: path.clone(),
                    key_path,
                    name: name.clone(),
                    project: project.clone(),
                });
            }
        }
    }
}

/// An object whose keys are Entries.
struct RootObject<'v> {
    key_path: Vec<String>,
    /// The Project key it sits under, in the Agent's spelling.
    project: Option<String>,
    entries: &'v serde_json::Map<String, serde_json::Value>,
}

/// The objects at `root` below `value`. Below `{project}`, every key is a Project.
fn find_objects<'v>(
    value: &'v serde_json::Value,
    root: &[String],
    key_path: Vec<String>,
    project: Option<String>,
    out: &mut Vec<RootObject<'v>>,
) {
    let serde_json::Value::Object(object) = value else {
        return;
    };
    let Some((key, rest)) = root.split_first() else {
        out.push(RootObject {
            key_path,
            project,
            entries: object,
        });
        return;
    };
    let children: Vec<(&String, &serde_json::Value)> = if key == PROJECT_KEY {
        object.iter().collect()
    } else {
        object.get_key_value(key).into_iter().collect()
    };
    for (child_key, child) in children {
        let mut child_key_path = key_path.clone();
        child_key_path.push(child_key.clone());
        let child_project = if key == PROJECT_KEY {
            Some(child_key.clone())
        } else {
            project.clone()
        };
        find_objects(child, rest, child_key_path, child_project, out);
    }
}

/// Reads each file once per scan, and reports each Read failure once per Agent home.
struct Reader<'p> {
    ports: &'p Ports,
    /// Files read as holding these bytes instead of what is on disk.
    written: &'p [(PathBuf, Vec<u8>)],
    files: BTreeMap<PathBuf, Result<Option<String>, ReadFailureKind>>,
    reported: BTreeSet<(usize, String, PathBuf)>,
    failures: Vec<ScannedReadFailure>,
}

impl<'p> Reader<'p> {
    fn new(ports: &'p Ports, written: &'p [(PathBuf, Vec<u8>)]) -> Self {
        Self {
            ports,
            written,
            files: BTreeMap::new(),
            reported: BTreeSet::new(),
            failures: Vec::new(),
        }
    }

    /// A file's text, `None` when it is missing or a Read failure, which is then reported.
    fn text(&mut self, home: &FoundHome, path: &Path) -> Option<&str> {
        if !self.files.contains_key(path) {
            self.files.insert(path.to_path_buf(), self.read(path));
        }
        if let Some(kind) = self.files[path].as_ref().err().copied() {
            self.fail(home, path, kind);
            return None;
        }
        self.files[path].as_ref().ok()?.as_deref()
    }

    fn read(&self, path: &Path) -> Result<Option<String>, ReadFailureKind> {
        match self.written.iter().find(|(written, _)| written == path) {
            Some((_, bytes)) => decode(bytes.clone()).map(Some),
            None => read(self.ports, path),
        }
    }

    fn fail(&mut self, home: &FoundHome, path: &Path, kind: ReadFailureKind) {
        let id = (home.agent_index, home.home.clone(), path.to_path_buf());
        if !self.reported.insert(id) {
            return;
        }
        tracing::warn!(path = %path.display(), ?kind, "read failure");
        self.failures.push(ScannedReadFailure {
            agent_index: home.agent_index,
            home: home.home.clone(),
            path: path.to_path_buf(),
            kind,
        });
    }
}

/// A file's text, `None` when it is missing.
fn read(ports: &Ports, path: &Path) -> Result<Option<String>, ReadFailureKind> {
    let bytes = match ports.fs.read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            return Err(ReadFailureKind::Permission);
        }
        Err(_) => return Err(ReadFailureKind::Io),
    };
    decode(bytes).map(Some)
}

fn decode(bytes: Vec<u8>) -> Result<String, ReadFailureKind> {
    String::from_utf8(bytes).map_err(|_| ReadFailureKind::Decode)
}

/// Every Project, found once however it is spelled.
struct Projects<'p> {
    ports: &'p Ports,
    list: Vec<ScannedProject>,
    /// The key of a lexically normalised spelling → index in `list`.
    by_spelling: BTreeMap<String, usize>,
}

impl<'p> Projects<'p> {
    fn new(ports: &'p Ports) -> Self {
        Self {
            ports,
            list: Vec::new(),
            by_spelling: BTreeMap::new(),
        }
    }

    /// Records a spelling of a Project and returns the Project's key, or
    /// `None` when the spelling is not an absolute path and names no Project.
    fn add(&mut self, spelling: &Path, registered_by: Option<usize>) -> Option<String> {
        let lexical = paths::lexical(spelling);
        if !paths::is_absolute(&lexical) {
            tracing::debug!(spelling = %spelling.display(), "not a Project path");
            return None;
        }
        let spelling = paths::key(&lexical);
        let index = match self.by_spelling.get(&spelling) {
            Some(&index) => index,
            None => {
                let (path, exists) = paths::canonical(self.ports.fs.as_ref(), &lexical);
                let key = paths::key(&path);
                let index = match self.list.iter().position(|project| project.key == key) {
                    Some(index) => index,
                    None => {
                        self.list.push(ScannedProject {
                            path,
                            key: key.clone(),
                            exists,
                            spellings: BTreeSet::from([key.clone()]),
                            registered_by: BTreeSet::new(),
                        });
                        self.by_spelling.insert(key, self.list.len() - 1);
                        self.list.len() - 1
                    }
                };
                self.by_spelling.insert(spelling.clone(), index);
                index
            }
        };
        let project = &mut self.list[index];
        project.spellings.insert(spelling);
        project.registered_by.extend(registered_by);
        Some(project.key.clone())
    }
}
