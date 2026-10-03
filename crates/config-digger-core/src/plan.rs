//! Plan: builds an Operation from an action and the Entries it targets,
//! without writing anything. It computes the edits of every touched file,
//! predicts the States after them, and lists what must be acknowledged.

use crate::api::{
    self, Action, Change, Context, DiffUnavailable, EditSummary, EffectiveChange, Error,
    FileChange, FileDiff, Inventory, Preview, ReadFailure, ScanOptions, StateAfter, Targets,
    Warning, WarningClass,
};
use crate::definitions::AgentDefinition;
use crate::definitions::blocks::Parse;
use crate::edit::{Dependency, Edit, EditError, EditableFile, ReadOnly, Syntax};
use crate::mask::{self, Side};
use crate::ports::Ports;
use crate::scan::{LocationReader, Snapshot};
use crate::{explain, history, resolve, scan, select};
use std::io;
use std::path::{Path, PathBuf};

/// A plan, as [`api::Plan`] holds it.
pub(crate) struct Planned {
    pub action: Action,
    /// How many Entries it targets.
    pub entries: usize,
    pub context: Context,
    /// What Verify scans with.
    pub scan_options: ScanOptions,
    pub files: Vec<PlannedFile>,
    pub preview: Preview,
    pub warnings: Vec<Warning>,
    /// The Read failures before the Operation: Verify fails it when a
    /// written file no longer parses for a reader that could read it.
    pub read_failures: Vec<ReadFailure>,
}

/// The edits to one file.
pub(crate) struct PlannedFile {
    pub path: PathBuf,
    pub syntax: Syntax,
    /// The bytes the edits were computed on: the minimal structure of the
    /// syntax when the file is missing and the plan creates it.
    pub read: Vec<u8>,
    /// The file is missing and the plan creates it.
    pub created: bool,
    pub edited: EditedFile,
    /// The locations that read the file, whose Entries say where its
    /// Secrets are.
    pub readers: Vec<LocationReader>,
}

/// A file with every edit of an Operation applied, in order.
pub(crate) struct EditedFile {
    pub bytes: Vec<u8>,
    pub edits: Vec<AppliedEdit>,
}

pub(crate) struct AppliedEdit {
    pub edit: Edit,
    pub inverse: Edit,
    pub depends_on: Vec<Dependency>,
}

/// Why a file's edits could not be computed.
pub(crate) enum EditFailure {
    ReadOnly(ReadOnly),
    Edit(EditError),
}

pub(crate) fn plan(
    definitions: &[AgentDefinition],
    ports: &Ports,
    snapshot: &Snapshot,
    scan_options: &ScanOptions,
    context: &Context,
    action: Action,
    targets: &Targets,
) -> Result<Planned, Error> {
    let before = resolve::resolve(definitions, snapshot, context);
    let targeted = select::select_entries(&before, &targets.selectors, targets.all)?;

    // The edits of each file, in the order its Entries were selected.
    let mut by_file: Vec<(PathBuf, Syntax, Vec<Edit>)> = Vec::new();
    for entry in &targeted {
        let edit = match action {
            Action::Remove => Edit::Remove {
                key_path: entry.key_path.clone(),
            },
            Action::Undo => {
                return Err(Error::unexpected(
                    "an Undo targets an Operation, not Entries: plan it with plan_undo",
                ));
            }
            Action::DeleteBackup => {
                return Err(Error::unexpected(
                    "a Delete backup targets an Operation, not Entries: plan it with plan_delete_backup",
                ));
            }
        };
        match by_file.iter_mut().find(|(path, ..)| *path == entry.path) {
            Some((_, _, edits)) => edits.push(edit),
            None => {
                let syntax = syntax(definitions, snapshot, entry);
                by_file.push((entry.path.clone(), syntax, vec![edit]));
            }
        }
    }

    let mut files = Vec::new();
    for (path, syntax, edits) in by_file {
        let read = read(ports, &path)?;
        let edited =
            apply_edits(&read, syntax, &edits).map_err(|failure| failure.into_error(&path))?;
        files.push(PlannedFile {
            readers: snapshot.readers_of(&path),
            path,
            syntax,
            read,
            created: false,
            edited,
        });
    }

    let removed: Vec<(PathBuf, Vec<String>)> = targeted
        .iter()
        .map(|entry| (entry.path.clone(), entry.key_path.clone()))
        .collect();
    let after = resolve::resolve(definitions, &snapshot.without(&removed), context);
    let title = explain::operation_title(action, targeted.len());
    let preview = preview(
        definitions,
        ports,
        action,
        title,
        None,
        &before,
        &after,
        &files,
    )?;
    Ok(Planned {
        action,
        entries: targeted.len(),
        context: context.clone(),
        scan_options: scan_options.clone(),
        files,
        preview,
        warnings: Vec::new(),
        read_failures: before.read_failures,
    })
}

/// Plans the Undo of the Operation `id` in History: the inverse of each of
/// its edits, last first, applied to each file as it is now, and resolved in
/// the context the Operation ran in (ADR 0003).
pub(crate) fn plan_undo(
    definitions: &[AgentDefinition],
    ports: &Ports,
    id: &str,
) -> Result<Planned, Error> {
    let manifests = history::load_all(ports)?;
    let manifest = history::find(&manifests, id)?;
    if let Some(reason) = history::not_undoable(manifest, &manifests) {
        return Err(Error::blocked(format!(
            "Operation {id} cannot be undone: {reason}"
        )));
    }
    let scan_options = ScanOptions {
        projects: manifest.project.iter().cloned().collect(),
    };
    let context = Context {
        project: manifest.project.clone(),
    };
    let scan = |written: &[(PathBuf, Vec<u8>)]| {
        scan::scan_as_if(definitions, ports, &scan_options.projects, written)
            .map_err(Error::unexpected)
    };
    let snapshot = scan(&[])?;
    let mut files = undo_files(ports, manifest).map_err(|blocked| match blocked {
        UndoBlocked::Refused(refused) => refused.into_error(definitions, &snapshot),
        UndoBlocked::Other(error) => error,
    })?;
    for file in &mut files {
        file.readers = snapshot.readers_of(&file.path);
    }

    let before = resolve::resolve(definitions, &snapshot, &context);
    let written: Vec<(PathBuf, Vec<u8>)> = files
        .iter()
        .map(|file| (file.path.clone(), file.edited.bytes.clone()))
        .collect();
    let after = resolve::resolve(definitions, &scan(&written)?, &context);
    let title = explain::undo_title(id, &manifest.title);
    let preview = preview(
        definitions,
        ports,
        Action::Undo,
        title,
        Some(id.to_owned()),
        &before,
        &after,
        &files,
    )?;
    Ok(Planned {
        action: Action::Undo,
        entries: preview.changes.len(),
        context,
        scan_options,
        files,
        preview,
        warnings: Vec::new(),
        read_failures: before.read_failures,
    })
}

/// Plans the deletion of the backup of the Operation `id` in History: the
/// copies it deletes, and when the Operation needs attention, the warning
/// that they are the only copy for manual recovery. It writes no
/// configuration, so it has no context, edits or States.
pub(crate) fn plan_delete_backup(ports: &Ports, id: &str) -> Result<Planned, Error> {
    let manifests = history::load_all(ports)?;
    let manifest = history::find(&manifests, id)?;
    let folder = history::operation_folder(ports, id)?;
    let deleted = history::backup_copies(ports, &folder, manifest);
    if history::backup_already_deleted(manifest, &deleted) {
        return Err(Error::blocked(format!(
            "the backup of Operation {id} was already deleted"
        )));
    }
    let mut warnings = Vec::new();
    if manifest.needs_attention() {
        warnings.push(Warning {
            class: WarningClass::NeedsAttention,
            sentence: explain::only_copy_for_recovery(id),
        });
    }
    let preview = Preview {
        action: Action::DeleteBackup,
        title: explain::delete_backup_title(id, &manifest.title),
        undo_of: None,
        backup_of: Some(id.to_owned()),
        project: None,
        changes: Vec::new(),
        effective: Vec::new(),
        files: Vec::new(),
        deleted,
        backup_note: explain::delete_backup_note(id),
        backup_folder: folder,
    };
    Ok(Planned {
        action: Action::DeleteBackup,
        entries: 0,
        context: Context::default(),
        scan_options: ScanOptions::default(),
        files: Vec::new(),
        preview,
        warnings,
        read_failures: Vec::new(),
    })
}

/// A touched file's bytes for an Undo, and whether it is missing. A missing
/// file whose folder exists is planned from the minimal structure of its
/// syntax, so that the Undo recreates it; one whose folder is gone cannot be.
fn read_or_create(ports: &Ports, path: &Path, syntax: Syntax) -> Result<(Vec<u8>, bool), Error> {
    match ports.fs.read(path) {
        Ok(bytes) => Ok((bytes, false)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            match path.parent().filter(|folder| ports.fs.exists(folder)) {
                Some(_) => Ok((empty_file(syntax).to_vec(), true)),
                None => Err(Error::blocked(format!(
                    "the folder of {} is gone, so there is nowhere to put the Entries back",
                    path.display()
                ))),
            }
        }
        Err(error) => Err(Error::blocked(format!(
            "cannot read {}: {error}",
            path.display()
        ))),
    }
}

/// The minimal structure of a file in a syntax, which keys are added to.
fn empty_file(syntax: Syntax) -> &'static [u8] {
    match syntax {
        Syntax::Jsonc => b"{\n}\n",
        Syntax::Toml => b"",
    }
}

/// Why the inverse edits of one file could not be computed.
enum UndoFailure {
    /// The key changed since the Operation wrote it.
    Changed(Vec<String>),
    /// The key the Operation took away exists again.
    Appeared(Vec<String>),
    Other(EditFailure),
}

/// Applies the inverse of each edit of a recorded file, last first, to the
/// file as it is now. Each inverse applies only while every key its edit
/// depends on still holds the value the edit gave it.
fn undo_edits(bytes: &[u8], location: &history::LocationRecord) -> Result<EditedFile, UndoFailure> {
    let read_only = |reason| UndoFailure::Other(EditFailure::ReadOnly(reason));
    let edit_error = |error| UndoFailure::Other(EditFailure::Edit(error));
    let mut file = EditableFile::decode(bytes, location.syntax).map_err(read_only)?;
    let mut edited = bytes.to_vec();
    let mut applied_edits = Vec::new();
    for record in location.edits.iter().rev() {
        for dependency in &record.depends_on {
            let now = file.value_at(&dependency.key_path).map_err(edit_error)?;
            if now == dependency.after {
                continue;
            }
            let key_path = dependency.key_path.clone();
            return Err(match dependency.after {
                None => UndoFailure::Appeared(key_path),
                Some(_) => UndoFailure::Changed(key_path),
            });
        }
        let inverse = with_parents(&file, &record.inverse).map_err(edit_error)?;
        let applied = file.apply(&inverse).map_err(edit_error)?;
        edited = applied.bytes;
        file = EditableFile::decode(&edited, location.syntax).map_err(read_only)?;
        applied_edits.push(AppliedEdit {
            edit: inverse,
            inverse: applied.inverse,
            depends_on: applied.depends_on,
        });
    }
    Ok(EditedFile {
        bytes: edited,
        edits: applied_edits,
    })
}

/// An insertion whose parent objects are gone becomes a set of the outermost
/// missing one, holding the rest, so that the file gets back the minimal
/// structure that holds the key.
fn with_parents(file: &EditableFile, edit: &Edit) -> Result<Edit, EditError> {
    let Edit::Insert {
        key_path, value, ..
    } = edit
    else {
        return Ok(edit.clone());
    };
    for depth in 1..key_path.len() {
        if file.value_at(&key_path[..depth])?.is_none() {
            let value = key_path[depth..].iter().rev().fold(
                value.clone(),
                |inner, key| serde_json::json!({ key: inner }),
            );
            return Ok(Edit::Set {
                key_path: key_path[..depth].to_vec(),
                value,
            });
        }
    }
    Ok(edit.clone())
}

/// Why an Operation cannot be undone, as [`undo_files`] finds it.
pub(crate) enum UndoBlocked<'m> {
    /// What the Operation touched changed since it ran.
    Refused(Refused<'m>),
    Other(Error),
}

impl UndoBlocked<'_> {
    /// Why, as a sentence.
    pub fn message(&self) -> String {
        match self {
            UndoBlocked::Refused(refused) => refused.message(),
            UndoBlocked::Other(error) => error.message.clone(),
        }
    }
}

impl From<Error> for UndoBlocked<'_> {
    fn from(error: Error) -> Self {
        UndoBlocked::Other(error)
    }
}

/// The files of an Operation with the inverse of its edits applied to each
/// one as it is now, or why its Undo is refused. History runs the same
/// check to say whether the Operation can be undone. The files' readers are
/// left for the caller to fill in.
pub(crate) fn undo_files<'m>(
    ports: &'m Ports,
    manifest: &'m history::Manifest,
) -> Result<Vec<PlannedFile>, UndoBlocked<'m>> {
    let mut files = Vec::new();
    let mut changed = Vec::new();
    for location in &manifest.locations {
        let (read, created) = read_or_create(ports, &location.path, location.syntax)?;
        match undo_edits(&read, location) {
            Ok(edited) => files.push(PlannedFile {
                path: location.path.clone(),
                syntax: location.syntax,
                read,
                created,
                edited,
                readers: Vec::new(),
            }),
            Err(UndoFailure::Changed(key_path)) => changed.push(ChangedFile {
                location,
                key_path,
                current: (!created).then_some(read),
            }),
            Err(UndoFailure::Appeared(key_path)) => {
                return Err(Error::blocked(format!(
                    "`{}` in {} appeared since Operation {}, and the Undo would overwrite it",
                    key_path.join("."),
                    location.path.display(),
                    manifest.id
                ))
                .into());
            }
            Err(UndoFailure::Other(failure)) => {
                return Err(failure.into_error(&location.path).into());
            }
        }
    }
    if !changed.is_empty() {
        return Err(UndoBlocked::Refused(Refused {
            folder: history::operation_folder(ports, &manifest.id)?,
            changed,
            ports,
        }));
    }
    Ok(files)
}

/// A touched file whose Undo is refused.
struct ChangedFile<'m> {
    location: &'m history::LocationRecord,
    /// The first key found changed.
    key_path: Vec<String>,
    /// What the file holds now, `None` when it was deleted.
    current: Option<Vec<u8>>,
}

/// The refusal of an Undo whose files changed since the Operation.
pub(crate) struct Refused<'m> {
    /// The Operation's folder, which holds its backup.
    folder: PathBuf,
    changed: Vec<ChangedFile<'m>>,
    ports: &'m Ports,
}

impl Refused<'_> {
    /// What changed, as a sentence.
    fn message(&self) -> String {
        Error::undo_refused_message(&self.what(), &self.folder)
    }

    fn what(&self) -> String {
        let what: Vec<String> = self
            .changed
            .iter()
            .map(|file| match file.current {
                Some(_) => format!(
                    "`{}` in {} changed",
                    file.key_path.join("."),
                    file.location.path.display()
                ),
                None => format!("{} was deleted", file.location.path.display()),
            })
            .collect();
        format!("{} since the Operation", what.join("; "))
    }

    /// The refusal, with a diff from each backed-up file to what it holds
    /// now, masked for the locations of `snapshot` that read it.
    fn into_error(self, definitions: &[AgentDefinition], snapshot: &Snapshot) -> Error {
        let diffs = self
            .changed
            .iter()
            .map(|file| {
                let path = &file.location.path;
                // A file the Operation created had nothing to back up.
                let backed_up = match &file.location.backup {
                    Some(backup) => {
                        let backup = self.folder.join(backup);
                        match self.ports.fs.read(&backup) {
                            Ok(bytes) => Some(bytes),
                            Err(error) => return unreadable_backup(path, &backup, &error),
                        }
                    }
                    None => None,
                };
                mask::diff(
                    definitions,
                    &snapshot.readers_of(path),
                    path,
                    Side {
                        bytes: backed_up.as_deref(),
                        name: "in the backup",
                    },
                    Side {
                        bytes: file.current.as_deref(),
                        name: "as it is now",
                    },
                )
            })
            .collect();
        Error::undo_refused(self.what(), self.folder, diffs)
    }
}

/// No diff of a file whose backup cannot be read.
fn unreadable_backup(path: &Path, backup: &Path, error: &io::Error) -> FileDiff {
    FileDiff {
        path: path.to_path_buf(),
        diff: None,
        diff_unavailable: Some(DiffUnavailable::Io),
        diff_note: Some(format!(
            "Can't show a diff: the backup {} cannot be read: {error}.",
            backup.display()
        )),
        // Nothing says how many lines differ without the backup.
        changed_lines: 0,
    }
}

/// What a plan shows before it is confirmed.
#[allow(clippy::too_many_arguments)]
fn preview(
    definitions: &[AgentDefinition],
    ports: &Ports,
    action: Action,
    title: String,
    undo_of: Option<String>,
    before: &Inventory,
    after: &Inventory,
    files: &[PlannedFile],
) -> Result<Preview, Error> {
    let (changes, effective) = predict(before, after);
    let backup_folder = history::backups_folder(&history::data_folder(ports.env.as_ref())?);
    Ok(Preview {
        action,
        title,
        undo_of,
        backup_of: None,
        project: before.project.clone(),
        changes,
        effective,
        files: files
            .iter()
            .map(|file| {
                let edits = file
                    .edited
                    .edits
                    .iter()
                    .map(|applied| EditSummary::from(&applied.edit))
                    .collect();
                let diff = mask::diff(
                    definitions,
                    &file.readers,
                    &file.path,
                    Side {
                        bytes: (!file.created).then_some(file.read.as_slice()),
                        name: "before the Operation",
                    },
                    Side {
                        bytes: Some(&file.edited.bytes),
                        name: "after the Operation",
                    },
                );
                FileChange::new(edits, diff)
            })
            .collect(),
        deleted: Vec::new(),
        backup_note: explain::backup_note(&backup_folder),
        backup_folder,
    })
}

/// The syntax an Entry's location is edited in.
fn syntax(definitions: &[AgentDefinition], snapshot: &Snapshot, entry: &api::Entry) -> Syntax {
    let scanned = snapshot
        .entries
        .iter()
        .find(|scanned| scanned.path == entry.path && scanned.key_path == entry.key_path)
        .expect("a resolved Entry comes from the snapshot");
    definitions[scanned.agent_index]
        .location(&scanned.location)
        .format
        .syntax()
}

/// A touched file's bytes. It held the Entry when it was scanned, so a file
/// that is gone or unreadable now changed since.
pub(crate) fn read(ports: &Ports, path: &Path) -> Result<Vec<u8>, Error> {
    ports.fs.read(path).map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => Error::stale(format!("{} is gone", path.display())),
        _ => Error::blocked(format!("cannot read {}: {error}", path.display())),
    })
}

/// Applies `edits` to a file's bytes one after the other.
pub(crate) fn apply_edits(
    bytes: &[u8],
    syntax: Syntax,
    edits: &[Edit],
) -> Result<EditedFile, EditFailure> {
    let mut file = EditableFile::decode(bytes, syntax).map_err(EditFailure::ReadOnly)?;
    let mut edited = bytes.to_vec();
    let mut applied_edits = Vec::new();
    for edit in edits {
        let applied = file.apply(edit).map_err(EditFailure::Edit)?;
        edited = applied.bytes;
        file = EditableFile::decode(&edited, syntax).map_err(EditFailure::ReadOnly)?;
        applied_edits.push(AppliedEdit {
            edit: edit.clone(),
            inverse: applied.inverse,
            depends_on: applied.depends_on,
        });
    }
    Ok(EditedFile {
        bytes: edited,
        edits: applied_edits,
    })
}

impl EditFailure {
    pub(crate) fn into_error(self, path: &Path) -> Error {
        match self {
            EditFailure::ReadOnly(reason) => {
                Error::blocked(format!("cannot write {}: {reason}", path.display()))
            }
            EditFailure::Edit(EditError::NotFound { key_path }) => Error::stale(format!(
                "`{}` is no longer in {}",
                key_path.join("."),
                path.display()
            )),
            EditFailure::Edit(EditError::Parse) => {
                Error::stale(format!("{} no longer parses", path.display()))
            }
            EditFailure::Edit(error) => {
                Error::blocked(format!("cannot edit {} safely: {error}", path.display()))
            }
        }
    }
}

/// The States an Operation changes, from the Items before it to the Items
/// after it, and for each changed Item with Entries on both sides, which
/// Entry is Effective afterwards.
fn predict(before: &Inventory, after: &Inventory) -> (Vec<Change>, Vec<EffectiveChange>) {
    let same = |a: &api::Item, b: &api::Item| {
        a.agent == b.agent && a.home == b.home && a.kind == b.kind && a.name == b.name
    };
    let added = after
        .items
        .iter()
        .filter(|item| !before.items.iter().any(|known| same(known, item)));
    let mut changes = Vec::new();
    let mut effective = Vec::new();
    for item in before.items.iter().chain(added) {
        let item_before = before.items.iter().find(|known| same(known, item));
        let item_after = after.items.iter().find(|known| same(known, item));
        let entries_before = item_before.map_or(&[][..], |item| item.entries.as_slice());
        let entries_after = item_after.map_or(&[][..], |item| item.entries.as_slice());
        // Rows follow the precedence of the side with more Entries, which is
        // the one that has them all when the Operation only takes Entries
        // away or only puts them back.
        let (first, second) = if entries_after.len() > entries_before.len() {
            (entries_after, entries_before)
        } else {
            (entries_before, entries_after)
        };
        let rows = first.iter().chain(
            second
                .iter()
                .filter(|entry| !first.iter().any(|row| row.locator == entry.locator)),
        );
        let agent_name = before
            .homes
            .iter()
            .chain(&after.homes)
            .find(|home| home.agent == item.agent)
            .map_or(item.agent.as_str(), |home| home.agent_name.as_str());
        let state = |entries: &[api::Entry], locator: &str| {
            entries
                .iter()
                .find(|entry| entry.locator == locator)
                .map_or(StateAfter::Removed, |entry| entry.state.into())
        };
        let mut changed = false;
        for entry in rows {
            let state_before = state(entries_before, &entry.locator);
            let state_after = state(entries_after, &entry.locator);
            if state_before == state_after {
                continue;
            }
            changed = true;
            changes.push(Change {
                agent: item.agent.clone(),
                agent_name: agent_name.to_owned(),
                home: item.home.clone(),
                item: item.name.clone(),
                path: entry.path.clone(),
                locator: entry.locator.clone(),
                project: before.project.clone(),
                before: state_before,
                after: state_after,
            });
        }
        if let (true, Some(item_before), Some(item_after)) = (changed, item_before, item_after) {
            let before_locator = &item_before.entries[0].locator;
            let after_locator = &item_after.entries[0].locator;
            effective.push(EffectiveChange {
                agent: item.agent.clone(),
                home: item.home.clone(),
                item: item.name.clone(),
                before: before_locator.clone(),
                after: after_locator.clone(),
                sentence: explain::effective_after(
                    &item.name,
                    agent_name,
                    after_locator,
                    before_locator == after_locator,
                ),
            });
        }
    }
    (changes, effective)
}

/// The Item of an Agent home that has an Entry at `locator`.
fn item_in<'i>(
    inventory: &'i Inventory,
    agent: &str,
    home: &str,
    locator: &str,
) -> Option<&'i api::Item> {
    inventory.items.iter().find(|item| {
        item.agent == agent
            && item.home == home
            && item.entries.iter().any(|entry| entry.locator == locator)
    })
}

/// The State of the Entry at `locator` in an Agent home, `Removed` when it is not there.
pub(crate) fn state_in(
    inventory: &Inventory,
    agent: &str,
    home: &str,
    locator: &str,
) -> StateAfter {
    item_in(inventory, agent, home, locator)
        .and_then(|item| item.entries.iter().find(|entry| entry.locator == locator))
        .map_or(StateAfter::Removed, |entry| entry.state.into())
}
