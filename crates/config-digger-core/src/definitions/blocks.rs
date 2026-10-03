//! The building blocks Agent definitions are assembled from (ADR 0005).
//!
//! Each family is a closed enum tagged by `block`. Every variant wraps a
//! parameter struct that implements [`Validate`] and its family's trait, so a
//! new block is added in one place and the compiler points at every `match`
//! that must handle it.

use crate::edit::{Edit, Syntax};
use crate::installations::pe;
use crate::paths;
use crate::ports::{Environment, FileSystem, Hive, Ports};
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

/// Finds one root of an Agent home on this machine.
pub(crate) trait Locate {
    /// The root's path, or `None` when the environment cannot say where it is.
    fn locate(&self, env: &dyn Environment) -> Option<PathBuf>;
}

/// Where one root of an Agent home lives.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "block", rename_all = "kebab-case")]
pub(crate) enum Locator {
    Path(PathLocator),
}

impl Validate for Locator {
    fn validate(&self) -> Result<(), String> {
        match self {
            Locator::Path(block) => block.validate(),
        }
    }
}

impl Locate for Locator {
    fn locate(&self, env: &dyn Environment) -> Option<PathBuf> {
        match self {
            Locator::Path(block) => block.locate(env),
        }
    }
}

/// The user's home folder, written `~`, or a fixed path below it, written
/// `~/<relative path>`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct PathLocator {
    pub path: String,
}

impl PathLocator {
    /// The path below the user's home folder, `None` for the folder itself.
    fn relative(&self) -> Result<Option<&str>, String> {
        if self.path == "~" {
            return Ok(None);
        }
        match self.path.strip_prefix("~/") {
            Some(relative) => validate_relative(relative).map(|()| Some(relative)),
            None => Err(format!(
                "path `{}` must be `~` or start with `~/`",
                self.path
            )),
        }
    }
}

impl Validate for PathLocator {
    fn validate(&self) -> Result<(), String> {
        self.relative().map(|_| ())
    }
}

impl Locate for PathLocator {
    fn locate(&self, env: &dyn Environment) -> Option<PathBuf> {
        let home = env.home_dir()?;
        match self.relative().ok()? {
            Some(relative) => Some(join_relative(&home, relative)),
            None => Some(home),
        }
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
}

impl Validate for Anchor {
    fn validate(&self) -> Result<(), String> {
        match self {
            Anchor::Home(block) => block.validate(),
            Anchor::Project(block) => block.validate(),
            Anchor::Machine(block) => block.validate(),
            Anchor::Settings(block) => block.validate(),
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
        }
    }

    fn home_root(&self) -> Option<&str> {
        match self {
            Anchor::Home(block) => block.home_root(),
            Anchor::Project(block) => block.home_root(),
            Anchor::Machine(block) => block.home_root(),
            Anchor::Settings(block) => block.home_root(),
        }
    }

    fn in_project(&self) -> bool {
        match self {
            Anchor::Home(block) => block.in_project(),
            Anchor::Project(block) => block.in_project(),
            Anchor::Machine(block) => block.in_project(),
            Anchor::Settings(block) => block.in_project(),
        }
    }

    fn machine(&self) -> Option<&str> {
        match self {
            Anchor::Home(block) => block.machine(),
            Anchor::Project(block) => block.machine(),
            Anchor::Machine(block) => block.machine(),
            Anchor::Settings(block) => block.machine(),
        }
    }

    fn walk(&self) -> ProjectWalk {
        match self {
            Anchor::Home(block) => block.walk(),
            Anchor::Project(block) => block.walk(),
            Anchor::Machine(block) => block.walk(),
            Anchor::Settings(block) => block.walk(),
        }
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
    /// A `/`-separated path relative to the root. Omitted when the root is the location.
    pub path: Option<String>,
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
    fn path(&self, roots: &BTreeMap<String, PathBuf>, _project: Option<&Path>) -> Option<PathBuf> {
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

/// A fixed path outside any Agent home or Project, read once for every
/// home of the Agent, such as a folder an organisation deploys to every
/// user of the machine.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct MachineAnchor {
    /// Starts with `%NAME%`, expanded from the environment, or a drive;
    /// `/`-separated, without wildcards.
    pub path: String,
}

impl Validate for MachineAnchor {
    fn validate(&self) -> Result<(), String> {
        paths::validate_glob(&self.path)?;
        if self.path.starts_with("~/") {
            return Err(format!(
                "machine path `{}` must start with `%NAME%` or a drive",
                self.path
            ));
        }
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

impl Anchor {
    /// The id of the `[[settings]]` the anchor reads, if it reads settings.
    pub fn settings(&self) -> Option<&str> {
        match self {
            Anchor::Settings(block) => Some(&block.settings),
            Anchor::Home(_) | Anchor::Project(_) | Anchor::Machine(_) => None,
        }
    }

    /// Refuses an anchor of settings, where one file is needed.
    pub fn one_file(&self) -> Result<(), String> {
        match self.settings() {
            Some(_) => Err("a settings anchor names several files; one file is needed here".into()),
            None => Ok(()),
        }
    }
}

// --- Settings sources -------------------------------------------------------

/// Where an Agent can read one set of its settings, such as the managed
/// settings an organisation deploys.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "block", rename_all = "kebab-case")]
pub(crate) enum SettingsSource {
    RegistryValue(RegistryValueSource),
    Files(FilesSource),
}

impl Validate for SettingsSource {
    fn validate(&self) -> Result<(), String> {
        match self {
            SettingsSource::RegistryValue(block) => block.validate(),
            SettingsSource::Files(block) => block.validate(),
        }
    }
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
        if self.key.trim().is_empty() || self.key.contains('/') || self.key.starts_with('\\') {
            return Err(format!(
                "registry key `{}` must be `\\`-separated below its hive",
                self.key
            ));
        }
        if self.value.trim().is_empty() {
            return Err("a registry value needs a name".into());
        }
        Ok(())
    }
}

impl RegistryValueSource {
    /// The hive as the port knows it.
    pub fn hive(&self) -> Hive {
        match self.hive {
            RegistryHive::CurrentUser => Hive::CurrentUser,
            RegistryHive::LocalMachine => Hive::LocalMachine,
        }
    }

    /// How the value is shown, standing for its path.
    pub fn path(&self) -> PathBuf {
        let hive = match self.hive {
            RegistryHive::CurrentUser => "HKCU",
            RegistryHive::LocalMachine => "HKLM",
        };
        PathBuf::from(format!(r"{hive}\{}\{}", self.key, self.value))
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
        }
        Ok(())
    }
}

// --- Project registries -----------------------------------------------------

/// Lists the Projects an Agent has recorded, in the Agent's own spelling.
pub(crate) trait Register {
    /// Where the registry lives.
    fn anchor(&self) -> &Anchor;

    /// The Project paths the registry's text records.
    fn spellings(&self, text: &str) -> Result<Vec<String>, ParseError>;
}

/// Where an Agent records the Projects it has been started on.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "block", rename_all = "kebab-case")]
pub(crate) enum Registry {
    JsonKeys(JsonKeys),
}

impl Validate for Registry {
    fn validate(&self) -> Result<(), String> {
        match self {
            Registry::JsonKeys(block) => block.validate(),
        }
    }
}

impl Register for Registry {
    fn anchor(&self) -> &Anchor {
        match self {
            Registry::JsonKeys(block) => block.anchor(),
        }
    }

    fn spellings(&self, text: &str) -> Result<Vec<String>, ParseError> {
        match self {
            Registry::JsonKeys(block) => block.spellings(text),
        }
    }
}

/// The keys of an object in a strict JSON file, each one a Project path.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct JsonKeys {
    /// Where the file is. It must not lie in a Project.
    pub anchor: Anchor,
    /// The key path of the object whose keys are Project paths.
    pub root: Vec<String>,
}

impl Validate for JsonKeys {
    fn validate(&self) -> Result<(), String> {
        self.anchor.validate()?;
        self.anchor.one_file()?;
        if self.anchor.in_project() {
            return Err("a project registry must not lie in a Project".into());
        }
        if self.anchor.machine().is_some() {
            return Err("a project registry must lie in the Agent home".into());
        }
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

    fn spellings(&self, text: &str) -> Result<Vec<String>, ParseError> {
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
}

impl Provenance {
    /// Whether config-digger only reads the Entries: it never Disables,
    /// Enables or Removes them.
    pub fn read_only(self) -> bool {
        match self {
            Provenance::File => false,
            Provenance::Managed | Provenance::CloudMirror => true,
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
        let Some(folder) = self.folder.locate(env) else {
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
}

impl Validate for Format {
    fn validate(&self) -> Result<(), String> {
        match self {
            Format::Json(block) => block.validate(),
            Format::Toml(block) => block.validate(),
            Format::SkillFolder(block) => block.validate(),
        }
    }
}

impl Format {
    /// How a file location is parsed, `None` for a folder of skill folders.
    pub fn file(&self) -> Option<&dyn Parse> {
        match self {
            Format::Json(block) => Some(block),
            Format::Toml(block) => Some(block),
            Format::SkillFolder(_) => None,
        }
    }

    /// How a folder of skill folders is read, `None` for a file location.
    pub fn skill_folder(&self) -> Option<&SkillFolder> {
        match self {
            Format::SkillFolder(block) => Some(block),
            Format::Json(_) | Format::Toml(_) => None,
        }
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
/// and `_`, plus `${namespace:argument}` for each listed namespace, and
/// `${NAME<separator>default}` when a default separator is set, whose
/// default is literal text.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct Braces {
    /// Such as `env` and `input` for `${env:NAME}` and `${input:id}`.
    #[serde(default)]
    pub namespaces: Vec<String>,
    /// Such as `:-` for `${NAME:-default}`.
    pub default_separator: Option<String>,
}

impl Validate for Braces {
    fn validate(&self) -> Result<(), String> {
        if let Some(namespace) = self.namespaces.iter().find(|ns| !is_name(ns)) {
            return Err(format!("namespace `{namespace}` is not a name"));
        }
        if self.default_separator.as_deref() == Some("") {
            return Err("default_separator must not be empty".into());
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
                    Some((name, _)) if is_name(name) => {
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
        match inner.split_once(':') {
            Some((namespace, argument)) => {
                self.namespaces.iter().any(|ns| ns == namespace) && !argument.is_empty()
            }
            None => is_name(inner),
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
    /// Entries with equal keys are the same Item.
    fn key(&self, name: &str) -> String;
}

/// How same-kind Entries of one Agent home compete.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "block", rename_all = "kebab-case")]
pub(crate) enum Competition {
    ExactName(ExactName),
}

impl Validate for Competition {
    fn validate(&self) -> Result<(), String> {
        match self {
            Competition::ExactName(block) => block.validate(),
        }
    }
}

impl Compete for Competition {
    fn key(&self, name: &str) -> String {
        match self {
            Competition::ExactName(block) => block.key(name),
        }
    }
}

/// Entries compete when their names are exactly equal.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ExactName {}

impl Validate for ExactName {
    fn validate(&self) -> Result<(), String> {
        Ok(())
    }
}

impl Compete for ExactName {
    fn key(&self, name: &str) -> String {
        name.to_owned()
    }
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

    fn locations(&self) -> &[String];

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

    /// The edit that Disables the Entry named `name` through the switch at
    /// `key_path`, which holds `value` now.
    fn disable_edit(&self, key_path: Vec<String>, value: Option<&Value>, name: &str) -> Edit;

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
    JsonField(JsonField),
}

impl Validate for DisableMechanism {
    fn validate(&self) -> Result<(), String> {
        match self {
            DisableMechanism::JsonNameList(block) => block.validate(),
            DisableMechanism::JsonField(block) => block.validate(),
        }
    }
}

impl Disables for DisableMechanism {
    fn switch(&self) -> &Switch {
        match self {
            DisableMechanism::JsonNameList(block) => block.switch(),
            DisableMechanism::JsonField(block) => block.switch(),
        }
    }

    fn target(&self) -> MechanismTarget {
        match self {
            DisableMechanism::JsonNameList(block) => block.target(),
            DisableMechanism::JsonField(block) => block.target(),
        }
    }

    fn context(&self) -> MechanismContext {
        match self {
            DisableMechanism::JsonNameList(block) => block.context(),
            DisableMechanism::JsonField(block) => block.context(),
        }
    }

    fn reason(&self) -> DisableReason {
        match self {
            DisableMechanism::JsonNameList(block) => block.reason(),
            DisableMechanism::JsonField(block) => block.reason(),
        }
    }

    fn locations(&self) -> &[String] {
        match self {
            DisableMechanism::JsonNameList(block) => block.locations(),
            DisableMechanism::JsonField(block) => block.locations(),
        }
    }
    fn enable_offered(&self) -> bool {
        match self {
            DisableMechanism::JsonNameList(block) => block.enable_offered(),
            DisableMechanism::JsonField(block) => block.enable_offered(),
        }
    }

    fn disables(&self, value: Option<&Value>, name: &str) -> bool {
        match self {
            DisableMechanism::JsonNameList(block) => block.disables(value, name),
            DisableMechanism::JsonField(block) => block.disables(value, name),
        }
    }

    fn partial(&self, value: Option<&Value>) -> bool {
        match self {
            DisableMechanism::JsonNameList(block) => block.partial(value),
            DisableMechanism::JsonField(block) => block.partial(value),
        }
    }

    fn disable_edit(&self, key_path: Vec<String>, value: Option<&Value>, name: &str) -> Edit {
        match self {
            DisableMechanism::JsonNameList(block) => block.disable_edit(key_path, value, name),
            DisableMechanism::JsonField(block) => block.disable_edit(key_path, value, name),
        }
    }

    fn enable_edits(&self, key_path: Vec<String>, value: Option<&Value>, name: &str) -> Vec<Edit> {
        match self {
            DisableMechanism::JsonNameList(block) => block.enable_edits(key_path, value, name),
            DisableMechanism::JsonField(block) => block.enable_edits(key_path, value, name),
        }
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
        validate_mechanism(&self.switch, self.context)?;
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

    fn disables(&self, value: Option<&Value>, name: &str) -> bool {
        occurrences(value, name) > 0
    }

    fn partial(&self, _value: Option<&Value>) -> bool {
        false
    }

    /// An append, which creates the list when it is missing.
    fn disable_edit(&self, key_path: Vec<String>, _value: Option<&Value>, name: &str) -> Edit {
        Edit::Append {
            key_path,
            value: Value::from(name),
        }
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
    /// Values that are partial switches, limiting part of the Entry, such
    /// as a skill listed by name only: the Entry stays Effective, with a note.
    #[serde(default)]
    pub partial_values: Vec<Value>,
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

impl Validate for JsonField {
    fn validate(&self) -> Result<(), String> {
        validate_mechanism(&self.switch, self.context)?;
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

    fn disables(&self, value: Option<&Value>, _name: &str) -> bool {
        value == Some(&self.disabled_value)
    }

    fn partial(&self, value: Option<&Value>) -> bool {
        value.is_some_and(|value| self.partial_values.contains(value))
    }

    fn disable_edit(&self, key_path: Vec<String>, _value: Option<&Value>, _name: &str) -> Edit {
        Edit::Set {
            key_path,
            value: self.disabled_value.clone(),
        }
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

/// Checks what every mechanism shares: its switch, and a context that
/// agrees with where the switch is.
fn validate_mechanism(switch: &Switch, context: MechanismContext) -> Result<(), String> {
    switch.validate()?;
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
    pub anchor: Anchor,
    /// The key path of the value written. One segment may be `{project}`:
    /// the context's Project key, in the Agent's own spelling, so that the
    /// switch applies only there. One segment after it may be `{name}`:
    /// the Entry's name.
    pub key_path: Vec<String>,
    /// How the switch's file behaves while the Agent runs.
    pub store_class: StoreClass,
}

/// The switch `key_path` segment that stands for the Entry's name.
pub(crate) const NAME_KEY: &str = "{name}";

impl Validate for Switch {
    fn validate(&self) -> Result<(), String> {
        self.anchor.validate()?;
        self.anchor.one_file()?;
        if self.key_path.is_empty() || self.key_path.iter().any(String::is_empty) {
            return Err("a switch needs a key path, with no empty key".into());
        }
        let position = |template: &str| self.key_path.iter().position(|key| key == template);
        let count = |template: &str| self.key_path.iter().filter(|key| *key == template).count();
        for template in [super::PROJECT_KEY, NAME_KEY] {
            if count(template) > 1 {
                return Err(format!("a switch key path holds `{template}` twice"));
            }
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
        if let (Some(project), Some(name)) = (position(super::PROJECT_KEY), position(NAME_KEY))
            && name < project
        {
            return Err(format!(
                "`{}` must come before `{NAME_KEY}` in a switch key path",
                super::PROJECT_KEY
            ));
        }
        Ok(())
    }
}

impl Switch {
    /// Whether the switch applies in one Project only, so that it needs one.
    pub fn per_project(&self) -> bool {
        self.anchor.in_project() || self.key_path.iter().any(|key| key == super::PROJECT_KEY)
    }

    /// Whether its key path holds `{name}`, so that it holds one value per name.
    pub fn names(&self) -> bool {
        self.key_path.iter().any(|key| key == NAME_KEY)
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
}

impl Validate for SettingPlace {
    fn validate(&self) -> Result<(), String> {
        self.anchor.validate()?;
        validate_key_path(&self.key_path)?;
        if self.anchor.walk() != ProjectWalk::None {
            return Err("a setting cannot walk up from the Project folder".into());
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
/// that holds it decides; the Entries are Disabled globally, or Blocked
/// when that source is policy.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct GlobalSwitch {
    /// Highest first.
    pub sources: Vec<SettingPlace>,
    /// The value that turns the Entries off.
    pub off: Value,
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
}

impl Validate for Gate {
    fn validate(&self) -> Result<(), String> {
        match self {
            Gate::ManagedExclusive(block) => block.validate(),
            Gate::ApprovalList(block) => block.validate(),
        }
    }
}

impl Gate {
    /// Whether it can Block the Entries of the location `id`.
    pub fn covers(&self, id: &str) -> bool {
        match self {
            Gate::ManagedExclusive(block) => block.covers(id),
            Gate::ApprovalList(block) => block.locations.iter().any(|location| location == id),
        }
    }

    /// The settings it reads.
    pub fn sources(&self) -> &[SettingPlace] {
        match self {
            Gate::ManagedExclusive(_) => &[],
            Gate::ApprovalList(block) => &block.sources,
        }
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
    let at = cached_at.and_then(at_path).and_then(|value| match value {
        Value::Number(millis) => {
            let millis = millis.as_f64().filter(|millis| *millis >= 0.0)?;
            SystemTime::UNIX_EPOCH.checked_add(Duration::try_from_secs_f64(millis / 1000.0).ok()?)
        }
        Value::String(text) => humantime::parse_rfc3339_weak(text).ok(),
        _ => None,
    });
    Some(Cached { on, at })
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
                if name.trim().is_empty() || name.contains('=') {
                    return Err(format!("`{name}` is not a variable name"));
                }
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
/// Agent reads only while a Feature gate is on.
pub(crate) trait Holds {
    /// Whether it holds, given whether each Feature gate is on, by name.
    fn holds(&self, gate_on: &dyn Fn(&str) -> bool) -> bool;

    /// The Feature gate it depends on, if any.
    fn gate(&self) -> Option<&str>;
}

/// A condition under which an Agent reads a location.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "block", rename_all = "kebab-case")]
pub(crate) enum Condition {
    FeatureGate(GateCondition),
}

impl Validate for Condition {
    fn validate(&self) -> Result<(), String> {
        match self {
            Condition::FeatureGate(block) => block.validate(),
        }
    }
}

impl Holds for Condition {
    fn holds(&self, gate_on: &dyn Fn(&str) -> bool) -> bool {
        match self {
            Condition::FeatureGate(block) => block.holds(gate_on),
        }
    }

    fn gate(&self) -> Option<&str> {
        match self {
            Condition::FeatureGate(block) => block.gate(),
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
    fn holds(&self, gate_on: &dyn Fn(&str) -> bool) -> bool {
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
        let (hive, name) = match self.hive {
            RegistryHive::CurrentUser => (Hive::CurrentUser, "HKCU"),
            RegistryHive::LocalMachine => (Hive::LocalMachine, "HKLM"),
        };
        let text = ports.env.registry_value(hive, &key, "DisplayVersion")?;
        VersionRead::new(&text, format!(r"{name}\{key}"))
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
