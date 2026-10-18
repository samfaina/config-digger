//! Explain: turns reason codes into the sentences the front-ends show.
//! Definitions carry codes and pointers only, never free text.

use crate::api::{
    Action, FolderChangeKind, OperationStatus, ReadFailureKind, Reason, ReasonCode, RiskCode,
    State, Verify,
};
use crate::definitions::ItemKind;
use crate::definitions::blocks::{
    ApprovalPrompt, FieldType, Malformed, MatchedBy, SettingIs, SkillDefect, UnseenServers,
};
use crate::edit::Edit;
use crate::folders::{FolderOp, FolderSize};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// A Shadowed Entry's reason, pointing at the Effective Entry `effective`.
/// `tied` says the two have the same precedence, as under two spellings of
/// one Project key, where which one the Agent uses is not known.
/// `same_endpoint` says they compete because they have the same endpoint.
pub(crate) fn shadowed_by_precedence(
    agent_name: &str,
    effective_scope: &str,
    effective: &str,
    tied: bool,
    same_endpoint: bool,
) -> Reason {
    let endpoint = if same_endpoint {
        "has the same endpoint and "
    } else {
        ""
    };
    let why = if tied {
        format!(
            "which {endpoint}has the same precedence in {agent_name}; which of the two it uses is not known"
        )
    } else if same_endpoint {
        format!("which {endpoint}{agent_name} ranks higher")
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

/// The fields `fields`, each named by its key path joined with `.`, as a
/// sentence lists them.
fn field_list<S: AsRef<str>>(fields: &[S]) -> String {
    let fields: Vec<String> = fields
        .iter()
        .map(|field| format!("`{}`", field.as_ref()))
        .collect();
    and_list(&fields)
}

/// `words` as a sentence lists them: separated by commas, the last by "and".
fn and_list(words: &[String]) -> String {
    match words.split_last() {
        None => String::new(),
        Some((last, [])) => last.clone(),
        Some((last, rest)) => format!("{} and {last}", rest.join(", ")),
    }
}

/// A Merged Entry's reason: `agent_name` builds the server `name` from
/// `count` same-named Entries, and this one supplies `fields`.
pub(crate) fn merged(agent_name: &str, name: &str, count: usize, fields: &[Vec<String>]) -> Reason {
    let supplies = if fields.is_empty() {
        "every field of this one comes from a higher Entry".to_owned()
    } else {
        let fields: Vec<String> = fields.iter().map(|field| field.join(".")).collect();
        format!("this one supplies {}", field_list(&fields))
    };
    Reason {
        code: ReasonCode::Merged,
        effective: None,
        switch: None,
        sentence: format!(
            "Merged: {agent_name} builds `{name}` from {count} Entries, field by field; {supplies}."
        ),
    }
}

/// The note on an Entry that lies underneath the Entry at `over`: the
/// Agent's own `command` lists the server built from both as that one alone.
pub(crate) fn cli_view_differs(agent_name: &str, command: &str, over: &str) -> String {
    format!(
        "`{command}` lists this server as {over} alone and hides this Entry, though {agent_name} builds the server it runs from both."
    )
}

/// An Invalid Entry's reason: `agent_name` refuses the plugin `plugin` it
/// is part of, since another of its plugins has that name.
pub(crate) fn duplicate_plugin_name(agent_name: &str, plugin: &str) -> Reason {
    Reason {
        code: ReasonCode::DuplicatePluginName,
        effective: None,
        switch: None,
        sentence: format!(
            "Invalid: {agent_name} refuses to load `{plugin}`, since another it installed has that name."
        ),
    }
}

/// An Invalid Entry's reason: the server `agent_name` builds from the
/// Entries of `name` holds `fields`, which exclude each other.
pub(crate) fn transport_conflict(agent_name: &str, name: &str, fields: &[&str]) -> Reason {
    Reason {
        code: ReasonCode::TransportConflict,
        effective: None,
        switch: None,
        sentence: format!(
            "Invalid: {agent_name} merges the Entries of `{name}` into a server with both {}, which it rejects, so it won't start.",
            field_list(fields)
        ),
    }
}

/// The flag of an Agent that refuses to start because the server it builds
/// from the Entries of `name` holds `fields`, which exclude each other.
pub(crate) fn cannot_start_conflict(agent_name: &str, name: &str, fields: &[&str]) -> String {
    format!(
        "{agent_name} won't start: it merges the Entries of `{name}` into a server with both {}.",
        field_list(fields)
    )
}

/// A Disabled Entry's reason: `field` is `false` in the server
/// `agent_name` builds from the Entries of `name`, set at `switch`.
pub(crate) fn turned_off(agent_name: &str, name: &str, field: &str, switch: &str) -> Reason {
    Reason {
        code: ReasonCode::DisabledNatively,
        effective: None,
        switch: Some(switch.to_owned()),
        sentence: format!(
            "Disabled: `{field} = false` in {switch} turns off the server {agent_name} builds from every Entry of `{name}`."
        ),
    }
}

/// What Enable says of an Entry Disabled by `field = false` at `switch`
/// in the server its Agent builds.
pub(crate) fn turned_off_enable(field: &str, switch: &str) -> String {
    format!(
        "Enable is not offered: config-digger does not write `{field}` of a merged server; change it at {switch} by hand."
    )
}

/// An Unread Entry's reason: it is keyed to `project`, whose folder no
/// longer exists.
pub(crate) fn project_missing(agent_name: &str, project: &Path) -> Reason {
    Reason {
        code: ReasonCode::ProjectMissing,
        effective: None,
        switch: None,
        sentence: format!(
            "{agent_name} never reads it: it is keyed to the Project {}, whose folder no longer exists. It can only be Removed.",
            project.display()
        ),
    }
}

/// Why `agent_name` never reads the Agent home at `folder`: its lookup
/// order picks the folder `instead`, or none that is there.
pub(crate) fn home_not_read(agent_name: &str, folder: &Path, instead: Option<&Path>) -> String {
    match instead {
        Some(instead) => format!(
            "{agent_name} never reads {}: its lookup order picks {} first.",
            folder.display(),
            instead.display()
        ),
        None => format!(
            "{agent_name} never reads {} on this machine: its lookup order skips it.",
            folder.display()
        ),
    }
}

/// An Unread Entry's reason: it is in the Agent home at `folder`, which
/// `agent_name`'s lookup order skips for `instead`, or for none.
pub(crate) fn lookup_order_skips_home(
    agent_name: &str,
    folder: &Path,
    instead: Option<&Path>,
) -> Reason {
    Reason {
        code: ReasonCode::LookupOrderSkipsHome,
        effective: None,
        switch: None,
        sentence: format!(
            "{} It can only be Removed.",
            home_not_read(agent_name, folder, instead)
        ),
    }
}

/// An Unread Linked Entry's reason: its link points to `target`, which is gone.
pub(crate) fn link_broken(agent_name: &str, target: &Path) -> Reason {
    Reason {
        code: ReasonCode::LinkBroken,
        effective: None,
        switch: None,
        sentence: format!(
            "{agent_name} reads nothing here: the link points to {}, which is gone. It can only be Removed.",
            target.display()
        ),
    }
}

/// An Unread Entry's reason: it is in a source of `agent_name`'s settings
/// `settings` that it skips, since it reads them only from `read`, the
/// first that holds any.
pub(crate) fn source_skipped(agent_name: &str, settings: &str, read: &Path) -> Reason {
    Reason {
        code: ReasonCode::SourceSkipped,
        effective: None,
        switch: None,
        sentence: format!(
            "{agent_name} never reads it: it reads its {settings} settings only from the first source that holds any, which is {}.",
            read.display()
        ),
    }
}

/// An Unread Entry's reason: `agent_name` skips the system file at `path`
/// unread, since someone other than an administrator can change it or a
/// folder above it.
pub(crate) fn system_file_not_admin_owned(agent_name: &str, path: &Path) -> Reason {
    Reason {
        code: ReasonCode::SystemFileNotAdminOwned,
        effective: None,
        switch: None,
        sentence: format!(
            "{agent_name} never reads it: it skips {} with a security warning, since someone other than an administrator can change it or a folder above it.",
            path.display()
        ),
    }
}

/// What Disable says of an Unread Entry.
pub(crate) fn unread_option() -> String {
    "Disable is not offered: the Agent never reads the Entry, so it can only be Removed.".to_owned()
}

/// Why Disable or Enable, `action`, is refused for the Unread Entry at `locator`.
pub(crate) fn only_removed(action: Action, locator: &str) -> String {
    let verb = match action {
        Action::Disable => "Disable",
        _ => "Enable",
    };
    format!("cannot {verb} {locator}: the Agent never reads it, so it can only be Removed")
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
    format!("{} {}", action_verb(action), count(entries))
}

/// The verb an action is said by, such as "Remove".
pub(crate) fn action_verb(action: Action) -> &'static str {
    match action {
        Action::Disable => "Disable",
        Action::Enable => "Enable",
        Action::Remove => "Remove",
        Action::Undo => "Undo",
        Action::DeleteBackup => "Delete the backup of",
    }
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

/// What Disable writes for the Entry `name`: `edits` on the switch in
/// `path`, which applies in the context's Project only when `per_project`.
pub(crate) fn disable_switch(edits: &[Edit], name: &str, path: &Path, per_project: bool) -> String {
    let scope = for_project(per_project);
    let path = path.display();
    // An item replaced in a list is said by what it adds.
    match edits.last().expect("Disable writes a switch") {
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

/// That the list `list` in `path` holds `item` for the Entry `id`.
pub(crate) fn item_in_list(
    id: &str,
    item: &Value,
    list: &str,
    path: &Path,
    per_project: bool,
) -> String {
    format!(
        "`{list}` in {}{} holds {item} for `{id}`",
        path.display(),
        for_project(per_project)
    )
}

/// That the list `list` in `path` holds `name`.
pub(crate) fn in_list(name: &str, list: &str, path: &Path, per_project: bool) -> String {
    format!(
        "`{name}` is in `{list}` in {}{}",
        path.display(),
        for_project(per_project)
    )
}

/// That the field at `key_path` in `path` holds `value`, or is missing,
/// which the Agent takes as `default`.
pub(crate) fn field_holds(
    key_path: &[String],
    value: Option<&Value>,
    default: Option<&Value>,
    path: &Path,
    per_project: bool,
) -> String {
    let (value, counts_as) = match (value, default) {
        (Some(value), _) => (value.to_string(), String::new()),
        (None, Some(default)) => ("missing".to_owned(), format!(", which counts as {default}")),
        (None, None) => ("missing".to_owned(), String::new()),
    };
    format!(
        "`{}` is {value} in {}{}{counts_as}",
        key_path.join("."),
        path.display(),
        for_project(per_project)
    )
}

/// That `rule`, in the list of path rules at `key_path` in `path`, is the
/// last that reaches the folder of the context's Project, `folder`.
pub(crate) fn path_rule_holds(
    rule: &str,
    key_path: &[String],
    path: &Path,
    folder: Option<&Path>,
) -> String {
    let folder = folder.map_or_else(String::new, |folder| format!(" {}", folder.display()));
    format!(
        "`{rule}` in `{}` in {} is the last rule that reaches the Project folder{folder}",
        key_path.join("."),
        path.display()
    )
}

/// What Disable writes in a list of path rules in `path`: `edits`, which
/// take the Project's own rules out and add one for it and the folders
/// below it.
pub(crate) fn disable_path_rule(edits: &[Edit], path: &Path) -> String {
    format!(
        "Disable {}, a rule for this Project and the folders below it.",
        describe_all(edits, path)
    )
}

/// What Enable writes in a list of path rules in `path`: `edits`.
pub(crate) fn enable_path_rules(edits: &[Edit], path: &Path) -> String {
    if edits.is_empty() {
        return format!("Enable leaves {} as it is.", path.display());
    }
    format!(
        "Enable {}, for this Project and the folders below it.",
        describe_all(edits, path)
    )
}

/// Edits of a switch in `path` as a sentence says them, one after another.
fn describe_all(edits: &[Edit], path: &Path) -> String {
    let path = path.display().to_string();
    let described: Vec<String> = edits.iter().map(|edit| describe(edit, &path)).collect();
    described.join(", then ")
}

/// A Disabled Entry's reason, when the switch `holds` names its Item.
pub(crate) fn disabled_by_name(holds: &str, agent_name: &str) -> String {
    format!("Disabled by name: {holds}, so {agent_name} skips the Entries with that name.")
}

/// A Disabled Entry's reason, when the switch `holds` is its own.
pub(crate) fn disabled_natively(holds: &str, agent_name: &str) -> String {
    format!("Disabled: {holds}, so {agent_name} skips it.")
}

/// A Disabled Entry's reason, when the switch `holds` is that of the
/// plugin `plugin` it is part of.
pub(crate) fn plugin_disabled(plugin: &str, holds: &str, agent_name: &str) -> String {
    format!(
        "Disabled with its plugin {plugin}: {holds}, so {agent_name} skips every server and skill of the plugin."
    )
}

/// What Disable of an Entry of the server `name` that `agent_name` builds
/// from same-named Entries takes with it.
pub(crate) fn turns_off_built(agent_name: &str, name: &str) -> String {
    format!("This turns off the server {agent_name} builds from every Entry of `{name}`.")
}

/// Why Disable is not offered for an Entry of a server `agent_name` builds
/// when the field that turns it off is set at `at`, in a higher layer than
/// any config-digger could write it in: `versioned` says that layer's file
/// is git-versioned, so that Disable is not offered there either.
pub(crate) fn merged_field_set_above(
    agent_name: &str,
    field: &str,
    at: &str,
    versioned: bool,
) -> String {
    let instead = if versioned {
        "; that file is git-versioned, where a change reaches every clone"
    } else {
        ": Disable the Entry there instead"
    };
    format!(
        "Disable is not offered: {at} sets `{field}` in a higher layer, and {agent_name} takes the highest layer's `{field}`{instead}."
    )
}

/// What Disable through the switch of the plugin `plugin` takes with it.
pub(crate) fn takes_whole_plugin(plugin: &str) -> String {
    format!("This Disables the whole plugin {plugin}, with every server and skill it carries.")
}

/// What Disable does to the Entry `name` of `origin`, which `agent_name`
/// has no switch for: it parks it in the store at `store`.
pub(crate) fn disable_parks(name: &str, origin: &Path, store: &Path, agent_name: &str) -> String {
    format!(
        "Disable moves `{name}` out of {} into config-digger's parked-Entry store {}, since {agent_name} has no switch for it; Enable puts it back.",
        origin.display(),
        store.display()
    )
}

/// A parked Entry's reason: config-digger moved it from `origin` into the
/// store at `store`.
pub(crate) fn parked(origin: &Path, store: &Path, agent_name: &str) -> String {
    format!(
        "Disabled: config-digger parked it in {}, out of {}, so {agent_name} no longer reads it.",
        store.display(),
        origin.display()
    )
}

/// What Enable does to the parked Entry `name`.
pub(crate) fn enable_unparks(name: &str, origin: &Path) -> String {
    format!(
        "Enable puts `{name}` back into {} and takes it out of config-digger's parked-Entry store.",
        origin.display()
    )
}

/// Why Enable of the parked Entry at `locator` is refused: an Entry is at
/// `key_path` in `origin` again.
pub(crate) fn unpark_conflict(locator: &str, key_path: &[String], origin: &Path) -> String {
    format!(
        "cannot Enable {locator}: `{}` appeared in {} since it was parked, and putting it back would overwrite it",
        key_path.join("."),
        origin.display()
    )
}

/// Why a plan to put the Entry at `locator` back needs planning again: it
/// is no longer in the parked-Entry store.
pub(crate) fn no_longer_parked(locator: &str) -> String {
    format!("{locator} is no longer parked")
}

/// Why Enable of the parked Entry at `locator` is refused: what the store at
/// `store` holds for it does not read as a parked Entry.
pub(crate) fn parked_unreadable(locator: &str, store: &Path) -> String {
    format!(
        "cannot Enable {locator}: what {} holds for it does not read as a parked Entry",
        store.display()
    )
}

/// The note of a Remove of the Entry `name`, which `agent_name` approved
/// by its name, or with `by_hash`, as it is configured.
pub(crate) fn approval_kept(agent_name: &str, name: &str, by_hash: bool) -> String {
    let again = if by_hash {
        "a server added again under that name and configured the same"
    } else {
        "a server added again under that name"
    };
    format!(
        "{agent_name}'s approval of `{name}` is left as it is, since config-digger never edits approvals: {agent_name} may run {again} without asking."
    )
}

/// Why Enable of the parked skill at `locator` is refused: something is at
/// its folder `origin` again.
pub(crate) fn unpark_folder_taken(locator: &str, origin: &Path) -> String {
    format!(
        "cannot Enable {locator}: something is at {} again since it was parked, and putting it back would overwrite it",
        origin.display()
    )
}

/// Why Enable of the parked skill at `locator` is refused: config-digger's
/// store no longer keeps the folder it parked from `origin`.
pub(crate) fn parked_folder_gone(locator: &str, origin: &Path) -> String {
    format!(
        "cannot Enable {locator}: the folder parked from {} is no longer in config-digger's parked-Entry store",
        origin.display()
    )
}

/// Why Enable of the parked Entry at `locator` is refused: the folder of
/// `origin` is gone.
pub(crate) fn unpark_folder_gone(locator: &str, origin: &Path) -> String {
    format!(
        "cannot Enable {locator}: the folder of {} is gone, so there is nowhere to put it back",
        origin.display()
    )
}

/// The block of a plan that parks an Entry out of, or puts one back into,
/// `files`, which the running `agent_name` rewrites from memory.
pub(crate) fn running_agent_parking(agent_name: &str, files: &[&Path]) -> String {
    format!(
        "{agent_name} is running and rewrites {} from memory, which would undo moving an Entry in or out of it. Close {agent_name} and retry.",
        path_list(files)
    )
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
        None => format!("Disabled: would be {} once Enabled.", state_name(state)),
        Some((scope, effective)) => {
            format!("Disabled: would be Shadowed by the {scope} Entry {effective} once Enabled.")
        }
    }
}

/// The State an Entry whose State is `from` would have without what sets
/// it, which Enable does not lift: `state`, or Shadowed by the `scope`
/// Entry `effective`.
pub(crate) fn what_if_otherwise(
    from: State,
    state: State,
    shadowed_by: Option<(&str, &str)>,
) -> String {
    let from = state_name(from);
    match shadowed_by {
        None => format!("{from}: would otherwise be {}.", state_name(state)),
        Some((scope, effective)) => {
            format!("{from}: would otherwise be Shadowed by the {scope} Entry {effective}.")
        }
    }
}

/// That `agent_name`, in its Agent home `home`, reads the Entry too, where
/// it is `state`.
pub(crate) fn also_read_by(agent_name: &str, home: &str, state: State) -> String {
    format!(
        "Also read by {agent_name} (home {home}): {}.",
        state_name(state)
    )
}

/// A State as a sentence names it.
fn state_name(state: State) -> &'static str {
    match state {
        State::Effective => "Effective",
        State::Shadowed => "Shadowed",
        State::Merged => "Merged",
        State::Disabled => "Disabled",
        State::Blocked => "Blocked",
        State::Invalid => "Invalid",
        State::Unread => "Unread",
    }
}

/// A Blocked Entry's reason: the managed file `path`, which `agent_name`
/// reads, takes exclusive control of its Items of `kind`.
pub(crate) fn managed_exclusive(agent_name: &str, path: &Path, kind: ItemKind) -> Reason {
    let items = match kind {
        ItemKind::Mcp => "servers",
        ItemKind::Skill => "skills",
    };
    Reason {
        code: ReasonCode::Policy,
        effective: None,
        switch: Some(path.display().to_string()),
        sentence: format!(
            "Blocked by policy: {} takes exclusive control, so {agent_name} loads only the {items} its administrator manages.",
            path.display()
        ),
    }
}

/// A Blocked Entry's reason: `agent_name` has not approved the Entry
/// `name` of the file `path`, which it asks about in the way `prompt` says.
pub(crate) fn not_approved(
    agent_name: &str,
    name: &str,
    path: &Path,
    prompt: ApprovalPrompt,
) -> Reason {
    let asks = match prompt {
        ApprovalPrompt::OnStart => "which asks about it when it starts in this Project",
        ApprovalPrompt::McpSettings => "which asks about it in its MCP settings",
    };
    Reason {
        code: ReasonCode::NotApproved,
        effective: None,
        switch: None,
        sentence: format!(
            "Blocked: {agent_name} has not approved `{name}` from {}. Approve it in {agent_name}, {asks}; config-digger never approves servers.",
            path.display()
        ),
    }
}

/// A Blocked Entry's reason: the user has not trusted the folder of the
/// Project at `project`, which `agent_name` needs first.
pub(crate) fn untrusted_folder(agent_name: &str, project: &Path) -> Reason {
    Reason {
        code: ReasonCode::UntrustedFolder,
        effective: None,
        switch: None,
        sentence: format!(
            "Blocked: {agent_name} uses it only once you trust {}, which it asks about when the Project opens. config-digger never trusts a folder.",
            project.display()
        ),
    }
}

/// The note on an Entry, with no Project, that `agent_name` uses it only when
/// it starts in a folder the user trusts.
pub(crate) fn untrusted_folder_blocks(agent_name: &str) -> String {
    format!(
        "{agent_name} uses it only when it starts in a folder you trust, and Blocks it in any other. Pick a Project to see whether it trusts that folder."
    )
}

/// A Blocked Entry's reason: `agent_name` has not trusted the Entry `name`
/// it discovered in the file `path` as it is now.
pub(crate) fn not_trusted(agent_name: &str, name: &str, path: &Path) -> Reason {
    Reason {
        code: ReasonCode::NotApproved,
        effective: None,
        switch: None,
        sentence: format!(
            "Blocked: you have not trusted `{name}` from {} in {agent_name}, which asks when the server starts, and again whenever its configuration changes; config-digger never trusts servers.",
            path.display()
        ),
    }
}

/// A setting, by its last key, in `path`.
pub(crate) fn setting(key: &str, path: &Path) -> String {
    format!("`{key}` in {}", path.display())
}

/// How a rule keeps an Entry out: Blocked by policy when `policy`,
/// Disabled by rule otherwise, with `why` and the verb that ends it.
fn kept_out(policy: bool, code: ReasonCode, why: &str, agent_name: &str) -> Reason {
    let sentence = if policy {
        format!("Blocked by policy: {why}, so {agent_name} does not load it.")
    } else {
        let kind = match code {
            ReasonCode::DisabledGlobally => "globally",
            _ => "by rule",
        };
        format!("Disabled {kind}: {why}, so {agent_name} skips it.")
    };
    Reason {
        code: if policy { ReasonCode::Policy } else { code },
        effective: None,
        switch: None,
        sentence,
    }
}

/// The reason of an Entry the deny list `key` in `path` names `by` its
/// name, command or URL.
pub(crate) fn denied(
    policy: bool,
    key: &str,
    path: &Path,
    by: MatchedBy,
    agent_name: &str,
) -> Reason {
    let by = match by {
        MatchedBy::Name => "name",
        MatchedBy::Command => "command",
        MatchedBy::Url => "URL",
    };
    let why = format!("{} names it by {by}", setting(key, path));
    kept_out(policy, ReasonCode::DisabledByRule, &why, agent_name)
}

/// The reason of an Entry nothing in the allow lists `key` in `paths` matches.
pub(crate) fn not_allowed(policy: bool, key: &str, paths: &[&Path], agent_name: &str) -> Reason {
    let why = format!(
        "`{key}` in {} names nothing that matches it",
        path_list(paths)
    );
    kept_out(policy, ReasonCode::DisabledByRule, &why, agent_name)
}

/// The reason of a server named `name` that the allowlists `key` in
/// `places` do not allow, each place as of when the Agent cached it, if it
/// did.
pub(crate) fn not_in_allowlist(
    policy: bool,
    key: &str,
    name: &str,
    places: &[(&Path, Option<SystemTime>)],
    agent_name: &str,
) -> Reason {
    let places: Vec<String> = places
        .iter()
        .map(|(path, cached_at)| match cached_at {
            Some(at) => format!("{} as of {}", path.display(), as_of(*at)),
            None => path.display().to_string(),
        })
        .collect();
    let why = format!(
        "`{key}` in {} allows no `{name}` with this command or URL",
        places.join(", ")
    );
    kept_out(policy, ReasonCode::DisabledByRule, &why, agent_name)
}

/// The reason of an Entry the switch `key` in `path` turns off, holding
/// `value`, as of `cached_at` when the Agent cached it then.
pub(crate) fn switched_off(
    policy: bool,
    key: &str,
    value: &Value,
    path: &Path,
    cached_at: Option<SystemTime>,
    agent_name: &str,
) -> Reason {
    let mut why = format!("`{key}` is {value} in {}", path.display());
    if let Some(at) = cached_at {
        why.push_str(&format!(" as of {}", as_of(at)));
    }
    kept_out(policy, ReasonCode::DisabledGlobally, &why, agent_name)
}

/// The reason of an Entry kept out by policy because `key` in `path` is set.
pub(crate) fn policy_field(key: &str, path: &Path, agent_name: &str) -> Reason {
    let why = format!("`{key}` is set in {}", path.display());
    kept_out(true, ReasonCode::Policy, &why, agent_name)
}

/// What Enable says of an Entry the rule `holds` keeps out.
pub(crate) fn rule_enable_not_offered(holds: &str) -> String {
    format!(
        "Enable is not offered: {holds} is a rule config-digger never lifts; change it there by hand."
    )
}

/// The caveat of an Entry when the file at `path`, holding a rule or gate
/// that could keep it out, cannot be read.
pub(crate) fn setting_read_failure(path: &Path) -> String {
    format!(
        "{} couldn't be read, so whether a rule or gate there keeps this Entry out is not known: it is shown as if none did.",
        path.display()
    )
}

/// An Entry, by locator, that a switch reaches beyond what an action targets.
pub(crate) enum Reached {
    /// In the action's own context.
    Here(String),
    /// In another Project.
    InProject(String, PathBuf),
    /// With no Project.
    WithoutProject(String),
    /// In every Project and with none.
    Everywhere(String),
}

/// The warning of a plan whose switches reach the Entries `others` beyond
/// those it targets, or hold the names `by_name` in every context, so that
/// they reach every Entry of those names wherever it is.
pub(crate) fn wide_reach(action: Action, others: &[Reached], by_name: &[String]) -> String {
    let verb = match action {
        Action::Enable => "Enable",
        _ => "Disable",
    };
    let names: Vec<String> = by_name.iter().map(|name| format!("`{name}`")).collect();
    let everywhere = format!(
        "every Entry named {} in all projects and with no Project",
        names.join(", ")
    );
    let rows: Vec<String> = others
        .iter()
        .map(|reached| match reached {
            Reached::Here(locator) => locator.clone(),
            Reached::InProject(locator, project) => {
                format!("{locator} in Project {}", project.display())
            }
            Reached::WithoutProject(locator) => format!("{locator} with no Project"),
            Reached::Everywhere(locator) => {
                format!("{locator} in all projects and with no Project")
            }
        })
        .collect();
    let reaches = format!("This {verb} reaches more than what it targets");
    match (rows.is_empty(), by_name.is_empty()) {
        (_, true) => format!(
            "{reaches}: what it writes also changes the State of {}.",
            rows.join(", ")
        ),
        (true, false) => format!("{reaches}: what it writes applies to {everywhere}."),
        (false, false) => format!(
            "{reaches}: what it writes applies to {everywhere}, and changes the State of {} now.",
            rows.join(", ")
        ),
    }
}

/// Why Disable is refused for an Entry it is not offered for: `sentence`.
pub(crate) fn disable_not_offered(locator: &str, sentence: &str) -> String {
    format!("cannot Disable {locator}: {sentence}")
}

/// Why Disable is refused for an Entry that is Disabled already.
pub(crate) fn already_disabled(locator: &str) -> String {
    format!("cannot Disable {locator}: it is Disabled already")
}

/// A Blocked Entry's reason, when the switch of managed policy `holds`
/// turns it off, or the plugin `plugin` it is part of.
pub(crate) fn switched_off_by_policy(
    plugin: Option<&str>,
    holds: &str,
    agent_name: &str,
) -> String {
    match plugin {
        Some(plugin) => format!(
            "Blocked by policy: {holds}, so {agent_name} skips every server and skill of the plugin {plugin}."
        ),
        None => format!("Blocked by policy: {holds}, so {agent_name} skips it."),
    }
}

/// Why Disable is not offered for an Entry a switch of managed policy,
/// `holds`, forces on, or off when not `on`, or forces its plugin so for
/// a switch of a plugin.
pub(crate) fn policy_option(holds: &str, on: bool, plugin: bool) -> String {
    let what = if plugin { "the plugin" } else { "it" };
    let way = if on { "on" } else { "off" };
    format!(
        "Disable is not offered: {holds}, which forces {what} {way}, and config-digger never overrides managed policy."
    )
}

/// What Enable says of an Entry a switch of managed policy, `holds`, turns off.
pub(crate) fn policy_enable_not_offered(holds: &str) -> String {
    format!("Enable is not offered: {holds}, and config-digger never overrides managed policy.")
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

/// Why an Operation cannot write the SQLite store at `path`: it is gone,
/// and config-digger never creates one.
pub(crate) fn store_missing(path: &Path) -> String {
    format!(
        "{} is gone; the Agent creates its store, config-digger never does",
        path.display()
    )
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

/// Why Disable is not offered for an Entry of an extension that
/// `agent_name` turns off by path rules, in the context of no Project.
pub(crate) fn disable_needs_folder(agent_name: &str) -> String {
    format!(
        "Disable is not offered: {agent_name} turns an extension off per folder; choose a Project."
    )
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

/// Why History shows no diff of a file once its Operation's backup is deleted.
pub(crate) fn diff_of_deleted_backup(operation: &str) -> String {
    format!(
        "Can't show a diff: the backup of Operation {operation} was deleted, with the values of its edits."
    )
}

/// Why History shows no diff of a file whose recorded edits no longer apply
/// to its backup.
pub(crate) fn edits_do_not_apply(path: &Path) -> String {
    format!(
        "Can't show a diff: the edits History holds no longer apply to the backup of {}.",
        path.display()
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
pub(crate) fn read_failure_caveat(agent_name: &str, path: &Path) -> String {
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
pub(crate) fn switch_read_failure(path: &Path) -> String {
    format!(
        "{} couldn't be read, so whether it Disables this Entry is not known: it is shown as if it did not.",
        path.display()
    )
}

/// The caveat of an Entry that depends on `agent_name`'s gate `gate`, which
/// the Agent last cached as `on` at `at`, when it recorded when.
pub(crate) fn gate_cached(
    agent_name: &str,
    gate: &str,
    on: bool,
    at: Option<SystemTime>,
) -> String {
    let value = on_off(on);
    match at {
        Some(at) => format!(
            "Depends on {agent_name} gate `{gate}`: {value} as of {}.",
            as_of(at)
        ),
        None => format!(
            "Depends on {agent_name} gate `{gate}`: {value} as {agent_name} last cached it, at a time it does not record."
        ),
    }
}

/// The caveat of an Entry read from the copy of configuration held elsewhere
/// that `agent_name` cached at `at`, when it recorded when.
pub(crate) fn cached_copy(agent_name: &str, at: Option<SystemTime>) -> String {
    match at {
        Some(at) => format!(
            "Read from the copy {agent_name} cached as of {}: what it copies may have changed since.",
            as_of(at)
        ),
        None => format!(
            "Read from the copy {agent_name} cached, at a time it does not record: what it copies may have changed since."
        ),
    }
}

/// The caveat of an Entry that depends on `agent_name`'s gate `gate`, which
/// it fetches from the vendor's side and caches nowhere config-digger reads.
pub(crate) fn gate_may_apply(agent_name: &str, gate: &str) -> String {
    format!(
        "{agent_name} fetches its gate {gate} from the account it signs in with and keeps it nowhere config-digger can read: whoever administers that account may turn this off there."
    )
}

/// The caveat of an Entry that depends on `agent_name`'s gate `gate`,
/// resolved with its default `on`: the gate cannot be read, or the Agent
/// ignores its cache while the variable `bypassed_by` is set.
pub(crate) fn gate_assumed(
    agent_name: &str,
    gate: &str,
    on: bool,
    bypassed_by: Option<&str>,
) -> String {
    let value = on_off(on);
    match bypassed_by {
        Some(variable) => format!(
            "Depends on {agent_name} gate `{gate}`: assumed {value}; {agent_name} ignores its cached value while `{variable}` is set."
        ),
        None => format!("Depends on {agent_name} gate `{gate}`: assumed {value}; cannot be read."),
    }
}

/// An Unread Entry's reason: `agent_name` does not read its location while
/// its gate `gate` is `on`.
pub(crate) fn feature_gate(agent_name: &str, gate: &str, on: bool) -> Reason {
    Reason {
        code: ReasonCode::FeatureGate,
        effective: None,
        switch: None,
        sentence: format!(
            "{agent_name} does not read it while its gate `{gate}` is {}.",
            on_off(on)
        ),
    }
}

/// An Unread Entry's reason: `agent_name` reads its location only while the
/// setting `setting` has the value it needs, which it has not in the context.
pub(crate) fn needs_setting(agent_name: &str, setting: &SettingIs) -> Reason {
    let key = setting
        .sources
        .first()
        .map(|source| source.key_path.join("` › `"))
        .unwrap_or_default();
    Reason {
        code: ReasonCode::SettingValue,
        effective: None,
        switch: None,
        sentence: format!(
            "{agent_name} reads it only while its setting `{key}` is {}.",
            setting.equals
        ),
    }
}

fn on_off(on: bool) -> &'static str {
    if on { "on" } else { "off" }
}

/// A time as a caveat shows it, such as `2 Oct 2026 07:40 UTC`.
fn as_of(time: SystemTime) -> String {
    const MONTHS: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    // `YYYY-MM-DDTHH:MM:SSZ`, always ASCII.
    let rfc3339 = humantime::format_rfc3339_seconds(time).to_string();
    let month = rfc3339[5..7].parse::<usize>().unwrap_or(1);
    let day = rfc3339[8..10].trim_start_matches('0');
    format!(
        "{day} {} {} {} UTC",
        MONTHS[month.clamp(1, 12) - 1],
        &rfc3339[..4],
        &rfc3339[11..16]
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

/// An Agent home where none of the files the Agent reads exists.
pub(crate) fn no_configuration(agent_name: &str) -> String {
    format!("No configuration found for {agent_name}")
}

/// Why an Agent home of `agent_name` at `folder` would be listed with no
/// configuration: none of `looked_for`, the files and folders the Agent
/// reads in a home, is there.
pub(crate) fn no_configuration_in(
    agent_name: &str,
    folder: &Path,
    looked_for: &[String],
) -> String {
    format!(
        "{} in {}: none of {} is there. It can still be added, as a home with no Entries.",
        no_configuration(agent_name),
        folder.display(),
        and_list(looked_for)
    )
}

/// Add Project's warning when `path` is the folder of an Agent home of
/// each of `agent_names` already listed.
pub(crate) fn project_is_home(path: &Path, agent_names: &[String]) -> String {
    format!(
        "{} is already listed as an Agent home of {}. As a Project, it is read as a folder Agents are started on, not as their configuration.",
        path.display(),
        and_list(agent_names)
    )
}

/// Why `path` cannot be added: it is not an existing folder, or
/// `.code-workspace` file when `workspace` is allowed.
pub(crate) fn not_existing(path: &Path, workspace: bool) -> String {
    let what = if workspace {
        "an existing folder or .code-workspace file"
    } else {
        "an existing folder"
    };
    format!("{} is not {what}", path.display())
}

/// Why the Agent home `home` of `agent_name`, which the definition finds,
/// cannot be forgotten, and how to hide it instead.
pub(crate) fn forget_refused_home(home: &str, agent_name: &str) -> String {
    format!(
        "Agent home {home} of {agent_name} is found without you, so forgetting it would not take it off the list; hide it with `config-digger homes hide`"
    )
}

/// Why the Project at `path`, which the registries of `agent_names`
/// record, cannot be forgotten, and how to hide it instead.
pub(crate) fn forget_refused_project(path: &Path, agent_names: &[&str]) -> String {
    format!(
        "Project {} is recorded by {}, so forgetting it would not take it off the list; hide it with `config-digger projects hide`",
        path.display(),
        agent_names.join(", ")
    )
}

/// The note for an Installation of `agent_name` whose version `version` is
/// `newer` or older than the range from `min` to `max` it was tested with.
pub(crate) fn version_outside_tested_range(
    agent_name: &str,
    version: &str,
    newer: bool,
    min: &str,
    max: &str,
) -> String {
    let side = if newer { "newer" } else { "older" };
    let range = if min == max {
        min.to_owned()
    } else {
        format!("{min} to {max}")
    };
    format!(
        "{agent_name} {version} is {side} than the versions this definition was tested with ({range})."
    )
}

/// The note for the Installation of `agent_name` at `executable`, whose
/// version cannot be read.
pub(crate) fn version_unreadable(agent_name: &str, executable: &Path) -> String {
    format!(
        "The version of {agent_name} at {} could not be read, so it is not known whether this definition was tested with it.",
        executable.display()
    )
}

/// The note for an Agent home of `agent_name` that no Installation found reads.
pub(crate) fn no_installation_found(agent_name: &str) -> String {
    format!(
        "No installation of {agent_name} that reads this Agent home was found, so its version could not be checked."
    )
}

/// The note for an Agent home of `agent_name` whose definition has no
/// tested range.
pub(crate) fn definition_untested(agent_name: &str) -> String {
    format!(
        "This definition of {agent_name} has no tested version range, so what it says may not match how {agent_name} behaves."
    )
}

/// The note of a home of `agent_name`, whose user Marketplace plugins live in
/// the account.
pub(crate) fn marketplace_plugins_not_on_disk(agent_name: &str) -> String {
    format!(
        "{agent_name} keeps the plugins you install for yourself from its Marketplace in your account, so they are not listed here. Those a Project installs are."
    )
}

/// The note of a home of `agent_name`, whose connectors live in the account.
pub(crate) fn cloud_connectors_not_on_disk(agent_name: &str) -> String {
    format!(
        "{agent_name}'s connectors live in your account, with no copy on disk, so they are not listed here."
    )
}

/// The caveat of an Entry that competes by URL in `agent_name`, which also
/// counts the `unseen` servers config-digger cannot see.
pub(crate) fn unseen_servers(agent_name: &str, unseen: &[UnseenServers]) -> String {
    let sources: Vec<&str> = unseen
        .iter()
        .filter_map(|unseen| match unseen {
            UnseenServers::Extensions => Some("extensions"),
            UnseenServers::Team => Some("the team"),
            UnseenServers::MarketplacePlugins => Some("user Marketplace plugins"),
            UnseenServers::UserPick => None,
        })
        .collect();
    let picked = unseen.contains(&UnseenServers::UserPick);
    let Some((last, first)) = sources.split_last() else {
        return format!(
            "{agent_name} runs one server per URL. One you picked in {agent_name} may run instead."
        );
    };
    let those = if first.is_empty() {
        format!("those of {last}")
    } else {
        format!("those of {} and of {last}", first.join(", of "))
    };
    let picked = if picked {
        format!(", or one you picked in {agent_name},")
    } else {
        String::new()
    };
    format!(
        "{agent_name} runs one server per URL, and servers config-digger cannot see take part too: {those}. One of them{picked} may run instead."
    )
}

/// What is wrong with a skill's `SKILL.md` or folder for `agent_name`, as
/// the end of a sentence.
fn skill_defect(agent_name: &str, defect: SkillDefect) -> String {
    match defect {
        SkillDefect::NoFrontmatter => {
            "its SKILL.md does not open with a `---` frontmatter".to_owned()
        }
        SkillDefect::MalformedFrontmatter => {
            "its SKILL.md frontmatter is not valid YAML".to_owned()
        }
        SkillDefect::NoName => "its SKILL.md frontmatter has no `name`".to_owned(),
        SkillDefect::Reserved => format!("{agent_name} keeps its folder's name for itself"),
    }
}

/// The reason of a skill the Agent skips because of `defect`.
pub(crate) fn malformed_skill(agent_name: &str, defect: SkillDefect) -> Reason {
    Reason {
        code: ReasonCode::MalformedSkill,
        effective: None,
        switch: None,
        sentence: format!(
            "{agent_name} skips this skill: {}.",
            skill_defect(agent_name, defect)
        ),
    }
}

/// The note of a skill the Agent loads under its folder's name despite `defect`.
pub(crate) fn skill_fallback(agent_name: &str, defect: SkillDefect, folder: &str) -> String {
    format!(
        "{agent_name} still loads this skill, as `{folder}`, its folder's name: {}.",
        skill_defect(agent_name, defect)
    )
}

/// Why config-digger never writes an Entry of a read-only Provenance.
const READ_ONLY: &str =
    "this Entry is managed, or a copy of the user's account, and config-digger only reads it";

/// Why Disable is not offered for an Entry config-digger only reads.
pub(crate) fn read_only_option() -> String {
    format!("Disable is not offered: {READ_ONLY}.")
}

/// Why Enable is not offered for a Disabled Entry config-digger only reads.
pub(crate) fn read_only_enable() -> String {
    format!("Enable is not offered: {READ_ONLY}.")
}

/// Why Remove is not offered for an Entry config-digger only reads.
pub(crate) fn read_only_remove_option() -> String {
    format!("Remove is not offered: {READ_ONLY}.")
}

/// The label of an Agent home `owner` copies from another.
pub(crate) fn mirror_label(owner: &str) -> String {
    format!("Mirrored by {owner}")
}

/// Why config-digger never writes to an Agent home `owner` copies from the
/// home at `source`, or from one whose folder is not known.
fn mirrored(owner: &str, source: Option<&Path>) -> String {
    let from = source.map_or_else(
        || "another Agent home".to_owned(),
        |source| source.display().to_string(),
    );
    format!("{owner} copies this Agent home from {from}, and config-digger never writes to it")
}

/// What a Mirrored home is, as a sentence.
pub(crate) fn mirror(owner: &str, source: Option<&Path>) -> String {
    format!("{}.", mirrored(owner, source))
}

/// Why `action`, `Disable`, `Enable` or `Remove`, is not offered for an
/// Entry of an Agent home `owner` copies from `source`.
pub(crate) fn mirrored_option(action: &str, owner: &str, source: Option<&Path>) -> String {
    format!("{action} is not offered: {}.", mirrored(owner, source))
}

/// Why `action`, `Disable`, `Enable` or `Remove`, is refused for the Entry
/// at `locator`, of an Agent home `owner` copies from `source`.
pub(crate) fn mirrored_refused(
    action: &str,
    locator: &str,
    owner: &str,
    source: Option<&Path>,
) -> String {
    format!("cannot {action} {locator}: {}", mirrored(owner, source))
}

/// Why `action` is refused for the Entry at `locator`, of an Agent home
/// `owner` copies from `source`, which holds no Entry it was copied from.
pub(crate) fn mirrored_without_source(
    action: &str,
    locator: &str,
    owner: &str,
    source: Option<&Path>,
) -> String {
    format!(
        "{}, and that home holds no Entry it was copied from to {action} instead",
        mirrored_refused(action, locator, owner, source)
    )
}

/// What a preview says of an Operation on Entries copied into the Agent
/// home at `mirrored`, which `owner` copies from `source`: it changes the
/// Entries they were copied from, and the change reaches `mirrored` through
/// `owner`.
pub(crate) fn reaches_mirrored_home(owner: &str, mirrored: &Path, source: Option<&Path>) -> String {
    let from = source.map_or_else(
        || "the Agent home it copies".to_owned(),
        |source| source.display().to_string(),
    );
    format!(
        "{} is a Mirrored home {owner} copies from {from}: this changes the Entries there that it copied, and the change reaches {} through {owner}, once {owner} copies it.",
        mirrored.display(),
        mirrored.display()
    )
}

/// Why Remove is not offered for an Entry of the plugin `plugin`.
pub(crate) fn plugin_not_removed(plugin: &str, agent_name: &str) -> String {
    format!("Not in v0.1: uninstall the plugin {plugin} in {agent_name}.")
}

/// Why Remove is not offered for the Entry of the extension `extension`.
pub(crate) fn extension_not_removed(extension: &str, agent_name: &str) -> String {
    format!("Not in v0.1: uninstall the extension {extension} in {agent_name}'s settings.")
}

/// Why Remove is not offered for a server an extension gives the Agent
/// named `agent_name`, which it keeps only in a cache.
pub(crate) fn cached_extension_not_removed(agent_name: &str) -> String {
    format!(
        "Not in v0.1: the server comes from an extension, which {agent_name} lists from its cache. Uninstall or disable the extension in {agent_name}."
    )
}

/// How to remove an Entry of the plugin `plugin`: uninstall it with `command`.
pub(crate) fn how_to_remove_plugin(plugin: &str, command: &str) -> String {
    format!(
        "How to remove: uninstall the plugin {plugin} with `{command}`. It takes every server and skill of the plugin with it."
    )
}

/// Why Remove is refused for the Entry at `locator`, which a plugin
/// carries: `why`, and how to remove it instead.
pub(crate) fn plugin_remove_refused(locator: &str, why: &str, how: &str) -> String {
    format!("cannot Remove {locator}: {why} {how}")
}

/// Why Remove is refused for the read-only Entry at `locator`.
pub(crate) fn read_only_remove(locator: &str) -> String {
    format!("cannot Remove {locator}: {READ_ONLY}")
}

/// What an Operation does at the skill folder's location `path`, whose
/// folder, or the link's `target`, has `size`.
pub(crate) fn folder_change(
    change: FolderChangeKind,
    path: &Path,
    target: Option<&PathBuf>,
    size: FolderSize,
) -> String {
    let target = target.map_or_else(String::new, |target| target.display().to_string());
    let size = folder_size(size);
    let path = path.display();
    match change {
        FolderChangeKind::MovedToBackup => {
            format!("Moves the skill folder {path} ({size}) into the backup.")
        }
        FolderChangeKind::LinkRemoved => {
            format!("Removes the link {path} only; its target {target} ({size}) stays as it is.")
        }
        FolderChangeKind::MovedBack => {
            format!("Moves the skill folder {path} ({size}) back from the backup.")
        }
        FolderChangeKind::LinkCreated => format!("Creates the link {path} to {target} again."),
        FolderChangeKind::Parked => format!(
            "Moves the skill folder {path} ({size}) into config-digger's parked-Entry store."
        ),
        FolderChangeKind::PutBack => format!(
            "Moves the skill folder {path} ({size}) back from config-digger's parked-Entry store."
        ),
    }
}

/// A folder's size as "2 files, 80 bytes".
pub(crate) fn folder_size(size: FolderSize) -> String {
    let files = match size.files {
        1 => "1 file".to_owned(),
        n => format!("{n} files"),
    };
    let bytes = match size.bytes {
        1 => "1 byte".to_owned(),
        n => format!("{n} bytes"),
    };
    format!("{files}, {bytes}")
}

/// The `breaks-links` warning: each folder moved into the backup leaves
/// the Linked Entries that point into it broken. Each pair is a link and
/// the folder it points into.
pub(crate) fn breaks_links(broken: &[(&Path, &Path)]) -> String {
    let mut folders: Vec<&Path> = Vec::new();
    for (_, folder) in broken {
        if !folders.contains(folder) {
            folders.push(folder);
        }
    }
    let sentences: Vec<String> = folders
        .iter()
        .map(|folder| {
            let links: Vec<&Path> = broken
                .iter()
                .filter(|(_, into)| into == folder)
                .map(|(link, _)| *link)
                .collect();
            let (noun, rest) = match links.len() {
                1 => (
                    "link",
                    "It stays, as an Unread Entry, until it is Removed on its own.",
                ),
                _ => (
                    "links",
                    "They stay, as Unread Entries, until each is Removed on its own.",
                ),
            };
            format!(
                "Removing {} leaves the {noun} {} pointing to nothing. {rest}",
                folder.display(),
                path_list(&links)
            )
        })
        .collect();
    sentences.join(" ")
}

/// Why an Operation could not do `op` at the skill folder's location `path`.
pub(crate) fn folder_failed(op: &FolderOp, path: &Path, error: &std::io::Error) -> String {
    let path = path.display();
    match op {
        FolderOp::MoveToBackup => format!("cannot move {path} into the backup: {error}"),
        FolderOp::RemoveLink { .. } => format!("cannot remove the link {path}: {error}"),
        FolderOp::MoveBack { .. } => format!("cannot move {path} back: {error}"),
        FolderOp::CreateLink { .. } => format!("cannot create the link {path}: {error}"),
        FolderOp::Park { .. } => {
            format!("cannot move {path} into the parked-Entry store: {error}")
        }
        FolderOp::Unpark { .. } => format!("cannot move {path} back: {error}"),
    }
}

/// The note on a skill whose skill manager, `manager`, is recognised only
/// by a weak detector.
pub(crate) fn provenance_by_name_only(manager: &str) -> String {
    format!(
        "config-digger takes it to be installed by {manager} from its name or where it is alone, which a folder made by hand matches too."
    )
}

/// Why an update cannot install while the CLI it replaces runs.
pub(crate) fn cli_running(cli: &Path) -> String {
    format!(
        "the config-digger CLI is running ({}), and the update cannot replace it while it runs: \
         let it finish or close it, then try again",
        cli.display()
    )
}
