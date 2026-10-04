//! The building blocks Agent definitions are assembled from (ADR 0005).
//!
//! Each family is a closed enum tagged by `block`. Every variant wraps a
//! parameter struct that implements [`Validate`] and its family's trait, so a
//! new block is added in one place and the compiler points at every `match`
//! that must handle it.

use crate::edit::{Edit, Syntax};
use crate::installations::pe;
use crate::paths;
use crate::ports::{Environment, FileSystem, Hive, Ports, RegistryData};
use crate::version::Version;
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};
use std::time::{Duration, SystemTime};

/// Checks a block's parameters beyond what deserialisation enforces.
pub(crate) trait Validate {
    fn validate(&self) -> Result<(), String>;
}

// --- Home locators ----------------------------------------------------------

/// Finds the folders of an Agent home on this machine.
pub(crate) trait Locate {
    /// Every folder it names here, in order: none when the environment
    /// cannot say where it is.
    fn folders(&self, env: &dyn Environment, fs: &dyn FileSystem) -> Vec<PathBuf>;

    /// Whether it lies in the user's home folder, so that nothing can be
    /// read without knowing where that is.
    fn in_home_folder(&self) -> bool;

    /// Whether the Agent uses a folder it names even before the folder
    /// exists, creating it, so that a lookup order picks it as soon as it
    /// is named.
    fn used_before_it_exists(&self) -> bool;
}

/// Where one root of an Agent home lives.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "block", rename_all = "kebab-case")]
pub(crate) enum Locator {
    Path(PathLocator),
    Env(EnvLocator),
    JsonList(JsonListLocator),
}

impl Validate for Locator {
    fn validate(&self) -> Result<(), String> {
        match self {
            Locator::Path(block) => block.validate(),
            Locator::Env(block) => block.validate(),
            Locator::JsonList(block) => block.validate(),
        }
    }
}

impl Locate for Locator {
    fn folders(&self, env: &dyn Environment, fs: &dyn FileSystem) -> Vec<PathBuf> {
        match self {
            Locator::Path(block) => block.folders(env, fs),
            Locator::Env(block) => block.folders(env, fs),
            Locator::JsonList(block) => block.folders(env, fs),
        }
    }

    fn in_home_folder(&self) -> bool {
        match self {
            Locator::Path(block) => block.in_home_folder(),
            Locator::Env(block) => block.in_home_folder(),
            Locator::JsonList(block) => block.in_home_folder(),
        }
    }

    fn used_before_it_exists(&self) -> bool {
        match self {
            Locator::Path(block) => block.used_before_it_exists(),
            Locator::Env(block) => block.used_before_it_exists(),
            Locator::JsonList(block) => block.used_before_it_exists(),
        }
    }
}

/// The folders a list in a strict JSON file names, each relative to a base
/// folder, such as the VS Code profiles `userDataProfiles` in `storage.json`
/// lists below `User\profiles`. An item whose `unless` holds `true` names no
/// home of its own, such as a profile that shares its MCP servers with the
/// default profile, and nor does one whose folder is not a relative path.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct JsonListLocator {
    /// The file. Starts with `%NAME%` or a drive; `/`-separated.
    pub file: String,
    /// The key path of the list, one key per segment.
    pub key_path: Vec<String>,
    /// The field of an item that holds its folder, relative to `base`.
    pub field: String,
    /// The folder the items' folders are in, written as `file` is.
    pub base: String,
    /// The key path in an item of a flag that, while `true`, leaves it out.
    #[serde(default)]
    pub unless: Vec<String>,
}

impl Validate for JsonListLocator {
    fn validate(&self) -> Result<(), String> {
        for path in [&self.file, &self.base] {
            MachineAnchor { path: path.clone() }.validate()?;
            if path.starts_with("~/") {
                return Err(format!("path `{path}` must start with `%NAME%` or a drive"));
            }
        }
        validate_key_path(&self.key_path)?;
        if self.field.is_empty() {
            return Err("a json-list field must not be empty".into());
        }
        if self.unless.iter().any(String::is_empty) {
            return Err("unless must have no empty key".into());
        }
        Ok(())
    }
}

impl Locate for JsonListLocator {
    fn folders(&self, env: &dyn Environment, fs: &dyn FileSystem) -> Vec<PathBuf> {
        let (Some(file), Some(base)) = (
            paths::expand(&self.file, env),
            paths::expand(&self.base, env),
        ) else {
            return Vec::new();
        };
        let document = fs
            .read(&file)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
            .and_then(|text| JsonFormat::STRICT.parse(&text).ok());
        let items = document
            .as_ref()
            .and_then(|document| {
                self.key_path
                    .iter()
                    .try_fold(document, |value, key| value.get(key))
            })
            .and_then(Value::as_array);
        items
            .into_iter()
            .flatten()
            .filter(|item| {
                let flag = self
                    .unless
                    .iter()
                    .try_fold(*item, |value, key| value.get(key));
                flag != Some(&Value::Bool(true))
            })
            .filter_map(|item| {
                let relative = item.get(&self.field)?.as_str()?;
                if validate_relative(relative).is_err() || relative.contains(':') {
                    tracing::debug!(folder = %relative, "not a relative folder");
                    return None;
                }
                Some(join_relative(&base, relative))
            })
            .collect()
    }

    fn in_home_folder(&self) -> bool {
        false
    }

    fn used_before_it_exists(&self) -> bool {
        false
    }
}

/// The folder an environment variable holds, such as Claude Desktop's
/// `CLAUDE_USER_DATA_DIR`, while it is set to an absolute path. The Agent
/// uses it whenever it is set, creating it if needed.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct EnvLocator {
    pub variable: String,
}

impl Validate for EnvLocator {
    fn validate(&self) -> Result<(), String> {
        validate_variable(&self.variable)
    }
}

impl Locate for EnvLocator {
    fn folders(&self, env: &dyn Environment, _fs: &dyn FileSystem) -> Vec<PathBuf> {
        let Some(value) = env.var(&self.variable) else {
            return Vec::new();
        };
        let folder = paths::lexical(Path::new(value.trim()));
        if !paths::is_absolute(&folder) {
            tracing::debug!(variable = %self.variable, "not an absolute path");
            return Vec::new();
        }
        vec![folder]
    }

    fn in_home_folder(&self) -> bool {
        false
    }

    fn used_before_it_exists(&self) -> bool {
        true
    }
}

/// An environment variable's name: not empty, without `=`.
fn validate_variable(name: &str) -> Result<(), String> {
    if name.trim().is_empty() || name.contains('=') {
        return Err(format!("`{name}` is not a variable name"));
    }
    Ok(())
}

/// Whether the Agent reads an Agent home it can find, given what holds on
/// this machine.
pub(crate) trait HoldsHere {
    /// Whether it holds, given the environment `env` the Agent starts in.
    fn holds(&self, env: &dyn Environment) -> bool;
}

/// A condition under which an Agent reads a home of its lookup order.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "block", rename_all = "kebab-case")]
pub(crate) enum HomeCondition {
    UncVariable(UncVariable),
}

impl Validate for HomeCondition {
    fn validate(&self) -> Result<(), String> {
        match self {
            HomeCondition::UncVariable(block) => block.validate(),
        }
    }
}

impl HoldsHere for HomeCondition {
    fn holds(&self, env: &dyn Environment) -> bool {
        match self {
            HomeCondition::UncVariable(block) => block.holds(env),
        }
    }
}

/// Holds while an environment variable holds a path on a network share,
/// `\\server\share\…`, such as `%APPDATA%` in a redirected roaming profile.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct UncVariable {
    pub variable: String,
}

impl Validate for UncVariable {
    fn validate(&self) -> Result<(), String> {
        validate_variable(&self.variable)
    }
}

impl HoldsHere for UncVariable {
    fn holds(&self, env: &dyn Environment) -> bool {
        env.var(&self.variable).is_some_and(|value| {
            let value = value.trim().replace('/', "\\");
            value.starts_with("\\\\") && !value.starts_with("\\\\?\\")
        })
    }
}

/// The user's home folder, written `~`, or a fixed path below it, written
/// `~/<relative path>`; or a path that starts with `%NAME%`, expanded from
/// the environment, or a drive, `/`-separated. Outside the user's home
/// folder, `*` and `?` stay within one folder name and `{a,b}` is either,
/// such as an MSIX package's folder: the path then names every existing
/// folder that matches.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct PathLocator {
    pub path: String,
}

impl PathLocator {
    /// The path below the user's home folder, `None` for the folder itself.
    /// An error for a path outside it.
    fn relative(&self) -> Result<Option<&str>, String> {
        if self.path == "~" {
            return Ok(None);
        }
        match self.path.strip_prefix("~/") {
            Some(relative) => validate_relative(relative).map(|()| Some(relative)),
            None => Err(format!(
                "path `{}` must be `~`, or start with `~/`, `%NAME%` or a drive",
                self.path
            )),
        }
    }
}

impl Validate for PathLocator {
    fn validate(&self) -> Result<(), String> {
        if self.in_home_folder() {
            if self.path.contains(['*', '?', '{', '}']) {
                return Err(format!(
                    "path `{}` in the user's home folder cannot hold wildcards",
                    self.path
                ));
            }
            return self.relative().map(|_| ());
        }
        paths::validate_glob(&self.path).map_err(|_| {
            format!(
                "path `{}` must be `~`, or start with `~/`, `%NAME%` or a drive",
                self.path
            )
        })
    }
}

impl Locate for PathLocator {
    fn folders(&self, env: &dyn Environment, fs: &dyn FileSystem) -> Vec<PathBuf> {
        if self.path.contains(['*', '?', '{']) {
            return paths::expand_glob(&self.path, fs, env);
        }
        if !self.in_home_folder() {
            return paths::expand(&self.path, env).into_iter().collect();
        }
        let Some(home) = env.home_dir() else {
            return Vec::new();
        };
        vec![match self.relative() {
            Ok(Some(relative)) => join_relative(&home, relative),
            _ => home,
        }]
    }

    fn in_home_folder(&self) -> bool {
        self.path == "~" || self.path.starts_with("~/")
    }

    fn used_before_it_exists(&self) -> bool {
        false
    }
}

// --- Location anchors -------------------------------------------------------

/// Turns an anchor into the path of a configuration location.
pub(crate) trait Anchored {
    /// The location's path, given the roots of the Agent home being scanned
    /// and, for an anchor inside a Project, the Project's folder.
    fn path(&self, roots: &BTreeMap<String, PathBuf>, project: Option<&Path>) -> Option<PathBuf>;

    /// The home root this anchor needs, if any.
    fn home_root(&self) -> Option<&str>;

    /// Whether the location lies in a Project folder, so that there is one per Project.
    fn in_project(&self) -> bool;

    /// The machine-wide folder or file this anchor names, as written, such
    /// as `%ProgramFiles%/ClaudeCode`. Its expansion is among the roots
    /// `path` is given, under that text.
    fn machine(&self) -> Option<&str>;

    /// How far up from the Project folder the location is read.
    fn walk(&self) -> ProjectWalk;
}

/// What a location's path is relative to.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "block", rename_all = "kebab-case")]
pub(crate) enum Anchor {
    Home(HomeAnchor),
    Project(ProjectAnchor),
    Machine(MachineAnchor),
    Settings(SettingsAnchor),
    Plugin(PluginAnchor),
    Entry(EntryAnchor),
    ProjectFile(ProjectFileAnchor),
    ProjectStorage(ProjectStorageAnchor),
}

impl Validate for Anchor {
    fn validate(&self) -> Result<(), String> {
        match self {
            Anchor::Home(block) => block.validate(),
            Anchor::Project(block) => block.validate(),
            Anchor::Machine(block) => block.validate(),
            Anchor::Settings(block) => block.validate(),
            Anchor::Plugin(block) => block.validate(),
            Anchor::Entry(block) => block.validate(),
            Anchor::ProjectFile(block) => block.validate(),
            Anchor::ProjectStorage(block) => block.validate(),
        }
    }
}

impl Anchored for Anchor {
    fn path(&self, roots: &BTreeMap<String, PathBuf>, project: Option<&Path>) -> Option<PathBuf> {
        match self {
            Anchor::Home(block) => block.path(roots, project),
            Anchor::Project(block) => block.path(roots, project),
            Anchor::Machine(block) => block.path(roots, project),
            Anchor::Settings(block) => block.path(roots, project),
            Anchor::Plugin(block) => block.path(roots, project),
            Anchor::Entry(block) => block.path(roots, project),
            Anchor::ProjectFile(block) => block.path(roots, project),
            Anchor::ProjectStorage(block) => block.path(roots, project),
        }
    }

    fn home_root(&self) -> Option<&str> {
        match self {
            Anchor::Home(block) => block.home_root(),
            Anchor::Project(block) => block.home_root(),
            Anchor::Machine(block) => block.home_root(),
            Anchor::Settings(block) => block.home_root(),
            Anchor::Plugin(block) => block.home_root(),
            Anchor::Entry(block) => block.home_root(),
            Anchor::ProjectFile(block) => block.home_root(),
            Anchor::ProjectStorage(block) => block.home_root(),
        }
    }

    fn in_project(&self) -> bool {
        match self {
            Anchor::Home(block) => block.in_project(),
            Anchor::Project(block) => block.in_project(),
            Anchor::Machine(block) => block.in_project(),
            Anchor::Settings(block) => block.in_project(),
            Anchor::Plugin(block) => block.in_project(),
            Anchor::Entry(block) => block.in_project(),
            Anchor::ProjectFile(block) => block.in_project(),
            Anchor::ProjectStorage(block) => block.in_project(),
        }
    }

    fn machine(&self) -> Option<&str> {
        match self {
            Anchor::Home(block) => block.machine(),
            Anchor::Project(block) => block.machine(),
            Anchor::Machine(block) => block.machine(),
            Anchor::Settings(block) => block.machine(),
            Anchor::Plugin(block) => block.machine(),
            Anchor::Entry(block) => block.machine(),
            Anchor::ProjectFile(block) => block.machine(),
            Anchor::ProjectStorage(block) => block.machine(),
        }
    }

    fn walk(&self) -> ProjectWalk {
        match self {
            Anchor::Home(block) => block.walk(),
            Anchor::Project(block) => block.walk(),
            Anchor::Machine(block) => block.walk(),
            Anchor::Settings(block) => block.walk(),
            Anchor::Plugin(block) => block.walk(),
            Anchor::Entry(block) => block.walk(),
            Anchor::ProjectFile(block) => block.walk(),
            Anchor::ProjectStorage(block) => block.walk(),
        }
    }
}

/// A file in a folder above an Entry's own location, read once per Entry,
/// such as the `manifest.json` two folders above each skill Claude Desktop
/// mirrors from the account. Only a Disable switch is anchored here.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct EntryAnchor {
    /// How many folders up from the Entry's location, at least 1: 1 is the
    /// folder holding it.
    pub up: usize,
    /// A `/`-separated path relative to that folder.
    pub path: String,
}

impl Validate for EntryAnchor {
    fn validate(&self) -> Result<(), String> {
        if self.up == 0 {
            return Err("an entry anchor goes up at least one folder".into());
        }
        validate_relative(&self.path)
    }
}

impl Anchored for EntryAnchor {
    /// There is one path per Entry, which [`EntryAnchor::of`] finds.
    fn path(&self, _roots: &BTreeMap<String, PathBuf>, _project: Option<&Path>) -> Option<PathBuf> {
        None
    }

    fn home_root(&self) -> Option<&str> {
        None
    }

    fn in_project(&self) -> bool {
        false
    }

    fn machine(&self) -> Option<&str> {
        None
    }

    fn walk(&self) -> ProjectWalk {
        ProjectWalk::None
    }
}

impl EntryAnchor {
    /// The file for the Entry at `entry`.
    pub fn of(&self, entry: &Path) -> Option<PathBuf> {
        Some(join_relative(entry.ancestors().nth(self.up)?, &self.path))
    }
}

/// How far up from the Project folder a location in a Project is read.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ProjectWalk {
    /// In the Project folder only.
    #[default]
    None,
    /// In the Project folder and each parent up to the root of its git
    /// repo, the folder holding `.git`. Outside a repo, in the Project
    /// folder only.
    UpToRepoRoot,
}

/// A root of the Agent home, or a path below it.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct HomeAnchor {
    /// The name of a root declared by every home of the definition.
    pub root: String,
    /// A `/`-separated path relative to the root. Omitted when the root is
    /// the location. In a Disable switch or a setting, `{plugin_id}` may
    /// stand for the id of a plugin, such as Claude Desktop's
    /// `Claude Extensions Settings/{plugin_id}.json`: there is then one file
    /// per plugin, which applies to that plugin's Entries only.
    pub path: Option<String>,
}

impl HomeAnchor {
    /// Whether its path names one file per plugin.
    fn per_plugin(&self) -> bool {
        self.path
            .as_deref()
            .is_some_and(|path| path.contains(PLUGIN_KEY))
    }
}

impl Validate for HomeAnchor {
    fn validate(&self) -> Result<(), String> {
        match &self.path {
            Some(path) => validate_relative(path),
            None => Ok(()),
        }
    }
}

impl Anchored for HomeAnchor {
    /// `None` for a path that names one file per plugin.
    fn path(&self, roots: &BTreeMap<String, PathBuf>, _project: Option<&Path>) -> Option<PathBuf> {
        if self.per_plugin() {
            return None;
        }
        let root = roots.get(&self.root)?;
        Some(match &self.path {
            Some(path) => join_relative(root, path),
            None => root.clone(),
        })
    }

    fn home_root(&self) -> Option<&str> {
        Some(&self.root)
    }

    fn in_project(&self) -> bool {
        false
    }

    fn machine(&self) -> Option<&str> {
        None
    }

    fn walk(&self) -> ProjectWalk {
        ProjectWalk::None
    }
}

/// A path in the Project folder, and with `walk`, in its parents too.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProjectAnchor {
    /// A `/`-separated path relative to the Project folder.
    pub path: String,
    /// Which folders the path is read in, from the Project folder up.
    /// Only a location can walk.
    #[serde(default)]
    pub walk: ProjectWalk,
}

impl Validate for ProjectAnchor {
    fn validate(&self) -> Result<(), String> {
        validate_relative(&self.path)
    }
}

impl Anchored for ProjectAnchor {
    /// `project` is the folder read: the Project folder, or a parent of it
    /// the walk reaches.
    fn path(&self, _roots: &BTreeMap<String, PathBuf>, project: Option<&Path>) -> Option<PathBuf> {
        Some(join_relative(project?, &self.path))
    }

    fn home_root(&self) -> Option<&str> {
        None
    }

    fn in_project(&self) -> bool {
        true
    }

    fn machine(&self) -> Option<&str> {
        None
    }

    fn walk(&self) -> ProjectWalk {
        self.walk
    }
}

/// The workspace file a Project is, such as a `.code-workspace` file, read
/// once per such Project of an Agent that opens them.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProjectFileAnchor {}

impl Validate for ProjectFileAnchor {
    fn validate(&self) -> Result<(), String> {
        Ok(())
    }
}

impl Anchored for ProjectFileAnchor {
    /// `project` is the workspace file.
    fn path(&self, _roots: &BTreeMap<String, PathBuf>, project: Option<&Path>) -> Option<PathBuf> {
        project.map(Path::to_path_buf)
    }

    fn home_root(&self) -> Option<&str> {
        None
    }

    fn in_project(&self) -> bool {
        true
    }

    fn machine(&self) -> Option<&str> {
        None
    }

    fn walk(&self) -> ProjectWalk {
        ProjectWalk::None
    }
}

/// A path in the folder the Agent home keeps for each Project, as the
/// definition's `json-folders` registry finds them, such as the
/// `state.vscdb` in each of VS Code's `workspaceStorage\<id>` folders. Its
/// Entries apply in that folder's Project only.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProjectStorageAnchor {
    /// A `/`-separated path relative to the Project's folder of storage.
    pub path: String,
}

impl Validate for ProjectStorageAnchor {
    fn validate(&self) -> Result<(), String> {
        validate_relative(&self.path)
    }
}

impl Anchored for ProjectStorageAnchor {
    /// There is one path per Project, which the scan finds.
    fn path(&self, _roots: &BTreeMap<String, PathBuf>, _project: Option<&Path>) -> Option<PathBuf> {
        None
    }

    fn home_root(&self) -> Option<&str> {
        None
    }

    fn in_project(&self) -> bool {
        false
    }

    fn machine(&self) -> Option<&str> {
        None
    }

    fn walk(&self) -> ProjectWalk {
        ProjectWalk::None
    }
}

/// A fixed path outside any Agent home or Project, read once for every
/// home of the Agent, such as a folder an organisation deploys to every
/// user of the machine, or one in the user's home folder that every home
/// of the Agent reads, such as VS Code's `~/.agents/skills`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct MachineAnchor {
    /// Starts with `~/`, `%NAME%`, expanded from the environment, or a
    /// drive; `/`-separated, without wildcards.
    pub path: String,
}

impl Validate for MachineAnchor {
    fn validate(&self) -> Result<(), String> {
        paths::validate_glob(&self.path)?;
        if self.path.contains(['*', '?', '{', '}']) {
            return Err(format!(
                "machine path `{}` cannot hold wildcards",
                self.path
            ));
        }
        Ok(())
    }
}

impl Anchored for MachineAnchor {
    fn path(&self, roots: &BTreeMap<String, PathBuf>, _project: Option<&Path>) -> Option<PathBuf> {
        roots.get(&self.path).cloned()
    }

    fn home_root(&self) -> Option<&str> {
        None
    }

    fn in_project(&self) -> bool {
        false
    }

    fn machine(&self) -> Option<&str> {
        Some(&self.path)
    }

    fn walk(&self) -> ProjectWalk {
        ProjectWalk::None
    }
}

/// The settings a definition's `[[settings]]` names, in every file of each
/// of its sources. A location reads them all, and lists the Entries of the
/// sources the Agent skips as Unread; a rule or gate reads only the source
/// the Agent reads. Where one file is needed, it is refused.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SettingsAnchor {
    /// The id of a declared `[[settings]]`.
    pub settings: String,
}

impl Validate for SettingsAnchor {
    fn validate(&self) -> Result<(), String> {
        if self.settings.trim().is_empty() {
            return Err("a settings anchor needs a settings id".into());
        }
        Ok(())
    }
}

impl Anchored for SettingsAnchor {
    /// Settings can be in several files, which the scan lists.
    fn path(&self, _roots: &BTreeMap<String, PathBuf>, _project: Option<&Path>) -> Option<PathBuf> {
        None
    }

    fn home_root(&self) -> Option<&str> {
        None
    }

    fn in_project(&self) -> bool {
        false
    }

    fn machine(&self) -> Option<&str> {
        None
    }

    fn walk(&self) -> ProjectWalk {
        ProjectWalk::None
    }
}

/// A path in the folder of each plugin a definition's `[[plugins]]` lists,
/// read once per plugin.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct PluginAnchor {
    /// The id of a declared `[[plugins]]`, or several, such as Cursor's
    /// plugins of a Project and its local ones, which it reads alike.
    pub plugins: PluginIds,
    /// A `/`-separated path relative to the plugin's folder, or several,
    /// of which the first that exists is read, such as the first manifest
    /// Cursor finds of `.cursor-plugin/plugin.json` and
    /// `.claude-plugin/plugin.json`.
    pub path: RelativePaths,
}

impl Validate for PluginAnchor {
    fn validate(&self) -> Result<(), String> {
        let ids = self.plugins.all();
        if ids.is_empty() || ids.iter().any(|id| id.trim().is_empty()) {
            return Err("a plugin anchor needs a plugins id, none of them empty".into());
        }
        if self.path.all().is_empty() {
            return Err("a plugin anchor needs a path".into());
        }
        self.path
            .all()
            .iter()
            .try_for_each(|path| validate_relative(path))
    }
}

impl Anchored for PluginAnchor {
    /// There is one path per plugin, which the scan finds.
    fn path(&self, _roots: &BTreeMap<String, PathBuf>, _project: Option<&Path>) -> Option<PathBuf> {
        None
    }

    fn home_root(&self) -> Option<&str> {
        None
    }

    fn in_project(&self) -> bool {
        false
    }

    fn machine(&self) -> Option<&str> {
        None
    }

    fn walk(&self) -> ProjectWalk {
        ProjectWalk::None
    }
}

impl Anchor {
    /// The id of the `[[settings]]` the anchor reads, if it reads settings.
    pub fn settings(&self) -> Option<&str> {
        match self {
            Anchor::Settings(block) => Some(&block.settings),
            Anchor::Home(_)
            | Anchor::Project(_)
            | Anchor::Machine(_)
            | Anchor::Plugin(_)
            | Anchor::Entry(_)
            | Anchor::ProjectFile(_)
            | Anchor::ProjectStorage(_) => None,
        }
    }

    /// The `[[plugins]]` id and the path in each plugin's folder, if the
    /// anchor is in plugins.
    pub fn plugin(&self) -> Option<&PluginAnchor> {
        match self {
            Anchor::Plugin(block) => Some(block),
            Anchor::Home(_)
            | Anchor::Project(_)
            | Anchor::Machine(_)
            | Anchor::Settings(_)
            | Anchor::Entry(_)
            | Anchor::ProjectFile(_)
            | Anchor::ProjectStorage(_) => None,
        }
    }

    /// Refuses an anchor of settings, of plugins, or with a file per plugin
    /// or per Entry, where one file is needed.
    pub fn one_file(&self) -> Result<(), String> {
        if self.per_plugin() {
            return Err(format!(
                "a path holding `{PLUGIN_KEY}` names a file per plugin; one file is needed here"
            ));
        }
        if self.entry().is_some() {
            return Err("an entry anchor names a file per Entry; one file is needed here".into());
        }
        match (self.settings(), self.plugin()) {
            (Some(_), _) => {
                Err("a settings anchor names several files; one file is needed here".into())
            }
            (_, Some(_)) => {
                Err("a plugin anchor names a file per plugin; one file is needed here".into())
            }
            (None, None) => Ok(()),
        }
    }

    /// Whether it names one file per plugin, by `{plugin_id}` in its path.
    pub fn per_plugin(&self) -> bool {
        match self {
            Anchor::Home(block) => block.per_plugin(),
            Anchor::Project(_)
            | Anchor::Machine(_)
            | Anchor::Settings(_)
            | Anchor::Plugin(_)
            | Anchor::Entry(_)
            | Anchor::ProjectFile(_)
            | Anchor::ProjectStorage(_) => false,
        }
    }

    /// Whether it names the workspace file a Project is, rather than a path
    /// in a Project folder.
    pub fn project_file(&self) -> bool {
        matches!(self, Anchor::ProjectFile(_))
    }

    /// The path in the folder the Agent home keeps for each Project, if it
    /// names one per such folder.
    pub fn project_storage(&self) -> Option<&str> {
        match self {
            Anchor::ProjectStorage(block) => Some(&block.path),
            Anchor::Home(_)
            | Anchor::Project(_)
            | Anchor::Machine(_)
            | Anchor::Settings(_)
            | Anchor::Plugin(_)
            | Anchor::Entry(_)
            | Anchor::ProjectFile(_) => None,
        }
    }

    /// Where the file of each Entry is, if it names one per Entry.
    pub fn entry(&self) -> Option<&EntryAnchor> {
        match self {
            Anchor::Entry(block) => Some(block),
            Anchor::Home(_)
            | Anchor::Project(_)
            | Anchor::Machine(_)
            | Anchor::Settings(_)
            | Anchor::Plugin(_)
            | Anchor::ProjectFile(_)
            | Anchor::ProjectStorage(_) => None,
        }
    }

    /// The file it names for the Entry at `entry`, part of the plugin with
    /// the id `plugin` if any: the Entry's own for an anchor per Entry, its
    /// plugin's for an anchor per plugin, and in `folder`, the Project
    /// folder, for an anchor in a Project.
    pub fn path_for(
        &self,
        roots: &BTreeMap<String, PathBuf>,
        folder: Option<&Path>,
        plugin: Option<&str>,
        entry: &Path,
    ) -> Option<PathBuf> {
        if let Some(beside) = self.entry() {
            return beside.of(entry);
        }
        if self.per_plugin() {
            return self.plugin_path(roots, plugin?);
        }
        self.path(roots, folder)
    }

    /// The file it names for the plugin with the id `plugin`, given the
    /// roots of the Agent home; `None` when it names no file per plugin.
    pub fn plugin_path(&self, roots: &BTreeMap<String, PathBuf>, plugin: &str) -> Option<PathBuf> {
        let Anchor::Home(block) = self else {
            return None;
        };
        let path = block.path.as_deref().filter(|_| block.per_plugin())?;
        Some(join_relative(
            roots.get(&block.root)?,
            &path.replace(PLUGIN_KEY, plugin),
        ))
    }
}

// --- Settings sources -------------------------------------------------------

/// Where an Agent can read one set of its settings, such as the managed
/// settings an organisation deploys.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "block", rename_all = "kebab-case")]
pub(crate) enum SettingsSource {
    RegistryValue(RegistryValueSource),
    RegistryKey(RegistryKeySource),
    Files(FilesSource),
}

impl Validate for SettingsSource {
    fn validate(&self) -> Result<(), String> {
        match self {
            SettingsSource::RegistryValue(block) => block.validate(),
            SettingsSource::RegistryKey(block) => block.validate(),
            SettingsSource::Files(block) => block.validate(),
        }
    }
}

/// The values of a key in the Windows registry, each one setting, such as
/// Claude Desktop's policy under `HKLM\SOFTWARE\Policies\Claude`. A string
/// value reads as a string, and a DWORD as `true` when it is 1 and `false`
/// when it is 0, as Windows policies store switches, and as its number
/// otherwise; a value of another type reads as `null`, holding a setting
/// config-digger does not read. It is shown as `<HKLM|HKCU>\<key>`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct RegistryKeySource {
    pub hive: RegistryHive,
    /// The key below the hive, `\`-separated.
    pub key: String,
}

impl Validate for RegistryKeySource {
    fn validate(&self) -> Result<(), String> {
        validate_registry_key(&self.key)
    }
}

impl RegistryKeySource {
    /// How the key is shown, standing for its path.
    pub fn path(&self) -> PathBuf {
        PathBuf::from(format!(r"{}\{}", self.hive.short(), self.key))
    }

    /// The key's values as the text of one JSON object, `None` when the
    /// key does not exist.
    pub fn text(&self, env: &dyn Environment) -> Option<String> {
        let values = env.registry_values(self.hive.port(), &self.key)?;
        let settings: serde_json::Map<String, Value> = values
            .into_iter()
            .map(|(name, data)| {
                let value = match data {
                    RegistryData::String(text) => Value::from(text),
                    RegistryData::Dword(0) => Value::from(false),
                    RegistryData::Dword(1) => Value::from(true),
                    RegistryData::Dword(number) => Value::from(number),
                    RegistryData::Other => Value::Null,
                };
                (name, value)
            })
            .collect();
        Some(Value::Object(settings).to_string())
    }
}

/// A registry key is `\`-separated below its hive.
fn validate_registry_key(key: &str) -> Result<(), String> {
    if key.trim().is_empty() || key.contains('/') || key.starts_with('\\') {
        return Err(format!(
            "registry key `{key}` must be `\\`-separated below its hive"
        ));
    }
    Ok(())
}

/// A string value in the Windows registry holding JSON, such as Claude
/// Code's `Settings` under `HKLM\SOFTWARE\Policies\ClaudeCode`. It is shown
/// as `<HKLM|HKCU>\<key>\<value>`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct RegistryValueSource {
    pub hive: RegistryHive,
    /// The key below the hive, `\`-separated.
    pub key: String,
    /// The value's name.
    pub value: String,
}

impl Validate for RegistryValueSource {
    fn validate(&self) -> Result<(), String> {
        validate_registry_key(&self.key)?;
        if self.value.trim().is_empty() {
            return Err("a registry value needs a name".into());
        }
        Ok(())
    }
}

impl RegistryValueSource {
    /// The hive as the port knows it.
    pub fn hive(&self) -> Hive {
        self.hive.port()
    }

    /// How the value is shown, standing for its path.
    pub fn path(&self) -> PathBuf {
        PathBuf::from(format!(
            r"{}\{}\{}",
            self.hive.short(),
            self.key,
            self.value
        ))
    }
}

impl RegistryHive {
    /// The hive as the port knows it.
    fn port(self) -> Hive {
        match self {
            RegistryHive::CurrentUser => Hive::CurrentUser,
            RegistryHive::LocalMachine => Hive::LocalMachine,
        }
    }

    /// Its short name, such as `HKLM`.
    fn short(self) -> &'static str {
        match self {
            RegistryHive::CurrentUser => "HKCU",
            RegistryHive::LocalMachine => "HKLM",
        }
    }
}

/// A JSON file on the machine, and the `.json` files of a drop-in folder,
/// by name, read after it, such as Claude Code's `managed-settings.json`
/// and `managed-settings.d`. They are one source.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct FilesSource {
    /// Starts with `%NAME%` or a drive; `/`-separated.
    pub path: String,
    /// The drop-in folder, written as `path` is.
    pub drop_ins: Option<String>,
}

impl Validate for FilesSource {
    fn validate(&self) -> Result<(), String> {
        for path in std::iter::once(&self.path).chain(&self.drop_ins) {
            MachineAnchor { path: path.clone() }.validate()?;
            if path.starts_with("~/") {
                return Err(format!("path `{path}` must start with `%NAME%` or a drive"));
            }
        }
        Ok(())
    }
}

// --- Project registries -----------------------------------------------------

/// Lists the Projects an Agent has recorded, in the Agent's own spelling.
pub(crate) trait Register {
    /// Where the registry lives: a file, or a folder of folders.
    fn anchor(&self) -> &Anchor;

    /// How the registry spells a Project.
    fn spelling(&self) -> Spelling;
}

/// Where an Agent records the Projects it has been started on.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "block", rename_all = "kebab-case")]
pub(crate) enum Registry {
    JsonKeys(JsonKeys),
    JsonFolders(JsonFolders),
    SqliteList(SqliteList),
}

impl Validate for Registry {
    fn validate(&self) -> Result<(), String> {
        match self {
            Registry::JsonKeys(block) => block.validate(),
            Registry::JsonFolders(block) => block.validate(),
            Registry::SqliteList(block) => block.validate(),
        }
    }
}

impl Register for Registry {
    fn anchor(&self) -> &Anchor {
        match self {
            Registry::JsonKeys(block) => block.anchor(),
            Registry::JsonFolders(block) => block.anchor(),
            Registry::SqliteList(block) => block.anchor(),
        }
    }

    fn spelling(&self) -> Spelling {
        match self {
            Registry::JsonKeys(block) => block.spelling(),
            Registry::JsonFolders(block) => block.spelling(),
            Registry::SqliteList(block) => block.spelling(),
        }
    }
}

/// How a registry spells a Project.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Spelling {
    /// A path, such as `E:\x` or `E:/x`.
    #[default]
    Path,
    /// A `file:` URI, such as `file:///e%3A/x`. A URI of any other scheme,
    /// such as a remote folder's, names no Project on this machine.
    FileUri,
}

impl Spelling {
    /// The path `text` spells, `None` when it names none on this machine.
    pub fn path(self, text: &str) -> Option<PathBuf> {
        match self {
            Spelling::Path => Some(PathBuf::from(text)),
            Spelling::FileUri => paths::from_file_uri(text),
        }
    }
}

/// A project registry lies in one file, or one folder, of the Agent home or
/// the machine, such as VS Code's workspace storage, which every profile
/// shares.
fn validate_registry_anchor(anchor: &Anchor) -> Result<(), String> {
    anchor.validate()?;
    anchor.one_file()?;
    if anchor.in_project() {
        return Err("a project registry must not lie in a Project".into());
    }
    if anchor.home_root().is_none() && anchor.machine().is_none() {
        return Err("a project registry must lie in the Agent home or the machine".into());
    }
    Ok(())
}

/// The keys of an object in a strict JSON file, each one a Project.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct JsonKeys {
    /// Where the file is. It must not lie in a Project.
    pub anchor: Anchor,
    /// The key path of the object whose keys are Projects.
    pub root: Vec<String>,
    /// How the keys spell a Project. Omitted for paths.
    #[serde(default)]
    pub spelling: Spelling,
}

impl Validate for JsonKeys {
    fn validate(&self) -> Result<(), String> {
        validate_registry_anchor(&self.anchor)?;
        if self.root.is_empty() {
            return Err("a project registry needs a root key path".into());
        }
        Ok(())
    }
}

impl Register for JsonKeys {
    fn anchor(&self) -> &Anchor {
        &self.anchor
    }

    fn spelling(&self) -> Spelling {
        self.spelling
    }
}

impl JsonKeys {
    /// The Projects the registry's text records, as spelled there.
    pub fn spellings(&self, text: &str) -> Result<Vec<String>, ParseError> {
        let document = JsonFormat::STRICT.parse(text)?;
        let root = self
            .root
            .iter()
            .try_fold(&document, |value, key| value.get(key));
        Ok(match root {
            Some(serde_json::Value::Object(projects)) => projects.keys().cloned().collect(),
            _ => Vec::new(),
        })
    }
}

/// A folder of the Agent home holding a folder per Project, each with a
/// strict JSON file naming its Project in the first of some fields, such as
/// VS Code's `workspaceStorage\<id>\workspace.json`, whose `folder` or
/// `workspace` is the Project. The Agent keeps what it stores for the
/// Project in that folder, which a `project-storage` anchor reads.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct JsonFolders {
    /// Where the folder of folders is. It must not lie in a Project.
    pub anchor: Anchor,
    /// The `/`-separated path of the file in each folder.
    pub file: String,
    /// The fields of the file that can name the Project, tried in turn.
    pub fields: Vec<String>,
    /// How the fields spell a Project. Omitted for paths.
    #[serde(default)]
    pub spelling: Spelling,
}

impl Validate for JsonFolders {
    fn validate(&self) -> Result<(), String> {
        validate_registry_anchor(&self.anchor)?;
        validate_relative(&self.file)?;
        if self.fields.is_empty() || self.fields.iter().any(String::is_empty) {
            return Err("a json-folders registry needs fields, none of them empty".into());
        }
        Ok(())
    }
}

impl Register for JsonFolders {
    fn anchor(&self) -> &Anchor {
        &self.anchor
    }

    fn spelling(&self) -> Spelling {
        self.spelling
    }
}

impl JsonFolders {
    /// The Project the text of one folder's file records, as spelled there.
    pub fn spelling_in(&self, text: &str) -> Result<Option<String>, ParseError> {
        let document = JsonFormat::STRICT.parse(text)?;
        Ok(self
            .fields
            .iter()
            .find_map(|field| document.get(field)?.as_str())
            .map(str::to_owned))
    }
}

/// A list in the JSON text an Agent keeps under a key of the `ItemTable` of
/// a SQLite store, each item naming a Project in the first of some fields,
/// such as Cursor's `workspaceMetadata.entries`, whose `entries` each name
/// their folder in `folderUri`. The store is opened read-only and closed
/// straight after.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SqliteList {
    /// Where the store is. It must not lie in a Project.
    pub anchor: Anchor,
    /// The `ItemTable` key whose value is the JSON text.
    pub key: String,
    /// The key path of the list in the JSON, one key per segment. Omitted
    /// when the JSON is the list.
    #[serde(default)]
    pub list: Vec<String>,
    /// The key paths in an item of the fields that can name its Project,
    /// tried in turn.
    pub fields: Vec<Vec<String>>,
    /// How the fields spell a Project. Omitted for paths.
    #[serde(default)]
    pub spelling: Spelling,
}

impl Validate for SqliteList {
    fn validate(&self) -> Result<(), String> {
        validate_registry_anchor(&self.anchor)?;
        if self.key.trim().is_empty() {
            return Err("a sqlite-list registry needs a key".into());
        }
        if self.list.iter().any(String::is_empty) {
            return Err("a sqlite-list registry's list must have no empty key".into());
        }
        if self.fields.is_empty() {
            return Err("a sqlite-list registry needs fields".into());
        }
        self.fields
            .iter()
            .try_for_each(|field| validate_key_path(field))
    }
}

impl Register for SqliteList {
    fn anchor(&self) -> &Anchor {
        &self.anchor
    }

    fn spelling(&self) -> Spelling {
        self.spelling
    }
}

impl SqliteList {
    /// The Projects the store's text under the key records, as spelled there.
    pub fn spellings(&self, text: &str) -> Result<Vec<String>, ParseError> {
        let document = JsonFormat::STRICT.parse(text)?;
        let items = self
            .list
            .iter()
            .try_fold(&document, |value, key| value.get(key))
            .and_then(Value::as_array);
        Ok(items
            .into_iter()
            .flatten()
            .filter_map(|item| {
                self.fields.iter().find_map(|field| {
                    let value = field.iter().try_fold(item, |value, key| value.get(key))?;
                    value.as_str().map(str::to_owned)
                })
            })
            .collect())
    }
}

// --- Plugin registries ------------------------------------------------------

/// What the Agent records of a plugin it installed, at one Scope.
pub(crate) struct PluginRecord {
    /// The plugin's id, such as `supabase@claude-plugins-official`.
    pub id: String,
    /// The name the Agent gives the plugin, when the registry says it, such
    /// as the name in a Cursor Project's plugin key.
    pub name: Option<String>,
    /// The folder the plugin's files are in, as recorded.
    pub folder: String,
    /// The Scope it is installed at, as recorded.
    pub scope: Option<String>,
    /// The Project it applies in, as recorded; `None` when it applies in every one.
    pub project: Option<String>,
    /// Its version, as recorded.
    pub version: Option<String>,
}

/// Lists the plugins an Agent installed.
pub(crate) trait ListPlugins {
    /// The files and folders of the Agent home the registry reads.
    fn anchors(&self) -> Vec<&Anchor>;
}

/// Where an Agent records the plugins it installed.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "block", rename_all = "kebab-case")]
pub(crate) enum PluginRegistry {
    JsonPluginRecords(JsonPluginRecords),
    PluginFolders(PluginFolders),
    ProjectPluginKeys(ProjectPluginKeys),
}

impl Validate for PluginRegistry {
    fn validate(&self) -> Result<(), String> {
        match self {
            PluginRegistry::JsonPluginRecords(block) => block.validate(),
            PluginRegistry::PluginFolders(block) => block.validate(),
            PluginRegistry::ProjectPluginKeys(block) => block.validate(),
        }
    }
}

impl ListPlugins for PluginRegistry {
    fn anchors(&self) -> Vec<&Anchor> {
        match self {
            PluginRegistry::JsonPluginRecords(block) => block.anchors(),
            PluginRegistry::PluginFolders(block) => block.anchors(),
            PluginRegistry::ProjectPluginKeys(block) => block.anchors(),
        }
    }
}

impl PluginRegistry {
    /// Whether it lies in each Project folder, so that it is read once
    /// every Project is known, and its plugins apply in that Project only.
    pub fn in_project(&self) -> bool {
        matches!(self, PluginRegistry::ProjectPluginKeys(_))
    }
}

/// The keys of an object in a file of each Project folder, each a plugin
/// the Project installs from a marketplace, such as the `plugins` of
/// Cursor's `.cursor/settings.json`. A key is the plugin's name, or its
/// marketplace's and its name joined by `/`, and holds an object; the
/// plugin is named by the name. Its files are in a folder of a cache on
/// the machine, `<marketplace>/<name>/<version>`, the version being a
/// folder that holds the marker. The marketplace of a key that names none
/// is the first, by name, whose cache holds the plugin. A key whose plugin
/// is not in the cache installs nothing on disk yet, and is left out.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProjectPluginKeys {
    /// The file, in the Project folder.
    pub anchor: Anchor,
    /// The dialect the Agent reads the file in. Omitted for strict JSON.
    pub dialect: Option<JsonDialect>,
    /// The key path of the object whose keys are plugins.
    pub root: Vec<String>,
    /// The Scope the plugins are recorded at, such as `project`.
    pub scope: String,
    /// The folder of the cache, on the machine.
    pub cache: Anchor,
    /// A `/`-separated path relative to a version folder whose presence
    /// says the folder is complete, such as `.cache-complete`.
    pub marker: String,
}

/// A plugin a key of a Project's file names.
pub(crate) struct PluginKey {
    /// The key, the plugin's id.
    pub key: String,
    /// The marketplace it names, if it names one.
    pub marketplace: Option<String>,
    /// The plugin's name.
    pub name: String,
}

impl Validate for ProjectPluginKeys {
    fn validate(&self) -> Result<(), String> {
        self.anchor.validate()?;
        self.anchor.one_file()?;
        if !self.anchor.in_project() || self.anchor.project_file() {
            return Err("a Project's plugin keys lie in a file of the Project folder".into());
        }
        if self.anchor.walk() != ProjectWalk::None {
            return Err("a Project's plugin keys cannot walk up from the Project folder".into());
        }
        self.cache.validate()?;
        if self.cache.machine().is_none() {
            return Err("a plugin cache must lie on the machine".into());
        }
        validate_key_path(&self.root)?;
        if self.scope.is_empty() {
            return Err("a Project's plugin keys need a scope".into());
        }
        validate_relative(&self.marker)
    }
}

impl ListPlugins for ProjectPluginKeys {
    fn anchors(&self) -> Vec<&Anchor> {
        vec![&self.anchor, &self.cache]
    }
}

impl ProjectPluginKeys {
    /// How the file is parsed.
    pub fn format(&self) -> JsonFormat {
        JsonFormat::of(self.dialect)
    }

    /// The plugins the file's text names, in its order. A key whose parts
    /// are not each one folder name names none.
    pub fn keys(&self, text: &str) -> Result<Vec<PluginKey>, ParseError> {
        let document = self.format().parse(text)?;
        let root = self
            .root
            .iter()
            .try_fold(&document, |value, key| value.get(key));
        let Some(Value::Object(plugins)) = root else {
            return Ok(Vec::new());
        };
        let one_folder = |part: &str| {
            !part.is_empty() && !matches!(part, "." | "..") && !part.contains(['/', '\\'])
        };
        Ok(plugins
            .iter()
            .filter(|(_, value)| value.is_object())
            .filter_map(|(key, _)| {
                let (marketplace, name) = match key.split_once('/') {
                    Some((marketplace, name)) => (Some(marketplace), name),
                    None => (None, key.as_str()),
                };
                (marketplace.is_none_or(one_folder) && one_folder(name)).then(|| PluginKey {
                    key: key.clone(),
                    marketplace: marketplace.map(str::to_owned),
                    name: name.to_owned(),
                })
            })
            .collect())
    }
}

/// The folders, in a folder of the Agent home or the machine, that hold a
/// marker file, each a plugin whose id is the folder's name, such as Claude
/// Desktop's extensions, each unpacked into `Claude Extensions\<id>` with
/// its `manifest.json`. A folder without a marker holds no plugin. Without
/// a marker, every folder whose name does not start with `.` is a plugin's,
/// such as Cursor's local plugins.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct PluginFolders {
    /// The folder the plugins' folders are in.
    pub anchor: Anchor,
    /// A `/`-separated path relative to a plugin's folder, or several, any
    /// of which makes the folder a plugin's.
    pub marker: Option<RelativePaths>,
    /// What the Agent records of each plugin it installed, when it does.
    pub records: Option<InstallRecords>,
}

/// An object in a strict JSON file of the Agent home whose keys are plugin
/// ids, each holding what the Agent recorded when it installed the plugin,
/// such as Claude Desktop's `extensions-installations.json`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct InstallRecords {
    pub anchor: Anchor,
    /// The key path of the object whose keys are plugin ids.
    pub root: Vec<String>,
    /// The field of a record that holds the version installed.
    pub version: String,
}

impl Validate for PluginFolders {
    fn validate(&self) -> Result<(), String> {
        for anchor in self.anchors() {
            anchor.validate()?;
            anchor.one_file()?;
            if anchor.home_root().is_none() && anchor.machine().is_none() {
                return Err(
                    "plugin folders and their records must lie in the Agent home or the machine"
                        .into(),
                );
            }
        }
        if let Some(marker) = &self.marker {
            if marker.all().is_empty() {
                return Err("a plugin folders marker needs a path".into());
            }
            for marker in marker.all() {
                validate_relative(marker)?;
            }
        }
        if let Some(records) = &self.records {
            validate_key_path(&records.root)?;
            if records.version.is_empty() {
                return Err("a records field must not be empty".into());
            }
        }
        Ok(())
    }
}

/// The id of a declared `[[plugins]]`, or several.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub(crate) enum PluginIds {
    /// The id of one `[[plugins]]`.
    One(String),
    /// Several such ids, in order.
    Several(Vec<String>),
}

impl PluginIds {
    /// Every id, in order.
    pub fn all(&self) -> &[String] {
        match self {
            PluginIds::One(id) => std::slice::from_ref(id),
            PluginIds::Several(ids) => ids,
        }
    }

    /// Whether it names the `[[plugins]]` with the id `id`.
    pub fn names(&self, id: &str) -> bool {
        self.all().iter().any(|known| known == id)
    }
}

/// One `/`-separated relative path, or several.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub(crate) enum RelativePaths {
    /// A `/`-separated relative path.
    One(String),
    /// Several such paths, in order.
    Several(Vec<String>),
}

impl RelativePaths {
    /// Every path, in order.
    pub fn all(&self) -> &[String] {
        match self {
            RelativePaths::One(path) => std::slice::from_ref(path),
            RelativePaths::Several(paths) => paths,
        }
    }
}

impl ListPlugins for PluginFolders {
    fn anchors(&self) -> Vec<&Anchor> {
        std::iter::once(&self.anchor)
            .chain(self.records.as_ref().map(|records| &records.anchor))
            .collect()
    }
}

impl InstallRecords {
    /// The version recorded for each plugin id in the file's text.
    pub fn versions(&self, text: &str) -> Result<BTreeMap<String, String>, ParseError> {
        let document = JsonFormat::STRICT.parse(text)?;
        let root = self
            .root
            .iter()
            .try_fold(&document, |value, key| value.get(key));
        let Some(Value::Object(records)) = root else {
            return Ok(BTreeMap::new());
        };
        Ok(records
            .iter()
            .filter_map(|(id, record)| {
                let version = record.get(&self.version)?.as_str()?;
                Some((id.clone(), version.to_owned()))
            })
            .collect())
    }
}

/// An object in a strict JSON file whose keys are plugin ids, each holding
/// the plugin's records: an array of objects, one per Scope it is
/// installed at, or one object. A record without a folder is skipped.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct JsonPluginRecords {
    /// Where the file is, in the Agent home.
    pub anchor: Anchor,
    /// The key path of the object whose keys are plugin ids.
    pub root: Vec<String>,
    /// The field of a record that holds its folder.
    pub folder: String,
    /// The field of a record that holds the Scope it is installed at.
    pub scope: Option<String>,
    /// The field of a record that holds the path of the one Project it
    /// applies in. Without it, a record applies in every Project.
    pub project: Option<String>,
    /// The field of a record that holds its version.
    pub version: Option<String>,
}

impl Validate for JsonPluginRecords {
    fn validate(&self) -> Result<(), String> {
        self.anchor.validate()?;
        self.anchor.one_file()?;
        if self.anchor.home_root().is_none() && self.anchor.machine().is_none() {
            return Err("a plugin registry must lie in the Agent home or the machine".into());
        }
        if self.root.is_empty() || self.root.iter().any(String::is_empty) {
            return Err("a plugin registry needs a root key path, with no empty key".into());
        }
        for field in std::iter::once(&self.folder)
            .chain(&self.scope)
            .chain(&self.project)
            .chain(&self.version)
        {
            if field.is_empty() {
                return Err("a plugin registry field must not be empty".into());
            }
        }
        Ok(())
    }
}

impl ListPlugins for JsonPluginRecords {
    fn anchors(&self) -> Vec<&Anchor> {
        vec![&self.anchor]
    }
}

impl JsonPluginRecords {
    /// The records the registry's text holds, in its order.
    pub fn records(&self, text: &str) -> Result<Vec<PluginRecord>, ParseError> {
        let document = JsonFormat::STRICT.parse(text)?;
        let root = self
            .root
            .iter()
            .try_fold(&document, |value, key| value.get(key));
        let Some(Value::Object(plugins)) = root else {
            return Ok(Vec::new());
        };
        let text_of = |record: &Value, field: Option<&String>| {
            record.get(field?)?.as_str().map(str::to_owned)
        };
        let mut records = Vec::new();
        for (id, value) in plugins {
            let each = match value {
                Value::Array(each) => each.iter().collect(),
                value => vec![value],
            };
            for record in each {
                let Some(folder) = text_of(record, Some(&self.folder)) else {
                    continue;
                };
                records.push(PluginRecord {
                    id: id.clone(),
                    name: None,
                    folder,
                    scope: text_of(record, self.scope.as_ref()),
                    project: text_of(record, self.project.as_ref()),
                    version: text_of(record, self.version.as_ref()),
                });
            }
        }
        Ok(records)
    }
}

// --- Provenance -------------------------------------------------------------

/// How the Entries of a location got there, which decides how they can be taken away.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Provenance {
    /// A key someone wrote into a configuration file, or a skill folder
    /// someone put there, by hand or through the Agent.
    File,
    /// Configuration an organisation deploys to the machine, such as
    /// enterprise skills. config-digger only reads it.
    Managed,
    /// A copy the Agent keeps of configuration held in the user's account,
    /// such as skills synced from claude.ai. config-digger only reads it.
    CloudMirror,
    /// Part of a plugin the Agent installed. config-digger Disables it only
    /// through the plugin's own switch, with every Entry of the plugin, and
    /// never Removes it: uninstalling is up to the Agent.
    Plugin,
    /// An extension the Agent installed, such as a Claude Desktop `.mcpb`,
    /// read as a plugin is: config-digger Disables it only through its own
    /// switch, and never Removes it.
    Extension,
    /// Shipped with the Agent, which restores it as it ships it, such as
    /// Cursor's built-in skills. config-digger only reads it.
    BuiltIn,
}

impl Provenance {
    /// Whether config-digger only reads the Entries: it never Disables,
    /// Enables or Removes them.
    pub fn read_only(self) -> bool {
        match self {
            Provenance::File | Provenance::Plugin | Provenance::Extension => false,
            Provenance::Managed | Provenance::CloudMirror | Provenance::BuiltIn => true,
        }
    }
}

// --- Store classes ----------------------------------------------------------

/// How a store config-digger writes behaves while its Agent runs, which
/// decides whether a write warns or is blocked then. Declared from the
/// weakest to the strongest: a file several locations read takes the
/// strongest of their classes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum StoreClass {
    /// A file the Agent reads and leaves alone: a write while it runs warns
    /// that it may not see the change until it restarts.
    Plain,
    /// A file the Agent rewrites from memory while it runs, such as
    /// `~/.claude.json`: a write while it runs warns strongly that the Agent
    /// may overwrite it.
    RewrittenWhileRunning,
    /// A SQLite database the Agent holds open: a write while it runs is
    /// blocked, whatever is acknowledged.
    Sqlite,
}

// --- Session files ----------------------------------------------------------

/// Lists the PIDs an Agent records for its running sessions.
pub(crate) trait Sessions {
    /// The PIDs the session files name, alive or not.
    fn pids(&self, fs: &dyn FileSystem, env: &dyn Environment) -> Vec<u32>;
}

/// Files an Agent keeps for each running session, naming its PID.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "block", rename_all = "kebab-case")]
pub(crate) enum SessionFiles {
    PidNamed(PidNamed),
}

impl Validate for SessionFiles {
    fn validate(&self) -> Result<(), String> {
        match self {
            SessionFiles::PidNamed(block) => block.validate(),
        }
    }
}

impl Sessions for SessionFiles {
    fn pids(&self, fs: &dyn FileSystem, env: &dyn Environment) -> Vec<u32> {
        match self {
            SessionFiles::PidNamed(block) => block.pids(fs, env),
        }
    }
}

/// One file per session directly in a folder, named `<pid>.<extension>`,
/// such as Claude Code's `~/.claude/sessions/<pid>.json`. Only the names are
/// read, never the content, which can hold a Secret.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct PidNamed {
    pub folder: Locator,
    /// Such as `json`, without the dot.
    pub extension: String,
}

impl Validate for PidNamed {
    fn validate(&self) -> Result<(), String> {
        self.folder.validate()?;
        if self.extension.is_empty() || self.extension.contains(['.', '/', '\\']) {
            return Err(format!(
                "extension `{}` must be a name without a dot or separator",
                self.extension
            ));
        }
        Ok(())
    }
}

impl Sessions for PidNamed {
    fn pids(&self, fs: &dyn FileSystem, env: &dyn Environment) -> Vec<u32> {
        let Some(folder) = self.folder.folders(env, fs).into_iter().next() else {
            return Vec::new();
        };
        let suffix = format!(".{}", self.extension.to_ascii_lowercase());
        fs.read_dir(&folder)
            .unwrap_or_default()
            .iter()
            .filter_map(|path| {
                let name = path.file_name()?.to_str()?.to_ascii_lowercase();
                let pid = name.strip_suffix(&suffix)?;
                if pid.is_empty() || !pid.bytes().all(|byte| byte.is_ascii_digit()) {
                    return None;
                }
                pid.parse().ok()
            })
            .collect()
    }
}

// --- Formats ----------------------------------------------------------------

/// Why a format could not read a location's text: where, and nothing of
/// what the text holds there, since parser messages can quote a Secret.
#[derive(Debug)]
pub(crate) struct ParseError {
    /// The 1-based line and column, when the parser knows them.
    pub position: Option<(usize, usize)>,
}

/// Parses a location's text in the reader's dialect.
pub(crate) trait Parse {
    fn parse(&self, text: &str) -> Result<serde_json::Value, ParseError>;

    /// The dialect, as a sentence names it, such as "strict JSON".
    fn dialect(&self) -> &'static str;

    /// The syntax a location in this format is edited in.
    fn syntax(&self) -> Syntax;
}

/// The format a location is read in: a file in a dialect, or a folder of
/// skill folders.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "block", rename_all = "kebab-case")]
pub(crate) enum Format {
    Json(JsonFormat),
    Toml(TomlFormat),
    SkillFolder(SkillFolder),
    SkillList(SkillList),
    ManifestSkills(ManifestSkills),
    SqliteJson(SqliteJson),
}

impl Validate for Format {
    fn validate(&self) -> Result<(), String> {
        match self {
            Format::Json(block) => block.validate(),
            Format::Toml(block) => block.validate(),
            Format::SkillFolder(block) => block.validate(),
            Format::SkillList(block) => block.validate(),
            Format::ManifestSkills(block) => block.validate(),
            Format::SqliteJson(block) => block.validate(),
        }
    }
}

impl Format {
    /// How a file location is parsed, `None` for a folder of skill folders
    /// or a SQLite store.
    pub fn file(&self) -> Option<&dyn Parse> {
        match self {
            Format::Json(block) => Some(block),
            Format::Toml(block) => Some(block),
            Format::SkillFolder(_)
            | Format::SkillList(_)
            | Format::ManifestSkills(_)
            | Format::SqliteJson(_) => None,
        }
    }

    /// How a value in a SQLite store is read, `None` for any other location.
    pub fn sqlite(&self) -> Option<&SqliteJson> {
        match self {
            Format::SqliteJson(block) => Some(block),
            Format::Json(_)
            | Format::Toml(_)
            | Format::SkillFolder(_)
            | Format::SkillList(_)
            | Format::ManifestSkills(_) => None,
        }
    }

    /// How a folder of skill folders, or each skill folder a list names, is
    /// read; `None` for a file location.
    pub fn skill_folder(&self) -> Option<&SkillFolder> {
        match self {
            Format::SkillFolder(block) => Some(block),
            Format::SkillList(block) => Some(&block.skills),
            Format::ManifestSkills(block) => Some(&block.skills),
            Format::Json(_) | Format::Toml(_) | Format::SqliteJson(_) => None,
        }
    }
}

/// The skills a plugin's manifest declares in a field,
/// a path relative to the plugin's folder or a list of them, such as the
/// `skills` of a Cursor plugin's `plugin.json`. A path is a folder of skill
/// folders; an item of a list may instead name one skill folder or its
/// `SKILL.md`. A manifest that declares none, or a plugin with no
/// manifest, has its skills in the `default` folder instead. Only a
/// location in plugins, anchored at the manifest, reads them.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ManifestSkills {
    /// The key path of the field in the strict JSON manifest.
    pub field: Vec<String>,
    /// A `/`-separated path relative to the plugin's folder.
    pub default: String,
    /// How each folder of skill folders is read.
    pub skills: SkillFolder,
}

impl Validate for ManifestSkills {
    fn validate(&self) -> Result<(), String> {
        validate_key_path(&self.field)?;
        validate_relative(&self.default)?;
        self.skills.validate()
    }
}

/// A path a plugin's manifest declares skills at, relative to the plugin's
/// folder.
pub(crate) struct DeclaredSkills {
    pub path: String,
    /// Whether it may name one skill folder, or its `SKILL.md`, rather than
    /// a folder of skill folders, as an item of a list may.
    pub may_name_one: bool,
}

impl ManifestSkills {
    /// The paths the manifest's text `manifest` declares, or else the
    /// default folder when it declares none. A manifest that does not
    /// parse declares none; a field that is neither a path nor a list of
    /// paths declares no skills.
    pub fn paths(&self, manifest: Option<&str>) -> Vec<DeclaredSkills> {
        let document = manifest.and_then(|text| JsonFormat::STRICT.parse(text).ok());
        let declared = document.as_ref().and_then(|document| {
            self.field
                .iter()
                .try_fold(document, |value, key| value.get(key))
        });
        let folder = |path: &str, may_name_one| DeclaredSkills {
            path: path.to_owned(),
            may_name_one,
        };
        match declared {
            None => vec![folder(&self.default, false)],
            Some(Value::String(path)) => vec![folder(path, false)],
            Some(Value::Array(paths)) => paths
                .iter()
                .filter_map(|path| Some(folder(path.as_str()?, true)))
                .collect(),
            Some(_) => Vec::new(),
        }
    }
}

/// The JSON text an Agent keeps under a key of the `ItemTable` of a SQLite
/// store, such as VS Code's cache of extension servers under
/// `mcp.extCachedServers` in a workspace's `state.vscdb`. The store is
/// opened read-only, waiting briefly for an Agent that holds a write lock,
/// and closed straight after. The location's `root` starts with the key,
/// and config-digger never writes its Entries.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SqliteJson {
    /// The `ItemTable` key whose value is the JSON text.
    pub key: String,
}

impl Validate for SqliteJson {
    fn validate(&self) -> Result<(), String> {
        if self.key.trim().is_empty() {
            return Err("a sqlite-json format needs a key".into());
        }
        Ok(())
    }
}

impl SqliteJson {
    /// The store's value under the key, `text`, as a document holding it
    /// under the key.
    pub fn parse(&self, text: &str) -> Result<Value, ParseError> {
        let value = JsonFormat::STRICT.parse(text)?;
        Ok(Value::Object(serde_json::Map::from_iter([(
            self.key.clone(),
            value,
        )])))
    }
}

/// A folder whose subfolders holding a `SKILL.md` are skills, each one an
/// Entry. A folder whose name starts with `.` is never read, and one
/// without `SKILL.md` is not an Entry.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SkillFolder {
    /// Whether a subfolder without `SKILL.md` is read in turn, at any depth.
    /// A skill folder's own subfolders never are.
    #[serde(default)]
    pub recursive: bool,
    /// Where a skill's name comes from.
    pub name_from: NameFrom,
    /// How the Agent rewrites a name into the one it uses, if it does.
    pub sanitize: Option<Sanitize>,
    /// Folder names the Agent never loads a skill from, compared
    /// case-insensitively, such as a name it keeps for itself.
    #[serde(default)]
    pub reserved: Vec<String>,
    /// What the Agent does with a `SKILL.md` that does not open with a
    /// `---` frontmatter.
    pub no_frontmatter: SkillFallback,
    /// What the Agent does with a frontmatter that is not YAML it reads.
    pub malformed_frontmatter: SkillFallback,
    /// What the Agent does with a frontmatter without a `name`, when
    /// `name_from` is `frontmatter`.
    pub no_name: SkillFallback,
}

/// The skill folders a list in a strict JSON file names, each by a path
/// relative to the file's folder to the skill folder or its `SKILL.md`, such
/// as the `contributes.chatSkills` of a VS Code extension's `package.json`.
/// Each is read as a folder of skill folders reads its skills.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SkillList {
    /// The key path of the list.
    pub list: Vec<String>,
    /// The field of each item that holds the path.
    pub field: String,
    /// How each skill folder is read.
    pub skills: SkillFolder,
}

impl Validate for SkillList {
    fn validate(&self) -> Result<(), String> {
        validate_key_path(&self.list)?;
        if self.field.is_empty() {
            return Err("a skill-list field must not be empty".into());
        }
        if self.skills.recursive {
            return Err("a skill list names each skill folder; it cannot be recursive".into());
        }
        self.skills.validate()
    }
}

/// Where a skill's name comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum NameFrom {
    /// The skill folder's name.
    Folder,
    /// The `name` of the `SKILL.md` frontmatter.
    Frontmatter,
}

/// How an Agent rewrites a skill's name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Sanitize {
    /// Lower case, each run of characters other than `a`-`z` and `0`-`9`
    /// replaced by one `-`, with no `-` at either end.
    LowercaseHyphens,
}

/// What an Agent does with a skill whose `SKILL.md` it cannot fully read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum SkillFallback {
    /// It skips the skill, which is Invalid.
    Skip,
    /// It loads the skill under its folder's name, with no other
    /// frontmatter field: Effective, with a note.
    FolderName,
}

/// Why an Agent may not read a skill's `SKILL.md` as it is written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum SkillDefect {
    /// It does not open with a `---` frontmatter.
    NoFrontmatter,
    /// Its frontmatter is not YAML the Agent reads.
    MalformedFrontmatter,
    /// Its frontmatter has no `name`, which the Agent names it by.
    NoName,
    /// Its folder has a name the Agent keeps for itself.
    Reserved,
}

impl Validate for SkillFolder {
    fn validate(&self) -> Result<(), String> {
        if let Some(name) = self
            .reserved
            .iter()
            .find(|name| name.is_empty() || name.contains(['/', '\\']))
        {
            return Err(format!("reserved name `{name}` must be one folder name"));
        }
        Ok(())
    }
}

impl SkillFolder {
    /// What the Agent does with a skill with `defect`.
    pub fn fallback(&self, defect: SkillDefect) -> SkillFallback {
        match defect {
            SkillDefect::NoFrontmatter => self.no_frontmatter,
            SkillDefect::MalformedFrontmatter => self.malformed_frontmatter,
            SkillDefect::NoName => self.no_name,
            SkillDefect::Reserved => SkillFallback::Skip,
        }
    }

    /// Whether the Agent keeps the folder name `folder` for itself.
    pub fn is_reserved(&self, folder: &str) -> bool {
        self.reserved
            .iter()
            .any(|name| name.eq_ignore_ascii_case(folder))
    }

    /// The name `name` as the Agent uses it.
    pub fn sanitize(&self, name: &str) -> String {
        match self.sanitize {
            None => name.to_owned(),
            Some(Sanitize::LowercaseHyphens) => {
                let mut sanitized = String::new();
                for c in name.chars().flat_map(char::to_lowercase) {
                    if c.is_ascii_lowercase() || c.is_ascii_digit() {
                        sanitized.push(c);
                    } else if !sanitized.is_empty() && !sanitized.ends_with('-') {
                        sanitized.push('-');
                    }
                }
                sanitized.trim_end_matches('-').to_owned()
            }
        }
    }
}

/// JSON, in the dialect the Agent reads it in.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct JsonFormat {
    pub dialect: JsonDialect,
}

/// How much a reader accepts beyond strict JSON. A file one reader
/// rejects can still yield Entries for a reader with a looser dialect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum JsonDialect {
    /// No comments, no trailing commas.
    Strict,
    /// `//` and `/* */` comments, and no trailing comma, such as Gemini
    /// CLI's settings.
    WithComments,
    /// Comments and trailing commas, such as Cursor's and VS Code's files.
    Jsonc,
}

impl Validate for JsonFormat {
    fn validate(&self) -> Result<(), String> {
        Ok(())
    }
}

impl JsonFormat {
    /// Strict JSON, as a registry or a Disable switch's file is read.
    pub const STRICT: JsonFormat = JsonFormat {
        dialect: JsonDialect::Strict,
    };

    /// The dialect `dialect` names, or else strict JSON.
    pub fn of(dialect: Option<JsonDialect>) -> JsonFormat {
        JsonFormat {
            dialect: dialect.unwrap_or(JsonDialect::Strict),
        }
    }
}

impl Parse for JsonFormat {
    fn parse(&self, text: &str) -> Result<serde_json::Value, ParseError> {
        let trailing_commas = match self.dialect {
            JsonDialect::Strict => {
                return serde_json::from_str(text).map_err(|error| ParseError {
                    position: (error.line() > 0).then(|| (error.line(), error.column())),
                });
            }
            JsonDialect::WithComments => false,
            JsonDialect::Jsonc => true,
        };
        let options = jsonc_parser::ParseOptions {
            allow_comments: true,
            allow_trailing_commas: trailing_commas,
            allow_loose_object_property_names: false,
            allow_missing_commas: false,
            allow_single_quoted_strings: false,
            allow_hexadecimal_numbers: false,
            allow_unary_plus_numbers: false,
            allow_bare_decimal_point_numbers: false,
            allow_non_finite_numbers: false,
            allow_extended_string_escapes: false,
        };
        let parsed: Option<serde_json::Value> = jsonc_parser::parse_to_serde_value(text, &options)
            .map_err(|error| ParseError {
                position: (error.line_display() > 0)
                    .then(|| (error.line_display(), error.column_display())),
            })?;
        // An empty file holds no JSON value.
        parsed.ok_or(ParseError { position: None })
    }

    fn dialect(&self) -> &'static str {
        match self.dialect {
            JsonDialect::Strict => "strict JSON",
            JsonDialect::WithComments => "JSON with comments",
            JsonDialect::Jsonc => "JSONC with trailing commas",
        }
    }

    /// JSON in every dialect is edited as JSONC, which accepts it as it is.
    fn syntax(&self) -> Syntax {
        Syntax::Jsonc
    }
}

/// TOML.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct TomlFormat {}

impl Validate for TomlFormat {
    fn validate(&self) -> Result<(), String> {
        Ok(())
    }
}

impl Parse for TomlFormat {
    fn parse(&self, text: &str) -> Result<serde_json::Value, ParseError> {
        toml::from_str(text).map_err(|error| ParseError {
            position: error.span().map(|span| line_and_column(text, span.start)),
        })
    }

    fn dialect(&self) -> &'static str {
        "TOML"
    }

    fn syntax(&self) -> Syntax {
        Syntax::Toml
    }
}

/// The 1-based line and column, in characters, of the byte offset `at` in `text`.
fn line_and_column(text: &str, at: usize) -> (usize, usize) {
    let before = &text[..at.min(text.len())];
    let line_start = before.rfind('\n').map_or(0, |newline| newline + 1);
    (
        before.matches('\n').count() + 1,
        before[line_start..].chars().count() + 1,
    )
}

// --- Interpolation ----------------------------------------------------------

/// One run of a value's text: a reference the Agent replaces with a value
/// it reads elsewhere, or literal text.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Run<'t> {
    pub text: &'t str,
    pub reference: bool,
}

/// Splits a value into references and literal text.
pub(crate) trait Interpolate {
    /// The runs of `text`, in order. Together they are exactly `text`.
    fn runs<'t>(&self, text: &'t str) -> Vec<Run<'t>>;
}

/// How an Agent writes a reference to a value it reads elsewhere, such as an
/// environment variable. A value made only of references is not a Secret.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "block", rename_all = "kebab-case")]
pub(crate) enum Interpolation {
    Braces(Braces),
}

impl Validate for Interpolation {
    fn validate(&self) -> Result<(), String> {
        match self {
            Interpolation::Braces(block) => block.validate(),
        }
    }
}

impl Interpolate for Interpolation {
    fn runs<'t>(&self, text: &'t str) -> Vec<Run<'t>> {
        match self {
            Interpolation::Braces(block) => block.runs(text),
        }
    }
}

/// `${NAME}`, where `NAME` is a letter or `_` followed by letters, digits
/// and `_`, or one of `names` when they are set, plus
/// `${namespace:argument}` for each listed namespace, and
/// `${NAME<separator>default}` when a default separator is set, whose
/// default is literal text.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Braces {
    /// Such as `env` and `input` for `${env:NAME}` and `${input:id}`.
    #[serde(default)]
    pub namespaces: Vec<String>,
    /// What comes between a namespace and its argument, such as the `.` of
    /// a Claude Desktop extension's `${user_config.KEY}`. Omitted for `:`.
    pub namespace_separator: Option<String>,
    /// Such as `:-` for `${NAME:-default}`.
    pub default_separator: Option<String>,
    /// The only names `${NAME}` stands for, such as VS Code's
    /// `workspaceFolder` and `userHome`, or Cursor's `/`. Omitted for any
    /// name.
    pub names: Option<Vec<String>>,
}

impl Validate for Braces {
    fn validate(&self) -> Result<(), String> {
        if let Some(namespace) = self.namespaces.iter().find(|ns| !is_name(ns)) {
            return Err(format!("namespace `{namespace}` is not a name"));
        }
        if self.default_separator.as_deref() == Some("") {
            return Err("default_separator must not be empty".into());
        }
        if self.namespace_separator.as_deref() == Some("") {
            return Err("namespace_separator must not be empty".into());
        }
        if let Some(name) = self
            .names
            .iter()
            .flatten()
            .find(|name| name.is_empty() || name.contains(['{', '}', ':']))
        {
            return Err(format!("`{name}` is not a name"));
        }
        Ok(())
    }
}

impl Interpolate for Braces {
    fn runs<'t>(&self, text: &'t str) -> Vec<Run<'t>> {
        let mut runs = Vec::new();
        let mut literal = 0;
        let mut at = 0;
        while let Some(found) = text[at..].find("${") {
            let start = at + found;
            let Some(close) = text[start..].find('}').map(|close| start + close) else {
                break;
            };
            let inner = &text[start + 2..close];
            let reference = match self.default_separator.as_deref() {
                Some(separator) => match inner.split_once(separator) {
                    Some((name, _)) if self.is_name(name) => {
                        Some(start + 2 + name.len() + separator.len())
                    }
                    _ => None,
                },
                None => None,
            };
            match reference {
                // `${NAME<separator>` and `}` are references; the default between them is not.
                Some(default) => {
                    push_run(&mut runs, &text[literal..start], false);
                    push_run(&mut runs, &text[start..default], true);
                    push_run(&mut runs, &text[default..close], false);
                    push_run(&mut runs, &text[close..close + 1], true);
                    literal = close + 1;
                }
                None if self.is_reference(inner) => {
                    push_run(&mut runs, &text[literal..start], false);
                    push_run(&mut runs, &text[start..close + 1], true);
                    literal = close + 1;
                }
                None => {}
            }
            at = close + 1;
        }
        push_run(&mut runs, &text[literal..], false);
        runs
    }
}

impl Braces {
    fn is_reference(&self, inner: &str) -> bool {
        match inner.split_once(self.namespace_separator.as_deref().unwrap_or(":")) {
            Some((namespace, argument)) => {
                self.namespaces.iter().any(|ns| ns == namespace) && !argument.is_empty()
            }
            None => self.is_name(inner),
        }
    }

    /// Whether `${name}` stands for a value.
    fn is_name(&self, name: &str) -> bool {
        match &self.names {
            Some(names) => names.iter().any(|known| known == name),
            None => is_name(name),
        }
    }
}

/// Appends a non-empty run.
fn push_run<'t>(runs: &mut Vec<Run<'t>>, text: &'t str, reference: bool) {
    if !text.is_empty() {
        runs.push(Run { text, reference });
    }
}

/// A letter or `_`, then letters, digits and `_`.
fn is_name(text: &str) -> bool {
    let mut chars = text.chars();
    chars
        .next()
        .is_some_and(|first| first.is_ascii_alphabetic() || first == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

// --- Competition rules ------------------------------------------------------

/// Decides which Entries of one Agent home compete as the same Item.
pub(crate) trait Compete {
    /// Entries with equal keys are the same Item: this is the key of the
    /// Entry named `name` at `path`.
    fn key(&self, name: &str, path: &Path) -> String;

    /// Whether the Entries of the location `id` compete by endpoint
    /// instead of by name.
    fn by_endpoint(&self, id: &str) -> bool;
}

/// How same-kind Entries of one Agent home compete.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "block", rename_all = "kebab-case")]
pub(crate) enum Competition {
    ExactName(ExactName),
    CaseInsensitiveName(CaseInsensitiveName),
    PathKey(PathKey),
    SameUrl(SameUrl),
}

impl Validate for Competition {
    fn validate(&self) -> Result<(), String> {
        match self {
            Competition::ExactName(block) => block.validate(),
            Competition::CaseInsensitiveName(block) => block.validate(),
            Competition::PathKey(block) => block.validate(),
            Competition::SameUrl(block) => block.validate(),
        }
    }
}

impl Compete for Competition {
    fn key(&self, name: &str, path: &Path) -> String {
        match self {
            Competition::ExactName(block) => block.key(name, path),
            Competition::CaseInsensitiveName(block) => block.key(name, path),
            Competition::PathKey(block) => block.key(name, path),
            Competition::SameUrl(block) => block.key(name, path),
        }
    }

    fn by_endpoint(&self, id: &str) -> bool {
        match self {
            Competition::ExactName(block) => block.by_endpoint(id),
            Competition::CaseInsensitiveName(block) => block.by_endpoint(id),
            Competition::PathKey(block) => block.by_endpoint(id),
            Competition::SameUrl(block) => block.by_endpoint(id),
        }
    }
}

impl Competition {
    /// The ids of the locations whose Entries compete by endpoint.
    pub fn endpoint_locations(&self) -> &[String] {
        match self {
            Competition::ExactName(block) => &block.by_endpoint,
            Competition::CaseInsensitiveName(_)
            | Competition::PathKey(_)
            | Competition::SameUrl(_) => &[],
        }
    }

    /// The setting Entries compete only while it holds, if any: while it
    /// does not, no two Entries compete.
    pub fn only_while(&self) -> Option<&SettingIs> {
        match self {
            Competition::ExactName(_) | Competition::PathKey(_) | Competition::SameUrl(_) => None,
            Competition::CaseInsensitiveName(block) => block.only_while.as_ref(),
        }
    }

    /// How remote Entries also compete by URL, if they do.
    pub fn same_url(&self) -> Option<&SameUrl> {
        match self {
            Competition::SameUrl(block) => Some(block),
            Competition::ExactName(_)
            | Competition::CaseInsensitiveName(_)
            | Competition::PathKey(_) => None,
        }
    }
}

/// Entries compete when their names are equal without regard to case, as
/// VS Code compares server labels, and with `only_while`, only while a
/// setting has a value, such as VS Code's `chat.mcp.collisionBehavior`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct CaseInsensitiveName {
    /// The setting Entries compete only while it holds. Omitted, they
    /// always do.
    pub only_while: Option<SettingIs>,
}

impl Validate for CaseInsensitiveName {
    fn validate(&self) -> Result<(), String> {
        match &self.only_while {
            Some(setting) => setting.validate().map_err(|e| format!("only_while: {e}")),
            None => Ok(()),
        }
    }
}

impl Compete for CaseInsensitiveName {
    fn key(&self, name: &str, _path: &Path) -> String {
        name.to_lowercase()
    }

    fn by_endpoint(&self, _id: &str) -> bool {
        false
    }
}

/// Whether a setting the Agent reads in layers has a value: the first of
/// its sources that holds it decides, and the Agent's default stands when
/// none does.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SettingIs {
    /// Where the setting is, highest first, such as a folder's settings
    /// before the user's.
    pub sources: Vec<SettingPlace>,
    /// The value it must have.
    pub equals: Value,
    /// The value the Agent takes when no source holds the setting.
    pub default: Value,
}

impl Validate for SettingIs {
    fn validate(&self) -> Result<(), String> {
        validate_sources(&self.sources)
    }
}

impl SettingIs {
    /// Whether it is met, given what each source holds, highest first.
    pub fn is_met(&self, mut values: impl Iterator<Item = Option<Value>>) -> bool {
        values
            .find_map(|value| value)
            .as_ref()
            .unwrap_or(&self.default)
            == &self.equals
    }
}

impl Holds for SettingIs {
    fn holds(
        &self,
        _gate_on: &dyn Fn(&str) -> bool,
        setting: &dyn Fn(usize) -> Option<Value>,
    ) -> bool {
        self.is_met((0..self.sources.len()).map(setting))
    }

    fn gate(&self) -> Option<&str> {
        None
    }
}

/// Entries compete when their names are exactly equal. An Entry of a
/// `by_endpoint` location competes instead with the Entries of other
/// locations that have its endpoint, joining the Item of the one the Agent
/// ranks highest, and otherwise with those of `by_endpoint` locations that
/// have it.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExactName {
    /// The ids of the locations whose Entries compete by endpoint, such as
    /// Claude Code's plugin servers.
    #[serde(default)]
    pub by_endpoint: Vec<String>,
}

impl Validate for ExactName {
    fn validate(&self) -> Result<(), String> {
        Ok(())
    }
}

impl Compete for ExactName {
    fn key(&self, name: &str, _path: &Path) -> String {
        name.to_owned()
    }

    fn by_endpoint(&self, id: &str) -> bool {
        self.by_endpoint.iter().any(|known| known == id)
    }
}

/// Entries compete when they lie at the same path below the same folder,
/// whatever their names, once each path is cut at the first of `markers` it
/// passes through, as Cursor compares skills: `~\.claude\skills\review` and
/// `~\.agents\skills\review` both lie at `skills\review` below `~`, while
/// `~\.claude\skills\x\review` does not. Paths compare without regard to
/// case. An Entry whose path passes through no marker competes with none.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct PathKey {
    /// Folder names, such as `.cursor` and `.claude`.
    pub markers: Vec<String>,
}

impl Validate for PathKey {
    fn validate(&self) -> Result<(), String> {
        if self.markers.is_empty() {
            return Err("a path-key competition needs markers".into());
        }
        if let Some(marker) = self
            .markers
            .iter()
            .find(|marker| marker.is_empty() || marker.contains(['/', '\\']))
        {
            return Err(format!("marker `{marker}` must be one folder name"));
        }
        Ok(())
    }
}

impl Compete for PathKey {
    fn key(&self, _name: &str, path: &Path) -> String {
        let components: Vec<_> = path.components().collect();
        let cut = components.iter().position(|component| {
            let name = component.as_os_str().to_string_lossy();
            self.markers
                .iter()
                .any(|marker| marker.eq_ignore_ascii_case(&name))
        });
        let Some(cut) = cut else {
            return paths::key(path);
        };
        let root: PathBuf = components[..cut].iter().collect();
        let below: PathBuf = components[cut + 1..].iter().collect();
        format!("{}::{}", paths::key(&root), paths::key(&below))
    }

    fn by_endpoint(&self, _id: &str) -> bool {
        false
    }
}

/// Entries compete when their names are exactly equal, except that, while
/// the Feature gate `gate` is on, the remote Entries of `locations` compete
/// by URL instead, as Cursor runs one server per URL: the one the Agent ranks
/// highest wins. A remote Entry has a `url` and no `command`.
/// URLs compare as WHATWG URLs, by scheme, host and port, path without a
/// trailing `/`, and query, a URL without a scheme taking `http://`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SameUrl {
    /// The ids of the locations whose remote Entries compete by URL.
    pub locations: Vec<String>,
    /// The name of a declared Feature gate, which must list `locations` in
    /// its `affects`.
    pub gate: String,
    /// What else takes part that config-digger cannot see, which every
    /// Entry that competes by URL notes.
    #[serde(default)]
    pub unseen: Vec<UnseenServers>,
}

/// Servers an Agent counts that config-digger cannot see.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum UnseenServers {
    /// Those extensions give the Agent while it runs.
    Extensions,
    /// Those the user's team configures for them.
    Team,
    /// Those of the plugins the user installs from a marketplace, which
    /// the Agent keeps in the user's account.
    MarketplacePlugins,
    /// The one the user picked among those with the same URL.
    UserPick,
}

impl Validate for SameUrl {
    fn validate(&self) -> Result<(), String> {
        if self.locations.is_empty() {
            return Err("a same-url competition needs locations".into());
        }
        if self.gate.trim().is_empty() {
            return Err("a same-url competition needs a gate".into());
        }
        Ok(())
    }
}

impl Compete for SameUrl {
    fn key(&self, name: &str, _path: &Path) -> String {
        name.to_owned()
    }

    fn by_endpoint(&self, _id: &str) -> bool {
        false
    }
}

impl SameUrl {
    /// The URL an Entry of the location `id` whose value is `config`
    /// competes by, `None` for one that does not compete by URL.
    pub fn url_of(&self, id: &str, config: &Value) -> Option<String> {
        if !self.locations.iter().any(|known| known == id) {
            return None;
        }
        if truthy(config, "command") {
            return None;
        }
        let url = config.get("url")?.as_str().filter(|url| !url.is_empty())?;
        Some(whatwg_url(url))
    }
}

/// `url` as Cursor compares it, given `http://` when it names no scheme: an
/// `http`, `https`, `ws` or `wss` URL's host in lower case,
/// without user info, the scheme's default port or a fragment, and with no
/// `/` ending its path; any other URL in lower case.
fn whatwg_url(url: &str) -> String {
    let url = with_scheme(url);
    let (scheme, rest) = url.split_once("://").expect("a scheme is given");
    let default_port = match scheme {
        "http" | "ws" => ":80",
        "https" | "wss" => ":443",
        _ => return url.trim().to_lowercase(),
    };
    let rest = rest.split('#').next().unwrap_or_default();
    let (authority, rest) = rest.split_at(rest.find(['/', '?']).unwrap_or(rest.len()));
    let host = authority
        .rsplit_once('@')
        .map_or(authority, |(_, host)| host);
    let host = host.to_ascii_lowercase();
    let host = host.strip_suffix(default_port).unwrap_or(&host);
    let (path, query) = rest.split_at(rest.find('?').unwrap_or(rest.len()));
    let path = match path.strip_suffix('/') {
        Some(trimmed) if !trimmed.is_empty() => trimmed,
        _ if path == "/" => "",
        _ => path,
    };
    format!("{scheme}://{host}{path}{query}")
}

/// A server's endpoint as an Agent compares it: its URL with the scheme
/// and host in lower case, without the scheme's default port or a
/// trailing slash on its path; or else its command and args. `None` for
/// an Entry with neither.
pub(crate) fn endpoint(config: &Value) -> Option<String> {
    if let Some(url) = config.get("url").and_then(Value::as_str) {
        return Some(format!("url {}", normal_url(url)));
    }
    let command = config.get("command").and_then(Value::as_str)?;
    let args = config
        .get("args")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|arg| arg.as_str().map_or_else(|| arg.to_string(), str::to_owned));
    let words: Vec<String> = std::iter::once(command.to_owned()).chain(args).collect();
    Some(format!("command {words:?}"))
}

/// `url` with the scheme and host in lower case, without the scheme's
/// default port or a trailing slash on its path.
fn normal_url(url: &str) -> String {
    let Some((scheme, rest)) = url.split_once("://") else {
        return url.to_owned();
    };
    let scheme = scheme.to_ascii_lowercase();
    let (authority, rest) = rest.split_at(rest.find(['/', '?', '#']).unwrap_or(rest.len()));
    let (userinfo, host) = match authority.rsplit_once('@') {
        Some((userinfo, host)) => (format!("{userinfo}@"), host),
        None => (String::new(), authority),
    };
    let mut host = host.to_ascii_lowercase();
    let default_port = match scheme.as_str() {
        "http" | "ws" => Some(":80"),
        "https" | "wss" => Some(":443"),
        _ => None,
    };
    if let Some(port) = default_port
        && let Some(without) = host.strip_suffix(port)
    {
        host = without.to_owned();
    }
    let (path, tail) = rest.split_at(rest.find(['?', '#']).unwrap_or(rest.len()));
    let path = path.trim_end_matches('/');
    format!("{scheme}://{userinfo}{host}{path}{tail}")
}

// --- Shapes -----------------------------------------------------------------

/// Why an Entry does not have the minimal shape its Agent requires.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum Malformed {
    /// The Entry is not an object.
    NotAnObject,
    /// It holds none of these fields.
    Missing(Vec<String>),
    /// This field holds another type than the Agent reads.
    WrongType(String, FieldType),
}

/// Checks an Entry against the minimal shape its Agent requires. Keys the
/// shape does not name are never flagged.
pub(crate) trait Conform {
    /// Why `value` does not conform, if it does not.
    fn malformed(&self, value: &Value) -> Option<Malformed>;

    /// Whether an Entry that does not conform still competes by name, as
    /// when the Agent lists it and fails to start it.
    fn still_competes(&self) -> bool;
}

/// The minimal shape of the Entries of a location.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "block", rename_all = "kebab-case")]
pub(crate) enum Shape {
    Fields(FieldShape),
}

impl Validate for Shape {
    fn validate(&self) -> Result<(), String> {
        match self {
            Shape::Fields(block) => block.validate(),
        }
    }
}

impl Conform for Shape {
    fn malformed(&self, value: &Value) -> Option<Malformed> {
        match self {
            Shape::Fields(block) => block.malformed(value),
        }
    }

    fn still_competes(&self) -> bool {
        match self {
            Shape::Fields(block) => block.still_competes(),
        }
    }
}

/// An object that holds at least one of some fields, each field it holds
/// being of the type the Agent reads.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct FieldShape {
    /// The Entry must hold at least one of these, such as `command` or `url`.
    #[serde(default)]
    pub one_of: Vec<String>,
    /// The type of each field, when the Entry holds it.
    #[serde(default)]
    pub types: BTreeMap<String, FieldType>,
    /// Whether an Entry without this shape still competes by name.
    #[serde(default)]
    pub still_competes: bool,
}

/// The type of a field's value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum FieldType {
    String,
    Number,
    Boolean,
    Array,
    Object,
    /// A list of strings.
    StringArray,
    /// An object whose values are strings.
    StringMap,
}

impl FieldType {
    fn holds(self, value: &Value) -> bool {
        match self {
            FieldType::String => value.is_string(),
            FieldType::Number => value.is_number(),
            FieldType::Boolean => value.is_boolean(),
            FieldType::Array => value.is_array(),
            FieldType::Object => value.is_object(),
            FieldType::StringArray => value
                .as_array()
                .is_some_and(|items| items.iter().all(Value::is_string)),
            FieldType::StringMap => value
                .as_object()
                .is_some_and(|object| object.values().all(Value::is_string)),
        }
    }
}

impl Validate for FieldShape {
    fn validate(&self) -> Result<(), String> {
        if let Some(field) = self
            .one_of
            .iter()
            .chain(self.types.keys())
            .find(|field| field.is_empty())
        {
            return Err(format!("a shape field `{field}` must not be empty"));
        }
        if self.one_of.is_empty() && self.types.is_empty() {
            return Err("a shape needs one_of or types".into());
        }
        Ok(())
    }
}

impl Conform for FieldShape {
    fn malformed(&self, value: &Value) -> Option<Malformed> {
        let Value::Object(object) = value else {
            return Some(Malformed::NotAnObject);
        };
        if !self.one_of.is_empty() && !self.one_of.iter().any(|field| object.contains_key(field)) {
            return Some(Malformed::Missing(self.one_of.clone()));
        }
        self.types.iter().find_map(|(field, &kind)| {
            let value = object.get(field)?;
            (!kind.holds(value)).then(|| Malformed::WrongType(field.clone(), kind))
        })
    }

    fn still_competes(&self) -> bool {
        self.still_competes
    }
}

// --- Disable mechanisms -----------------------------------------------------

/// Disables Entries through a switch the Agent itself reads to skip an
/// Entry while keeping it.
pub(crate) trait Disables {
    /// Where the switch is written.
    fn switch(&self) -> &Switch;

    /// What one write of the switch Disables.
    fn target(&self) -> MechanismTarget;

    /// Where the switch applies.
    fn context(&self) -> MechanismContext;

    /// The reason code an Entry Disabled through it gets.
    fn reason(&self) -> DisableReason;

    /// The ids of the locations whose Entries it Disables, empty for
    /// every location of the Item kind.
    /// Whether config-digger writes the switch: Disable through it, and
    /// Enable lifting it.
    fn enable_offered(&self) -> bool;

    /// Whether its switch is one layer of a setting the Agent merges across
    /// files: among the layered mechanisms of the Item kind, in the
    /// definition's order, the first whose switch holds a value for the
    /// Entry decides, and the others are ignored.
    fn layered(&self) -> bool;

    fn locations(&self) -> &[String];

    /// The Scopes a plugin must be installed at for the switch to apply to
    /// its Entries, as the Agent records them; empty for any.
    fn plugin_scopes(&self) -> &[String] {
        &[]
    }

    /// Whether it applies to the Entries of a plugin installed at `scope`,
    /// or to an Entry of no plugin.
    fn covers_plugin(&self, scope: Option<&str>) -> bool {
        let scopes = self.plugin_scopes();
        scopes.is_empty() || scope.is_some_and(|scope| scopes.iter().any(|known| known == scope))
    }

    /// Whether it Disables the Entries of the location `id`.
    fn covers(&self, id: &str) -> bool {
        let locations = self.locations();
        locations.is_empty() || locations.iter().any(|location| location == id)
    }

    /// Whether `value`, what the switch holds for the Entry named `name`,
    /// Disables it. `None` when the switch holds nothing for it.
    fn disables(&self, value: Option<&Value>, name: &str) -> bool;

    /// Whether `value` is a partial switch: it limits part of the Entry,
    /// leaving it Effective.
    fn partial(&self, value: Option<&Value>) -> bool;

    /// The edits that Disable the Entry held by `name` through the switch
    /// at `key_path`, which holds `value` for it now.
    fn disable_edits(&self, key_path: Vec<String>, value: Option<&Value>, name: &str) -> Vec<Edit>;

    /// When the value where `{name}` stands is a list rather than an object
    /// keyed by name, what of each item holds its name: a field, or an
    /// index in a list of lists.
    fn named_by(&self) -> Option<&str> {
        self.switch().named_by.as_deref()
    }

    /// Whether Enable takes the switch away rather than setting it to a
    /// value that Enables, so that a later layer then decides.
    fn enable_unsets(&self) -> bool;

    /// Whether it holds an Entry only by the id its location's `switch_id`
    /// makes, so that it cannot hold one of a location without it.
    fn by_id(&self) -> bool {
        false
    }

    /// Whether the Agent compares the ids of a list named by a field
    /// without regard to case.
    fn ignores_case(&self) -> bool {
        false
    }

    /// The edits that lift the switch at `key_path`, which holds `value`
    /// for the Entry named `name`.
    fn enable_edits(&self, key_path: Vec<String>, value: Option<&Value>, name: &str) -> Vec<Edit>;
}

/// How an Agent Disables an Entry. An Item kind lists its mechanisms in
/// the Agent's order of preference.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "block", rename_all = "kebab-case")]
pub(crate) enum DisableMechanism {
    JsonNameList(JsonNameList),
    JsonField(Box<JsonField>),
    JsonItemList(JsonItemList),
    Park(Park),
}

impl Validate for DisableMechanism {
    fn validate(&self) -> Result<(), String> {
        match self {
            DisableMechanism::JsonNameList(block) => block.validate(),
            DisableMechanism::JsonField(block) => block.validate(),
            DisableMechanism::JsonItemList(block) => block.validate(),
            DisableMechanism::Park(block) => block.validate(),
        }
    }
}

impl DisableMechanism {
    /// The mechanism as a switch the Agent reads, `None` for parking.
    pub fn switched(&self) -> Option<Switched<'_>> {
        match self {
            DisableMechanism::JsonNameList(block) => Some(Switched::NameList(block)),
            DisableMechanism::JsonField(block) => Some(Switched::Field(block)),
            DisableMechanism::JsonItemList(block) => Some(Switched::ItemList(block)),
            DisableMechanism::Park(_) => None,
        }
    }

    /// The switch it writes, `None` for parking.
    pub fn switch(&self) -> Option<&Switch> {
        match self {
            DisableMechanism::JsonNameList(block) => Some(&block.switch),
            DisableMechanism::JsonField(block) => Some(&block.switch),
            DisableMechanism::JsonItemList(block) => Some(&block.switch),
            DisableMechanism::Park(_) => None,
        }
    }

    /// Whether it parks the Entries it covers rather than writing a switch.
    pub fn parks(&self) -> bool {
        matches!(self, DisableMechanism::Park(_))
    }

    /// The ids of the locations whose Entries it Disables, empty for every
    /// location of the Item kind.
    pub fn locations(&self) -> &[String] {
        match self {
            DisableMechanism::JsonNameList(block) => block.locations(),
            DisableMechanism::JsonField(block) => block.locations(),
            DisableMechanism::JsonItemList(block) => block.locations(),
            DisableMechanism::Park(block) => &block.locations,
        }
    }

    /// Whether it Disables the Entries of the location `id`.
    pub fn covers(&self, id: &str) -> bool {
        let locations = self.locations();
        locations.is_empty() || locations.iter().any(|location| location == id)
    }
}

/// A Disable mechanism that writes a switch the Agent reads.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Switched<'m> {
    NameList(&'m JsonNameList),
    Field(&'m JsonField),
    ItemList(&'m JsonItemList),
}

impl<'m> Switched<'m> {
    /// The mechanism's own block.
    fn block(self) -> &'m dyn Disables {
        match self {
            Switched::NameList(block) => block,
            Switched::Field(block) => block,
            Switched::ItemList(block) => block,
        }
    }
}

impl Disables for Switched<'_> {
    fn switch(&self) -> &Switch {
        self.block().switch()
    }

    fn target(&self) -> MechanismTarget {
        self.block().target()
    }

    fn context(&self) -> MechanismContext {
        self.block().context()
    }

    fn reason(&self) -> DisableReason {
        self.block().reason()
    }

    fn locations(&self) -> &[String] {
        self.block().locations()
    }

    fn enable_offered(&self) -> bool {
        self.block().enable_offered()
    }

    fn layered(&self) -> bool {
        self.block().layered()
    }

    fn disables(&self, value: Option<&Value>, name: &str) -> bool {
        self.block().disables(value, name)
    }

    fn partial(&self, value: Option<&Value>) -> bool {
        self.block().partial(value)
    }

    fn disable_edits(&self, key_path: Vec<String>, value: Option<&Value>, name: &str) -> Vec<Edit> {
        self.block().disable_edits(key_path, value, name)
    }

    fn enable_edits(&self, key_path: Vec<String>, value: Option<&Value>, name: &str) -> Vec<Edit> {
        self.block().enable_edits(key_path, value, name)
    }

    fn named_by(&self) -> Option<&str> {
        self.block().named_by()
    }

    fn ignores_case(&self) -> bool {
        self.block().ignores_case()
    }

    fn enable_unsets(&self) -> bool {
        self.block().enable_unsets()
    }

    fn by_id(&self) -> bool {
        self.block().by_id()
    }

    fn plugin_scopes(&self) -> &[String] {
        self.block().plugin_scopes()
    }
}

/// Disables an Entry that has no switch of its own, such as a server in
/// Claude Desktop's user file, by moving it out of the Agent's reach into
/// config-digger's parked-Entry store, where it stays listed as Disabled;
/// Enable puts it back (ADR 0002). Disable prefers any switch to it, and
/// never parks an Entry in a git-versioned file.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Park {
    /// The ids of the locations whose Entries it parks: each a file outside
    /// any plugin, holding Entries under keys.
    pub locations: Vec<String>,
}

impl Validate for Park {
    fn validate(&self) -> Result<(), String> {
        if self.locations.is_empty() {
            return Err("park needs its locations listed".into());
        }
        Ok(())
    }
}

/// What one write of a mechanism's switch Disables. Declared from the most
/// specific to the widest: Disable prefers the most specific mechanism.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum MechanismTarget {
    /// The one Entry.
    Entry,
    /// Every Entry of the Item it covers, since they share the name.
    Name,
    /// Every Entry of the plugin the Entry is part of, of every Item kind.
    /// The switch's key path names the plugin with `{plugin_id}`, and it
    /// covers locations in plugins only.
    Plugin,
}

/// Where a mechanism's switch applies. Declared from the most specific to
/// the widest: Disable prefers the most specific mechanism.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum MechanismContext {
    /// In one Project: the switch is in a Project folder, or keyed by `{project}`.
    Project,
    /// In every context.
    All,
}

/// What Enable writes to lift a mechanism's switch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum EnableAction {
    /// Removes the name from the list.
    RemoveItem,
    /// Deletes the key.
    DeleteKey,
    /// Sets the key to the mechanism's `enabled_value`.
    Set,
}

/// The reason code an Entry Disabled through a mechanism gets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DisableReason {
    /// The switch names the Item, so every Entry of it that the mechanism
    /// covers is Disabled.
    DisabledByName,
    /// The switch is the Entry's own.
    DisabledNatively,
    /// The switch is that of the plugin the Entry is part of, so every
    /// Entry of the plugin is Disabled.
    PluginDisabled,
}

/// A list of names in a JSON file: the Agent skips every Entry of an Item
/// whose name is in it. Disable appends the name, creating the list and
/// any missing parent; Enable removes it.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct JsonNameList {
    /// The list.
    pub switch: Switch,
    /// Must be `name`.
    pub target: MechanismTarget,
    pub context: MechanismContext,
    /// Must be `remove-item`.
    pub enable_action: EnableAction,
    pub reason: DisableReason,
    /// Whether config-digger writes the switch: Disable through it, and
    /// Enable lifting it. Off for a switch it only reads, such as one in a
    /// file shared with a team or applying to every Project.
    #[serde(default = "offered")]
    pub enable_offered: bool,
    /// The ids of the locations whose Entries it Disables. Omitted, it
    /// Disables Entries of every location of the Item kind.
    #[serde(default)]
    pub locations: Vec<String>,
}

impl Validate for JsonNameList {
    fn validate(&self) -> Result<(), String> {
        validate_mechanism(&self.switch, self.target, self.context)?;
        if self.switch.named_by.is_some() {
            return Err("a name list takes no named_by".into());
        }
        if self.switch.names() {
            return Err(format!(
                "a name list's key path cannot hold `{NAME_KEY}`: the names are its items"
            ));
        }
        if self.target != MechanismTarget::Name {
            return Err("a name list's target must be `name`".into());
        }
        if self.enable_action != EnableAction::RemoveItem {
            return Err("a name list's enable_action must be `remove-item`".into());
        }
        Ok(())
    }
}

impl Disables for JsonNameList {
    fn switch(&self) -> &Switch {
        &self.switch
    }

    fn target(&self) -> MechanismTarget {
        self.target
    }

    fn context(&self) -> MechanismContext {
        self.context
    }

    fn reason(&self) -> DisableReason {
        self.reason
    }

    fn locations(&self) -> &[String] {
        &self.locations
    }
    fn enable_offered(&self) -> bool {
        self.enable_offered
    }

    /// Enable removes the name.
    fn enable_unsets(&self) -> bool {
        true
    }

    /// The Agent concatenates name lists rather than layering them.
    fn layered(&self) -> bool {
        false
    }

    fn disables(&self, value: Option<&Value>, name: &str) -> bool {
        occurrences(value, name) > 0
    }

    fn partial(&self, _value: Option<&Value>) -> bool {
        false
    }

    /// An append, which creates the list when it is missing.
    fn disable_edits(
        &self,
        key_path: Vec<String>,
        _value: Option<&Value>,
        name: &str,
    ) -> Vec<Edit> {
        vec![Edit::Append {
            key_path,
            value: Value::from(name),
        }]
    }

    fn enable_edits(&self, key_path: Vec<String>, value: Option<&Value>, name: &str) -> Vec<Edit> {
        (0..occurrences(value, name))
            .map(|_| Edit::RemoveItem {
                key_path: key_path.clone(),
                value: Value::from(name),
            })
            .collect()
    }
}

/// What `enable_offered` is when a definition leaves it out.
fn offered() -> bool {
    true
}

/// How many times the list `value` holds `name`.
fn occurrences(value: Option<&Value>, name: &str) -> usize {
    value.and_then(Value::as_array).map_or(0, |names| {
        names
            .iter()
            .filter(|item| item.as_str() == Some(name))
            .count()
    })
}

/// A field in a JSON file whose value says whether the Agent skips an
/// Entry, such as `enabled: false`. Disable sets it to `disabled_value`,
/// adding it and any missing parent; Enable deletes it or sets it to
/// `enabled_value`, as `enable_action` says.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct JsonField {
    /// The field. Its key path may hold `{name}`, the Entry's name.
    pub switch: Switch,
    pub target: MechanismTarget,
    pub context: MechanismContext,
    /// `delete-key` or `set`.
    pub enable_action: EnableAction,
    pub reason: DisableReason,
    /// The value that Disables the Entry.
    pub disabled_value: Value,
    /// The value Enable sets, with `enable_action = "set"`.
    pub enabled_value: Option<Value>,
    /// The value the Agent takes while the field, or its file, is missing,
    /// such as `false` for Claude Desktop's `isEnabled`, which an extension
    /// without a settings file has. Of layered fields, the Agent takes the
    /// last one's while no layer holds a value. Omitted, a missing field
    /// Disables nothing.
    pub default_value: Option<Value>,
    /// Values that are partial switches, limiting part of the Entry, such
    /// as a skill listed by name only: the Entry stays Effective, with a note.
    #[serde(default)]
    pub partial_values: Vec<Value>,
    /// Whether config-digger writes the switch: Disable through it, and
    /// Enable lifting it. Off for a switch it only reads, such as one in a
    /// file shared with a team or applying to every Project.
    #[serde(default = "offered")]
    pub enable_offered: bool,
    /// Whether the field is one layer of a setting the Agent merges across
    /// files, such as Claude Code's `enabledPlugins`: among the layered
    /// mechanisms of the Item kind, in the definition's order, the first
    /// whose field holds a value for the Entry decides.
    #[serde(default)]
    pub layered: bool,
    /// The ids of the locations whose Entries it Disables. Omitted, it
    /// Disables Entries of every location of the Item kind.
    #[serde(default)]
    pub locations: Vec<String>,
    /// With target `plugin`, the Scopes a plugin must be installed at for
    /// the field to apply to it, as the Agent records them, such as Cursor
    /// reading a user install's `enabledPlugins` in the user settings alone.
    /// Omitted, it applies to every plugin.
    #[serde(default)]
    pub plugin_scopes: Vec<String>,
}

impl Validate for JsonField {
    fn validate(&self) -> Result<(), String> {
        validate_mechanism(&self.switch, self.target, self.context)?;
        if !self.plugin_scopes.is_empty() && self.target != MechanismTarget::Plugin {
            return Err("plugin_scopes needs target `plugin`".into());
        }
        if self.plugin_scopes.iter().any(String::is_empty) {
            return Err("a plugin scope must not be empty".into());
        }
        if self.switch.named_by.is_some() && self.enable_offered {
            return Err(
                "a switch named_by a list item is only read: enable_offered must be false".into(),
            );
        }
        match (self.enable_action, &self.enabled_value) {
            (EnableAction::RemoveItem, _) => {
                Err("a field's enable_action must be `delete-key` or `set`".into())
            }
            (EnableAction::Set, None) => Err("enable_action `set` needs an enabled_value".into()),
            (EnableAction::DeleteKey, Some(_)) => {
                Err("enable_action `delete-key` takes no enabled_value".into())
            }
            (EnableAction::Set, Some(enabled)) if *enabled == self.disabled_value => {
                Err("enabled_value must differ from disabled_value".into())
            }
            _ if self.partial_values.contains(&self.disabled_value) => {
                Err("partial_values must not hold disabled_value".into())
            }
            _ => Ok(()),
        }
    }
}

impl Disables for JsonField {
    fn switch(&self) -> &Switch {
        &self.switch
    }

    fn plugin_scopes(&self) -> &[String] {
        &self.plugin_scopes
    }

    fn target(&self) -> MechanismTarget {
        self.target
    }

    fn context(&self) -> MechanismContext {
        self.context
    }

    fn reason(&self) -> DisableReason {
        self.reason
    }

    fn locations(&self) -> &[String] {
        &self.locations
    }
    fn enable_offered(&self) -> bool {
        self.enable_offered
    }

    fn enable_unsets(&self) -> bool {
        self.enable_action != EnableAction::Set
    }

    fn layered(&self) -> bool {
        self.layered
    }

    fn disables(&self, value: Option<&Value>, _name: &str) -> bool {
        value.or(self.default_value.as_ref()) == Some(&self.disabled_value)
    }

    fn partial(&self, value: Option<&Value>) -> bool {
        value
            .or(self.default_value.as_ref())
            .is_some_and(|value| self.partial_values.contains(value))
    }

    fn disable_edits(
        &self,
        key_path: Vec<String>,
        _value: Option<&Value>,
        _name: &str,
    ) -> Vec<Edit> {
        vec![Edit::Set {
            key_path,
            value: self.disabled_value.clone(),
        }]
    }

    fn enable_edits(
        &self,
        key_path: Vec<String>,
        _value: Option<&Value>,
        _name: &str,
    ) -> Vec<Edit> {
        vec![match (&self.enabled_value, self.enable_action) {
            (Some(value), EnableAction::Set) => Edit::Set {
                key_path,
                value: value.clone(),
            },
            _ => Edit::Remove { key_path },
        }]
    }
}

/// A list in a JSON value holding an item per Entry it switches, each named
/// by its `named_by` field or index, such as the `[id, enabled]` pairs of
/// VS Code's `mcp.enablement` or the skill files of its
/// `chat.disabledPromptFiles.skill`. An Entry is held by the id its
/// location's `switch_id` makes, and the switch's key path ends in
/// `{name}`, standing for its item. The Agent skips an Entry whose item is
/// there and is not `enabled_item`. Disable replaces the item with
/// `disabled_item`, creating the list and any missing parent; Enable takes
/// it away, or with `set`, replaces it with `enabled_item`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct JsonItemList {
    /// The list, `{name}` standing for the Entry's item in it.
    pub switch: Switch,
    /// Must be `entry`.
    pub target: MechanismTarget,
    pub context: MechanismContext,
    /// `remove-item`, or `set` with an `enabled_item`.
    pub enable_action: EnableAction,
    pub reason: DisableReason,
    /// The item Disable writes, each string `{name}` in it standing for the
    /// Entry's id, such as `["{name}", false]`.
    pub disabled_item: Value,
    /// The item that Enables the Entry, likewise, such as `["{name}", true]`,
    /// which beats an item of a later layer. Enable writes it with
    /// `enable_action = "set"`.
    pub enabled_item: Option<Value>,
    /// Whether the Agent compares ids without regard to case, as it
    /// compares Windows paths.
    #[serde(default)]
    pub ignore_case: bool,
    /// Whether config-digger writes the switch: Disable through it, and
    /// Enable lifting it.
    #[serde(default = "offered")]
    pub enable_offered: bool,
    /// Whether the list is one layer of a setting the Agent merges across
    /// stores, such as VS Code's `mcp.enablement` in a workspace's store over
    /// the profile's: among the layered mechanisms of the Item kind, in the
    /// definition's order, the first whose list holds the Entry decides.
    #[serde(default)]
    pub layered: bool,
    /// The ids of the locations whose Entries it Disables. Omitted, it
    /// Disables Entries of every location of the Item kind.
    #[serde(default)]
    pub locations: Vec<String>,
}

impl Validate for JsonItemList {
    fn validate(&self) -> Result<(), String> {
        validate_mechanism(&self.switch, self.target, self.context)?;
        if self.switch.named_by.is_none() {
            return Err("an item list needs named_by, what of each item holds its id".into());
        }
        if self.switch.key_path.last().map(String::as_str) != Some(NAME_KEY) {
            return Err(format!("an item list's key path ends in `{NAME_KEY}`"));
        }
        if self.target != MechanismTarget::Entry {
            return Err("an item list's target must be `entry`".into());
        }
        match (self.enable_action, &self.enabled_item) {
            (_, Some(enabled)) if *enabled == self.disabled_item => {
                Err("enabled_item must differ from disabled_item".into())
            }
            (EnableAction::RemoveItem, _) | (EnableAction::Set, Some(_)) => Ok(()),
            _ => Err(
                "an item list's enable_action is `remove-item`, or `set` with an enabled_item"
                    .into(),
            ),
        }
    }
}

impl JsonItemList {
    /// `template` with each string `{name}` in it replaced by `id`.
    fn item(template: &Value, id: &str) -> Value {
        match template {
            Value::String(text) if text == NAME_KEY => Value::from(id),
            Value::Array(items) => items.iter().map(|item| Self::item(item, id)).collect(),
            Value::Object(fields) => fields
                .iter()
                .map(|(key, value)| (key.clone(), Self::item(value, id)))
                .collect(),
            other => other.clone(),
        }
    }

    /// The edits that replace `item`, if the list holds one, with `with`, if
    /// any: `key_path` names the item, and the list is its parent.
    fn replace(key_path: &[String], item: Option<&Value>, with: Option<Value>) -> Vec<Edit> {
        let list = key_path[..key_path.len() - 1].to_vec();
        let removed = item.map(|item| Edit::RemoveItem {
            key_path: list.clone(),
            value: item.clone(),
        });
        let added = with.map(|value| Edit::Append {
            key_path: list,
            value,
        });
        removed.into_iter().chain(added).collect()
    }
}

impl Disables for JsonItemList {
    fn switch(&self) -> &Switch {
        &self.switch
    }

    fn target(&self) -> MechanismTarget {
        self.target
    }

    fn context(&self) -> MechanismContext {
        self.context
    }

    fn reason(&self) -> DisableReason {
        self.reason
    }

    fn locations(&self) -> &[String] {
        &self.locations
    }

    fn enable_offered(&self) -> bool {
        self.enable_offered
    }

    fn layered(&self) -> bool {
        self.layered
    }

    fn disables(&self, value: Option<&Value>, name: &str) -> bool {
        let enabled = self
            .enabled_item
            .as_ref()
            .map(|template| Self::item(template, name));
        value.is_some_and(|item| Some(item) != enabled.as_ref())
    }

    fn partial(&self, _value: Option<&Value>) -> bool {
        false
    }

    fn disable_edits(&self, key_path: Vec<String>, value: Option<&Value>, name: &str) -> Vec<Edit> {
        let disabled = Self::item(&self.disabled_item, name);
        Self::replace(&key_path, value, Some(disabled))
    }

    fn enable_edits(&self, key_path: Vec<String>, value: Option<&Value>, name: &str) -> Vec<Edit> {
        let enabled = (self.enable_action == EnableAction::Set)
            .then_some(self.enabled_item.as_ref())
            .flatten()
            .map(|template| Self::item(template, name));
        Self::replace(&key_path, value, enabled)
    }

    fn ignores_case(&self) -> bool {
        self.ignore_case
    }

    fn enable_unsets(&self) -> bool {
        self.enable_action != EnableAction::Set
    }

    fn by_id(&self) -> bool {
        true
    }
}

/// Checks what every mechanism shares: its switch, a target that agrees
/// with what the switch is keyed by, and a context that agrees with where
/// the switch is.
fn validate_mechanism(
    switch: &Switch,
    target: MechanismTarget,
    context: MechanismContext,
) -> Result<(), String> {
    switch.validate()?;
    if (target == MechanismTarget::Plugin) != switch.plugins() {
        return Err(format!(
            "a switch key path holds `{PLUGIN_KEY}` exactly when the target is `plugin`"
        ));
    }
    match (context, switch.per_project()) {
        (MechanismContext::Project, false) => Err(format!(
            "context `project` needs a switch in a Project or keyed by `{}`",
            super::PROJECT_KEY
        )),
        (MechanismContext::All, true) => Err(format!(
            "context `all` needs a switch outside any Project and not keyed by `{}`",
            super::PROJECT_KEY
        )),
        _ => Ok(()),
    }
}

/// The key a mechanism writes, in a JSON file outside or inside a Project.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Switch {
    /// The file. Its path may name `{plugin_id}` instead of the key path,
    /// for a file per plugin.
    pub anchor: Anchor,
    /// The key path of the value written. One segment may be `{project}`:
    /// the context's Project key, in the Agent's own spelling, so that the
    /// switch applies only there. One segment after it may be `{name}`,
    /// the Entry's name, or `{plugin_id}`, the id of the plugin it is part of.
    pub key_path: Vec<String>,
    /// When the value where `{name}` stands is a list of objects rather
    /// than an object keyed by name, the field of each that holds its name,
    /// such as `name` in the `skills` of the manifest of Claude Desktop's
    /// skill mirror. config-digger only reads such a switch.
    pub named_by: Option<String>,
    /// How the switch's file behaves while the Agent runs. A `sqlite` file
    /// is a SQLite store, such as VS Code's `state.vscdb`, whose `ItemTable`
    /// holds JSON text under the key path's first key.
    pub store_class: StoreClass,
    /// The dialect the Agent reads the file in, such as Cursor's JSONC
    /// `.cursor/settings.json`. Omitted for strict JSON.
    pub dialect: Option<JsonDialect>,
}

/// The switch `key_path` segment that stands for the Entry's name.
pub(crate) const NAME_KEY: &str = "{name}";

/// The switch `key_path` segment that stands for the id of the plugin the
/// Entry is part of.
pub(crate) const PLUGIN_KEY: &str = "{plugin_id}";

/// Whether a switch `key_path` segment stands for what the switch holds an
/// Entry by: its name, or its plugin's id.
pub(crate) fn is_entry_key(key: &str) -> bool {
    key == NAME_KEY || key == PLUGIN_KEY
}

impl Validate for Switch {
    fn validate(&self) -> Result<(), String> {
        self.anchor.validate()?;
        if !self.anchor.per_plugin() && self.anchor.entry().is_none() {
            self.anchor.one_file()?;
        } else if self.key_path.iter().any(|key| key == PLUGIN_KEY) {
            return Err(format!(
                "a switch holds `{PLUGIN_KEY}` in its file's path or its key path, not both"
            ));
        }
        if self.key_path.is_empty() || self.key_path.iter().any(String::is_empty) {
            return Err("a switch needs a key path, with no empty key".into());
        }
        if self.sqlite()
            && (is_entry_key(&self.key_path[0]) || self.key_path[0] == super::PROJECT_KEY)
        {
            return Err("a SQLite switch's key path starts with a key of its `ItemTable`".into());
        }
        if let Some(field) = &self.named_by {
            if field.is_empty() {
                return Err("named_by must not be empty".into());
            }
            if !self.names() {
                return Err(format!("named_by needs `{NAME_KEY}` in the key path"));
            }
        }
        let position = |template: &str| self.key_path.iter().position(|key| key == template);
        let count = |template: &str| self.key_path.iter().filter(|key| *key == template).count();
        for template in [super::PROJECT_KEY, NAME_KEY, PLUGIN_KEY] {
            if count(template) > 1 {
                return Err(format!("a switch key path holds `{template}` twice"));
            }
        }
        if self.names() && self.plugins() {
            return Err(format!(
                "a switch key path cannot hold both `{NAME_KEY}` and `{PLUGIN_KEY}`"
            ));
        }
        if self.anchor.walk() != ProjectWalk::None {
            return Err("a switch cannot walk up from the Project folder".into());
        }
        if position(super::PROJECT_KEY).is_some() && self.anchor.in_project() {
            return Err(format!(
                "a switch in a Project cannot be keyed by `{}`",
                super::PROJECT_KEY
            ));
        }
        let entry = self.key_path.iter().position(|key| is_entry_key(key));
        if let (Some(project), Some(entry)) = (position(super::PROJECT_KEY), entry)
            && entry < project
        {
            return Err(format!(
                "`{}` must come before `{NAME_KEY}` or `{PLUGIN_KEY}` in a switch key path",
                super::PROJECT_KEY
            ));
        }
        Ok(())
    }
}

impl Switch {
    /// Whether its file is a SQLite store, whose `ItemTable` holds JSON text
    /// under the key path's first key.
    pub fn sqlite(&self) -> bool {
        self.store_class == StoreClass::Sqlite
    }

    /// How the file is parsed.
    pub fn format(&self) -> JsonFormat {
        JsonFormat::of(self.dialect)
    }

    /// Whether the switch applies in one Project only, so that it needs one.
    pub fn per_project(&self) -> bool {
        self.anchor.in_project()
            || self.anchor.project_storage().is_some()
            || self.key_path.iter().any(|key| key == super::PROJECT_KEY)
    }

    /// Whether its key path holds `{name}`, so that it holds one value per name.
    pub fn names(&self) -> bool {
        self.key_path.iter().any(|key| key == NAME_KEY)
    }

    /// Whether its key path, or its file's path, holds `{plugin_id}`, so
    /// that it holds one value per plugin.
    pub fn plugins(&self) -> bool {
        self.key_path.iter().any(|key| key == PLUGIN_KEY) || self.anchor.per_plugin()
    }
}

// --- Settings read by rules and gates ---------------------------------------

/// A value an Agent reads from one of its settings files, such as the list
/// a rule holds. config-digger only reads it.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SettingPlace {
    pub anchor: Anchor,
    /// The key path of the value. One segment may be `{project}`: the
    /// context's Project key, in the Agent's own spelling, so that the
    /// value applies only there.
    pub key_path: Vec<String>,
    /// The value is managed policy: what it keeps out is Blocked, by
    /// `policy`, rather than Disabled.
    #[serde(default)]
    pub policy: bool,
    /// The dialect the Agent reads the file in. Omitted for strict JSON.
    pub dialect: Option<JsonDialect>,
    /// Whether the file is a SQLite store, such as VS Code's `state.vscdb`,
    /// whose `ItemTable` holds JSON text under the key path's first key.
    #[serde(default)]
    pub sqlite: bool,
    /// The key path, in the same file, of when the Agent cached the value:
    /// milliseconds since 1970, or an RFC 3339 time, such as when VS Code
    /// fetched the account's policy. Omitted for a value the user writes.
    pub cached_at: Option<Vec<String>>,
    /// How the Agent reads the value as the setting's, as `[held, read]`
    /// pairs, such as VS Code reading a policy's `mcp: false` as
    /// `chat.mcp.access: "none"`. A value no pair holds then sets nothing.
    /// Omitted, the value is read as it is.
    #[serde(default)]
    pub map: Vec<(Value, Value)>,
}

impl SettingPlace {
    /// The value `held` sets, as the Agent reads it.
    pub fn read_as(&self, held: Value) -> Option<Value> {
        if self.map.is_empty() {
            return Some(held);
        }
        self.map
            .iter()
            .find(|(known, _)| *known == held)
            .map(|(_, read)| read.clone())
    }

    /// How the file is parsed.
    pub fn format(&self) -> JsonFormat {
        JsonFormat::of(self.dialect)
    }
}

impl Validate for SettingPlace {
    fn validate(&self) -> Result<(), String> {
        self.anchor.validate()?;
        validate_key_path(&self.key_path)?;
        if self.anchor.walk() != ProjectWalk::None {
            return Err("a setting cannot walk up from the Project folder".into());
        }
        if self.anchor.plugin().is_some() || self.anchor.entry().is_some() {
            return Err("a setting cannot lie in a plugin's folder, nor beside an Entry".into());
        }
        let projects = self
            .key_path
            .iter()
            .filter(|key| *key == super::PROJECT_KEY)
            .count();
        if projects > 1 {
            return Err(format!(
                "a setting key path holds `{}` twice",
                super::PROJECT_KEY
            ));
        }
        if projects == 1 && (self.anchor.in_project() || self.anchor.settings().is_some()) {
            return Err(format!(
                "only a setting in a file of the Agent home can be keyed by `{}`",
                super::PROJECT_KEY
            ));
        }
        if self.key_path.iter().any(|key| key == NAME_KEY) {
            return Err(format!("a setting key path cannot hold `{NAME_KEY}`"));
        }
        if self.sqlite && (self.key_path[0] == super::PROJECT_KEY || self.dialect.is_some()) {
            return Err(format!(
                "a SQLite setting's key path starts with a key of its `ItemTable`, not `{}`, and it takes no dialect",
                super::PROJECT_KEY
            ));
        }
        if let Some(cached_at) = &self.cached_at {
            validate_key_path(cached_at).map_err(|e| format!("cached_at: {e}"))?;
            if self.sqlite && cached_at[0] != self.key_path[0] {
                return Err(
                    "a SQLite setting's cached_at is under the same key as its value".into(),
                );
            }
        }
        Ok(())
    }
}

// --- Read-only rules --------------------------------------------------------

/// A setting that keeps Entries out by a rule rather than one by one, such
/// as a deny list: the Entries it keeps out are Disabled, or Blocked when
/// the setting is policy. config-digger never writes it, so Enable is not
/// offered.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "block", rename_all = "kebab-case")]
pub(crate) enum Rule {
    AllowDenyList(AllowDenyList),
    GlobalSwitch(GlobalSwitch),
}

impl Validate for Rule {
    fn validate(&self) -> Result<(), String> {
        match self {
            Rule::AllowDenyList(block) => block.validate(),
            Rule::GlobalSwitch(block) => block.validate(),
        }
    }
}

impl Rule {
    /// Where the rule is read, highest first.
    pub fn sources(&self) -> &[SettingPlace] {
        match self {
            Rule::AllowDenyList(block) => &block.sources,
            Rule::GlobalSwitch(block) => &block.sources,
        }
    }

    /// Every setting it reads: its sources, in their order, then any
    /// setting that changes how it reads them.
    pub fn settings(&self) -> impl Iterator<Item = &SettingPlace> {
        let policy_only = match self {
            Rule::AllowDenyList(block) => block.policy_only.as_ref(),
            Rule::GlobalSwitch(_) => None,
        };
        self.sources().iter().chain(policy_only)
    }

    /// The ids of the locations whose Entries it covers, empty for every
    /// location of the Item kind.
    pub fn locations(&self) -> &[String] {
        match self {
            Rule::AllowDenyList(block) => &block.locations,
            Rule::GlobalSwitch(block) => &block.locations,
        }
    }

    /// Whether it covers the Entries of the location `id`.
    pub fn covers(&self, id: &str) -> bool {
        let locations = self.locations();
        locations.is_empty() || locations.iter().any(|location| location == id)
    }
}

/// Lists of servers, each item naming servers by name, command or URL,
/// merged from every source that holds one. A deny list keeps out every
/// server an item matches. An allow list, once any source holds one, keeps
/// out every server no item matches: an empty one keeps out all of them.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct AllowDenyList {
    pub list: ListKind,
    /// Where the lists are. A server an allow list keeps out is Blocked
    /// when any allow list is policy.
    pub sources: Vec<SettingPlace>,
    /// The field of an item that matches each part of a server.
    #[serde(rename = "match")]
    pub matching: ItemFields,
    /// A setting that, while it holds `true`, leaves only the allow lists
    /// of policy sources, such as Claude Code's `allowManagedMcpServersOnly`.
    pub policy_only: Option<SettingPlace>,
    /// When the allow list holds an item matching by command, a server
    /// with a command is allowed only by such an item, and when it holds
    /// one matching by URL, a remote server only by such an item.
    #[serde(default)]
    pub strict: bool,
    /// The ids of the locations whose Entries it covers. Omitted, it covers
    /// every location of the Item kind.
    #[serde(default)]
    pub locations: Vec<String>,
}

/// Whether a list keeps out what it matches or what it does not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ListKind {
    Allow,
    Deny,
}

/// The fields of a list item, each matching one part of a server on the
/// common server model.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ItemFields {
    /// Holds a server name, matched exactly.
    pub name: Option<String>,
    /// Holds the command and its args as one list, matched exactly.
    pub command: Option<String>,
    /// Holds a URL, where `*` stands for any text.
    pub url: Option<String>,
}

/// What a list item matched a server by.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum MatchedBy {
    Name,
    Command,
    Url,
}

/// A server as a list item matches it: its name, its command and args,
/// and its URL, as the Entry shows them.
pub(crate) struct ServerIdentity<'a> {
    pub name: &'a str,
    pub command: Option<Vec<&'a str>>,
    pub url: Option<&'a str>,
}

impl Validate for AllowDenyList {
    fn validate(&self) -> Result<(), String> {
        validate_sources(&self.sources)?;
        let fields = [
            &self.matching.name,
            &self.matching.command,
            &self.matching.url,
        ];
        if fields.iter().all(|field| field.is_none()) {
            return Err("match needs a name, command or url field".into());
        }
        if fields
            .iter()
            .flat_map(|field| field.as_deref())
            .any(str::is_empty)
        {
            return Err("a match field must not be empty".into());
        }
        if self.strict && self.list == ListKind::Deny {
            return Err("only an allow list can be strict".into());
        }
        if let Some(setting) = &self.policy_only {
            if self.list == ListKind::Deny {
                return Err("only an allow list can be policy_only".into());
            }
            setting
                .validate()
                .map_err(|e| format!("policy_only: {e}"))?;
        }
        Ok(())
    }
}

impl AllowDenyList {
    /// What `item`, one item of a list, matches `server` by, if anything.
    /// `item_command` and `item_url` give an item's command or URL as the
    /// server's would be shown, so that masked values compare alike.
    pub fn matches(
        &self,
        item: &Value,
        server: &ServerIdentity,
        item_command: &dyn Fn(&[&str]) -> Vec<String>,
        item_url: &dyn Fn(&str) -> String,
    ) -> Option<MatchedBy> {
        let field = |name: &Option<String>| item.get(name.as_deref()?);
        if let Some(name) = field(&self.matching.name).and_then(Value::as_str)
            && name == server.name
        {
            return Some(MatchedBy::Name);
        }
        if let (Some(command), Some(own)) = (
            field(&self.matching.command).and_then(Value::as_array),
            &server.command,
        ) {
            let command: Option<Vec<&str>> = command.iter().map(Value::as_str).collect();
            if command.is_some_and(|command| item_command(&command) == *own) {
                return Some(MatchedBy::Command);
            }
        }
        if let (Some(pattern), Some(url)) = (
            field(&self.matching.url).and_then(Value::as_str),
            server.url,
        ) && wildcard_match(&item_url(pattern), url)
        {
            return Some(MatchedBy::Url);
        }
        None
    }

    /// Whether `item` names servers by command, and whether by URL.
    pub fn kinds(&self, item: &Value) -> (bool, bool) {
        let has =
            |name: &Option<String>| name.as_deref().is_some_and(|name| item.get(name).is_some());
        (has(&self.matching.command), has(&self.matching.url))
    }
}

/// Whether `text` matches `pattern`, where `*` stands for any text.
fn wildcard_match(pattern: &str, text: &str) -> bool {
    let mut parts = pattern.split('*');
    let first = parts.next().unwrap_or_default();
    let Some(mut rest) = text.strip_prefix(first) else {
        return false;
    };
    let parts: Vec<&str> = parts.collect();
    let Some((last, middle)) = parts.split_last() else {
        return rest.is_empty();
    };
    for part in middle {
        match rest.find(part) {
            Some(at) => rest = &rest[at + part.len()..],
            None => return false,
        }
    }
    rest.len() >= last.len() && rest.ends_with(last)
}

/// A setting that turns off every Entry it covers while it holds `off`,
/// such as Claude Code's `syncClaudeAiSkills: false`. The first source
/// that holds it decides, or with `every_source`, the first that holds
/// `off`; the Entries are Disabled globally, or Blocked when that source is
/// policy.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct GlobalSwitch {
    /// Highest first.
    pub sources: Vec<SettingPlace>,
    /// The value that turns the Entries off.
    pub off: Value,
    /// Whether every source must leave the Entries on, as when the Agent
    /// loads them only while neither its policy nor its own file holds
    /// `off`, such as Claude Desktop's `isLocalDevMcpEnabled`.
    #[serde(default)]
    pub every_source: bool,
    /// The ids of the locations whose Entries it covers. Omitted, it covers
    /// every location of the Item kind.
    #[serde(default)]
    pub locations: Vec<String>,
}

impl Validate for GlobalSwitch {
    fn validate(&self) -> Result<(), String> {
        validate_sources(&self.sources)
    }
}

/// A rule or gate reads at least one source.
fn validate_sources(sources: &[SettingPlace]) -> Result<(), String> {
    if sources.is_empty() {
        return Err("sources needs at least one setting".into());
    }
    for (index, source) in sources.iter().enumerate() {
        source
            .validate()
            .map_err(|e| format!("sources[{index}]: {e}"))?;
    }
    Ok(())
}

// --- Gates ------------------------------------------------------------------

/// Something outside an Entry that keeps the Agent from using it, which
/// Enable cannot lift and config-digger never lifts: the Entry is Blocked.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "block", rename_all = "kebab-case")]
pub(crate) enum Gate {
    ManagedExclusive(ManagedExclusive),
    ApprovalList(ApprovalList),
    PolicyField(PolicyField),
    TrustedFolders(TrustedFolders),
    NonceTrust(NonceTrust),
    ApprovalHash(ApprovalHash),
}

impl Validate for Gate {
    fn validate(&self) -> Result<(), String> {
        match self {
            Gate::ManagedExclusive(block) => block.validate(),
            Gate::ApprovalList(block) => block.validate(),
            Gate::PolicyField(block) => block.validate(),
            Gate::TrustedFolders(block) => block.validate(),
            Gate::NonceTrust(block) => block.validate(),
            Gate::ApprovalHash(block) => block.validate(),
        }
    }
}

impl Gate {
    /// Whether it can Block the Entries of the location `id`.
    pub fn covers(&self, id: &str) -> bool {
        match self {
            Gate::ManagedExclusive(block) => block.covers(id),
            Gate::ApprovalList(_)
            | Gate::PolicyField(_)
            | Gate::TrustedFolders(_)
            | Gate::NonceTrust(_)
            | Gate::ApprovalHash(_) => self.locations().contains(&id),
        }
    }

    /// Where it reads what keeps Entries out.
    pub fn sources(&self) -> &[SettingPlace] {
        match self {
            Gate::ManagedExclusive(_) => &[],
            Gate::ApprovalList(block) => &block.sources,
            Gate::PolicyField(block) => &block.sources,
            Gate::TrustedFolders(block) => &block.sources,
            Gate::NonceTrust(block) => &block.sources,
            Gate::ApprovalHash(block) => &block.sources,
        }
    }

    /// Every setting it reads: its sources, in their order, then any
    /// setting that lifts it.
    pub fn settings(&self) -> impl Iterator<Item = &SettingPlace> {
        let lifted_by = match self {
            Gate::TrustedFolders(block) => block.trust_all.as_ref(),
            _ => None,
        };
        self.sources()
            .iter()
            .chain(lifted_by.into_iter().flat_map(|setting| &setting.sources))
    }

    /// The ids of the locations it names.
    pub fn locations(&self) -> Vec<&str> {
        match self {
            Gate::ManagedExclusive(block) => std::iter::once(block.location.as_str())
                .chain(block.except.iter().map(String::as_str))
                .collect(),
            Gate::ApprovalList(block) => block.locations.iter().map(String::as_str).collect(),
            Gate::PolicyField(block) => block.locations.iter().map(String::as_str).collect(),
            Gate::TrustedFolders(block) => block.locations.iter().map(String::as_str).collect(),
            Gate::NonceTrust(block) => block.locations.iter().map(String::as_str).collect(),
            Gate::ApprovalHash(block) => block.locations.iter().map(String::as_str).collect(),
        }
    }
}

/// Locations whose Entries the Agent uses only in a Project the user trusts,
/// such as the servers of a workspace VS Code reads only once its Workspace
/// Trust is given: an Entry of one in a Project the user has not trusted is
/// Blocked, with `untrusted_folder`. A folder is trusted when an item of a
/// source trusts it or a folder above it, and a workspace file when it and
/// each of its folders are. config-digger never trusts a folder.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct TrustedFolders {
    /// The ids of the locations whose Entries need trust, each in a Project.
    pub locations: Vec<String>,
    /// Where the lists of trusted folders are, every one of them read.
    pub sources: Vec<SettingPlace>,
    /// The field of an item that holds the folder's `file:` URI, as text
    /// or as VS Code's URI object.
    pub uri: String,
    /// The field of an item that must hold `true` for it to trust the
    /// folder. Omitted, every item trusts its folder.
    pub trusted: Option<String>,
    /// A setting that, while it is met, trusts every folder, such as VS
    /// Code's `security.workspace.trust.enabled: false`.
    pub trust_all: Option<SettingIs>,
}

impl Validate for TrustedFolders {
    fn validate(&self) -> Result<(), String> {
        if self.locations.is_empty() {
            return Err("a trusted-folders gate needs locations".into());
        }
        if self.uri.is_empty() || self.trusted.as_deref() == Some("") {
            return Err("a trusted-folders gate needs its fields named".into());
        }
        validate_sources(&self.sources)?;
        if let Some(setting) = &self.trust_all {
            setting.validate().map_err(|e| format!("trust_all: {e}"))?;
        }
        Ok(())
    }
}

impl TrustedFolders {
    /// The folders the items of the list `value` trust.
    pub fn trusted_folders(&self, value: &Value) -> Vec<PathBuf> {
        let items = value.as_array().map(Vec::as_slice).unwrap_or_default();
        items
            .iter()
            .filter(|item| {
                self.trusted
                    .as_ref()
                    .is_none_or(|field| item.get(field) == Some(&Value::Bool(true)))
            })
            .filter_map(|item| paths::from_uri_value(item.get(&self.uri)?))
            .collect()
    }
}

/// Entries the Agent uses only once the user trusts them, and asks about
/// again whenever their configuration changes, such as the servers VS Code
/// discovers in other apps' files: one no source trusts is Blocked, with
/// `not_approved`. A source holds a list of `[id, record]` pairs, each id
/// as the location's `switch_id` makes it, and the Entry is trusted while
/// its record's `trusted_at` field equals its `nonce` field. config-digger
/// cannot tell whether the configuration changed since the Agent recorded
/// the nonce, and never trusts an Entry.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct NonceTrust {
    /// The ids of the locations whose Entries need trust.
    pub locations: Vec<String>,
    /// Where the lists of records are, every one of them read.
    pub sources: Vec<SettingPlace>,
    /// The field of a record holding the nonce the user trusted.
    pub trusted_at: String,
    /// The field of a record holding the nonce of the configuration the
    /// Agent last saw.
    pub nonce: String,
}

impl Validate for NonceTrust {
    fn validate(&self) -> Result<(), String> {
        if self.locations.is_empty() {
            return Err("a nonce-trust gate needs locations".into());
        }
        if self.trusted_at.is_empty() || self.nonce.is_empty() {
            return Err("a nonce-trust gate needs its fields named".into());
        }
        validate_sources(&self.sources)
    }
}

impl NonceTrust {
    /// Whether the list `value` trusts the Entry whose id is `id`.
    pub fn trusts(&self, value: &Value, id: &str) -> bool {
        let pairs = value.as_array().map(Vec::as_slice).unwrap_or_default();
        pairs.iter().any(|pair| {
            let record = pair.get(1);
            let field = |name: &str| record.and_then(|record| record.get(name));
            pair.get(0).and_then(Value::as_str) == Some(id)
                && field(&self.trusted_at).is_some_and(|nonce| Some(nonce) == field(&self.nonce))
        })
    }
}

/// A field an organisation sets to keep an Entry out, such as the
/// `orgBlockedReason` Claude Desktop writes into the settings of an
/// extension on its organisation's blocklist: while a source holds anything
/// but `null`, `false` or `""` there, the Entry is Blocked by `policy`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct PolicyField {
    /// The ids of the locations whose Entries it can Block.
    pub locations: Vec<String>,
    /// Where the field is.
    pub sources: Vec<SettingPlace>,
}

impl Validate for PolicyField {
    fn validate(&self) -> Result<(), String> {
        if self.locations.is_empty() {
            return Err("a policy field needs locations".into());
        }
        validate_sources(&self.sources)
    }
}

impl PolicyField {
    /// Whether `value`, what a source holds, keeps the Entry out.
    pub fn keeps_out(value: &Value) -> bool {
        !matches!(value, Value::Null | Value::Bool(false))
            && value.as_str().is_none_or(|text| !text.is_empty())
    }
}

/// Entries the Agent uses only once the user approves them, such as Claude
/// Code's `.mcp.json` servers: one no source approves is Blocked, with
/// `not_approved`. config-digger never approves an Entry.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ApprovalList {
    /// The ids of the locations whose Entries need approval.
    pub locations: Vec<String>,
    /// Where approvals are: a list of the names approved, or `true`,
    /// which approves every Entry.
    pub sources: Vec<SettingPlace>,
    /// Where the Agent asks the user to approve an Entry.
    pub prompt: ApprovalPrompt,
}

/// Where an Agent asks the user to approve an Entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ApprovalPrompt {
    /// When it starts in the Project.
    OnStart,
    /// In its settings of MCP servers.
    McpSettings,
}

/// Entries the Agent uses only once the user approves them as they are
/// configured, such as Cursor's Project servers: a source holds a list of
/// `<id>:<hash>` texts, each id as the location's `switch_id` makes it, or
/// else the Entry's name, and each hash that of the configuration approved;
/// a bare `<id>` approves the Entry however it is configured. One no source
/// approves is Blocked, with `not_approved`, so that changing an approved
/// Entry Blocks it again. config-digger never approves an Entry.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ApprovalHash {
    /// The ids of the locations whose Entries need approval.
    pub locations: Vec<String>,
    /// Where the lists of approvals are, every one of them read.
    pub sources: Vec<SettingPlace>,
    /// How the Agent hashes an Entry's configuration.
    pub hash: ConfigHash,
    /// Where the Agent asks the user to approve an Entry.
    pub prompt: ApprovalPrompt,
}

impl Validate for ApprovalHash {
    fn validate(&self) -> Result<(), String> {
        if self.locations.is_empty() {
            return Err("an approval-hash gate needs locations".into());
        }
        validate_sources(&self.sources)?;
        if self.sources.iter().any(|source| source.policy) {
            return Err("an approval is no policy".into());
        }
        Ok(())
    }
}

impl ApprovalHash {
    /// Whether `value`, what a source holds, approves the Entry whose id is
    /// `id` and whose configuration hashes to `hash`.
    pub fn approves(value: &Value, id: &str, hash: Option<&str>) -> bool {
        let approval = hash.map(|hash| format!("{id}:{hash}"));
        let approvals = value.as_array().map(Vec::as_slice).unwrap_or_default();
        approvals
            .iter()
            .filter_map(Value::as_str)
            .any(|known| known == id || approval.as_deref() == Some(known))
    }
}

/// How an Agent hashes an Entry's configuration to tell whether it changed
/// since the user approved it. config-digger computes it from the Entry as
/// written, Secrets included, and never shows it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ConfigHash {
    /// Cursor's: VS Code's 32-bit string hash, written in hexadecimal with
    /// a `-` when negative, of the JSON text of the fields of the server's
    /// transport, in this order: its `command`, `args`, `env` and `envFile`
    /// when it has a `command`, and otherwise its `url`, given `http://`
    /// when it names no scheme, `headers` and `auth`.
    Cursor,
}

impl ConfigHash {
    /// The hash of the Entry `config`, as written; `None` for an Entry the
    /// Agent cannot run.
    pub fn of(self, config: &Value) -> Option<String> {
        match self {
            ConfigHash::Cursor => {
                let mut fields = serde_json::Map::new();
                let keep: &[&str] = if truthy(config, "command") {
                    &["command", "args", "env", "envFile"]
                } else if truthy(config, "url") {
                    &["url", "headers", "auth"]
                } else {
                    return None;
                };
                for &field in keep {
                    let Some(value) = config.get(field) else {
                        continue;
                    };
                    let value = match (field, value) {
                        ("url", Value::String(url)) => Value::from(with_scheme(url)),
                        _ => value.clone(),
                    };
                    fields.insert(field.to_owned(), value);
                }
                let mut text = String::new();
                js_stringify(&Value::Object(fields), &mut text);
                let hash = text.encode_utf16().fold(149_417i32, |hash, unit| {
                    hash.wrapping_mul(31).wrapping_add(i32::from(unit))
                });
                let hash = i64::from(hash);
                Some(if hash < 0 {
                    format!("-{:x}", -hash)
                } else {
                    format!("{hash:x}")
                })
            }
        }
    }
}

/// Whether `config` holds a value at `field` that JavaScript takes as true,
/// as Cursor tests a server's `command` and `url`.
fn truthy(config: &Value, field: &str) -> bool {
    config.get(field).is_some_and(|value| match value {
        Value::Null | Value::Bool(false) => false,
        Value::String(text) => !text.is_empty(),
        Value::Number(number) => number.as_f64() != Some(0.0),
        Value::Bool(true) | Value::Array(_) | Value::Object(_) => true,
    })
}

/// `url`, given `http://` unless it starts with a scheme of one to eight
/// lower-case letters and `://`, as Cursor gives a server's URL one.
fn with_scheme(url: &str) -> String {
    let has_scheme = url.split_once("://").is_some_and(|(scheme, _)| {
        (1..=8).contains(&scheme.len()) && scheme.bytes().all(|byte| byte.is_ascii_lowercase())
    });
    if has_scheme {
        url.to_owned()
    } else {
        format!("http://{url}")
    }
}

/// `value` as JavaScript's `JSON.stringify` writes it: no spaces, an
/// object's keys that are array indexes first, in numeric order, then the
/// others in their order, and a whole number without a fraction.
fn js_stringify(value: &Value, out: &mut String) {
    match value {
        Value::Object(object) => {
            let index = |key: &str| {
                key.parse::<u32>()
                    .ok()
                    .filter(|index| *index != u32::MAX && index.to_string() == key)
            };
            let mut keys: Vec<&String> = object.keys().filter(|key| index(key).is_some()).collect();
            keys.sort_by_key(|key| index(key));
            keys.extend(object.keys().filter(|key| index(key).is_none()));
            out.push('{');
            for (at, key) in keys.into_iter().enumerate() {
                if at > 0 {
                    out.push(',');
                }
                out.push_str(&Value::from(key.as_str()).to_string());
                out.push(':');
                js_stringify(&object[key], out);
            }
            out.push('}');
        }
        Value::Array(items) => {
            out.push('[');
            for (at, item) in items.iter().enumerate() {
                if at > 0 {
                    out.push(',');
                }
                js_stringify(item, out);
            }
            out.push(']');
        }
        Value::Number(number) => match number.as_f64() {
            Some(float) if number.is_f64() && float.fract() == 0.0 && float.abs() < 1e21 => {
                out.push_str(&format!("{float:.0}"));
            }
            _ => out.push_str(&number.to_string()),
        },
        other => out.push_str(&other.to_string()),
    }
}

impl Validate for ApprovalList {
    fn validate(&self) -> Result<(), String> {
        if self.locations.is_empty() {
            return Err("an approval list needs locations".into());
        }
        validate_sources(&self.sources)?;
        if self.sources.iter().any(|source| source.policy) {
            return Err("an approval is no policy".into());
        }
        Ok(())
    }
}

impl ApprovalList {
    /// Whether `value`, what a source holds, approves the Entry named `name`.
    pub fn approves(&self, value: &Value, name: &str) -> bool {
        match value {
            Value::Bool(all) => *all,
            Value::Array(names) => names.iter().any(|known| known.as_str() == Some(name)),
            _ => false,
        }
    }
}

/// A managed file that takes exclusive control while it exists, even
/// empty: the Agent then loads no Entry of the kind from any other
/// location but those it exempts, which are Blocked by `policy`. Such as
/// Claude Code's `managed-mcp.json`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ManagedExclusive {
    /// The id of the location whose file takes control, outside any Project.
    pub location: String,
    /// The ids of the other locations it leaves alone, such as servers the
    /// managed settings declare.
    #[serde(default)]
    pub except: Vec<String>,
}

impl Validate for ManagedExclusive {
    fn validate(&self) -> Result<(), String> {
        if self.location.trim().is_empty() {
            return Err("a managed-exclusive gate needs a location".into());
        }
        Ok(())
    }
}

impl ManagedExclusive {
    /// Whether it Blocks the Entries of the location `id` while it holds.
    pub fn covers(&self, id: &str) -> bool {
        self.location != id && !self.except.iter().any(|except| except == id)
    }
}

// --- Feature gate lookups ---------------------------------------------------

/// The lookup `key_path` segment that stands for the gate's name, or its
/// hash when the lookup has one.
pub(crate) const GATE_KEY: &str = "{gate}";

/// Where an Agent caches the value of a Feature gate, and how config-digger
/// finds it there.
pub(crate) trait LookUp {
    /// The file the value is cached in.
    fn anchor(&self) -> &Anchor;

    /// What in the file holds the cached values.
    fn store(&self) -> GateStore<'_>;

    /// The value the Agent last cached for the gate `name`, and when it
    /// cached it, read from `document`, what the store holds. `None` when
    /// it holds no `true` or `false` for the gate.
    fn cached(&self, document: &Value, name: &str) -> Option<Cached>;
}

/// What in a lookup's file holds the cached values.
pub(crate) enum GateStore<'a> {
    /// The whole file, as strict JSON.
    JsonFile,
    /// The JSON text under this key of a SQLite store's `ItemTable`.
    SqliteItem(&'a str),
}

/// A Feature gate's value as the Agent last cached it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Cached {
    pub on: bool,
    /// When the Agent cached it, when the lookup says where that is and
    /// the store holds it.
    pub at: Option<SystemTime>,
}

/// Where an Agent caches a Feature gate's value.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "block", rename_all = "kebab-case")]
pub(crate) enum GateLookup {
    JsonValue(JsonGateLookup),
    SqliteJson(SqliteGateLookup),
}

impl Validate for GateLookup {
    fn validate(&self) -> Result<(), String> {
        match self {
            GateLookup::JsonValue(block) => block.validate(),
            GateLookup::SqliteJson(block) => block.validate(),
        }
    }
}

impl LookUp for GateLookup {
    fn anchor(&self) -> &Anchor {
        match self {
            GateLookup::JsonValue(block) => block.anchor(),
            GateLookup::SqliteJson(block) => block.anchor(),
        }
    }

    fn store(&self) -> GateStore<'_> {
        match self {
            GateLookup::JsonValue(block) => block.store(),
            GateLookup::SqliteJson(block) => block.store(),
        }
    }

    fn cached(&self, document: &Value, name: &str) -> Option<Cached> {
        match self {
            GateLookup::JsonValue(block) => block.cached(document, name),
            GateLookup::SqliteJson(block) => block.cached(document, name),
        }
    }
}

/// A gate's value in a JSON file of the Agent home, such as Claude Code's
/// `cachedGrowthBookFeatures` in `~/.claude.json`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct JsonGateLookup {
    /// The file, in the Agent home.
    pub anchor: Anchor,
    /// The key path of the gate's `true` or `false`, one key per segment.
    /// One segment is `{gate}`: the gate's name, or its hash with `hash`.
    pub key_path: Vec<String>,
    /// How the Agent hashes gate names in its cache. Omitted when it keys
    /// them by name.
    pub hash: Option<GateHash>,
    /// The key path of when the Agent cached its values: milliseconds
    /// since 1970, or an RFC 3339 time. Omitted when it records none.
    pub cached_at: Option<Vec<String>>,
}

impl Validate for JsonGateLookup {
    fn validate(&self) -> Result<(), String> {
        validate_gate_anchor(&self.anchor)?;
        validate_gate_value(&self.key_path, self.cached_at.as_deref())
    }
}

impl LookUp for JsonGateLookup {
    fn anchor(&self) -> &Anchor {
        &self.anchor
    }

    fn store(&self) -> GateStore<'_> {
        GateStore::JsonFile
    }

    fn cached(&self, document: &Value, name: &str) -> Option<Cached> {
        let key = gate_key(self.hash, name);
        cached_value(document, &self.key_path, &key, self.cached_at.as_deref())
    }
}

/// A gate's value in the JSON text an Agent keeps under a key of the
/// `ItemTable` of a SQLite store, as VS Code and Cursor keep their state in
/// `state.vscdb`, such as Cursor's Statsig bootstrap.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SqliteGateLookup {
    /// The SQLite file, in the Agent home.
    pub anchor: Anchor,
    /// The `ItemTable` key whose value is the JSON text.
    pub key: String,
    /// The key path of the gate's `true` or `false` in the JSON, one key
    /// per segment. One segment is `{gate}`: the gate's name, or its hash
    /// with `hash`.
    pub key_path: Vec<String>,
    /// How the Agent hashes gate names in its cache. Omitted when it keys
    /// them by name.
    pub hash: Option<GateHash>,
    /// The key path in the JSON of when the Agent cached its values:
    /// milliseconds since 1970, or an RFC 3339 time. Omitted when it
    /// records none.
    pub cached_at: Option<Vec<String>>,
}

impl Validate for SqliteGateLookup {
    fn validate(&self) -> Result<(), String> {
        validate_gate_anchor(&self.anchor)?;
        if self.key.trim().is_empty() {
            return Err("a sqlite-json lookup needs a key".into());
        }
        validate_gate_value(&self.key_path, self.cached_at.as_deref())
    }
}

impl LookUp for SqliteGateLookup {
    fn anchor(&self) -> &Anchor {
        &self.anchor
    }

    fn store(&self) -> GateStore<'_> {
        GateStore::SqliteItem(&self.key)
    }

    fn cached(&self, document: &Value, name: &str) -> Option<Cached> {
        let key = gate_key(self.hash, name);
        cached_value(document, &self.key_path, &key, self.cached_at.as_deref())
    }
}

/// How an Agent hashes gate names in its cache.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum GateHash {
    /// Statsig's djb2: a 32-bit hash of the name's UTF-16 code units,
    /// written as an unsigned decimal number.
    Djb2,
}

/// A lookup key path names `{gate}` once, and `cached_at` is a key path.
fn validate_gate_value(key_path: &[String], cached_at: Option<&[String]>) -> Result<(), String> {
    validate_key_path(key_path)?;
    if key_path.iter().filter(|key| *key == GATE_KEY).count() != 1 {
        return Err(format!("a lookup key path holds `{GATE_KEY}` exactly once"));
    }
    if let Some(cached_at) = cached_at {
        validate_key_path(cached_at).map_err(|e| format!("cached_at: {e}"))?;
    }
    Ok(())
}

/// The key the gate `name` has in a cache that hashes names with `hash`.
fn gate_key(hash: Option<GateHash>, name: &str) -> String {
    match hash {
        Some(GateHash::Djb2) => djb2(name),
        None => name.to_owned(),
    }
}

/// The `true` or `false` at `key_path` in `document`, `{gate}` standing
/// for `key`, and the time at `cached_at`.
fn cached_value(
    document: &Value,
    key_path: &[String],
    key: &str,
    cached_at: Option<&[String]>,
) -> Option<Cached> {
    let at_path = |key_path: &[String]| {
        key_path.iter().try_fold(document, |value, segment| {
            value.get(if segment == GATE_KEY { key } else { segment })
        })
    };
    let on = at_path(key_path)?.as_bool()?;
    let at = cached_at.and_then(at_path).and_then(time_of);
    Some(Cached { on, at })
}

/// The time `value` records: milliseconds since 1970, or an RFC 3339 time.
pub(crate) fn time_of(value: &Value) -> Option<SystemTime> {
    match value {
        Value::Number(millis) => {
            let millis = millis.as_f64().filter(|millis| *millis >= 0.0)?;
            SystemTime::UNIX_EPOCH.checked_add(Duration::try_from_secs_f64(millis / 1000.0).ok()?)
        }
        Value::String(text) => humantime::parse_rfc3339_weak(text).ok(),
        _ => None,
    }
}

/// Statsig's djb2 hash of `name`, as it keys gates in its cache.
pub(crate) fn djb2(name: &str) -> String {
    name.encode_utf16()
        .fold(0u32, |hash, unit| {
            hash.wrapping_mul(31).wrapping_add(u32::from(unit))
        })
        .to_string()
}

/// A gate's cache is one file of the Agent home, so that it always has a path.
fn validate_gate_anchor(anchor: &Anchor) -> Result<(), String> {
    anchor.validate()?;
    anchor.one_file()?;
    if anchor.home_root().is_none() {
        return Err("a lookup must lie in the Agent home".into());
    }
    Ok(())
}

// --- Feature gate bypasses --------------------------------------------------

/// When the Agent ignores its cached gate values and uses its own defaults.
pub(crate) trait Bypasses {
    /// The variable that makes the Agent ignore its cache, if one does.
    /// `var` gives a variable's value as the Agent sees it.
    fn bypassed_by(&self, var: &dyn Fn(&str) -> Option<String>) -> Option<String>;

    /// The files whose maps of variables the Agent applies over its
    /// environment, highest first, each with the key path of the map. A
    /// machine-wide file whose path cannot be expanded is skipped.
    fn env_maps(&self) -> &[EnvMap];
}

/// When an Agent ignores its cached Feature gate values.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "block", rename_all = "kebab-case")]
pub(crate) enum Bypass {
    Env(EnvBypass),
}

impl Validate for Bypass {
    fn validate(&self) -> Result<(), String> {
        match self {
            Bypass::Env(block) => block.validate(),
        }
    }
}

impl Bypasses for Bypass {
    fn bypassed_by(&self, var: &dyn Fn(&str) -> Option<String>) -> Option<String> {
        match self {
            Bypass::Env(block) => block.bypassed_by(var),
        }
    }

    fn env_maps(&self) -> &[EnvMap] {
        match self {
            Bypass::Env(block) => block.env_maps(),
        }
    }
}

/// Environment variables that make the Agent ignore its cache, such as
/// Claude Code's `DISABLE_GROWTHBOOK`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct EnvBypass {
    /// The cache is ignored when any of them is set.
    pub variables: Vec<BypassVariable>,
    /// Maps of variables the Agent applies over the environment it starts
    /// in, such as the `env` of Claude Code's user settings, highest
    /// first. A variable there replaces the one in the environment.
    #[serde(default)]
    pub env_maps: Vec<EnvMap>,
}

/// One variable that makes the Agent ignore its cache.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct BypassVariable {
    pub name: String,
    /// Which of its values count.
    pub values: VariableValues,
    /// Variables that, when any is set to a non-empty value, keep this one
    /// from counting.
    #[serde(default)]
    pub unless: Vec<String>,
}

/// Which values of a variable count as setting it.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum VariableValues {
    /// Any non-empty value, `0` and `false` included.
    NonEmpty,
    /// `1`, `true`, `yes` or `on`, in any case.
    Truthy,
}

/// A JSON object of variable names and string values in a file of the
/// Agent home or the machine, such as the `env` of a settings file.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct EnvMap {
    pub anchor: Anchor,
    /// The key path of the object, one key per segment.
    pub key_path: Vec<String>,
}

impl Validate for EnvBypass {
    fn validate(&self) -> Result<(), String> {
        if self.variables.is_empty() {
            return Err("an env bypass needs variables".into());
        }
        for variable in &self.variables {
            for name in std::iter::once(&variable.name).chain(&variable.unless) {
                validate_variable(name)?;
            }
        }
        for map in &self.env_maps {
            map.anchor.validate().map_err(|e| format!("env map: {e}"))?;
            map.anchor.one_file().map_err(|e| format!("env map: {e}"))?;
            if map.anchor.in_project() {
                return Err("an env map must lie in the Agent home or the machine".into());
            }
            validate_key_path(&map.key_path).map_err(|e| format!("env map: {e}"))?;
        }
        Ok(())
    }
}

impl Bypasses for EnvBypass {
    fn bypassed_by(&self, var: &dyn Fn(&str) -> Option<String>) -> Option<String> {
        let non_empty = |name: &str| var(name).is_some_and(|value| !value.is_empty());
        self.variables
            .iter()
            .find(|variable| {
                let set = match variable.values {
                    VariableValues::NonEmpty => non_empty(&variable.name),
                    VariableValues::Truthy => var(&variable.name).is_some_and(|value| {
                        ["1", "true", "yes", "on"].contains(&value.trim().to_lowercase().as_str())
                    }),
                };
                set && !variable.unless.iter().any(|name| non_empty(name))
            })
            .map(|variable| variable.name.clone())
    }

    fn env_maps(&self) -> &[EnvMap] {
        &self.env_maps
    }
}

// --- Conditions -------------------------------------------------------------

/// Whether something a definition declares applies, such as a location the
/// Agent reads only while a Feature gate is on, or a setting has a value.
pub(crate) trait Holds {
    /// Whether it holds, given whether each Feature gate is on, by name,
    /// and what each source of its setting holds, by index.
    fn holds(
        &self,
        gate_on: &dyn Fn(&str) -> bool,
        setting: &dyn Fn(usize) -> Option<Value>,
    ) -> bool;

    /// The Feature gate it depends on, if any.
    fn gate(&self) -> Option<&str>;
}

/// A condition under which an Agent reads a location.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "block", rename_all = "kebab-case")]
pub(crate) enum Condition {
    FeatureGate(GateCondition),
    Setting(SettingIs),
}

impl Validate for Condition {
    fn validate(&self) -> Result<(), String> {
        match self {
            Condition::FeatureGate(block) => block.validate(),
            Condition::Setting(block) => block.validate(),
        }
    }
}

impl Holds for Condition {
    fn holds(
        &self,
        gate_on: &dyn Fn(&str) -> bool,
        setting: &dyn Fn(usize) -> Option<Value>,
    ) -> bool {
        match self {
            Condition::FeatureGate(block) => block.holds(gate_on, setting),
            Condition::Setting(block) => block.holds(gate_on, setting),
        }
    }

    fn gate(&self) -> Option<&str> {
        match self {
            Condition::FeatureGate(block) => block.gate(),
            Condition::Setting(block) => block.gate(),
        }
    }
}

impl Condition {
    /// The setting it depends on, if any.
    pub fn setting(&self) -> Option<&SettingIs> {
        match self {
            Condition::FeatureGate(_) => None,
            Condition::Setting(block) => Some(block),
        }
    }
}

/// Holds while a Feature gate of the same definition is on, or with
/// `is = false`, while it is off.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct GateCondition {
    /// The name of a declared Feature gate.
    pub gate: String,
    /// The value the gate must have. Omitted for on.
    #[serde(default = "gate_on_by_default")]
    pub is: bool,
}

fn gate_on_by_default() -> bool {
    true
}

impl Validate for GateCondition {
    fn validate(&self) -> Result<(), String> {
        if self.gate.trim().is_empty() {
            return Err("a feature-gate condition needs a gate".into());
        }
        Ok(())
    }
}

impl Holds for GateCondition {
    fn holds(
        &self,
        gate_on: &dyn Fn(&str) -> bool,
        _setting: &dyn Fn(usize) -> Option<Value>,
    ) -> bool {
        gate_on(&self.gate) == self.is
    }

    fn gate(&self) -> Option<&str> {
        Some(&self.gate)
    }
}

// --- Version sources --------------------------------------------------------

/// Reads an Installation's version from a file or the registry, never by
/// running the Agent.
pub(crate) trait ReadVersion {
    /// The version and where it was read from, `None` when it cannot be read.
    fn read(&self, installation: &InstallationPaths, ports: &Ports) -> Option<VersionRead>;
}

/// Where an Installation is.
pub(crate) struct InstallationPaths<'a> {
    pub folder: &'a Path,
    pub executable: &'a Path,
}

/// A version as read, which starts with a number.
#[derive(Clone)]
pub(crate) struct VersionRead {
    pub text: String,
    /// The file or registry key it was read from.
    pub from: String,
}

impl VersionRead {
    /// `text` read from `from`, `None` when it does not start with a number.
    fn new(text: &str, from: String) -> Option<Self> {
        let text = text.trim();
        Version::parse(text)?;
        Some(Self {
            text: text.to_owned(),
            from,
        })
    }
}

/// Where an Installation's version is.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "block", rename_all = "kebab-case")]
pub(crate) enum VersionSource {
    ExeVersion(ExeVersion),
    AppxIdentity(AppxIdentity),
    JsonValue(JsonValue),
    UninstallKey(UninstallKey),
}

impl Validate for VersionSource {
    fn validate(&self) -> Result<(), String> {
        match self {
            VersionSource::ExeVersion(block) => block.validate(),
            VersionSource::AppxIdentity(block) => block.validate(),
            VersionSource::JsonValue(block) => block.validate(),
            VersionSource::UninstallKey(block) => block.validate(),
        }
    }
}

impl ReadVersion for VersionSource {
    fn read(&self, installation: &InstallationPaths, ports: &Ports) -> Option<VersionRead> {
        match self {
            VersionSource::ExeVersion(block) => block.read(installation, ports),
            VersionSource::AppxIdentity(block) => block.read(installation, ports),
            VersionSource::JsonValue(block) => block.read(installation, ports),
            VersionSource::UninstallKey(block) => block.read(installation, ports),
        }
    }
}

/// The product version in the version resource of an executable, such as
/// `claude.exe` or `Code.exe`. A fourth component of 0 is left out.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExeVersion {
    /// A `/`-separated path relative to the Installation's folder, where
    /// `*` and `?` stand for any name. Omitted for its executable.
    pub file: Option<String>,
}

impl Validate for ExeVersion {
    fn validate(&self) -> Result<(), String> {
        self.file.as_deref().map_or(Ok(()), validate_relative)
    }
}

impl ReadVersion for ExeVersion {
    fn read(&self, installation: &InstallationPaths, ports: &Ports) -> Option<VersionRead> {
        files(installation, self.file.as_deref(), ports)
            .into_iter()
            .find_map(|path| {
                let [a, b, c, d] = pe::product_version(ports.fs.as_ref(), &path)?;
                let text = match d {
                    0 => format!("{a}.{b}.{c}"),
                    d => format!("{a}.{b}.{c}.{d}"),
                };
                VersionRead::new(&text, path.display().to_string())
            })
    }
}

/// The `Version` of the `<Identity>` in an MSIX package's manifest.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct AppxIdentity {
    /// A `/`-separated path relative to the Installation's folder, where
    /// `*` and `?` stand for any name. Omitted for `AppxManifest.xml`.
    pub file: Option<String>,
}

impl Validate for AppxIdentity {
    fn validate(&self) -> Result<(), String> {
        self.file.as_deref().map_or(Ok(()), validate_relative)
    }
}

impl ReadVersion for AppxIdentity {
    fn read(&self, installation: &InstallationPaths, ports: &Ports) -> Option<VersionRead> {
        let file = self.file.as_deref().unwrap_or("AppxManifest.xml");
        files(installation, Some(file), ports)
            .into_iter()
            .find_map(|path| {
                let text = String::from_utf8(ports.fs.read(&path).ok()?).ok()?;
                VersionRead::new(identity_version(&text)?, path.display().to_string())
            })
    }
}

/// The `Version` attribute of the first `<Identity>` element of `xml`.
fn identity_version(xml: &str) -> Option<&str> {
    let start = xml.find("<Identity")? + "<Identity".len();
    let element = &xml[start..start + xml[start..].find('>')?];
    let mut rest = element;
    while let Some(at) = rest.find("Version=") {
        let before = rest[..at].chars().next_back();
        let value = &rest[at + "Version=".len()..];
        if before.is_some_and(char::is_whitespace) {
            let quote = value.chars().next()?;
            let value = value.strip_prefix(['"', '\''])?;
            return Some(&value[..value.find(quote)?]);
        }
        rest = value;
    }
    None
}

/// A string in a JSON file, such as `version` in a `package.json`, a
/// `product.json` or Codex's `codex-package.json`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct JsonValue {
    /// A `/`-separated path relative to the Installation's folder, where
    /// `*` and `?` stand for any name, such as a commit folder.
    pub file: String,
    /// The key path of the string, one key per segment.
    pub key_path: Vec<String>,
}

impl Validate for JsonValue {
    fn validate(&self) -> Result<(), String> {
        validate_relative(&self.file)?;
        validate_key_path(&self.key_path)
    }
}

impl ReadVersion for JsonValue {
    fn read(&self, installation: &InstallationPaths, ports: &Ports) -> Option<VersionRead> {
        files(installation, Some(&self.file), ports)
            .into_iter()
            .find_map(|path| {
                let text = String::from_utf8(ports.fs.read(&path).ok()?).ok()?;
                let document = JsonFormat::STRICT
                    .parse(text.trim_start_matches('\u{feff}'))
                    .ok()?;
                let value = self
                    .key_path
                    .iter()
                    .try_fold(&document, |value, key| value.get(key))?;
                VersionRead::new(value.as_str()?, path.display().to_string())
            })
    }
}

/// `DisplayVersion` in an uninstall key, such as an Inno Setup
/// `{<AppId>}_is1`: a fallback for an Installation whose files say nothing.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct UninstallKey {
    /// The hive the key is in, for an install per user or per machine.
    pub hive: RegistryHive,
    /// The key's name under `Software\Microsoft\Windows\CurrentVersion\Uninstall`.
    pub key: String,
}

/// A Windows registry hive.
#[derive(Debug, Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum RegistryHive {
    CurrentUser,
    LocalMachine,
}

impl Validate for UninstallKey {
    fn validate(&self) -> Result<(), String> {
        if self.key.trim().is_empty() || self.key.contains(['\\', '/']) {
            return Err(format!(
                "uninstall key `{}` must be one key name, without `\\` or `/`",
                self.key
            ));
        }
        Ok(())
    }
}

impl ReadVersion for UninstallKey {
    fn read(&self, _installation: &InstallationPaths, ports: &Ports) -> Option<VersionRead> {
        let key = format!(
            r"Software\Microsoft\Windows\CurrentVersion\Uninstall\{}",
            self.key
        );
        let text = ports
            .env
            .registry_value(self.hive.port(), &key, "DisplayVersion")?;
        VersionRead::new(&text, format!(r"{}\{key}", self.hive.short()))
    }
}

/// The files `file` names in the Installation's folder, sorted, or its
/// executable when `file` is `None`.
fn files(installation: &InstallationPaths, file: Option<&str>, ports: &Ports) -> Vec<PathBuf> {
    match file {
        Some(file) => paths::expand_relative(ports.fs.as_ref(), installation.folder, file),
        None => vec![installation.executable.to_path_buf()],
    }
}

// --- Paths ------------------------------------------------------------------

/// A key path names at least one key, none of them empty.
pub(crate) fn validate_key_path(key_path: &[String]) -> Result<(), String> {
    if key_path.is_empty() || key_path.iter().any(String::is_empty) {
        return Err("key_path needs keys, none of them empty".into());
    }
    Ok(())
}

/// A `/`-separated path must stay below the folder it is relative to.
pub(crate) fn validate_relative(path: &str) -> Result<(), String> {
    let normal = !path.is_empty()
        && Path::new(path)
            .components()
            .all(|component| matches!(component, Component::Normal(_)));
    if normal {
        Ok(())
    } else {
        Err(format!(
            "`{path}` must be a relative path without `.` or `..`"
        ))
    }
}

/// Joins a `/`-separated relative path so the result uses native separators.
pub(crate) fn join_relative(base: &Path, relative: &str) -> PathBuf {
    relative
        .split('/')
        .fold(base.to_path_buf(), |path, part| path.join(part))
}
