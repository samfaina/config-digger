//! `scan()`: reads every location of every found Agent home and Project into
//! an immutable snapshot of Projects, Entries and Read failures. Nothing here
//! decides a State.

use crate::api::ReadFailureKind;
use crate::definitions::blocks::{
    self, Anchor, Anchored, Bypasses, Cached, DisableMechanism, Disables, FindsBy, Format, Gate,
    GateStore, HoldsHere, JsonDialect, JsonFormat, JsonPluginRecords, ListPlugins, Locate, LookUp,
    ManifestSkills, NAME_KEY, NameFrom, PLUGIN_KEY, Parse, PluginCache, PluginFolders,
    PluginRecord, PluginRegistry, ProjectPluginKeys, ProjectWalk, Register, Registry,
    RelativePaths, SettingsSource, SkillDefect, SkillFolder, SkillList, Spelling, StoreClass,
    is_entry_key,
};
use crate::definitions::skill_managers::{Detector, SkillManagerDefinition, Strength};
use crate::definitions::{
    ANY_KEY, AgentDefinition, Definitions, ENTRY_NAME, FOLDER_INDEX, HomeDefinition, ItemKind,
    LocationDefinition, NameParts, PROJECT_KEY, PluginNames, PluginsDefinition, References,
};
use crate::folders::{AsIf, PlannedFolder};
use crate::installations::{self, ScannedInstallation};
use crate::ports::{FileSystem, Ports};
use crate::store::Written;
use crate::user_list::{self, UserList};
use crate::{mask, park, paths, skill, version};
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
    /// The files of managed-exclusive gates that exist, so that they take
    /// control.
    pub exclusive: BTreeSet<PathBuf>,
    /// Every `[[settings]]` of every definition, with the source the Agent reads.
    pub settings: Vec<ScannedSettings>,
    /// Every value a rule reads, at every place.
    pub setting_values: Vec<ScannedSetting>,
    /// The files a rule reads that cannot be read or parsed. They set
    /// nothing.
    pub unreadable_settings: BTreeSet<PathBuf>,
    /// The manifest of config-digger's parked-Entry store, `None` when
    /// there is no data folder to keep it in.
    pub parked_store: Option<PathBuf>,
    /// Where each parked Entry, listed among the Entries at its key path in
    /// the store, came from.
    pub parked: Vec<ScannedParked>,
    /// The folder each Agent home keeps for each Project.
    pub storage: Vec<StorageFolder>,
    /// Each file read for a location that is a copy the Agent caches, with
    /// when it cached it, when the file says.
    pub cached: BTreeMap<PathBuf, Option<SystemTime>>,
    /// The files each Agent, by its index, skips without reading them,
    /// since someone other than an administrator can change them.
    pub not_admin_owned: NotAdminOwned,
}

/// A parked Entry: one at `key_path` in the parked-Entry store, moved there
/// from `origin`.
#[derive(Clone)]
pub(crate) struct ScannedParked {
    /// Its key path in the store.
    pub key_path: Vec<String>,
    /// The file it was parked from, and its key path there.
    pub origin: PathBuf,
    pub origin_key_path: Vec<String>,
    /// When it was parked, in RFC 3339.
    pub parked_at: String,
    /// For a skill, where the store keeps its folder.
    pub folder: Option<PathBuf>,
}

impl Snapshot {
    /// Where `entry` came from, when it is parked.
    pub fn parked_of(&self, entry: &ScannedEntry) -> Option<&ScannedParked> {
        if self.parked_store.as_ref() != Some(&entry.path) {
            return None;
        }
        self.parked
            .iter()
            .find(|parked| parked.key_path == entry.key_path)
    }
}

/// A plugin an Agent home loads, as one of its records says.
#[derive(Clone)]
pub(crate) struct ScannedPlugin {
    /// The id of the `[[plugins]]` that lists it.
    pub plugins: String,
    /// Such as `supabase@claude-plugins-official`.
    pub id: String,
    /// The name its Entries are named by, such as `supabase`.
    pub name: String,
    /// The folder its files are in.
    pub folder: PathBuf,
    /// The Scope it is installed at, as the Agent records it.
    pub scope: Option<String>,
    /// The key of the one Project it applies in, `None` when it applies in every one.
    pub project: Option<String>,
    /// Its version, as the Agent records it.
    pub version: Option<String>,
    /// Another plugin of the same list has its name, and the Agent refuses
    /// both.
    pub name_shared: bool,
}

/// A value a rule or gate reads, at one place, for one Agent home.
#[derive(Clone)]
pub(crate) struct ScannedSetting {
    pub agent_index: usize,
    pub home: String,
    pub kind: ItemKind,
    pub owner: SettingOwner,
    pub path: PathBuf,
    /// The key of the Project it applies in, or `None` when it applies in every context.
    pub project: Option<String>,
    /// For a file per plugin or per Entry, which one it is the file of.
    pub file_of: Option<Owner>,
    /// Its key path, `{project}` replaced by the Project key in the Agent's
    /// own spelling.
    pub key_path: Vec<String>,
    /// What the file holds there: `None` when the file or key is missing.
    pub value: Option<serde_json::Value>,
    /// When the Agent cached the value, for a value it caches.
    pub cached_at: Option<SystemTime>,
}

/// The rule or gate a setting is read for, by its index among those of its
/// Item kind, or the setting its competition depends on, and the source
/// among its sources.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum SettingOwner {
    Rule {
        rule: usize,
        source: usize,
    },
    Gate {
        gate: usize,
        source: usize,
    },
    /// A source of the setting the Item kind's competition depends on.
    Competition {
        source: usize,
    },
    /// A source of the setting of the `when` condition `condition` of the
    /// location at `location` among the definition's.
    Condition {
        location: usize,
        condition: usize,
        source: usize,
    },
    /// A source of the setting under which the Agent fetches the Feature
    /// gate at `gate` among the definition's.
    FeatureGate {
        gate: usize,
        source: usize,
    },
}

impl Snapshot {
    /// The values of `owner` read in the Agent home of `entry` that apply
    /// to it in the context of the Project with the key `project`.
    pub fn settings_for<'s>(
        &'s self,
        entry: &'s ScannedEntry,
        kind: ItemKind,
        owner: SettingOwner,
        project: Option<&'s str>,
    ) -> impl Iterator<Item = &'s ScannedSetting> {
        self.setting_values.iter().filter(move |value| {
            value.agent_index == entry.agent_index
                && value.home == entry.home
                && value.kind == kind
                && value.owner == owner
                && (value.project.is_none() || value.project.as_deref() == project)
                && applies(value.file_of.as_ref(), entry)
        })
    }
}

/// What a file read once per plugin, or once per Entry, is the file of.
#[derive(Clone, PartialEq)]
pub(crate) enum Owner {
    /// The plugin with this id.
    Plugin(String),
    /// The Entry at this path.
    Entry(PathBuf),
}

/// Whether what the file of `file_of` holds applies to `entry`: the file of
/// its plugin, or its own. What any other file holds applies to every Entry.
pub(crate) fn applies(file_of: Option<&Owner>, entry: &ScannedEntry) -> bool {
    match file_of {
        None => true,
        Some(Owner::Plugin(id)) => entry.plugin.as_ref().is_some_and(|own| own.id == *id),
        Some(Owner::Entry(path)) => entry.path == *path,
    }
}

/// One `[[settings]]` of an Agent, with the files of its sources.
#[derive(Clone)]
pub(crate) struct ScannedSettings {
    pub agent_index: usize,
    pub id: String,
    /// The files of each source, highest first. A registry value stands as
    /// `<HKLM|HKCU>\<key>\<value>`.
    pub sources: Vec<Vec<PathBuf>>,
    /// The source the Agent reads: the first that holds any setting.
    pub selected: Option<usize>,
}

impl ScannedSettings {
    /// The files of the source the Agent reads.
    pub fn read(&self) -> &[PathBuf] {
        self.selected
            .map_or(&[], |selected| &self.sources[selected])
    }

    /// Whether `path` is a file of a source the Agent skips.
    pub fn skips(&self, path: &Path) -> bool {
        self.sources.iter().flatten().any(|file| file == path)
            && !self.read().contains(&path.to_path_buf())
    }
}

impl Snapshot {
    /// The `[[settings]]` `id` of the Agent at `agent_index`.
    pub fn settings_of(&self, agent_index: usize, id: &str) -> &ScannedSettings {
        settings_named(&self.settings, agent_index, id)
            .expect("validation declares the settings an anchor names")
    }
}

/// The `[[settings]]` `id` of the Agent at `agent_index` among `settings`.
fn settings_named<'s>(
    settings: &'s [ScannedSettings],
    agent_index: usize,
    id: &str,
) -> Option<&'s ScannedSettings> {
    settings
        .iter()
        .find(|known| known.agent_index == agent_index && known.id == id)
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
    /// For a file per plugin or per Entry, which one it is the file of.
    pub file_of: Option<Owner>,
    /// Its key path, `{project}` replaced by the Project key in the Agent's
    /// own spelling, `{name}` or `{plugin_id}` left in place.
    pub key_path: Vec<String>,
    /// What the file holds at the key path, up to `{name}` or `{plugin_id}`
    /// when it has one:
    /// `None` when the file or key is missing.
    pub value: Option<serde_json::Value>,
    /// When the value is a list, what of each item holds the name it stands
    /// for: a field, or an index in a list of lists.
    pub named_by: Option<String>,
    /// Whether names in the list compare without regard to case.
    pub ignore_case: bool,
    /// Whether of several items naming one Entry, the last counts.
    pub later_wins: bool,
    /// How the file behaves while the Agent runs.
    pub store_class: StoreClass,
}

impl ScannedSwitch {
    /// The switch's key path for the Entry it holds by `key`: its name, or
    /// its plugin's id, or in a list, the index of the item it names.
    pub fn key_path_for(&self, key: &str) -> Vec<String> {
        let index = self.item(key).map(|(index, _)| index.to_string());
        let key = index.as_deref().unwrap_or(key);
        self.key_path
            .iter()
            .map(|known| if is_entry_key(known) { key } else { known }.to_owned())
            .collect()
    }

    /// What the switch holds for the Entry it holds by `key`.
    pub fn value_for(&self, key: &str) -> Option<&serde_json::Value> {
        let value = self.value.as_ref()?;
        match self.key_path.iter().position(|known| is_entry_key(known)) {
            Some(at) => {
                let held = match self.item(key) {
                    Some((_, item)) => item,
                    None => value.get(key)?,
                };
                lookup(held, &self.key_path[at + 1..])
            }
            None => Some(value),
        }
    }

    /// In a list named by a field or index, the item that names `key`, and
    /// its index.
    fn item(&self, key: &str) -> Option<(usize, &serde_json::Value)> {
        let field = self.named_by.as_deref()?;
        let items = self.value.as_ref()?.as_array()?;
        let names_key = |(_, item): &(usize, &serde_json::Value)| {
            let name = match item {
                serde_json::Value::Array(values) => field
                    .parse::<usize>()
                    .ok()
                    .and_then(|index| values.get(index)),
                item => item.get(field),
            };
            name.and_then(serde_json::Value::as_str)
                .is_some_and(|name| {
                    if self.ignore_case {
                        name.eq_ignore_ascii_case(key)
                    } else {
                        name == key
                    }
                })
        };
        let mut naming = items.iter().enumerate();
        if self.later_wins {
            naming.rfind(names_key)
        } else {
            naming.find(names_key)
        }
    }
}

/// The value at `key_path` below `value`, where a key below a list is an
/// index into it.
fn lookup<'v>(value: &'v serde_json::Value, key_path: &[String]) -> Option<&'v serde_json::Value> {
    key_path.iter().try_fold(value, |value, key| match value {
        serde_json::Value::Array(items) => items.get(key.parse::<usize>().ok()?),
        value => value.get(key),
    })
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

    /// What the Agent home of `entry` is a copy of, when another tool
    /// copies it from another home.
    pub fn mirror_of(&self, entry: &ScannedEntry) -> Option<&Mirror> {
        self.home_of(entry).mirror.as_ref()
    }

    /// The Agent home of `entry`.
    pub fn home_of(&self, entry: &ScannedEntry) -> &ScannedHome {
        self.homes
            .iter()
            .find(|found| found.agent_index == entry.agent_index && found.home == entry.home)
            .expect("Entries come from found homes")
    }

    /// The Entry that `entry`, of a Mirrored home, was copied from: the
    /// Entry of the same location, Project and key path in the source home,
    /// in the file at the same place below the source home's folder. `None`
    /// for an Entry of no Mirrored home, or one the source home lacks.
    pub fn copied_from(&self, entry: &ScannedEntry) -> Option<&ScannedEntry> {
        let mirror = self.mirror_of(entry)?;
        let folder_of = |home: &str| {
            self.homes
                .iter()
                .find(|found| found.agent_index == entry.agent_index && found.home == home)
                .map(|found| &found.folder)
        };
        let source_folder = folder_of(&mirror.source)?;
        let path = match entry.path.strip_prefix(folder_of(&entry.home)?) {
            Ok(relative) => source_folder.join(relative),
            Err(_) => entry.path.clone(),
        };
        self.entries.iter().find(|source| {
            source.agent_index == entry.agent_index
                && source.home == mirror.source
                && source.location == entry.location
                && source.project == entry.project
                && source.key_path == entry.key_path
                && source.path == path
        })
    }

    /// The Entry at `path` and `key_path` that the Agent home `home` of the
    /// Agent at `agent_index` reads, where several homes read one file.
    pub fn entry_in(
        &self,
        agent_index: usize,
        home: &str,
        path: &Path,
        key_path: &[String],
    ) -> Option<&ScannedEntry> {
        self.entries.iter().find(|scanned| {
            scanned.agent_index == agent_index
                && scanned.home == home
                && scanned.path == path
                && scanned.key_path == key_path
        })
    }

    /// The Entry at `path` and `key_path`, which a resolved Entry comes from.
    pub fn entry(&self, path: &Path, key_path: &[String]) -> &ScannedEntry {
        self.entries
            .iter()
            .find(|scanned| scanned.path == path && scanned.key_path == key_path)
            .expect("a resolved Entry comes from the snapshot")
    }

    /// The file `anchor` names for `entry` in the context of the Project
    /// with the key `project` and the folder `folder`, as
    /// [`ScannedEntry::file_of`] finds it, or for an anchor in the folder the
    /// Agent home keeps for each Project, in that Project's.
    pub fn file_for(
        &self,
        entry: &ScannedEntry,
        anchor: &Anchor,
        project: Option<(&str, &Path)>,
    ) -> Option<PathBuf> {
        let roots = self.roots(entry.agent_index, &entry.home);
        let Some(relative) = anchor.project_storage() else {
            let folder = project
                .map(|(_, folder)| folder)
                .filter(|_| anchor.in_project());
            return entry.file_of(anchor, roots, folder);
        };
        let (key, _) = project?;
        storage_paths(&self.storage, entry.agent_index, &entry.home, relative)
            .find(|(_, project)| *project == key)
            .map(|(path, _)| path)
    }

    /// The Project with the key `key`.
    pub fn project(&self, key: &str) -> Option<&ScannedProject> {
        self.projects.iter().find(|project| project.key == key)
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
                && applies(switch.file_of.as_ref(), entry)
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
    /// The id of the definition's home it was found as, `None` for one
    /// the user added. It differs from `home` when the home's folder glob
    /// matches several folders, each a home with its folder as its id.
    pub declared: Option<String>,
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
    /// When the Agent's lookup order skips it, what it reads instead.
    pub skipped: Option<Skipped>,
    /// For a home another tool copies from one of the Agent's, which.
    pub mirror: Option<Mirror>,
}

/// An Agent home another tool copies from a source Agent home of the same
/// Agent. config-digger never writes to it.
#[derive(Clone)]
pub(crate) struct Mirror {
    /// The tool that keeps the copy, such as `Orca`.
    pub owner: String,
    /// The id of the home it copies.
    pub source: String,
    /// That home's folder, `None` when the environment cannot say where it is.
    pub source_folder: Option<PathBuf>,
}

/// An Agent home the Agent's lookup order skips: the Agent never reads it.
#[derive(Clone)]
pub(crate) struct Skipped {
    /// The id of the home the order picks, when it is listed.
    pub instead: Option<String>,
    /// The folder the order picks, `None` when it picks none that is there.
    pub folder: Option<PathBuf>,
}

/// Where an Agent home came from.
#[derive(Clone, PartialEq, Eq)]
pub(crate) enum HomeSource {
    /// The location the Agent definition gives as its default.
    Default,
    /// The environment variable, by name, that holds its folder.
    Environment(String),
    /// Another location the Agent definition knows of, a fixed path or a
    /// search such as a glob.
    KnownLocation,
    /// The user added it, and no definition location finds it.
    Added,
}

impl HomeSource {
    /// Where a home that the definition's home `home` finds came from.
    fn of(home: &HomeDefinition) -> HomeSource {
        match home.folder.finds_by() {
            FindsBy::Variable(variable) => HomeSource::Environment(variable.to_owned()),
            FindsBy::FixedPath if home.default => HomeSource::Default,
            FindsBy::FixedPath | FindsBy::Search => HomeSource::KnownLocation,
        }
    }
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
    /// For a workspace file, the folders it opens, first first.
    pub members: Vec<PathBuf>,
    /// For a folder, the folder at or above it that holds `.git`, the root of
    /// its git repository.
    pub repo_root: Option<PathBuf>,
    /// For a folder in a git repository, the repository's main checkout: its
    /// root, or for a linked worktree, the checkout the worktree belongs to.
    pub main_checkout: Option<PathBuf>,
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
    /// For an Entry of a plugin, the plugin.
    pub plugin: Option<ScannedPlugin>,
    /// What the Agent's Disable switches hold it by, made by its location's
    /// `switch_id`; `None` for a location without one.
    pub switch_id: Option<String>,
    /// The hash of its configuration as written, Secrets included, for an
    /// Entry the Agent approves by it; never shown.
    pub config_hash: Option<String>,
}

/// The skill manager whose detectors match a skill folder most strongly.
#[derive(Clone)]
pub(crate) struct ManagerMatch {
    /// The id and display name of its definition.
    pub id: String,
    pub name: String,
    pub strength: Strength,
}

impl ScannedEntry {
    /// The file `anchor` names for this Entry, as [`Anchor::path_for`]
    /// finds it, given the roots of its Agent home and the Project folder.
    pub fn file_of(
        &self,
        anchor: &Anchor,
        roots: &BTreeMap<String, PathBuf>,
        folder: Option<&Path>,
    ) -> Option<PathBuf> {
        let plugin = self.plugin.as_ref().map(|plugin| plugin.id.as_str());
        anchor.path_for(roots, folder, plugin, &self.path)
    }
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
    /// The id of the definition's home it was found as, `None` for one
    /// the user added.
    declared: Option<String>,
    folder: PathBuf,
    /// Its roots by name, and the machine-wide paths of its definition by
    /// the text that names them.
    roots: BTreeMap<String, PathBuf>,
    source: HomeSource,
    /// Any of its own roots exists.
    configured: bool,
    skipped: Option<Skipped>,
    mirror: Option<Mirror>,
}

/// `extra_projects` are evaluated whether or not a registry records them.
pub(crate) fn scan(
    definitions: &Definitions,
    ports: &Ports,
    extra_projects: &[PathBuf],
) -> Result<Snapshot, ScanError> {
    scan_as_if(definitions, ports, extra_projects, &[], &[])
}

/// Scans as [`scan`] does, as if each location in `written` held what it
/// will hold and `folders` were done, so that a plan can predict what a scan
/// will read once it is done.
pub(crate) fn scan_as_if(
    definitions: &Definitions,
    ports: &Ports,
    extra_projects: &[PathBuf],
    written: &[Written],
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
        unmasked: Vec::new(),
        readers: Vec::new(),
        storage: Vec::new(),
        cached: BTreeMap::new(),
    };
    scanner.reader.not_admin_owned = find_not_admin_owned(definitions, &found, ports, &as_if);

    let settings = read_settings(definitions, ports, &mut scanner.reader);
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
    let mut installed = Installed::new();
    for home in &found {
        let definition = &definitions[home.agent_index];
        // A Project's own plugins wait for every Project.
        let plugins: Vec<ScannedPlugin> = definition
            .plugins
            .iter()
            .filter(|plugins| !plugins.registry.in_project())
            .flat_map(|plugins| scanner.read_plugins(home, plugins, None))
            .collect();
        installed.insert((home.agent_index, home.home.clone()), plugins.clone());
        scanner.read_plugin_locations(home, definition, &plugins);
        for location in definition.locations.iter() {
            if location.anchor.in_project() || location.anchor.plugin().is_some() {
                continue;
            }
            // Read in the folder the home keeps for each Project, whose
            // Entries apply in that Project.
            if let Some(relative) = location.anchor.project_storage() {
                let folders: Vec<(PathBuf, String)> =
                    storage_paths(&scanner.storage, home.agent_index, &home.home, relative)
                        .map(|(path, project)| (path, project.to_owned()))
                        .collect();
                let start = scanner.entries.len();
                for (path, project) in folders {
                    scanner.read_location(home, definition, location, path, Some(project), 0);
                }
                scanner.name_entries(start, location, &NameParts::default());
                continue;
            }
            // Every source is read, so that the Entries of those the Agent
            // skips are listed as Unread.
            if let Some(id) = location.anchor.settings() {
                let found = settings_named(&settings, home.agent_index, id);
                let start = scanner.entries.len();
                for path in found
                    .into_iter()
                    .flat_map(|found| found.sources.iter().flatten())
                {
                    scanner.read_location(home, definition, location, path.clone(), None, 0);
                }
                scanner.name_entries(start, location, &NameParts::default());
                continue;
            }
            if let Some(path) = location.anchor.path(&home.roots, None) {
                let start = scanner.entries.len();
                scanner.read_location(home, definition, location, path, None, 0);
                scanner.name_entries(start, location, &NameParts::default());
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
    // Each workspace file with its member folders, first first.
    let mut workspaces: Vec<(PathBuf, String, Vec<PathBuf>)> = Vec::new();
    for project in &scanner.projects.list {
        if project.exists && project.workspace_file {
            let members = members(&mut scanner.reader, &project.path);
            workspaces.push((project.path.clone(), project.key.clone(), members));
        }
    }
    for project in &mut scanner.projects.list {
        if let Some((_, _, members)) = workspaces.iter().find(|(path, ..)| *path == project.path) {
            project.members = members.clone();
        }
    }
    // The plugins each Project folder installs, whose Entries apply there.
    for home in &found {
        let definition = &definitions[home.agent_index];
        let mut plugins = Vec::new();
        for list in definition.plugins.iter() {
            if list.registry.in_project() {
                for (folder, key) in &folders {
                    plugins.extend(scanner.read_plugins(home, list, Some((folder, key))));
                }
            }
        }
        scanner.read_plugin_locations(home, definition, &plugins);
        installed
            .entry((home.agent_index, home.home.clone()))
            .or_default()
            .extend(plugins);
    }
    for home in &found {
        let definition = &definitions[home.agent_index];
        for location in definition.locations.iter() {
            if !location.anchor.in_project() {
                continue;
            }
            let walk = location.anchor.walk();
            // Each folder or file read, with the key of its Project and how
            // far it ranks below the Project's own.
            let mut read: Vec<(PathBuf, &String, usize)> = Vec::new();
            if !location.anchor.project_file() {
                for (project_folder, key) in &folders {
                    for (folder, depth) in folders_read(ports, project_folder, walk) {
                        read.push((folder, key, depth));
                    }
                }
            }
            if definition.projects.workspace_files {
                for (file, key, members) in &workspaces {
                    if location.anchor.project_file() {
                        read.push((file.clone(), key, 0));
                        continue;
                    }
                    for (index, member) in members.iter().enumerate() {
                        for (folder, depth) in folders_read(ports, member, walk) {
                            read.push((folder, key, index * MEMBER_RANKS + depth));
                        }
                    }
                }
            }
            // A file of the Agent home itself, such as `~\.codex\config.toml`
            // in a Project at the user's home folder, is the home's alone.
            let own: BTreeSet<String> = home
                .roots
                .values()
                .map(|root| paths::canonical_key(ports.fs.as_ref(), root))
                .collect();
            for (folder, key, depth) in read {
                if let Some(path) = location.anchor.path(&home.roots, Some(&folder))
                    && !own.contains(&paths::canonical_key(ports.fs.as_ref(), &path))
                {
                    let key = Some(key.clone());
                    let start = scanner.entries.len();
                    scanner.read_location(home, definition, location, path, key, depth);
                    let name = folder.file_name().unwrap_or_default().to_string_lossy();
                    let parts = NameParts {
                        plugin: None,
                        folder: Some((depth / MEMBER_RANKS, &name)),
                    };
                    scanner.name_entries(start, location, &parts);
                }
            }
        }
    }

    scanner.mask_entries(definitions);
    let (parked_store, parked) = scanner.read_parked(definitions, &found, ports);
    let mut projects = scanner.projects.list;
    projects.sort_by(|a, b| a.key.cmp(&b.key));
    for project in &mut projects {
        if project.is_folder() {
            (project.repo_root, project.main_checkout) = repository(&as_if, &project.path);
        }
        project.hidden = user_list
            .hidden_projects
            .iter()
            .any(|hidden| project.spellings.contains(&paths::lexical_key(hidden)));
    }
    let (switches, unreadable_switches) = read_switches(
        definitions,
        &found,
        &projects,
        &scanner.storage,
        &installed,
        &scanner.entries,
        &mut scanner.reader,
    );
    let (setting_values, unreadable_settings) = read_rules(
        definitions,
        &found,
        &projects,
        &scanner.storage,
        &installed,
        &settings,
        &mut scanner.reader,
    );
    let switch_files = switch_files(definitions, &found, &projects, &installed, &scanner.entries);
    let gates = read_gates(definitions, &found, &mut scanner.reader, ports);
    let exclusive = exclusive_files(definitions, &found, &as_if);
    scanner.entries = one_per_entry(definitions, scanner.entries);
    for entry in &mut scanner.entries {
        let location = definitions[entry.agent_index].location(&entry.location);
        entry.switch_id = location
            .switch_id
            .as_deref()
            .map(|template| switch_id(template, entry, &location.root));
    }
    for entry in &mut scanner.entries {
        let location = definitions[entry.agent_index].location(&entry.location);
        if location.format.skill_folder().is_some() {
            entry.manager = manager_of(
                &definitions.skill_managers,
                &mut scanner.reader,
                ports,
                entry,
            );
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
    // even from a location of it that did parse. One in a settings source
    // the Agent skips, or in a home its lookup order skips, is no failure
    // of the Agent's.
    let mut failures = scanner.reader.failures;
    failures.retain(|failure| {
        !settings
            .iter()
            .any(|known| known.agent_index == failure.agent_index && known.skips(&failure.path))
            && !scanner
                .reader
                .not_admin_owned
                .holds(failure.agent_index, &failure.path)
            && !found.iter().any(|home| {
                home.agent_index == failure.agent_index
                    && home.home == failure.home
                    && home.skipped.is_some()
            })
    });
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
                skipped: home.skipped,
                mirror: home.mirror,
                declared: home.declared,
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
        exclusive,
        settings,
        setting_values,
        unreadable_settings,
        parked_store,
        parked,
        storage: scanner.storage,
        cached: scanner.cached,
        not_admin_owned: scanner.reader.not_admin_owned,
    })
}

/// The files of each source of each `[[settings]]`, and the first source
/// that holds any setting. A registry value's text is read into `reader`
/// under the path that stands for it.
fn read_settings(
    definitions: &[AgentDefinition],
    ports: &Ports,
    reader: &mut Reader,
) -> Vec<ScannedSettings> {
    let mut scanned = Vec::new();
    for (agent_index, definition) in definitions.iter().enumerate() {
        for settings in &definition.settings {
            let sources: Vec<Vec<PathBuf>> = settings
                .sources
                .iter()
                .map(|source| match source {
                    SettingsSource::RegistryValue(block) => {
                        let path = block.path();
                        let text = ports
                            .env
                            .registry_value(block.hive(), &block.key, &block.value);
                        reader.files.insert(path.clone(), Ok(text));
                        vec![path]
                    }
                    SettingsSource::RegistryKey(block) => {
                        let path = block.path();
                        let text = block.text(ports.env.as_ref());
                        reader.files.insert(path.clone(), Ok(text));
                        vec![path]
                    }
                    SettingsSource::Files(block) => {
                        let env = ports.env.as_ref();
                        let mut files: Vec<PathBuf> =
                            paths::expand(&block.path, env).into_iter().collect();
                        let drop_ins = block
                            .drop_ins
                            .as_deref()
                            .and_then(|folder| paths::expand(folder, env));
                        let mut found: Vec<PathBuf> = drop_ins
                            .and_then(|folder| reader.fs.read_dir(&folder).ok())
                            .unwrap_or_default()
                            .into_iter()
                            .filter(|path| {
                                path.extension()
                                    .is_some_and(|extension| extension.eq_ignore_ascii_case("json"))
                            })
                            .collect();
                        found.sort_by_key(|path| paths::key(path));
                        files.extend(found);
                        files
                    }
                })
                .collect();
            // A file that cannot be read or parsed may hold settings too.
            let holds = |path: &PathBuf, reader: &mut Reader| match reader.peek(path) {
                Ok(None) => false,
                Ok(Some(text)) => JsonFormat::STRICT.parse(text).map_or(true, |document| {
                    document.as_object().is_none_or(|object| !object.is_empty())
                }),
                Err(_) => true,
            };
            let selected = sources
                .iter()
                .position(|files| files.iter().any(|path| holds(path, reader)));
            scanned.push(ScannedSettings {
                agent_index,
                id: settings.id.clone(),
                sources,
                selected,
            });
        }
    }
    scanned
}

/// The machine files that each Agent with a found home reads only while
/// an administrator owns them, and that exist but someone else can change,
/// by the Agent's index.
fn find_not_admin_owned(
    definitions: &[AgentDefinition],
    found: &[FoundHome],
    ports: &Ports,
    fs: &dyn FileSystem,
) -> NotAdminOwned {
    let agents: BTreeSet<usize> = found.iter().map(|home| home.agent_index).collect();
    let files = agents.into_iter().flat_map(|agent_index| {
        definitions[agent_index]
            .admin_owned_files(ports.env.as_ref())
            .into_iter()
            .filter(|path| fs.exists(path) && !fs.admin_owned(path))
            .map(move |path| (agent_index, path))
    });
    NotAdminOwned(files.collect())
}

/// The files each Agent, by its index, skips without reading them, since
/// someone other than an administrator can change them.
#[derive(Clone, Default)]
pub(crate) struct NotAdminOwned(BTreeSet<(usize, PathBuf)>);

impl NotAdminOwned {
    /// Whether the Agent at `agent_index` skips the file at `path`.
    pub fn holds(&self, agent_index: usize, path: &Path) -> bool {
        self.0
            .iter()
            .any(|(agent, skipped)| *agent == agent_index && skipped == path)
    }
}

/// The file of each managed-exclusive gate of each found Agent home that
/// exists, whatever it holds.
fn exclusive_files(
    definitions: &[AgentDefinition],
    found: &[FoundHome],
    fs: &dyn FileSystem,
) -> BTreeSet<PathBuf> {
    let mut files = BTreeSet::new();
    for home in found {
        let definition = &definitions[home.agent_index];
        for rules in definition.items.values() {
            for gate in &rules.gates {
                let Gate::ManagedExclusive(block) = gate else {
                    continue;
                };
                let location = definition.location(&block.location);
                files.extend(
                    location
                        .anchor
                        .path(&home.roots, None)
                        .filter(|path| fs.exists(path)),
                );
            }
        }
    }
    files
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
            // A gate the Agent caches nowhere is read through its `when`.
            let Some(cache) = &gate.lookup else {
                continue;
            };
            let path = cache
                .anchor()
                .path(&home.roots, None)
                .expect("validation puts a lookup in the Agent home");
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
                        .find_map(|map| map.get(name)?.as_str())
                        .map(str::to_owned)
                        .or_else(|| ports.env.var(name))
                };
                bypass.bypassed_by(&var)
            });
            let reading = match bypassed {
                Some(variable) => GateReading::Bypassed(variable),
                None => {
                    let text = match cache.store() {
                        GateStore::JsonFile => reader.peek(&path).ok().flatten().map(str::to_owned),
                        GateStore::SqliteItem(key) => {
                            reader.fs.read_sqlite_item(&path, key).ok().flatten()
                        }
                    };
                    text.and_then(|text| JsonFormat::STRICT.parse(&text).ok())
                        .and_then(|document| cache.cached(&document, &gate.name))
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
    storage: &[StorageFolder],
    installed: &Installed,
    entries: &[ScannedEntry],
    reader: &mut Reader,
) -> (Vec<ScannedSwitch>, BTreeSet<PathBuf>) {
    let mut switches = Vec::new();
    let mut unreadable = BTreeSet::new();
    for home in found {
        let definition = &definitions[home.agent_index];
        for (&kind, rules) in &definition.items {
            for (mechanism, block) in rules.disable.iter().enumerate() {
                let Some(switched) = block.switched() else {
                    continue;
                };
                let switch = switched.switch();
                let places = Places {
                    home,
                    projects,
                    storage,
                    settings: &[],
                    plugins: plugins_of(installed, home),
                    entries: covered(definition, entries, home, kind, block),
                };
                for place in places.read(
                    &switch.anchor,
                    &switch.key_path,
                    switch.format().as_ref(),
                    switch.sqlite(),
                    None,
                    None,
                    reader,
                    &mut unreadable,
                ) {
                    switches.push(ScannedSwitch {
                        agent_index: home.agent_index,
                        home: home.home.clone(),
                        kind,
                        mechanism,
                        path: place.path,
                        project: place.project,
                        file_of: place.file_of,
                        value: place.value,
                        key_path: place.key_path,
                        named_by: switched.named_by().map(str::to_owned),
                        later_wins: switch.later_wins,
                        ignore_case: switched.ignores_case(),
                        store_class: switch.store_class,
                    });
                }
            }
        }
    }
    (switches, unreadable)
}

/// Every value each rule and gate of each found Agent home reads, at every
/// place its sources name, as [`read_switches`] reads switches. A file that
/// cannot be read is listed apart and has no places.
fn read_rules(
    definitions: &[AgentDefinition],
    found: &[FoundHome],
    projects: &[ScannedProject],
    storage: &[StorageFolder],
    installed: &Installed,
    settings: &[ScannedSettings],
    reader: &mut Reader,
) -> (Vec<ScannedSetting>, BTreeSet<PathBuf>) {
    let mut values = Vec::new();
    let mut unreadable = BTreeSet::new();
    for home in found {
        let definition = &definitions[home.agent_index];
        for (&kind, rules) in &definition.items {
            let rule_sources = rules.rules.iter().enumerate().flat_map(|(rule, block)| {
                block
                    .settings()
                    .enumerate()
                    .map(move |(source, setting)| (SettingOwner::Rule { rule, source }, setting))
            });
            let gate_sources = rules.gates.iter().enumerate().flat_map(|(gate, block)| {
                block
                    .settings()
                    .enumerate()
                    .map(move |(source, setting)| (SettingOwner::Gate { gate, source }, setting))
            });
            let competition_sources = rules
                .competition
                .only_while()
                .into_iter()
                .flat_map(|setting| setting.sources.iter().enumerate())
                .map(|(source, setting)| (SettingOwner::Competition { source }, setting));
            let condition_sources =
                definition
                    .locations
                    .iter()
                    .enumerate()
                    .filter(|(_, location)| location.kind == kind)
                    .flat_map(|(location, block)| {
                        block
                            .when
                            .iter()
                            .enumerate()
                            .flat_map(move |(condition, block)| {
                                block.setting().into_iter().flat_map(move |setting| {
                                    setting.sources.iter().enumerate().map(
                                        move |(source, setting)| {
                                            let owner = SettingOwner::Condition {
                                                location,
                                                condition,
                                                source,
                                            };
                                            (owner, setting)
                                        },
                                    )
                                })
                            })
                    });
            let feature_gate_sources =
                definition
                    .feature_gates
                    .iter()
                    .enumerate()
                    .flat_map(|(gate, block)| {
                        block.when.iter().flat_map(move |when| {
                            when.sources
                                .iter()
                                .enumerate()
                                .map(move |(source, setting)| {
                                    (SettingOwner::FeatureGate { gate, source }, setting)
                                })
                        })
                    });
            let sources = rule_sources
                .chain(gate_sources)
                .chain(competition_sources)
                .chain(condition_sources)
                .chain(feature_gate_sources);
            for (owner, setting) in sources {
                let places = Places {
                    home,
                    projects,
                    storage,
                    settings,
                    plugins: plugins_of(installed, home),
                    entries: Vec::new(),
                };
                let format = setting.format();
                for place in places.read(
                    &setting.anchor,
                    &setting.key_path,
                    format.as_ref(),
                    setting.sqlite,
                    setting.table.as_deref(),
                    setting.cached_at.as_deref(),
                    reader,
                    &mut unreadable,
                ) {
                    values.push(ScannedSetting {
                        agent_index: home.agent_index,
                        home: home.home.clone(),
                        kind,
                        owner,
                        path: place.path,
                        project: place.project,
                        file_of: place.file_of,
                        key_path: place.key_path,
                        value: place.value.and_then(|held| setting.read_as(held)),
                        cached_at: place.cached_at,
                    });
                }
            }
        }
    }
    (values, unreadable)
}

/// One place a value is read, for one Agent home and, for a value that
/// applies per Project, one Project.
struct Place {
    path: PathBuf,
    /// The key of the Project it applies in, or `None` when it applies in every context.
    project: Option<String>,
    /// For a file per plugin or per Entry, which one it is the file of.
    file_of: Option<Owner>,
    /// Its key path, `{project}` replaced by the Project key in the
    /// Agent's own spelling, `{name}` left in place.
    key_path: Vec<String>,
    /// What the file holds at the key path, up to `{name}` when it has one.
    value: Option<serde_json::Value>,
    /// When the Agent cached the value, where the setting says.
    cached_at: Option<SystemTime>,
}

/// Where the values an anchor and a key path name are, for one Agent home.
struct Places<'a> {
    home: &'a FoundHome,
    projects: &'a [ScannedProject],
    /// The folders the Agent homes keep for each Project.
    storage: &'a [StorageFolder],
    /// The settings a settings anchor reads.
    settings: &'a [ScannedSettings],
    /// The plugins of the home, each with a file of an anchor per plugin.
    plugins: &'a [ScannedPlugin],
    /// The Entries of the home, each with a file of an anchor per Entry.
    entries: Vec<&'a ScannedEntry>,
}

/// The plugins each found Agent home loads, by Agent index and home id.
type Installed = BTreeMap<(usize, String), Vec<ScannedPlugin>>;

/// The Entries among `entries` of `home`, of the Item kind `kind`, that
/// `mechanism` covers.
fn covered<'e>(
    definition: &AgentDefinition,
    entries: &'e [ScannedEntry],
    home: &FoundHome,
    kind: ItemKind,
    mechanism: &DisableMechanism,
) -> Vec<&'e ScannedEntry> {
    entries
        .iter()
        .filter(|entry| {
            entry.agent_index == home.agent_index
                && entry.home == home.home
                && definition.location(&entry.location).kind == kind
                && mechanism.covers(&entry.location)
        })
        .collect()
}

/// The plugins `home` loads.
fn plugins_of<'i>(installed: &'i Installed, home: &FoundHome) -> &'i [ScannedPlugin] {
    installed
        .get(&(home.agent_index, home.home.clone()))
        .map_or(&[], Vec::as_slice)
}

impl Places<'_> {
    /// Every place `anchor` and `key_path` name: in each existing Project
    /// folder for an anchor in a Project, in each file of the source the
    /// Agent reads for a settings anchor, in the file of each plugin for an
    /// anchor per plugin, under each spelling of each Project key for a key
    /// path holding `{project}`, and once otherwise. A file that is missing
    /// is read as empty; one that cannot be read or parsed as JSON, or that
    /// is a Read failure for the Agent home, goes into `unreadable` and has
    /// no places. With `sqlite`, the file is a SQLite store, read as an
    /// object of the key path's first key and the JSON its `ItemTable` holds
    /// there; a missing store has no places. With `table` too, the object
    /// holds the list of the texts of that column of that table instead.
    /// With `cached_at`, each place says when the Agent cached the value,
    /// read at that key path.
    #[allow(clippy::too_many_arguments)]
    fn read(
        &self,
        anchor: &Anchor,
        key_path: &[String],
        format: &dyn Parse,
        sqlite: bool,
        table: Option<&str>,
        cached_at: Option<&[String]>,
        reader: &mut Reader,
        unreadable: &mut BTreeSet<PathBuf>,
    ) -> Vec<Place> {
        let home = self.home;
        let files: Vec<(PathBuf, Option<String>, Option<Owner>)> =
            if let Some(id) = anchor.settings() {
                // A later file of the source overrides an earlier one, so it
                // comes first.
                settings_named(self.settings, home.agent_index, id)
                    .into_iter()
                    .flat_map(|known| {
                        known
                            .read()
                            .iter()
                            .rev()
                            .map(|path| (path.clone(), None, None))
                    })
                    .collect()
            } else if let Some(beside) = anchor.entry() {
                let mut files: Vec<(PathBuf, Option<String>, Option<Owner>)> = Vec::new();
                for entry in &self.entries {
                    let Some(path) = beside.of(&entry.path) else {
                        continue;
                    };
                    let file_of = Some(Owner::Entry(entry.path.clone()));
                    if !files.iter().any(|(_, _, known)| *known == file_of) {
                        files.push((path, entry.project.clone(), file_of));
                    }
                }
                files
            } else if anchor.per_plugin() {
                self.plugins
                    .iter()
                    .filter_map(|plugin| {
                        let path = anchor.plugin_path(&home.roots, &plugin.id)?;
                        Some((
                            path,
                            plugin.project.clone(),
                            Some(Owner::Plugin(plugin.id.clone())),
                        ))
                    })
                    .collect()
            } else if let Some(relative) = anchor.project_storage() {
                storage_paths(self.storage, home.agent_index, &home.home, relative)
                    .map(|(path, project)| (path, Some(project.to_owned()), None))
                    .collect()
            } else if anchor.project_file() {
                self.projects
                    .iter()
                    .filter(|project| project.exists && project.workspace_file)
                    .map(|project| (project.path.clone(), Some(project.key.clone()), None))
                    .collect()
            } else if anchor.in_project() {
                self.projects
                    .iter()
                    .filter(|project| project.is_folder())
                    .filter_map(|project| {
                        let path = anchor.path(&home.roots, Some(&project.path))?;
                        Some((path, Some(project.key.clone()), None))
                    })
                    .collect()
            } else {
                anchor
                    .path(&home.roots, None)
                    .map(|path| (path, None, None))
                    .into_iter()
                    .collect()
            };
        let mut places = Vec::new();
        for (path, project, file_of) in files {
            // A file the Agent home cannot read, even one that is valid
            // JSON, sets nothing.
            let failed = reader.failures.iter().any(|failure| {
                failure.agent_index == home.agent_index
                    && failure.home == home.home
                    && failure.path == path
            });
            if failed {
                unreadable.insert(path);
                continue;
            }
            // A file the Agent skips unread holds nothing for it.
            let skipped = reader.not_admin_owned.holds(home.agent_index, &path);
            let read = if skipped {
                Ok(None)
            } else if sqlite {
                if !reader.fs.exists(&path) {
                    continue;
                }
                match table {
                    Some(table) => reader.peek_column(&path, table, &key_path[0]),
                    None => reader.peek_item(&path, &key_path[0]),
                }
            } else {
                reader.peek(&path).map(|text| text.map(str::to_owned))
            };
            let document = match read {
                Ok(None) => None,
                Ok(Some(text)) => match format.parse(&text) {
                    Ok(document) if sqlite => Some(serde_json::json!({ &key_path[0]: document })),
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
            let cached = cached_at
                .zip(document.as_ref())
                .and_then(|(key_path, document)| lookup(document, key_path))
                .and_then(blocks::time_of);
            let place = |project: Option<String>, key_path: Vec<String>| {
                let up_to_name = key_path
                    .iter()
                    .position(|key| is_entry_key(key))
                    .unwrap_or(key_path.len());
                Place {
                    path: path.clone(),
                    project,
                    file_of: file_of.clone(),
                    cached_at: cached,
                    value: document
                        .as_ref()
                        .and_then(|document| lookup(document, &key_path[..up_to_name]))
                        .cloned(),
                    key_path,
                }
            };
            // A key path keyed by plugin and by name, such as Codex's
            // `plugins.<id>.mcp_servers.<name>`, has a place per plugin,
            // its id put in.
            if key_path.iter().any(|key| key == PLUGIN_KEY)
                && key_path.iter().any(|key| key == NAME_KEY)
            {
                for plugin in self.plugins {
                    let key_path = key_path
                        .iter()
                        .map(|key| if key == PLUGIN_KEY { &plugin.id } else { key })
                        .cloned()
                        .collect();
                    places.push(Place {
                        file_of: Some(Owner::Plugin(plugin.id.clone())),
                        ..place(plugin.project.clone(), key_path)
                    });
                }
                continue;
            }
            let Some(at) = key_path.iter().position(|key| key == PROJECT_KEY) else {
                places.push(place(project, key_path.to_vec()));
                continue;
            };
            // Each key there is a Project in the Agent's own spelling.
            let keyed = document
                .as_ref()
                .and_then(|document| lookup(document, &key_path[..at]))
                .and_then(serde_json::Value::as_object);
            for spelling in keyed.into_iter().flat_map(serde_json::Map::keys) {
                let Some(project) = project_of(self.projects, spelling) else {
                    continue;
                };
                let mut key_path = key_path.to_vec();
                key_path[at] = spelling.clone();
                places.push(place(Some(project), key_path));
            }
        }
        places
    }
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
    installed: &Installed,
    entries: &[ScannedEntry],
) -> BTreeSet<PathBuf> {
    let mut files = BTreeSet::new();
    for home in found {
        let definition = &definitions[home.agent_index];
        for (&kind, rules) in &definition.items {
            for mechanism in &rules.disable {
                let Some(switched) = mechanism.switched() else {
                    continue;
                };
                let anchor = &switched.switch().anchor;
                if let Some(beside) = anchor.entry() {
                    files.extend(
                        covered(definition, entries, home, kind, mechanism)
                            .iter()
                            .filter_map(|entry| beside.of(&entry.path)),
                    );
                    continue;
                }
                if anchor.per_plugin() {
                    files.extend(
                        plugins_of(installed, home)
                            .iter()
                            .filter_map(|plugin| anchor.plugin_path(&home.roots, &plugin.id)),
                    );
                    continue;
                }
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
        // Each home with its folders, each folder with the home's id there:
        // a home whose glob matches several folders has one per folder,
        // each with its folder as its id.
        let mut located: Vec<(&HomeDefinition, Vec<HomeFolder>)> = Vec::new();
        for home in &definition.homes {
            let folders = home.folder.folders(ports.env.as_ref(), fs);
            if folders.is_empty() && home.folder.in_home_folder() {
                return Err(ScanError::HomeUnknown {
                    agent: definition.agent.name.clone(),
                    home: home.id.clone(),
                });
            }
            let several = folders.len() > 1;
            let folders = folders
                .into_iter()
                .map(|folder| {
                    let id = if several {
                        folder.display().to_string()
                    } else {
                        home.id.clone()
                    };
                    (id, folder)
                })
                .collect();
            located.push((home, folders));
        }
        // What each lookup order picks.
        let picked: Vec<(&Vec<String>, Option<HomeFolder>)> = definition
            .lookup_orders
            .iter()
            .map(|order| (order, pick(order, &located, ports)))
            .collect();
        // Roots are one Agent home however many homes of the definition find
        // them: a mirrored home's, else the first home's that finds them.
        let mirrors = mirrored(definition, ports);
        let mut taken: Vec<(String, String)> = mirrors
            .iter()
            .map(|mirror| (roots_key(fs, &mirror.roots), mirror.home.clone()))
            .collect();
        // Each home not listed for its roots, with the home listed instead.
        let mut same: Vec<(String, String)> = Vec::new();
        let start = found.len();
        for (home, folders) in located {
            for (id, folder) in folders {
                let key = paths::canonical_key(fs, &folder);
                definition_folders.push((agent_index, key.clone()));
                let same_roots = roots_key(fs, &home.roots_in(&folder));
                if let Some((_, listed)) = taken.iter().find(|(known, _)| *known == same_roots) {
                    same.push((id, listed.clone()));
                    continue;
                }
                // Skipped when an order holds it and none picks its folder,
                // for what the first order that holds it picks.
                let picks_it = picked.iter().any(|(_, pick)| {
                    pick.as_ref()
                        .is_some_and(|(_, pick)| paths::canonical_key(fs, pick) == key)
                });
                let skipped = picked
                    .iter()
                    .find(|(order, _)| order.contains(&home.id))
                    .filter(|_| !picks_it)
                    .map(|(_, pick)| Skipped {
                        instead: pick.as_ref().map(|(id, _)| id.clone()),
                        folder: pick.as_ref().map(|(_, folder)| folder.clone()),
                    });
                let mut roots = home.roots_in(&folder);
                // A definition home the user added keeps its id, so that it is
                // the same home before and after its files appear.
                let configured = home.configured(&roots, fs);
                let source = if configured {
                    HomeSource::of(home)
                } else if added_to(&definition.agent.id, &folder) {
                    HomeSource::Added
                } else {
                    tracing::debug!(agent = %definition.agent.id, home = %id, "home not found");
                    continue;
                };
                tracing::debug!(agent = %definition.agent.id, home = %id, "home found");
                roots.extend(machine_paths(definition, ports));
                taken.push((same_roots, id.clone()));
                found.push(FoundHome {
                    agent_index,
                    home: id,
                    declared: Some(home.id.clone()),
                    folder,
                    roots,
                    source,
                    configured,
                    skipped,
                    mirror: None,
                });
            }
        }
        for mirror in mirrors {
            tracing::debug!(agent = %definition.agent.id, home = %mirror.home, "mirrored home found");
            let mut roots = mirror.roots;
            roots.extend(machine_paths(definition, ports));
            found.push(FoundHome {
                agent_index,
                declared: Some(mirror.home.clone()),
                home: mirror.home,
                folder: mirror.folder,
                roots,
                source: HomeSource::KnownLocation,
                configured: true,
                skipped: None,
                mirror: Some(mirror.mirror),
            });
        }
        // The home an order picks is named only when it is listed, as
        // itself or as the home listed for its folder.
        let listed: Vec<String> = found[start..]
            .iter()
            .map(|home| home.home.clone())
            .collect();
        for skipped in found[start..]
            .iter_mut()
            .filter_map(|home| home.skipped.as_mut())
        {
            if let Some((_, listed)) = same
                .iter()
                .find(|(id, _)| skipped.instead.as_ref() == Some(id))
            {
                skipped.instead = Some(listed.clone());
            }
            if skipped
                .instead
                .as_ref()
                .is_some_and(|id| !listed.contains(id))
            {
                skipped.instead = None;
            }
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
        let configured = definition.homes[0].configured(&roots, fs);
        roots.extend(machine_paths(definition, ports));
        found.push(FoundHome {
            agent_index,
            home: added.folder.display().to_string(),
            folder: added.folder.clone(),
            roots,
            source: HomeSource::Added,
            configured,
            declared: None,
            skipped: None,
            mirror: None,
        });
    }
    Ok(found)
}

/// A folder of a home, with the home's id there.
type HomeFolder = (String, PathBuf);

/// A mirrored home of a definition found on this machine.
struct FoundMirror {
    home: String,
    folder: PathBuf,
    /// Its roots by name, laid out like its source home's.
    roots: BTreeMap<String, PathBuf>,
    mirror: Mirror,
}

/// What two homes compare by: the real path of each of their roots, by name.
fn roots_key(fs: &dyn FileSystem, roots: &BTreeMap<String, PathBuf>) -> String {
    let keys: Vec<String> = roots
        .iter()
        .map(|(name, path)| format!("{name}={}", paths::canonical_key(fs, path)))
        .collect();
    keys.join("|")
}

/// The mirrored homes of `definition` whose folders hold any of their
/// roots, each set of roots once. A glob that matches several folders gives one
/// home per folder, with its folder as its id.
fn mirrored(definition: &AgentDefinition, ports: &Ports) -> Vec<FoundMirror> {
    let fs = ports.fs.as_ref();
    let mut found: Vec<FoundMirror> = Vec::new();
    for mirror in &definition.mirrored_homes {
        let source = definition
            .homes
            .iter()
            .find(|home| home.id == mirror.source)
            .expect("validation declares a mirrored home's source");
        let source_folder = source
            .folder
            .folders(ports.env.as_ref(), fs)
            .into_iter()
            .next();
        let folders = mirror.folder.folders(ports.env.as_ref(), fs);
        let several = folders.len() > 1;
        for folder in folders {
            let roots = source.roots_in(&folder);
            let key = roots_key(fs, &roots);
            let known = found.iter().any(|other| roots_key(fs, &other.roots) == key);
            if known || !source.configured(&roots, fs) {
                continue;
            }
            found.push(FoundMirror {
                home: if several {
                    folder.display().to_string()
                } else {
                    mirror.id.clone()
                },
                folder,
                roots,
                mirror: Mirror {
                    owner: mirror.owner.clone(),
                    source: mirror.source.clone(),
                    source_folder: source_folder.clone(),
                },
            });
        }
    }
    found
}

/// The id and folder of the home `order` picks among the `located` folders
/// of the homes of a definition: the first whose conditions hold and whose
/// folder exists, or that the Agent uses before it exists.
fn pick(
    order: &[String],
    located: &[(&HomeDefinition, Vec<HomeFolder>)],
    ports: &Ports,
) -> Option<HomeFolder> {
    order.iter().find_map(|id| {
        let (home, folders) = located.iter().find(|(home, _)| home.id == *id)?;
        if !home
            .when
            .iter()
            .all(|condition| condition.holds(ports.env.as_ref()))
        {
            return None;
        }
        folders
            .iter()
            .find(|(_, folder)| home.folder.used_before_it_exists() || ports.fs.exists(folder))
            .cloned()
    })
}

/// The machine-wide paths a definition's locations, plugin and project
/// registries and Feature gate env maps name, each by the text that names it, as far as the environment
/// says where they are.
fn machine_paths(definition: &AgentDefinition, ports: &Ports) -> BTreeMap<String, PathBuf> {
    let env_maps = definition
        .feature_gates
        .iter()
        .filter_map(|gate| gate.bypass.as_ref())
        .flat_map(|bypass| bypass.env_maps())
        .map(|map| &map.anchor);
    let plugins = definition
        .plugins
        .iter()
        .flat_map(|plugins| plugins.registry.anchors());
    let registries = definition
        .projects
        .registries
        .iter()
        .map(|registry| registry.anchor());
    // What rules, gates, switches and conditions read can be on the
    // machine too, such as a store every profile of VS Code shares.
    let settings = definition.items.values().flat_map(|rules| {
        let read = rules
            .rules
            .iter()
            .flat_map(|rule| rule.settings())
            .chain(rules.gates.iter().flat_map(|gate| gate.settings()))
            .chain(
                rules
                    .competition
                    .only_while()
                    .into_iter()
                    .flat_map(|setting| &setting.sources),
            )
            .map(|setting| &setting.anchor);
        let switches = rules
            .disable
            .iter()
            .filter_map(|mechanism| Some(&mechanism.switch()?.anchor));
        read.chain(switches)
    });
    let conditions = definition
        .locations
        .iter()
        .flat_map(|location| &location.when)
        .filter_map(|condition| condition.setting())
        .flat_map(|setting| &setting.sources)
        .chain(
            definition
                .feature_gates
                .iter()
                .filter_map(|gate| gate.when.as_ref())
                .flat_map(|when| &when.sources),
        )
        .map(|setting| &setting.anchor);
    definition
        .locations
        .iter()
        .map(|location| &location.anchor)
        .chain(env_maps)
        .chain(plugins)
        .chain(registries)
        .chain(settings)
        .chain(conditions)
        .filter_map(|anchor| anchor.machine())
        .filter_map(|text| {
            let path = definition.machine_path(text, ports.env.as_ref())?;
            Some((text.to_owned(), path))
        })
        .collect()
}

/// How many folders up one member folder of a workspace file is read at
/// most, so that every location of the first member ranks above any of the
/// second.
const MEMBER_RANKS: usize = 1 << 16;

/// The member folders of the `.code-workspace` file at `file`, in its
/// order: each item of its `folders` names one by a `path`, relative to the
/// file's folder or absolute, or by a `file:` `uri`. A file that cannot be
/// read has none; the location reading it reports why.
fn members(reader: &mut Reader, file: &Path) -> Vec<PathBuf> {
    let Some(folder) = file.parent() else {
        return Vec::new();
    };
    let jsonc = JsonFormat {
        dialect: JsonDialect::Jsonc,
    };
    let document = match reader.peek(file) {
        Ok(Some(text)) => jsonc.parse(text).ok(),
        _ => None,
    };
    let items = document
        .as_ref()
        .and_then(|document| document.get("folders"))
        .and_then(serde_json::Value::as_array);
    items
        .into_iter()
        .flatten()
        .filter_map(|item| {
            if let Some(path) = item.get("path").and_then(serde_json::Value::as_str) {
                let path = paths::lexical(&folder.join(path));
                return paths::is_absolute(&path).then_some(path);
            }
            paths::from_file_uri(item.get("uri")?.as_str()?)
        })
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

/// The root of the git repository the folder `folder` is in, the folder at
/// or above it that holds `.git`, and the repository's main checkout: the
/// root itself, or for a linked worktree, whose `.git` is a file naming
/// `<main>\.git\worktrees\<name>`, that `<main>`.
fn repository(fs: &dyn FileSystem, folder: &Path) -> (Option<PathBuf>, Option<PathBuf>) {
    let Some(root) = folder
        .ancestors()
        .find(|parent| fs.exists(&parent.join(".git")))
    else {
        return (None, None);
    };
    // A `.git` folder cannot be read as a file.
    let main = fs
        .read(&root.join(".git"))
        .ok()
        .and_then(|bytes| String::from_utf8(bytes).ok())
        .and_then(|text| {
            let gitdir = text.lines().find_map(|line| line.strip_prefix("gitdir:"))?;
            let gitdir = paths::lexical(&root.join(gitdir.trim()));
            let worktrees = gitdir.parent()?;
            let dot_git = worktrees.parent()?;
            let named = |path: &Path, name: &str| {
                path.file_name()
                    .is_some_and(|known| known.eq_ignore_ascii_case(name))
            };
            (named(worktrees, "worktrees") && named(dot_git, ".git"))
                .then(|| dot_git.parent().map(Path::to_path_buf))
                .flatten()
        });
    let root = root.to_path_buf();
    (Some(root.clone()), Some(main.unwrap_or(root)))
}

/// The `SKILL.md` of the skill Entry `entry` at its real path: through the
/// link a Linked Entry is, when it leads somewhere.
fn skill_file(entry: &ScannedEntry) -> PathBuf {
    let folder = entry
        .link
        .as_ref()
        .and_then(|link| link.real.as_ref())
        .unwrap_or(&entry.path);
    folder.join("SKILL.md")
}

/// The id `template`, a location's `switch_id`, makes for `entry`, read
/// at `root`.
fn switch_id(template: &str, entry: &ScannedEntry, root: &[String]) -> String {
    // The key `*` stands for in the root, such as `<extension>/<provider>`.
    let extension = root
        .iter()
        .position(|key| key == "*")
        .and_then(|at| entry.key_path.get(at))
        .map(|key| key.split('/').next().unwrap_or_default().to_lowercase())
        .unwrap_or_default();
    let plugin_uri = entry
        .plugin
        .as_ref()
        .map(|plugin| paths::to_file_uri(&plugin.folder))
        .unwrap_or_default();
    template
        .replace(ENTRY_NAME, &entry.name)
        .replace(FOLDER_INDEX, &(entry.depth / MEMBER_RANKS).to_string())
        .replace("{uri_path}", &paths::uri_path(&entry.path))
        .replace("{skill_file}", &skill_file(entry).display().to_string())
        .replace("{plugin_uri}", &plugin_uri)
        .replace("{extension}", &extension)
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
    /// The indices in `entries` of the Entries whose `config` is still their
    /// real value: they are masked once every location is read, when every
    /// location that reads their file is known.
    unmasked: Vec<usize>,
    readers: Vec<LocationReader>,
    /// The folder an Agent home keeps for each Project, as its registry
    /// of such folders records it.
    storage: Vec<StorageFolder>,
    /// Each file read for a location that is a copy the Agent caches, with
    /// when it cached it, when the file says.
    cached: BTreeMap<PathBuf, Option<SystemTime>>,
}

/// The path `relative` names in each folder the Agent home `home` of the
/// Agent at `agent_index` keeps for a Project, with that Project's key.
fn storage_paths<'s>(
    storage: &'s [StorageFolder],
    agent_index: usize,
    home: &'s str,
    relative: &'s str,
) -> impl Iterator<Item = (PathBuf, &'s str)> + 's {
    storage
        .iter()
        .filter(move |storage| storage.agent_index == agent_index && storage.home == home)
        .map(|storage| {
            let path = blocks::join_relative(&storage.folder, relative);
            (path, storage.project.as_str())
        })
}

/// The folder an Agent home keeps what it stores for one Project in, such as
/// VS Code's `workspaceStorage\<id>`.
#[derive(Clone)]
pub(crate) struct StorageFolder {
    pub agent_index: usize,
    pub home: String,
    pub folder: PathBuf,
    /// The Project's key.
    pub project: String,
}

impl Scanner<'_> {
    /// Adds the Projects an Agent home's registries record.
    fn read_registries(&mut self, home: &FoundHome, definition: &AgentDefinition) {
        for registry in &definition.projects.registries {
            let Some(path) = registry.anchor().path(&home.roots, None) else {
                continue;
            };
            let spelling = registry.spelling();
            match registry {
                Registry::JsonKeys(block) => {
                    let Some(text) = self.reader.text(home, &path, None) else {
                        continue;
                    };
                    match block.spellings(text) {
                        Ok(spellings) => {
                            for text in spellings {
                                self.add_spelling(home, spelling, &text);
                            }
                        }
                        Err(error) => self.reader.fail(
                            home,
                            &path,
                            None,
                            ReadFailureKind::Parse,
                            error.position,
                        ),
                    }
                }
                Registry::SqliteList(block) => {
                    let Some(text) = self.reader.sqlite_text(home, &path, &block.key, None) else {
                        continue;
                    };
                    // The store holds much else, which a list it cannot parse
                    // leaves readable.
                    let Ok(spellings) = block.spellings(&text) else {
                        tracing::debug!(path = %path.display(), key = %block.key, "cannot parse the list");
                        continue;
                    };
                    for text in spellings {
                        self.add_spelling(home, spelling, &text);
                    }
                }
                Registry::TomlKeys(block) => {
                    let Some(text) = self.reader.text(home, &path, None) else {
                        continue;
                    };
                    match block.spellings(text) {
                        Ok(spellings) => {
                            for text in spellings {
                                self.add_spelling(home, spelling, &text);
                            }
                        }
                        Err(error) => self.reader.fail(
                            home,
                            &path,
                            None,
                            ReadFailureKind::Parse,
                            error.position,
                        ),
                    }
                }
                // Session files are many and large, and hold nothing else
                // config-digger reads: one that cannot be read records no
                // Project, and is no Read failure.
                Registry::JsonlHeads(block) => {
                    let fs = self.reader.fs;
                    for file in paths::expand_relative(fs, &path, &block.files) {
                        match block.spelling_in(fs, &file) {
                            Some(text) => {
                                self.add_spelling(home, spelling, &text);
                            }
                            None => {
                                tracing::debug!(path = %file.display(), "names no Project");
                            }
                        }
                    }
                }
                // The store holds much else, which a table it lacks, as in
                // another version of the Agent, leaves readable.
                Registry::SqliteColumn(block) => {
                    let fs = self.reader.fs;
                    let values = match fs.read_sqlite_column(&path, &block.table, &block.column) {
                        Ok(values) => values,
                        Err(error) => {
                            tracing::debug!(path = %path.display(), table = %block.table, %error, "cannot read the column");
                            continue;
                        }
                    };
                    for text in values {
                        self.add_spelling(home, spelling, &text);
                    }
                }
                Registry::JsonFolders(block) => {
                    let mut folders = self.reader.fs.read_dir(&path).unwrap_or_default();
                    folders.sort_by_key(|folder| paths::key(folder));
                    for folder in folders {
                        let file = blocks::join_relative(&folder, &block.file);
                        let Some(text) = self.reader.text(home, &file, None) else {
                            continue;
                        };
                        match block.spelling_in(text) {
                            Ok(Some(text)) => {
                                if let Some(project) = self.add_spelling(home, spelling, &text) {
                                    self.storage.push(StorageFolder {
                                        agent_index: home.agent_index,
                                        home: home.home.clone(),
                                        folder,
                                        project,
                                    });
                                }
                            }
                            Ok(None) => {}
                            Err(error) => self.reader.fail(
                                home,
                                &file,
                                None,
                                ReadFailureKind::Parse,
                                error.position,
                            ),
                        }
                    }
                }
            }
        }
    }

    /// Records the Project a registry of `home` spells `text`, and returns
    /// its key; `None` when it names none on this machine.
    fn add_spelling(&mut self, home: &FoundHome, spelling: Spelling, text: &str) -> Option<String> {
        let path = spelling.path(text)?;
        self.projects.add(&path, Some(home.agent_index))
    }

    /// Adds the Entries of every location in plugins, in the folder of each
    /// of `plugins` it reads, which apply where the plugin does.
    fn read_plugin_locations(
        &mut self,
        home: &FoundHome,
        definition: &AgentDefinition,
        plugins: &[ScannedPlugin],
    ) {
        for location in definition.locations.iter() {
            let Some(anchor) = location.anchor.plugin() else {
                continue;
            };
            for plugin in plugins
                .iter()
                .filter(|plugin| anchor.plugins.names(&plugin.plugins))
            {
                self.read_plugin_location(home, definition, location, plugin, &anchor.path);
            }
        }
    }

    /// The plugins of `plugins` that the Agent home installed, each record
    /// once, leaving out those whose folder carries the superseded marker.
    /// The Project a record applies in is recorded as a Project. A registry
    /// in a Project is read in `folder`, a Project folder and its key.
    fn read_plugins(
        &mut self,
        home: &FoundHome,
        plugins: &PluginsDefinition,
        folder: Option<(&Path, &str)>,
    ) -> Vec<ScannedPlugin> {
        let records = match &plugins.registry {
            PluginRegistry::JsonPluginRecords(block) => self.plugin_records(home, block),
            PluginRegistry::PluginFolders(block) => self.plugin_folders(home, block),
            PluginRegistry::PluginCache(block) => self.plugin_cache(home, block),
            PluginRegistry::ProjectPluginKeys(block) => match folder {
                Some((folder, key)) => self.project_plugin_keys(home, block, folder, key),
                None => Vec::new(),
            },
        };
        let mut found = Vec::new();
        for record in records {
            let folder = paths::lexical(Path::new(&record.folder));
            if !paths::is_absolute(&folder) {
                tracing::debug!(plugin = %record.id, "plugin folder is not absolute");
                continue;
            }
            let superseded = plugins.superseded_marker.as_ref().is_some_and(|marker| {
                self.reader
                    .fs
                    .exists(&blocks::join_relative(&folder, marker))
            });
            if superseded {
                tracing::debug!(plugin = %record.id, "plugin folder superseded");
                continue;
            }
            let project = match &record.project {
                Some(spelling) => match self.projects.add(Path::new(spelling), None) {
                    Some(key) => Some(key),
                    // Bound to something that is not a Project path: no context loads it.
                    None => continue,
                },
                None => None,
            };
            let names = plugins.name.iter().flat_map(PluginNames::places);
            let named = record.name.clone().or_else(|| {
                names.into_iter().find_map(|name| {
                    let file = blocks::join_relative(&folder, &name.file);
                    let text = self.reader.peek(&file).ok()??;
                    let document = JsonFormat::STRICT.parse(text).ok()?;
                    let named = lookup(&document, &name.key_path)?.as_str()?;
                    (!named.is_empty()).then(|| named.to_owned())
                })
            });
            let name = named.unwrap_or_else(|| {
                let id = record.id.as_str();
                let separator = plugins.id_separator.as_deref();
                let name = separator.and_then(|separator| id.split_once(separator));
                name.map_or(id, |(name, _)| name).to_owned()
            });
            let taken = found.iter().any(|known: &ScannedPlugin| known.name == name);
            let name = if plugins.unique_names && taken {
                record.id.clone()
            } else {
                name
            };
            found.push(ScannedPlugin {
                plugins: plugins.id.clone(),
                id: record.id,
                name,
                folder,
                scope: record.scope,
                project,
                version: record.version,
                name_shared: false,
            });
        }
        if plugins.same_names_invalid {
            let names: Vec<String> = found.iter().map(|plugin| plugin.name.clone()).collect();
            for plugin in &mut found {
                plugin.name_shared = names.iter().filter(|name| **name == plugin.name).count() > 1;
            }
        }
        found
    }

    /// The records of a JSON file of plugin records, in its order.
    fn plugin_records(&mut self, home: &FoundHome, block: &JsonPluginRecords) -> Vec<PluginRecord> {
        let Some(path) = block.anchor.path(&home.roots, None) else {
            return Vec::new();
        };
        let Some(text) = self.reader.text(home, &path, None) else {
            return Vec::new();
        };
        match block.records(text) {
            Ok(records) => records,
            Err(error) => {
                self.reader
                    .fail(home, &path, None, ReadFailureKind::Parse, error.position);
                Vec::new()
            }
        }
    }

    /// A record for each plugin the Project folder `folder`, whose key is
    /// `project`, installs that is in the cache, in the file's order,
    /// applying in that Project. Of several complete versions, the last by
    /// name is taken.
    fn project_plugin_keys(
        &mut self,
        home: &FoundHome,
        block: &ProjectPluginKeys,
        folder: &Path,
        project: &str,
    ) -> Vec<PluginRecord> {
        let (Some(path), Some(cache)) = (
            block.anchor.path(&home.roots, Some(folder)),
            block.cache.path(&home.roots, None),
        ) else {
            return Vec::new();
        };
        let Some(text) = self.reader.text(home, &path, Some(project)) else {
            return Vec::new();
        };
        let keys = match block.keys(text) {
            Ok(keys) => keys,
            Err(error) => {
                self.reader.fail(
                    home,
                    &path,
                    Some(project),
                    ReadFailureKind::Parse,
                    error.position,
                );
                return Vec::new();
            }
        };
        // As the Project is spelled for its records.
        let spelling = folder.display().to_string();
        let fs = self.reader.fs;
        let complete = |plugin: PathBuf| {
            let mut versions = fs.read_dir(&plugin).unwrap_or_default();
            versions.retain(|version| fs.exists(&blocks::join_relative(version, &block.marker)));
            versions.sort_by_key(|version| paths::key(version));
            versions.pop()
        };
        keys.into_iter()
            .filter_map(|plugin| {
                let version = match &plugin.marketplace {
                    Some(marketplace) => complete(cache.join(marketplace).join(&plugin.name)),
                    None => {
                        let mut marketplaces = fs.read_dir(&cache).unwrap_or_default();
                        marketplaces.sort_by_key(|marketplace| paths::key(marketplace));
                        marketplaces
                            .into_iter()
                            .find_map(|marketplace| complete(marketplace.join(&plugin.name)))
                    }
                };
                let Some(version) = version else {
                    tracing::debug!(plugin = %plugin.key, "plugin not in the cache");
                    return None;
                };
                Some(PluginRecord {
                    id: plugin.key,
                    name: Some(plugin.name),
                    folder: version.display().to_string(),
                    scope: Some(block.scope.clone()),
                    project: Some(spelling.clone()),
                    version: version
                        .file_name()
                        .map(|name| name.to_string_lossy().into_owned()),
                })
            })
            .collect()
    }

    /// A record for each folder that holds the marker, or with no marker
    /// each folder whose name does not start with `.`, sorted by name, with
    /// the version its install record gives.
    fn plugin_folders(&mut self, home: &FoundHome, block: &PluginFolders) -> Vec<PluginRecord> {
        let Some(folder) = block.anchor.path(&home.roots, None) else {
            return Vec::new();
        };
        let versions = block
            .records
            .as_ref()
            .and_then(|records| {
                let path = records.anchor.path(&home.roots, None)?;
                let text = self.reader.text(home, &path, None)?;
                match records.versions(text) {
                    Ok(versions) => Some(versions),
                    Err(error) => {
                        self.reader
                            .fail(home, &path, None, ReadFailureKind::Parse, error.position);
                        None
                    }
                }
            })
            .unwrap_or_default();
        let fs = self.reader.fs;
        let mut children = fs.read_dir(&folder).unwrap_or_default();
        children.sort_by_key(|child| paths::key(child));
        children
            .into_iter()
            .filter_map(|child| {
                let id = child.file_name()?.to_str()?.to_owned();
                let plugin = match &block.marker {
                    Some(marker) => marker
                        .all()
                        .iter()
                        .any(|marker| fs.exists(&blocks::join_relative(&child, marker))),
                    None => !id.starts_with('.') && fs.read_dir(&child).is_ok(),
                };
                if !plugin {
                    return None;
                }
                let linked = block.linked.as_ref().and_then(|linked| {
                    let file = blocks::join_relative(&child, &linked.file);
                    let text = String::from_utf8(fs.read(&file).ok()?).ok()?;
                    linked.target(&text)
                });
                Some(PluginRecord {
                    version: versions.get(&id).cloned(),
                    folder: linked.as_ref().unwrap_or(&child).display().to_string(),
                    id,
                    name: None,
                    scope: None,
                    project: None,
                })
            })
            .collect()
    }

    /// A record for the highest version of each plugin of the cache that
    /// holds what the cache requires and not what it excludes, sorted by id.
    fn plugin_cache(&mut self, home: &FoundHome, block: &PluginCache) -> Vec<PluginRecord> {
        let Some(cache) = block.anchor.path(&home.roots, None) else {
            return Vec::new();
        };
        let fs = self.reader.fs;
        // The folders of `folder`, sorted by name, leaving out dot-folders.
        let folders = |folder: &Path| {
            let mut children: Vec<(String, PathBuf)> = fs
                .read_dir(folder)
                .unwrap_or_default()
                .into_iter()
                .filter(|child| fs.read_dir(child).is_ok())
                .filter_map(|child| {
                    let name = child.file_name()?.to_str()?.to_owned();
                    (!name.starts_with('.')).then_some((name, child))
                })
                .collect();
            children.sort_by_key(|(name, _)| name.to_lowercase());
            children
        };
        let holds = |plugin: &Path, marker: &str| fs.exists(&blocks::join_relative(plugin, marker));
        let mut records = Vec::new();
        for (marketplace, marketplace_folder) in folders(&cache) {
            for (name, plugin) in folders(&marketplace_folder) {
                if block
                    .requires
                    .as_ref()
                    .is_some_and(|marker| !holds(&plugin, marker))
                    || block
                        .excludes
                        .as_ref()
                        .is_some_and(|marker| holds(&plugin, marker))
                {
                    continue;
                }
                let highest = folders(&plugin).into_iter().max_by(|(a, _), (b, _)| {
                    match (version::Version::parse(a), version::Version::parse(b)) {
                        (Some(a), Some(b)) => a.cmp(&b),
                        _ => a.cmp(b),
                    }
                });
                let Some((version, folder)) = highest else {
                    tracing::debug!(plugin = %name, "plugin without a version folder");
                    continue;
                };
                records.push(PluginRecord {
                    id: format!("{name}{}{marketplace}", block.separator),
                    name: Some(name),
                    folder: folder.display().to_string(),
                    scope: None,
                    project: None,
                    version: Some(version),
                });
            }
        }
        // Of plugins whose Entries tie, the first by id wins, as in Codex.
        records.sort_by(|a, b| a.id.cmp(&b.id));
        records
    }

    /// Adds the Entries of config-digger's parked-Entry store, each one of
    /// the location it was parked from, in its Agent home, at its key path
    /// in the store. A parked Entry of an Agent, home or location not found
    /// now is left out. Returns the store's path and where each Entry came
    /// from.
    fn read_parked(
        &mut self,
        definitions: &[AgentDefinition],
        found: &[FoundHome],
        ports: &Ports,
    ) -> (Option<PathBuf>, Vec<ScannedParked>) {
        let Ok(store) = park::store_path(ports.env.as_ref()) else {
            return (None, Vec::new());
        };
        let document = match self.reader.peek(&store) {
            Ok(None) => return (Some(store), Vec::new()),
            Ok(Some(text)) => JsonFormat::STRICT.parse(text).ok(),
            Err(_) => None,
        };
        let Some(document) = document else {
            tracing::warn!(path = %store.display(), "cannot read the parked-Entry store");
            return (Some(store), Vec::new());
        };
        let mut parked = Vec::new();
        for (id, parked_entry) in park::parked_entries(&document) {
            let home = found.iter().find(|home| {
                definitions[home.agent_index].agent.id == parked_entry.origin.agent
                    && home.home == parked_entry.origin.home
            });
            let location = home.and_then(|home| {
                definitions[home.agent_index]
                    .locations
                    .iter()
                    .find(|location| location.id == parked_entry.origin.location)
            });
            let (Some(home), Some(location)) = (home, location) else {
                tracing::debug!(%id, "parked Entry of no home found");
                continue;
            };
            let key_path = park::key_path(&id);
            let masked = mask::entry(location, &parked_entry.entry);
            self.entries.push(ScannedEntry {
                agent_index: home.agent_index,
                home: home.home.clone(),
                location: location.id.clone(),
                path: store.clone(),
                key_path: key_path.clone(),
                name: parked_entry.origin.name,
                project: parked_entry.origin.project,
                config: masked.value,
                masked: masked.key_paths,
                depth: 0,
                defect: None,
                link: None,
                manager: None,
                plugin: None,
                switch_id: None,
                config_hash: None,
            });
            parked.push(ScannedParked {
                key_path,
                origin: parked_entry.origin.path,
                origin_key_path: parked_entry.origin.key_path,
                parked_at: parked_entry.parked_at,
                folder: parked_entry
                    .folder
                    .map(|name| park::folders_path(&store).join(name)),
            });
        }
        (Some(store), parked)
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
        self.add_reader(home, location, &path);
        match &location.format {
            Format::SkillFolder(folder) => {
                return self.read_skills(home, location, folder, &path, project, depth);
            }
            Format::SkillList(list) => {
                return self.read_skill_list(home, location, list, &path, project, depth);
            }
            // Read in the folder of each plugin alone.
            Format::ManifestSkills(_) => return,
            Format::Json(_)
            | Format::Toml(_)
            | Format::TomlFragments(_)
            | Format::SqliteJson(_) => {}
        }
        let Some(document) = self.document(home, location, &path, project.as_deref()) else {
            return;
        };
        if let Some(key_path) = &location.cached_at {
            let at = lookup(&document, key_path).and_then(blocks::time_of);
            self.cached.insert(path.clone(), at);
        }
        if location.one_entry {
            // Named by its plugin, once read.
            if let Some(value) = lookup(&document, &location.root) {
                self.unmasked.push(self.entries.len());
                self.entries.push(ScannedEntry {
                    agent_index: home.agent_index,
                    home: home.home.clone(),
                    location: location.id.clone(),
                    path,
                    key_path: location.root.clone(),
                    name: String::new(),
                    project,
                    config: value.clone(),
                    masked: Vec::new(),
                    depth,
                    defect: None,
                    link: None,
                    manager: None,
                    plugin: None,
                    switch_id: None,
                    config_hash: None,
                });
            }
            return;
        }
        let Ok(objects) = entries_of(location, &document) else {
            let project = project.as_deref();
            self.reader
                .fail(home, &path, project, ReadFailureKind::Parse, None);
            return;
        };
        self.add_objects(home, definition, location, &path, project, depth, objects);
    }

    /// Adds the Entries of one location in the folder of `plugin`, each
    /// named as the location says and part of the plugin. With references,
    /// the value at its root can also point to other files of the plugin,
    /// whose Entries are read from there.
    fn read_plugin_location(
        &mut self,
        home: &FoundHome,
        definition: &AgentDefinition,
        location: &LocationDefinition,
        plugin: &ScannedPlugin,
        relative: &RelativePaths,
    ) {
        // The first of its paths that exists, or else the first.
        let candidates = relative
            .all()
            .iter()
            .map(|relative| blocks::join_relative(&plugin.folder, relative));
        let path = candidates
            .clone()
            .find(|path| self.reader.fs.exists(path))
            .or_else(|| candidates.clone().next())
            .expect("validation gives a plugin anchor a path");
        let start = self.entries.len();
        match &location.references {
            None if let Format::ManifestSkills(block) = &location.format => {
                self.read_manifest_skills(home, location, block, plugin, path);
            }
            None => {
                let project = plugin.project.clone();
                self.read_location(home, definition, location, path, project, 0);
            }
            Some(references) => {
                self.read_references(home, definition, location, references, plugin, path);
            }
        }
        for entry in &mut self.entries[start..] {
            entry.name = if location.one_entry {
                plugin.name.clone()
            } else {
                let parts = NameParts {
                    plugin: Some(&plugin.name),
                    folder: None,
                };
                location.entry_name(&entry.name, &parts)
            };
            entry.plugin = Some(plugin.clone());
        }
    }

    /// Adds the skills at the paths the manifest `manifest` of `plugin`
    /// declares, or else of its default folder. A folder outside the
    /// plugin's is skipped.
    fn read_manifest_skills(
        &mut self,
        home: &FoundHome,
        location: &LocationDefinition,
        block: &ManifestSkills,
        plugin: &ScannedPlugin,
        manifest: PathBuf,
    ) {
        let project = plugin.project.clone();
        let text = match self.reader.fs.exists(&manifest) {
            true => {
                self.add_reader(home, location, &manifest);
                self.reader
                    .text(home, &manifest, project.as_deref())
                    .map(str::to_owned)
            }
            false => None,
        };
        for declared in block.paths(text.as_deref()) {
            let base = &plugin.folder;
            if declared.may_name_one {
                let added = self.add_named_skill(
                    home,
                    location,
                    &block.skills,
                    base,
                    &declared.path,
                    &project,
                    0,
                );
                if added {
                    continue;
                }
            }
            let folder = paths::lexical(&base.join(&declared.path));
            if !folder.starts_with(base) {
                tracing::debug!(plugin = %plugin.id, "skills folder outside the plugin");
                continue;
            }
            self.add_reader(home, location, &folder);
            self.read_skills(home, location, &block.skills, &folder, project.clone(), 0);
        }
    }

    /// Adds the Entries at the root of the file `path` of `plugin`: an
    /// object of Entries, a path to another file of the plugin holding
    /// them, or a list of either.
    fn read_references(
        &mut self,
        home: &FoundHome,
        definition: &AgentDefinition,
        location: &LocationDefinition,
        references: &References,
        plugin: &ScannedPlugin,
        path: PathBuf,
    ) {
        self.add_reader(home, location, &path);
        let project = plugin.project.as_deref();
        let Some(document) = self.document(home, location, &path, project) else {
            return;
        };
        let values: Vec<(Option<usize>, &serde_json::Value)> =
            match lookup(&document, &location.root) {
                None => Vec::new(),
                Some(serde_json::Value::Array(values)) => values
                    .iter()
                    .enumerate()
                    .map(|(i, v)| (Some(i), v))
                    .collect(),
                Some(value) => vec![(None, value)],
            };
        for (index, value) in values {
            match value {
                serde_json::Value::String(relative) => {
                    let file = paths::lexical(&plugin.folder.join(relative));
                    if !file.starts_with(&plugin.folder) {
                        tracing::debug!(plugin = %plugin.id, "reference outside the plugin");
                        continue;
                    }
                    self.add_reader(home, location, &file);
                    let Some(referenced) = self.document(home, location, &file, project) else {
                        continue;
                    };
                    let mut objects = Vec::new();
                    if find_objects(
                        &referenced,
                        &references.root,
                        None,
                        Vec::new(),
                        None,
                        &mut objects,
                    )
                    .is_err()
                    {
                        self.reader
                            .fail(home, &file, project, ReadFailureKind::Parse, None);
                        continue;
                    }
                    let owned = plugin.project.clone();
                    self.add_objects(home, definition, location, &file, owned, 0, objects);
                }
                serde_json::Value::Object(entries) => {
                    let mut key_path = location.root.clone();
                    key_path.extend(index.map(|index| index.to_string()));
                    let entries = entries
                        .iter()
                        .map(|(key, value)| FoundEntry {
                            key: key.clone(),
                            name: key.clone(),
                            value,
                        })
                        .collect();
                    let objects = vec![RootObject {
                        key_path,
                        project: None,
                        entries,
                    }];
                    let owned = plugin.project.clone();
                    self.add_objects(home, definition, location, &path, owned, 0, objects);
                }
                _ => self
                    .reader
                    .fail(home, &path, project, ReadFailureKind::Parse, None),
            }
        }
    }

    /// Gives the Entries read since the first `start` the names the
    /// `name_template` of `location` makes of theirs and `parts`.
    fn name_entries(&mut self, start: usize, location: &LocationDefinition, parts: &NameParts) {
        for entry in &mut self.entries[start..] {
            entry.name = location.entry_name(&entry.name, parts);
        }
    }

    /// Masks the Secrets of each Entry read so far whose `config` is still
    /// its real value, those of the fields any location that reads its file
    /// marks included, so that every Agent's view of the file masks them.
    fn mask_entries(&mut self, definitions: &Definitions) {
        let mut readers: BTreeMap<&Path, Vec<&LocationDefinition>> = BTreeMap::new();
        for reader in &self.readers {
            let location = definitions[reader.agent_index].location(&reader.location);
            readers.entry(&reader.path).or_default().push(location);
        }
        for index in self.unmasked.drain(..) {
            let entry = &mut self.entries[index];
            let location = definitions[entry.agent_index].location(&entry.location);
            let readers = readers
                .get(entry.path.as_path())
                .map_or(&[][..], Vec::as_slice);
            let masked = mask::entry_read_by(location, readers, &entry.config);
            entry.config = masked.value;
            entry.masked = masked.key_paths;
        }
    }

    /// Records that `location` reads the file or folder at `path`.
    fn add_reader(&mut self, home: &FoundHome, location: &LocationDefinition, path: &Path) {
        let reader = LocationReader {
            agent_index: home.agent_index,
            home: home.home.clone(),
            location: location.id.clone(),
            path: path.to_path_buf(),
        };
        if !self.readers.contains(&reader) {
            self.readers.push(reader);
        }
    }

    /// The file at `path` parsed in the format of `location`, `None` when
    /// it is missing or a Read failure, which is then reported.
    fn document(
        &mut self,
        home: &FoundHome,
        location: &LocationDefinition,
        path: &Path,
        project: Option<&str>,
    ) -> Option<serde_json::Value> {
        if let Some(store) = location.format.sqlite() {
            let text = self.reader.sqlite_text(home, path, &store.key, project)?;
            return match store.parse(&text) {
                Ok(document) => Some(document),
                Err(error) => {
                    self.reader
                        .fail(home, path, project, ReadFailureKind::Parse, error.position);
                    None
                }
            };
        }
        let format = location.format.file()?;
        let text = self.reader.text(home, path, project)?;
        match format.parse(text) {
            Ok(document) => Some(document),
            Err(error) => {
                self.reader
                    .fail(home, path, project, ReadFailureKind::Parse, error.position);
                None
            }
        }
    }

    /// Adds the Entries of `objects`, read at `path` for `location`.
    #[allow(clippy::too_many_arguments)]
    fn add_objects(
        &mut self,
        home: &FoundHome,
        definition: &AgentDefinition,
        location: &LocationDefinition,
        path: &Path,
        project: Option<String>,
        depth: usize,
        objects: Vec<RootObject>,
    ) {
        // How the Agent hashes the configuration of an Entry it approves.
        let hash = definition
            .rules(location.kind)
            .gates
            .iter()
            .find_map(|gate| match gate {
                Gate::ApprovalHash(block) if block.locations.contains(&location.id) => {
                    Some(block.hash)
                }
                _ => None,
            });
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
            for found in object.entries {
                let mut key_path = object.key_path.clone();
                key_path.push(found.key);
                self.unmasked.push(self.entries.len());
                self.entries.push(ScannedEntry {
                    agent_index: home.agent_index,
                    home: home.home.clone(),
                    location: location.id.clone(),
                    path: path.to_path_buf(),
                    key_path,
                    name: found.name,
                    project: project.clone(),
                    config: found.value.clone(),
                    masked: Vec::new(),
                    depth,
                    defect: None,
                    link: None,
                    manager: None,
                    plugin: None,
                    switch_id: None,
                    config_hash: hash.and_then(|hash| hash.of(found.value)),
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
                        plugin: None,
                        switch_id: None,
                        config_hash: None,
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
                let skill = SkillAt {
                    folder: &child,
                    path: real.join(name),
                    name,
                };
                self.add_skill(home, location, format, skill, &project, depth);
            }
        }
    }

    /// Adds a skill Entry for the folder `skill`, whose `SKILL.md` exists.
    fn add_skill(
        &mut self,
        home: &FoundHome,
        location: &LocationDefinition,
        format: &SkillFolder,
        skill: SkillAt,
        project: &Option<String>,
        depth: usize,
    ) {
        let fs = self.reader.fs;
        let skill_md = skill.folder.join(SKILL_MD);
        let Some(text) = self.reader.text(home, &skill_md, project.as_deref()) else {
            return;
        };
        let read = read_skill(format, skill.name, text);
        tracing::debug!(location = %location.id, skill = %read.name, "read skill");
        // A link at the skill's own location is an Entry apart from its target.
        let link = fs.link_target(skill.folder).map(|target| ScannedLink {
            target: paths::lexical(&target),
            real: fs.real_path(skill.folder).map(|real| paths::lexical(&real)),
        });
        self.entries.push(ScannedEntry {
            agent_index: home.agent_index,
            home: home.home.clone(),
            location: location.id.clone(),
            path: skill.path,
            key_path: Vec::new(),
            name: read.name,
            project: project.clone(),
            config: read.config,
            masked: Vec::new(),
            depth,
            defect: read.defect,
            link,
            manager: None,
            plugin: None,
            switch_id: None,
            config_hash: None,
        });
    }

    /// Adds a skill Entry for each folder the list in the JSON file `path`
    /// names, such as an extension's `contributes.chatSkills`, each by its
    /// own path or its `SKILL.md`'s, relative to the file's folder. A path
    /// outside that folder, or without a `SKILL.md`, names no skill.
    fn read_skill_list(
        &mut self,
        home: &FoundHome,
        location: &LocationDefinition,
        list: &SkillList,
        path: &Path,
        project: Option<String>,
        depth: usize,
    ) {
        let Some(base) = path.parent() else {
            return;
        };
        let Some(text) = self.reader.text(home, path, project.as_deref()) else {
            return;
        };
        let document = match JsonFormat::STRICT.parse(text) {
            Ok(document) => document,
            Err(error) => {
                let project = project.as_deref();
                self.reader
                    .fail(home, path, project, ReadFailureKind::Parse, error.position);
                return;
            }
        };
        let relatives: Vec<String> = lookup(&document, &list.list)
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(|item| Some(item.get(&list.field)?.as_str()?.to_owned()))
            .collect();
        for relative in relatives {
            let added = self.add_named_skill(
                home,
                location,
                &list.skills,
                base,
                &relative,
                &project,
                depth,
            );
            if !added {
                tracing::debug!(path = %relative, "names no skill");
            }
        }
    }

    /// Adds the skill whose folder, or its `SKILL.md`, `relative` names
    /// below `base`. Returns `false`, adding nothing, when it names no
    /// skill folder there.
    #[allow(clippy::too_many_arguments)]
    fn add_named_skill(
        &mut self,
        home: &FoundHome,
        location: &LocationDefinition,
        skills: &SkillFolder,
        base: &Path,
        relative: &str,
        project: &Option<String>,
        depth: usize,
    ) -> bool {
        let fs = self.reader.fs;
        let mut folder = paths::lexical(&base.join(relative));
        let names_file = folder
            .file_name()
            .is_some_and(|name| name.eq_ignore_ascii_case(SKILL_MD));
        if names_file {
            folder.pop();
        }
        if !folder.starts_with(base) || !fs.exists(&folder.join(SKILL_MD)) {
            return false;
        }
        let Some(name) = folder.file_name().and_then(|name| name.to_str()) else {
            return false;
        };
        let name = name.to_owned();
        let real = fs
            .real_path(&folder)
            .map_or_else(|| folder.clone(), |real| paths::lexical(&real));
        let skill = SkillAt {
            folder: &folder,
            path: real,
            name: &name,
        };
        self.add_skill(home, location, skills, skill, project, depth);
        true
    }
}

/// A skill folder to read: where it is, the path its Entry has, with links
/// in its parent folders resolved, and its folder's name.
struct SkillAt<'a> {
    folder: &'a Path,
    path: PathBuf,
    name: &'a str,
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
    for definition in managers {
        for detector in &definition.detectors {
            let strength = detector.strength();
            if best.as_ref().is_some_and(|best| best.strength >= strength) {
                continue;
            }
            if detects(detector, reader, ports, entry) {
                best = Some(ManagerMatch {
                    id: definition.manager.id.clone(),
                    name: definition.manager.name.clone(),
                    strength,
                });
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

/// An object or list whose items are Entries.
pub(crate) struct RootObject<'v> {
    pub key_path: Vec<String>,
    /// The Project key it sits under, in the Agent's spelling.
    pub project: Option<String>,
    pub entries: Vec<FoundEntry<'v>>,
}

/// One Entry of a [`RootObject`].
pub(crate) struct FoundEntry<'v> {
    /// Its key below the object, or its index in the list.
    pub key: String,
    /// The name it goes by: its key, or in a list, the field that names it.
    pub name: String,
    /// Its value as declared.
    pub value: &'v serde_json::Value,
}

/// Where Entries are, a file holds something other than an object, so
/// config-digger cannot enumerate them.
#[derive(Debug)]
pub(crate) struct NotEnumerable;

/// The objects of Entries `location` reads in `document`: at its root, or
/// at the top level of a bare file without it.
pub(crate) fn entries_of<'v>(
    location: &LocationDefinition,
    document: &'v serde_json::Value,
) -> Result<Vec<RootObject<'v>>, NotEnumerable> {
    let root: &[String] = if location.bare && lookup(document, &location.root).is_none() {
        &[]
    } else {
        &location.root
    };
    let mut objects = Vec::new();
    find_objects(
        document,
        root,
        location.named_by.as_deref(),
        Vec::new(),
        None,
        &mut objects,
    )?;
    Ok(objects)
}

/// The objects at `root` below `value`, or with `named_by`, the lists there
/// of objects each named by that field. Below `{project}`, every key is a
/// Project, and below `*` every key is read. A missing key holds no
/// objects; a key on the way that holds something other than an object
/// fails, as does a list where an object is expected, or the reverse. An
/// item of a list without its name is skipped.
pub(crate) fn find_objects<'v>(
    value: &'v serde_json::Value,
    root: &[String],
    named_by: Option<&str>,
    key_path: Vec<String>,
    project: Option<String>,
    out: &mut Vec<RootObject<'v>>,
) -> Result<(), NotEnumerable> {
    let Some((key, rest)) = root.split_first() else {
        let entries = match (value, named_by) {
            (serde_json::Value::Object(object), None) => object
                .iter()
                .map(|(key, value)| FoundEntry {
                    key: key.clone(),
                    name: key.clone(),
                    value,
                })
                .collect(),
            (serde_json::Value::Array(items), Some(field)) => items
                .iter()
                .enumerate()
                .filter_map(|(index, value)| {
                    let name = value.get(field)?.as_str()?;
                    Some(FoundEntry {
                        key: index.to_string(),
                        name: name.to_owned(),
                        value,
                    })
                })
                .collect(),
            _ => return Err(NotEnumerable),
        };
        out.push(RootObject {
            key_path,
            project,
            entries,
        });
        return Ok(());
    };
    let serde_json::Value::Object(object) = value else {
        return Err(NotEnumerable);
    };
    let children: Vec<(&String, &serde_json::Value)> = if key == PROJECT_KEY || key == ANY_KEY {
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
        find_objects(child, rest, named_by, child_key_path, child_project, out)?;
    }
    Ok(())
}

/// Reads each file once per scan, and reports each Read failure once per Agent home.
struct Reader<'p> {
    /// The file system as the scan reads it.
    fs: &'p dyn FileSystem,
    /// Files read as holding these bytes instead of what is on disk.
    written: &'p [Written],
    files: BTreeMap<PathBuf, Result<Option<String>, ReadFailureKind>>,
    /// What SQLite stores hold, by store and key.
    items: BTreeMap<(PathBuf, String), Result<Option<String>, ReadFailureKind>>,
    reported: BTreeSet<(usize, String, PathBuf)>,
    failures: Vec<ScannedReadFailure>,
    /// The files each Agent, by its index, skips without reading them, since
    /// someone other than an administrator can change them.
    not_admin_owned: NotAdminOwned,
}

impl<'p> Reader<'p> {
    fn new(fs: &'p dyn FileSystem, written: &'p [Written]) -> Self {
        Self {
            fs,
            written,
            files: BTreeMap::new(),
            items: BTreeMap::new(),
            reported: BTreeSet::new(),
            failures: Vec::new(),
            not_admin_owned: NotAdminOwned::default(),
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

    /// The text under `key` in the `ItemTable` of the SQLite store at
    /// `path`, `None` when the store or the key is missing, or the store
    /// cannot be opened or queried, which is then reported.
    fn sqlite_text(
        &mut self,
        home: &FoundHome,
        path: &Path,
        key: &str,
        project: Option<&str>,
    ) -> Option<String> {
        match self.peek_item(path, key) {
            Ok(text) => text,
            Err(kind) => {
                self.fail(home, path, project, kind, None);
                None
            }
        }
    }

    /// The text under `key` in the `ItemTable` of the SQLite store at
    /// `path`, `None` when the store or the key is missing, as
    /// [`sqlite_text`](Self::sqlite_text) reads it but without reporting a
    /// failure: for a store that holds no Entries, such as a switch's.
    fn peek_item(&mut self, path: &Path, key: &str) -> Result<Option<String>, ReadFailureKind> {
        let id = (path.to_path_buf(), key.to_owned());
        if !self.items.contains_key(&id) {
            let read = match Written::item(self.written, path, key)
                .map_or_else(|| self.fs.read_sqlite_item(path, key), Ok)
            {
                Ok(text) => Ok(text),
                Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
                Err(error) => {
                    tracing::debug!(path = %path.display(), %error, "cannot read the store");
                    Err(ReadFailureKind::Sqlite)
                }
            };
            self.items.insert(id.clone(), read);
        }
        self.items[&id].clone()
    }

    /// The texts of the column `column` of the table `table` in the SQLite
    /// store at `path`, as the text of a JSON list, `None` when the store
    /// is missing, without reporting a failure: for a store that holds no
    /// Entries, such as a gate's. A store that cannot be opened or queried,
    /// its table missing included, is unreadable: what it holds is not
    /// known, rather than empty.
    fn peek_column(
        &mut self,
        path: &Path,
        table: &str,
        column: &str,
    ) -> Result<Option<String>, ReadFailureKind> {
        match self.fs.read_sqlite_column(path, table, column) {
            Ok(values) => Ok(Some(serde_json::Value::from(values).to_string())),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => {
                tracing::debug!(path = %path.display(), %table, %error, "cannot read the column");
                Err(ReadFailureKind::Sqlite)
            }
        }
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
        match Written::file(self.written, path) {
            Some(bytes) => decode(bytes.to_vec()).map(Some),
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
                            members: Vec::new(),
                            repo_root: None,
                            main_checkout: None,
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
