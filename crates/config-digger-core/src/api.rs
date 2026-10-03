//! The public surface shared by the app and the CLI: [`Core`] and the serde
//! DTOs it returns. Domain types stay internal. The DTOs also derive
//! `specta::Type`, from which the app's TypeScript bindings are generated.

use crate::definitions::blocks::Provenance as DefinitionProvenance;
use crate::definitions::{self, AgentDefinition, ItemKind as DefinitionItemKind};
use crate::edit::Edit;
use crate::ports::Ports;
use crate::{execute, history, plan, resolve, scan, select, skill};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

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

    /// Loads the built-in Agent definitions plus the extra ones in `files`,
    /// each a file name and its TOML text, as `--definitions` does for
    /// development and tests (ADR 0005). A file that fails to load is
    /// rejected whole.
    pub fn with_extra_definitions(ports: Ports, files: &[(&str, &str)]) -> Result<Self, Error> {
        let mut definitions = definitions::built_in().map_err(Error::unexpected)?;
        for (file, text) in files {
            definitions.push(definitions::parse(file, text).map_err(Error::unexpected)?);
        }
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

    /// Plans the deletion of the backup of the Operation `id`, such as one
    /// holding Secrets: its copies go for good, and so do the values of its
    /// edits in History, so it can no longer be undone. When the Operation
    /// needs attention, the plan warns that the backup is the only copy for
    /// manual recovery, and executing it dismisses the Operation.
    pub fn plan_delete_backup(&self, id: &str) -> Result<Plan, Error> {
        plan::plan_delete_backup(&self.ports, id).map(Plan)
    }

    /// Every Operation in History, newest first, across restarts.
    pub fn history(&self) -> Result<Vec<OperationSummary>, Error> {
        Ok(history::summaries(
            &self.ports,
            &history::load_all(&self.ports)?,
        ))
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

    /// Dismisses an Operation that needs attention, once its files have been
    /// recovered by hand: it is no longer flagged, and pruning treats it like
    /// any other. It is refused for any other Operation.
    pub fn dismiss(&self, id: &str) -> Result<OperationSummary, Error> {
        history::dismiss(&self.ports, id)
    }

    /// What the skill folder at `folder` holds, as a skill Entry shows it:
    /// each file's relative path and size, and the `name` and `description`
    /// of its `SKILL.md` frontmatter. File content is never shown, since a
    /// skill folder can hold Secrets, such as a `.env` file.
    pub fn skill_content(&self, folder: &Path) -> Result<SkillContent, Error> {
        skill::content(self.ports.fs.as_ref(), folder)
    }

    /// How the skill folder at `after` differs from the one at `before`,
    /// such as a backed-up copy and the folder as it is now: each file
    /// added, removed or changed, with its sizes and never its content. A
    /// missing folder holds no files.
    pub fn compare_skill_folders(
        &self,
        before: &Path,
        after: &Path,
    ) -> Result<Vec<SkillFileChange>, Error> {
        skill::compare(self.ports.fs.as_ref(), before, after)
    }
}

/// A skill folder as config-digger shows it, without any file's content.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct SkillContent {
    /// The `name` of the `SKILL.md` frontmatter.
    pub name: Option<String>,
    /// The `description` of the `SKILL.md` frontmatter.
    pub description: Option<String>,
    /// Every file in the folder and below it, by path.
    pub files: Vec<SkillFile>,
}

/// One file of a skill folder.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct SkillFile {
    /// Relative to the skill folder, `/`-separated.
    pub path: String,
    /// In bytes.
    #[specta(type = f64)]
    pub size: u64,
}

/// A file that differs between two versions of a skill folder.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct SkillFileChange {
    /// Relative to the skill folder, `/`-separated.
    pub path: String,
    pub change: SkillFileChangeKind,
    /// In bytes, `None` when the file is added.
    #[specta(type = Option<f64>)]
    pub size_before: Option<u64>,
    /// In bytes, `None` when the file is removed.
    #[specta(type = Option<f64>)]
    pub size_after: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum SkillFileChangeKind {
    Added,
    Removed,
    /// Its content differs, which is never shown.
    Changed,
}

/// What an Operation does to the Entries it targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// Makes each Entry stop taking effect for one Agent while keeping it,
    /// through the Agent's own machine-local switch.
    Disable,
    /// Makes each Disabled Entry take effect again, lifting every switch
    /// that Disables it, whoever set it.
    Enable,
    /// Deletes each Entry from where it lives, for every Agent that reads it.
    Remove,
    /// Applies the inverse of every edit of an earlier Operation to the files
    /// as they are now.
    Undo,
    /// Deletes the backup of an earlier Operation, which can no longer be
    /// undone afterwards.
    DeleteBackup,
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

    /// Why it cannot execute, whatever is acknowledged. Empty when it can.
    pub fn blocks(&self) -> &[Block] {
        &self.0.blocks
    }
}

/// Why a plan cannot execute, whatever is acknowledged. The user removes
/// the cause, such as by closing an Agent, and plans again.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct Block {
    pub code: BlockCode,
    /// The cause and what to do about it, as a sentence rendered by the core.
    pub sentence: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum BlockCode {
    /// The plan writes a SQLite store while the Agent holding it open runs.
    RunningAgentSqlite,
}

/// What a plan will change, shown before it is confirmed.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct Preview {
    pub action: Action,
    /// What the Operation does, as a title such as "Remove 2 Entries".
    pub title: String,
    /// The Operation this one undoes, when it is an Undo.
    pub undo_of: Option<String>,
    /// The Operation whose backup it deletes, when it is a Delete backup.
    pub backup_of: Option<String>,
    /// The context's Project, as in [`Inventory::project`].
    pub project: Option<PathBuf>,
    /// One row per (Agent, Entry, Project) whose State the Operation changes.
    pub changes: Vec<Change>,
    /// For each Item that keeps Entries, which one is Effective afterwards.
    pub effective: Vec<EffectiveChange>,
    pub files: Vec<FileChange>,
    /// The backed-up copies a Delete backup deletes for good.
    pub deleted: Vec<PathBuf>,
    /// Where each touched file is copied before anything is written, or for
    /// a Delete backup, the folder it deletes the copies from.
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
    Disabled,
    Removed,
}

impl From<State> for StateAfter {
    fn from(state: State) -> Self {
        match state {
            State::Effective => StateAfter::Effective,
            State::Shadowed => StateAfter::Shadowed,
            State::Disabled => StateAfter::Disabled,
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
    /// The locator of the Effective Entry before, `None` when none was.
    pub before: Option<String>,
    /// The locator of the Effective Entry after, `None` when none is.
    pub after: Option<String>,
    /// The change as a sentence, rendered by the core.
    pub sentence: String,
}

/// The edits to one file and their diff.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct FileChange {
    pub path: PathBuf,
    /// The file is in the index of its git repo, so the edits show in the
    /// git diff.
    pub git_versioned: bool,
    pub edits: Vec<EditSummary>,
    /// A unified diff of the whole file, before and after every edit, as
    /// [`FileDiff::diff`] has it.
    pub diff: Option<String>,
    pub diff_unavailable: Option<DiffUnavailable>,
    pub diff_note: Option<String>,
    pub changed_lines: u32,
}

impl FileChange {
    pub(crate) fn new(edits: Vec<EditSummary>, diff: FileDiff, git_versioned: bool) -> Self {
        Self {
            path: diff.path,
            git_versioned,
            edits,
            diff: diff.diff,
            diff_unavailable: diff.diff_unavailable,
            diff_note: diff.diff_note,
            changed_lines: diff.changed_lines,
        }
    }
}

/// A diff of two versions of one file, with every Secret masked in both,
/// or why it cannot be shown.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct FileDiff {
    pub path: PathBuf,
    /// A unified diff, computed on the real text and shown with every Secret
    /// masked, context lines and other Entries included. A line whose only
    /// change is a Secret shows the same on both sides and is followed by
    /// the line `\ secret value changed`. `None` when it cannot be shown.
    pub diff: Option<String>,
    /// Why there is no diff.
    pub diff_unavailable: Option<DiffUnavailable>,
    /// Why there is no diff, as a sentence rendered by the core.
    pub diff_note: Option<String>,
    /// How many lines differ, whether or not the diff can be shown.
    pub changed_lines: u32,
}

/// Why a diff cannot be shown with its Secrets masked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum DiffUnavailable {
    /// One side does not parse in the dialect of an Agent that reads the file.
    Parse,
    /// One side is not UTF-8 text.
    Decode,
    /// One side cannot be read, such as a backup copy that is gone.
    Io,
    /// No Agent reads Entries from the file, such as one that only holds
    /// Disable switches, or none any more, so nothing says where its
    /// Secrets are.
    NoReader,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct EditSummary {
    pub op: EditOp,
    pub key_path: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
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
    /// The backup a Delete backup deletes is the only copy for the manual
    /// recovery of an Operation that needs attention.
    NeedsAttention,
}

impl WarningClass {
    /// The class as it appears in JSON and in `--ack`.
    pub fn code(self) -> &'static str {
        match self {
            WarningClass::GitVersioned => "git-versioned",
            WarningClass::WideReach => "wide-reach",
            WarningClass::RunningAgent => "running-agent",
            WarningClass::NeedsAttention => "needs-attention",
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
    /// What to do next, such as restarting a running Agent that reads a
    /// written file, as sentences rendered by the core.
    pub hints: Vec<String>,
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
    /// Failed, and rolled back: no file holds any of its edits.
    Failed,
    /// Failed, and its Rollback could not restore every file, so its backup
    /// is needed to recover. Its backup is kept until it is dismissed.
    NeedsAttention,
}

/// Where a failed Operation's Rollback left one file it touched.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct RollbackLocation {
    pub path: PathBuf,
    pub outcome: RollbackOutcome,
    /// Why it was left alone or not restored, as a sentence.
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum RollbackOutcome {
    /// It holds the bytes it held before the Operation again, or is gone
    /// again when the Operation created it.
    Restored,
    /// The Operation never wrote it.
    NotWritten,
    /// It changed after config-digger wrote it, and holds none of the
    /// Operation's edits any more, so it is left as it is.
    LeftAlone,
    /// It still holds some of the Operation's edits and could not be
    /// restored. It needs manual recovery from the backup.
    NotRestored,
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
    /// Why it failed, as a sentence, when it did.
    pub cause: Option<String>,
    /// Its Rollback is incomplete and it was not dismissed yet.
    pub needs_attention: bool,
    /// The Operation this one undoes, when it is an Undo.
    pub undo_of: Option<String>,
    /// The Undo of this Operation, while that Undo is in effect.
    pub undone_by: Option<String>,
    /// Its backup was deleted, and with it the values of its edits.
    pub backup_deleted: bool,
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
    /// Where its Rollback left each file it touched, when it failed.
    pub rollback: Vec<RollbackLocation>,
    pub verify: Option<Verify>,
    /// Where the States after differed from the prediction.
    pub differences: Vec<Difference>,
}

/// One file an Operation touched.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct OperationLocation {
    pub path: PathBuf,
    /// Its copy as it was before the Operation, or `None` when the Operation
    /// created the file or its backup was deleted.
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
    /// Its file is in the index of its git repo, so a change to it shows in
    /// the git diff and reaches every clone once committed.
    pub git_versioned: bool,
    pub state: State,
    /// Why the Entry has its State, when it is not Effective: the first of
    /// the Agent's checks that it fails.
    pub reason: Option<Reason>,
    /// The other checks it fails, in the Agent's order.
    pub secondary: Vec<Reason>,
    /// For a Disabled Entry, the State it would have once Enabled.
    pub what_if: Option<WhatIf>,
    /// Switches that turn part of it off, leaving its State as it is.
    pub notes: Vec<Note>,
    /// The Entry's value as declared, with every Secret masked as `••••`.
    #[specta(type = JsonValue)]
    pub config: serde_json::Value,
    /// The key paths in `config`, relative to the Entry, whose values were
    /// masked, so that a mask can be told from a literal. An array item's
    /// key is its index.
    pub masked: Vec<Vec<String>>,
    /// What Disable would do to it in this context.
    pub disable: DisableOption,
    /// For a Disabled Entry, what Enable writes, as a sentence rendered by
    /// the core.
    pub enable: Option<String>,
}

/// The State a Disabled Entry would have once Enabled.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct WhatIf {
    pub state: State,
    /// The locator of the Entry that would beat it, if one would.
    pub effective: Option<String>,
    /// Such as "Disabled: would be Shadowed by …", rendered by the core.
    pub sentence: String,
}

/// Something about an Entry that does not change its State.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct Note {
    pub code: NoteCode,
    /// The `<path>#<key path>` locator of the switch it comes from.
    pub switch: String,
    /// The note as a sentence, rendered by the core.
    pub sentence: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum NoteCode {
    /// A switch turns part of the Entry off, and the Agent still uses it.
    PartialSwitch,
}

/// What Disable would do to an Entry in its context: write the Agent's own
/// switch for it, or nothing, with the reason.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum DisableOption {
    /// Disable writes the switch of the Agent's most specific mechanism that
    /// covers the Entry in the context, the first in its order of preference
    /// among equally specific ones, whose file is machine-local, not
    /// git-versioned, and readable.
    Offered {
        /// The file the switch is in.
        path: PathBuf,
        /// What Disable writes, as a sentence rendered by the core.
        sentence: String,
    },
    /// Disable is not offered.
    NotOffered {
        code: DisableNotOffered,
        /// The reason as a sentence, rendered by the core.
        sentence: String,
    },
}

impl DisableOption {
    /// What Disable writes, or why it is not offered.
    pub fn sentence(&self) -> &str {
        match self {
            DisableOption::Offered { sentence, .. }
            | DisableOption::NotOffered { sentence, .. } => sentence,
        }
    }
}

/// Why Disable is not offered for an Entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum DisableNotOffered {
    /// Every switch of the Agent that could Disable the Entry is in a
    /// git-versioned file, where a change reaches every clone.
    GitVersioned,
    /// The Agent has no switch that Disables the Entry in this context.
    NoMechanism,
    /// Every switch of the Agent that could Disable the Entry is in a file
    /// config-digger cannot read, so it cannot be written safely.
    UnreadableSwitch,
    /// The Entry is Disabled already.
    AlreadyDisabled,
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
    /// The Agent skips it because a switch Disables it, natively or set by
    /// config-digger.
    Disabled,
}

/// Why an Entry has its State.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct Reason {
    pub code: ReasonCode,
    /// The locator of the Effective Entry that beats this one, if one does.
    pub effective: Option<String>,
    /// The `<path>#<key path>` locator of the switch that Disables it, if one does.
    pub switch: Option<String>,
    /// The reason as a sentence, rendered by the core.
    pub sentence: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ReasonCode {
    /// A same-named Entry at a location the Agent ranks higher is Effective.
    ShadowedByPrecedence,
    /// A switch names the Item, so the Agent skips every Entry of it that
    /// the switch covers.
    DisabledByName,
    /// The Entry's own switch is off.
    DisabledNatively,
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

impl ReadFailure {
    /// Whether `other` is the same reader failing on the same location,
    /// whatever the kind of failure.
    pub(crate) fn same_reader_and_location(&self, other: &ReadFailure) -> bool {
        self.agent == other.agent && self.home == other.home && self.path == other.path
    }
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
    /// The backup folder of a failed Operation, or of one whose Undo was
    /// refused.
    pub backup: Option<PathBuf>,
    /// For a refused Undo, or a Rollback that is incomplete, a diff from
    /// each backed-up file to what it holds now, with every Secret masked.
    pub diffs: Vec<FileDiff>,
    /// The Operation that failed, once it was recorded in History.
    pub failed: Option<Box<FailedOperation>>,
}

/// An Operation that failed after it was recorded in History.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct FailedOperation {
    /// Its id in History.
    pub operation: String,
    /// For `rollback_incomplete`, the code of the failure that started the
    /// Rollback.
    pub failure: Option<ErrorCode>,
    /// Where its Rollback left each file it touched.
    pub rollback: Vec<RollbackLocation>,
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
    /// A write failed; the Operation was rolled back.
    WriteFailed,
    /// An edit did not land; the Operation was rolled back.
    VerifyFailed,
    /// An Agent rewrote a file after config-digger wrote it, taking an edit
    /// away; the Operation was rolled back.
    OverwrittenByAgent,
    /// A file no longer parses for a reader that could read it before; the
    /// Operation was rolled back.
    ReadFailureIntroduced,
    /// The Operation failed and its Rollback could not restore every file,
    /// so it needs manual recovery from its backup.
    RollbackIncomplete,
    /// What the Operation to undo touched changed since it ran.
    UndoRefused,
}

impl ErrorCode {
    /// The code as it appears in JSON.
    pub fn code(self) -> &'static str {
        match self {
            ErrorCode::Unexpected => "unexpected",
            ErrorCode::NoMatch => "no_match",
            ErrorCode::Ambiguous => "ambiguous",
            ErrorCode::AckRequired => "ack_required",
            ErrorCode::Blocked => "blocked",
            ErrorCode::Locked => "locked",
            ErrorCode::Stale => "stale",
            ErrorCode::BackupFailed => "backup_failed",
            ErrorCode::WriteFailed => "write_failed",
            ErrorCode::VerifyFailed => "verify_failed",
            ErrorCode::OverwrittenByAgent => "overwritten_by_agent",
            ErrorCode::ReadFailureIntroduced => "read_failure_introduced",
            ErrorCode::RollbackIncomplete => "rollback_incomplete",
            ErrorCode::UndoRefused => "undo_refused",
        }
    }
}

impl Error {
    fn new(code: ErrorCode, message: String) -> Self {
        Self {
            code,
            message,
            candidates: Vec::new(),
            backup: None,
            diffs: Vec::new(),
            failed: None,
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

    /// No Operation in History has the id `id`.
    pub(crate) fn no_operation(id: &str) -> Self {
        Self::new(
            ErrorCode::NoMatch,
            format!("no Operation `{id}` in History"),
        )
    }

    pub(crate) fn undo_refused(message: String, backup: PathBuf, diffs: Vec<FileDiff>) -> Self {
        Self {
            message: Self::undo_refused_message(&message, &backup),
            backup: Some(backup),
            diffs,
            ..Self::new(ErrorCode::UndoRefused, String::new())
        }
    }

    /// Why an Undo is refused, as [`Error::undo_refused`] says it.
    pub(crate) fn undo_refused_message(message: &str, backup: &Path) -> String {
        format!(
            "{message}, so the Undo is refused; the files as they were before the Operation are in {}",
            backup.display()
        )
    }

    /// The Operation `operation` failed with `cause`, and its Rollback left
    /// each file as `rollback` says. `diffs` go from each backed-up file it
    /// could not restore to what that file holds now.
    pub(crate) fn rolled_back(
        cause: ErrorCode,
        message: String,
        operation: String,
        backup: PathBuf,
        rollback: Vec<RollbackLocation>,
        diffs: Vec<FileDiff>,
    ) -> Self {
        let not_restored = rollback
            .iter()
            .filter(|location| location.outcome == RollbackOutcome::NotRestored)
            .count();
        let complete = not_restored == 0;
        let failure = Self {
            backup: Some(backup.clone()),
            failed: Some(Box::new(FailedOperation {
                operation: operation.clone(),
                failure: (!complete).then_some(cause),
                rollback,
            })),
            ..Self::new(cause, String::new())
        };
        if complete {
            return Self {
                message: format!("{message}; Operation {operation} was rolled back"),
                ..failure
            };
        }
        let files = match not_restored {
            1 => "1 file".to_owned(),
            n => format!("{n} files"),
        };
        Self {
            code: ErrorCode::RollbackIncomplete,
            message: format!(
                "{message}; the Rollback of Operation {operation} could not restore {files}, which need manual recovery: the files as they were before it are in {}",
                backup.display()
            ),
            diffs,
            ..failure
        }
    }
}

/// A JSON value as the TypeScript bindings type it: its numbers are
/// JavaScript numbers. Only the bindings use it; values are `serde_json`'s.
#[derive(Serialize, specta::Type)]
#[serde(untagged)]
#[allow(dead_code)]
pub enum JsonValue {
    Null(()),
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<JsonValue>),
    Object(std::collections::BTreeMap<String, JsonValue>),
}
