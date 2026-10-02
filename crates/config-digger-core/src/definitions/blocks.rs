//! The building blocks Agent definitions are assembled from (ADR 0005).
//!
//! Each family is a closed enum tagged by `block`. Every variant wraps a
//! parameter struct that implements [`Validate`] and its family's trait, so a
//! new block is added in one place and the compiler points at every `match`
//! that must handle it.

use crate::ports::Environment;
use schemars::JsonSchema;
use serde::Deserialize;
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

// --- Formats ----------------------------------------------------------------

/// Why a format could not read a location's text.
#[derive(Debug)]
pub(crate) struct ParseError;

/// Parses a location's text in the reader's dialect.
pub(crate) trait Parse {
    fn parse(&self, text: &str) -> Result<serde_json::Value, ParseError>;
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
        serde_json::from_str(text).map_err(|_| ParseError)
    }
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
