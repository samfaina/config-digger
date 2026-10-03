//! Explain: turns reason codes into the sentences the front-ends show.
//! Definitions carry codes and pointers only, never free text.

use crate::api::{Action, OperationStatus, Reason, ReasonCode, Verify};
use std::path::{Path, PathBuf};

/// A Shadowed Entry's reason, pointing at the Effective Entry `effective`.
/// `tied` says the two have the same precedence, as under two spellings of
/// one Project key, where which one the Agent uses is not known.
pub(crate) fn shadowed_by_precedence(
    agent_name: &str,
    effective_scope: &str,
    effective: &str,
    tied: bool,
) -> Reason {
    let why = if tied {
        format!(
            "which has the same precedence in {agent_name}; which of the two it uses is not known"
        )
    } else {
        format!("which {agent_name} ranks higher")
    };
    Reason {
        code: ReasonCode::ShadowedByPrecedence,
        effective: Some(effective.to_owned()),
        sentence: format!("Shadowed by the {effective_scope} Entry {effective}, {why}."),
    }
}

/// Which Entry of an Item is Effective after an Operation that keeps some of
/// its Entries. `unchanged` says it was Effective before too.
pub(crate) fn effective_after(
    item: &str,
    agent_name: &str,
    effective: &str,
    unchanged: bool,
) -> String {
    let verb = if unchanged { "stays" } else { "becomes" };
    format!("{effective} {verb} Effective for {item} in {agent_name}.")
}

/// What an Operation does, as a title: "Remove 2 Entries".
pub(crate) fn operation_title(action: Action, entries: usize) -> String {
    let verb = match action {
        Action::Disable => "Disable",
        Action::Remove => "Remove",
        Action::Undo => "Undo",
        Action::DeleteBackup => "Delete the backup of",
    };
    format!("{verb} {}", count(entries))
}

/// The title of an Undo: `Undo 20261001-143012-a3f "Remove 1 Entry"`.
pub(crate) fn undo_title(operation: &str, title: &str) -> String {
    format!("Undo {operation} \"{title}\"")
}

/// The title of a Delete backup: `Delete the backup of 20261001-143012-a3f
/// "Remove 1 Entry"`.
pub(crate) fn delete_backup_title(operation: &str, title: &str) -> String {
    format!("Delete the backup of {operation} \"{title}\"")
}

/// Where the preview says the backup goes.
pub(crate) fn backup_note(folder: &Path) -> String {
    format!(
        "Each touched file is copied to {} before anything is written.",
        folder.display()
    )
}

/// The warning of a plan that edits the git-versioned `files`.
pub(crate) fn git_versioned(files: &[&Path]) -> String {
    let verb = if files.len() == 1 { "is" } else { "are" };
    format!(
        "{} {verb} git-versioned: this change will show in the git diff, and reaches every clone once committed. config-digger never stages or commits it.",
        path_list(files)
    )
}

/// What Disable would write for the Entry `name`: add it to the list `list`
/// in `path`, for the context's Project when `per_project`.
pub(crate) fn disable_target(name: &str, list: &str, path: &Path, per_project: bool) -> String {
    let scope = if per_project { " for this Project" } else { "" };
    format!(
        "Disable adds `{name}` to `{list}` in {}{scope}.",
        path.display()
    )
}

/// Why Disable is not offered when every switch of `agent_name` that could
/// Disable the Entry is in one of the git-versioned `files`. `needs_project`
/// says it also has switches per Project, and there is no Project.
pub(crate) fn disable_only_in_versioned(
    agent_name: &str,
    files: &[PathBuf],
    needs_project: bool,
) -> String {
    let files: Vec<&Path> = files.iter().map(PathBuf::as_path).collect();
    let also = if needs_project {
        " Its other switches apply per Project, and there is no Project in this context."
    } else {
        ""
    };
    format!(
        "Disable is not offered: {agent_name}'s switches for this Entry are in git-versioned files ({}), where a change reaches every clone.{also}",
        path_list(&files)
    )
}

/// Why Disable is not offered when `agent_name` has no switch for the Entry
/// in the context. `needs_project` says it has one per Project.
pub(crate) fn no_disable_mechanism(agent_name: &str, needs_project: bool) -> String {
    if needs_project {
        format!(
            "Disable is not offered: {agent_name} Disables this Entry per Project, and there is no Project in this context."
        )
    } else {
        format!("Disable is not offered: {agent_name} has no switch for this Entry here.")
    }
}

/// Paths as a sentence lists them, separated by commas.
fn path_list(paths: &[&Path]) -> String {
    let names: Vec<String> = paths
        .iter()
        .map(|path| path.display().to_string())
        .collect();
    names.join(", ")
}

/// What the preview of a Delete backup says in place of the backup note.
pub(crate) fn delete_backup_note(operation: &str) -> String {
    format!(
        "Nothing is copied first: the backup is deleted for good, with the values of the Operation's edits in History, and Operation {operation} can no longer be undone."
    )
}

/// The check a Delete backup adds when its Operation needs attention.
pub(crate) fn only_copy_for_recovery(operation: &str) -> String {
    format!(
        "Operation {operation} needs attention: this backup is the only copy for manual recovery of the files its Rollback could not restore. Deleting it also dismisses the Operation."
    )
}

/// What an executed Operation did: "Removed 2 Entries. Operation
/// 20261001-143012-a3f: verified.", or for an Undo of the Operation
/// `undo_of`: "Undid 20261001-143012-a3f. Operation …: verified." A Delete
/// backup of the Operation `operation` says so and that it is not undoable.
pub(crate) fn outcome(
    action: Action,
    entries: usize,
    undo_of: Option<&str>,
    operation: &str,
    verify: Verify,
) -> String {
    let done = match action {
        Action::Disable => format!("Disabled {}", count(entries)),
        Action::Remove => format!("Removed {}", count(entries)),
        Action::Undo => format!("Undid {}", undo_of.unwrap_or("an Operation")),
        Action::DeleteBackup => {
            return format!("Deleted the backup of {operation}. It can no longer be undone.");
        }
    };
    let verify = match verify {
        Verify::Verified => "verified",
        Verify::VerifiedWithDifferences => "verified with differences",
    };
    format!("{done}. Operation {operation}: {verify}.")
}

/// An Operation's status as History shows it: "Applied", "Undone by …",
/// "Undo of …", or both of the last two for an Undo that was undone;
/// "Failed, rolled back" or "Needs attention" for a failed one; then
/// ", backup deleted, not undoable" once its backup was.
pub(crate) fn operation_status(
    status: OperationStatus,
    dismissed: bool,
    undo_of: Option<&str>,
    undone_by: Option<&str>,
    backup_deleted: bool,
) -> String {
    let status = match (status, undo_of, undone_by) {
        (OperationStatus::InProgress, ..) => "Incomplete".to_owned(),
        (OperationStatus::Failed, ..) => "Failed, rolled back".to_owned(),
        (OperationStatus::NeedsAttention, ..) if dismissed => {
            "Rollback incomplete, dismissed".to_owned()
        }
        (OperationStatus::NeedsAttention, ..) => "Needs attention".to_owned(),
        (OperationStatus::Applied, None, None) => "Applied".to_owned(),
        (OperationStatus::Applied, Some(undone), None) => format!("Undo of {undone}"),
        (OperationStatus::Applied, None, Some(undo)) => format!("Undone by {undo}"),
        (OperationStatus::Applied, Some(undone), Some(undo)) => {
            format!("Undo of {undone}, undone by {undo}")
        }
    };
    if backup_deleted {
        return format!("{status}, backup deleted, not undoable");
    }
    status
}

/// Why an Operation with this status cannot be undone, or `None` for an
/// applied one whose backup was not deleted.
pub(crate) fn not_undoable(status: OperationStatus, backup_deleted: bool) -> Option<String> {
    if backup_deleted {
        return Some("Its backup was deleted, with the values its Undo would put back.".to_owned());
    }
    let reason = match status {
        OperationStatus::Applied => return None,
        OperationStatus::InProgress => "It did not complete; recover its files from its backup.",
        OperationStatus::Failed => "It failed and was rolled back, so there is nothing to undo.",
        OperationStatus::NeedsAttention => {
            "Its Rollback is incomplete; recover its files from its backup by hand."
        }
    };
    Some(reason.to_owned())
}

/// Why an Operation that was undone cannot be undone again.
pub(crate) fn already_undone(undo: &str) -> String {
    format!("It was undone by {undo}; undo that Operation to apply it again.")
}

fn count(entries: usize) -> String {
    match entries {
        1 => "1 Entry".to_owned(),
        n => format!("{n} Entries"),
    }
}
