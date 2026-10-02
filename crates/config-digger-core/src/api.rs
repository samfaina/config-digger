//! The public surface shared by the app and the CLI: [`Core`] and the serde
//! DTOs it returns. Domain types stay internal. The DTOs also derive
//! `specta::Type`, from which the app's TypeScript bindings are generated.

use crate::definitions::blocks::Provenance as DefinitionProvenance;
use crate::definitions::{self, AgentDefinition, ItemKind as DefinitionItemKind};
use crate::ports::Ports;
use crate::{resolve, scan, select};
use serde::Serialize;
use std::path::PathBuf;

/// The version of every JSON document config-digger emits.
pub const SCHEMA_VERSION: u32 = 1;

/// The core, holding the ports and the loaded Agent definitions.
pub struct Core {
    ports: Ports,
    definitions: Vec<AgentDefinition>,
}

/// What [`Core::scan`] read. Resolve it as often as needed without reading the disk again.
pub struct Snapshot(scan::Snapshot);

/// What to read besides what the Agents' registries record.
#[derive(Debug, Clone, Default)]
pub struct ScanOptions {
    /// Projects to evaluate even when no registry knows them, in any spelling.
    pub projects: Vec<PathBuf>,
}

/// The context Entries are resolved for: every Agent home, with one Project or none.
#[derive(Debug, Clone, Default)]
pub struct Context {
    /// The Project in any spelling, or `None` for no Project. It names the same
    /// Project as any spelling of the same folder that the scan saw, including
    /// a link to it; otherwise it is compared lexically.
    pub project: Option<PathBuf>,
}

impl Core {
    /// Loads the built-in Agent definitions.
    pub fn new(ports: Ports) -> Result<Self, Error> {
        let definitions = definitions::built_in().map_err(Error::unexpected)?;
        Ok(Self { ports, definitions })
    }

    /// Reads every location of every found Agent home and Project.
    pub fn scan(&self, options: &ScanOptions) -> Result<Snapshot, Error> {
        scan::scan(&self.definitions, &self.ports, &options.projects)
            .map(Snapshot)
            .map_err(Error::unexpected)
    }

    /// The Items of every Agent home and each Entry's State, for `context`.
    /// It never reads the disk.
    pub fn resolve(&self, snapshot: &Snapshot, context: &Context) -> Inventory {
        resolve::resolve(&self.definitions, &snapshot.0, context)
    }

    /// The one Item `selector` names in `context`: an Item name, or the
    /// `<path>#<key path>` locator of any of its Entries.
    pub fn show(
        &self,
        snapshot: &Snapshot,
        context: &Context,
        selector: &str,
    ) -> Result<Item, Error> {
        self.resolve(snapshot, context)
            .select_one(selector)
            .cloned()
    }
}

/// Every Item found, with the Agent homes they belong to.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct Inventory {
    /// When the snapshot was read, in RFC 3339.
    pub read_at: String,
    /// The context's Project, as found by the scan, or `None` for no Project.
    pub project: Option<PathBuf>,
    /// Every Project the scan found or was asked for, sorted by path.
    pub projects: Vec<Project>,
    pub homes: Vec<Home>,
    /// Sorted by Agent, home, Item kind and name.
    pub items: Vec<Item>,
    pub read_failures: Vec<ReadFailure>,
}

impl Inventory {
    /// The Items a selector names: those with an Entry at a `<path>#<key
    /// path>` locator, or else those with that name.
    pub fn select(&self, selector: &str) -> Vec<&Item> {
        select::select(self, selector)
    }

    /// The one Item a selector names, as [`Core::show`] finds it.
    pub fn select_one(&self, selector: &str) -> Result<&Item, Error> {
        match self.select(selector).as_slice() {
            [item] => Ok(item),
            [] => Err(Error::no_match(selector)),
            items => Err(Error::ambiguous(selector, items)),
        }
    }
}

/// What an Agent can be started on.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct Project {
    /// The folder's real path, or its lexical normalisation when it is missing.
    pub path: PathBuf,
    pub missing: bool,
    /// The Agents whose registries record it. Empty when it was only asked for.
    pub registered_by: Vec<String>,
}

/// An Agent home found on this machine.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct Home {
    pub agent: String,
    pub agent_name: String,
    pub home: String,
    /// The Agent's Scopes, in display order.
    pub scopes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct Item {
    pub agent: String,
    pub home: String,
    pub kind: ItemKind,
    pub name: String,
    pub state: State,
    /// In precedence order, highest first.
    pub entries: Vec<Entry>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    Mcp,
}

impl From<DefinitionItemKind> for ItemKind {
    fn from(kind: DefinitionItemKind) -> Self {
        match kind {
            DefinitionItemKind::Mcp => ItemKind::Mcp,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct Entry {
    pub path: PathBuf,
    pub key_path: Vec<String>,
    /// `<path>#<key path>`.
    pub locator: String,
    pub scope: String,
    pub provenance: Provenance,
    pub state: State,
    /// Why the Entry has its State, when it is not Effective.
    pub reason: Option<Reason>,
}

/// How an Entry got where it is, which decides how it can be taken away.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Provenance {
    /// A key in a configuration file.
    File,
}

impl From<DefinitionProvenance> for Provenance {
    fn from(provenance: DefinitionProvenance) -> Self {
        match provenance {
            DefinitionProvenance::File => Provenance::File,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Effective,
    Shadowed,
}

/// Why an Entry has its State.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct Reason {
    pub code: ReasonCode,
    /// The locator of the Effective Entry that beats this one, if one does.
    pub effective: Option<String>,
    /// The reason as a sentence, rendered by the core.
    pub sentence: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ReasonCode {
    /// A same-named Entry at a location the Agent ranks higher is Effective.
    ShadowedByPrecedence,
}

/// A location config-digger could not turn into Entries for one Agent.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct ReadFailure {
    pub agent: String,
    pub agent_name: String,
    pub home: String,
    pub path: PathBuf,
    pub kind: ReadFailureKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ReadFailureKind {
    Parse,
    Decode,
    Permission,
    Io,
}

impl ReadFailureKind {
    /// The kind as it appears in JSON.
    pub fn code(self) -> &'static str {
        match self {
            ReadFailureKind::Parse => "parse",
            ReadFailureKind::Decode => "decode",
            ReadFailureKind::Permission => "permission",
            ReadFailureKind::Io => "io",
        }
    }
}

/// A failure of the call itself, as opposed to a Read failure.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type, thiserror::Error)]
#[error("{message}")]
pub struct Error {
    pub code: ErrorCode,
    pub message: String,
    /// What an ambiguous selector could mean, one locator per Item.
    pub candidates: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    Unexpected,
    /// A selector matched nothing in the context.
    NoMatch,
    /// A selector matched more than one Item.
    Ambiguous,
}

impl Error {
    /// A failure the caller cannot act on, such as a bug or a lost worker thread.
    pub fn unexpected(error: impl std::fmt::Display) -> Self {
        Self {
            code: ErrorCode::Unexpected,
            message: error.to_string(),
            candidates: Vec::new(),
        }
    }

    /// A selector that matched no Item in the context.
    pub fn no_match(selector: &str) -> Self {
        Self {
            code: ErrorCode::NoMatch,
            message: format!("no Item matches `{selector}` in this context"),
            candidates: Vec::new(),
        }
    }

    /// Each candidate is the locator of an Item's first Entry.
    pub(crate) fn ambiguous(selector: &str, items: &[&Item]) -> Self {
        Self {
            code: ErrorCode::Ambiguous,
            message: format!(
                "`{selector}` matches {} Items; select one by locator",
                items.len()
            ),
            candidates: items
                .iter()
                .map(|item| item.entries[0].locator.clone())
                .collect(),
        }
    }
}
