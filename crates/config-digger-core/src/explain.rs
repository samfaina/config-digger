//! Explain: turns reason codes into the sentences the front-ends show.
//! Definitions carry codes and pointers only, never free text.

use crate::api::{
    Action, OperationStatus, ReadFailureKind, Reason, ReasonCode, RiskCode, State, Verify,
};
use crate::definitions::blocks::{FieldType, Malformed};
use crate::edit::Edit;
use serde_json::Value;
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
        switch: None,
        sentence: format!("Shadowed by the {effective_scope} Entry {effective}, {why}."),
    }
}

/// Which Entry of an Item is Effective after an Operation that keeps some of
/// its Entries, if any. `unchanged` says it was Effective before too.
pub(crate) fn effective_after(
    item: &str,
    agent_name: &str,
    effective: Option<&str>,
    unchanged: bool,
) -> String {
    let Some(effective) = effective else {
        return format!("No Entry is Effective for {item} in {agent_name} afterwards.");
    };
    let verb = if unchanged { "stays" } else { "becomes" };
    format!("{effective} {verb} Effective for {item} in {agent_name}.")
}

/// What an Operation does, as a title: "Remove 2 Entries".
pub(crate) fn operation_title(action: Action, entries: usize) -> String {
    let verb = match action {
        Action::Disable => "Disable",
        Action::Enable => "Enable",
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

/// The block of a plan that writes the SQLite stores `files` while
/// `agent_name`, which holds them open, runs.
pub(crate) fn running_agent_sqlite(agent_name: &str, files: &[&Path]) -> String {
    format!(
        "{agent_name} is running and holds {} open in SQLite, so nothing can be written there while it runs. Close {agent_name} and retry.",
        path_list(files)
    )
}

/// The strong warning of a plan that writes `files`, which `agent_name`
/// rewrites while it runs.
pub(crate) fn running_agent_rewrites(agent_name: &str, files: &[&Path]) -> String {
    format!(
        "{agent_name} is running and rewrites {} while it runs: it may overwrite what this Operation writes. Close {agent_name} first to be safe.",
        path_list(files)
    )
}

/// The warning of a plan that writes `files`, which the running
/// `agent_name` reads.
pub(crate) fn running_agent_reads(agent_name: &str, files: &[&Path]) -> String {
    format!(
        "{agent_name} is running and reads {}: it may not see what this Operation writes until it restarts.",
        path_list(files)
    )
}

/// Why an Operation needs a fresh preview when the Agents `agent_names`,
/// which read what it writes, started since its preview.
pub(crate) fn agent_started(agent_names: &[String]) -> String {
    format!(
        "{} started since the preview, which did not warn about it",
        agent_names.join(", ")
    )
}

/// What the result says to do once a running `agent_name` reads a written file.
pub(crate) fn restart_hint(agent_name: &str) -> String {
    format!("Restart {agent_name} so that it reads what the Operation wrote.")
}

/// What the result says of a written file `path` that the running
/// `agent_name` rewrites while it runs.
pub(crate) fn may_be_overwritten(path: &Path, agent_name: &str) -> String {
    format!(
        "{} may be overwritten while {agent_name} runs.",
        path.display()
    )
}

/// Where a switch applies, after the file it is in.
fn for_project(per_project: bool) -> &'static str {
    if per_project { " for this Project" } else { "" }
}

/// What Disable writes for the Entry `name`: `edit` on the switch in
/// `path`, which applies in the context's Project only when `per_project`.
pub(crate) fn disable_switch(edit: &Edit, name: &str, path: &Path, per_project: bool) -> String {
    let scope = for_project(per_project);
    let path = path.display();
    match edit {
        Edit::Append { key_path, .. } => {
            format!(
                "Disable adds `{name}` to `{}` in {path}{scope}.",
                last(key_path)
            )
        }
        edit => format!("Disable {}{scope}.", describe(edit, &path.to_string())),
    }
}

/// What Enable writes for the Entry `name`: `edits` on the switch in
/// `path`, which applies in the context's Project only when `per_project`.
pub(crate) fn enable_switch(edits: &[Edit], name: &str, path: &Path, per_project: bool) -> String {
    let scope = for_project(per_project);
    let path = path.display();
    match edits.first() {
        Some(Edit::RemoveItem { key_path, .. }) => {
            format!(
                "Enable removes `{name}` from `{}` in {path}{scope}.",
                last(key_path)
            )
        }
        Some(edit) => format!("Enable {}{scope}.", describe(edit, &path.to_string())),
        None => format!("Enable leaves {path} as it is."),
    }
}

/// An edit of a switch as a sentence says it, after its verb's subject.
fn describe(edit: &Edit, path: &str) -> String {
    let key = edit.key_path().join(".");
    match edit {
        Edit::Set { value, .. } | Edit::Insert { value, .. } => {
            format!("sets `{key}` to {value} in {path}")
        }
        Edit::Remove { .. } => format!("deletes `{key}` in {path}"),
        Edit::Append { value, .. } => format!("adds {value} to `{key}` in {path}"),
        Edit::RemoveItem { value, .. } => format!("removes {value} from `{key}` in {path}"),
    }
}

fn last(key_path: &[String]) -> &str {
    key_path.last().map_or("", String::as_str)
}

/// That the list `list` in `path` holds `name`.
pub(crate) fn in_list(name: &str, list: &str, path: &Path, per_project: bool) -> String {
    format!(
        "`{name}` is in `{list}` in {}{}",
        path.display(),
        for_project(per_project)
    )
}

/// That the field at `key_path` in `path` holds `value`.
pub(crate) fn field_holds(
    key_path: &[String],
    value: Option<&Value>,
    path: &Path,
    per_project: bool,
) -> String {
    let value = value.map_or_else(|| "missing".to_owned(), Value::to_string);
    format!(
        "`{}` is {value} in {}{}",
        key_path.join("."),
        path.display(),
        for_project(per_project)
    )
}

/// A Disabled Entry's reason, when the switch `holds` names its Item.
pub(crate) fn disabled_by_name(holds: &str, agent_name: &str) -> String {
    format!("Disabled by name: {holds}, so {agent_name} skips the Entries with that name.")
}

/// A Disabled Entry's reason, when the switch `holds` is its own.
pub(crate) fn disabled_natively(holds: &str, agent_name: &str) -> String {
    format!("Disabled: {holds}, so {agent_name} skips it.")
}

/// The note on an Entry part of which the partial switch `holds` limits.
pub(crate) fn partial_switch(holds: &str, agent_name: &str) -> String {
    format!(
        "Partial switch: {holds}. {agent_name} still uses the Entry, with only part of it available."
    )
}

/// The State a Disabled Entry would have once Enabled: Effective, or
/// Shadowed by the `scope` Entry `effective`.
pub(crate) fn what_if(state: State, shadowed_by: Option<(&str, &str)>) -> String {
    match shadowed_by {
        None if state == State::Invalid => "Disabled: would be Invalid once Enabled.".to_owned(),
        None => "Disabled: would be Effective once Enabled.".to_owned(),
        Some((scope, effective)) => {
            format!("Disabled: would be Shadowed by the {scope} Entry {effective} once Enabled.")
        }
    }
}

/// An Entry, by locator, that a switch reaches beyond what an action targets.
pub(crate) enum Reached {
    /// In the action's own context.
    Here(String),
    /// In another Project.
    InProject(String, PathBuf),
    /// With no Project.
    WithoutProject(String),
}

/// The warning of a plan whose switches reach the Entries `others` beyond
/// those it targets.
pub(crate) fn wide_reach(action: Action, others: &[Reached]) -> String {
    let verb = match action {
        Action::Enable => "Enable",
        _ => "Disable",
    };
    let rows: Vec<String> = others
        .iter()
        .map(|reached| match reached {
            Reached::Here(locator) => locator.clone(),
            Reached::InProject(locator, project) => {
                format!("{locator} in Project {}", project.display())
            }
            Reached::WithoutProject(locator) => format!("{locator} with no Project"),
        })
        .collect();
    format!(
        "This {verb} reaches more than what it targets: the switch it writes also changes the State of {}.",
        rows.join(", ")
    )
}

/// Why Disable is refused for an Entry it is not offered for: `sentence`.
pub(crate) fn disable_not_offered(locator: &str, sentence: &str) -> String {
    format!("cannot Disable {locator}: {sentence}")
}

/// Why Disable is refused for an Entry that is Disabled already.
pub(crate) fn already_disabled(locator: &str) -> String {
    format!("cannot Disable {locator}: it is Disabled already")
}

/// What Disable says of an Entry that is Disabled already.
pub(crate) fn already_disabled_option() -> String {
    "Disable is not offered: the Entry is Disabled already.".to_owned()
}

/// What Enable says of a Disabled Entry when the switch `holds` is one
/// config-digger only reads.
pub(crate) fn enable_not_offered(holds: &str) -> String {
    format!(
        "Enable is not offered: {holds}, which config-digger does not write; take the name out of it there by hand."
    )
}

/// Why Enable is refused for an Entry it is not offered for: `sentence`.
pub(crate) fn enable_refused(locator: &str, sentence: &str) -> String {
    format!("cannot Enable {locator}: {sentence}")
}

/// Why Enable is refused for an Entry that is not Disabled.
pub(crate) fn not_disabled(locator: &str) -> String {
    format!("cannot Enable {locator}: it is not Disabled")
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

/// Why Disable is not offered when every switch of `agent_name` that could
/// Disable the Entry is in one of `files`, which cannot be read.
pub(crate) fn disable_only_in_unreadable(agent_name: &str, files: &[PathBuf]) -> String {
    let files: Vec<&Path> = files.iter().map(PathBuf::as_path).collect();
    format!(
        "Disable is not offered: {agent_name}'s switches for this Entry are in files config-digger cannot read ({}), so it cannot write them safely.",
        path_list(&files)
    )
}

/// Why Disable is not offered when `agent_name` has no switch for the Entry
/// in the context. `needs_project` says it has one per Project, and there is
/// no Project; `unrecorded` says it has one per Project, and nowhere to keep
/// it for this one.
pub(crate) fn no_disable_mechanism(
    agent_name: &str,
    needs_project: bool,
    unrecorded: bool,
) -> String {
    if unrecorded {
        format!(
            "Disable is not offered: {agent_name} Disables this Entry per Project, and has nowhere to keep the switch for this Project: it has not recorded the Project, or its folder is gone."
        )
    } else if needs_project {
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
        Action::Enable => format!("Enabled {}", count(entries)),
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

/// What a Read failure of `kind` at `path` means for `agent_name`, which
/// reads the file in `dialect`. It never quotes the file.
pub(crate) fn read_failure(
    agent_name: &str,
    path: &Path,
    kind: ReadFailureKind,
    dialect: &str,
    position: Option<(usize, usize)>,
) -> String {
    let path = path.display();
    match kind {
        ReadFailureKind::Parse => {
            let at = match position {
                Some((line, column)) => format!(" (line {line}, column {column})"),
                None => String::new(),
            };
            format!(
                "{agent_name} cannot parse {path} as {dialect}{at}, so it reads no Entries from it."
            )
        }
        ReadFailureKind::Decode => {
            format!("{path} is not text {agent_name} can decode, so it reads no Entries from it.")
        }
        ReadFailureKind::Permission => format!(
            "config-digger has no permission to read {path}, so it lists no {agent_name} Entries from it."
        ),
        ReadFailureKind::Io | ReadFailureKind::Sqlite => format!(
            "config-digger cannot read {path}, so it lists no {agent_name} Entries from it."
        ),
    }
}

/// The note of a Read failure in a file `agent_name` asks about at startup.
pub(crate) fn asks_user(agent_name: &str) -> String {
    format!("{agent_name} will ask about this file at startup.")
}

/// The warning of a Read failure in a file whose recovery by `agent_name`
/// can lose configuration.
pub(crate) fn risk(agent_name: &str, code: RiskCode) -> String {
    let what = match code {
        RiskCode::ResetFile => "it may reset the file, losing what it holds",
        RiskCode::DropEntries => "it may write the file again without its Entries",
        RiskCode::ClearAuth => "it may clear the sign-in of the servers it no longer sees",
    };
    format!("Fix this before starting {agent_name}: {what}.")
}

/// The flag of an Agent that refuses to start because it cannot read `path`.
pub(crate) fn cannot_start(agent_name: &str, path: &Path) -> String {
    format!(
        "{agent_name} won't start: it cannot read {}.",
        path.display()
    )
}

/// The caveat of an Item when a location at `path` that could take part
/// cannot be read, and what `agent_name` does then is not known.
pub(crate) fn unreadable_location(agent_name: &str, path: &Path) -> String {
    format!(
        "A location that could take part couldn't be read: {}, so the State shown may not be the one {agent_name} has.",
        path.display()
    )
}

/// An Invalid Entry's reason: why it lacks the minimal shape `agent_name`
/// requires. `competes` says the Agent still lists it, so that it competes
/// by name.
pub(crate) fn malformed_entry(agent_name: &str, malformed: &Malformed, competes: bool) -> Reason {
    let why = match malformed {
        Malformed::NotAnObject => "it is not an object".to_owned(),
        Malformed::Missing(fields) => {
            let fields: Vec<String> = fields.iter().map(|field| format!("`{field}`")).collect();
            let fields = match fields.split_last() {
                Some((last, [])) => last.clone(),
                Some((last, rest)) => format!("{} or {last}", rest.join(", ")),
                None => String::new(),
            };
            format!("it needs {fields}")
        }
        Malformed::WrongType(field, kind) => {
            let kind = match kind {
                FieldType::String => "a string",
                FieldType::Number => "a number",
                FieldType::Boolean => "true or false",
                FieldType::Array => "a list",
                FieldType::Object => "an object",
                FieldType::StringArray => "a list of strings",
                FieldType::StringMap => "an object of strings",
            };
            format!("`{field}` must be {kind}")
        }
    };
    let sentence = if competes {
        format!("{agent_name} cannot use this Entry: {why}. It still competes by name.")
    } else {
        format!("{agent_name} skips this Entry: {why}.")
    };
    Reason {
        code: ReasonCode::MalformedEntry,
        effective: None,
        switch: None,
        sentence,
    }
}

/// The caveat of an Entry when the file at `path`, holding a switch that
/// could Disable it, cannot be read.
pub(crate) fn unreadable_switch(path: &Path) -> String {
    format!(
        "{} couldn't be read, so whether it Disables this Entry is not known: it is shown as if it did not.",
        path.display()
    )
}

/// The info row of a plan that writes `path`, which `agent_name` rejects
/// before and after it, as `failure` says.
pub(crate) fn still_rejected(agent_name: &str, path: &Path, failure: &str) -> String {
    format!(
        "{agent_name} still rejects {} after this change: {failure}",
        path.display()
    )
}

/// The info row of a plan whose States after could differ, since `path`,
/// which `agent_name` could read, cannot be read.
pub(crate) fn outcome_may_differ(agent_name: &str, path: &Path) -> String {
    format!(
        "{agent_name} couldn't read {}, so its States after this change may differ.",
        path.display()
    )
}

/// Why a plan that must write `path`, a Read failure as `failure` says, is
/// refused.
pub(crate) fn read_failure_target(path: &Path, failure: &str) -> String {
    format!(
        "{} is a Read failure, and config-digger never writes a file it cannot read: {failure}",
        path.display()
    )
}
