//! Explain: turns reason codes into the sentences the front-ends show.
//! Definitions carry codes and pointers only, never free text.

use crate::api::{Action, Reason, ReasonCode, Verify};
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
    };
    format!("{verb} {}", count(entries))
}

/// Where the preview says the backup goes.
pub(crate) fn backup_note(folder: &Path) -> String {
    format!(
        "Each touched file is copied to {} before anything is written.",
        folder.display()
    )
}

/// What an executed Operation did: "Removed 2 Entries. Operation
/// 20261001-143012-a3f: verified."
pub(crate) fn outcome(action: Action, entries: usize, operation: &str, verify: Verify) -> String {
    let verb = match action {
        Action::Remove => "Removed",
    };
    let verify = match verify {
        Verify::Verified => "verified",
        Verify::VerifiedWithDifferences => "verified with differences",
    };
    format!(
        "{verb} {}. Operation {operation}: {verify}.",
        count(entries)
    )
}

fn count(entries: usize) -> String {
    match entries {
        1 => "1 Entry".to_owned(),
        n => format!("{n} Entries"),
    }
}
