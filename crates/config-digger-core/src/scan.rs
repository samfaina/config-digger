//! `scan()`: reads every location of every found Agent home and Project into
//! an immutable snapshot of Projects, Entries and Read failures. Nothing here
//! decides a State.

use crate::api::ReadFailureKind;
use crate::definitions::blocks::{
    Anchored, Bypasses, Cached, Disables, Format, GateStore, JsonFormat, Locate, LookUp, NAME_KEY,
    NameFrom, Parse, ProjectWalk, Register, SkillDefect, SkillFolder, StoreClass,
};
use crate::definitions::skill_managers::{self, Detector, SkillManagerDefinition, Strength};
use crate::definitions::{AgentDefinition, ItemKind, LocationDefinition, PROJECT_KEY};
use crate::folders::{AsIf, PlannedFolder};
use crate::installations::{self, ScannedInstallation};
use crate::ports::{FileSystem, Ports};
use crate::user_list::{self, UserList};
use crate::{mask, paths, skill};
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

#[derive(Clone)]
pub(crate) struct Snapshot {
    pub read_at: SystemTime,
    pub homes: Vec<ScannedHome>,
    /// Every Installation found, with its version as read at this scan.
    pub installations: Vec<ScannedInstallation>,
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
    /// Every Feature gate of every found Agent home, as far as it can be read.
    pub gates: Vec<ScannedGate>,
}

/// A Feature gate of one Agent home, as config-digger reads it.
#[derive(Clone)]
pub(crate) struct ScannedGate {
    pub agent_index: usize,
    pub home: String,
    /// The gate's name.
    pub name: String,
    /// The file the Agent caches it in.
    pub path: PathBuf,
    pub reading: GateReading,
}

/// What config-digger reads of a Feature gate.
#[derive(Clone, PartialEq)]
pub(crate) enum GateReading {
    /// The value the Agent last cached.
    Cached(Cached),
    /// No value can be read: the cache is missing, cannot be read, or holds
    /// none for the gate.
    Unreadable,
    /// The Agent ignores its cache because this variable is set.
    Bypassed(String),
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

/// An Agent home found on this machine, or added by the user.
#[derive(Clone)]
pub(crate) struct ScannedHome {
    /// Index of the Agent definition in the definitions the snapshot was scanned with.
    pub agent_index: usize,
    pub home: String,
    /// The folder its roots are in.
    pub folder: PathBuf,
    /// The path of each of its roots, by name.
    pub roots: BTreeMap<String, PathBuf>,
    pub source: HomeSource,
    /// Any of its roots exists.
    pub configured: bool,
    /// The user hid it.
    pub hidden: bool,
    /// The version that last ran against it, as the Agent records it.
    pub last_ran: Option<String>,
}

/// Where an Agent home came from.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum HomeSource {
    /// The location the Agent definition gives.
    Default,
    /// The user added it, and no definition location finds it.
    Added,
}

/// A Project, found in a registry, added by the user, or asked for.
#[derive(Clone)]
pub(crate) struct ScannedProject {
    /// The real path of an existing folder or workspace file, the lexical
    /// normalisation otherwise.
    pub path: PathBuf,
    /// What Projects are compared by.
    pub key: String,
    pub exists: bool,
    /// It is a `.code-workspace` file rather than a folder.
    pub workspace_file: bool,
    /// The keys of every lexically normalised spelling that names this Project.
    pub spellings: BTreeSet<String>,
    /// The Agents whose registries record it.
    pub registered_by: BTreeSet<usize>,
    /// The user added it.
    pub added: bool,
    /// The user hid it.
    pub hidden: bool,
}

impl ScannedProject {
    /// It is a folder that exists, so that locations in it can be read.
    pub fn is_folder(&self) -> bool {
        self.exists && !self.workspace_file
    }
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
    /// How many folders up from the Project folder its location was read,
    /// 0 outside a Project.
    pub depth: usize,
    /// For a skill, why the Agent may not read its `SKILL.md` as written.
    pub defect: Option<SkillDefect>,
    /// For a Linked Entry, where its link points.
    pub link: Option<ScannedLink>,
    /// For a skill, the skill manager that installed it, as far as one's
    /// definition recognises it.
    pub manager: Option<ManagerMatch>,
}

/// The skill manager whose detectors match a skill folder most strongly.
#[derive(Clone, Copy)]
pub(crate) struct ManagerMatch {
    /// Index of its definition among the built-in skill manager definitions.
    pub manager: usize,
    pub strength: Strength,
}

/// Where the link or junction at a Linked Entry's location points.
#[derive(Clone)]
pub(crate) struct ScannedLink {
    /// The path the link holds, made absolute: what Remove checks it still
    /// points to, and Undo points it to again.
    pub target: PathBuf,
    /// The real path the link leads to, through any further links; `None`
    /// when the target is gone, so the Agent reads nothing there.
    pub real: Option<PathBuf>,
}

impl ScannedLink {
    pub fn broken(&self) -> bool {
        self.real.is_none()
    }
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
    #[error("{0}")]
    UserList(String),
}

/// A found Agent home with the paths of its roots.
struct FoundHome {
    agent_index: usize,
    home: String,
    folder: PathBuf,
    /// Its roots by name, and the machine-wide paths of its definition by
    /// the text that names them.
    roots: BTreeMap<String, PathBuf>,
    source: HomeSource,
    /// Any of its own roots exists.
    configured: bool,
}

/// `extra_projects` are evaluated whether or not a registry records them.
pub(crate) fn scan(
    definitions: &[AgentDefinition],
    ports: &Ports,
    extra_projects: &[PathBuf],
) -> Result<Snapshot, ScanError> {
    scan_as_if(definitions, ports, extra_projects, &[], &[])
}

/// Scans as [`scan`] does, as if each file in `written` held the bytes paired
/// with it and `folders` were done, so that a plan can predict what a scan
/// will read once it is done.
pub(crate) fn scan_as_if(
    definitions: &[AgentDefinition],
    ports: &Ports,
    extra_projects: &[PathBuf],
    written: &[(PathBuf, Vec<u8>)],
    folders: &[PlannedFolder],
) -> Result<Snapshot, ScanError> {
    let as_if = AsIf::new(ports.fs.as_ref(), folders);
    let read_at = ports.clock.now();
    // Without the list, hidden homes and Projects would show again and added
    // ones vanish without a word, so a list that cannot be read stops the scan.
    let user_list = user_list::load(ports).map_err(|error| ScanError::UserList(error.message))?;
    let found = find_homes(definitions, ports, &user_list)?;
    let mut scanner = Scanner {
        reader: Reader::new(&as_if, written),
        projects: Projects::new(ports),
        entries: Vec::new(),
        readers: Vec::new(),
    };

    for home in &found {
        scanner.read_registries(home, &definitions[home.agent_index]);
    }
    for path in &user_list.projects {
        scanner.projects.add_by_user(path);
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
                scanner.read_location(home, definition, location, path, None, 0);
            }
        }
    }
    let folders: Vec<(PathBuf, String)> = scanner
        .projects
        .list
        .iter()
        .filter(|project| project.is_folder())
        .map(|project| (project.path.clone(), project.key.clone()))
        .collect();
    for home in &found {
        let definition = &definitions[home.agent_index];
        for location in definition.locations.iter() {
            if !location.anchor.in_project() {
                continue;
            }
            for (project_folder, key) in &folders {
                for (folder, depth) in folders_read(ports, project_folder, location.anchor.walk()) {
                    if let Some(path) = location.anchor.path(&home.roots, Some(&folder)) {
                        let key = Some(key.clone());
                        scanner.read_location(home, definition, location, path, key, depth);
                    }
                }
            }
        }
    }

    let mut projects = scanner.projects.list;
    projects.sort_by(|a, b| a.key.cmp(&b.key));
    for project in &mut projects {
        project.hidden = user_list
            .hidden_projects
            .iter()
            .any(|hidden| project.spellings.contains(&paths::lexical_key(hidden)));
    }
    let (switches, unreadable_switches) =
        read_switches(definitions, &found, &projects, &mut scanner.reader);
    let switch_files = switch_files(definitions, &found, &projects);
    let gates = read_gates(definitions, &found, &mut scanner.reader, ports);
    scanner.entries = one_per_entry(definitions, scanner.entries);
    let managers = skill_managers::built_in().unwrap_or_default();
    for entry in &mut scanner.entries {
        let location = definitions[entry.agent_index].location(&entry.location);
        if location.format.skill_folder().is_some() {
            entry.manager = manager_of(managers, &mut scanner.reader, ports, entry);
        }
    }
    // A skill folder is git-versioned when a file below it is.
    let skill_folders = scanner
        .entries
        .iter()
        .filter(|entry| entry.key_path.is_empty())
        .map(|entry| entry.path.as_path());
    let locations = scanner
        .readers
        .iter()
        .map(|reader| reader.path.as_path())
        .filter(|path| ports.fs.exists(path))
        .chain(skill_folders);
    let switch_files = switch_files
        .iter()
        .map(PathBuf::as_path)
        .filter(|path| path.parent().is_some_and(|folder| ports.fs.exists(folder)));
    let versioned = versioned(ports, locations.chain(switch_files));
    let last_ran: Vec<Option<String>> = found
        .iter()
        .map(|home| last_ran(&definitions[home.agent_index], home, &mut scanner.reader))
        .collect();
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
        installations: installations::find(definitions, ports),
        homes: found
            .into_iter()
            .zip(last_ran)
            .map(|(home, last_ran)| ScannedHome {
                last_ran,
                configured: home.configured,
                hidden: user_list.hidden_homes.iter().any(|hidden| {
                    hidden.agent == definitions[home.agent_index].agent.id
                        && hidden.home.eq_ignore_ascii_case(&home.home)
                }),
                agent_index: home.agent_index,
                home: home.home,
                folder: home.folder,
                roots: home.roots,
                source: home.source,
            })
            .collect(),
        projects,
        entries: scanner.entries,
        readers: scanner.readers,
        read_failures: failures,
        versioned,
        switches,
        unreadable_switches,
        gates,
    })
}

/// The version that last ran against `home`, as the string its definition's
/// `last_ran` points to; `None` when there is none or it cannot be read.
fn last_ran(definition: &AgentDefinition, home: &FoundHome, reader: &mut Reader) -> Option<String> {
    let last_ran = definition.last_ran.as_ref()?;
    let path = last_ran.anchor.path(&home.roots, None)?;
    let text = reader.peek(&path).ok()??;
    let document = JsonFormat::STRICT.parse(text).ok()?;
    let value = lookup(&document, &last_ran.key_path)?;
    Some(value.as_str()?.to_owned())
}

/// Each Feature gate of each found Agent home: bypassed when a variable
/// makes the Agent ignore its cache, else the value it cached, else
/// unreadable. Nothing here applies the gate's assumed default.
fn read_gates(
    definitions: &[AgentDefinition],
    found: &[FoundHome],
    reader: &mut Reader,
    ports: &Ports,
) -> Vec<ScannedGate> {
    let mut gates = Vec::new();
    for home in found {
        let definition = &definitions[home.agent_index];
        for gate in &definition.feature_gates {
            let Some(path) = gate.lookup.anchor().path(&home.roots, None) else {
                continue;
            };
            let bypassed = gate.bypass.as_ref().and_then(|bypass| {
                let maps: Vec<serde_json::Map<String, serde_json::Value>> = bypass
                    .env_maps()
                    .iter()
                    .filter_map(|map| {
                        let path = map.anchor.path(&home.roots, None)?;
                        let document = JsonFormat::STRICT.parse(reader.peek(&path).ok()??).ok()?;
                        lookup(&document, &map.key_path)?.as_object().cloned()
                    })
                    .collect();
                // A variable in a map replaces the one in the environment.
                let var = |name: &str| {
                    maps.iter()
                        .find_map(|map| map.get(name))
                        .map(|value| match value {
                            serde_json::Value::String(text) => text.clone(),
                            other => other.to_string(),
                        })
                        .or_else(|| ports.env.var(name))
                };
                bypass.bypassed_by(&var)
            });
            let reading = match bypassed {
                Some(variable) => GateReading::Bypassed(variable),
                None => {
                    let document = match gate.lookup.store() {
                        GateStore::JsonFile => reader
                            .peek(&path)
                            .ok()
                            .flatten()
                            .and_then(|text| JsonFormat::STRICT.parse(text).ok()),
                        GateStore::SqliteItem(key) => reader
                            .fs
                            .read_sqlite_item(&path, key)
                            .ok()
                            .flatten()
                            .and_then(|text| JsonFormat::STRICT.parse(&text).ok()),
                    };
                    document
                        .and_then(|document| gate.lookup.cached(&document, &gate.name))
                        .map_or(GateReading::Unreadable, GateReading::Cached)
                }
            };
            gates.push(ScannedGate {
                agent_index: home.agent_index,
                home: home.home.clone(),
                name: gate.name.clone(),
                path,
                reading,
            });
        }
    }
    gates
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
                        .filter(|project| project.is_folder())
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
                for project in projects.iter().filter(|project| project.is_folder()) {
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

fn find_homes(
    definitions: &[AgentDefinition],
    ports: &Ports,
    user_list: &UserList,
) -> Result<Vec<FoundHome>, ScanError> {
    let fs = ports.fs.as_ref();
    let added_to = |agent: &str, folder: &Path| {
        let key = paths::canonical_key(fs, folder);
        user_list
            .homes
            .iter()
            .any(|added| added.agent == agent && paths::canonical_key(fs, &added.folder) == key)
    };
    let mut found = Vec::new();
    let mut definition_folders = Vec::new();
    for (agent_index, definition) in definitions.iter().enumerate() {
        for home in &definition.homes {
            let folder =
                home.folder
                    .locate(ports.env.as_ref())
                    .ok_or_else(|| ScanError::HomeUnknown {
                        agent: definition.agent.name.clone(),
                        home: home.id.clone(),
                    })?;
            definition_folders.push((agent_index, paths::canonical_key(fs, &folder)));
            let mut roots = home.roots_in(&folder);
            // A definition home the user added keeps its id, so that it is
            // the same home before and after its files appear.
            let source = if roots.values().any(|root| fs.exists(root)) {
                HomeSource::Default
            } else if added_to(&definition.agent.id, &folder) {
                HomeSource::Added
            } else {
                tracing::debug!(agent = %definition.agent.id, home = %home.id, "home not found");
                continue;
            };
            tracing::debug!(agent = %definition.agent.id, home = %home.id, "home found");
            let configured = source == HomeSource::Default;
            roots.extend(machine_paths(definition, ports));
            found.push(FoundHome {
                agent_index,
                home: home.id.clone(),
                folder,
                roots,
                source,
                configured,
            });
        }
    }
    // Any other home the user added is listed even with none of its roots,
    // laid out like the definition's first home. Its id is its folder.
    for added in &user_list.homes {
        let Some(agent_index) = definitions
            .iter()
            .position(|definition| definition.agent.id == added.agent)
        else {
            tracing::debug!(agent = %added.agent, "added home of an Agent not loaded");
            continue;
        };
        let key = paths::canonical_key(fs, &added.folder);
        if definition_folders.contains(&(agent_index, key)) {
            continue;
        }
        let definition = &definitions[agent_index];
        let mut roots = definition.homes[0].roots_in(&added.folder);
        let configured = roots.values().any(|root| fs.exists(root));
        roots.extend(machine_paths(definition, ports));
        found.push(FoundHome {
            agent_index,
            home: added.folder.display().to_string(),
            folder: added.folder.clone(),
            roots,
            source: HomeSource::Added,
            configured,
        });
    }
    Ok(found)
}

/// The machine-wide paths a definition's locations name, each by the text
/// that names it, as far as the environment says where they are.
fn machine_paths(definition: &AgentDefinition, ports: &Ports) -> BTreeMap<String, PathBuf> {
    definition
        .locations
        .iter()
        .filter_map(|location| location.anchor.machine())
        .filter_map(|text| Some((text.to_owned(), paths::expand(text, ports.env.as_ref())?)))
        .collect()
}

/// The folders a location in the Project folder `folder` is read in, with
/// how many levels up each one is.
fn folders_read(ports: &Ports, folder: &Path, walk: ProjectWalk) -> Vec<(PathBuf, usize)> {
    let fs = ports.fs.as_ref();
    match walk {
        ProjectWalk::None => vec![(folder.to_path_buf(), 0)],
        ProjectWalk::UpToRepoRoot => {
            let up: Vec<&Path> = folder.ancestors().collect();
            match up.iter().position(|parent| fs.exists(&parent.join(".git"))) {
                Some(root) => up[..=root]
                    .iter()
                    .enumerate()
                    .map(|(depth, parent)| (parent.to_path_buf(), depth))
                    .collect(),
                None => vec![(folder.to_path_buf(), 0)],
            }
        }
    }
}

/// The Entries, each one once: an Entry several locations of an Agent home
/// reach, such as a skill folder through a linked folder, is
/// kept once, from the location the Agent ranks highest, and the nearest
/// Project folder among equals. An Entry keyed to one Project and another
/// keyed to a second Project are not the same.
fn one_per_entry(definitions: &[AgentDefinition], entries: Vec<ScannedEntry>) -> Vec<ScannedEntry> {
    let rank = |entry: &ScannedEntry| {
        let definition = &definitions[entry.agent_index];
        (
            definition.rank(definition.location(&entry.location)),
            entry.depth,
        )
    };
    let mut kept: Vec<ScannedEntry> = Vec::new();
    for entry in entries {
        let same = kept.iter().position(|known| {
            known.agent_index == entry.agent_index
                && known.home == entry.home
                && known.path == entry.path
                && known.key_path == entry.key_path
                && (known.project.is_none()
                    || entry.project.is_none()
                    || known.project == entry.project)
        });
        match same {
            Some(index) if rank(&entry) < rank(&kept[index]) => kept[index] = entry,
            Some(_) => {}
            None => kept.push(entry),
        }
    }
    kept
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
        depth: usize,
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
        let format: &dyn Parse = match &location.format {
            Format::Json(format) => format,
            Format::Toml(format) => format,
            Format::SkillFolder(folder) => {
                return self.read_skills(home, location, folder, &path, project, depth);
            }
        };
        let Some(text) = self.reader.text(home, &path, project.as_deref()) else {
            return;
        };
        let document = match format.parse(text) {
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
                    depth,
                    defect: None,
                    link: None,
                    manager: None,
                });
            }
        }
    }
}

impl Scanner<'_> {
    /// Adds a skill Entry for each folder holding a `SKILL.md` below the
    /// folder `root`, at the real path its parent folders lead to. A folder
    /// named with a leading `.` is never read, and one without `SKILL.md`
    /// only when the location is recursive.
    fn read_skills(
        &mut self,
        home: &FoundHome,
        location: &LocationDefinition,
        format: &SkillFolder,
        root: &Path,
        project: Option<String>,
        depth: usize,
    ) {
        let fs = self.reader.fs;
        let mut folders = vec![root.to_path_buf()];
        // A link back to a folder already read would never end.
        let mut seen = BTreeSet::new();
        while let Some(folder) = folders.pop() {
            if !seen.insert(paths::canonical_key(fs, &folder)) {
                continue;
            }
            let mut children = match fs.read_dir(&folder) {
                Ok(children) => children,
                Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
                Err(error) => {
                    let kind = match error.kind() {
                        io::ErrorKind::PermissionDenied => ReadFailureKind::Permission,
                        _ => ReadFailureKind::Io,
                    };
                    self.reader
                        .fail(home, &folder, project.as_deref(), kind, None);
                    continue;
                }
            };
            children.sort();
            // Links in the parent folders are resolved, so that a skill
            // reached through a linked folder is one Entry.
            let real = fs
                .real_path(&folder)
                .map_or(folder, |real| paths::lexical(&real));
            for child in children {
                let Some(name) = child.file_name().and_then(|name| name.to_str()) else {
                    continue;
                };
                if name.starts_with('.') {
                    continue;
                }
                // A link whose target is gone is still an Entry, which
                // only Remove takes away.
                if let Some(target) = fs.link_target(&child)
                    && !fs.exists(&child)
                {
                    self.entries.push(ScannedEntry {
                        agent_index: home.agent_index,
                        home: home.home.clone(),
                        location: location.id.clone(),
                        path: real.join(name),
                        key_path: Vec::new(),
                        name: format.sanitize(name),
                        project: project.clone(),
                        config: serde_json::Value::Object(serde_json::Map::new()),
                        masked: Vec::new(),
                        depth,
                        defect: None,
                        link: Some(ScannedLink {
                            target: paths::lexical(&target),
                            real: None,
                        }),
                        manager: None,
                    });
                    continue;
                }
                if fs.read_dir(&child).is_err() {
                    continue;
                }
                let skill_md = child.join(SKILL_MD);
                if !fs.exists(&skill_md) {
                    if format.recursive {
                        folders.push(child);
                    }
                    continue;
                }
                let Some(text) = self.reader.text(home, &skill_md, project.as_deref()) else {
                    continue;
                };
                let skill = read_skill(format, name, text);
                tracing::debug!(location = %location.id, skill = %skill.name, "read skill");
                // A link at the skill's own location is an Entry apart from
                // its target.
                let link = fs.link_target(&child).map(|target| ScannedLink {
                    target: paths::lexical(&target),
                    real: fs.real_path(&child).map(|real| paths::lexical(&real)),
                });
                self.entries.push(ScannedEntry {
                    agent_index: home.agent_index,
                    home: home.home.clone(),
                    location: location.id.clone(),
                    path: real.join(name),
                    key_path: Vec::new(),
                    name: skill.name,
                    project: project.clone(),
                    config: skill.config,
                    masked: Vec::new(),
                    depth,
                    defect: skill.defect,
                    link,
                    manager: None,
                });
            }
        }
    }
}

/// The skill manager whose detectors match the skill folder of `entry`
/// most strongly, the first one loaded among equals.
fn manager_of(
    managers: &[SkillManagerDefinition],
    reader: &mut Reader,
    ports: &Ports,
    entry: &ScannedEntry,
) -> Option<ManagerMatch> {
    let mut best: Option<ManagerMatch> = None;
    for (manager, definition) in managers.iter().enumerate() {
        for detector in &definition.detectors {
            let strength = detector.strength();
            if best.is_some_and(|best| best.strength >= strength) {
                continue;
            }
            if detects(detector, reader, ports, entry) {
                best = Some(ManagerMatch { manager, strength });
            }
        }
    }
    best
}

/// Whether `detector` matches the skill folder of `entry`.
fn detects(detector: &Detector, reader: &mut Reader, ports: &Ports, entry: &ScannedEntry) -> bool {
    match detector {
        Detector::LinkTargetGlob(block) => entry.link.as_ref().is_some_and(|link| {
            let real = link.real.iter();
            std::iter::once(&link.target)
                .chain(real)
                .any(|target| paths::tail_matches(&block.glob, target))
        }),
        Detector::MarkerFile(block) => reader.fs.exists(&entry.path.join(&block.file)),
        Detector::LocationGlob(block) => paths::tail_matches(&block.glob, &entry.path),
        Detector::LockFileEntry(block) => {
            let anchored = block.file.starts_with("~/") || block.file.starts_with('%');
            let lock = if anchored {
                paths::expand(&block.file, ports.env.as_ref())
            } else {
                Some(paths::lexical(&entry.path.join(&block.file)))
            };
            let Some(Ok(Some(text))) = lock.as_deref().map(|lock| reader.peek(lock)) else {
                return false;
            };
            let Ok(document) = JsonFormat::STRICT.parse(text) else {
                return false;
            };
            let folder = entry.path.file_name().unwrap_or_default().to_string_lossy();
            let keys = lookup(&document, &block.key_path).and_then(serde_json::Value::as_object);
            keys.into_iter().flat_map(serde_json::Map::keys).any(|key| {
                let name = block
                    .sanitize
                    .map_or_else(|| key.clone(), |sanitize| sanitize.apply(key));
                name.eq_ignore_ascii_case(&folder)
            })
        }
    }
}

/// The file that makes a folder a skill.
const SKILL_MD: &str = "SKILL.md";

/// A skill as its Agent reads it.
struct ReadSkill {
    /// The name the Agent gives it, which it competes by.
    name: String,
    /// The frontmatter's `name` and `description`, as far as it has them.
    config: serde_json::Value,
    /// Why the Agent may not read its `SKILL.md` as written.
    defect: Option<SkillDefect>,
}

/// The skill in the folder named `folder`, whose `SKILL.md` holds `text`,
/// as an Agent that reads skill folders as `format` says reads it.
fn read_skill(format: &SkillFolder, folder: &str, text: &str) -> ReadSkill {
    let mut config = serde_json::Map::new();
    let mut defect = None;
    let mut named = None;
    match skill::frontmatter(text) {
        Ok(keys) => {
            for key in ["name", "description"] {
                if let Some(value) = keys.get(key) {
                    config.insert(key.to_owned(), value.clone().into());
                }
            }
            named = keys.get("name").filter(|name| !name.is_empty()).cloned();
        }
        Err(found) => defect = Some(found),
    }
    let name = match (format.name_from, named) {
        (NameFrom::Frontmatter, Some(name)) => name,
        (NameFrom::Frontmatter, None) => {
            defect.get_or_insert(SkillDefect::NoName);
            folder.to_owned()
        }
        (NameFrom::Folder, _) => folder.to_owned(),
    };
    if format.is_reserved(folder) {
        defect = Some(SkillDefect::Reserved);
    }
    ReadSkill {
        name: format.sanitize(&name),
        config: config.into(),
        defect,
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
    /// The file system as the scan reads it.
    fs: &'p dyn FileSystem,
    /// Files read as holding these bytes instead of what is on disk.
    written: &'p [(PathBuf, Vec<u8>)],
    files: BTreeMap<PathBuf, Result<Option<String>, ReadFailureKind>>,
    reported: BTreeSet<(usize, String, PathBuf)>,
    failures: Vec<ScannedReadFailure>,
}

impl<'p> Reader<'p> {
    fn new(fs: &'p dyn FileSystem, written: &'p [(PathBuf, Vec<u8>)]) -> Self {
        Self {
            fs,
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
            None => read(self.fs, path),
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
fn read(fs: &dyn FileSystem, path: &Path) -> Result<Option<String>, ReadFailureKind> {
    let bytes = match fs.read(path) {
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
                            workspace_file: is_workspace(&path)
                                && self.ports.fs.read_dir(&path).is_err(),
                            path,
                            key: key.clone(),
                            exists,
                            spellings: BTreeSet::from([key.clone()]),
                            registered_by: BTreeSet::new(),
                            added: false,
                            hidden: false,
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

    /// Records a Project the user added.
    fn add_by_user(&mut self, path: &Path) {
        if let Some(key) = self.add(path, None)
            && let Some(project) = self.list.iter_mut().find(|project| project.key == key)
        {
            project.added = true;
        }
    }
}

/// Whether a Project path is named like a VS Code workspace file.
pub(crate) fn is_workspace(path: &Path) -> bool {
    path.extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("code-workspace"))
}
