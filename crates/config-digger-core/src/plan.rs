//! Plan: builds an Operation from an action and the Entries it targets,
//! without writing anything. It computes the edits of every touched file,
//! predicts the States after them, and lists what must be acknowledged.

use crate::api::{
    self, Action, Change, Context, EditSummary, EffectiveChange, Error, FileChange, Inventory,
    Preview, ScanOptions, StateAfter, Targets, Warning,
};
use crate::definitions::AgentDefinition;
use crate::definitions::blocks::Parse;
use crate::edit::{Dependency, Edit, EditError, EditableFile, ReadOnly, Syntax};
use crate::ports::Ports;
use crate::scan::Snapshot;
use crate::{explain, history, resolve, select};
use std::io;
use std::path::{Path, PathBuf};

/// A plan, as [`api::Plan`] holds it.
pub(crate) struct Planned {
    pub action: Action,
    pub context: Context,
    /// What Verify scans with.
    pub scan_options: ScanOptions,
    pub files: Vec<PlannedFile>,
    pub preview: Preview,
    pub warnings: Vec<Warning>,
}

/// The edits to one file.
pub(crate) struct PlannedFile {
    pub path: PathBuf,
    pub syntax: Syntax,
    /// The bytes the edits were computed on.
    pub read: Vec<u8>,
    pub edited: EditedFile,
}

/// A file with every edit of an Operation applied, in order.
pub(crate) struct EditedFile {
    pub bytes: Vec<u8>,
    pub edits: Vec<AppliedEdit>,
    pub diff: String,
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
        let edited = apply_edits(&path, &read, syntax, &edits)
            .map_err(|failure| failure.into_error(&path))?;
        files.push(PlannedFile {
            path,
            syntax,
            read,
            edited,
        });
    }

    let removed: Vec<(PathBuf, Vec<String>)> = targeted
        .iter()
        .map(|entry| (entry.path.clone(), entry.key_path.clone()))
        .collect();
    let after = resolve::resolve(definitions, &snapshot.without(&removed), context);
    let removed_locators: Vec<&str> = targeted
        .iter()
        .map(|entry| entry.locator.as_str())
        .collect();
    let (changes, effective) = predict(&before, &after, &removed_locators);

    let backup_folder = history::data_folder(ports.env.as_ref())
        .map(|data| history::backups_folder(&data))
        .ok_or_else(|| Error::unexpected("cannot determine config-digger's data folder"))?;
    let preview = Preview {
        action,
        project: before.project.clone(),
        changes,
        effective,
        files: files
            .iter()
            .map(|file| FileChange {
                path: file.path.clone(),
                edits: file
                    .edited
                    .edits
                    .iter()
                    .map(|applied| EditSummary::from(&applied.edit))
                    .collect(),
                diff: file.edited.diff.clone(),
            })
            .collect(),
        backup_folder,
    };
    Ok(Planned {
        action,
        context: context.clone(),
        scan_options: scan_options.clone(),
        files,
        preview,
        warnings: Vec::new(),
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
    path: &Path,
    bytes: &[u8],
    syntax: Syntax,
    edits: &[Edit],
) -> Result<EditedFile, EditFailure> {
    let original = EditableFile::decode(bytes, syntax).map_err(EditFailure::ReadOnly)?;
    let mut file = original.clone();
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
        diff: original.unified_diff(&file, &path.display().to_string()),
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

/// The States the Operation changes, and for each Item that keeps Entries,
/// which one is Effective afterwards.
fn predict(
    before: &Inventory,
    after: &Inventory,
    removed: &[&str],
) -> (Vec<Change>, Vec<EffectiveChange>) {
    let mut changes = Vec::new();
    let mut effective = Vec::new();
    for item in &before.items {
        if !item
            .entries
            .iter()
            .any(|entry| removed.contains(&entry.locator.as_str()))
        {
            continue;
        }
        let agent_name = before
            .homes
            .iter()
            .find(|home| home.agent == item.agent)
            .map_or(item.agent.as_str(), |home| home.agent_name.as_str());
        for entry in &item.entries {
            let state_after = if removed.contains(&entry.locator.as_str()) {
                StateAfter::Removed
            } else {
                state_in(after, &item.agent, &item.home, &entry.locator)
            };
            if state_after == StateAfter::from(entry.state) {
                continue;
            }
            changes.push(Change {
                agent: item.agent.clone(),
                agent_name: agent_name.to_owned(),
                home: item.home.clone(),
                item: item.name.clone(),
                path: entry.path.clone(),
                locator: entry.locator.clone(),
                project: before.project.clone(),
                before: entry.state,
                after: state_after,
            });
        }
        let kept = item
            .entries
            .iter()
            .find(|entry| !removed.contains(&entry.locator.as_str()));
        let winner = kept.and_then(|kept| item_in(after, &item.agent, &item.home, &kept.locator));
        if let Some(winner) = winner {
            let before_locator = &item.entries[0].locator;
            let after_locator = &winner.entries[0].locator;
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
