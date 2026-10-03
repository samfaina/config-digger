//! Disable mechanism building blocks through the core `api`, with an extra
//! Agent definition over fake ports: `json-name-list` and `json-field`, the
//! choice by specificity and preference, a switch that reaches every
//! Project, and the note of a partial switch.

use config_digger_core::api::{
    Action, Context, Core, DisableOption, Entry, Inventory, Plan, ScanOptions, Targets,
    WarningClass,
};
use config_digger_core::ports::fake::{self, FakeEnvironment, FakeFileSystem};
use serde_json::{Value, json};
use std::path::PathBuf;

const HOME: &str = r"C:\Users\fixture";
const LOCAL_APP_DATA: &str = r"C:\Users\fixture\AppData\Local";
const ONE: &str = r"E:\work\one";
const TWO: &str = r"E:\work\two";

/// An Agent with a global name list listed first, a name list per Project,
/// and an `enabled` field for user servers.
const DEFINITION: &str = r#"
format_version = 1

[agent]
id = "test-agent"
name = "Test Agent"

[[homes]]
id = "default"
folder = { block = "path", path = "~" }
roots.state = ".test-agent.json"

[[scopes]]
id = "project"

[[scopes]]
id = "user"

[projects]
registries = [{ block = "json-keys", anchor = { block = "home", root = "state" }, root = ["projects"] }]

[[locations]]
id = "project-mcp"
anchor = { block = "project", path = ".test-agent.json" }
format = { block = "json", dialect = "strict" }
root = ["servers"]
kind = "mcp"
scope = "project"
provenance = "file"
store_class = "plain"
on_read_failure = "ignores_file"

[[locations]]
id = "user-mcp"
anchor = { block = "home", root = "state" }
format = { block = "json", dialect = "strict" }
root = ["servers"]
kind = "mcp"
scope = "user"
provenance = "file"
store_class = "plain"
on_read_failure = "ignores_file"

[items.mcp]
precedence = ["project-mcp", "user-mcp"]
competition = { block = "exact-name" }
checks = ["disable", "precedence"]

[[items.mcp.disable]]
block = "json-name-list"
switch = { anchor = { block = "home", root = "state" }, key_path = ["disabledServers"], store_class = "plain" }
target = "name"
context = "all"
enable_action = "remove-item"
reason = "disabled_by_name"

[[items.mcp.disable]]
block = "json-name-list"
switch = { anchor = { block = "home", root = "state" }, key_path = ["projects", "{project}", "disabledServers"], store_class = "plain" }
target = "name"
context = "project"
enable_action = "remove-item"
reason = "disabled_by_name"

[[items.mcp.disable]]
block = "json-field"
switch = { anchor = { block = "home", root = "state" }, key_path = ["servers", "{name}", "enabled"], store_class = "plain" }
target = "entry"
context = "all"
enable_action = "set"
enabled_value = true
disabled_value = false
partial_values = ["tools-only"]
reason = "disabled_natively"
locations = ["user-mcp"]
"#;

/// `alpha` is Disabled everywhere by name, `beta` has some tools off, and
/// Project one declares `gamma` and `delta`.
const HOME_FILE: &str = r#"{
  "servers": {
    "alpha": { "command": "a" },
    "beta": { "command": "b", "enabled": "tools-only" },
    "gamma": { "command": "g" }
  },
  "disabledServers": ["alpha"],
  "projects": {
    "E:/work/one": {},
    "E:/work/two": {}
  }
}
"#;

const PROJECT_FILE: &str = r#"{
  "servers": {
    "gamma": { "command": "g1" },
    "delta": { "command": "d" }
  }
}
"#;

fn home_file() -> PathBuf {
    PathBuf::from(HOME).join(".test-agent.json")
}

fn project_file() -> PathBuf {
    PathBuf::from(ONE).join(".test-agent.json")
}

fn fixture() -> FakeFileSystem {
    FakeFileSystem::new()
        .with_folder(ONE)
        .with_folder(TWO)
        .with_file(home_file(), HOME_FILE)
        .with_file(project_file(), PROJECT_FILE)
}

fn core(fs: &FakeFileSystem) -> Core {
    let env = FakeEnvironment::with_home(HOME).with_var("LOCALAPPDATA", LOCAL_APP_DATA);
    Core::with_extra_definitions(
        fake::ports(fs.clone(), env),
        &[("test-agent.toml", DEFINITION)],
    )
    .unwrap()
}

fn in_one() -> Context {
    Context {
        project: Some(PathBuf::from(ONE)),
    }
}

fn inventory(core: &Core, context: &Context) -> Inventory {
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    core.resolve(&snapshot, context)
}

/// The Entries of the test Agent's Item `name`, in precedence order.
fn entries(inventory: &Inventory, name: &str) -> Vec<Entry> {
    inventory
        .items
        .iter()
        .find(|item| item.agent == "test-agent" && item.name == name)
        .unwrap_or_else(|| panic!("no Item `{name}`"))
        .entries
        .clone()
}

fn code(value: impl serde::Serialize) -> String {
    match serde_json::to_value(value).unwrap() {
        Value::String(code) => code,
        other => panic!("not a code: {other}"),
    }
}

fn plan(core: &Core, context: &Context, action: Action, selector: &str) -> Plan {
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    let targets = Targets {
        selectors: vec![selector.to_owned()],
        all: false,
    };
    core.plan(&snapshot, context, action, &targets).unwrap()
}

fn home_json(fs: &FakeFileSystem) -> Value {
    serde_json::from_slice(&fs.contents(home_file()).unwrap()).unwrap()
}

fn user(name: &str) -> String {
    format!("{}#servers.{name}", home_file().display())
}

fn project(name: &str) -> String {
    format!("{}#servers.{name}", project_file().display())
}

/// The file Disable would write for an Entry.
fn disable_path(entry: &Entry) -> PathBuf {
    match &entry.disable {
        DisableOption::Offered { path, .. } => path.clone(),
        DisableOption::NotOffered { sentence, .. } => panic!("not offered: {sentence}"),
    }
}

#[test]
fn the_most_specific_mechanism_wins_whatever_the_order_of_preference() {
    let fs = fixture();
    let core = core(&fs);

    // A user server has its own field, more specific than any name list.
    let gamma = plan(&core, &in_one(), Action::Disable, &user("gamma"));
    let edits = &gamma.preview().files[0].edits;
    assert_eq!(edits[0].key_path, ["servers", "gamma", "enabled"]);
    // A project server has no field, and the per-Project list beats the
    // global one listed first.
    let delta = plan(&core, &in_one(), Action::Disable, "delta");
    let edits = &delta.preview().files[0].edits;
    assert_eq!(
        edits[0].key_path,
        ["projects", "E:/work/one", "disabledServers"]
    );
}

#[test]
fn a_json_field_disables_its_entry_only_and_enable_sets_it_back() {
    let fs = fixture();
    let core = core(&fs);

    let disable = plan(&core, &in_one(), Action::Disable, &user("gamma"));
    // The field applies in every context, and the project `gamma` it does
    // not cover stays Effective.
    let rows: Vec<(String, String, String)> = disable
        .preview()
        .changes
        .iter()
        .map(|change| {
            (
                change.locator.clone(),
                code(change.before),
                code(change.after),
            )
        })
        .collect();
    let row = |before: &str| (user("gamma"), before.to_owned(), "disabled".to_owned());
    assert_eq!(rows, [row("shadowed"), row("effective"), row("effective")]);
    core.execute(disable, &[WarningClass::WideReach]).unwrap();
    assert_eq!(home_json(&fs)["servers"]["gamma"]["enabled"], json!(false));
    let gamma = entries(&inventory(&core, &Context::default()), "gamma");
    assert_eq!(code(gamma[0].state), "disabled");
    assert_eq!(
        code(gamma[0].reason.as_ref().unwrap().code),
        "disabled_natively"
    );

    // Shadowed again in Project one, Effective elsewhere.
    let enable = plan(&core, &in_one(), Action::Enable, &user("gamma"));
    let outcome = core.execute(enable, &[WarningClass::WideReach]).unwrap();
    assert_eq!(
        code(outcome.verify),
        "verified",
        "{:?}",
        outcome.differences
    );
    assert_eq!(home_json(&fs)["servers"]["gamma"]["enabled"], json!(true));
}

#[test]
fn a_global_switch_disables_the_item_in_every_context() {
    let fs = fixture();
    let core = core(&fs);

    for context in [in_one(), Context::default()] {
        let alpha = entries(&inventory(&core, &context), "alpha");
        assert_eq!(code(alpha[0].state), "disabled");
    }
}

#[test]
fn an_enable_through_a_global_switch_lists_every_project_it_reaches() {
    let fs = fixture();
    let core = core(&fs);

    let enable = plan(&core, &in_one(), Action::Enable, "alpha");

    let rows: Vec<(Option<PathBuf>, String, String)> = enable
        .preview()
        .changes
        .iter()
        .map(|change| {
            (
                change.project.clone(),
                change.locator.clone(),
                code(change.after),
            )
        })
        .collect();
    assert_eq!(
        rows,
        [
            (
                Some(PathBuf::from(ONE)),
                user("alpha"),
                "effective".to_owned()
            ),
            (
                Some(PathBuf::from(TWO)),
                user("alpha"),
                "effective".to_owned()
            ),
            (None, user("alpha"), "effective".to_owned()),
        ]
    );
    let warning = &enable.warnings()[0];
    assert_eq!(code(warning.class), "wide-reach");
    assert!(warning.sentence.contains(TWO), "{}", warning.sentence);
    let outcome = core.execute(enable, &[WarningClass::WideReach]).unwrap();
    assert_eq!(
        code(outcome.verify),
        "verified",
        "{:?}",
        outcome.differences
    );
}

#[test]
fn a_partial_switch_leaves_the_entry_effective_with_a_note() {
    let fs = fixture();
    let core = core(&fs);

    let beta = entries(&inventory(&core, &in_one()), "beta");

    assert_eq!(code(beta[0].state), "effective");
    assert_eq!(beta[0].notes.len(), 1);
    let note = &beta[0].notes[0];
    assert_eq!(code(note.code), "partial_switch");
    assert_eq!(
        note.switch,
        format!("{}#servers.beta.enabled", home_file().display())
    );
    assert!(note.sentence.contains("tools-only"), "{}", note.sentence);
    assert_eq!(disable_path(&beta[0]), home_file());
}

#[test]
fn a_disabled_entry_offers_enable_instead_of_disable() {
    let fs = fixture();
    let core = core(&fs);

    let alpha = &entries(&inventory(&core, &in_one()), "alpha")[0];

    assert_eq!(
        alpha.enable.as_deref(),
        Some(
            format!(
                "Enable removes `alpha` from `disabledServers` in {}.",
                home_file().display()
            )
            .as_str()
        )
    );
    match &alpha.disable {
        DisableOption::NotOffered { code: reason, .. } => {
            assert_eq!(code(reason), "already_disabled")
        }
        offered => panic!("Disable offered: {offered:?}"),
    }
    let delta = &entries(&inventory(&core, &in_one()), "delta")[0];
    assert_eq!(delta.enable, None);
    assert_eq!(project("delta"), delta.locator);
}
