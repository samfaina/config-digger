//! Missing Projects and their leftovers through the core `api`, with fake
//! ports: Entries keyed to a Project whose folder is gone are Unread, and
//! can only be Removed.

use config_digger_core::api::{
    Action, Context, Core, ErrorCode, ScanOptions, State, StateAfter, Targets, Verify,
};
use config_digger_core::ports::fake::{self, FakeEnvironment, FakeFileSystem};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const HOME: &str = r"C:\Users\fixture";
const LOCAL_APP_DATA: &str = r"C:\Users\fixture\AppData\Local";
const APP_DATA: &str = r"C:\Users\fixture\AppData\Roaming";
const PETME: &str = r"E:\projects\petme-app";
const GONE: &str = r"E:\projects\gone";

/// User `linear`, a local `linear` and `old` in the missing [`GONE`], whose
/// key also records a trust, and a local `dart` in the existing [`PETME`].
const CLAUDE_JSON: &str = r#"{
  "mcpServers": {
    "linear": { "url": "https://user" }
  },
  "projects": {
    "E:/projects/gone": {
      "hasTrustDialogAccepted": true,
      "mcpServers": {
        "linear": { "url": "https://local" },
        "old": { "command": "old-mcp" }
      }
    },
    "E:/projects/petme-app": {
      "mcpServers": {
        "dart": { "command": "dart" }
      }
    }
  }
}
"#;

fn claude_json() -> PathBuf {
    PathBuf::from(HOME).join(".claude.json")
}

fn fixture() -> FakeFileSystem {
    FakeFileSystem::new()
        .with_file(claude_json(), CLAUDE_JSON)
        .with_folder(PETME)
}

fn core(fs: &FakeFileSystem) -> Core {
    let env = FakeEnvironment::with_home(HOME)
        .with_var("LOCALAPPDATA", LOCAL_APP_DATA)
        .with_var("APPDATA", APP_DATA);
    Core::new(fake::ports(fs.clone(), env)).unwrap()
}

fn context(project: Option<&str>) -> Context {
    Context {
        project: project.map(PathBuf::from),
    }
}

fn inventory(fs: &FakeFileSystem, project: Option<&str>) -> Value {
    let core = core(fs);
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    serde_json::to_value(core.resolve(&snapshot, &context(project))).unwrap()
}

/// `(name, [(scope, state)])` per Item, Entries in the order listed.
fn states(inventory: &Value) -> Vec<(String, Vec<(String, String)>)> {
    inventory["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            let entries = item["entries"]
                .as_array()
                .unwrap()
                .iter()
                .map(|entry| {
                    (
                        entry["scope"].as_str().unwrap().to_owned(),
                        entry["state"].as_str().unwrap().to_owned(),
                    )
                })
                .collect();
            (item["name"].as_str().unwrap().to_owned(), entries)
        })
        .collect()
}

fn expected(rows: &[(&str, &[(&str, &str)])]) -> Vec<(String, Vec<(String, String)>)> {
    rows.iter()
        .map(|(name, entries)| {
            let entries = entries
                .iter()
                .map(|(scope, state)| ((*scope).to_owned(), (*state).to_owned()))
                .collect();
            ((*name).to_owned(), entries)
        })
        .collect()
}

fn item<'a>(inventory: &'a Value, name: &str) -> &'a Value {
    inventory["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == name)
        .unwrap_or_else(|| panic!("no Item {name}: {inventory:#}"))
}

#[test]
fn entries_keyed_to_a_missing_project_are_unread_and_compete_with_nothing() {
    let inventory = inventory(&fixture(), Some(GONE));

    assert_eq!(
        states(&inventory),
        expected(&[
            ("linear", &[("user", "effective"), ("local", "unread")]),
            ("old", &[("local", "unread")]),
        ])
    );
    assert_eq!(item(&inventory, "old")["state"], "unread");
    assert_eq!(
        item(&inventory, "old")["entries"][0]["reason"],
        json!({
            "code": "project_missing",
            "effective": null,
            "switch": null,
            "sentence": r"Claude Code never reads it: it is keyed to the Project E:\projects\gone, whose folder no longer exists. It can only be Removed."
        })
    );
}

#[test]
fn disable_is_not_offered_for_an_unread_entry() {
    let inventory = inventory(&fixture(), Some(GONE));

    let old = &item(&inventory, "old")["entries"][0];
    assert_eq!(
        old["disable"],
        json!({
            "status": "not_offered",
            "code": "unread",
            "sentence": "Disable is not offered: the Agent never reads the Entry, so it can only be Removed."
        })
    );
    assert_eq!(old["enable"], Value::Null);
    assert_eq!(old["what_if"], Value::Null);
}

fn targets(selectors: &[&str]) -> Targets {
    Targets {
        selectors: selectors.iter().map(|s| (*s).to_owned()).collect(),
        all: false,
    }
}

fn text(fs: &FakeFileSystem) -> String {
    String::from_utf8(fs.contents(claude_json()).expect("the file exists")).unwrap()
}

const OLD: &str = r"C:\Users\fixture\.claude.json#projects.E:/projects/gone.mcpServers.old";
const LOCAL_LINEAR: &str =
    r"C:\Users\fixture\.claude.json#projects.E:/projects/gone.mcpServers.linear";

#[test]
fn disable_and_enable_are_refused_for_an_unread_entry() {
    let fs = fixture();
    let core = core(&fs);
    let snapshot = core.scan(&ScanOptions::default()).unwrap();

    for action in [Action::Disable, Action::Enable] {
        let error = core
            .plan(&snapshot, &context(Some(GONE)), action, &targets(&[OLD]))
            .err()
            .unwrap();

        assert_eq!(error.code, ErrorCode::Blocked, "{action:?}");
        assert!(
            error
                .message
                .ends_with("never reads it, so it can only be Removed"),
            "{}",
            error.message
        );
    }
    assert_eq!(text(&fs), CLAUDE_JSON);
}

#[test]
fn removing_an_unread_entry_leaves_the_trust_and_the_project() {
    let fs = fixture();
    let core = core(&fs);
    let snapshot = core.scan(&ScanOptions::default()).unwrap();

    let plan = core
        .plan(
            &snapshot,
            &context(Some(GONE)),
            Action::Remove,
            &targets(&[OLD, LOCAL_LINEAR]),
        )
        .unwrap();
    let changes = serde_json::to_value(&plan.preview().changes).unwrap();
    assert_eq!(
        changes
            .as_array()
            .unwrap()
            .iter()
            .map(|change| (
                change["item"].clone(),
                change["before"].clone(),
                change["after"].clone()
            ))
            .collect::<Vec<_>>(),
        [
            (json!("linear"), json!("unread"), json!("removed")),
            (json!("old"), json!("unread"), json!("removed")),
        ]
    );
    let outcome = core.execute(plan, &[]).unwrap();

    assert_eq!(outcome.verify, Verify::Verified);
    let after: Value = serde_json::from_str(&text(&fs)).unwrap();
    assert_eq!(
        after["projects"]["E:/projects/gone"],
        json!({ "hasTrustDialogAccepted": true, "mcpServers": {} })
    );
    let inventory = inventory(&fs, None);
    let gone = inventory["projects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|project| project["path"] == GONE)
        .expect("Claude Code's registry still knows it");
    assert_eq!(gone["missing"], true);
    assert_eq!(gone["registered_by"], json!(["claude-code"]));
}

/// [`CLAUDE_JSON`] with a second missing Project, `E:\archive\legacy`.
fn two_missing_projects() -> FakeFileSystem {
    let mut document: Value = serde_json::from_str(CLAUDE_JSON).unwrap();
    document["projects"][r"e:\archive\legacy"] =
        json!({ "mcpServers": { "legacy": { "command": "legacy-mcp" } } });
    FakeFileSystem::new()
        .with_file(claude_json(), document.to_string())
        .with_folder(PETME)
}

#[test]
fn leftovers_are_the_unread_entries_of_every_missing_project() {
    let fs = two_missing_projects();
    let core = core(&fs);
    let snapshot = core.scan(&ScanOptions::default()).unwrap();

    let leftovers = core.leftovers(&snapshot);

    let listed: Vec<(PathBuf, Vec<String>)> = leftovers
        .iter()
        .map(|leftovers| {
            let locators = leftovers
                .items
                .iter()
                .flat_map(|item| &item.entries)
                .map(|entry| entry.locator.clone())
                .collect();
            (leftovers.project.clone(), locators)
        })
        .collect();
    assert_eq!(
        listed,
        [
            (
                PathBuf::from(r"E:\archive\legacy"),
                vec![
                    r"C:\Users\fixture\.claude.json#projects.e:\archive\legacy.mcpServers.legacy"
                        .to_owned()
                ]
            ),
            (
                PathBuf::from(GONE),
                vec![LOCAL_LINEAR.to_owned(), OLD.to_owned()]
            ),
        ]
    );
    for item in leftovers.iter().flat_map(|leftovers| &leftovers.items) {
        assert_eq!(item.state, State::Unread, "{}", item.name);
    }
}

const LEGACY: &str = r"C:\Users\fixture\.claude.json#projects.e:\archive\legacy.mcpServers.legacy";

#[test]
fn the_leftovers_of_several_missing_projects_are_removed_in_one_operation_from_any_context() {
    let fs = two_missing_projects();
    let core = core(&fs);
    let snapshot = core.scan(&ScanOptions::default()).unwrap();

    let plan = core
        .plan(
            &snapshot,
            &context(Some(PETME)),
            Action::Remove,
            &targets(&[OLD, LEGACY]),
        )
        .unwrap();
    let changes: Vec<(String, Option<PathBuf>, StateAfter, StateAfter)> = plan
        .preview()
        .changes
        .iter()
        .map(|change| {
            (
                change.locator.clone(),
                change.project.clone(),
                change.before,
                change.after,
            )
        })
        .collect();
    assert_eq!(
        changes,
        [
            (
                LEGACY.to_owned(),
                Some(PathBuf::from(r"E:\archive\legacy")),
                StateAfter::Unread,
                StateAfter::Removed
            ),
            (
                OLD.to_owned(),
                Some(PathBuf::from(GONE)),
                StateAfter::Unread,
                StateAfter::Removed
            ),
        ]
    );
    let outcome = core.execute(plan, &[]).unwrap();

    assert_eq!(outcome.verify, Verify::Verified);
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    let left: Vec<String> = core
        .leftovers(&snapshot)
        .iter()
        .flat_map(|leftovers| &leftovers.items)
        .flat_map(|item| &item.entries)
        .map(|entry| entry.locator.clone())
        .collect();
    assert_eq!(left, [LOCAL_LINEAR]);
}

#[test]
fn a_leftover_of_another_project_is_selected_by_locator_only() {
    let fs = fixture();
    let core = core(&fs);
    let snapshot = core.scan(&ScanOptions::default()).unwrap();

    let error = core
        .plan(
            &snapshot,
            &context(Some(PETME)),
            Action::Remove,
            &targets(&["old"]),
        )
        .err()
        .unwrap();

    assert_eq!(error.code, ErrorCode::NoMatch);
}

#[test]
fn remove_leftovers_removes_every_leftover_of_one_missing_project_in_one_operation() {
    let fs = two_missing_projects();
    let core = core(&fs);
    let snapshot = core.scan(&ScanOptions::default()).unwrap();

    let plan = core
        .plan_remove_leftovers(&snapshot, Path::new(r"e:/Projects/GONE/"))
        .unwrap();
    assert_eq!(plan.preview().project, Some(PathBuf::from(GONE)));
    assert_eq!(plan.preview().changes.len(), 2);
    let outcome = core.execute(plan, &[]).unwrap();

    assert_eq!(outcome.verify, Verify::Verified);
    let history = core.history().unwrap();
    assert_eq!(history.len(), 1);
    let after: Value = serde_json::from_str(&text(&fs)).unwrap();
    assert_eq!(
        after["projects"]["E:/projects/gone"],
        json!({ "hasTrustDialogAccepted": true, "mcpServers": {} })
    );
    assert_eq!(
        after["projects"][r"e:\archive\legacy"]["mcpServers"]["legacy"],
        json!({ "command": "legacy-mcp" })
    );
}

#[test]
fn remove_leftovers_is_refused_for_a_project_that_exists_or_has_none() {
    let fs = fixture();
    let core = core(&fs);
    let snapshot = core.scan(&ScanOptions::default()).unwrap();

    for project in [PETME, r"E:\projects\never-known"] {
        let error = core
            .plan_remove_leftovers(&snapshot, Path::new(project))
            .err()
            .unwrap();

        assert_eq!(error.code, ErrorCode::NoMatch, "{project}");
        assert!(error.message.contains(project), "{}", error.message);
    }
}

#[test]
fn the_undo_of_a_leftover_removed_from_another_context_predicts_it_back_in_its_project() {
    let fs = fixture();
    let core = core(&fs);
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    let plan = core
        .plan(
            &snapshot,
            &context(Some(PETME)),
            Action::Remove,
            &targets(&[OLD]),
        )
        .unwrap();
    let removed = core.execute(plan, &[]).unwrap();

    let undo = core.plan_undo(&removed.operation).unwrap();

    let changes: Vec<(String, Option<PathBuf>, StateAfter, StateAfter)> = undo
        .preview()
        .changes
        .iter()
        .map(|change| {
            (
                change.locator.clone(),
                change.project.clone(),
                change.before,
                change.after,
            )
        })
        .collect();
    assert_eq!(
        changes,
        [(
            OLD.to_owned(),
            Some(PathBuf::from(GONE)),
            StateAfter::Removed,
            StateAfter::Unread
        )]
    );
    let outcome = core.execute(undo, &[]).unwrap();
    assert_eq!(outcome.verify, Verify::Verified);
    assert_eq!(text(&fs), CLAUDE_JSON);
}

#[test]
fn the_leftovers_of_a_hidden_project_are_listed_only_when_asked_for() {
    let fs = two_missing_projects();
    let core = core(&fs);
    core.hide_project(Path::new(GONE)).unwrap();
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    let inventory = core.resolve(&snapshot, &Context::default());

    let shown = inventory.leftovers_without_hidden(core.leftovers(&snapshot));

    let projects: Vec<&Path> = shown
        .iter()
        .map(|leftovers| leftovers.project.as_path())
        .collect();
    assert_eq!(projects, [Path::new(r"E:\archive\legacy")]);
    assert_eq!(core.leftovers(&snapshot).len(), 2);
}
