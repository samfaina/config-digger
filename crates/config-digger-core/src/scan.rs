//! `scan()`: reads every location of every found Agent home and Project into
//! an immutable snapshot of Projects, Entries and Read failures. Nothing here
//! decides a State.

use crate::api::ReadFailureKind;
use crate::definitions::blocks::{
    Anchored, Disables, JsonFormat, Locate, NAME_KEY, Parse, Register, StoreClass,
};
use crate::definitions::{AgentDefinition, ItemKind, LocationDefinition, PROJECT_KEY};
use crate::ports::Ports;
use crate::{mask, paths};
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
    /// Every location read, existing or not, once per Agent home.
    pub readers: Vec<LocationReader>,
    /// The location files and Disable switch files that are in the index of
    /// their git repo. A location file is asked about when it exists, and a
    /// switch file when its folder does, since a file in the index can be
    /// missing from the working tree.
    pub versioned: BTreeSet<PathBuf>,
    /// Every place a Disable mechanism's switch is, set or not.
    pub switches: Vec<ScannedSwitch>,
    /// The switch files that cannot be read or parsed. They Disable nothing,
    /// and are never written.
    pub unreadable_switches: BTreeSet<PathBuf>,
}

/// One place a Disable mechanism's switch is, for one Agent home and, for a
/// switch that applies per Project, one Project.
#[derive(Clone)]
pub(crate) struct ScannedSwitch {
    pub agent_index: usize,
    pub home: String,
    pub kind: ItemKind,
    /// The mechanism's index among its Item kind's mechanisms.
    pub mechanism: usize,
    pub path: PathBuf,
    /// The key of the Project it applies in, or `None` when it applies in every context.
    pub project: Option<String>,
    /// Its key path, `{project}` replaced by the Project key in the Agent's
    /// own spelling, `{name}` left in place.
    pub key_path: Vec<String>,
    /// What the file holds at the key path, up to `{name}` when it has one:
    /// `None` when the file or key is missing.
    pub value: Option<serde_json::Value>,
    /// How the file behaves while the Agent runs.
    pub store_class: StoreClass,
}

impl ScannedSwitch {
    /// The switch's key path for the Entry named `name`.
    pub fn key_path_for(&self, name: &str) -> Vec<String> {
        self.key_path
            .iter()
            .map(|key| if key == NAME_KEY { name } else { key }.to_owned())
            .collect()
    }

    /// What the switch holds for the Entry named `name`.
    pub fn value_for(&self, name: &str) -> Option<&serde_json::Value> {
        let value = self.value.as_ref()?;
        match self.key_path.iter().position(|key| key == NAME_KEY) {
            Some(at) => lookup(value.get(name)?, &self.key_path[at + 1..]),
            None => Some(value),
        }
    }
}

/// The value at `key_path` below `value`.
fn lookup<'v>(value: &'v serde_json::Value, key_path: &[String]) -> Option<&'v serde_json::Value> {
    key_path.iter().try_fold(value, |value, key| value.get(key))
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

/// A location of an Agent that reads a file.
#[derive(Clone, PartialEq)]
pub(crate) struct LocationReader {
    pub agent_index: usize,
    pub home: String,
    pub location: String,
    pub path: PathBuf,
}

impl Snapshot {
    /// The locations that read the file at `path`.
    pub fn readers_of(&self, path: &Path) -> Vec<LocationReader> {
        self.readers
            .iter()
            .filter(|reader| reader.path == path)
            .cloned()
            .collect()
    }

    /// The roots of the Agent home `home` of the Agent at `agent_index`,
    /// which the scan found.
    pub fn roots(&self, agent_index: usize, home: &str) -> &BTreeMap<String, PathBuf> {
        &self
            .homes
            .iter()
            .find(|found| found.agent_index == agent_index && found.home == home)
            .expect("Entries come from found homes")
            .roots
    }

    /// The Entry at `path` and `key_path`, which a resolved Entry comes from.
    pub fn entry(&self, path: &Path, key_path: &[String]) -> &ScannedEntry {
        self.entries
            .iter()
            .find(|scanned| scanned.path == path && scanned.key_path == key_path)
            .expect("a resolved Entry comes from the snapshot")
    }

    /// The places of the switch of the mechanism at `mechanism` among those
    /// of `kind`, in the Agent home of `entry`.
    pub fn switches_of<'s>(
        &'s self,
        entry: &'s ScannedEntry,
        kind: ItemKind,
        mechanism: usize,
    ) -> impl Iterator<Item = &'s ScannedSwitch> + Clone {
        self.switches.iter().filter(move |switch| {
            switch.agent_index == entry.agent_index
                && switch.home == entry.home
                && switch.kind == kind
                && switch.mechanism == mechanism
        })
    }

    /// The Agent homes whose Disable switches are in the file at `path`,
    /// each with the strongest store class of its switches there.
    pub fn switch_stores_of(&self, path: &Path) -> Vec<SwitchStore> {
        let mut stores: Vec<SwitchStore> = Vec::new();
        for switch in self.switches.iter().filter(|switch| switch.path == path) {
            match stores
                .iter_mut()
                .find(|store| store.agent_index == switch.agent_index && store.home == switch.home)
            {
                Some(store) => store.store_class = store.store_class.max(switch.store_class),
                None => stores.push(SwitchStore {
                    agent_index: switch.agent_index,
                    home: switch.home.clone(),
                    store_class: switch.store_class,
                }),
            }
        }
        stores
    }
}

/// An Agent home whose Disable switches are in a file, and how the file
/// behaves while the Agent runs.
#[derive(Clone)]
pub(crate) struct SwitchStore {
    pub agent_index: usize,
    pub home: String,
    pub store_class: StoreClass,
}

/// An Agent home found on this machine.
#[derive(Clone)]
pub(crate) struct ScannedHome {
    /// Index of the Agent definition in the definitions the snapshot was scanned with.
    pub agent_index: usize,
    pub home: String,
    /// The path of each of its roots, by name.
    pub roots: BTreeMap<String, PathBuf>,
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
    /// Its value with every Secret masked. The real value never leaves the scan.
    pub config: serde_json::Value,
    /// The key paths below the Entry whose values were masked.
    pub masked: Vec<Vec<String>>,
}

#[derive(Clone)]
pub(crate) struct ScannedReadFailure {
    pub agent_index: usize,
    pub home: String,
    pub path: PathBuf,
    /// The key of the Project whose folder holds the location, or `None`
    /// when it reaches every context.
    pub project: Option<String>,
    pub kind: ReadFailureKind,
    /// The 1-based line and column of a parse error, when the parser knows them.
    pub position: Option<(usize, usize)>,
}

impl ScannedReadFailure {
    /// Whether it reaches the context whose Project key is `project_key`:
    /// one outside any Project reaches every context.
    pub fn reaches(&self, project_key: Option<&str>) -> bool {
        self.project.is_none() || self.project.as_deref() == project_key
    }
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
        readers: Vec::new(),
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
    let (switches, unreadable_switches) =
        read_switches(definitions, &found, &projects, &mut scanner.reader);
    let switch_files = switch_files(definitions, &found, &projects);
    let locations = scanner
        .readers
        .iter()
        .map(|reader| reader.path.as_path())
        .filter(|path| ports.fs.exists(path));
    let switch_files = switch_files
        .iter()
        .map(PathBuf::as_path)
        .filter(|path| path.parent().is_some_and(|folder| ports.fs.exists(folder)));
    let versioned = versioned(ports, locations.chain(switch_files));
    // A file that is a Read failure for an Agent home yields it no Entries,
    // even from a location of it that did parse.
    let failures = scanner.reader.failures;
    scanner.entries.retain(|entry| {
        !failures.iter().any(|failure| {
            failure.agent_index == entry.agent_index
                && failure.home == entry.home
                && failure.path == entry.path
        })
    });
    Ok(Snapshot {
        read_at,
        homes: found
            .into_iter()
            .map(|home| ScannedHome {
                agent_index: home.agent_index,
                home: home.home,
                roots: home.roots,
            })
            .collect(),
        projects,
        entries: scanner.entries,
        readers: scanner.readers,
        read_failures: failures,
        versioned,
        switches,
        unreadable_switches,
    })
}

/// Every place each Disable mechanism's switch is, for each found Agent
/// home: in each existing Project folder for a switch in a Project, under
/// each spelling of each Project key for a switch keyed by `{project}`, and
/// once otherwise. A switch file that is missing is read as empty; one that
/// cannot be read or parsed as JSON, or that is a Read failure for the
/// Agent home, is listed apart and has no places.
fn read_switches(
    definitions: &[AgentDefinition],
    found: &[FoundHome],
    projects: &[ScannedProject],
    reader: &mut Reader,
) -> (Vec<ScannedSwitch>, BTreeSet<PathBuf>) {
    let mut switches = Vec::new();
    let mut unreadable = BTreeSet::new();
    for home in found {
        let definition = &definitions[home.agent_index];
        for (&kind, rules) in &definition.items {
            for (mechanism, block) in rules.disable.iter().enumerate() {
                let switch = block.switch();
                let places: Vec<(PathBuf, Option<String>)> = if switch.anchor.in_project() {
                    projects
                        .iter()
                        .filter(|project| project.exists)
                        .filter_map(|project| {
                            let path = switch.anchor.path(&home.roots, Some(&project.path))?;
                            Some((path, Some(project.key.clone())))
                        })
                        .collect()
                } else {
                    switch
                        .anchor
                        .path(&home.roots, None)
                        .map(|path| (path, None))
                        .into_iter()
                        .collect()
                };
                for (path, project) in places {
                    // A file the Agent home cannot read, even one that is
                    // valid JSON, sets none of its switches.
                    let failed = reader.failures.iter().any(|failure| {
                        failure.agent_index == home.agent_index
                            && failure.home == home.home
                            && failure.path == path
                    });
                    if failed {
                        unreadable.insert(path);
                        continue;
                    }
                    let document = match reader.peek(&path) {
                        Ok(None) => None,
                        Ok(Some(text)) => match JsonFormat::STRICT.parse(text) {
                            Ok(document) => Some(document),
                            Err(_) => {
                                unreadable.insert(path);
                                continue;
                            }
                        },
                        Err(_) => {
                            unreadable.insert(path);
                            continue;
                        }
                    };
                    let place = |project: Option<String>, key_path: Vec<String>| {
                        let up_to_name = key_path
                            .iter()
                            .position(|key| key == NAME_KEY)
                            .unwrap_or(key_path.len());
                        ScannedSwitch {
                            agent_index: home.agent_index,
                            home: home.home.clone(),
                            kind,
                            mechanism,
                            path: path.clone(),
                            project,
                            value: document
                                .as_ref()
                                .and_then(|document| lookup(document, &key_path[..up_to_name]))
                                .cloned(),
                            key_path,
                            store_class: switch.store_class,
                        }
                    };
                    let Some(at) = switch.key_path.iter().position(|key| key == PROJECT_KEY) else {
                        switches.push(place(project, switch.key_path.clone()));
                        continue;
                    };
                    // Each key there is a Project in the Agent's own spelling.
                    let keyed = document
                        .as_ref()
                        .and_then(|document| lookup(document, &switch.key_path[..at]))
                        .and_then(serde_json::Value::as_object);
                    for spelling in keyed.into_iter().flat_map(serde_json::Map::keys) {
                        let Some(project) = project_of(projects, spelling) else {
                            continue;
                        };
                        let mut key_path = switch.key_path.clone();
                        key_path[at] = spelling.clone();
                        switches.push(place(Some(project), key_path));
                    }
                }
            }
        }
    }
    (switches, unreadable)
}

/// The key of the scanned Project that `spelling` names, if any.
fn project_of(projects: &[ScannedProject], spelling: &str) -> Option<String> {
    let spelling = paths::key(&paths::lexical(Path::new(spelling)));
    projects
        .iter()
        .find(|project| project.spellings.contains(&spelling))
        .map(|project| project.key.clone())
}

/// The file of every Disable mechanism's switch, for each found Agent
/// home, and for a switch in a Project, in each existing Project folder.
fn switch_files(
    definitions: &[AgentDefinition],
    found: &[FoundHome],
    projects: &[ScannedProject],
) -> BTreeSet<PathBuf> {
    let mut files = BTreeSet::new();
    for home in found {
        let definition = &definitions[home.agent_index];
        for rules in definition.items.values() {
            for mechanism in &rules.disable {
                let anchor = &mechanism.switch().anchor;
                if !anchor.in_project() {
                    files.extend(anchor.path(&home.roots, None));
                    continue;
                }
                for project in projects.iter().filter(|project| project.exists) {
                    files.extend(anchor.path(&home.roots, Some(&project.path)));
                }
            }
        }
    }
    files
}

/// The files among `paths` that are in the index of their git repo, asking
/// once per file. Ignored and untracked files are not.
fn versioned<'a>(ports: &Ports, paths: impl Iterator<Item = &'a Path>) -> BTreeSet<PathBuf> {
    let paths: BTreeSet<&Path> = paths.collect();
    paths
        .into_iter()
        .filter(|path| ports.git.is_versioned(path))
        .map(Path::to_path_buf)
        .collect()
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
    readers: Vec<LocationReader>,
}

impl Scanner<'_> {
    /// Adds the Projects an Agent home's registries record.
    fn read_registries(&mut self, home: &FoundHome, definition: &AgentDefinition) {
        for registry in &definition.projects.registries {
            let Some(path) = registry.anchor().path(&home.roots, None) else {
                continue;
            };
            let Some(text) = self.reader.text(home, &path, None) else {
                continue;
            };
            match registry.spellings(text) {
                Ok(spellings) => {
                    for spelling in spellings {
                        self.projects
                            .add(Path::new(&spelling), Some(home.agent_index));
                    }
                }
                Err(error) => {
                    self.reader
                        .fail(home, &path, None, ReadFailureKind::Parse, error.position)
                }
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
        let reader = LocationReader {
            agent_index: home.agent_index,
            home: home.home.clone(),
            location: location.id.clone(),
            path: path.clone(),
        };
        if !self.readers.contains(&reader) {
            self.readers.push(reader);
        }
        let Some(text) = self.reader.text(home, &path, project.as_deref()) else {
            return;
        };
        let document = match location.format.parse(text) {
            Ok(document) => document,
            Err(error) => {
                let project = project.as_deref();
                self.reader
                    .fail(home, &path, project, ReadFailureKind::Parse, error.position);
                return;
            }
        };
        let mut objects = Vec::new();
        if find_objects(&document, &location.root, Vec::new(), None, &mut objects).is_err() {
            let project = project.as_deref();
            self.reader
                .fail(home, &path, project, ReadFailureKind::Parse, None);
            return;
        }
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
            for (name, value) in object.entries {
                let mut key_path = object.key_path.clone();
                key_path.push(name.clone());
                let masked = mask::entry(location, value);
                self.entries.push(ScannedEntry {
                    agent_index: home.agent_index,
                    home: home.home.clone(),
                    location: location.id.clone(),
                    path: path.clone(),
                    key_path,
                    name: name.clone(),
                    project: project.clone(),
                    config: masked.value,
                    masked: masked.key_paths,
                });
            }
        }
    }
}

/// An object whose keys are Entries.
pub(crate) struct RootObject<'v> {
    pub key_path: Vec<String>,
    /// The Project key it sits under, in the Agent's spelling.
    pub project: Option<String>,
    pub entries: &'v serde_json::Map<String, serde_json::Value>,
}

/// Where Entries are, a file holds something other than an object, so
/// config-digger cannot enumerate them.
#[derive(Debug)]
pub(crate) struct NotEnumerable;

/// The objects at `root` below `value`. Below `{project}`, every key is a
/// Project. A missing key holds no objects; a key on the way that holds
/// something other than an object fails.
pub(crate) fn find_objects<'v>(
    value: &'v serde_json::Value,
    root: &[String],
    key_path: Vec<String>,
    project: Option<String>,
    out: &mut Vec<RootObject<'v>>,
) -> Result<(), NotEnumerable> {
    let serde_json::Value::Object(object) = value else {
        return Err(NotEnumerable);
    };
    let Some((key, rest)) = root.split_first() else {
        out.push(RootObject {
            key_path,
            project,
            entries: object,
        });
        return Ok(());
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
        find_objects(child, rest, child_key_path, child_project, out)?;
    }
    Ok(())
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

    /// A file's text, `None` when it is missing or a Read failure, which is
    /// then reported. `project` is the key of the Project whose folder
    /// holds the file, if one does.
    fn text(&mut self, home: &FoundHome, path: &Path, project: Option<&str>) -> Option<&str> {
        if !self.files.contains_key(path) {
            self.files.insert(path.to_path_buf(), self.read(path));
        }
        if let Some(kind) = self.files[path].as_ref().err().copied() {
            self.fail(home, path, project, kind, None);
            return None;
        }
        self.files[path].as_ref().ok()?.as_deref()
    }

    /// A file's text, `None` when it is missing, as [`text`](Self::text)
    /// reads it but without reporting a failure: for a file that holds no
    /// Entries, such as a switch's.
    fn peek(&mut self, path: &Path) -> Result<Option<&str>, ReadFailureKind> {
        if !self.files.contains_key(path) {
            self.files.insert(path.to_path_buf(), self.read(path));
        }
        match &self.files[path] {
            Ok(text) => Ok(text.as_deref()),
            Err(kind) => Err(*kind),
        }
    }

    fn read(&self, path: &Path) -> Result<Option<String>, ReadFailureKind> {
        match self.written.iter().find(|(written, _)| written == path) {
            Some((_, bytes)) => decode(bytes.clone()).map(Some),
            None => read(self.ports, path),
        }
    }

    fn fail(
        &mut self,
        home: &FoundHome,
        path: &Path,
        project: Option<&str>,
        kind: ReadFailureKind,
        position: Option<(usize, usize)>,
    ) {
        let id = (home.agent_index, home.home.clone(), path.to_path_buf());
        if !self.reported.insert(id) {
            return;
        }
        tracing::warn!(path = %path.display(), ?kind, "read failure");
        self.failures.push(ScannedReadFailure {
            agent_index: home.agent_index,
            home: home.home.clone(),
            path: path.to_path_buf(),
            project: project.map(str::to_owned),
            kind,
            position,
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
