//! The building blocks Agent definitions are assembled from (ADR 0005).
//!
//! Each family is a closed enum tagged by `block`. Every variant wraps a
//! parameter struct that implements [`Validate`] and its family's trait, so a
//! new block is added in one place and the compiler points at every `match`
//! that must handle it.

use crate::edit::{Edit, Syntax};
use crate::ports::{Environment, FileSystem};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::{Component, Path, PathBuf};

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

/// A fixed path below the user's home folder, written `~/<relative path>`.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct PathLocator {
    pub path: String,
}

impl PathLocator {
    fn relative(&self) -> Option<&str> {
        self.path.strip_prefix("~/")
    }
}

impl Validate for PathLocator {
    fn validate(&self) -> Result<(), String> {
        match self.relative() {
            Some(relative) => validate_relative(relative),
            None => Err(format!("path `{}` must start with `~/`", self.path)),
        }
    }
}

impl Locate for PathLocator {
    fn locate(&self, env: &dyn Environment) -> Option<PathBuf> {
        let relative = self.relative()?;
        Some(join_relative(&env.home_dir()?, relative))
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
}

/// What a location's path is relative to.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "block", rename_all = "kebab-case")]
pub(crate) enum Anchor {
    Home(HomeAnchor),
    Project(ProjectAnchor),
}

impl Validate for Anchor {
    fn validate(&self) -> Result<(), String> {
        match self {
            Anchor::Home(block) => block.validate(),
            Anchor::Project(block) => block.validate(),
        }
    }
}

impl Anchored for Anchor {
    fn path(&self, roots: &BTreeMap<String, PathBuf>, project: Option<&Path>) -> Option<PathBuf> {
        match self {
            Anchor::Home(block) => block.path(roots, project),
            Anchor::Project(block) => block.path(roots, project),
        }
    }

    fn home_root(&self) -> Option<&str> {
        match self {
            Anchor::Home(block) => block.home_root(),
            Anchor::Project(block) => block.home_root(),
        }
    }

    fn in_project(&self) -> bool {
        match self {
            Anchor::Home(block) => block.in_project(),
            Anchor::Project(block) => block.in_project(),
        }
    }
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
}

/// A path in the Project folder itself.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProjectAnchor {
    /// A `/`-separated path relative to the Project folder.
    pub path: String,
}

impl Validate for ProjectAnchor {
    fn validate(&self) -> Result<(), String> {
        validate_relative(&self.path)
    }
}

impl Anchored for ProjectAnchor {
    fn path(&self, _roots: &BTreeMap<String, PathBuf>, project: Option<&Path>) -> Option<PathBuf> {
        Some(join_relative(project?, &self.path))
    }

    fn home_root(&self) -> Option<&str> {
        None
    }

    fn in_project(&self) -> bool {
        true
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
        if self.anchor.in_project() {
            return Err("a project registry must not lie in a Project".into());
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
        let document = JsonFormat {}.parse(text)?;
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
    /// A key someone wrote into a configuration file, by hand or through the Agent.
    File,
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

/// The file format and dialect a location is read in.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "block", rename_all = "kebab-case")]
pub(crate) enum Format {
    Json(JsonFormat),
}

impl Validate for Format {
    fn validate(&self) -> Result<(), String> {
        match self {
            Format::Json(block) => block.validate(),
        }
    }
}

impl Parse for Format {
    fn parse(&self, text: &str) -> Result<serde_json::Value, ParseError> {
        match self {
            Format::Json(block) => block.parse(text),
        }
    }

    fn dialect(&self) -> &'static str {
        match self {
            Format::Json(block) => block.dialect(),
        }
    }

    fn syntax(&self) -> Syntax {
        match self {
            Format::Json(block) => block.syntax(),
        }
    }
}

/// Strict JSON: no comments, no trailing commas.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct JsonFormat {}

impl Validate for JsonFormat {
    fn validate(&self) -> Result<(), String> {
        Ok(())
    }
}

impl Parse for JsonFormat {
    fn parse(&self, text: &str) -> Result<serde_json::Value, ParseError> {
        serde_json::from_str(text).map_err(|error| ParseError {
            position: (error.line() > 0).then(|| (error.line(), error.column())),
        })
    }

    fn dialect(&self) -> &'static str {
        "strict JSON"
    }

    /// Strict JSON is edited as JSONC, which accepts it as it is.
    fn syntax(&self) -> Syntax {
        Syntax::Jsonc
    }
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

// --- Paths ------------------------------------------------------------------

/// A `/`-separated path must stay below the folder it is relative to.
fn validate_relative(path: &str) -> Result<(), String> {
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
fn join_relative(base: &Path, relative: &str) -> PathBuf {
    relative
        .split('/')
        .fold(base.to_path_buf(), |path, part| path.join(part))
}
