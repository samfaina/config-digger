//! The fixtures of the UI's fake backend, which `npm run dev:fake` serves the
//! UI on in a browser. They are recorded through the commands' own code over
//! the UAT's throwaway data, as `uat/data/cd-throwaway-setup.ps1` creates it,
//! so the DTOs keep their real shape. The core sees only that folder: its
//! environment names no variable, home or registry key of this machine, and
//! no process runs. `cargo xtask fake-backend` runs the recording.
//!
//! The recording does what the UAT does by hand: it adds the Claude Desktop
//! home, the second Claude Code home and the extra Project, hides a Project,
//! and disables two servers, so that History holds Operations and Settings a
//! parked Entry. Every path under the folder is then written as under
//! [`UAT_ROOT`], where the UAT's steps have it.

use crate::about::About;
use crate::actions::{ActionRequest, Actions, Executed, Verb};
use crate::commands::{self, DataFolder};
use crate::lists::{self, ListChange};
use crate::session::{Scans, Session, SessionState};
use crate::updates::UpdateStatus;
use config_digger_core::api::{Core, Error, Inventory, WarningClass};
use config_digger_core::ports::Ports;
use config_digger_core::ports::fake::{FakeClock, FakeEnvironment, FakeGitIndex, FakeProcesses};
use config_digger_core::ports::real::RealFileSystem;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// Where the fixtures have the throwaway data: the UAT script's default.
pub const UAT_ROOT: &str = r"E:\cd-throwaway";

/// The header of the generated fixtures.
const HEADER: &str =
    "// Recorded over the UAT's throwaway data by `cargo xtask fake-backend`. Do not edit.";

/// Records the fake backend's fixtures over the throwaway data in `root`,
/// which it changes as the UAT does.
pub fn record(root: &Path) -> Result<Value, Error> {
    let root = real(root)?;
    let core = || Core::new(ports(&root));

    let home_check = core()?.check_home(&root.join("claude-desktop-home"), "claude-desktop")?;
    let project_check = core()?.check_project(&root.join("project-extra"))?;
    let kept = SessionState::default();
    let scanned = root.clone();
    let scans = Scans::new(move || Session::scan(Core::new(ports(&scanned))?));
    let changes = [
        ListChange::AddHome {
            folder: root.join("claude-desktop-home"),
            agent: "claude-desktop".to_owned(),
        },
        ListChange::AddHome {
            folder: root.join("claude-code-many-home"),
            agent: "claude-code".to_owned(),
        },
        ListChange::AddProject {
            path: root.join("project-extra"),
        },
        ListChange::HideProject {
            path: root.join("project-hide"),
        },
    ];
    let mut list_changed = None;
    for change in &changes {
        list_changed = Some(lists::change_list(&kept, change, &scans)?);
    }

    core()?.record_update_check()?;
    let mut actions = Actions::new(core()?);
    let inventory = Session::scan(core()?)?.resolve(None);
    let mut executed = None;
    for name in ["cd-throwaway-codex", "cd-throwaway-desktop"] {
        let request = ActionRequest::Entry {
            verb: Verb::Disable,
            locator: locator(&inventory, name)?,
            project: None,
        };
        let preview = actions.plan(&request)?;
        let acknowledged: Vec<WarningClass> = preview
            .warnings
            .iter()
            .map(|warning| warning.class)
            .collect();
        let done = actions.execute(&preview.plan_id, &acknowledged)?;
        executed.get_or_insert(done);
    }

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
    // Each Entry once, in the first context that lists it: its actions
    // differ little from one context to another.
    let mut requests: Vec<ActionRequest> = Vec::new();
    let mut planned = HashSet::new();
    for inventory in &inventories {
        let entries = inventory.items.iter().flat_map(|item| &item.entries);
        for entry in entries.filter(|entry| planned.insert(entry.locator.clone())) {
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
    for operation in &history {
        requests.push(ActionRequest::Undo {
            operation: operation.id.clone(),
        });
        requests.push(ActionRequest::DeleteBackup {
            operation: operation.id.clone(),
        });
    }
    let previews: Vec<Value> = requests
        .iter()
        // An action the Entry does not offer is not planned.
        .filter_map(|request| {
            let preview = actions.plan(request).ok()?;
            actions.cancel(&preview.plan_id);
            Some(json!({ "request": request, "preview": preview }))
        })
        .collect();

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

    let folders = core()?.data_folders()?;
    let data_folders: serde_json::Map<String, Value> = DataFolder::ALL
        .into_iter()
        .map(|folder| {
            let kept = folder.path(folders.clone()).is_dir();
            (key(&folder), Value::Bool(kept))
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
        "executed": executed.map(|executed| match executed {
            Executed::Applied { .. } => json!(executed),
            Executed::Replanned { .. } => Value::Null,
        }),
        "settings": commands::settings_read(core()?)?,
        "dataFolders": data_folders,
        "about": About::this_build(),
        "updateStatus": update_status,
    });
    Ok(under_uat_root(fixtures, &root))
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
fn real(root: &Path) -> Result<PathBuf, Error> {
    let real = std::fs::canonicalize(root)
        .map_err(|error| Error::unexpected(format!("cannot read {}: {error}", root.display())))?;
    let text = real.display().to_string();
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
fn key(value: &impl serde::Serialize) -> String {
    match json!(value) {
        Value::String(text) => text,
        other => other.to_string(),
    }
}

/// `value` with `root` written as [`UAT_ROOT`] in every string and key,
/// with either slash, escaped or not, and in any letter case.
fn under_uat_root(value: Value, root: &Path) -> Value {
    let root = root.display().to_string();
    let forms = [
        (root.clone(), UAT_ROOT.to_owned()),
        (root.replace('\\', "/"), UAT_ROOT.replace('\\', "/")),
        // As a JSON file's text has it, such as in a diff.
        (root.replace('\\', r"\\"), UAT_ROOT.replace('\\', r"\\")),
    ];
    map_strings(value, &|text| {
        forms
            .iter()
            .fold(text.to_owned(), |text, (from, to)| replace(&text, from, to))
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
fn replace(text: &str, from: &str, to: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = find(rest, from) {
        out.push_str(&rest[..at]);
        out.push_str(to);
        rest = &rest[at + from.len()..];
    }
    out.push_str(rest);
    out
}

fn find(text: &str, needle: &str) -> Option<usize> {
    if needle.is_empty() {
        return None;
    }
    text.char_indices().map(|(at, _)| at).find(|&at| {
        text.as_bytes()
            .get(at..at + needle.len())
            .is_some_and(|bytes| bytes.eq_ignore_ascii_case(needle.as_bytes()))
    })
}
