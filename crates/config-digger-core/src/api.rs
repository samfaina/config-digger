//! The public surface shared by the app and the CLI: [`Core`] and the serde
//! DTOs it returns. Domain types stay internal. The DTOs also derive
//! `specta::Type`, from which the app's TypeScript bindings are generated.

use crate::definitions::blocks::Provenance as DefinitionProvenance;
use crate::definitions::{self, AgentDefinition, ItemKind as DefinitionItemKind};
use crate::edit::Edit;
use crate::ports::Ports;
use crate::{execute, history, plan, resolve, scan, select};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// The version of every JSON document config-digger emits.
pub const SCHEMA_VERSION: u32 = 1;

/// The core, holding the ports and the loaded Agent definitions.
pub struct Core {
    ports: Ports,
    definitions: Vec<AgentDefinition>,
}

/// What [`Core::scan`] read. Resolve it as often as needed without reading the disk again.
pub struct Snapshot {
    scanned: scan::Snapshot,
    /// What it was scanned with, so that Verify can scan the same way again.
    options: ScanOptions,
}

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
        let scanned = scan::scan(&self.definitions, &self.ports, &options.projects)
            .map_err(Error::unexpected)?;
        Ok(Snapshot {
            scanned,
            options: options.clone(),
        })
    }

    /// The Items of every Agent home and each Entry's State, for `context`.
    /// It never reads the disk.
    pub fn resolve(&self, snapshot: &Snapshot, context: &Context) -> Inventory {
        resolve::resolve(&self.definitions, &snapshot.scanned, context)
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

    /// Plans `action` on the Entries `targets` names in `context`, without
    /// writing anything. Several selectors form one Operation, and it fails
    /// whole when any of them does.
    pub fn plan(
        &self,
        snapshot: &Snapshot,
        context: &Context,
        action: Action,
        targets: &Targets,
    ) -> Result<Plan, Error> {
        plan::plan(
            &self.definitions,
            &self.ports,
            &snapshot.scanned,
            &snapshot.options,
            context,
            action,
            targets,
        )
        .map(Plan)
    }

    /// Executes a plan once every warning in it is acknowledged: takes the
    /// global lock, backs up each touched file, writes, verifies and records
    /// the Operation in History.
    pub fn execute(&self, plan: Plan, acknowledged: &[WarningClass]) -> Result<Outcome, Error> {
        execute::execute(&self.definitions, &self.ports, plan.0, acknowledged)
    }

    /// Plans the Undo of the Operation `id`: the inverse of each of its
    /// edits, applied to the files as they are now, in the context it ran
    /// in. It is refused when what the Operation touched changed since.
    pub fn plan_undo(&self, id: &str) -> Result<Plan, Error> {
        plan::plan_undo(&self.definitions, &self.ports, id).map(Plan)
    }

    /// Every Operation in History, newest first, across restarts.
    pub fn history(&self) -> Result<Vec<OperationSummary>, Error> {
        Ok(history::summaries(&history::load_all(&self.ports)?))
    }

    /// Prunes History, as the front-ends do at startup: it keeps at least the
    /// last 50 Operations and every one from the last 30 days, never prunes
    /// one that needs attention, and removes an Operation with its backup.
    /// Returns the ids pruned, oldest first. It fails with `Locked` while
    /// another Operation holds the global lock.
    pub fn prune_history(&self) -> Result<Vec<String>, Error> {
        history::prune(&self.ports)
    }

    /// One Operation in History, with the edits it made and the files it
    /// copied to its backup.
    pub fn history_show(&self, id: &str) -> Result<OperationDetail, Error> {
        history::detail(&self.ports, id)
    }
}

/// What an Operation does to the Entries it targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// Deletes each Entry from where it lives, for every Agent that reads it.
    Remove,
    /// Applies the inverse of every edit of an earlier Operation to the files
    /// as they are now.
    Undo,
}

/// The Entries an action targets.
#[derive(Debug, Clone, Default)]
pub struct Targets {
    /// Each one an Item name, or the `<path>#<key path>` locator of an Entry.
    pub selectors: Vec<String>,
    /// Each selector stands for every Entry of the one Item it names, instead
    /// of the one Entry it names.
    pub all: bool,
}

/// An Operation planned and not yet executed. It never leaves the process:
/// front-ends show its preview and warnings, then hand it to [`Core::execute`].
pub struct Plan(pub(crate) plan::Planned);

impl Plan {
    pub fn preview(&self) -> &Preview {
        &self.0.preview
    }

    /// What the user must acknowledge, each on its own, before it executes.
    pub fn warnings(&self) -> &[Warning] {
        &self.0.warnings
    }
}

/// What a plan will change, shown before it is confirmed.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct Preview {
    pub action: Action,
    /// What the Operation does, as a title such as "Remove 2 Entries".
    pub title: String,
    /// The Operation this one undoes, when it is an Undo.
    pub undo_of: Option<String>,
    /// The context's Project, as in [`Inventory::project`].
    pub project: Option<PathBuf>,
    /// One row per (Agent, Entry, Project) whose State the Operation changes.
    pub changes: Vec<Change>,
    /// For each Item that keeps Entries, which one is Effective afterwards.
    pub effective: Vec<EffectiveChange>,
    pub files: Vec<FileChange>,
    /// Where each touched file is copied before anything is written.
    pub backup_folder: PathBuf,
    /// The same, as a sentence.
    pub backup_note: String,
}

/// An Entry's State in one Agent and context, before and after an Operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct Change {
    pub agent: String,
    pub agent_name: String,
    pub home: String,
    /// The Item's name.
    pub item: String,
    pub path: PathBuf,
    pub locator: String,
    pub project: Option<PathBuf>,
    pub before: StateAfter,
    pub after: StateAfter,
}

/// An Entry's State before or after an Operation, or that it does not exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum StateAfter {
    Effective,
    Shadowed,
    Removed,
}

impl From<State> for StateAfter {
    fn from(state: State) -> Self {
        match state {
            State::Effective => StateAfter::Effective,
            State::Shadowed => StateAfter::Shadowed,
        }
    }
}

/// Which Entry of an Item is Effective after an Operation that leaves some of
/// its Entries in place.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct EffectiveChange {
    pub agent: String,
    pub home: String,
    pub item: String,
    /// The locator of the Effective Entry before.
    pub before: String,
    /// The locator of the Effective Entry after.
    pub after: String,
    /// The change as a sentence, rendered by the core.
    pub sentence: String,
}

/// The edits to one file and their diff.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct FileChange {
    pub path: PathBuf,
    pub edits: Vec<EditSummary>,
    /// A unified diff of the whole file, before and after every edit.
    pub diff: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct EditSummary {
    pub op: EditOp,
    pub key_path: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum EditOp {
    Remove,
    Set,
    Insert,
    Append,
    RemoveItem,
}

impl From<&Edit> for EditSummary {
    fn from(edit: &Edit) -> Self {
        let op = match edit {
            Edit::Remove { .. } => EditOp::Remove,
            Edit::Set { .. } => EditOp::Set,
            Edit::Insert { .. } => EditOp::Insert,
            Edit::Append { .. } => EditOp::Append,
            Edit::RemoveItem { .. } => EditOp::RemoveItem,
        };
        EditSummary {
            op,
            key_path: edit.key_path().to_vec(),
        }
    }
}

/// A risk the user must acknowledge before a plan executes.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct Warning {
    pub class: WarningClass,
    /// The risk as a sentence, rendered by the core.
    pub sentence: String,
}

/// Each class of warning is acknowledged on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum WarningClass {
    /// The write lands in a git-versioned file.
    GitVersioned,
    /// The write reaches more than the targeted Entries.
    WideReach,
    /// An Agent that reads or rewrites the file is running.
    RunningAgent,
}

impl WarningClass {
    /// The class as it appears in JSON and in `--ack`.
    pub fn code(self) -> &'static str {
        match self {
            WarningClass::GitVersioned => "git-versioned",
            WarningClass::WideReach => "wide-reach",
            WarningClass::RunningAgent => "running-agent",
        }
    }
}

/// An executed Operation.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct Outcome {
    /// The Operation's id in History, such as `20261001-143012-a3f`.
    pub operation: String,
    pub verify: Verify,
    /// Where the States after differ from the preview's prediction.
    pub differences: Vec<Difference>,
    /// The folder holding the Operation's backup and manifest.
    pub backup: PathBuf,
    /// What the Operation did, as a sentence rendered by the core.
    pub sentence: String,
}

/// What Verify found once every edit had landed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Verify {
    /// Every State after is the one the preview predicted.
    Verified,
    /// Some States after differ from the prediction, as listed.
    VerifiedWithDifferences,
}

/// An Entry whose State after an Operation is not the one predicted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct Difference {
    pub agent: String,
    pub home: String,
    pub locator: String,
    pub predicted: StateAfter,
    pub actual: StateAfter,
}

/// Where an Operation got to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum OperationStatus {
    /// Backed up and being written, or interrupted while it was.
    InProgress,
    Applied,
    /// Failed before any write.
    Failed,
    /// Failed after some writes; the backup is needed to recover.
    NeedsAttention,
}

/// One Operation as History lists it.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct OperationSummary {
    /// Such as `20261001-143012-a3f`.
    pub id: String,
    /// When it started, in RFC 3339.
    pub created_at: String,
    pub action: Action,
    /// What it does, as its preview titled it.
    pub title: String,
    pub status: OperationStatus,
    /// The status as History shows it, such as "Applied", rendered by the core.
    pub status_text: String,
    /// The Operation this one undoes, when it is an Undo.
    pub undo_of: Option<String>,
    /// The Undo that reverted this Operation, while that Undo is in effect.
    pub undone_by: Option<String>,
    /// The names of the Agents whose States it changed.
    pub agents: Vec<String>,
    /// The files it touched.
    pub locations: Vec<PathBuf>,
    pub undoable: bool,
    /// Why it cannot be undone, as a sentence, when it cannot.
    pub not_undoable: Option<String>,
}

/// One Operation as `history show` details it.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct OperationDetail {
    pub summary: OperationSummary,
    /// The context's Project, as in [`Preview::project`].
    pub project: Option<PathBuf>,
    /// The States its preview predicted.
    pub changes: Vec<Change>,
    pub locations: Vec<OperationLocation>,
    /// The folder holding its backup and manifest.
    pub backup: PathBuf,
    /// Why it did not complete.
    pub cause: Option<String>,
    pub verify: Option<Verify>,
    /// Where the States after differed from the prediction.
    pub differences: Vec<Difference>,
}

/// One file an Operation touched.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct OperationLocation {
    pub path: PathBuf,
    /// Its copy as it was before the Operation, or `None` when the Operation
    /// created the file.
    pub backup: Option<PathBuf>,
    pub edits: Vec<EditSummary>,
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
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
    /// What an ambiguous selector could mean, one locator per Item or Entry.
    pub candidates: Vec<String>,
    /// The backup folder of an Operation that needs manual recovery, or of
    /// one whose Undo was refused.
    pub backup: Option<PathBuf>,
    /// For a refused Undo, a unified diff from each backed-up file to what
    /// it holds now.
    pub diff: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    Unexpected,
    /// A selector matched nothing in the context.
    NoMatch,
    /// A selector matched more than one Item, or more than one Entry.
    Ambiguous,
    /// A warning in the plan was not acknowledged.
    AckRequired,
    /// The action cannot be done on a target, whatever is acknowledged.
    Blocked,
    /// Another Operation holds the global write lock.
    Locked,
    /// A touched Entry changed after it was read: plan again.
    Stale,
    /// A backup copy does not hold the bytes read; nothing was written.
    BackupFailed,
    /// The first write failed; nothing was written.
    WriteFailed,
    /// Some files were written and the Operation did not complete, so it
    /// needs manual recovery from its backup.
    NeedsRecovery,
    /// What the Operation to undo touched changed since it ran.
    UndoRefused,
}

impl Error {
    fn new(code: ErrorCode, message: String) -> Self {
        Self {
            code,
            message,
            candidates: Vec::new(),
            backup: None,
            diff: None,
        }
    }

    /// A failure the caller cannot act on, such as a bug or a lost worker thread.
    pub fn unexpected(error: impl std::fmt::Display) -> Self {
        Self::new(ErrorCode::Unexpected, error.to_string())
    }

    /// A selector that matched no Item in the context.
    pub fn no_match(selector: &str) -> Self {
        Self::new(
            ErrorCode::NoMatch,
            format!("no Item matches `{selector}` in this context"),
        )
    }

    /// Each candidate is the locator of an Item's first Entry.
    pub(crate) fn ambiguous(selector: &str, items: &[&Item]) -> Self {
        Self {
            candidates: items
                .iter()
                .map(|item| item.entries[0].locator.clone())
                .collect(),
            ..Self::new(
                ErrorCode::Ambiguous,
                format!(
                    "`{selector}` matches {} Items; select one by locator",
                    items.len()
                ),
            )
        }
    }

    /// Each candidate is the locator of an Entry.
    pub(crate) fn ambiguous_entries(selector: &str, entries: &[&Entry]) -> Self {
        Self {
            candidates: entries.iter().map(|entry| entry.locator.clone()).collect(),
            ..Self::new(
                ErrorCode::Ambiguous,
                format!(
                    "`{selector}` matches {} Entries; select one by locator, or every Entry of the Item",
                    entries.len()
                ),
            )
        }
    }

    pub(crate) fn ack_required(missing: &[WarningClass]) -> Self {
        let classes: Vec<&str> = missing.iter().map(|class| class.code()).collect();
        Self::new(
            ErrorCode::AckRequired,
            format!("not acknowledged: {}", classes.join(", ")),
        )
    }

    pub(crate) fn blocked(message: String) -> Self {
        Self::new(ErrorCode::Blocked, message)
    }

    pub(crate) fn locked(lock: &std::path::Path) -> Self {
        Self::new(
            ErrorCode::Locked,
            format!(
                "another config-digger Operation is writing (lock held on {}); try again when it ends",
                lock.display()
            ),
        )
    }

    pub(crate) fn stale(message: String) -> Self {
        Self::new(ErrorCode::Stale, format!("{message}; plan again"))
    }

    pub(crate) fn backup_failed(message: String) -> Self {
        Self::new(
            ErrorCode::BackupFailed,
            format!("{message}; nothing was written"),
        )
    }

    pub(crate) fn write_failed(message: String) -> Self {
        Self::new(
            ErrorCode::WriteFailed,
            format!("{message}; nothing was written"),
        )
    }

    /// No Operation in History has the id `id`.
    pub(crate) fn no_operation(id: &str) -> Self {
        Self::new(
            ErrorCode::NoMatch,
            format!("no Operation `{id}` in History"),
        )
    }

    pub(crate) fn undo_refused(message: String, backup: PathBuf, diff: String) -> Self {
        Self {
            message: format!(
                "{message}, so the Undo is refused; the files as they were before the Operation are in {}",
                backup.display()
            ),
            backup: Some(backup),
            diff: Some(diff),
            ..Self::new(ErrorCode::UndoRefused, String::new())
        }
    }

    pub(crate) fn needs_recovery(message: String, backup: PathBuf) -> Self {
        Self {
            message: format!(
                "{message}; the Operation needs manual recovery: the files as they were before it are in {}",
                backup.display()
            ),
            backup: Some(backup),
            ..Self::new(ErrorCode::NeedsRecovery, String::new())
        }
    }
}
