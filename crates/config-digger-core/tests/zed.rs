//! The Zed acceptance walk of ADR 0005, through the core `api` with fake
//! ports: the fixture definition in `fixtures/zed/definitions`, written
//! with the blocks the six v0.1 Agents need and two new parameters of them,
//! and loaded as `--definitions` loads it, resolves a fixture Zed home and
//! Project to the States Zed gives their servers and skills.

use config_digger_core::api::{
    Action, Context, Core, Inventory, ScanOptions, Targets, WarningClass,
};
use config_digger_core::ports::fake::{self, FakeEnvironment, FakeFileSystem};
use serde_json::Value;
use std::path::{Path, PathBuf};

const HOME: &str = r"C:\Users\fixture";
const APP_DATA: &str = r"C:\Users\fixture\AppData\Roaming";
const LOCAL_APP_DATA: &str = r"C:\Users\fixture\AppData\Local";
const PROJECT: &str = r"E:\work\zed-app";
const DATABASE: &str = r"C:\Users\fixture\AppData\Local\Zed\db\0-stable\db.sqlite";

/// The fixture folder `name`.
fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(r"tests\fixtures\zed")
        .join(name)
}

/// Every file below `folder` on disk, at the same place below `at`. A
/// `<store>.rows.json` is the fixture of a SQLite store: an object of each
/// table, of each column, of the texts its rows hold, which the store at
/// `<store>` holds.
fn load(fs: FakeFileSystem, folder: &Path, at: &Path) -> FakeFileSystem {
    let mut fs = fs.with_folder(at);
    for child in std::fs::read_dir(folder).unwrap() {
        let child = child.unwrap().path();
        let name = child.file_name().unwrap();
        let store = name.to_str().unwrap().strip_suffix(".rows.json");
        fs = if child.is_dir() {
            load(fs, &child, &at.join(name))
        } else if let Some(store) = store {
            let tables: serde_json::Map<String, Value> =
                serde_json::from_slice(&std::fs::read(&child).unwrap()).unwrap();
            let mut fs = fs;
            for (table, columns) in tables {
                for (column, rows) in columns.as_object().unwrap() {
                    let rows: Vec<&str> = rows
                        .as_array()
                        .unwrap()
                        .iter()
                        .map(|row| row.as_str().unwrap())
                        .collect();
                    fs = fs.with_sqlite_rows(at.join(store), &table, column, &rows);
                }
            }
            fs
        } else {
            fs.with_file(at.join(name), std::fs::read(&child).unwrap())
        };
    }
    fs
}

/// The fixture Zed home: its configuration, its data, the user's skills
/// and the Project `E:\work\zed-app`, which Zed recorded as a workspace.
fn zed() -> FakeFileSystem {
    let fs = load(
        FakeFileSystem::new(),
        &fixture("config"),
        &Path::new(APP_DATA).join("Zed"),
    );
    let fs = load(fs, &fixture("data"), &Path::new(LOCAL_APP_DATA).join("Zed"));
    let fs = load(fs, &fixture("home"), Path::new(HOME));
    load(fs, &fixture("project"), Path::new(PROJECT))
}

/// The fixture Zed home, with the Project trusted.
fn trusted() -> FakeFileSystem {
    zed().with_sqlite_rows(DATABASE, "trusted_worktrees", "absolute_path", &[PROJECT])
}

fn env() -> FakeEnvironment {
    FakeEnvironment::with_home(HOME)
        .with_var("APPDATA", APP_DATA)
        .with_var("LOCALAPPDATA", LOCAL_APP_DATA)
        .with_var("USERPROFILE", HOME)
}

fn core(fs: &FakeFileSystem) -> Core {
    let fs = load(
        fs.clone(),
        &fixture("definitions"),
        Path::new(r"E:\dev\definitions"),
    );
    let loaded =
        Core::with_definitions_folder(fake::ports(fs, env()), Path::new(r"E:\dev\definitions"))
            .unwrap();
    assert!(loaded.rejected.is_empty(), "{:?}", loaded.rejected);
    loaded.core
}

fn inventory_in(fs: &FakeFileSystem, project: Option<&str>) -> Inventory {
    let core = core(fs);
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    let context = Context {
        project: project.map(PathBuf::from),
    };
    core.resolve(&snapshot, &context)
}

fn inventory(fs: &FakeFileSystem) -> Inventory {
    inventory_in(fs, Some(PROJECT))
}

/// An Entry's `(scope, state, reason)`, the reason empty when it has none.
type EntryState = (String, String, String);

/// An Item's kind and name, with each of its Entries in precedence order.
type ItemStates = (String, String, Vec<EntryState>);

/// Each of Zed's Items, by kind and name.
fn states(inventory: &Inventory) -> Vec<ItemStates> {
    let mut items: Vec<_> = inventory
        .items
        .iter()
        .filter(|item| item.agent == "zed")
        .map(|item| {
            let entries = item
                .entries
                .iter()
                .map(|entry| {
                    let entry = serde_json::to_value(entry).unwrap();
                    let reason = entry["reason"]["code"].as_str().unwrap_or("").to_owned();
                    (
                        entry["scope"].as_str().unwrap().to_owned(),
                        entry["state"].as_str().unwrap().to_owned(),
                        reason,
                    )
                })
                .collect();
            let kind = serde_json::to_value(item.kind).unwrap();
            (
                kind.as_str().unwrap().to_owned(),
                item.name.clone(),
                entries,
            )
        })
        .collect();
    items.sort();
    items
}

/// The Entries of the Item `name` of the kind `kind`.
fn entries_of(states: &[ItemStates], kind: &str, name: &str) -> Vec<EntryState> {
    states
        .iter()
        .find(|(k, n, _)| k == kind && n == name)
        .map(|(_, _, entries)| entries.clone())
        .unwrap_or_else(|| panic!("no {kind} `{name}`"))
}

fn item(kind: &str, name: &str, list: &[(&str, &str, &str)]) -> ItemStates {
    (kind.to_owned(), name.to_owned(), entries(list))
}

fn entries(entries: &[(&str, &str, &str)]) -> Vec<EntryState> {
    entries
        .iter()
        .map(|(scope, state, reason)| {
            (
                (*scope).to_owned(),
                (*state).to_owned(),
                (*reason).to_owned(),
            )
        })
        .collect()
}

#[test]
fn a_trusted_project_resolves_to_the_states_zed_gives() {
    let inventory = inventory(&trusted());

    let expected = vec![
        item("mcp", "browser", &[("global", "effective", "")]),
        item("mcp", "docs", &[("project", "effective", "")]),
        item("mcp", "github", &[("user", "effective", "")]),
        item(
            "mcp",
            "linear",
            &[("user", "disabled", "disabled_natively")],
        ),
        item(
            "mcp",
            "postgres-context-server",
            &[
                ("user", "merged", "merged"),
                ("extension", "merged", "merged"),
            ],
        ),
        item(
            "mcp",
            "shared",
            &[
                ("project", "effective", ""),
                ("user", "shadowed", "shadowed_by_precedence"),
                ("user", "shadowed", "shadowed_by_precedence"),
                ("global", "shadowed", "shadowed_by_precedence"),
            ],
        ),
        item("skill", "notes", &[("user", "effective", "")]),
        item(
            "skill",
            "review",
            &[
                ("project", "effective", ""),
                ("user", "shadowed", "shadowed_by_precedence"),
            ],
        ),
    ];
    assert_eq!(states(&inventory), expected);
}

#[test]
fn an_untrusted_project_applies_none_of_its_servers_or_skills() {
    let states = states(&inventory(&zed()));

    assert_eq!(
        entries_of(&states, "mcp", "docs"),
        entries(&[("project", "blocked", "untrusted_folder")])
    );
    assert_eq!(
        entries_of(&states, "mcp", "shared"),
        entries(&[
            ("project", "blocked", "untrusted_folder"),
            ("user", "effective", ""),
            ("user", "shadowed", "shadowed_by_precedence"),
            ("global", "shadowed", "shadowed_by_precedence"),
        ])
    );
    assert_eq!(
        entries_of(&states, "skill", "review"),
        entries(&[
            ("project", "blocked", "untrusted_folder"),
            ("user", "effective", ""),
        ])
    );
}

#[test]
fn trust_all_worktrees_trusts_every_project() {
    let settings = Path::new(APP_DATA).join(r"Zed\settings.json");
    let fs = zed();
    let text = String::from_utf8(fs.contents(&settings).unwrap()).unwrap();
    let fs = fs.with_file(
        &settings,
        text.replacen(
            "\"theme\"",
            "\"session\": { \"trust_all_worktrees\": true },\n  \"theme\"",
            1,
        ),
    );

    assert_eq!(states(&inventory(&fs)), states(&inventory(&trusted())));
}

#[test]
fn disable_ai_turns_every_server_and_skill_off() {
    let settings = Path::new(APP_DATA).join(r"Zed\settings.json");
    let fs = trusted();
    let text = String::from_utf8(fs.contents(&settings).unwrap()).unwrap();
    let fs = fs.with_file(
        &settings,
        text.replacen("\"theme\"", "\"disable_ai\": true,\n  \"theme\"", 1),
    );

    let inventory = inventory(&fs);

    let on: Vec<_> = inventory
        .items
        .iter()
        .filter(|item| item.agent == "zed")
        .flat_map(|item| &item.entries)
        .map(|entry| serde_json::to_value(entry).unwrap())
        .filter(|entry| entry["state"] == "effective" || entry["state"] == "merged")
        .collect();
    assert!(on.is_empty(), "{on:#?}");
    let github = inventory
        .items
        .iter()
        .find(|item| item.agent == "zed" && item.name == "github")
        .unwrap();
    let entry = serde_json::to_value(&github.entries[0]).unwrap();
    assert_eq!(entry["state"], "disabled");
    assert_eq!(entry["reason"]["code"], "disabled_globally");
}

#[test]
fn zed_s_workspaces_are_its_projects() {
    let core = core(&trusted());
    let snapshot = core.scan(&ScanOptions::default()).unwrap();

    let inventory = core.resolve(&snapshot, &Context::default());

    assert!(
        inventory
            .projects
            .iter()
            .any(|project| project.path == Path::new(PROJECT)),
        "{:?}",
        inventory.projects
    );
}

#[test]
fn the_secrets_of_a_server_and_of_an_extension_s_settings_are_masked() {
    let inventory = inventory(&trusted());

    let masked = |name: &str| {
        let item = inventory
            .items
            .iter()
            .find(|item| item.agent == "zed" && item.name == name)
            .unwrap();
        let entry = serde_json::to_value(&item.entries[0]).unwrap();
        assert!(
            !entry.to_string().contains("not-a-real-secret"),
            "{entry:#}"
        );
        entry["masked"].clone()
    };
    assert_eq!(
        masked("github"),
        serde_json::json!([["env", "GITHUB_TOKEN"]])
    );
    assert_eq!(
        masked("postgres-context-server"),
        serde_json::json!([["settings", "database_url"]])
    );
}

#[test]
fn a_user_server_notes_that_a_settings_profile_may_change_it() {
    let inventory = inventory(&trusted());

    let caveats = |name: &str, scope: &str| {
        let item = inventory
            .items
            .iter()
            .find(|item| item.agent == "zed" && item.name == name)
            .unwrap();
        let entry = item
            .entries
            .iter()
            .find(|entry| entry.scope == scope)
            .unwrap();
        serde_json::to_value(entry).unwrap()["caveats"].clone()
    };
    let github = caveats("github", "user");
    assert_eq!(github[0]["code"], "gate_assumed");
    assert_eq!(
        github[0]["sentence"],
        "Depends on Zed gate `active_settings_profile`: assumed off; cannot be read."
    );
    assert_eq!(caveats("docs", "project"), serde_json::json!([]));
}

fn disable(fs: &FakeFileSystem, selector: &str, acknowledged: &[WarningClass]) -> Core {
    let core = core(fs);
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    let context = Context {
        project: Some(PathBuf::from(PROJECT)),
    };
    let targets = Targets {
        selectors: vec![selector.to_owned()],
        all: false,
        ..Targets::default()
    };
    let plan = core
        .plan(&snapshot, &context, Action::Disable, &targets)
        .unwrap();
    core.execute(plan, acknowledged).unwrap();
    core
}

#[test]
fn disable_writes_enabled_false_into_the_user_s_settings_as_zed_s_toggle_does() {
    let settings = Path::new(APP_DATA).join(r"Zed\settings.json");
    let fs = trusted();

    // The switch works by name, in every Project.
    disable(
        &fs,
        &format!("{}#context_servers.github", settings.display()),
        &[WarningClass::WideReach],
    );

    let text = String::from_utf8(fs.contents(&settings).unwrap()).unwrap();
    assert!(text.contains("// Started by Zed."), "{text}");
    let written = jsonc(&text);
    assert_eq!(written["context_servers"]["github"]["enabled"], false);
    assert_eq!(
        written["context_servers"]["github"]["command"],
        "github-mcp"
    );
    let github = entries_of(&states(&inventory(&fs)), "mcp", "github");
    assert_eq!(github[0].1, "disabled");
}

#[test]
fn a_project_server_is_parked_since_zed_s_toggle_does_not_reach_it() {
    let project_settings = Path::new(PROJECT).join(r".zed\settings.json");
    let fs = trusted();

    disable(
        &fs,
        &format!("{}#context_servers.docs", project_settings.display()),
        &[],
    );

    let written = jsonc(&String::from_utf8(fs.contents(&project_settings).unwrap()).unwrap());
    assert!(
        written["context_servers"].get("docs").is_none(),
        "{written:#}"
    );
    let docs = entries_of(&states(&inventory(&fs)), "mcp", "docs");
    assert_eq!(docs[0].1, "disabled");
    assert_eq!(docs[0].2, "parked");
}

/// A JSONC text's value, as Zed reads it.
fn jsonc(text: &str) -> Value {
    jsonc_parser::parse_to_serde_value::<Option<Value>>(text, &Default::default())
        .unwrap()
        .unwrap()
}

#[test]
fn disable_inserts_enabled_false_for_a_server_the_user_s_settings_lack() {
    let settings = Path::new(APP_DATA).join(r"Zed\settings.json");
    let global = Path::new(APP_DATA).join(r"Zed\global_settings.json");
    let fs = trusted();

    disable(
        &fs,
        &format!("{}#context_servers.browser", global.display()),
        &[WarningClass::WideReach],
    );

    let written = jsonc(&String::from_utf8(fs.contents(&settings).unwrap()).unwrap());
    assert_eq!(
        written["context_servers"]["browser"],
        serde_json::json!({ "enabled": false })
    );
    let global_text = String::from_utf8(fs.contents(&global).unwrap()).unwrap();
    assert_eq!(
        jsonc(&global_text)["context_servers"]["browser"]["command"],
        "browser-mcp"
    );
    let browser = entries_of(&states(&inventory(&fs)), "mcp", "browser");
    assert!(
        browser.iter().all(|(_, state, _)| state != "effective"),
        "{browser:?}"
    );
}
