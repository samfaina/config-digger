//! The public surface shared by the app and the CLI: [`Core`] and the serde
//! DTOs it returns. Domain types stay internal. The DTOs also derive
//! `specta::Type`, from which the app's TypeScript bindings are generated.

use crate::definitions::{self, AgentDefinition, ItemKind as DefinitionItemKind};
use crate::ports::Ports;
use crate::{resolve, scan};
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

/// The context Entries are resolved for.
#[derive(Debug, Clone, Default)]
pub struct Context {
    /// The Project, or none. User-Scope Entries apply in every Project, so the
    /// current locations do not depend on it yet.
    pub project: Option<PathBuf>,
}

impl Core {
    /// Loads the built-in Agent definitions.
    pub fn new(ports: Ports) -> Result<Self, Error> {
        let definitions = definitions::built_in().map_err(Error::unexpected)?;
        Ok(Self { ports, definitions })
    }

    /// Reads every location of every found Agent home.
    pub fn scan(&self) -> Result<Snapshot, Error> {
        scan::scan(&self.definitions, &self.ports)
            .map(Snapshot)
            .map_err(Error::unexpected)
    }

    /// The Items of every Agent home and each Entry's State, for `context`.
    pub fn resolve(&self, snapshot: &Snapshot, context: &Context) -> Inventory {
        resolve::resolve(&self.definitions, &snapshot.0, context)
    }
}

/// Every Item found, with the Agent homes they belong to.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct Inventory {
    /// When the snapshot was read, in RFC 3339.
    pub read_at: String,
    pub homes: Vec<Home>,
    /// Sorted by Agent, home, Item kind and name.
    pub items: Vec<Item>,
    pub read_failures: Vec<ReadFailure>,
}

/// An Agent home found on this machine.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct Home {
    pub agent: String,
    pub agent_name: String,
    pub home: String,
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
    pub state: State,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Effective,
    Shadowed,
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    Unexpected,
}

impl Error {
    /// A failure the caller cannot act on, such as a bug or a lost worker thread.
    pub fn unexpected(error: impl std::fmt::Display) -> Self {
        Self {
            code: ErrorCode::Unexpected,
            message: error.to_string(),
        }
    }
}
