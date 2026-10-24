//! Agent definitions: TOML data over the closed set of building blocks in
//! [`blocks`]. Taking out its `unverified` [`marks`], deserialisation and
//! `validate()` are the only validation path.

pub(crate) mod blocks;
pub mod marks;
pub mod skill_managers;

use crate::ports::{Environment, FileSystem};
use crate::version::{Standing, Version};
use blocks::{
    Anchor, Anchored, Bypass, Bypasses, Competition, Condition, DisableMechanism, Disables,
    FindsBy, Format, Gate, GateLookup, Holds, HomeCondition, Interpolation, ListPlugins, Locate,
    Locator, LookUp, Merge, PluginRegistry, ProjectWalk, Provenance, Register, Registry, Rule,
    SessionFiles, SettingOneOf, SettingsSource, Shape, StoreClass, Validate, VersionSource,
};
use schemars::JsonSchema;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// The definitions that ship inside config-digger, embedded at compile time.
const BUILT_IN: &[(&str, &str)] = &[
    (
        "claude-code.toml",
        include_str!("../../definitions/claude-code.toml"),
    ),
    (
        "claude-desktop.toml",
        include_str!("../../definitions/claude-desktop.toml"),
    ),
    ("vscode.toml", include_str!("../../definitions/vscode.toml")),
    ("cursor.toml", include_str!("../../definitions/cursor.toml")),
    ("codex.toml", include_str!("../../definitions/codex.toml")),
    (
        "gemini-cli.toml",
        include_str!("../../definitions/gemini-cli.toml"),
    ),
];

/// The only `format_version` this release reads.
const FORMAT_VERSION: u32 = 1;

/// The JSON Schema of an Agent definition file, generated from the block enums.
pub fn json_schema() -> String {
    marks::json_schema::<AgentDefinition>()
}

/// Every `unverified` mark of the built-in definitions, the Agent
/// definitions' first and then the skill manager definitions', file by
/// file in load order, each in the order they appear: what the release
/// gate still has to test. A skill manager definition's file is named
/// inside its `skill-managers` folder.
pub fn unverified_marks() -> Result<Vec<marks::Mark>, String> {
    let mut found = Vec::new();
    for (file, text) in BUILT_IN {
        let (_, marks) = parse_marked(file, text).map_err(|e| e.to_string())?;
        found.extend(marks);
    }
    for (file, text) in skill_managers::BUILT_IN {
        let file = format!("skill-managers/{file}");
        let (_, marks) = skill_managers::parse_marked(&file, text).map_err(|e| e.to_string())?;
        found.extend(marks);
    }
    Ok(found)
}

/// The `unverified` marks of the Agent definition `text` of `file`, in the
/// order they appear, once it loads.
pub fn agent_marks(file: &str, text: &str) -> Result<Vec<marks::Mark>, String> {
    parse_marked(file, text)
        .map(|(_, marks)| marks)
        .map_err(|e| e.to_string())
}

/// Checks the definition files in `folder`, laid out as the built-in ones
/// are in the source tree: each `*.toml` file an Agent definition, and each
/// one in its `skill-managers` folder a skill manager definition, as CI
/// checks them. Every file must load, and the files must be exactly those
/// built in, so that none ships unchecked and none is checked but not
/// shipped. Returns one line per problem, Agent definitions first, each
/// kind in the order of the files' names.
pub fn check_built_in_folder(folder: &Path) -> Vec<String> {
    let mut problems = Vec::new();
    check_files(folder, BUILT_IN, &mut problems, |file, text| {
        parse(file, text).map(drop)
    });
    let managers = folder.join("skill-managers");
    check_files(
        &managers,
        skill_managers::BUILT_IN,
        &mut problems,
        |file, text| skill_managers::parse(file, text).map(drop),
    );
    problems
}

/// Checks the `*.toml` files in `folder` against `built_in`, each loaded
/// with `load`, adding a line to `problems` for each that fails.
fn check_files(
    folder: &Path,
    built_in: &[(&str, &str)],
    problems: &mut Vec<String>,
    load: impl Fn(&str, &str) -> Result<(), DefinitionError>,
) {
    let mut files: Vec<PathBuf> = match std::fs::read_dir(folder) {
        Ok(entries) => entries
            .filter_map(|entry| Some(entry.ok()?.path()))
            .filter(|path| {
                path.is_file()
                    && path
                        .extension()
                        .is_some_and(|extension| extension.eq_ignore_ascii_case("toml"))
            })
            .collect(),
        Err(error) => {
            problems.push(format!("cannot read {}: {error}", folder.display()));
            return;
        }
    };
    files.sort();
    for file in &files {
        let name = file.file_name().unwrap_or_default().to_string_lossy();
        if !built_in.iter().any(|(known, _)| *known == name) {
            problems.push(format!(
                "{} is not built in: list it with the built-in definitions",
                file.display()
            ));
            continue;
        }
        match std::fs::read_to_string(file) {
            Ok(text) => {
                if let Err(error) = load(&file.display().to_string(), &text) {
                    problems.push(error.to_string());
                }
            }
            Err(error) => problems.push(format!("cannot read {}: {error}", file.display())),
        }
    }
    for (known, _) in built_in {
        if !files
            .iter()
            .any(|file| file.file_name().is_some_and(|name| name == *known))
        {
            problems.push(format!(
                "{} is missing: it is built in",
                folder.join(known).display()
            ));
        }
    }
}

/// A definition file that failed to load. It is rejected whole.
#[derive(Debug, thiserror::Error)]
#[error("{kind} definition `{file}` is invalid: {message}")]
pub(crate) struct DefinitionError {
    /// Which kind of definition it is: `Agent` or `skill manager`.
    pub kind: &'static str,
    pub file: String,
    pub message: String,
}

/// The loaded definitions: the Agent definitions, which it derefs to, and
/// the skill manager definitions that recognise who installed a skill
/// folder of any of them, each in load order.
pub(crate) struct Definitions {
    pub agents: Vec<AgentDefinition>,
    pub skill_managers: Vec<skill_managers::SkillManagerDefinition>,
}

impl Definitions {
    /// The built-in definitions of both kinds.
    pub fn built_in() -> Result<Self, DefinitionError> {
        Ok(Self {
            agents: built_in()?,
            skill_managers: skill_managers::built_in()?,
        })
    }
}

impl std::ops::Deref for Definitions {
    type Target = [AgentDefinition];

    fn deref(&self) -> &[AgentDefinition] {
        &self.agents
    }
}

/// The built-in Agent definitions, in load order.
pub(crate) fn built_in() -> Result<Vec<AgentDefinition>, DefinitionError> {
    BUILT_IN
        .iter()
        .map(|(file, text)| parse(file, text))
        .collect()
}

pub(crate) fn parse(file: &str, text: &str) -> Result<AgentDefinition, DefinitionError> {
    parse_marked(file, text).map(|(definition, _)| definition)
}

/// The Agent definition `text` of `file`, and its marks.
fn parse_marked(
    file: &str,
    text: &str,
) -> Result<(AgentDefinition, Vec<marks::Mark>), DefinitionError> {
    let error = |message: String| DefinitionError {
        kind: "Agent",
        file: file.to_owned(),
        message,
    };
    let (definition, marks): (AgentDefinition, _) =
        marks::deserialize(file, text).map_err(error)?;
    definition.validate().map_err(error)?;
    Ok((definition, marks))
}

/// Where an Agent keeps its configuration, in which format, and with what precedence.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "config-digger Agent definition")]
pub(crate) struct AgentDefinition {
    /// The version of this file format. Must be 1.
    pub format_version: u32,
    /// The lowest and highest versions of the Agent the real-Agent tests
    /// passed on. Without it, the definition is untested, which every
    /// Agent home of it notes.
    pub verified: Option<VerifiedRange>,
    pub agent: AgentInfo,
    /// The launchable copies of the Agent, each with its own version.
    #[serde(default)]
    pub installations: Vec<InstallationDefinition>,
    /// Where the Agent records the version that last ran against an Agent
    /// home, shown next to the Installations.
    pub last_ran: Option<LastRan>,
    /// How to tell that the Agent is running. Without it, config-digger
    /// never finds it running.
    pub process: Option<ProcessDefinition>,
    /// The Agent homes this Agent can have.
    pub homes: Vec<HomeDefinition>,
    /// Agent homes another tool copies from one of `homes`, such as the
    /// Codex home Orca keeps in sync with `~\.codex`. config-digger reads
    /// them and never writes to them.
    #[serde(default)]
    pub mirrored_homes: Vec<MirroredHomeDefinition>,
    /// The orders in which a launch of the Agent picks one of its homes,
    /// each a list of home ids, such as the candidates for Claude Desktop's
    /// data folder of each of its packages: it reads the first whose `when`
    /// holds and whose folder exists, or that it uses before it exists. A
    /// home some order holds and none picks is skipped, and shown as not
    /// read. A home outside every order is read wherever it is found.
    #[serde(default)]
    pub lookup_orders: Vec<Vec<String>>,
    /// The Scopes this Agent applies Entries at, in display order. A Scope is
    /// only a label: precedence comes from the order of locations in `items`.
    pub scopes: Vec<ScopeDefinition>,
    /// Sets of settings the Agent reads from the first of several sources
    /// that holds any, such as managed settings.
    #[serde(default)]
    pub settings: Vec<SettingsDefinition>,
    /// Where this Agent records the Projects it has been started on.
    #[serde(default)]
    pub projects: ProjectsDefinition,
    /// The plugins this Agent installs, each in a folder of its own that
    /// locations anchored in plugins are read in.
    #[serde(default)]
    pub plugins: Vec<PluginsDefinition>,
    /// The configuration locations this Agent reads.
    pub locations: Vec<LocationDefinition>,
    /// Rules per Item kind.
    pub items: BTreeMap<ItemKind, ItemRules>,
    /// Switches on the Agent vendor's side that change how the Agent treats
    /// its configuration, only those that change a State.
    #[serde(default)]
    pub feature_gates: Vec<FeatureGateDefinition>,
    /// Files on the machine that the Agent reads somewhere else while an
    /// environment variable says so, or only while an administrator owns
    /// them. Every anchor that names one's path reads it so.
    #[serde(default)]
    pub machine_files: Vec<MachineFileDefinition>,
}

/// A file on the machine, by the path the definition's machine anchors
/// name it with, such as Gemini CLI's system settings, which
/// `GEMINI_CLI_SYSTEM_SETTINGS_PATH` moves. It is where its variable says
/// while that is set to an absolute path; else, with `beside`, in the folder
/// of that other file, wherever that is, under its own name; else at its
/// path.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct MachineFileDefinition {
    /// Starts with `%NAME%` or a drive; `/`-separated, without wildcards.
    pub path: String,
    /// The environment variable that holds the path of the file the Agent
    /// reads instead.
    pub variable: Option<String>,
    /// The path of another of `machine_files`, one with no `beside` of its
    /// own, whose folder this file follows, such as Gemini CLI's
    /// `system-defaults.json` beside its system settings.
    pub beside: Option<String>,
    /// Whether the Agent reads the file only while only an administrator
    /// can change it and the folders above it, as Gemini CLI reads its
    /// system settings: otherwise its Entries are Unread, and nothing it
    /// holds is read.
    #[serde(default)]
    pub admin_owned: bool,
}

impl MachineFileDefinition {
    /// Where the file is on this machine, given where the file it is beside
    /// is, as far as the environment says.
    fn locate(&self, env: &dyn Environment, beside: Option<PathBuf>) -> Option<PathBuf> {
        if let Some(moved) = self
            .variable
            .as_deref()
            .and_then(|variable| env.var(variable))
            .map(|value| crate::paths::lexical(Path::new(value.trim())))
            .filter(|path| crate::paths::is_absolute(path))
        {
            return Some(moved);
        }
        if self.beside.is_some() {
            let name = self.path.rsplit('/').next().unwrap_or_default();
            return Some(beside?.parent()?.join(name));
        }
        crate::paths::expand(&self.path, env)
    }
}

/// A switch on the Agent vendor's side, such as whether Claude Code syncs
/// skills from claude.ai. config-digger sees at most the value the Agent
/// last cached, shown as of when it cached it; otherwise it assumes the
/// Agent's default. Every Entry it affects gets a caveat saying which.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct FeatureGateDefinition {
    /// The gate's name as the vendor knows it, unique in the definition.
    pub name: String,
    /// The value the Agent uses when it has none cached.
    pub assumed: bool,
    /// The ids of the locations whose Entries depend on it.
    pub affects: Vec<String>,
    /// Where the Agent caches the gate's value. Omitted when it caches it
    /// nowhere config-digger reads, such as the admin controls Gemini CLI
    /// fetches when it signs in. Omitted with `when` too, the Agent keeps
    /// it nowhere on disk, such as the settings profile Zed has active:
    /// each Entry it affects gets the caveat `gate_assumed`, at its own
    /// location.
    pub lookup: Option<GateLookup>,
    /// When the Agent ignores its cache and uses its own default. Only a
    /// gate with a lookup takes one.
    pub bypass: Option<Bypass>,
    /// For a gate without a lookup, when the Agent fetches it at all, such
    /// as Gemini CLI's admin controls for an account signed in through
    /// Google: while this setting holds, each Entry the gate affects gets
    /// the caveat `gate_may_apply`, since the vendor's side may turn it off
    /// where config-digger cannot see, and otherwise none.
    pub when: Option<SettingOneOf>,
}

/// One set of settings, which the Agent reads from the first of its
/// sources that holds any setting, and from no other, unless `merge` says
/// otherwise. Entries in the sources it skips are Unread.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SettingsDefinition {
    /// Unique in the definition, such as `managed`.
    pub id: String,
    /// Highest first.
    pub sources: Vec<SettingsSource>,
    /// Top-level keys that say how the Agent reads the sources rather than
    /// set anything, such as Claude Code's `managedSourcesBehavior`: a
    /// source holding only these holds no setting.
    #[serde(default)]
    pub control_keys: Vec<String>,
    /// The control key that, holding a value, has the Agent merge the source
    /// it reads with every source below it that holds any setting and that
    /// the user cannot write. The Agent reads it from the highest source that
    /// holds it or any setting.
    pub merge: Option<MergeKey>,
}

/// The control key that has the Agent merge sources of settings, and the
/// value that turns merging on.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct MergeKey {
    /// A top-level key, one of the settings' `control_keys`, such as
    /// `managedSourcesBehavior`.
    pub key: String,
    /// The value it holds while the Agent merges, such as `"merge"`.
    pub value: serde_json::Value,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct AgentInfo {
    /// A stable id, such as `claude-code`.
    pub id: String,
    /// The display name, such as `Claude Code`.
    pub name: String,
    /// What every Agent home of the Agent notes, such as configuration it
    /// keeps only in the user's account.
    #[serde(default)]
    pub notes: Vec<DefinitionNote>,
}

/// Something every Agent home of an Agent notes, from a closed set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum DefinitionNote {
    /// The Agent's connectors live in the user's account, with no copy on
    /// disk to list, such as Claude Desktop's.
    CloudConnectorsNotOnDisk,
    /// The plugins the user installs for themself from the Agent's
    /// marketplace live in the user's account, with no install record on
    /// disk, such as Cursor's.
    MarketplacePluginsNotOnDisk,
}

/// A range of Agent versions, both ends included. Versions compare
/// numerically, component by component, padded with zeros; a pre-release
/// suffix counts as the numeric version it follows.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct VerifiedRange {
    pub min: String,
    pub max: String,
}

impl VerifiedRange {
    /// Where `version` stands against the range.
    pub fn standing(&self, version: &Version) -> Standing {
        let parse = |text: &str| Version::parse(text).expect("validation parses the range");
        crate::version::standing(version, &parse(&self.min), &parse(&self.max))
    }
}

impl Validate for VerifiedRange {
    fn validate(&self) -> Result<(), String> {
        let parse =
            |text: &str| Version::parse(text).ok_or_else(|| format!("`{text}` is not a version"));
        if parse(&self.min)? > parse(&self.max)? {
            return Err(format!("min {} is above max {}", self.min, self.max));
        }
        Ok(())
    }
}

/// A launchable copy of the Agent, with its own version. It is found when
/// its executable exists, once per folder its `folder` glob matches.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct InstallationDefinition {
    /// A stable id, such as `native`, unique in the definition.
    pub id: String,
    /// A glob of the folder it is in, starting with `~/`, `%NAME%` or a
    /// drive; `*` and `?` stay within one folder name and `{a,b}` is
    /// either, such as a version folder.
    pub folder: String,
    /// The `/`-separated path of its executable, relative to its folder.
    pub executable: String,
    /// Where its version is, tried in order: the first one read wins.
    pub versions: Vec<VersionSource>,
    /// The ids of the Agent homes it reads. Omitted when it reads every
    /// one. One that reads the first home also reads the homes the user
    /// adds, which are laid out like it.
    pub homes: Option<Vec<String>>,
}

/// A string in a file of each Agent home: the version that last ran
/// against it, such as Claude Code's `lastReleaseNotesSeen`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct LastRan {
    /// A JSON file of the Agent home.
    pub anchor: Anchor,
    /// The key path of the string, one key per segment.
    pub key_path: Vec<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct HomeDefinition {
    pub id: String,
    /// The folder the home's roots are in.
    pub folder: Locator,
    /// Named roots of the home, each a `/`-separated path relative to its
    /// folder. The home is found when any root exists. An Agent home the
    /// user adds is laid out like the definition's first home.
    pub roots: BTreeMap<String, String>,
    /// The names of roots other Agents read too, such as `~\.agents\skills`,
    /// which do not on their own say the Agent keeps its configuration in
    /// the home: it is found when any other root exists.
    #[serde(default)]
    pub shared_roots: Vec<String>,
    /// How to tell that the Agent of this home is running, in place of the
    /// definition's `[process]`, such as for a home read by another
    /// Installation of the Agent.
    pub process: Option<ProcessDefinition>,
    /// Conditions under which the lookup order may pick the home, every one
    /// of them, such as `%APPDATA%` being on a network share. Only a home
    /// in the lookup order takes them.
    #[serde(default)]
    pub when: Vec<HomeCondition>,
    /// The home is the definition default, where the Agent keeps its
    /// configuration unless told otherwise. At most one home is, and its
    /// folder is one fixed path. Any other home found through a fixed path
    /// or a search is found at a known location, and one an environment
    /// variable names, through that variable.
    #[serde(default)]
    pub default: bool,
}

/// An Agent home whose content another tool copies from a source Agent
/// home, laid out like it. It is found, as a known location, when any of
/// its roots exists. A folder another home of the Agent finds too is this
/// home, whichever finds it.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct MirroredHomeDefinition {
    /// Unique among the definition's homes.
    pub id: String,
    /// The tool that keeps the copy, as it names itself, such as `Orca`.
    pub owner: String,
    /// The folder the home's roots are in.
    pub folder: Locator,
    /// The id of the home it copies, one of the definition's `homes`.
    pub source: String,
}

/// How to tell that an Agent is running. It runs when a process matches
/// `images`, `paths` and `cmdline_contains`, or a session file names a PID
/// that is alive. Detection is best-effort: it warns or blocks a write, and
/// config-digger never closes an Agent.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProcessDefinition {
    /// Image names, such as `claude.exe`, compared case-insensitively.
    #[serde(default)]
    pub images: Vec<String>,
    /// Globs the executable's path must match one of, when set, such as
    /// `~/.local/bin/claude.exe`: Claude Code and Claude Desktop are both
    /// `claude.exe`. A glob starts with `~/`, `%NAME%` or a drive; `*` and
    /// `?` stay within one folder name and `{a,b}` is either. Compared
    /// case-insensitively. A process whose path the OS hides matches on its
    /// image alone.
    #[serde(default)]
    pub paths: Vec<String>,
    /// Text the command line must contain, every one of them, compared
    /// case-insensitively, such as Gemini CLI's script in `node.exe`. A
    /// process whose command line the OS hides does not match: unlike a
    /// path, it narrows down images such as `node.exe` that run much else.
    #[serde(default)]
    pub cmdline_contains: Vec<String>,
    /// Files the Agent keeps per running session, each naming a PID that
    /// counts only while it is alive.
    pub session_files: Option<SessionFiles>,
}

impl Validate for ProcessDefinition {
    fn validate(&self) -> Result<(), String> {
        if self.images.is_empty() && self.session_files.is_none() {
            return Err("process needs images or session_files".into());
        }
        if self.images.is_empty() && !(self.paths.is_empty() && self.cmdline_contains.is_empty()) {
            return Err("process paths and cmdline_contains need images".into());
        }
        for text in self.images.iter().chain(&self.cmdline_contains) {
            non_empty("process image or cmdline_contains", text)?;
        }
        for glob in &self.paths {
            crate::paths::validate_glob(glob)?;
        }
        if let Some(session_files) = &self.session_files {
            session_files.validate()?;
        }
        Ok(())
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ScopeDefinition {
    pub id: String,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProjectsDefinition {
    /// Each registry adds the Projects it records. Projects found by any Agent
    /// are evaluated for every Agent.
    #[serde(default)]
    pub registries: Vec<Registry>,
    /// Whether the Agent opens `.code-workspace` files: such a Project
    /// reads `project-file` locations in the file itself, and the other
    /// locations in a Project in each of its member folders, the first
    /// member ranking highest.
    #[serde(default)]
    pub workspace_files: bool,
}

/// Plugins an Agent installs, each in a folder of its own, such as Claude
/// Code's plugin cache. A plugin's Entries are Disabled with the whole
/// plugin through its own switch, and never Removed: the Agent uninstalls
/// it.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct PluginsDefinition {
    /// Unique in the definition, such as `installed`.
    pub id: String,
    /// Where the Agent records the plugins it installed.
    pub registry: PluginRegistry,
    /// A file whose presence in a plugin's folder means the Agent no longer
    /// loads that folder, such as a version it superseded.
    pub superseded_marker: Option<String>,
    /// Where a plugin's name is, or a list of places tried in turn, such as
    /// an extension's `display_name`, then its `name`. When the plugin's
    /// folder holds none, its name is its id up to `id_separator`.
    pub name: Option<PluginNames>,
    /// The text that ends a plugin's name in its id, such as the `@` before
    /// a marketplace. Without it, a plugin with no name is named by its id.
    pub id_separator: Option<String>,
    /// Whether the Agent names a plugin by its id when one before it, in
    /// the registry's order, has its name, as Claude Desktop does with
    /// extensions, so that their Entries never share a name.
    #[serde(default)]
    pub unique_names: bool,
    /// Whether the Agent refuses plugins that share a name, as Gemini CLI
    /// refuses two extensions whose manifests give one `name`: the Entries
    /// of each are Invalid, with `duplicate_plugin_name`.
    #[serde(default)]
    pub same_names_invalid: bool,
    /// The Agent's own command that uninstalls a plugin, shown in place of
    /// Remove. `{id}` stands for the plugin's id, and `{scope}` for the
    /// Scope it is installed at: when that is not recorded, the word
    /// holding it and an option just before it are left out. Without it,
    /// the Agent uninstalls a plugin only through its own interface.
    pub uninstall: Option<String>,
}

/// Where a plugin's name is: one place, or several tried in turn.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(untagged)]
pub(crate) enum PluginNames {
    One(PluginName),
    Several(Vec<PluginName>),
}

impl PluginNames {
    /// The places, in the order they are tried.
    pub fn places(&self) -> &[PluginName] {
        match self {
            PluginNames::One(name) => std::slice::from_ref(name),
            PluginNames::Several(names) => names,
        }
    }
}

/// A string in a JSON file of a plugin's folder that names the plugin.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct PluginName {
    /// A `/`-separated path relative to the plugin's folder.
    pub file: String,
    /// The key path of the string, one key per segment.
    pub key_path: Vec<String>,
}

/// The `uninstall` segment that stands for a plugin's id.
pub(crate) const PLUGIN_ID: &str = "{id}";
/// The `uninstall` segment that stands for the Scope a plugin is installed at.
pub(crate) const PLUGIN_SCOPE: &str = "{scope}";

impl PluginsDefinition {
    fn validate(&self) -> Result<(), String> {
        non_empty("plugins id", &self.id)?;
        self.registry.validate()?;
        if let Some(marker) = &self.superseded_marker {
            blocks::validate_relative(marker)?;
        }
        for name in self.name.iter().flat_map(PluginNames::places) {
            blocks::validate_relative(&name.file)?;
            blocks::validate_key_path(&name.key_path)?;
        }
        if self.id_separator.as_ref().is_some_and(String::is_empty) {
            return Err("id_separator must not be empty".into());
        }
        if self.unique_names && self.same_names_invalid {
            return Err("plugins that share a name are renamed or invalid, not both".into());
        }
        if self
            .uninstall
            .as_ref()
            .is_some_and(|uninstall| !uninstall.contains(PLUGIN_ID))
        {
            return Err(format!("uninstall must name the plugin with `{PLUGIN_ID}`"));
        }
        Ok(())
    }

    /// The command that uninstalls the plugin `id`, installed at `scope`,
    /// `None` when the Agent has none.
    pub fn uninstall(&self, id: &str, scope: Option<&str>) -> Option<String> {
        let command = self.uninstall.as_ref()?.replace(PLUGIN_ID, id);
        if let Some(scope) = scope {
            return Some(command.replace(PLUGIN_SCOPE, scope));
        }
        let mut words: Vec<&str> = Vec::new();
        for word in command.split_whitespace() {
            if !word.contains(PLUGIN_SCOPE) {
                words.push(word);
            } else if words.last().is_some_and(|option| option.starts_with('-')) {
                words.pop();
            }
        }
        Some(words.join(" "))
    }
}

/// The `root` segment that stands for any Project path key.
pub(crate) const PROJECT_KEY: &str = "{project}";
/// The `root` segment that stands for every key.
pub(crate) const ANY_KEY: &str = "*";

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct LocationDefinition {
    pub id: String,
    pub anchor: Anchor,
    pub format: Format,
    /// The key path of the object whose keys are Entries. One segment may be
    /// `{project}`: each key there is a Project path, and the Entries below it
    /// apply only in that Project. Any segment may be `*`, which stands for
    /// every key there, such as each extension's servers in VS Code's cache.
    /// Omitted for a folder of skill folders.
    #[serde(default)]
    pub root: Vec<String>,
    /// When the value at `root` is a list of objects rather than an object
    /// keyed by name, the field of each that holds its name, such as the
    /// `label` of each server in VS Code's cache of extension servers. An
    /// item without it is no Entry.
    pub named_by: Option<String>,
    /// Whether a file without `root` holds its Entries at its top level
    /// instead, as VS Code reads a `.mcp.json` that is a bare object of
    /// servers.
    #[serde(default)]
    pub bare: bool,
    /// Whether the value at `root` is one Entry rather than an object of
    /// them, named by the plugin it is part of, such as the server a Claude
    /// Desktop extension's manifest declares in `server.mcp_config`. Only a
    /// location in plugins can hold one.
    #[serde(default)]
    pub one_entry: bool,
    pub kind: ItemKind,
    /// The id of a declared Scope.
    pub scope: String,
    /// The base Provenance of the location's Entries.
    pub provenance: Provenance,
    /// How the location's file behaves while the Agent runs.
    pub store_class: StoreClass,
    /// How the Agent writes references in this location's values. Without
    /// it, every value is literal text.
    pub interpolation: Option<Interpolation>,
    /// Fields of this location's Entries that hold Secrets, or that never
    /// do, beyond what the core masks by default. The fields, URL fields
    /// and encrypted prefixes it marks are Secrets in the Entries of every
    /// location that reads the same file, whichever Agent's it is.
    #[serde(default)]
    pub secrets: SecretFields,
    /// What the Agent does when it cannot open, decode or parse the location.
    pub on_read_failure: ReadFailureReaction,
    /// How the Agent's own recovery from a Read failure here can lose
    /// configuration, shown as warnings on the Read failure.
    #[serde(default)]
    pub risks: Vec<RiskCode>,
    /// Whether config-digger keeps comments out of the location's file: it
    /// writes none, not even those an Undo would put back above an Entry,
    /// such as into Cursor's `mcp.json`, which a writer of Cursor's that
    /// parses it as strict JSON empties when it holds one.
    #[serde(default)]
    pub no_comments: bool,
    /// The minimal shape the Agent requires of the location's Entries:
    /// one without it is Invalid. Without it, no Entry here is Invalid.
    pub shape: Option<Shape>,
    /// Conditions under which the Agent reads the location, every one of
    /// them. While one does not hold, its Entries are Unread.
    #[serde(default)]
    pub when: Vec<Condition>,
    /// The name the Agent gives an Entry of the location, made of `{name}`,
    /// the Entry's own name; in plugins, `{plugin}`, the plugin's name, such
    /// as `{plugin}:{name}`; and in a Project, `{folder_index}`, the
    /// position of the Entry's folder among those of the Project, 0 for a
    /// folder Project, and `{folder}`, that folder's name, such as Cursor's
    /// `project-{folder_index}-{folder}-{name}`. Without it, an Entry goes
    /// by its own name.
    pub name_template: Option<String>,
    /// What the Agent's Disable switches hold an Entry of this location by,
    /// when not by its name, such as the server ids VS Code pairs with
    /// `true` or `false` in `mcp.enablement`: a template of `{name}`, the
    /// Entry's name; `{folder_index}`, the position of its folder among
    /// those of the Project, 0 for a folder Project; `{uri_path}`, the path
    /// of the `file:` URI of its file or skill folder, such as `/C:/x`;
    /// `{skill_file}`, the absolute path of a skill's `SKILL.md`, the links
    /// of its folder and of the folders above it resolved;
    /// `{plugin_uri}`, the `file:` URI of its plugin's folder; and
    /// `{extension}`, the key `*` stands for in its root up to a `/`, in
    /// lower case.
    pub switch_id: Option<String>,
    /// What the value at `root` of a location in plugins may hold besides
    /// an object of Entries: a path, relative to the plugin's folder, of
    /// another file in the location's format, or a list of such paths and
    /// objects of Entries, such as a Claude Code plugin manifest's
    /// `mcpServers`.
    pub references: Option<References>,
    /// How the Agent writes the folder of an Entry's plugin in the values
    /// of a location in plugins, such as Claude Code's
    /// `${CLAUDE_PLUGIN_ROOT}`. The Entry shows the folder in its place, as
    /// the Agent runs it, and competes by endpoint with it.
    pub plugin_root: Option<PluginRoot>,
    /// The key path, in the file as its format reads it, of when the Agent
    /// cached the file's copy of configuration held elsewhere, such as the
    /// bundle Codex caches from the account: milliseconds since 1970, or an
    /// RFC 3339 time. Each Entry then carries a caveat that it is as of
    /// then, or that the Agent does not record when, if the file does not
    /// say. Omitted for a file the Agent reads as it is.
    pub cached_at: Option<Vec<String>>,
}

/// Files a value can point to, each holding Entries in the format of the
/// location that holds the value.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct References {
    /// The key path of the object whose keys are Entries in a file pointed to.
    pub root: Vec<String>,
    /// The bundles a value can name instead of a file, such as the `.mcpb`
    /// bundles of a Claude Code plugin's `mcpServers`.
    pub bundles: Option<blocks::Bundles>,
    /// A `/`-separated path relative to the plugin's folder of the file
    /// read in place of the value at the location's root when the
    /// location's file declares none there, or there is no such file, as a
    /// Codex plugin's `.mcp.json` is read unless its manifest declares
    /// `mcpServers`. While the location's file cannot be parsed, neither
    /// is read.
    pub default: Option<String>,
    /// Whether the value at the location's root counts only when it is an
    /// object of Entries or a path starting with `./`, as Codex requires
    /// of its plugins' manifests: any other value, a list included, is
    /// ignored, as if not declared.
    #[serde(default)]
    pub dot_slash: bool,
    /// Whether a file pointed to without `root` holds its Entries at its
    /// top level instead, as Codex reads a plugin's `.mcp.json` that is a
    /// bare object of servers.
    #[serde(default)]
    pub bare: bool,
}

impl References {
    /// Whether the Agent ignores `value`, the value at the location's
    /// root, as if it declared nothing there.
    pub fn ignores(&self, value: &serde_json::Value) -> bool {
        self.dot_slash
            && match value {
                serde_json::Value::String(path) => !blocks::is_dot_slash_path(path),
                serde_json::Value::Object(_) => false,
                _ => true,
            }
    }
}

/// A reference that stands for the folder of an Entry's plugin in some of
/// the Entry's fields.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct PluginRoot {
    /// The reference as written, such as `${CLAUDE_PLUGIN_ROOT}`.
    pub reference: String,
    /// The fields it stands for the folder in, such as `command` and
    /// `args`: in a text, each text of a list, and each text value of an
    /// object.
    pub fields: Vec<String>,
    /// Whether the Agent writes the folder with `/` between its names, as
    /// Claude Code does on Windows.
    #[serde(default)]
    pub forward_slashes: bool,
}

impl Validate for PluginRoot {
    fn validate(&self) -> Result<(), String> {
        if self.reference.is_empty() || self.fields.is_empty() {
            return Err("plugin_root needs a reference and the fields it stands in".into());
        }
        Ok(())
    }
}

impl PluginRoot {
    /// Puts `folder` in place of the reference in the fields of `config`.
    pub fn expand(&self, config: &mut serde_json::Value, folder: &Path) {
        let folder = folder.display().to_string();
        let folder = match self.forward_slashes {
            true => folder.replace('\\', "/"),
            false => folder,
        };
        for field in &self.fields {
            if let Some(value) = config.get_mut(field) {
                blocks::replace_in_texts(value, &self.reference, &folder);
            }
        }
    }
}

/// The `name_template` segment that stands for the plugin's name.
pub(crate) const PLUGIN_NAME: &str = "{plugin}";
/// The `name_template` segment that stands for the Entry's own name.
pub(crate) const ENTRY_NAME: &str = "{name}";
/// The `name_template` segment that stands for the position of the Entry's
/// folder among those of its Project.
pub(crate) const FOLDER_INDEX: &str = "{folder_index}";
/// The `name_template` segment that stands for the name of the Entry's folder.
pub(crate) const FOLDER_NAME: &str = "{folder}";

/// What a `name_template` names an Entry by besides its own name.
#[derive(Default)]
pub(crate) struct NameParts<'a> {
    /// The name of the plugin it is part of.
    pub plugin: Option<&'a str>,
    /// The position of its folder among those of its Project, and that
    /// folder's name.
    pub folder: Option<(usize, &'a str)>,
}

/// What an Agent does with a location it cannot read. Declared from the
/// mildest to the strongest: a file several locations of one Agent read
/// takes the strongest of their reactions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReadFailureReaction {
    /// It starts without the location's Entries.
    IgnoresFile,
    /// It asks the user at startup what to do with the file, and otherwise
    /// starts without its Entries.
    AsksUser,
    /// What it does is not known: every Item in the contexts the location
    /// reaches gets a caveat.
    Unknown,
    /// It refuses to start in the contexts the location reaches.
    StopsAgent,
}

/// How an Agent's own recovery from a Read failure can lose configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, JsonSchema)]
pub(crate) enum RiskCode {
    /// It may offer to reset the file, losing what it holds.
    #[serde(rename = "agent_may_reset_file")]
    ResetFile,
    /// It may write the file again from empty, dropping its Entries.
    #[serde(rename = "agent_may_drop_entries")]
    DropEntries,
    /// It may clear the sign-in state of servers that vanish.
    #[serde(rename = "agent_may_clear_auth")]
    ClearAuth,
}

/// Key paths below an Entry, one key per segment, where `*` stands for any
/// key or array index.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SecretFields {
    /// Fields whose values are Secrets. Below an object or array, every
    /// value is.
    #[serde(default)]
    pub fields: Vec<Vec<String>>,
    /// Fields that hold the name of a variable, such as Codex's
    /// `bearer_token_env_var`: never a Secret, whatever else says so.
    #[serde(default)]
    pub variable_names: Vec<Vec<String>>,
    /// Fields that hold a URL, besides `url`, such as Gemini CLI's
    /// `httpUrl`: their userinfo and query values are Secrets, and the rest
    /// of the URL, its host included, is shown.
    #[serde(default)]
    pub urls: Vec<Vec<String>>,
    /// Key paths from the root of the file, outside any Entry, whose values
    /// are Secrets, such as an API key stored next to the Entries. They are
    /// masked wherever the file's text is shown, as in a diff's context lines.
    #[serde(default)]
    pub file_fields: Vec<Vec<String>>,
    /// Prefixes the Agent writes before a value it encrypted, such as
    /// Claude Desktop's `__encrypted__:`. Such a value is a Secret wherever
    /// it is, in an Entry or anywhere in the file, and keeps its prefix.
    #[serde(default)]
    pub encrypted_prefixes: Vec<String>,
}

impl SecretFields {
    /// Whether it names no field at all.
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
            && self.variable_names.is_empty()
            && self.urls.is_empty()
            && self.file_fields.is_empty()
            && self.encrypted_prefixes.is_empty()
    }

    /// The encrypted prefix `text` starts with, if any.
    pub fn encrypted_prefix<'t>(&self, text: &'t str) -> Option<&'t str> {
        self.encrypted_prefixes
            .iter()
            .find(|prefix| text.starts_with(prefix.as_str()))
            .map(|prefix| &text[..prefix.len()])
    }
}

impl Validate for SecretFields {
    fn validate(&self) -> Result<(), String> {
        for field in self
            .fields
            .iter()
            .chain(&self.variable_names)
            .chain(&self.urls)
            .chain(&self.file_fields)
        {
            if field.is_empty() || field.iter().any(String::is_empty) {
                return Err(format!(
                    "secret field {field:?} needs keys, none of them empty"
                ));
            }
        }
        if self.encrypted_prefixes.iter().any(String::is_empty) {
            return Err("an encrypted prefix must not be empty".into());
        }
        Ok(())
    }
}

impl LocationDefinition {
    /// Whether the location's Entries are keyed by Project inside the file.
    pub fn keyed_by_project(&self) -> bool {
        self.root.iter().any(|key| key == PROJECT_KEY)
    }

    /// The name the Agent gives an Entry named `name`, of the plugin and
    /// folder `parts` name.
    pub fn entry_name(&self, name: &str, parts: &NameParts) -> String {
        let Some(template) = &self.name_template else {
            return name.to_owned();
        };
        let (index, folder) = parts.folder.unwrap_or_default();
        template
            .replace(PLUGIN_NAME, parts.plugin.unwrap_or_default())
            .replace(FOLDER_INDEX, &index.to_string())
            .replace(FOLDER_NAME, folder)
            .replace(ENTRY_NAME, name)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ItemKind {
    Mcp,
    Skill,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ItemRules {
    /// Location ids of this kind, highest precedence first.
    pub precedence: Vec<String>,
    pub competition: Competition,
    /// How the Agent combines same-named Entries into the one it uses, field
    /// by field. Omitted, the highest wins whole and Shadows the others.
    pub merge: Option<Merge>,
    /// The Agent's checks, in the order it applies them: an Entry's State
    /// comes from the first it fails, and the others it fails are listed as
    /// secondary reasons. Each check appears once.
    pub checks: Vec<Check>,
    /// How the Agent Disables an Entry, in its order of preference. Disable
    /// uses the most specific one (target, then context) that covers the
    /// Entry and applies in the context, taking the first in this order
    /// among equally specific ones, and skipping any whose switch is
    /// git-versioned.
    #[serde(default)]
    pub disable: Vec<DisableMechanism>,
    /// What keeps the Agent from using an Entry whatever it declares, such
    /// as managed policy: config-digger never lifts these.
    #[serde(default)]
    pub gates: Vec<Gate>,
    /// Settings that keep Entries out by a rule rather than one by one,
    /// such as deny lists: config-digger never writes them.
    #[serde(default)]
    pub rules: Vec<Rule>,
}

/// One of an Agent's checks that decides an Entry's State.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Check {
    /// A managed file takes exclusive control and leaves the Entry out, or
    /// a field the organisation sets keeps it out: Blocked, with `policy`.
    /// A Blocked Entry does not compete. Required when a gate of the kind
    /// is managed-exclusive or a policy field, and refused otherwise.
    Policy,
    /// A Disable mechanism's switch is set for the Entry: Disabled.
    Disable,
    /// The Agent has not approved the Entry: Blocked, with `not_approved`.
    /// Required when a gate of the kind is an approval list, an approval
    /// hash or a nonce trust, and refused otherwise.
    Approval,
    /// The user has not trusted the Entry's Project: Blocked, with
    /// `untrusted_folder`. Required when a gate of the kind is trusted
    /// folders, project trust or folder trust, and refused otherwise.
    Trust,
    /// A rule keeps the Entry out: Blocked, with `policy`, when the rule is
    /// managed policy, and Disabled, with `disabled_by_rule` or
    /// `disabled_globally`, otherwise. Before `precedence`, an Entry a rule
    /// keeps out does not compete; after it, a rule applies to the Entry
    /// that wins, which still beats the others. Required when the kind has
    /// rules, and refused otherwise.
    Rules,
    /// A same-named Entry the Agent ranks higher is not Disabled: Shadowed.
    /// Where the kind merges, each Entry that takes part, with another, in
    /// the server the Agent builds is Merged instead.
    Precedence,
    /// The server the Agent builds from the Item's Entries holds fields that
    /// exclude each other: every Entry that takes part is Invalid, with
    /// `transport_conflict`. Required when the kind merges with exclusive
    /// fields, and refused otherwise.
    Merge,
    /// The Entry lacks the minimal shape its location declares: Invalid.
    /// Required when a location of the kind declares a shape, and refused
    /// otherwise.
    Shape,
    /// The Agent skips the skill because of its `SKILL.md` or its folder's
    /// name: Invalid. Required when a location of the kind is a folder of
    /// skill folders, and refused otherwise.
    Frontmatter,
    /// The Agent refuses the plugin the Entry is part of, since another of
    /// its list has the same name: Invalid, with `duplicate_plugin_name`.
    /// Required when a location of the kind is in plugins whose list refuses
    /// same names, and refused otherwise.
    PluginName,
}

impl AgentDefinition {
    /// The rules for an Item kind. Validation guarantees every location's kind has some.
    pub fn rules(&self, kind: ItemKind) -> &ItemRules {
        &self.items[&kind]
    }

    /// The precedence rank of a location among its kind, 0 being highest.
    pub fn rank(&self, location: &LocationDefinition) -> usize {
        let precedence = &self.rules(location.kind).precedence;
        precedence
            .iter()
            .position(|id| *id == location.id)
            .expect("validation puts every location in its kind's precedence")
    }

    /// How to tell that the Agent of the home `home` is running: the home's
    /// own process, or else the definition's.
    pub fn process(&self, home: &str) -> Option<&ProcessDefinition> {
        self.homes
            .iter()
            .find(|found| found.id == home)
            .and_then(|found| found.process.as_ref())
            .or(self.process.as_ref())
    }

    /// Whether a lookup order holds the home `id`.
    pub fn in_lookup_order(&self, id: &str) -> bool {
        self.lookup_orders.iter().flatten().any(|known| known == id)
    }

    /// The Feature gate named `name`, if the definition declares one.
    pub fn feature_gate(&self, name: &str) -> Option<&FeatureGateDefinition> {
        self.feature_gates.iter().find(|gate| gate.name == name)
    }

    /// Where the machine-wide path `text` that an anchor names is on this
    /// machine, as far as the environment says: where a declared machine
    /// file of that path is, or else `text` expanded.
    pub fn machine_path(&self, text: &str, env: &dyn Environment) -> Option<PathBuf> {
        let declared = |path: &str| self.machine_files.iter().find(|file| file.path == path);
        let Some(file) = declared(text) else {
            return crate::paths::expand(text, env);
        };
        let beside = file
            .beside
            .as_deref()
            .and_then(declared)
            .and_then(|other| other.locate(env, None));
        file.locate(env, beside)
    }

    /// The machine files the Agent reads only while an administrator owns
    /// them, where they are on this machine.
    pub fn admin_owned_files(&self, env: &dyn Environment) -> Vec<PathBuf> {
        self.machine_files
            .iter()
            .filter(|file| file.admin_owned)
            .filter_map(|file| self.machine_path(&file.path, env))
            .collect()
    }

    pub fn location(&self, id: &str) -> &LocationDefinition {
        self.locations
            .iter()
            .find(|location| location.id == id)
            .expect("snapshot location ids come from this definition")
    }

    fn validate(&self) -> Result<(), String> {
        if self.format_version != FORMAT_VERSION {
            return Err(format!(
                "format_version {} is not supported; expected {FORMAT_VERSION}",
                self.format_version
            ));
        }
        non_empty("agent.id", &self.agent.id)?;
        non_empty("agent.name", &self.agent.name)?;
        if self.homes.is_empty() {
            return Err("at least one home is required".into());
        }
        if let Some(process) = &self.process {
            process.validate().map_err(|e| format!("process: {e}"))?;
        }
        unique(
            "home",
            self.homes
                .iter()
                .map(|home| home.id.as_str())
                .chain(self.mirrored_homes.iter().map(|home| home.id.as_str())),
        )?;
        for mirror in &self.mirrored_homes {
            let context = |e: String| format!("mirrored home `{}`: {e}", mirror.id);
            non_empty("owner", &mirror.owner).map_err(context)?;
            mirror.folder.validate().map_err(context)?;
            if !self.homes.iter().any(|home| home.id == mirror.source) {
                return Err(context(format!(
                    "undeclared source home `{}`",
                    mirror.source
                )));
            }
        }
        unique("scope", self.scopes.iter().map(|scope| scope.id.as_str()))?;
        unique(
            "location",
            self.locations.iter().map(|location| location.id.as_str()),
        )?;

        for home in &self.homes {
            non_empty("home id", &home.id)?;
            if home.roots.is_empty() {
                return Err(format!("home `{}` declares no roots", home.id));
            }
            home.folder
                .validate()
                .map_err(|e| format!("home `{}` folder: {e}", home.id))?;
            for (name, path) in &home.roots {
                blocks::validate_relative(path)
                    .map_err(|e| format!("home `{}` root `{name}`: {e}", home.id))?;
            }
            if let Some(name) = home
                .shared_roots
                .iter()
                .find(|name| !home.roots.contains_key(*name))
            {
                return Err(format!(
                    "home `{}` shares undeclared root `{name}`",
                    home.id
                ));
            }
            if home
                .roots
                .keys()
                .all(|name| home.shared_roots.contains(name))
            {
                return Err(format!("home `{}` shares every root", home.id));
            }
            if let Some(process) = &home.process {
                process
                    .validate()
                    .map_err(|e| format!("home `{}` process: {e}", home.id))?;
            }
            for condition in &home.when {
                condition
                    .validate()
                    .map_err(|e| format!("home `{}` when: {e}", home.id))?;
            }
            if !home.when.is_empty() && !self.in_lookup_order(&home.id) {
                return Err(format!(
                    "home `{}` takes `when` only in a lookup order",
                    home.id
                ));
            }
            if home.default && home.folder.finds_by() != FindsBy::FixedPath {
                return Err(format!(
                    "home `{}` is the default, so its folder must be one fixed path",
                    home.id
                ));
            }
        }
        if self.homes.iter().filter(|home| home.default).count() > 1 {
            return Err("at most one home can be the default".into());
        }
        for order in &self.lookup_orders {
            unique("lookup order home", order.iter().map(String::as_str))?;
            if let Some(id) = order
                .iter()
                .find(|id| !self.homes.iter().any(|home| home.id == **id))
            {
                return Err(format!("lookup_orders names undeclared home `{id}`"));
            }
        }

        if let Some(verified) = &self.verified {
            verified.validate().map_err(|e| format!("verified: {e}"))?;
        }
        unique(
            "installation",
            self.installations
                .iter()
                .map(|installation| installation.id.as_str()),
        )?;
        for installation in &self.installations {
            let context = |e: String| format!("installation `{}`: {e}", installation.id);
            crate::paths::validate_glob(&installation.folder).map_err(context)?;
            blocks::validate_relative(&installation.executable).map_err(context)?;
            if installation.versions.is_empty() {
                return Err(context("versions needs at least one source".into()));
            }
            for source in &installation.versions {
                source.validate().map_err(context)?;
            }
            for home in installation.homes.iter().flatten() {
                if !self.homes.iter().any(|known| known.id == *home) {
                    return Err(context(format!("undeclared home `{home}`")));
                }
            }
        }
        if let Some(last_ran) = &self.last_ran {
            let context = |e: String| format!("last_ran: {e}");
            last_ran.anchor.validate().map_err(context)?;
            last_ran.anchor.one_file().map_err(context)?;
            if last_ran.anchor.in_project() {
                return Err(context("must lie in the Agent home, not a Project".into()));
            }
            self.validate_home_root(&last_ran.anchor).map_err(context)?;
            blocks::validate_key_path(&last_ran.key_path).map_err(context)?;
        }

        for (index, registry) in self.projects.registries.iter().enumerate() {
            let context = |e: String| format!("projects.registries[{index}]: {e}");
            registry.validate().map_err(context)?;
            self.validate_home_root(registry.anchor())
                .map_err(context)?;
        }

        unique(
            "settings",
            self.settings.iter().map(|settings| settings.id.as_str()),
        )?;
        for settings in &self.settings {
            let context = |e: String| format!("settings `{}`: {e}", settings.id);
            if settings.sources.is_empty() {
                return Err(context("sources needs at least one source".into()));
            }
            for source in &settings.sources {
                source.validate().map_err(context)?;
            }
            if let Some(merge) = &settings.merge
                && !settings.control_keys.contains(&merge.key)
            {
                return Err(context(format!(
                    "merge key `{}` must be one of control_keys",
                    merge.key
                )));
            }
        }

        unique(
            "plugins",
            self.plugins.iter().map(|plugins| plugins.id.as_str()),
        )?;
        for plugins in &self.plugins {
            let context = |e: String| format!("plugins `{}`: {e}", plugins.id);
            plugins.validate().map_err(context)?;
            for anchor in plugins.registry.anchors() {
                self.validate_home_root(anchor).map_err(context)?;
            }
        }

        unique(
            "machine file",
            self.machine_files.iter().map(|file| file.path.as_str()),
        )?;
        for file in &self.machine_files {
            let context = |e: String| format!("machine file `{}`: {e}", file.path);
            blocks::MachineAnchor {
                path: file.path.clone(),
            }
            .validate()
            .map_err(context)?;
            if file.path.starts_with("~/") {
                return Err(context("must start with `%NAME%` or a drive".into()));
            }
            if let Some(variable) = &file.variable {
                non_empty("variable", variable).map_err(context)?;
            }
            if let Some(beside) = &file.beside {
                let other = self
                    .machine_files
                    .iter()
                    .find(|other| other.path == *beside && other.path != file.path);
                if other.is_none_or(|other| other.beside.is_some()) {
                    return Err(context(format!(
                        "beside `{beside}` must be another machine file, one with no `beside`"
                    )));
                }
            }
        }

        unique(
            "feature gate",
            self.feature_gates.iter().map(|gate| gate.name.as_str()),
        )?;
        for gate in &self.feature_gates {
            let context = |e: String| format!("feature gate `{}`: {e}", gate.name);
            if gate.affects.is_empty() {
                return Err(context("affects names no location".into()));
            }
            if let Some(id) = gate
                .affects
                .iter()
                .find(|id| !self.locations.iter().any(|location| location.id == **id))
            {
                return Err(context(format!("affects undeclared location `{id}`")));
            }
            match (&gate.lookup, &gate.when) {
                (Some(lookup), None) => {
                    lookup.validate().map_err(context)?;
                    self.validate_home_root(lookup.anchor()).map_err(context)?;
                }
                (None, when) => {
                    if let Some(when) = when {
                        when.validate().map_err(context)?;
                        for source in &when.sources {
                            self.validate_setting(&source.anchor).map_err(context)?;
                        }
                    }
                    if gate.bypass.is_some() {
                        return Err(context("only a gate with a lookup takes a bypass".into()));
                    }
                }
                (Some(_), Some(_)) => {
                    return Err(context("takes a lookup or a `when`, not both".into()));
                }
            }
            if let Some(bypass) = &gate.bypass {
                bypass.validate().map_err(context)?;
                for map in bypass.env_maps() {
                    self.validate_setting(&map.anchor).map_err(context)?;
                }
            }
        }

        for location in &self.locations {
            let context = |e: String| format!("location `{}`: {e}", location.id);
            location.anchor.validate().map_err(context)?;
            if location.anchor.project_file() && !self.projects.workspace_files {
                return Err(context(
                    "a project-file location needs `projects.workspace_files`".into(),
                ));
            }
            if location.anchor.per_plugin() || location.anchor.entry().is_some() {
                return Err(context(
                    "a location is anchored once per home, not by `{plugin_id}` or beside an Entry"
                        .into(),
                ));
            }
            location.format.validate().map_err(context)?;
            if let Some(interpolation) = &location.interpolation {
                interpolation.validate().map_err(context)?;
            }
            location.secrets.validate().map_err(context)?;
            if let Some(shape) = &location.shape {
                shape.validate().map_err(context)?;
            }
            self.validate_home_root(&location.anchor).map_err(context)?;
            if let Some(id) = location.anchor.settings() {
                self.declared_settings(id).map_err(context)?;
                if location.format.file().is_none() {
                    return Err(context("settings are read in a file format".into()));
                }
            }
            if let Some(anchor) = location.anchor.plugin() {
                for id in anchor.plugins.all() {
                    self.declared_plugins(id).map_err(context)?;
                }
                if location.keyed_by_project() {
                    return Err(context(format!(
                        "a location in plugins cannot be keyed by `{PROJECT_KEY}`"
                    )));
                }
            }
            let manifest_skills = matches!(location.format, Format::ManifestSkills(_));
            if manifest_skills
                && (location.anchor.plugin().is_none() || location.references.is_some())
            {
                return Err(context(
                    "only a location in plugins, with no references, reads manifest skills".into(),
                ));
            }
            if let Some(template) = &location.name_template {
                if template.contains(PLUGIN_NAME) && location.anchor.plugin().is_none() {
                    return Err(context(format!(
                        "only a location in plugins names `{PLUGIN_NAME}` in its name_template"
                    )));
                }
                let in_folder = location.anchor.in_project() && !location.anchor.project_file();
                if (template.contains(FOLDER_INDEX) || template.contains(FOLDER_NAME)) && !in_folder
                {
                    return Err(context(format!(
                        "only a location in a Project folder names `{FOLDER_INDEX}` or `{FOLDER_NAME}` in its name_template"
                    )));
                }
                if template.matches(ENTRY_NAME).count() != 1 {
                    return Err(context(format!(
                        "name_template must hold `{ENTRY_NAME}` once"
                    )));
                }
            }
            if location.one_entry {
                let in_plugin_file =
                    location.anchor.plugin().is_some() && location.format.file().is_some();
                if !in_plugin_file || location.root.is_empty() {
                    return Err(context(
                        "only a file location in plugins, with a root, holds one Entry".into(),
                    ));
                }
                if location.name_template.is_some() || location.references.is_some() {
                    return Err(context(
                        "a location that holds one Entry takes no name_template or references"
                            .into(),
                    ));
                }
            }
            if location.bare
                && (location.root.is_empty()
                    || location.keyed_by_project()
                    || location.one_entry
                    || location.format.file().is_none())
            {
                return Err(context(
                    "only a file location with a root, not keyed by Project nor holding one Entry, can be bare"
                        .into(),
                ));
            }
            if let Some(field) = &location.named_by {
                if field.is_empty() {
                    return Err(context("named_by must not be empty".into()));
                }
                if location.root.is_empty() || location.one_entry || location.references.is_some() {
                    return Err(context(
                        "named_by needs a root, and no one_entry or references".into(),
                    ));
                }
            }
            if let Some(store) = location.format.sqlite()
                && location.root.first() != Some(&store.key)
            {
                return Err(context(format!(
                    "the root of a sqlite-json location starts with its key `{}`",
                    store.key
                )));
            }
            if location.anchor.project_storage().is_some()
                && !self
                    .projects
                    .registries
                    .iter()
                    .any(|registry| matches!(registry, Registry::JsonFolders(_)))
            {
                return Err(context(
                    "a project-storage location needs a json-folders project registry".into(),
                ));
            }
            if let Some(references) = &location.references {
                if location.anchor.plugin().is_none() || location.format.file().is_none() {
                    return Err(context(
                        "only a file location in plugins takes references".into(),
                    ));
                }
                if location.root.is_empty() {
                    return Err(context("references need a root to hold them".into()));
                }
                blocks::validate_key_path(&references.root).map_err(context)?;
                if let Some(bundles) = &references.bundles {
                    bundles.validate().map_err(context)?;
                }
                if let Some(default) = &references.default {
                    blocks::validate_relative(default).map_err(context)?;
                }
            }
            if let Some(root) = &location.plugin_root {
                if location.anchor.plugin().is_none() {
                    return Err(context(
                        "only a location in plugins takes plugin_root".into(),
                    ));
                }
                root.validate().map_err(context)?;
            }
            if let Some(cached_at) = &location.cached_at {
                if location.format.file().is_none() {
                    return Err(context("only a file location takes cached_at".into()));
                }
                blocks::validate_key_path(cached_at).map_err(context)?;
            }
            // Its key paths are in the TOML it holds, not in its own text.
            if matches!(location.format, Format::TomlFragments(_))
                && !location.provenance.read_only()
            {
                return Err(context(
                    "a toml-fragments location is read only: its provenance must be".into(),
                ));
            }
            let project_keys = location.root.iter().filter(|key| *key == PROJECT_KEY);
            if project_keys.count() > 1 {
                return Err(context(format!("root holds `{PROJECT_KEY}` twice")));
            }
            if location.keyed_by_project() && location.anchor.in_project() {
                return Err(context(format!(
                    "a location in a Project cannot be keyed by `{PROJECT_KEY}`"
                )));
            }
            if location.anchor.walk() != ProjectWalk::None && location.keyed_by_project() {
                return Err(context(format!(
                    "a location that walks cannot be keyed by `{PROJECT_KEY}`"
                )));
            }
            match (location.format.skill_folder(), location.kind) {
                (Some(_), ItemKind::Skill) => {
                    if !location.root.is_empty() {
                        return Err(context("a folder of skill folders has no root".into()));
                    }
                    if location.interpolation.is_some()
                        || location.shape.is_some()
                        || !location.secrets.is_empty()
                    {
                        return Err(context(
                            "a folder of skill folders takes no interpolation, secrets or shape"
                                .into(),
                        ));
                    }
                }
                (None, ItemKind::Mcp) => {}
                (Some(_), ItemKind::Mcp) => {
                    return Err(context("kind `mcp` needs a file format".into()));
                }
                (None, ItemKind::Skill) => {
                    return Err(context("kind `skill` needs the skill-folder format".into()));
                }
            }
            if !self.scopes.iter().any(|scope| scope.id == location.scope) {
                return Err(context(format!("undeclared scope `{}`", location.scope)));
            }
            if location.no_comments && location.format.file().is_none() {
                return Err(context("no_comments needs a file format".into()));
            }
            for condition in &location.when {
                condition.validate().map_err(context)?;
                for source in condition
                    .setting()
                    .iter()
                    .flat_map(|setting| &setting.sources)
                {
                    self.validate_setting(&source.anchor).map_err(context)?;
                }
                let Some(name) = condition.gate() else {
                    continue;
                };
                match self.feature_gate(name) {
                    None => return Err(context(format!("undeclared feature gate `{name}`"))),
                    Some(gate) if !gate.affects.contains(&location.id) => {
                        return Err(context(format!(
                            "feature gate `{name}` does not list it in `affects`"
                        )));
                    }
                    Some(_) => {}
                }
            }
            if !self.items.contains_key(&location.kind) {
                return Err(context(format!(
                    "no [items.{}] rules for its kind",
                    location.kind.id()
                )));
            }
        }

        for (kind, rules) in &self.items {
            let context = |e: String| format!("items.{}: {e}", kind.id());
            rules.competition.validate().map_err(context)?;
            for source in rules
                .competition
                .only_while()
                .iter()
                .flat_map(|setting| &setting.sources)
            {
                self.validate_setting(&source.anchor).map_err(context)?;
            }
            unique(
                "precedence entry",
                rules.precedence.iter().map(String::as_str),
            )
            .map_err(context)?;
            let declared: BTreeSet<&str> = self
                .locations
                .iter()
                .filter(|location| location.kind == *kind)
                .map(|location| location.id.as_str())
                .collect();
            let ranked: BTreeSet<&str> = rules.precedence.iter().map(String::as_str).collect();
            if let Some(id) = ranked.difference(&declared).next() {
                return Err(context(format!(
                    "precedence names `{id}`, which is not a location of this kind"
                )));
            }
            if let Some(id) = declared.difference(&ranked).next() {
                return Err(context(format!("precedence leaves out location `{id}`")));
            }
            let same_url = rules.competition.same_url();
            if let Some(id) = rules
                .competition
                .endpoint_locations()
                .iter()
                .chain(same_url.iter().flat_map(|block| &block.locations))
                .find(|id| !declared.contains(id.as_str()))
            {
                return Err(context(format!(
                    "competition names `{id}`, which is not a location of this kind"
                )));
            }
            if let Some(block) = same_url {
                let Some(gate) = self.feature_gate(&block.gate) else {
                    return Err(context(format!(
                        "competition needs undeclared feature gate `{}`",
                        block.gate
                    )));
                };
                if let Some(id) = block.locations.iter().find(|id| !gate.affects.contains(id)) {
                    return Err(context(format!(
                        "feature gate `{}` does not list `{id}` in `affects`",
                        block.gate
                    )));
                }
            }
            for check in [Check::Disable, Check::Precedence] {
                match rules.checks.iter().filter(|known| **known == check).count() {
                    1 => {}
                    0 => return Err(context(format!("checks leaves out `{}`", check.id()))),
                    _ => return Err(context(format!("checks holds `{}` twice", check.id()))),
                }
            }
            let shaped = self
                .locations
                .iter()
                .any(|location| location.kind == *kind && location.shape.is_some());
            rules
                .check_when(
                    Check::Shape,
                    shaped,
                    "which a location declares",
                    "but no location declares one",
                )
                .map_err(context)?;
            let folders = self
                .locations
                .iter()
                .any(|location| location.kind == *kind && location.format.skill_folder().is_some());
            rules
                .check_when(
                    Check::Frontmatter,
                    folders,
                    "which a folder of skill folders needs",
                    "but no location is a folder of skill folders",
                )
                .map_err(context)?;
            let refused = self.locations.iter().any(|location| {
                location.kind == *kind
                    && location.anchor.plugin().is_some_and(|anchor| {
                        anchor.plugins.all().iter().any(|id| {
                            self.declared_plugins(id)
                                .is_ok_and(|plugins| plugins.same_names_invalid)
                        })
                    })
            });
            rules
                .check_when(
                    Check::PluginName,
                    refused,
                    "which plugins that refuse same names need",
                    "but no location is in plugins that refuse same names",
                )
                .map_err(context)?;
            rules
                .check_when(
                    Check::Policy,
                    rules.gates.iter().any(|gate| {
                        matches!(gate, Gate::ManagedExclusive(_) | Gate::PolicyField(_))
                    }),
                    "which a managed-exclusive gate or policy field needs",
                    "but no gate is managed-exclusive or a policy field",
                )
                .map_err(context)?;
            rules
                .check_when(
                    Check::Approval,
                    rules.gates.iter().any(|gate| {
                        matches!(
                            gate,
                            Gate::ApprovalList(_) | Gate::NonceTrust(_) | Gate::ApprovalHash(_)
                        )
                    }),
                    "which an approval, approval-hash or nonce-trust gate needs",
                    "but no gate is an approval, an approval hash or a nonce trust",
                )
                .map_err(context)?;
            rules
                .check_when(
                    Check::Trust,
                    rules.gates.iter().any(|gate| {
                        matches!(
                            gate,
                            Gate::TrustedFolders(_) | Gate::ProjectTrust(_) | Gate::FolderTrust(_)
                        )
                    }),
                    "which a trusted-folders, project-trust or folder-trust gate needs",
                    "but no gate is trusted folders, project trust or folder trust",
                )
                .map_err(context)?;
            rules
                .check_when(
                    Check::Rules,
                    !rules.rules.is_empty(),
                    "which a rule needs",
                    "but the kind has no rule",
                )
                .map_err(context)?;
            rules
                .check_when(
                    Check::Merge,
                    rules.merge.as_ref().is_some_and(Merge::excludes),
                    "which a merge with exclusive fields needs",
                    "but the kind does not merge exclusive fields",
                )
                .map_err(context)?;
            if let Some(merge) = &rules.merge {
                merge.validate().map_err(context)?;
                if let Some(id) = merge
                    .locations()
                    .iter()
                    .find(|id| !declared.contains(id.as_str()))
                {
                    return Err(context(format!("`{id}` is not a location of this kind")));
                }
                // The server built from the merging locations wins over the others.
                let merging = rules.precedence.iter().map(|id| merge.covers(id));
                if merging.skip_while(|covers| *covers).any(|covers| covers) {
                    return Err(context(
                        "merging locations must rank above every other location".into(),
                    ));
                }
                if !matches!(
                    rules.competition,
                    Competition::ExactName(_) | Competition::CaseInsensitiveName(_)
                ) || !rules.competition.endpoint_locations().is_empty()
                {
                    return Err(context(
                        "only Entries that compete by name alone merge".into(),
                    ));
                }
                if self
                    .locations
                    .iter()
                    .any(|location| location.kind == *kind && location.format.file().is_none())
                {
                    return Err(context("only Entries of files merge".into()));
                }
            }
            for (index, rule) in rules.rules.iter().enumerate() {
                let context = |e: String| context(format!("rules[{index}]: {e}"));
                rule.validate().map_err(context)?;
                for source in rule.settings() {
                    self.validate_setting(&source.anchor).map_err(context)?;
                }
                if let Some(id) = rule
                    .locations()
                    .iter()
                    .find(|id| !declared.contains(id.as_str()))
                {
                    return Err(context(format!("`{id}` is not a location of this kind")));
                }
            }
            for (index, gate) in rules.gates.iter().enumerate() {
                let context = |e: String| context(format!("gates[{index}]: {e}"));
                gate.validate().map_err(context)?;
                match gate {
                    Gate::ManagedExclusive(block) => {
                        if !declared.contains(block.location.as_str()) {
                            return Err(context(format!(
                                "`{}` is not a location of this kind",
                                block.location
                            )));
                        }
                        let location = self.location(&block.location);
                        if location.anchor.in_project() || location.keyed_by_project() {
                            return Err(context(format!(
                                "location `{}` must apply in every context",
                                block.location
                            )));
                        }
                        if let Some(id) = block
                            .except
                            .iter()
                            .find(|id| !declared.contains(id.as_str()))
                        {
                            return Err(context(format!("`{id}` is not a location of this kind")));
                        }
                    }
                    Gate::ApprovalList(_)
                    | Gate::PolicyField(_)
                    | Gate::TrustedFolders(_)
                    | Gate::NonceTrust(_)
                    | Gate::ApprovalHash(_)
                    | Gate::ProjectTrust(_)
                    | Gate::FolderTrust(_) => {
                        if let Some(id) = gate
                            .locations()
                            .into_iter()
                            .find(|id| !declared.contains(id))
                        {
                            return Err(context(format!("`{id}` is not a location of this kind")));
                        }
                        if let Gate::ProjectTrust(block) = gate
                            && let Some(id) = block
                                .locations
                                .iter()
                                .find(|id| !self.location(id).anchor.in_project())
                        {
                            return Err(context(format!("`{id}` is not in a Project")));
                        }
                    }
                }
                for source in gate.settings() {
                    self.validate_setting(&source.anchor).map_err(context)?;
                }
            }
            for (index, mechanism) in rules.disable.iter().enumerate() {
                let context = |e: String| context(format!("disable[{index}]: {e}"));
                mechanism.validate().map_err(context)?;
                if let Some(switched) = mechanism.switched() {
                    let anchor = &switched.switch().anchor;
                    if anchor.settings().is_some() && switched.enable_offered() {
                        return Err(context(
                            "a switch in settings is only read: enable_offered must be false"
                                .into(),
                        ));
                    }
                    self.validate_setting(anchor).map_err(context)?;
                }
                if let Some(id) = mechanism
                    .locations()
                    .iter()
                    .find(|id| !declared.contains(id.as_str()))
                {
                    return Err(context(format!("`{id}` is not a location of this kind")));
                }
                if let Some(block) = mechanism.merged_field() {
                    let Some(merge) = rules.merge.as_ref().filter(|m| m.enabled().is_some()) else {
                        return Err(context(
                            "merged-field needs a merge with an `enabled` field".into(),
                        ));
                    };
                    if let Some(id) = block.locations.iter().find(|id| !merge.covers(id)) {
                        return Err(context(format!("`{id}` does not merge")));
                    }
                    if let Some(id) = &block.fallback {
                        let fallback = declared
                            .contains(id.as_str())
                            .then(|| self.location(id))
                            .filter(|fallback| {
                                merge.covers(id)
                                    && fallback.format.file().is_some()
                                    && !fallback.anchor.in_project()
                                    && fallback.anchor.plugin().is_none()
                                    && !fallback.provenance.read_only()
                            });
                        if fallback.is_none() {
                            return Err(context(format!(
                                "fallback `{id}` is no writable file outside Projects and plugins that merges"
                            )));
                        }
                    }
                    continue;
                }
                let Some(mechanism) = mechanism.switched() else {
                    // Parking moves a key out of a file, or a skill's folder
                    // out of a folder of skill folders, and back.
                    if let Some(id) = mechanism.locations().iter().find(|id| {
                        let location = self.location(id);
                        (location.format.file().is_none()
                            && !matches!(location.format, Format::SkillFolder(_)))
                            || location.anchor.plugin().is_some()
                            || location.one_entry
                    }) {
                        return Err(context(format!(
                            "park covers keys in files and folders of skill folders outside plugins only, not `{id}`"
                        )));
                    }
                    continue;
                };
                self.validate_home_root(&mechanism.switch().anchor)
                    .map_err(context)?;
                for name in mechanism.when().iter().filter_map(Condition::gate) {
                    let Some(gate) = self.feature_gate(name) else {
                        return Err(context(format!("undeclared feature gate `{name}`")));
                    };
                    let covered: Vec<&str> = match mechanism.locations() {
                        [] => declared.iter().copied().collect(),
                        listed => listed.iter().map(String::as_str).collect(),
                    };
                    if let Some(id) = covered
                        .iter()
                        .find(|id| !gate.affects.iter().any(|known| known == *id))
                    {
                        return Err(context(format!(
                            "feature gate `{name}` does not list `{id}` in `affects`"
                        )));
                    }
                }
                if mechanism.switch().plugins() {
                    if mechanism.locations().is_empty() {
                        return Err(context(
                            "a switch keyed by plugin needs its locations listed".into(),
                        ));
                    }
                    if let Some(id) = mechanism
                        .locations()
                        .iter()
                        .find(|id| self.location(id).anchor.plugin().is_none())
                    {
                        return Err(context(format!(
                            "a switch keyed by plugin covers locations in plugins only, not `{id}`"
                        )));
                    }
                }
            }
        }
        Ok(())
    }
}

impl HomeDefinition {
    /// The path of each root of the home whose folder is `folder`, by name.
    pub fn roots_in(&self, folder: &Path) -> BTreeMap<String, PathBuf> {
        self.roots
            .iter()
            .map(|(name, path)| (name.clone(), blocks::join_relative(folder, path)))
            .collect()
    }

    /// Whether `roots`, laid out like this home's, say the Agent keeps its
    /// configuration there: any of them but the shared ones exists.
    pub fn configured(&self, roots: &BTreeMap<String, PathBuf>, fs: &dyn FileSystem) -> bool {
        roots
            .iter()
            .filter(|(name, _)| self.is_own_root(name))
            .any(|(_, root)| fs.exists(root))
    }

    /// Each root of the home other Agents do not read too, as its path
    /// relative to the home's folder, by name.
    pub fn own_roots(&self) -> impl Iterator<Item = (&String, &String)> {
        self.roots.iter().filter(|(name, _)| self.is_own_root(name))
    }

    /// Whether the root `name` is not one other Agents read too.
    fn is_own_root(&self, name: &str) -> bool {
        !self.shared_roots.iter().any(|shared| shared == name)
    }
}

impl AgentDefinition {
    /// The home an Agent home the user adds is laid out like, at a folder
    /// none of the definition's homes finds: the first.
    pub fn added_home(&self) -> &HomeDefinition {
        &self.homes[0]
    }

    /// The `[[settings]]` with the id `id`, which a settings anchor names.
    pub fn declared_settings(&self, id: &str) -> Result<&SettingsDefinition, String> {
        self.settings
            .iter()
            .find(|settings| settings.id == id)
            .ok_or_else(|| format!("undeclared settings `{id}`"))
    }

    /// The `[[plugins]]` with the id `id`, which a plugin anchor names.
    pub fn declared_plugins(&self, id: &str) -> Result<&PluginsDefinition, String> {
        self.plugins
            .iter()
            .find(|plugins| plugins.id == id)
            .ok_or_else(|| format!("undeclared plugins `{id}`"))
    }

    /// A setting's anchor needs the root or the `[[settings]]` it names.
    fn validate_setting(&self, anchor: &Anchor) -> Result<(), String> {
        self.validate_home_root(anchor)?;
        match anchor.settings() {
            Some(id) => self.declared_settings(id).map(|_| ()),
            None => Ok(()),
        }
    }

    /// Every home must declare the root an anchor needs.
    fn validate_home_root(&self, anchor: &Anchor) -> Result<(), String> {
        if let Some(root) = anchor.home_root()
            && let Some(home) = self.homes.iter().find(|h| !h.roots.contains_key(root))
        {
            return Err(format!("home `{}` declares no root `{root}`", home.id));
        }
        Ok(())
    }
}

impl ItemKind {
    pub fn id(self) -> &'static str {
        match self {
            ItemKind::Mcp => "mcp",
            ItemKind::Skill => "skill",
        }
    }
}

impl ItemRules {
    /// `check` appears once when `needed`, `needs` saying why, and not at
    /// all otherwise, `refused` saying why not.
    fn check_when(
        &self,
        check: Check,
        needed: bool,
        needs: &str,
        refused: &str,
    ) -> Result<(), String> {
        let id = check.id();
        match (
            self.checks.iter().filter(|known| **known == check).count(),
            needed,
        ) {
            (1, true) | (0, false) => Ok(()),
            (0, true) => Err(format!("checks leaves out `{id}`, {needs}")),
            (_, true) => Err(format!("checks holds `{id}` twice")),
            (_, false) => Err(format!("checks holds `{id}`, {refused}")),
        }
    }
}

impl Check {
    fn id(self) -> &'static str {
        match self {
            Check::Policy => "policy",
            Check::Disable => "disable",
            Check::Rules => "rules",
            Check::Approval => "approval",
            Check::Trust => "trust",
            Check::Precedence => "precedence",
            Check::Merge => "merge",
            Check::Shape => "shape",
            Check::Frontmatter => "frontmatter",
            Check::PluginName => "plugin-name",
        }
    }
}

fn non_empty(what: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        Err(format!("{what} must not be empty"))
    } else {
        Ok(())
    }
}

fn unique<'a>(what: &str, ids: impl Iterator<Item = &'a str>) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    for id in ids {
        non_empty(&format!("{what} id"), id)?;
        if !seen.insert(id) {
            return Err(format!("{what} `{id}` is declared twice"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = r#"
format_version = 1

[agent]
id = "test-agent"
name = "Test Agent"

[[homes]]
id = "default"
folder = { block = "path", path = "~" }
roots.state = ".test.json"

[[scopes]]
id = "user"

[[locations]]
id = "user-mcp"
anchor = { block = "home", root = "state" }
format = { block = "json", dialect = "strict" }
root = ["servers"]
kind = "mcp"
scope = "user"
provenance = "file"
store_class = "plain"
on_read_failure = "ignores_file"

[items.mcp]
precedence = ["user-mcp"]
competition = { block = "exact-name" }
checks = ["disable", "precedence"]
"#;

    /// `VALID` plus a project registry and a location keyed by Project.
    fn with_projects(registry_anchor: &str, local_anchor: &str, local_root: &str) -> String {
        let local = format!(
            r#"
[[locations]]
id = "local-mcp"
anchor = {local_anchor}
format = {{ block = "json", dialect = "strict" }}
root = {local_root}
kind = "mcp"
scope = "user"
provenance = "file"
store_class = "plain"
on_read_failure = "ignores_file"
"#
        );
        VALID
            .replace(
                "[[locations]]",
                &format!(
                    "[projects]\nregistries = [{{ block = \"json-keys\", anchor = {registry_anchor}, root = [\"projects\"] }}]\n\n{local}\n[[locations]]"
                ),
            )
            .replace(r#"["user-mcp"]"#, r#"["local-mcp", "user-mcp"]"#)
    }

    const STATE: &str = r#"{ block = "home", root = "state" }"#;
    const IN_PROJECT: &str = r#"{ block = "project", path = ".test.json" }"#;

    fn error(text: &str) -> String {
        parse("test.toml", text).unwrap_err().message
    }

    #[test]
    fn built_in_definitions_load() {
        let ids: Vec<String> = built_in()
            .unwrap()
            .into_iter()
            .map(|definition| definition.agent.id)
            .collect();
        assert_eq!(
            ids,
            [
                "claude-code",
                "claude-desktop",
                "vscode",
                "cursor",
                "codex",
                "gemini-cli"
            ]
        );
    }

    #[test]
    fn a_valid_definition_loads() {
        assert!(parse("test.toml", VALID).is_ok());
    }

    #[test]
    fn a_definition_with_projects_loads() {
        let text = with_projects(STATE, STATE, r#"["projects", "{project}", "servers"]"#);
        assert!(parse("test.toml", &text).is_ok(), "{}", error(&text));
        let text = with_projects(STATE, IN_PROJECT, r#"["servers"]"#);
        assert!(parse("test.toml", &text).is_ok(), "{}", error(&text));
    }

    #[test]
    fn a_project_registry_in_a_project_is_rejected() {
        let text = with_projects(IN_PROJECT, STATE, r#"["servers"]"#);
        assert!(
            error(&text).contains("must not lie in a Project"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn a_project_registry_with_an_undeclared_root_is_rejected() {
        let text = with_projects(
            r#"{ block = "home", root = "config" }"#,
            STATE,
            r#"["servers"]"#,
        );
        assert!(
            error(&text).contains("no root `config`"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn a_location_in_a_project_cannot_be_keyed_by_project() {
        let text = with_projects(STATE, IN_PROJECT, r#"["{project}", "servers"]"#);
        assert!(
            error(&text).contains("cannot be keyed by `{project}`"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn a_root_holds_at_most_one_project_key() {
        let text = with_projects(STATE, STATE, r#"["{project}", "{project}"]"#);
        assert!(
            error(&text).contains("`{project}` twice"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn a_location_declares_its_interpolation_and_secret_fields() {
        let text = VALID.replace(
            "provenance = \"file\"\n",
            "provenance = \"file\"\ninterpolation = { block = \"braces\", namespaces = [\"env\"], default_separator = \":-\" }\nsecrets.fields = [[\"auth\", \"*\"]]\nsecrets.variable_names = [[\"token_env_var\"]]\n",
        );
        assert!(parse("test.toml", &text).is_ok(), "{}", error(&text));
    }

    #[test]
    fn an_empty_secret_field_key_is_rejected() {
        let text = VALID.replace(
            "provenance = \"file\"\n",
            "provenance = \"file\"\nsecrets.fields = [[\"auth\", \"\"]]\n",
        );
        assert!(error(&text).contains("secret field"), "{}", error(&text));
    }

    #[test]
    fn an_interpolation_namespace_must_be_a_name() {
        let text = VALID.replace(
            "provenance = \"file\"\n",
            "provenance = \"file\"\ninterpolation = { block = \"braces\", namespaces = [\"e:nv\"] }\n",
        );
        assert!(
            error(&text).contains("namespace `e:nv`"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn a_location_needs_a_provenance() {
        let text = VALID.replace("provenance = \"file\"\n", "");
        assert!(error(&text).contains("provenance"), "{}", error(&text));
    }

    #[test]
    fn a_location_needs_a_store_class() {
        let text = VALID.replace("store_class = \"plain\"\n", "");
        assert!(error(&text).contains("store_class"), "{}", error(&text));
    }

    #[test]
    fn a_disable_switch_needs_a_store_class() {
        let text = with_disable(&name_list(false, "").replace(r#", store_class = "plain""#, ""));
        assert!(error(&text).contains("store_class"), "{}", error(&text));
    }

    /// `VALID` with `process` as its `[process]` table.
    fn with_process(process: &str) -> String {
        VALID.replace("[[homes]]", &format!("[process]\n{process}\n\n[[homes]]"))
    }

    #[test]
    fn a_process_loads() {
        let text = with_process(
            r#"images = ["claude.exe"]
paths = ["~/.local/bin/claude.exe", "%LOCALAPPDATA%/Programs/x/*", "C:/Program Files/WindowsApps/{Claude,AnthropicPBC.Claude}_*/app/*"]
cmdline_contains = ["claude"]
session_files = { block = "pid-named", folder = { block = "path", path = "~/.claude/sessions" }, extension = "json" }"#,
        );
        assert!(parse("test.toml", &text).is_ok(), "{}", error(&text));
    }

    #[test]
    fn a_home_may_override_the_process() {
        let text = VALID.replace(
            "roots.state = ",
            "process = { images = [\"other.exe\"] }\nroots.state = ",
        );
        assert!(parse("test.toml", &text).is_ok(), "{}", error(&text));
    }

    #[test]
    fn a_process_needs_images_or_session_files() {
        let text = with_process("cmdline_contains = [\"x\"]");
        assert!(error(&text).contains("images"), "{}", error(&text));
    }

    #[test]
    fn a_path_glob_must_start_from_home_a_variable_or_a_drive() {
        let text = with_process("images = [\"x.exe\"]\npaths = [\"bin/x.exe\"]");
        assert!(error(&text).contains("must start with"), "{}", error(&text));
        let text = with_process("images = [\"x.exe\"]\npaths = [\"C:/{a,b/x.exe\"]");
        assert!(error(&text).contains("unbalanced"), "{}", error(&text));
    }

    #[test]
    fn a_session_file_extension_has_no_dot() {
        let text = with_process(
            r#"session_files = { block = "pid-named", folder = { block = "path", path = "~/s" }, extension = ".json" }"#,
        );
        assert!(error(&text).contains("extension"), "{}", error(&text));
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let text = VALID.replace("scope = \"user\"\n", "scope = \"user\"\ncolour = \"red\"\n");
        assert!(error(&text).contains("colour"), "{}", error(&text));
    }

    #[test]
    fn unknown_fields_inside_a_block_are_rejected() {
        let text = VALID.replace(
            r#"{ block = "json", dialect = "strict" }"#,
            r#"{ block = "json", dialect = "strict", comments = true }"#,
        );
        assert!(error(&text).contains("comments"), "{}", error(&text));
    }

    #[test]
    fn unknown_blocks_are_rejected() {
        let text = VALID.replace(
            r#"{ block = "json", dialect = "strict" }"#,
            r#"{ block = "yaml" }"#,
        );
        assert!(error(&text).contains("yaml"), "{}", error(&text));
    }

    #[test]
    fn an_unsupported_format_version_is_rejected() {
        let text = VALID.replace("format_version = 1", "format_version = 2");
        assert!(
            error(&text).contains("format_version 2"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn a_home_folder_that_is_not_anchored_is_rejected() {
        let text = VALID.replace(r#"path = "~" }"#, r#"path = "test/home" }"#);
        assert!(
            error(&text).contains("must be `~`, or start with `~/`, `%NAME%` or a drive"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn a_home_folder_from_a_variable_or_a_drive_loads() {
        for path in [
            "%APPDATA%/Test",
            "C:/test",
            "%LOCALAPPDATA%/Packages/Test_*/Data",
        ] {
            let text = VALID.replace(r#"path = "~" }"#, &format!(r#"path = "{path}" }}"#));
            assert!(
                parse("test.toml", &text).is_ok(),
                "{path}: {}",
                error(&text)
            );
        }
    }

    #[test]
    fn a_home_folder_in_the_user_folder_cannot_hold_wildcards() {
        let text = VALID.replace(r#"path = "~" }"#, r#"path = "~/.test-*" }"#);
        assert!(
            error(&text).contains("cannot hold wildcards"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn a_relative_path_that_climbs_out_is_rejected() {
        let text = VALID.replace(r#"".test.json""#, r#""../other/.test.json""#);
        assert!(
            error(&text).contains("without `.` or `..`"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn an_anchor_to_an_undeclared_root_is_rejected() {
        let text = VALID.replace(r#"root = "state""#, r#"root = "config""#);
        assert!(
            error(&text).contains("no root `config`"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn a_location_in_an_undeclared_scope_is_rejected() {
        let text = VALID.replace("scope = \"user\"\n", "scope = \"local\"\n");
        assert!(
            error(&text).contains("undeclared scope `local`"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn precedence_must_rank_every_location_of_the_kind() {
        let text = VALID.replace(r#"precedence = ["user-mcp"]"#, "precedence = []");
        assert!(
            error(&text).contains("leaves out location `user-mcp`"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn precedence_must_not_name_unknown_locations() {
        let text = VALID.replace(r#"["user-mcp"]"#, r#"["user-mcp", "ghost"]"#);
        assert!(error(&text).contains("`ghost`"), "{}", error(&text));
    }

    /// `VALID` with `mechanism` as the one Disable mechanism of its servers.
    fn with_disable(mechanism: &str) -> String {
        format!(
            "{VALID}disable = [{mechanism}]
"
        )
    }

    /// A name list in the home file, keyed by `{project}` when `per_project`,
    /// with `extra` fields.
    fn name_list(per_project: bool, extra: &str) -> String {
        let (key_path, context) = if per_project {
            (r#"["projects", "{project}", "off"]"#, "project")
        } else {
            (r#"["off"]"#, "all")
        };
        format!(
            r#"{{ block = "json-name-list", switch = {{ anchor = {{ block = "home", root = "state" }}, key_path = {key_path}, store_class = "plain" }}, target = "name", context = "{context}", enable_action = "remove-item", reason = "disabled_by_name"{extra} }}"#
        )
    }

    /// A field per name in the home file, with `extra` fields.
    fn json_field(extra: &str) -> String {
        format!(
            r#"{{ block = "json-field", switch = {{ anchor = {{ block = "home", root = "state" }}, key_path = ["servers", "{{name}}", "enabled"], store_class = "plain" }}, target = "entry", context = "all", reason = "disabled_natively", disabled_value = false{extra} }}"#
        )
    }

    #[test]
    fn a_disable_mechanism_loads() {
        let text = with_disable(&name_list(true, r#", locations = ["user-mcp"]"#));
        assert!(parse("test.toml", &text).is_ok(), "{}", error(&text));
    }

    #[test]
    fn a_json_field_mechanism_loads() {
        let text = with_disable(&json_field(
            r#", enable_action = "set", enabled_value = true, partial_values = ["name-only"]"#,
        ));
        assert!(parse("test.toml", &text).is_ok(), "{}", error(&text));
        let text = with_disable(&json_field(r#", enable_action = "delete-key""#));
        assert!(parse("test.toml", &text).is_ok(), "{}", error(&text));
    }

    #[test]
    fn a_json_field_set_on_enable_needs_an_enabled_value() {
        let text = with_disable(&json_field(r#", enable_action = "set""#));
        assert!(error(&text).contains("enabled_value"), "{}", error(&text));
    }

    #[test]
    fn a_name_list_targets_names_and_enables_by_removing_the_item() {
        let text = with_disable(&name_list(false, "").replace(r#""name""#, r#""entry""#));
        assert!(error(&text).contains("must be `name`"), "{}", error(&text));
        let text = with_disable(&name_list(false, "").replace("remove-item", "delete-key"));
        assert!(error(&text).contains("remove-item"), "{}", error(&text));
    }

    #[test]
    fn a_mechanism_context_agrees_with_where_its_switch_is() {
        let text = with_disable(&name_list(true, "").replace(r#""project""#, r#""all""#));
        assert!(error(&text).contains("context `all`"), "{}", error(&text));
        let text = with_disable(&name_list(false, "").replace(r#""all""#, r#""project""#));
        assert!(
            error(&text).contains("context `project`"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn a_mechanism_needs_a_reason_code() {
        let text =
            with_disable(&name_list(false, "").replace(r#", reason = "disabled_by_name""#, ""));
        assert!(error(&text).contains("reason"), "{}", error(&text));
    }

    #[test]
    fn a_disable_mechanism_for_an_unknown_location_is_rejected() {
        let text = with_disable(&name_list(false, r#", locations = ["ghost"]"#));
        assert!(
            error(&text).contains("`ghost` is not a location of this kind"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn a_disable_switch_in_a_project_cannot_be_keyed_by_project() {
        let text = with_disable(&name_list(true, "").replace(
            r#"{ block = "home", root = "state" }"#,
            r#"{ block = "project", path = ".test.json" }"#,
        ));
        assert!(
            error(&text).contains("cannot be keyed by `{project}`"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn checks_name_each_check_once() {
        let text = VALID.replace(
            r#"checks = ["disable", "precedence"]"#,
            r#"checks = ["precedence"]"#,
        );
        assert!(
            error(&text).contains("leaves out `disable`"),
            "{}",
            error(&text)
        );
        let text = VALID.replace(
            r#"checks = ["disable", "precedence"]"#,
            r#"checks = ["disable", "precedence", "disable"]"#,
        );
        assert!(error(&text).contains("twice"), "{}", error(&text));
    }

    #[test]
    fn a_shape_needs_the_shape_check_and_the_shape_check_needs_a_shape() {
        let shaped = VALID.replace(
            "on_read_failure = \"ignores_file\"
",
            "on_read_failure = \"ignores_file\"
shape = { block = \"fields\", one_of = [\"command\"] }
",
        );
        assert!(
            error(&shaped).contains("leaves out `shape`"),
            "{}",
            error(&shaped)
        );
        let checked = shaped.replace(r#"checks = ["disable""#, r#"checks = ["shape", "disable""#);
        assert!(parse("test.toml", &checked).is_ok(), "{}", error(&checked));
        let unshaped = VALID.replace(r#"checks = ["disable""#, r#"checks = ["shape", "disable""#);
        assert!(
            error(&unshaped).contains("no location declares one"),
            "{}",
            error(&unshaped)
        );
    }

    #[test]
    fn an_empty_shape_is_rejected() {
        let text = VALID
            .replace(
                "on_read_failure = \"ignores_file\"
",
                "on_read_failure = \"ignores_file\"
shape = { block = \"fields\" }
",
            )
            .replace(r#"checks = ["disable""#, r#"checks = ["shape", "disable""#);
        assert!(error(&text).contains("one_of or types"), "{}", error(&text));
    }

    #[test]
    fn a_location_needs_a_read_failure_reaction() {
        let text = VALID.replace(
            "on_read_failure = \"ignores_file\"
",
            "",
        );
        assert!(error(&text).contains("on_read_failure"), "{}", error(&text));
    }

    /// `VALID` with `verified` at its top and `installations` before its homes.
    fn with_installations(verified: &str, installations: &str) -> String {
        VALID
            .replace(
                "format_version = 1\n",
                &format!("format_version = 1\n{verified}\n"),
            )
            .replace("[[homes]]", &format!("{installations}\n\n[[homes]]"))
    }

    const INSTALLATION: &str = r#"[[installations]]
id = "native"
folder = "%LOCALAPPDATA%/Programs/test-*"
executable = "bin/test.exe"
versions = [
  { block = "exe-version" },
  { block = "appx-identity" },
  { block = "json-value", file = "*/resources/app/product.json", key_path = ["version"] },
  { block = "uninstall-key", hive = "current-user", key = "{DADADADA-ADAD}_is1" },
]
homes = ["default"]"#;

    #[test]
    fn installations_and_a_tested_range_load() {
        let text = with_installations(r#"verified = { min = "1.2", max = "1.4.0" }"#, INSTALLATION)
            + "\n[last_ran]\nanchor = { block = \"home\", root = \"state\" }\nkey_path = [\"lastVersion\"]\n";
        assert!(parse("test.toml", &text).is_ok(), "{}", error(&text));
    }

    #[test]
    fn a_tested_range_holds_versions_in_order() {
        let text = with_installations(r#"verified = { min = "2.0", max = "1.9" }"#, "");
        assert!(
            error(&text).contains("min 2.0 is above max 1.9"),
            "{}",
            error(&text)
        );
        let text = with_installations(r#"verified = { min = "latest", max = "1.9" }"#, "");
        assert!(error(&text).contains("not a version"), "{}", error(&text));
    }

    #[test]
    fn an_installation_reads_declared_homes_from_a_rooted_folder() {
        let text = with_installations("", &INSTALLATION.replace(r#"["default"]"#, r#"["ghost"]"#));
        assert!(
            error(&text).contains("undeclared home `ghost`"),
            "{}",
            error(&text)
        );
        let text = with_installations("", &INSTALLATION.replace("%LOCALAPPDATA%/", ""));
        assert!(error(&text).contains("must start with"), "{}", error(&text));
        let text = with_installations("", &INSTALLATION.replace("bin/test.exe", "../test.exe"));
        assert!(
            error(&text).contains("without `.` or `..`"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn an_installation_needs_a_known_version_source() {
        let text = with_installations(
            "",
            "[[installations]]\nid = \"x\"\nfolder = \"~/x\"\nexecutable = \"x.exe\"\nversions = []",
        );
        assert!(
            error(&text).contains("at least one source"),
            "{}",
            error(&text)
        );
        let text = with_installations(
            "",
            "[[installations]]\nid = \"x\"\nfolder = \"~/x\"\nexecutable = \"x.exe\"\nversions = [{ block = \"run-version-flag\" }]",
        );
        assert!(
            error(&text).contains("run-version-flag"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn the_last_ran_version_is_in_the_agent_home() {
        let text = format!(
            "{VALID}\n[last_ran]\nanchor = {{ block = \"project\", path = \".x.json\" }}\nkey_path = [\"v\"]\n"
        );
        assert!(error(&text).contains("not a Project"), "{}", error(&text));
    }

    #[test]
    fn duplicate_ids_are_rejected() {
        let text = format!("{VALID}\n[[scopes]]\nid = \"user\"\n");
        assert!(
            error(&text).contains("scope `user` is declared twice"),
            "{}",
            error(&text)
        );
    }

    const SKILL_FORMAT: &str = r#"{ block = "skill-folder", name_from = "frontmatter", no_frontmatter = "folder-name", malformed_frontmatter = "folder-name", no_name = "skip" }"#;

    /// `VALID` plus a folder of skill folders at `anchor`, read as `format`,
    /// with `checks` for its skills.
    fn with_skills(anchor: &str, format: &str, checks: &str) -> String {
        format!(
            r#"{VALID}
[[locations]]
id = "user-skills"
anchor = {anchor}
format = {format}
kind = "skill"
scope = "user"
provenance = "file"
store_class = "plain"
on_read_failure = "ignores_file"

[items.skill]
precedence = ["user-skills"]
competition = {{ block = "exact-name" }}
checks = {checks}
"#
        )
    }

    const CHECKS: &str = r#"["frontmatter", "disable", "precedence"]"#;

    #[test]
    fn a_folder_of_skill_folders_loads_in_the_home_a_project_or_the_machine() {
        for anchor in [
            r#"{ block = "home", root = "state", path = "skills" }"#,
            r#"{ block = "project", path = ".test/skills", walk = "up-to-repo-root" }"#,
            r#"{ block = "machine", path = "%ProgramFiles%/Test/skills" }"#,
            r#"{ block = "machine", path = "C:/Test/skills" }"#,
        ] {
            let text = with_skills(anchor, SKILL_FORMAT, CHECKS);
            assert!(parse("test.toml", &text).is_ok(), "{}", error(&text));
        }
    }

    #[test]
    fn a_machine_path_starts_from_the_home_folder_a_variable_or_a_drive_without_wildcards() {
        for (path, message) in [
            ("skills", "must start with"),
            ("%ProgramFiles%/*/skills", "wildcards"),
        ] {
            let anchor = format!(r#"{{ block = "machine", path = "{path}" }}"#);
            let text = with_skills(&anchor, SKILL_FORMAT, CHECKS);
            assert!(error(&text).contains(message), "{path}: {}", error(&text));
        }
        let anchor = r#"{ block = "machine", path = "~/.agents/skills" }"#;
        let text = with_skills(anchor, SKILL_FORMAT, CHECKS);
        assert!(parse("test.toml", &text).is_ok(), "{}", error(&text));
    }

    #[test]
    fn skills_and_the_skill_folder_format_go_together() {
        let anchor = r#"{ block = "home", root = "state" }"#;
        let text = with_skills(anchor, r#"{ block = "json", dialect = "strict" }"#, CHECKS);
        assert!(
            error(&text).contains("kind `skill` needs the skill-folder format"),
            "{}",
            error(&text)
        );
        let text = VALID.replace(r#"{ block = "json", dialect = "strict" }"#, SKILL_FORMAT);
        assert!(
            error(&text).contains("kind `mcp` needs a file format"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn a_folder_of_skill_folders_has_no_root() {
        let text = with_skills(
            r#"{ block = "home", root = "state" }"#,
            SKILL_FORMAT,
            CHECKS,
        )
        .replace("kind = \"skill\"", "root = [\"skills\"]\nkind = \"skill\"");
        assert!(error(&text).contains("has no root"), "{}", error(&text));
    }

    #[test]
    fn skills_need_the_frontmatter_check_and_only_skills_take_it() {
        let anchor = r#"{ block = "home", root = "state" }"#;
        let text = with_skills(anchor, SKILL_FORMAT, r#"["disable", "precedence"]"#);
        assert!(
            error(&text).contains("leaves out `frontmatter`"),
            "{}",
            error(&text)
        );
        let text = VALID.replace(
            r#"checks = ["disable", "precedence"]"#,
            r#"checks = ["frontmatter", "disable", "precedence"]"#,
        );
        assert!(
            error(&text).contains("no location is a folder of skill folders"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn a_reserved_skill_name_is_one_folder_name() {
        let format = SKILL_FORMAT.replace(" }", r#", reserved = ["a/b"] }"#);
        let text = with_skills(r#"{ block = "home", root = "state" }"#, &format, CHECKS);
        assert!(
            error(&text).contains("reserved name `a/b`"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn a_disable_switch_cannot_walk() {
        let text = with_disable(&name_list(false, "").replace(
            r#"{ block = "home", root = "state" }"#,
            r#"{ block = "project", path = ".test.json", walk = "up-to-repo-root" }"#,
        ));
        assert!(error(&text).contains("cannot walk"), "{}", error(&text));
    }

    /// `VALID` with `lookup` as the lookup of a gate affecting its servers,
    /// which they are read only while it is on, and `extra` gate fields.
    fn with_gate(lookup: &str, extra: &str) -> String {
        VALID.replace(
            "on_read_failure = \"ignores_file\"\n",
            "on_read_failure = \"ignores_file\"\nwhen = [{ block = \"feature-gate\", gate = \"sync\" }]\n",
        ) + &format!(
            "\n[[feature_gates]]\nname = \"sync\"\nassumed = true\naffects = [\"user-mcp\"]\nlookup = {lookup}\n{extra}"
        )
    }

    const JSON_LOOKUP: &str = r#"{ block = "json-value", anchor = { block = "home", root = "state" }, key_path = ["features", "{gate}"], cached_at = ["fetchedAt"] }"#;

    #[test]
    fn a_feature_gate_with_a_lookup_and_a_bypass_loads() {
        let bypass = r#"bypass = { block = "env", variables = [{ name = "NO_FLAGS", values = "truthy" }, { name = "USE_OTHER", values = "non-empty", unless = ["HOSTED"] }], env_maps = [{ anchor = { block = "home", root = "state" }, key_path = ["env"] }] }"#;
        let text = with_gate(JSON_LOOKUP, bypass);
        assert!(parse("test.toml", &text).is_ok(), "{}", error(&text));
        let sqlite = r#"{ block = "sqlite-json", anchor = { block = "home", root = "state" }, key = "bootstrap", key_path = ["feature_gates", "{gate}", "value"], hash = "djb2" }"#;
        let text = with_gate(sqlite, "");
        assert!(parse("test.toml", &text).is_ok(), "{}", error(&text));
    }

    #[test]
    fn a_lookup_key_path_names_the_gate_once() {
        let text = with_gate(&JSON_LOOKUP.replace(r#", "{gate}""#, ""), "");
        assert!(error(&text).contains("exactly once"), "{}", error(&text));
        let text = with_gate(&JSON_LOOKUP.replace(r#""features""#, r#""{gate}""#), "");
        assert!(error(&text).contains("exactly once"), "{}", error(&text));
    }

    #[test]
    fn a_lookup_lies_in_the_agent_home() {
        for anchor in [
            r#"{ block = "project", path = ".test.json" }"#,
            r#"{ block = "machine", path = "%ProgramData%/Test/flags.json" }"#,
        ] {
            let text = with_gate(
                &JSON_LOOKUP.replace(r#"{ block = "home", root = "state" }"#, anchor),
                "",
            );
            assert!(
                error(&text).contains("must lie in the Agent home"),
                "{}",
                error(&text)
            );
        }
        let text = with_gate(
            &JSON_LOOKUP.replace(r#"root = "state""#, r#"root = "cache""#),
            "",
        );
        assert!(error(&text).contains("no root `cache`"), "{}", error(&text));
    }

    #[test]
    fn a_sqlite_lookup_needs_a_key() {
        let sqlite = r#"{ block = "sqlite-json", anchor = { block = "home", root = "state" }, key = " ", key_path = ["{gate}"] }"#;
        let text = with_gate(sqlite, "");
        assert!(error(&text).contains("needs a key"), "{}", error(&text));
    }

    #[test]
    fn a_feature_gate_affects_declared_locations() {
        let text = with_gate(JSON_LOOKUP, "").replace(r#"affects = ["user-mcp"]"#, "affects = []");
        assert!(
            error(&text).contains("names no location"),
            "{}",
            error(&text)
        );
        let text = with_gate(JSON_LOOKUP, "").replace(
            r#"affects = ["user-mcp"]"#,
            r#"affects = ["user-mcp", "ghost"]"#,
        );
        assert!(
            error(&text).contains("undeclared location `ghost`"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn a_condition_names_a_declared_gate_that_affects_its_location() {
        let text = with_gate(JSON_LOOKUP, "").replace(r#"gate = "sync""#, r#"gate = "other""#);
        assert!(
            error(&text).contains("undeclared feature gate `other`"),
            "{}",
            error(&text)
        );
        let text = format!(
            "{}\n[[locations]]\nid = \"other-mcp\"\nanchor = {{ block = \"home\", root = \"state\" }}\nformat = {{ block = \"json\", dialect = \"strict\" }}\nroot = [\"other\"]\nkind = \"mcp\"\nscope = \"user\"\nprovenance = \"file\"\nstore_class = \"plain\"\non_read_failure = \"ignores_file\"\n",
            with_gate(JSON_LOOKUP, "")
        )
        .replace(r#"precedence = ["user-mcp"]"#, r#"precedence = ["user-mcp", "other-mcp"]"#)
        .replace(r#"affects = ["user-mcp"]"#, r#"affects = ["other-mcp"]"#);
        assert!(
            error(&text).contains("does not list it in `affects`"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn a_feature_gate_is_declared_once() {
        let gate = with_gate(JSON_LOOKUP, "");
        let twice = gate.clone() + &gate[gate.find("\n[[feature_gates]]").unwrap()..];
        assert!(
            error(&twice).contains("feature gate `sync` is declared twice"),
            "{}",
            error(&twice)
        );
    }

    #[test]
    fn a_bypass_names_variables() {
        let text = with_gate(JSON_LOOKUP, r#"bypass = { block = "env", variables = [] }"#);
        assert!(error(&text).contains("needs variables"), "{}", error(&text));
        let text = with_gate(
            JSON_LOOKUP,
            r#"bypass = { block = "env", variables = [{ name = "A=1", values = "truthy" }] }"#,
        );
        assert!(
            error(&text).contains("not a variable name"),
            "{}",
            error(&text)
        );
    }

    /// `VALID` with managed settings, a location reading them, and `items`
    /// added to the rules of its servers, whose checks are `checks`.
    fn with_policy(items: &str, checks: &str) -> String {
        let settings = r#"
[[settings]]
id = "managed"
sources = [
  { block = "registry-value", hive = "local-machine", key = 'SOFTWARE\Policies\Test', value = "Settings" },
  { block = "files", path = "%ProgramFiles%/Test/settings.json", drop_ins = "%ProgramFiles%/Test/settings.d" },
]

[[locations]]
id = "managed-mcp"
anchor = { block = "settings", settings = "managed" }
format = { block = "json", dialect = "strict" }
root = ["servers"]
kind = "mcp"
scope = "user"
provenance = "managed"
store_class = "plain"
on_read_failure = "ignores_file"
"#;
        let text = VALID
            .replace("[items.mcp]", &format!("{settings}\n[items.mcp]"))
            .replace(r#"["user-mcp"]"#, r#"["managed-mcp", "user-mcp"]"#)
            .replace(
                r#"checks = ["disable", "precedence"]"#,
                &format!("checks = {checks}"),
            );
        format!("{text}{items}")
    }

    const POLICY_CHECKS: &str = r#"["policy", "rules", "disable", "approval", "precedence"]"#;

    const RULES_AND_GATES: &str = r#"
[[items.mcp.gates]]
block = "managed-exclusive"
location = "user-mcp"
except = ["managed-mcp"]

[[items.mcp.gates]]
block = "approval-list"
locations = ["user-mcp"]
prompt = "on-start"
sources = [{ anchor = { block = "home", root = "state" }, key_path = ["projects", "{project}", "approved"] }]

[[items.mcp.rules]]
block = "allow-deny-list"
list = "deny"
match = { name = "name", command = "command", url = "url" }
sources = [
  { anchor = { block = "settings", settings = "managed" }, key_path = ["denied"], policy = true },
  { anchor = { block = "home", root = "state" }, key_path = ["denied"] },
]

[[items.mcp.rules]]
block = "global-switch"
off = false
locations = ["user-mcp"]
sources = [{ anchor = { block = "home", root = "state" }, key_path = ["serversOn"] }]
"#;

    #[test]
    fn settings_rules_and_gates_load() {
        let text = with_policy(RULES_AND_GATES, POLICY_CHECKS);
        assert!(parse("test.toml", &text).is_ok(), "{}", error(&text));
    }

    #[test]
    fn a_settings_anchor_names_declared_settings() {
        let text = with_policy(RULES_AND_GATES, POLICY_CHECKS).replace(
            r#"settings = "managed" }, key_path = ["denied"]"#,
            r#"settings = "other" }, key_path = ["denied"]"#,
        );
        assert!(
            error(&text).contains("undeclared settings `other`"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn a_disable_switch_in_settings_is_only_read() {
        let switch = name_list(false, "").replace(
            r#"{ block = "home", root = "state" }"#,
            r#"{ block = "settings", settings = "managed" }"#,
        );
        let text = with_policy(
            &format!("disable = [{switch}]\n"),
            r#"["disable", "precedence"]"#,
        );
        assert!(
            error(&text).contains("a switch in settings is only read"),
            "{}",
            error(&text)
        );
        let text = text.replace(
            r#"reason = "disabled_by_name""#,
            r#"reason = "disabled_by_name", enable_offered = false"#,
        );
        assert!(parse("test.toml", &text).is_ok(), "{}", error(&text));
    }

    #[test]
    fn a_policy_switch_is_only_read() {
        let text = with_disable(&json_field(
            r#", enable_action = "delete-key", policy = true"#,
        ));
        assert!(
            error(&text).contains("a policy switch is only read"),
            "{}",
            error(&text)
        );
        let text = with_disable(&json_field(
            r#", enable_action = "delete-key", policy = true, enable_offered = false"#,
        ));
        assert!(parse("test.toml", &text).is_ok(), "{}", error(&text));
    }

    #[test]
    fn settings_need_sources_that_name_a_registry_key_or_files() {
        let text = with_policy("", r#"["disable", "precedence"]"#)
            .replace(r#"'SOFTWARE\Policies\Test'"#, r#"'HKLM/SOFTWARE'"#);
        assert!(error(&text).contains("registry key"), "{}", error(&text));
        let text = with_policy("", r#"["disable", "precedence"]"#)
            .replace("%ProgramFiles%/Test/settings.d", "%ProgramFiles%/Test/*.d");
        assert!(error(&text).contains("wildcards"), "{}", error(&text));
        // A copy the Agent caches in the user's home is a source too.
        let text = with_policy("", r#"["disable", "precedence"]"#)
            .replace("%ProgramFiles%/Test/settings.d", "~/.test/settings.d");
        assert!(parse("test.toml", &text).is_ok(), "{}", error(&text));
    }

    #[test]
    fn the_merge_key_of_settings_is_a_control_key() {
        let merging = |control_keys: &str| {
            with_policy("", r#"["disable", "precedence"]"#).replace(
                "id = \"managed\"\n",
                &format!(
                    "id = \"managed\"\ncontrol_keys = {control_keys}\nmerge = {{ key = \"behavior\", value = \"merge\" }}\n"
                ),
            )
        };
        let text = merging("[]");
        assert!(
            error(&text).contains("merge key `behavior` must be one of control_keys"),
            "{}",
            error(&text)
        );
        let text = merging(r#"["behavior"]"#);
        assert!(parse("test.toml", &text).is_ok(), "{}", error(&text));
    }

    #[test]
    fn only_a_setting_in_settings_is_read_from_the_highest_source() {
        let text = with_policy(RULES_AND_GATES, POLICY_CHECKS).replace(
            r#"{ anchor = { block = "home", root = "state" }, key_path = ["denied"] }"#,
            r#"{ anchor = { block = "home", root = "state" }, key_path = ["denied"], highest_source = true }"#,
        );
        assert!(
            error(&text).contains("highest_source needs a settings anchor"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn policy_rules_and_approvals_each_need_their_check() {
        for (check, message) in [
            ("\"policy\", ", "leaves out `policy`"),
            ("\"rules\", ", "leaves out `rules`"),
            ("\"approval\", ", "leaves out `approval`"),
        ] {
            let text = with_policy(RULES_AND_GATES, &POLICY_CHECKS.replace(check, ""));
            assert!(error(&text).contains(message), "{check}: {}", error(&text));
        }
        let text = with_policy("", POLICY_CHECKS);
        assert!(
            error(&text).contains("checks holds `policy`, but no gate is managed-exclusive"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn rules_and_gates_cover_locations_of_their_kind() {
        for (from, to) in [
            (
                r#"locations = ["user-mcp"]
sources = [{ anchor = { block = "home", root = "state" }, key_path = ["serversOn"] }]"#,
                r#"locations = ["ghost"]
sources = [{ anchor = { block = "home", root = "state" }, key_path = ["serversOn"] }]"#,
            ),
            (r#"location = "user-mcp""#, r#"location = "ghost""#),
            (
                r#"locations = ["user-mcp"]
prompt"#,
                r#"locations = ["ghost"]
prompt"#,
            ),
        ] {
            let text = with_policy(&RULES_AND_GATES.replace(from, to), POLICY_CHECKS);
            assert!(
                error(&text).contains("`ghost` is not a location of this kind"),
                "{to}: {}",
                error(&text)
            );
        }
    }

    #[test]
    fn an_allow_or_deny_list_matches_by_some_field_and_only_an_allow_list_is_strict() {
        let text = with_policy(
            &RULES_AND_GATES.replace(
                r#"match = { name = "name", command = "command", url = "url" }"#,
                "match = {}",
            ),
            POLICY_CHECKS,
        );
        assert!(error(&text).contains("match needs"), "{}", error(&text));
        let text = with_policy(
            &RULES_AND_GATES.replace(r#"list = "deny""#, "list = \"deny\"\nstrict = true"),
            POLICY_CHECKS,
        );
        assert!(
            error(&text).contains("only an allow list can be strict"),
            "{}",
            error(&text)
        );
        let text = with_policy(
            &RULES_AND_GATES.replace(
                r#"list = "deny""#,
                "list = \"deny\"\npolicy_only = { anchor = { block = \"settings\", settings = \"managed\" }, key_path = [\"only\"] }",
            ),
            POLICY_CHECKS,
        );
        assert!(
            error(&text).contains("only an allow list can be policy_only"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn an_approval_is_no_policy_and_a_setting_in_a_project_is_not_keyed_by_project() {
        let text = with_policy(
            &RULES_AND_GATES.replace(
                r#"key_path = ["projects", "{project}", "approved"] }"#,
                r#"key_path = ["approved"], policy = true }"#,
            ),
            POLICY_CHECKS,
        );
        assert!(
            error(&text).contains("an approval is no policy"),
            "{}",
            error(&text)
        );
        let text = with_policy(
            &RULES_AND_GATES.replace(
                r#"{ block = "home", root = "state" }, key_path = ["projects", "{project}", "approved"]"#,
                r#"{ block = "project", path = ".test.json" }, key_path = ["projects", "{project}", "approved"]"#,
            ),
            POLICY_CHECKS,
        );
        assert!(
            error(&text).contains("can be keyed by `{project}`"),
            "{}",
            error(&text)
        );
    }

    /// `VALID` plus plugins and a location in them, with `extra` appended.
    fn with_plugins(extra: &str) -> String {
        let plugins = r#"
roots.plugins = "plugins"

[[plugins]]
id = "installed"
registry = { block = "json-plugin-records", anchor = { block = "home", root = "plugins", path = "installed.json" }, root = ["plugins"], folder = "path" }
uninstall = "agent plugin uninstall {id} --scope {scope}"

[[locations]]
id = "plugin-mcp"
anchor = { block = "plugin", plugins = "installed", path = "mcp.json" }
name_template = "plugin:{plugin}:{name}"
format = { block = "json", dialect = "strict" }
root = ["servers"]
kind = "mcp"
scope = "user"
provenance = "plugin"
store_class = "plain"
on_read_failure = "ignores_file"
"#;
        VALID
            .replace(
                "roots.state = \".test.json\"
",
                &format!("roots.state = \".test.json\"{plugins}"),
            )
            .replace(
                r#"precedence = ["user-mcp"]"#,
                r#"precedence = ["user-mcp", "plugin-mcp"]"#,
            )
            + extra
    }

    /// A Disable mechanism of `target` whose switch is keyed by `key`.
    fn plugin_switch(target: &str, key: &str, locations: &str) -> String {
        format!(
            r#"
[[items.mcp.disable]]
block = "json-field"
switch = {{ anchor = {{ block = "home", root = "state" }}, key_path = ["enabledPlugins", "{key}"], store_class = "plain" }}
target = "{target}"
context = "all"
enable_action = "set"
enabled_value = true
disabled_value = false
reason = "plugin_disabled"
locations = {locations}
"#
        )
    }

    #[test]
    fn plugins_and_their_switch_load() {
        let text = with_plugins(&plugin_switch("plugin", "{plugin_id}", r#"["plugin-mcp"]"#));
        assert!(parse("test.toml", &text).is_ok(), "{}", error(&text));
    }

    const MANIFEST_SKILLS: &str = r#"{ block = "manifest-skills", field = ["skills"], default = "skills", skills = { name_from = "folder", no_frontmatter = "folder-name", malformed_frontmatter = "skip", no_name = "folder-name" } }"#;

    #[test]
    fn manifest_skills_are_read_in_plugins_alone() {
        let in_plugins = with_plugins("")
            + &format!(
                r#"
[[locations]]
id = "plugin-skills"
anchor = {{ block = "plugin", plugins = ["installed"], path = ["plugin.json"] }}
format = {MANIFEST_SKILLS}
kind = "skill"
scope = "user"
provenance = "plugin"
store_class = "plain"
on_read_failure = "ignores_file"

[items.skill]
precedence = ["plugin-skills"]
competition = {{ block = "exact-name" }}
checks = {CHECKS}
"#
            );
        assert!(
            parse("test.toml", &in_plugins).is_ok(),
            "{}",
            error(&in_plugins)
        );
        let in_home = with_skills(STATE, MANIFEST_SKILLS, CHECKS);
        assert!(
            error(&in_home).contains("only a location in plugins"),
            "{}",
            error(&in_home)
        );
    }

    #[test]
    fn a_project_s_plugin_keys_lie_in_the_project_folder() {
        let keys = |anchor: &str| {
            with_plugins("").replace(
                r#"registry = { block = "json-plugin-records", anchor = { block = "home", root = "plugins", path = "installed.json" }, root = ["plugins"], folder = "path" }"#,
                &format!(
                    r#"registry = {{ block = "project-plugin-keys", anchor = {anchor}, root = ["plugins"], scope = "project", cache = {{ block = "machine", path = "~/.agent/cache" }}, marker = ".complete" }}"#
                ),
            )
        };
        let text = keys(IN_PROJECT);
        assert!(parse("test.toml", &text).is_ok(), "{}", error(&text));
        let text = keys(STATE);
        assert!(
            error(&text).contains("lie in a file of the Project folder"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn a_plugin_anchor_names_declared_plugins() {
        let text = with_plugins("").replace(
            r#"plugins = "installed", path"#,
            r#"plugins = "other", path"#,
        );
        assert!(
            error(&text).contains("undeclared plugins `other`"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn a_name_template_names_the_entry_once_in_a_location_in_plugins() {
        let text = with_plugins("").replace("plugin:{plugin}:{name}", "plugin:{plugin}");
        assert!(
            error(&text).contains("must hold `{name}` once"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn the_plugin_target_and_a_switch_keyed_by_plugin_go_together() {
        let text = with_plugins(&plugin_switch("name", "{plugin_id}", r#"["plugin-mcp"]"#));
        assert!(
            error(&text).contains("exactly when the target is `plugin`"),
            "{}",
            error(&text)
        );
        let text = with_plugins(&plugin_switch("plugin", "{name}", r#"["plugin-mcp"]"#));
        assert!(
            error(&text).contains("exactly when the target is `plugin`"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn the_plugin_target_covers_locations_in_plugins_only() {
        let text = with_plugins(&plugin_switch("plugin", "{plugin_id}", r#"["user-mcp"]"#));
        assert!(
            error(&text).contains("locations in plugins only"),
            "{}",
            error(&text)
        );
        let text = with_plugins(&plugin_switch("plugin", "{plugin_id}", "[]"));
        assert!(
            error(&text).contains("needs its locations listed"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn an_uninstall_command_names_the_plugin_and_drops_an_unknown_scope() {
        let definition = parse("test.toml", &with_plugins("")).unwrap();
        let plugins = definition.declared_plugins("installed").unwrap();
        assert_eq!(
            plugins.uninstall("a@m", Some("local")).as_deref(),
            Some("agent plugin uninstall a@m --scope local")
        );
        assert_eq!(
            plugins.uninstall("a@m", None).as_deref(),
            Some("agent plugin uninstall a@m")
        );
    }

    #[test]
    fn lookup_orders_name_declared_homes_and_only_their_homes_take_conditions() {
        let ordered = VALID.replace(
            "format_version = 1\n",
            "format_version = 1\nlookup_orders = [[\"default\"]]\n",
        );
        let when = "roots.state = \".test.json\"\nwhen = [{ block = \"unc-variable\", variable = \"APPDATA\" }]\n";
        let text = ordered.replace("roots.state = \".test.json\"\n", when);
        assert!(parse("test.toml", &text).is_ok(), "{}", error(&text));

        let text = VALID.replace("roots.state = \".test.json\"\n", when);
        assert!(
            error(&text).contains("takes `when` only in a lookup order"),
            "{}",
            error(&text)
        );
        let text = ordered.replace("[\"default\"]", "[\"other\"]");
        assert!(
            error(&text).contains("lookup_orders names undeclared home `other`"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn the_default_home_is_one_fixed_path() {
        let default = "roots.state = \".test.json\"\ndefault = true\n";
        let text = VALID.replace("roots.state = \".test.json\"\n", default);
        assert!(parse("test.toml", &text).is_ok(), "{}", error(&text));

        for folder in [
            r#"{ block = "env", variable = "TEST_HOME" }"#,
            r#"{ block = "path", path = "%LOCALAPPDATA%/Test_*" }"#,
        ] {
            let text = text.replace(r#"{ block = "path", path = "~" }"#, folder);
            assert!(
                error(&text).contains("is the default, so its folder must be one fixed path"),
                "{}",
                error(&text)
            );
        }

        let second = format!(
            "{default}\n[[homes]]\nid = \"other\"\nfolder = {{ block = \"path\", path = \"~/.other\" }}\n{default}"
        );
        let text = VALID.replace("roots.state = \".test.json\"\n", &second);
        assert!(
            error(&text).contains("at most one home can be the default"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn only_a_location_in_plugins_holds_one_entry() {
        let text = VALID.replace(
            "root = [\"servers\"]\n",
            "root = [\"server\"]\none_entry = true\n",
        );
        assert!(
            error(&text).contains("only a file location in plugins, with a root, holds one Entry"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn a_switch_file_per_plugin_counts_as_keyed_by_plugin() {
        let switch = plugin_switch("plugin", "isEnabled", r#"["plugin-mcp"]"#).replace(
            r#"{ block = "home", root = "state" }"#,
            r#"{ block = "home", root = "plugins", path = "settings/{plugin_id}.json" }"#,
        );
        let text = with_plugins(&switch);
        assert!(parse("test.toml", &text).is_ok(), "{}", error(&text));

        let text = with_plugins(&switch.replace("\"isEnabled\"", "\"{plugin_id}\""));
        assert!(
            error(&text).contains("in its file's path or its key path, not both"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn a_location_cannot_have_a_file_per_plugin() {
        let text = VALID.replace(
            r#"anchor = { block = "home", root = "state" }"#,
            r#"anchor = { block = "home", root = "state", path = "{plugin_id}.json" }"#,
        );
        assert!(
            error(&text).contains("anchored once per home"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn a_switch_named_by_a_list_item_is_only_read() {
        let mechanism = r#"
[[items.mcp.disable]]
block = "json-field"
switch = { anchor = { block = "entry", up = 2, path = "manifest.json" }, key_path = ["items", "{name}", "on"], named_by = "name", store_class = "plain" }
target = "name"
context = "all"
enable_action = "set"
enabled_value = true
disabled_value = false
reason = "disabled_natively"
enable_offered = false
"#;
        let text = format!("{VALID}{mechanism}");
        assert!(parse("test.toml", &text).is_ok(), "{}", error(&text));

        let text = text.replace("enable_offered = false\n", "");
        assert!(
            error(&text).contains("enable_offered must be false"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn a_disable_mechanism_is_read_under_feature_gates_that_affect_its_locations() {
        let mechanism = r#"
[[items.mcp.disable]]
block = "json-name-list"
switch = { anchor = { block = "home", root = "state" }, key_path = ["off"], store_class = "plain" }
target = "name"
context = "all"
enable_action = "remove-item"
reason = "disabled_natively"
when = [{ block = "feature-gate", gate = "moved", is = false }]
"#;
        let gate = format!(
            "\n[[feature_gates]]\nname = \"moved\"\nassumed = false\naffects = [\"user-mcp\"]\nlookup = {JSON_LOOKUP}\n"
        );
        let text = format!("{VALID}{mechanism}{gate}");
        assert!(parse("test.toml", &text).is_ok(), "{}", error(&text));

        let undeclared = format!("{VALID}{mechanism}");
        assert!(
            error(&undeclared).contains("undeclared feature gate `moved`"),
            "{}",
            error(&undeclared)
        );
        let unaffected = text.replace("affects = [\"user-mcp\"]", "affects = []");
        assert!(
            error(&unaffected).contains("affects"),
            "{}",
            error(&unaffected)
        );
        let setting = text.replace(
            r#"{ block = "feature-gate", gate = "moved", is = false }"#,
            r#"{ block = "setting", sources = [{ anchor = { block = "home", root = "state" }, key_path = ["x"] }], equals = true, default = false }"#,
        );
        assert!(
            error(&setting).contains("feature-gate conditions only"),
            "{}",
            error(&setting)
        );
    }

    #[test]
    fn only_a_file_keeps_comments_out() {
        let text = VALID.replace(
            "on_read_failure = \"ignores_file\"
",
            "on_read_failure = \"ignores_file\"
no_comments = true
",
        );
        assert!(parse("test.toml", &text).is_ok(), "{}", error(&text));
        let skills = r#"
[[locations]]
id = "skills"
anchor = { block = "home", root = "state" }
format = { block = "skill-folder", name_from = "folder", no_frontmatter = "folder-name", malformed_frontmatter = "skip", no_name = "folder-name" }
kind = "skill"
scope = "user"
provenance = "file"
store_class = "plain"
on_read_failure = "ignores_file"
no_comments = true

[items.skill]
precedence = ["skills"]
competition = { block = "exact-name" }
checks = ["frontmatter", "disable", "precedence"]
"#;
        let text = format!("{VALID}{skills}");
        assert!(
            error(&text).contains("no_comments needs a file format"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn djb2_hashes_gate_names_as_statsig_does() {
        assert_eq!(blocks::djb2("dedupe_mcp_servers"), "1316230643");
        assert_eq!(
            blocks::djb2("mcp_profile_scoped_global_disabled_state"),
            "713966870"
        );
        assert_eq!(blocks::djb2("enable_cc_plugin_import"), "1307925454");
        assert_eq!(blocks::djb2(""), "0");
    }

    /// `VALID` with its location's `anchor`, `format` and `root` lines replaced.
    fn with_location(anchor: &str, format: &str, root: &str) -> String {
        VALID
            .replace(
                r#"anchor = { block = "home", root = "state" }"#,
                &format!("anchor = {anchor}"),
            )
            .replace(
                r#"format = { block = "json", dialect = "strict" }"#,
                &format!("format = {format}"),
            )
            .replace(r#"root = ["servers"]"#, &format!("root = {root}"))
    }

    const SQLITE: &str = r#"{ block = "sqlite-json", key = "cache" }"#;
    const STORAGE: &str = r#"{ block = "project-storage", path = "state.vscdb" }"#;
    const STORAGE_REGISTRY: &str = r#"[projects]
registries = [{ block = "json-folders", anchor = { block = "home", root = "state" }, file = "workspace.json", fields = ["folder"], spelling = "file-uri" }]

[[scopes]]"#;

    #[test]
    fn a_project_storage_location_needs_a_json_folders_registry() {
        let text = with_location(STORAGE, SQLITE, r#"["cache", "*", "servers"]"#);
        assert!(error(&text).contains("json-folders"), "{}", error(&text));
        let text = text.replacen("[[scopes]]", STORAGE_REGISTRY, 1);
        assert!(parse("test.toml", &text).is_ok(), "{}", error(&text));
    }

    #[test]
    fn a_sqlite_location_s_root_starts_with_its_key() {
        let text = with_location(STATE, SQLITE, r#"["servers"]"#);
        assert!(
            error(&text).contains("starts with its key `cache`"),
            "{}",
            error(&text)
        );
        let text = with_location(STATE, SQLITE, r#"["cache"]"#);
        assert!(parse("test.toml", &text).is_ok(), "{}", error(&text));
    }

    #[test]
    fn named_by_needs_a_root_and_a_name() {
        let named = |field: &str, root: &str| {
            with_location(STATE, r#"{ block = "json", dialect = "strict" }"#, root).replace(
                r#"kind = "mcp""#,
                &format!("named_by = \"{field}\"\nkind = \"mcp\""),
            )
        };
        let text = named("", r#"["servers"]"#);
        assert!(
            error(&text).contains("named_by must not be empty"),
            "{}",
            error(&text)
        );
        let text = named("label", "[]");
        assert!(
            error(&text).contains("named_by needs a root"),
            "{}",
            error(&text)
        );
        assert!(parse("test.toml", &named("label", r#"["servers"]"#)).is_ok());
    }

    #[test]
    fn a_project_file_location_needs_an_agent_that_opens_workspace_files() {
        let text = with_location(
            r#"{ block = "project-file" }"#,
            r#"{ block = "json", dialect = "jsonc" }"#,
            r#"["settings", "mcp", "servers"]"#,
        );
        assert!(error(&text).contains("workspace_files"), "{}", error(&text));
        let text = text.replacen(
            "[[scopes]]",
            "[projects]\nworkspace_files = true\n\n[[scopes]]",
            1,
        );
        assert!(parse("test.toml", &text).is_ok(), "{}", error(&text));
    }

    #[test]
    fn a_bare_location_needs_a_root_in_a_file() {
        let text = VALID.replace(r#"kind = "mcp""#, "bare = true\nkind = \"mcp\"");
        assert!(parse("test.toml", &text).is_ok(), "{}", error(&text));
        let text = with_location(STATE, r#"{ block = "json", dialect = "strict" }"#, "[]")
            .replace(r#"kind = "mcp""#, "bare = true\nkind = \"mcp\"");
        assert!(error(&text).contains("can be bare"), "{}", error(&text));
    }

    #[test]
    fn a_case_insensitive_competition_reads_its_setting_in_declared_files() {
        let competition = r#"competition = { block = "case-insensitive-name", only_while = { equals = "disable", default = "disable", sources = [{ anchor = { block = "home", root = "settings" }, key_path = ["collision"], dialect = "jsonc" }] } }"#;
        let text = VALID.replace(r#"competition = { block = "exact-name" }"#, competition);
        assert!(
            error(&text).contains("declares no root `settings`"),
            "{}",
            error(&text)
        );
        let text = text.replace(
            "roots.state = \".test.json\"",
            "roots.state = \".test.json\"\nroots.settings = \"settings.json\"",
        );
        assert!(parse("test.toml", &text).is_ok(), "{}", error(&text));
    }

    #[test]
    fn a_json_list_home_reads_a_list_of_folders_below_a_base() {
        let home = |file: &str| {
            VALID.replace(
                r#"folder = { block = "path", path = "~" }"#,
                &format!(r#"folder = {{ block = "json-list", file = "{file}", key_path = ["profiles"], field = "location", base = "%APPDATA%/Test/profiles" }}"#),
            )
        };
        assert!(parse("test.toml", &home("%APPDATA%/Test/storage.json")).is_ok());
        let text = home("~/storage.json");
        assert!(
            error(&text).contains("`%NAME%` or a drive"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn a_mirrored_home_copies_a_declared_home() {
        let mirror = |source: &str| {
            VALID.replace(
                "[[scopes]]",
                &format!(
                    "[[mirrored_homes]]\nid = \"mirror\"\nowner = \"Orca\"\nfolder = {{ block = \"path\", path = \"%APPDATA%/mirror\" }}\nsource = \"{source}\"\n\n[[scopes]]"
                ),
            )
        };
        assert!(parse("test.toml", &mirror("default")).is_ok());
        let text = mirror("other");
        assert!(
            error(&text).contains("undeclared source home `other`"),
            "{}",
            error(&text)
        );
        let text = mirror("default").replace("id = \"mirror\"", "id = \"default\"");
        assert!(error(&text).contains("declared twice"), "{}", error(&text));
    }

    #[test]
    fn a_kind_that_merges_checks_its_merge_and_competes_by_name() {
        let merging = VALID.replace(
            r#"checks = ["disable", "precedence"]"#,
            "merge = { block = \"deep-merge\", exclusive = [\"command\", \"url\"], enabled = \"enabled\" }\nchecks = [\"merge\", \"disable\", \"precedence\"]",
        );
        assert!(parse("test.toml", &merging).is_ok(), "{}", error(&merging));
        let text = merging.replace(
            r#"["merge", "disable", "precedence"]"#,
            r#"["disable", "precedence"]"#,
        );
        assert!(
            error(&text).contains("checks leaves out `merge`"),
            "{}",
            error(&text)
        );
        let text = VALID.replace(
            r#"checks = ["disable", "precedence"]"#,
            r#"checks = ["merge", "disable", "precedence"]"#,
        );
        assert!(
            error(&text).contains("the kind does not merge"),
            "{}",
            error(&text)
        );
        let text = merging.replace(r#"{ block = "exact-name" }"#, r#"{ block = "never" }"#);
        assert!(error(&text).contains("compete by name"), "{}", error(&text));
        let text = merging.replace(
            r#"exclusive = ["command", "url"]"#,
            r#"exclusive = ["command"]"#,
        );
        assert!(error(&text).contains("two or more"), "{}", error(&text));
    }

    #[test]
    fn the_locations_that_merge_rank_above_the_others() {
        let merging = |locations: &str| {
            with_projects(STATE, STATE, r#"["projects", "{project}", "servers"]"#).replace(
                r#"checks = ["disable", "precedence"]"#,
                &format!(
                    "merge = {{ block = \"deep-merge\", locations = {locations} }}\nchecks = [\"disable\", \"precedence\"]"
                ),
            )
        };
        let text = merging(r#"["local-mcp"]"#);
        assert!(parse("test.toml", &text).is_ok(), "{}", error(&text));
        let text = merging(r#"["user-mcp"]"#);
        assert!(error(&text).contains("rank above"), "{}", error(&text));
        let text = merging(r#"["other-mcp"]"#);
        assert!(
            error(&text).contains("`other-mcp` is not a location"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn a_location_of_toml_fragments_is_read_only() {
        let fragments = |provenance: &str| {
            VALID
                .replace(
                    r#"format = { block = "json", dialect = "strict" }"#,
                    "format = { block = \"toml-fragments\", lists = [[\"bundle\"]], contents = \"contents\" }\ncached_at = [\"cached_at\"]",
                )
                .replace(
                    r#"provenance = "file""#,
                    &format!("provenance = \"{provenance}\""),
                )
        };
        let text = fragments("managed");
        assert!(parse("test.toml", &text).is_ok(), "{}", error(&text));
        let text = fragments("file");
        assert!(error(&text).contains("read only"), "{}", error(&text));
    }

    #[test]
    fn only_a_file_location_says_when_it_was_cached() {
        let text = VALID
            .replace(
                r#"format = { block = "json", dialect = "strict" }"#,
                "format = { block = \"skill-folder\", name_from = \"folder\", no_frontmatter = \"skip\", malformed_frontmatter = \"skip\", no_name = \"skip\" }\ncached_at = [\"cached_at\"]",
            )
            .replace("root = [\"servers\"]\nkind = \"mcp\"", "kind = \"skill\"")
            .replace("[items.mcp]", "[items.skill]");
        assert!(
            error(&text).contains("only a file location takes cached_at"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn a_project_trust_gate_covers_locations_in_a_project() {
        let gated = |anchor: &str| {
            with_location(anchor, r#"{ block = "toml" }"#, r#"["servers"]"#).replace(
                r#"checks = ["disable", "precedence"]"#,
                r#"checks = ["trust", "disable", "precedence"]"#,
            ) + "\n[[items.mcp.gates]]\nblock = \"project-trust\"\nlocations = [\"user-mcp\"]\nsources = [{ anchor = { block = \"home\", root = \"state\" }, key_path = [\"projects\"], toml = true }]\nfield = \"trust_level\"\ntrusted = \"trusted\"\n"
        };
        let text =
            gated(r#"{ block = "project", path = ".test/config.toml", walk = "up-to-repo-root" }"#);
        assert!(parse("test.toml", &text).is_ok(), "{}", error(&text));
        let text = gated(STATE);
        assert!(
            error(&text).contains("`user-mcp` is not in a Project"),
            "{}",
            error(&text)
        );
        let text = gated(IN_PROJECT).replace("toml = true", "toml = true, dialect = \"jsonc\"");
        assert!(error(&text).contains("TOML setting"), "{}", error(&text));
    }

    #[test]
    fn a_sqlite_column_registry_names_its_table_and_column_plainly() {
        let registry = |table: &str| {
            VALID.replace(
                "[[locations]]",
                &format!(
                    "[projects]\nregistries = [{{ block = \"sqlite-column\", anchor = {STATE}, table = \"{table}\", column = \"cwd\" }}]\n\n[[locations]]"
                ),
            )
        };
        assert!(parse("test.toml", &registry("threads")).is_ok());
        let text = registry("threads; DROP TABLE threads");
        assert!(error(&text).contains("plain names"), "{}", error(&text));
    }

    #[test]
    fn a_machine_file_beside_another_needs_that_one_declared() {
        let with_files = |files: &str| format!("{VALID}\n{files}");
        let beside = r#"
[[machine_files]]
path = "%ProgramData%/x/settings.json"
variable = "X_SETTINGS"

[[machine_files]]
path = "%ProgramData%/x/defaults.json"
beside = "%ProgramData%/x/settings.json"
admin_owned = true
"#;
        let text = with_files(beside);
        assert!(parse("test.toml", &text).is_ok(), "{}", error(&text));
        let text = with_files(&beside.replace(
            r#"beside = "%ProgramData%/x/settings.json""#,
            r#"beside = "%ProgramData%/x/other.json""#,
        ));
        assert!(
            error(&text).contains("another machine file"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn a_shared_root_is_one_of_the_home_s_and_not_every_one() {
        let text = VALID.replace(
            "roots.state = \".test.json\"",
            "shared_roots = [\"skills\"]\nroots.state = \".test.json\"",
        );
        assert!(
            error(&text).contains("undeclared root `skills`"),
            "{}",
            error(&text)
        );
        let text = VALID.replace(
            "roots.state = \".test.json\"",
            "shared_roots = [\"state\"]\nroots.state = \".test.json\"",
        );
        assert!(
            error(&text).contains("shares every root"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn a_feature_gate_takes_a_lookup_or_a_when_or_neither() {
        let gate = |lookup: &str| {
            format!(
                "{VALID}\n[[feature_gates]]\nname = \"admin\"\nassumed = true\naffects = [\"user-mcp\"]\n{lookup}"
            )
        };
        let when = r#"when = { sources = [{ anchor = { block = "home", root = "state" }, key_path = ["auth"] }], one_of = ["google"] }"#;
        let text = gate(when);
        assert!(parse("test.toml", &text).is_ok(), "{}", error(&text));
        let text = gate("");
        assert!(parse("test.toml", &text).is_ok(), "{}", error(&text));
        let lookup = r#"lookup = { block = "json-value", anchor = { block = "home", root = "state" }, key_path = ["{gate}"] }"#;
        let text = gate(&format!("{when}\n{lookup}"));
        assert!(
            error(&text).contains("a lookup or a `when`, not both"),
            "{}",
            error(&text)
        );
        let text = gate(&when.replace(r#"["google"]"#, "[]"));
        assert!(
            error(&text).contains("one_of needs a value"),
            "{}",
            error(&text)
        );
    }
}
