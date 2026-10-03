//! Agent definitions: TOML data over the closed set of building blocks in
//! [`blocks`]. Deserialisation plus `validate()` is the only validation path.

pub(crate) mod blocks;

use crate::version::{Standing, Version};
use blocks::{
    Anchor, Anchored, Competition, DisableMechanism, Disables, Format, Interpolation, Locator,
    ProjectWalk, Provenance, Register, Registry, SessionFiles, Shape, StoreClass, Validate,
    VersionSource,
};
use schemars::JsonSchema;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// The definitions that ship inside config-digger, embedded at compile time.
const BUILT_IN: &[(&str, &str)] = &[(
    "claude-code.toml",
    include_str!("../../definitions/claude-code.toml"),
)];

/// The only `format_version` this release reads.
const FORMAT_VERSION: u32 = 1;

/// The JSON Schema of an Agent definition file, generated from the block enums.
pub fn json_schema() -> String {
    let schema = schemars::schema_for!(AgentDefinition);
    let mut json = serde_json::to_string_pretty(&schema).expect("a schema serialises");
    json.push('\n');
    json
}

/// A definition file that failed to load. It is rejected whole.
#[derive(Debug, thiserror::Error)]
#[error("Agent definition `{file}` is invalid: {message}")]
pub(crate) struct DefinitionError {
    pub file: String,
    pub message: String,
}

/// The built-in Agent definitions, in load order.
pub(crate) fn built_in() -> Result<Vec<AgentDefinition>, DefinitionError> {
    BUILT_IN
        .iter()
        .map(|(file, text)| parse(file, text))
        .collect()
}

pub(crate) fn parse(file: &str, text: &str) -> Result<AgentDefinition, DefinitionError> {
    let error = |message: String| DefinitionError {
        file: file.to_owned(),
        message,
    };
    let definition: AgentDefinition = toml::from_str(text).map_err(|e| error(e.to_string()))?;
    definition.validate().map_err(error)?;
    Ok(definition)
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
    /// The Scopes this Agent applies Entries at, in display order. A Scope is
    /// only a label: precedence comes from the order of locations in `items`.
    pub scopes: Vec<ScopeDefinition>,
    /// Where this Agent records the Projects it has been started on.
    #[serde(default)]
    pub projects: ProjectsDefinition,
    /// The configuration locations this Agent reads.
    pub locations: Vec<LocationDefinition>,
    /// Rules per Item kind.
    pub items: BTreeMap<ItemKind, ItemRules>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct AgentInfo {
    /// A stable id, such as `claude-code`.
    pub id: String,
    /// The display name, such as `Claude Code`.
    pub name: String,
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
    /// How to tell that the Agent of this home is running, in place of the
    /// definition's `[process]`, such as for a home read by another
    /// Installation of the Agent.
    pub process: Option<ProcessDefinition>,
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
}

/// The `root` segment that stands for any Project path key.
pub(crate) const PROJECT_KEY: &str = "{project}";

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct LocationDefinition {
    pub id: String,
    pub anchor: Anchor,
    pub format: Format,
    /// The key path of the object whose keys are Entries. One segment may be
    /// `{project}`: each key there is a Project path, and the Entries below it
    /// apply only in that Project. Omitted for a folder of skill folders.
    #[serde(default)]
    pub root: Vec<String>,
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
    /// do, beyond what the core masks by default.
    #[serde(default)]
    pub secrets: SecretFields,
    /// What the Agent does when it cannot open, decode or parse the location.
    pub on_read_failure: ReadFailureReaction,
    /// How the Agent's own recovery from a Read failure here can lose
    /// configuration, shown as warnings on the Read failure.
    #[serde(default)]
    pub risks: Vec<RiskCode>,
    /// The minimal shape the Agent requires of the location's Entries:
    /// one without it is Invalid. Without it, no Entry here is Invalid.
    pub shape: Option<Shape>,
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
    /// Key paths from the root of the file, outside any Entry, whose values
    /// are Secrets, such as an API key stored next to the Entries. They are
    /// masked wherever the file's text is shown, as in a diff's context lines.
    #[serde(default)]
    pub file_fields: Vec<Vec<String>>,
}

impl SecretFields {
    /// Whether it names no field at all.
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty() && self.variable_names.is_empty() && self.file_fields.is_empty()
    }
}

impl Validate for SecretFields {
    fn validate(&self) -> Result<(), String> {
        for field in self
            .fields
            .iter()
            .chain(&self.variable_names)
            .chain(&self.file_fields)
        {
            if field.is_empty() || field.iter().any(String::is_empty) {
                return Err(format!(
                    "secret field {field:?} needs keys, none of them empty"
                ));
            }
        }
        Ok(())
    }
}

impl LocationDefinition {
    /// Whether the location's Entries are keyed by Project inside the file.
    pub fn keyed_by_project(&self) -> bool {
        self.root.iter().any(|key| key == PROJECT_KEY)
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
}

/// One of an Agent's checks that decides an Entry's State.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Check {
    /// A Disable mechanism's switch is set for the Entry: Disabled.
    Disable,
    /// A same-named Entry the Agent ranks higher is not Disabled: Shadowed.
    Precedence,
    /// The Entry lacks the minimal shape its location declares: Invalid.
    /// Required when a location of the kind declares a shape, and refused
    /// otherwise.
    Shape,
    /// The Agent skips the skill because of its `SKILL.md` or its folder's
    /// name: Invalid. Required when a location of the kind is a folder of
    /// skill folders, and refused otherwise.
    Frontmatter,
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
        unique("home", self.homes.iter().map(|home| home.id.as_str()))?;
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
            if let Some(process) = &home.process {
                process
                    .validate()
                    .map_err(|e| format!("home `{}` process: {e}", home.id))?;
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

        for location in &self.locations {
            let context = |e: String| format!("location `{}`: {e}", location.id);
            location.anchor.validate().map_err(context)?;
            location.format.validate().map_err(context)?;
            if let Some(interpolation) = &location.interpolation {
                interpolation.validate().map_err(context)?;
            }
            location.secrets.validate().map_err(context)?;
            if let Some(shape) = &location.shape {
                shape.validate().map_err(context)?;
            }
            self.validate_home_root(&location.anchor).map_err(context)?;
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
            match (
                rules
                    .checks
                    .iter()
                    .filter(|known| **known == Check::Shape)
                    .count(),
                shaped,
            ) {
                (1, true) | (0, false) => {}
                (0, true) => {
                    return Err(context(
                        "checks leaves out `shape`, which a location declares".into(),
                    ));
                }
                (_, true) => return Err(context("checks holds `shape` twice".into())),
                (_, false) => {
                    return Err(context(
                        "checks holds `shape`, but no location declares one".into(),
                    ));
                }
            }
            let folders = self
                .locations
                .iter()
                .any(|location| location.kind == *kind && location.format.skill_folder().is_some());
            match (
                rules
                    .checks
                    .iter()
                    .filter(|known| **known == Check::Frontmatter)
                    .count(),
                folders,
            ) {
                (1, true) | (0, false) => {}
                (0, true) => {
                    return Err(context(
                        "checks leaves out `frontmatter`, which a folder of skill folders needs"
                            .into(),
                    ));
                }
                (_, true) => return Err(context("checks holds `frontmatter` twice".into())),
                (_, false) => {
                    return Err(context(
                        "checks holds `frontmatter`, but no location is a folder of skill folders"
                            .into(),
                    ));
                }
            }
            for (index, mechanism) in rules.disable.iter().enumerate() {
                let context = |e: String| context(format!("disable[{index}]: {e}"));
                mechanism.validate().map_err(context)?;
                self.validate_home_root(&mechanism.switch().anchor)
                    .map_err(context)?;
                if let Some(id) = mechanism
                    .locations()
                    .iter()
                    .find(|id| !declared.contains(id.as_str()))
                {
                    return Err(context(format!("`{id}` is not a location of this kind")));
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
}

impl AgentDefinition {
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

impl Check {
    fn id(self) -> &'static str {
        match self {
            Check::Disable => "disable",
            Check::Precedence => "precedence",
            Check::Shape => "shape",
            Check::Frontmatter => "frontmatter",
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
        assert_eq!(ids, ["claude-code"]);
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
    fn a_home_folder_outside_the_user_folder_is_rejected() {
        let text = VALID.replace(r#"path = "~" }"#, r#"path = "C:/test" }"#);
        assert!(
            error(&text).contains("must be `~` or start with `~/`"),
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
    fn a_machine_path_starts_from_a_variable_or_a_drive_without_wildcards() {
        for (path, message) in [
            ("~/skills", "`%NAME%` or a drive"),
            ("skills", "must start with"),
            ("%ProgramFiles%/*/skills", "wildcards"),
        ] {
            let anchor = format!(r#"{{ block = "machine", path = "{path}" }}"#);
            let text = with_skills(&anchor, SKILL_FORMAT, CHECKS);
            assert!(error(&text).contains(message), "{path}: {}", error(&text));
        }
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
}
