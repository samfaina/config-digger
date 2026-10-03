//! Plan: builds an Operation from an action and the Entries it targets,
//! without writing anything. It computes the edits of every touched file,
//! predicts the States after them, and lists what must be acknowledged.

use crate::api::{
    self, Action, Block, Change, Context, DiffUnavailable, EditSummary, EffectiveChange, Error,
    FileChange, FileDiff, Home, Info, InfoCode, Inventory, Leftovers, Preview, ReadFailure,
    ReadFailureKind, ScanOptions, State, StateAfter, Targets, Warning, WarningClass,
};
use crate::definitions::AgentDefinition;
use crate::disable::{self, ContextProject, Hit};
use crate::edit::{Dependency, Edit, EditError, EditableFile, ReadOnly, Syntax};
use crate::explain::Reached;
use crate::mask::{self, Side};
use crate::ports::Ports;
use crate::scan::{LocationReader, Snapshot, SwitchStore};
use crate::{explain, history, installations, resolve, running, scan, select};
use std::collections::BTreeSet;
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
    /// Why it cannot execute, whatever is acknowledged.
    pub blocks: Vec<Block>,
    /// The Read failures before the Operation: Verify fails it when a
    /// written file no longer parses for a reader that could read it.
    pub read_failures: Vec<ReadFailure>,
    /// The Agent homes an Installation found reads, by Agent index and
    /// id: an Agent can run only for them.
    pub installed: BTreeSet<(usize, String)>,
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
    /// The Agents whose Disable switches are in the file.
    pub switches: Vec<SwitchStore>,
    /// The file is in the index of its git repo, so the edits show in the
    /// git diff.
    pub git_versioned: bool,
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
    // The leftovers of the context's own Project are in it already.
    let mut leftovers = resolve::leftovers(definitions, snapshot);
    leftovers.retain(|leftovers| before.project.as_ref() != Some(&leftovers.project));
    let targeted = select::select_entries(&before, &leftovers, &targets.selectors, targets.all)?;
    let planned = Targeted {
        definitions,
        ports,
        snapshot,
        scan_options,
        context,
        action,
        before: &before,
        leftovers: &leftovers,
        entries: &targeted,
    };
    match action {
        Action::Remove => planned.remove(),
        Action::Disable | Action::Enable => planned.switch(),
        Action::Undo => Err(Error::unexpected(
            "an Undo targets an Operation, not Entries: plan it with plan_undo",
        )),
        Action::DeleteBackup => Err(Error::unexpected(
            "a Delete backup targets an Operation, not Entries: plan it with plan_delete_backup",
        )),
    }
}

/// Plans the Remove of every leftover of the missing Project `project`, in
/// any spelling, as one Operation in that Project's context.
pub(crate) fn plan_remove_leftovers(
    definitions: &[AgentDefinition],
    ports: &Ports,
    snapshot: &Snapshot,
    scan_options: &ScanOptions,
    project: &Path,
) -> Result<Planned, Error> {
    let context = Context {
        project: Some(project.to_path_buf()),
    };
    let found = resolve::context_project(snapshot, &context).and_then(|(path, _)| {
        resolve::leftovers(definitions, snapshot)
            .into_iter()
            .find(|leftovers| leftovers.project == path)
    });
    let Some(found) = found else {
        return Err(Error::no_leftovers(project));
    };
    let targets = Targets {
        selectors: found.entries().map(|entry| entry.locator.clone()).collect(),
        all: false,
    };
    let context = Context {
        project: Some(found.project),
    };
    plan(
        definitions,
        ports,
        snapshot,
        scan_options,
        &context,
        Action::Remove,
        &targets,
    )
}

/// An action on the Entries it targets, about to be planned.
struct Targeted<'a> {
    definitions: &'a [AgentDefinition],
    ports: &'a Ports,
    snapshot: &'a Snapshot,
    scan_options: &'a ScanOptions,
    context: &'a Context,
    action: Action,
    /// The Items in the context before the Operation.
    before: &'a Inventory,
    /// The leftovers of the missing Projects other than the context's.
    leftovers: &'a [Leftovers],
    entries: &'a [&'a api::Entry],
}

impl Targeted<'_> {
    /// A Remove: each Entry's key goes from its file.
    fn remove(&self) -> Result<Planned, Error> {
        // The edits of each file, in the order its Entries were selected.
        let mut by_file: Vec<(PathBuf, Syntax, Vec<Edit>)> = Vec::new();
        for entry in self.entries {
            let syntax = removable_syntax(self.definitions, self.snapshot, entry)?;
            let edit = Edit::Remove {
                key_path: entry.key_path.clone(),
            };
            match by_file.iter_mut().find(|(path, ..)| *path == entry.path) {
                Some((_, _, edits)) => edits.push(edit),
                None => {
                    by_file.push((entry.path.clone(), syntax, vec![edit]));
                }
            }
        }

        let mut files = Vec::new();
        for (path, syntax, edits) in by_file {
            let read = read(self.ports, &path)?;
            let edited =
                apply_edits(&read, syntax, &edits).map_err(|failure| failure.into_error(&path))?;
            files.push(self.planned_file(path, syntax, read, false, edited));
        }

        let removed: Vec<(PathBuf, Vec<String>)> = self
            .entries
            .iter()
            .map(|entry| (entry.path.clone(), entry.key_path.clone()))
            .collect();
        let without = self.snapshot.without(&removed);
        let after = resolve::resolve(self.definitions, &without, self.context);
        let (mut changes, effective) = predict(self.before, &after);
        // A leftover of another missing Project goes in that Project's context.
        let holds_target = |leftovers: &&Leftovers| {
            leftovers.entries().any(|leftover| {
                self.entries
                    .iter()
                    .any(|entry| entry.locator == leftover.locator)
            })
        };
        for leftovers in self.leftovers.iter().filter(holds_target) {
            let context = Context {
                project: Some(leftovers.project.clone()),
            };
            let before = resolve::resolve(self.definitions, self.snapshot, &context);
            let after = resolve::resolve(self.definitions, &without, &context);
            changes.extend(predict(&before, &after).0);
        }
        self.planned(files, changes, effective, None)
    }

    /// A Disable or an Enable: each Entry's switch is set or lifted, which
    /// can reach other Entries and Projects than those targeted.
    fn switch(&self) -> Result<Planned, Error> {
        let project = resolve::context_project(self.snapshot, self.context);
        let context_project = project
            .as_ref()
            .map(|(path, key)| ContextProject { path, key });
        // The edits of each switch file, in the order its Entries were selected.
        let mut by_file: Vec<(PathBuf, Vec<Edit>)> = Vec::new();
        let mut lifted: Vec<Hit> = Vec::new();
        for entry in self.entries {
            // Its switches would be offered as for any Entry of the Project.
            if entry.state == State::Unread {
                return Err(Error::blocked(explain::only_removed(
                    self.action,
                    &entry.locator,
                )));
            }
            let scanned = self.snapshot.entry(&entry.path, &entry.key_path);
            let definition = &self.definitions[scanned.agent_index];
            match self.action {
                Action::Disable => {
                    if entry.state == State::Disabled {
                        return Err(Error::blocked(explain::already_disabled(&entry.locator)));
                    }
                    let choice = disable::choose(
                        definition,
                        self.snapshot,
                        scanned,
                        context_project.as_ref(),
                    )
                    .map_err(|(_, sentence)| {
                        Error::blocked(explain::disable_not_offered(&entry.locator, &sentence))
                    })?;
                    // Entries Disabled by one name share its switch.
                    let shared = by_file
                        .iter()
                        .any(|(path, edits)| *path == choice.path && edits.contains(&choice.edit));
                    if !shared {
                        add_edit(&mut by_file, choice.path, choice.edit);
                    }
                }
                _ => {
                    let project = project.as_ref().map(|(_, key)| key.as_str());
                    let (disabling, _) = disable::hits(definition, self.snapshot, scanned, project);
                    if disabling.is_empty() {
                        return Err(Error::blocked(explain::not_disabled(&entry.locator)));
                    }
                    let location = definition.location(&scanned.location);
                    if let Err(sentence) = disable::enable(location, &disabling) {
                        return Err(Error::blocked(explain::enable_refused(
                            &entry.locator,
                            &sentence,
                        )));
                    }
                    for hit in disabling {
                        // Entries Disabled by one name share its switch.
                        if lifted.iter().any(|known| known.same_switch(&hit)) {
                            continue;
                        }
                        for edit in hit.enable_edits() {
                            add_edit(&mut by_file, hit.switch.path.clone(), edit);
                        }
                        lifted.push(hit);
                    }
                }
            }
        }

        let mut files = Vec::new();
        for (path, edits) in by_file {
            let (read, created) = match self.ports.fs.read(&path) {
                Ok(bytes) => (bytes, false),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    (empty_file(Syntax::Jsonc).to_vec(), true)
                }
                Err(error) => {
                    return Err(Error::blocked(format!(
                        "cannot read {}: {error}",
                        path.display()
                    )));
                }
            };
            let edited = apply_edits(&read, Syntax::Jsonc, &edits)
                .map_err(|failure| failure.into_error(&path))?;
            files.push(self.planned_file(path, Syntax::Jsonc, read, created, edited));
        }

        let written: Vec<(PathBuf, Vec<u8>)> = files
            .iter()
            .map(|file| (file.path.clone(), file.edited.bytes.clone()))
            .collect();
        let after = scan::scan_as_if(
            self.definitions,
            self.ports,
            &self.scan_options.projects,
            &written,
        )
        .map_err(Error::unexpected)?;
        let (changes, effective) = self.reach(&after);
        let wide = self.wide_reach(&changes);
        self.planned(files, changes, effective, wide)
    }

    /// What a write reaches: the States it changes in the context, then in
    /// every other one, each other Project and no Project, as a write to a
    /// switch can apply beyond the context.
    fn reach(&self, after: &Snapshot) -> (Vec<Change>, Vec<EffectiveChange>) {
        let (mut changes, effective) = predict(
            self.before,
            &resolve::resolve(self.definitions, after, self.context),
        );
        let others = self
            .before
            .projects
            .iter()
            .map(|project| Some(project.path.clone()))
            .chain([None])
            .filter(|project| *project != self.before.project);
        for project in others {
            let context = Context { project };
            let before = resolve::resolve(self.definitions, self.snapshot, &context);
            let after = resolve::resolve(self.definitions, after, &context);
            changes.extend(predict(&before, &after).0);
        }
        (changes, effective)
    }

    /// The `wide-reach` warning when the switches written Disable, or lift
    /// for, Entries the action does not target, or apply in other contexts
    /// than its own. An Entry that only wins or loses against one that the
    /// switches reach is not reached itself.
    fn wide_reach(&self, changes: &[Change]) -> Option<Warning> {
        let others: Vec<Reached> = changes
            .iter()
            .filter(|change| {
                change.before == StateAfter::Disabled || change.after == StateAfter::Disabled
            })
            .filter_map(|change| {
                let locator = change.locator.clone();
                if change.project != self.before.project {
                    return Some(match &change.project {
                        Some(project) => Reached::InProject(locator, project.clone()),
                        None => Reached::WithoutProject(locator),
                    });
                }
                let targeted = self.entries.iter().any(|entry| entry.locator == locator);
                (!targeted).then_some(Reached::Here(locator))
            })
            .collect();
        (!others.is_empty()).then(|| Warning {
            class: WarningClass::WideReach,
            sentence: explain::wide_reach(self.action, &others),
        })
    }

    fn planned_file(
        &self,
        path: PathBuf,
        syntax: Syntax,
        read: Vec<u8>,
        created: bool,
        edited: EditedFile,
    ) -> PlannedFile {
        PlannedFile {
            readers: self.snapshot.readers_of(&path),
            switches: self.snapshot.switch_stores_of(&path),
            git_versioned: self.snapshot.versioned.contains(&path),
            path,
            syntax,
            read,
            created,
            edited,
        }
    }

    /// The plan that writes `files`, whose preview shows `changes` and
    /// `effective`, with `wide`, its `wide-reach` warning if it has one.
    fn planned(
        &self,
        files: Vec<PlannedFile>,
        changes: Vec<Change>,
        effective: Vec<EffectiveChange>,
        wide: Option<Warning>,
    ) -> Result<Planned, Error> {
        let title = explain::operation_title(self.action, self.entries.len());
        let preview = preview(
            self.definitions,
            self.ports,
            self.action,
            title,
            None,
            self.before.project.clone(),
            changes,
            effective,
            &files,
            &self.before.read_failures,
            &self.before.homes,
        )?;
        let installed = installations::installed_homes(self.definitions, self.snapshot);
        let running = running::check(self.definitions, self.ports, &installed, &files);
        let mut warnings = warnings(&files, running.warning);
        warnings.extend(wide);
        Ok(Planned {
            action: self.action,
            entries: self.entries.len(),
            context: self.context.clone(),
            scan_options: self.scan_options.clone(),
            warnings,
            blocks: running.blocks,
            files,
            preview,
            read_failures: self.before.read_failures.clone(),
            installed,
        })
    }
}

/// Adds `edit` to the edits of the file at `path`, after those it has.
fn add_edit(by_file: &mut Vec<(PathBuf, Vec<Edit>)>, path: PathBuf, edit: Edit) {
    match by_file.iter_mut().find(|(known, _)| *known == path) {
        Some((_, edits)) => edits.push(edit),
        None => by_file.push((path, vec![edit])),
    }
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
        file.switches = snapshot.switch_stores_of(&file.path);
        file.git_versioned = snapshot.versioned.contains(&file.path);
    }

    let before = resolve::resolve(definitions, &snapshot, &context);
    let written: Vec<(PathBuf, Vec<u8>)> = files
        .iter()
        .map(|file| (file.path.clone(), file.edited.bytes.clone()))
        .collect();
    let after_snapshot = scan(&written)?;
    let after = resolve::resolve(definitions, &after_snapshot, &context);
    let title = explain::undo_title(id, &manifest.title);
    let (mut changes, effective) = predict(&before, &after);
    // What the Operation changed in another Project, such as a leftover it
    // Removed, changes back there.
    let mut others: Vec<&Option<PathBuf>> = Vec::new();
    for change in &manifest.changes {
        if change.project != manifest.project && !others.contains(&&change.project) {
            others.push(&change.project);
        }
    }
    for project in others {
        let context = Context {
            project: project.clone(),
        };
        let before = resolve::resolve(definitions, &snapshot, &context);
        let after = resolve::resolve(definitions, &after_snapshot, &context);
        changes.extend(predict(&before, &after).0);
    }
    let preview = preview(
        definitions,
        ports,
        Action::Undo,
        title,
        Some(id.to_owned()),
        before.project.clone(),
        changes,
        effective,
        &files,
        &before.read_failures,
        &before.homes,
    )?;
    let installed = installations::installed_homes(definitions, &snapshot);
    let running = running::check(definitions, ports, &installed, &files);
    Ok(Planned {
        action: Action::Undo,
        entries: preview.changes.len(),
        context,
        scan_options,
        warnings: warnings(&files, running.warning),
        blocks: running.blocks,
        files,
        preview,
        read_failures: before.read_failures,
        installed,
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
        info: Vec::new(),
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
        blocks: Vec::new(),
        read_failures: Vec::new(),
        installed: BTreeSet::new(),
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

/// An append to a missing list becomes a set of the list holding the item.
/// An insertion or a set whose parent objects are missing becomes a set of
/// the outermost missing one, holding the rest, so that the file gets the
/// minimal structure that holds the key.
fn with_parents(file: &EditableFile, edit: &Edit) -> Result<Edit, EditError> {
    let created;
    let edit = match edit {
        Edit::Append { key_path, value } if file.value_at(key_path)?.is_none() => {
            created = Edit::Set {
                key_path: key_path.clone(),
                value: serde_json::json!([value]),
            };
            &created
        }
        edit => edit,
    };
    let (Edit::Insert {
        key_path, value, ..
    }
    | Edit::Set { key_path, value }) = edit
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
/// check to say whether the Operation can be undone. The files' readers, the
/// switches in them, and whether each is git-versioned, are left for the
/// caller to fill in.
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
                switches: Vec::new(),
                git_versioned: false,
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
    project: Option<PathBuf>,
    changes: Vec<Change>,
    effective: Vec<EffectiveChange>,
    files: &[PlannedFile],
    read_failures: &[ReadFailure],
    homes: &[Home],
) -> Result<Preview, Error> {
    let backup_folder = history::backups_folder(&history::data_folder(ports.env.as_ref())?);
    refuse_read_failure_targets(read_failures, &changes, files)?;
    let mut info = read_failure_info(read_failures, &changes, files);
    info.extend(version_info(definitions, homes, &changes, files));
    Ok(Preview {
        action,
        title,
        undo_of,
        backup_of: None,
        project,
        changes,
        effective,
        info,
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
                FileChange::new(edits, diff, file.git_versioned)
            })
            .collect(),
        deleted: Vec::new(),
        backup_note: explain::backup_note(&backup_folder),
        backup_folder,
    })
}

/// Refuses a plan that must write a file in `files` that is one of
/// `read_failures` for an Agent home whose States it changes in `changes`,
/// or that cannot be opened or decoded at all: config-digger never writes a
/// file it cannot read.
fn refuse_read_failure_targets(
    read_failures: &[ReadFailure],
    changes: &[Change],
    files: &[PlannedFile],
) -> Result<(), Error> {
    for failure in read_failures {
        if !files.iter().any(|file| file.path == failure.path) {
            continue;
        }
        let acting = changes
            .iter()
            .any(|change| change.agent == failure.agent && change.home == failure.home);
        if acting || failure.kind != ReadFailureKind::Parse {
            return Err(Error::blocked(explain::read_failure_target(
                &failure.path,
                &failure.sentence,
            )));
        }
    }
    Ok(())
}

/// What the preview of a plan that writes `files` and changes the States
/// `changes` says about `read_failures`, with nothing to acknowledge: that
/// another Agent still rejects a file it writes, and that a Read failure
/// which could change the outcome may make the States after differ.
fn read_failure_info(
    read_failures: &[ReadFailure],
    changes: &[Change],
    files: &[PlannedFile],
) -> Vec<Info> {
    let mut info = Vec::new();
    for failure in read_failures {
        if files.iter().any(|file| file.path == failure.path) {
            info.push(Info {
                code: InfoCode::StillRejected,
                path: failure.path.clone(),
                sentence: explain::still_rejected(
                    &failure.agent_name,
                    &failure.path,
                    &failure.sentence,
                ),
            });
            continue;
        }
        let could_change = changes.iter().any(|change| {
            change.agent == failure.agent
                && change.home == failure.home
                && failure.reaches(change.project.as_deref())
        });
        if could_change {
            info.push(Info {
                code: InfoCode::ReadFailure,
                path: failure.path.clone(),
                sentence: explain::outcome_may_differ(&failure.agent_name, &failure.path),
            });
        }
    }
    info
}

/// What the preview of a plan that writes `files` and changes the States
/// `changes` says about the versions of the Agents it touches, with nothing
/// to acknowledge: each note of each Agent home among `homes` whose States
/// it changes, or that reads or keeps switches in a file it writes, once.
fn version_info(
    definitions: &[AgentDefinition],
    homes: &[Home],
    changes: &[Change],
    files: &[PlannedFile],
) -> Vec<Info> {
    let readers = files.iter().flat_map(|file| {
        let locations = file
            .readers
            .iter()
            .map(|reader| (reader.agent_index, &reader.home));
        let switches = file
            .switches
            .iter()
            .map(|store| (store.agent_index, &store.home));
        locations
            .chain(switches)
            .map(|(agent_index, home)| (definitions[agent_index].agent.id.as_str(), home.as_str()))
    });
    let touched: Vec<(&str, &str)> = changes
        .iter()
        .map(|change| (change.agent.as_str(), change.home.as_str()))
        .chain(readers)
        .collect();
    let mut info: Vec<Info> = Vec::new();
    for home in homes {
        if !touched.contains(&(home.agent.as_str(), home.home.as_str())) {
            continue;
        }
        for note in &home.notes {
            if info.iter().any(|known| known.sentence == note.sentence) {
                continue;
            }
            info.push(Info {
                code: note.code.into(),
                path: note
                    .installation
                    .clone()
                    .unwrap_or_else(|| home.folder.clone()),
                sentence: note.sentence.clone(),
            });
        }
    }
    info
}

/// What a plan that edits `files` must have acknowledged: one warning for
/// the git-versioned files among them, and `running`, the warning for the
/// running Agents that read them.
fn warnings(files: &[PlannedFile], running: Option<Warning>) -> Vec<Warning> {
    let versioned: Vec<&Path> = files
        .iter()
        .filter(|file| file.git_versioned)
        .map(|file| file.path.as_path())
        .collect();
    let versioned = (!versioned.is_empty()).then(|| Warning {
        class: WarningClass::GitVersioned,
        sentence: explain::git_versioned(&versioned),
    });
    versioned.into_iter().chain(running).collect()
}

/// The syntax the location of an Entry to Remove is edited in. Or why it
/// cannot be Removed: config-digger only reads it, or it is a skill folder.
fn removable_syntax(
    definitions: &[AgentDefinition],
    snapshot: &Snapshot,
    entry: &api::Entry,
) -> Result<Syntax, Error> {
    let scanned = snapshot.entry(&entry.path, &entry.key_path);
    let location = definitions[scanned.agent_index].location(&scanned.location);
    if location.provenance.read_only() {
        return Err(Error::blocked(explain::read_only_remove(&entry.locator)));
    }
    match location.format.file() {
        Some(format) => Ok(format.syntax()),
        None => Err(Error::blocked(explain::skill_remove_not_offered(
            &entry.locator,
        ))),
    }
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
        // A key a switch adds may need parents the file does not have yet.
        let edit = with_parents(&file, edit).map_err(EditFailure::Edit)?;
        let applied = file.apply(&edit).map_err(EditFailure::Edit)?;
        edited = applied.bytes;
        file = EditableFile::decode(&edited, syntax).map_err(EditFailure::ReadOnly)?;
        applied_edits.push(AppliedEdit {
            edit,
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
            let effective_in = |item: &api::Item| {
                item.entries
                    .iter()
                    .find(|entry| entry.state == State::Effective)
                    .map(|entry| entry.locator.clone())
            };
            let before_locator = effective_in(item_before);
            let after_locator = effective_in(item_after);
            effective.push(EffectiveChange {
                agent: item.agent.clone(),
                home: item.home.clone(),
                item: item.name.clone(),
                sentence: explain::effective_after(
                    &item.name,
                    agent_name,
                    after_locator.as_deref(),
                    before_locator == after_locator,
                ),
                before: before_locator,
                after: after_locator,
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
