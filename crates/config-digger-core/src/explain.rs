//! Explain: turns reason codes into the sentences the front-ends show.
//! Definitions carry codes and pointers only, never free text.

use crate::api::{Action, OperationStatus, Reason, ReasonCode, Verify};
use std::path::Path;

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
        Action::Remove => "Remove",
        Action::Undo => "Undo",
    };
    format!("{verb} {}", count(entries))
}

/// The title of an Undo: `Undo 20261001-143012-a3f "Remove 1 Entry"`.
pub(crate) fn undo_title(operation: &str, title: &str) -> String {
    format!("Undo {operation} \"{title}\"")
}

/// Where the preview says the backup goes.
pub(crate) fn backup_note(folder: &Path) -> String {
    format!(
        "Each touched file is copied to {} before anything is written.",
        folder.display()
    )
}

/// What an executed Operation did: "Removed 2 Entries. Operation
/// 20261001-143012-a3f: verified.", or for an Undo of the Operation
/// `undo_of`: "Undid 20261001-143012-a3f. Operation …: verified."
pub(crate) fn outcome(
    action: Action,
    entries: usize,
    undo_of: Option<&str>,
    operation: &str,
    verify: Verify,
) -> String {
    let done = match action {
        Action::Remove => format!("Removed {}", count(entries)),
        Action::Undo => format!("Undid {}", undo_of.unwrap_or("an Operation")),
    };
    let verify = match verify {
        Verify::Verified => "verified",
        Verify::VerifiedWithDifferences => "verified with differences",
    };
    format!("{done}. Operation {operation}: {verify}.")
}

/// An Operation's status as History shows it: "Applied", "Undone by …",
/// "Undo of …", or both of the last two for an Undo that was undone;
/// "Failed, rolled back" or "Needs attention" for a failed one.
pub(crate) fn operation_status(
    status: OperationStatus,
    dismissed: bool,
    undo_of: Option<&str>,
    undone_by: Option<&str>,
) -> String {
    match (status, undo_of, undone_by) {
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
    }
}

/// Why an Operation with this status cannot be undone, or `None` for an
/// applied one.
pub(crate) fn not_undoable(status: OperationStatus) -> Option<String> {
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
