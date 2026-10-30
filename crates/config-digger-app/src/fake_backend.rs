//! The fixtures of the UI's fake backend, which `npm run dev:fake` serves the
//! UI on in a browser. They are recorded through the commands' own code over
//! the UAT's throwaway data, as `uat/data/cd-throwaway-setup.ps1` creates it,
//! so the DTOs keep their real shape. The core sees only that folder: its
//! environment names no variable, home or registry key of this machine, and
//! no process runs. `cargo xtask fake-backend` runs the recording.
//!
//! The recording does what the UAT does by hand: it adds the Claude Desktop
//! home, three more Claude Code homes and the extra Project, hides a Project,
//! and disables two servers, so that History holds Operations and Settings a
//! parked Entry. It then parks the server of a second Claude Desktop home
//! and forgets the home, so that Projects & homes lists a Stranded Entry.
//! Every path under the folder is then written as under
//! [`UAT_ROOT`], where the UAT's steps have it.

use crate::about::About;
use crate::actions::{ActionRequest, Actions, Executed, Verb};
use crate::commands::{self, DataFolder};
use crate::lists::{self, ListChange, ListChanged};
use crate::session::{Places, Scans, Session, SessionState};
use crate::updates::UpdateStatus;
use config_digger_core::api::{Core, Error, Inventory, OperationSummary, WarningClass};
use config_digger_core::ports::Ports;
use config_digger_core::ports::fake::{FakeClock, FakeEnvironment, FakeGitIndex, FakeProcesses};
use config_digger_core::ports::real::RealFileSystem;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Where the fixtures have the throwaway data: the UAT script's default.
pub const UAT_ROOT: &str = r"E:\cd-throwaway";

/// The Claude Desktop home, under the UAT script's root: a folder long enough
/// to wrap in Projects & homes, as the script makes it.
const DESKTOP_HOME: &str =
    r"paths-that-wrap\in-the-smallest-window\of-config-digger\claude-desktop-home";

/// The header of the generated fixtures.
const HEADER: &str =
    "// Recorded over the UAT's throwaway data by `cargo xtask fake-backend`. Do not edit.";

/// Records the fake backend's fixtures over the throwaway data in `root`,
/// which it changes as the UAT does.
pub fn record(root: &Path) -> Result<Value, Error> {
    let given = root;
    let root = canonical(root)?;
    let core = || Core::new(ports(&root));

    let home_check = core()?.check_home(&root.join(DESKTOP_HOME), "claude-desktop")?;
    let project_check = core()?.check_project(&root.join("project-extra"))?;
    let list_changed = change_lists(&root)?;
    core()?.record_update_check()?;
    let mut actions = Actions::new(core()?);
    let executed = disable_servers(&mut actions, &Session::scan(core()?)?.resolve(None))?;
    strand_server(&root, &mut actions)?;

    let mut session = Session::scan(core()?)?;
    let places = session.places();
    let contexts = [
        None,
        Some(root.join("project")),
        Some(root.join("project-extra")),
    ];
    let inventories: Vec<Inventory> = contexts
        .into_iter()
        .map(|project| session.resolve(project))
        .collect();
    let history = actions.history()?;
    let details = history
        .iter()
        .map(|operation| actions.detail(&operation.id))
        .collect::<Result<Vec<_>, _>>()?;
    let previews: Vec<Value> = requests(&inventories, &places, &history)
        .iter()
        // An action the Entry does not offer is not planned.
        .filter_map(|request| {
            let preview = actions.plan(request).ok()?;
            actions.cancel(&preview.plan_id);
            Some(json!({ "request": request, "preview": preview }))
        })
        .collect();
    let backups = backups(&mut session, &inventories)?;
    let folders = core()?.data_folders()?;
    let data_folders: serde_json::Map<String, Value> = DataFolder::ALL
        .into_iter()
        .map(|folder| {
            let kept = folder.path(&folders).is_dir();
            (serialized(&folder), Value::Bool(kept))
        })
        .collect();
    let update_status = UpdateStatus {
        last_checked: core()?.last_update_check()?,
        update: None,
    };

    let fixtures = json!({
        "inventories": inventories
            .iter()
            .map(|inventory| json!({ "project": inventory.project, "inventory": inventory }))
            .collect::<Vec<_>>(),
        "places": places,
        "listChanged": list_changed,
        "homeCheck": home_check,
        "projectCheck": project_check,
        "history": history,
        "details": details,
        "backups": backups,
        "previews": previews,
        "executed": executed,
        "settings": commands::settings_read(core()?)?,
        "dataFolders": data_folders,
        "about": About::this_build(),
        "updateStatus": update_status,
    });
    Ok(under_uat_root(fixtures, &[given, &root]))
}

/// Adds the Claude Desktop home, three more Claude Code homes, whose last
/// parts fill a tile in the smallest window, and the extra Project, then
/// hides a Project, as the UAT does: what the last change did.
fn change_lists(root: &Path) -> Result<ListChanged, Error> {
    let kept = SessionState::default();
    let scanned = root.to_path_buf();
    let scans = Scans::new(move || Session::scan(Core::new(ports(&scanned))?));
    let added = [
        ListChange::AddHome {
            folder: root.join(DESKTOP_HOME),
            agent: "claude-desktop".to_owned(),
        },
        ListChange::AddHome {
            folder: root.join("claude-code-shape-home"),
            agent: "claude-code".to_owned(),
        },
        ListChange::AddHome {
            folder: root.join("claude-code-synced-home"),
            agent: "claude-code".to_owned(),
        },
        ListChange::AddHome {
            folder: root.join("claude-code-many-home"),
            agent: "claude-code".to_owned(),
        },
        ListChange::AddProject {
            path: root.join("project-extra"),
        },
    ];
    for change in &added {
        lists::change_list(&kept, change, &scans)?;
    }
    let hidden = ListChange::HideProject {
        path: root.join("project-hide"),
    };
    lists::change_list(&kept, &hidden, &scans)
}

/// Disables the Codex server, which writes its file, then the Claude Desktop
/// one, which parks it, every warning acknowledged: what the first did.
fn disable_servers(actions: &mut Actions, inventory: &Inventory) -> Result<Executed, Error> {
    let mut executed = Vec::new();
    for name in ["cd-throwaway-codex", "cd-throwaway-desktop"] {
        let request = ActionRequest::Entry {
            verb: Verb::Disable,
            locator: locator(inventory, name)?,
            project: None,
        };
        let preview = actions.plan(&request)?;
        let acknowledged: Vec<WarningClass> = preview
            .warnings
            .iter()
            .map(|warning| warning.class)
            .collect();
        executed.push(actions.execute(&preview.plan_id, &acknowledged)?);
    }
    Ok(executed.swap_remove(0))
}

/// Adds the second Claude Desktop home, Disables its server, which parks
/// it, then forgets the home, as the UAT does, so that the server is
/// Stranded.
fn strand_server(root: &Path, actions: &mut Actions) -> Result<(), Error> {
    let kept = SessionState::default();
    let scanned = root.to_path_buf();
    let scans = Scans::new(move || Session::scan(Core::new(ports(&scanned))?));
    let folder = root.join("claude-desktop-stranded-home");
    let agent = "claude-desktop".to_owned();
    let added = ListChange::AddHome {
        folder: folder.clone(),
        agent: agent.clone(),
    };
    lists::change_list(&kept, &added, &scans)?;
    let inventory = Session::scan(Core::new(ports(root))?)?.resolve(None);
    let request = ActionRequest::Entry {
        verb: Verb::Disable,
        locator: locator(&inventory, "cd-throwaway-stranded")?,
        project: None,
    };
    let preview = actions.plan(&request)?;
    let acknowledged: Vec<WarningClass> = preview
        .warnings
        .iter()
        .map(|warning| warning.class)
        .collect();
    actions.execute(&preview.plan_id, &acknowledged)?;
    let home = folder.display().to_string();
    lists::change_list(&kept, &ListChange::ForgetHome { agent, home }, &scans)?;
    Ok(())
}

/// Every action to preview: each verb on each Entry, once, in the first
/// context that lists it, since its preview differs little from one context
/// to another; the Remove of each missing Project's leftovers, of each
/// Stranded Entry and of all of them; and the Undo and Delete backup of each
/// Operation.
fn requests(
    inventories: &[Inventory],
    places: &Places,
    history: &[OperationSummary],
) -> Vec<ActionRequest> {
    let mut requests = Vec::new();
    let mut listed = HashSet::new();
    for inventory in inventories {
        let entries = inventory.items.iter().flat_map(|item| &item.entries);
        for entry in entries.filter(|entry| listed.insert(entry.locator.clone())) {
            for verb in [Verb::Disable, Verb::Enable, Verb::Remove] {
                requests.push(ActionRequest::Entry {
                    verb,
                    locator: entry.locator.clone(),
                    project: inventory.project.clone(),
                });
            }
        }
    }
    requests.extend(
        places
            .leftovers
            .iter()
            .map(|leftovers| ActionRequest::Leftovers {
                project: leftovers.project.clone(),
            }),
    );
    requests.extend(places.stranded.iter().map(|stranded| ActionRequest::Entry {
        verb: Verb::Remove,
        locator: stranded.locator.clone(),
        project: None,
    }));
    if !places.stranded.is_empty() {
        requests.push(ActionRequest::Stranded);
    }
    for operation in history {
        requests.push(ActionRequest::Undo {
            operation: operation.id.clone(),
        });
        requests.push(ActionRequest::DeleteBackup {
            operation: operation.id.clone(),
        });
    }
    requests
}

/// The backups of each location of an Entry in `inventories` that has any.
fn backups(session: &mut Session, inventories: &[Inventory]) -> Result<Vec<Value>, Error> {
    let mut paths: Vec<PathBuf> = inventories
        .iter()
        .flat_map(|inventory| &inventory.items)
        .flat_map(|item| &item.entries)
        .map(|entry| entry.path.clone())
        .collect();
    paths.sort();
    paths.dedup();
    let mut backups = Vec::new();
    for path in paths {
        let of = session.backups_of(&path)?;
        if !of.is_empty() {
            backups.push(json!({ "path": path, "backups": of }));
        }
    }
    Ok(backups)
}

/// The fixtures as the TypeScript module the fake backend imports, typed by
/// its `Fixtures`, so that they typecheck against the bindings.
pub fn typescript(fixtures: &Value) -> String {
    let json = serde_json::to_string_pretty(fixtures).expect("JSON values serialize");
    format!(
        "{HEADER}\n\nimport type {{ Fixtures }} from \"./core\";\n\nexport const fixtures: Fixtures = {json};\n"
    )
}

/// The ports of a core that sees only the throwaway data in `root`: its
/// files, and an environment whose home is the Claude Code one there.
fn ports(root: &Path) -> Ports {
    let folder = |name: &str| root.join(name).display().to_string();
    let env = FakeEnvironment::with_home(root.join("claude-code-home"))
        .with_var("APPDATA", &folder("appdata"))
        .with_var("LOCALAPPDATA", &folder("localappdata"))
        .with_var("CODEX_HOME", &folder("codex-home"));
    Ports {
        fs: Box::new(RealFileSystem),
        processes: Box::new(FakeProcesses::default()),
        git: Box::new(FakeGitIndex::default()),
        env: Box::new(env),
        clock: Box::new(FakeClock::default()),
    }
}

/// `root` as the core spells it: long names, no `\\?\` prefix.
fn canonical(root: &Path) -> Result<PathBuf, Error> {
    let canonical = std::fs::canonicalize(root)
        .map_err(|error| Error::unexpected(format!("cannot read {}: {error}", root.display())))?;
    let text = canonical.display().to_string();
    Ok(PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(&text)))
}

/// The locator of the first Entry of the Item `name`.
fn locator(inventory: &Inventory, name: &str) -> Result<String, Error> {
    inventory
        .items
        .iter()
        .find(|item| item.name == name)
        .and_then(|item| item.entries.first())
        .map(|entry| entry.locator.clone())
        .ok_or_else(|| Error::unexpected(format!("the throwaway data lacks {name}")))
}

/// How `value` serializes as a string, such as a key.
fn serialized(value: &impl serde::Serialize) -> String {
    match json!(value) {
        Value::String(text) => text,
        other => other.to_string(),
    }
}

/// `value` with each of `roots` written as [`UAT_ROOT`] in every string and
/// key, with either slash, escaped or not, and in any letter case. The roots
/// are one folder spelled two ways: as given, which is how the throwaway
/// data names it, such as with a short name like `RUNNER~1` in a temporary
/// folder, and canonical, which is how the core does.
fn under_uat_root(value: Value, roots: &[&Path]) -> Value {
    let forms: Vec<(String, String)> = roots
        .iter()
        .map(|root| root.display().to_string())
        .flat_map(|root| {
            [
                (root.clone(), UAT_ROOT.to_owned()),
                (root.replace('\\', "/"), UAT_ROOT.replace('\\', "/")),
                // As a JSON file's text has it, such as in a diff.
                (root.replace('\\', r"\\"), UAT_ROOT.replace('\\', r"\\")),
            ]
        })
        .collect();
    map_strings(value, &|text| {
        forms.iter().fold(text.to_owned(), |text, (from, to)| {
            replace_ignoring_case(&text, from, to)
        })
    })
}

/// `value` with `map` applied to every string and key.
fn map_strings(value: Value, map: &impl Fn(&str) -> String) -> Value {
    match value {
        Value::String(text) => Value::String(map(&text)),
        Value::Array(values) => Value::Array(
            values
                .into_iter()
                .map(|value| map_strings(value, map))
                .collect(),
        ),
        Value::Object(object) => Value::Object(
            object
                .into_iter()
                .map(|(key, value)| (map(&key), map_strings(value, map)))
                .collect(),
        ),
        other => other,
    }
}

/// `text` with every `from`, in any ASCII letter case, replaced by `to`.
fn replace_ignoring_case(text: &str, from: &str, to: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = find_ignoring_case(rest, from) {
        out.push_str(&rest[..at]);
        out.push_str(to);
        rest = &rest[at + from.len()..];
    }
    out.push_str(rest);
    out
}

/// Where `needle` first is in `text`, in any ASCII letter case.
fn find_ignoring_case(text: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() {
        return None;
    }
    text.char_indices().map(|(at, _)| at).find(|&at| {
        text.as_bytes()
            .get(at..at + needle.len())
            .is_some_and(|bytes| bytes.eq_ignore_ascii_case(needle.as_bytes()))
    })
}
