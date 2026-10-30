//! A `json-field` switch that holds an Entry under further keys through
//! `also_names`, through the core `api`, with an extra Agent definition over
//! fake ports: a location whose `name_template` names its Entries
//! `synced:<name>`, beside a local one, and a switch that holds each Entry
//! by its name or by its own name before the template, as Claude Code's
//! `skillOverrides` holds a synced skill, a partial value under its own
//! name limiting it.

use config_digger_core::api::{Action, Context, Core, Inventory, Plan, ScanOptions, Targets};
use config_digger_core::api::{Entry, WarningClass};
use config_digger_core::ports::fake::{self, FakeEnvironment, FakeFileSystem};
use serde_json::{Value, json};
use std::path::PathBuf;

const HOME: &str = r"C:\Users\fixture";
const LOCAL_APP_DATA: &str = r"C:\Users\fixture\AppData\Local";
const PROJECT: &str = r"E:\work\one";

/// An Agent with local and synced servers, the synced ones named
/// `synced:<name>`, and a switch in the Project's settings that holds a
/// server by its name or its own name.
const DEFINITION: &str = r#"
format_version = 1

[agent]
id = "test-agent"
name = "Test Agent"

[[homes]]
id = "default"
folder = { block = "path", path = "~" }
roots.state = ".test-agent.json"
roots.synced = ".test-agent-synced.json"

[[scopes]]
id = "user"

[projects]
registries = [{ block = "json-keys", anchor = { block = "home", root = "state" }, root = ["projects"] }]

[[locations]]
id = "local"
anchor = { block = "home", root = "state" }
format = { block = "json", dialect = "strict" }
root = ["servers"]
kind = "mcp"
scope = "user"
provenance = "file"
store_class = "plain"
on_read_failure = "ignores_file"

[[locations]]
id = "synced"
anchor = { block = "home", root = "synced" }
format = { block = "json", dialect = "strict" }
root = ["servers"]
name_template = "synced:{name}"
kind = "mcp"
scope = "user"
provenance = "file"
store_class = "plain"
on_read_failure = "ignores_file"

[items.mcp]
precedence = ["local", "synced"]
competition = { block = "exact-name" }
checks = ["disable", "precedence"]

[[items.mcp.disable]]
block = "json-field"
switch = { anchor = { block = "project", path = ".test-agent/settings.json" }, key_path = ["overrides", "{name}"], also_names = ["{own_name}"], store_class = "plain" }
target = "name"
context = "project"
enable_action = "delete-key"
reason = "disabled_by_name"
disabled_value = "off"
partial_values = ["name-only"]
"#;

fn home_file() -> PathBuf {
    PathBuf::from(HOME).join(".test-agent.json")
}

fn synced_file() -> PathBuf {
    PathBuf::from(HOME).join(".test-agent-synced.json")
}

fn settings() -> PathBuf {
    PathBuf::from(PROJECT).join(r".test-agent\settings.json")
}

/// The synced server `pdf`, the local one too when `local`, and the
/// Project's settings holding `overrides`.
fn fixture(local: bool, overrides: Value) -> FakeFileSystem {
    let local_servers = if local {
        json!({ "pdf": { "command": "local" } })
    } else {
        json!({})
    };
    let state = json!({ "servers": local_servers, "projects": { "E:/work/one": {} } });
    let synced = json!({ "servers": { "pdf": { "command": "synced" } } });
    FakeFileSystem::new()
        .with_folder(PROJECT)
        .with_file(home_file(), state.to_string())
        .with_file(synced_file(), synced.to_string())
        .with_file(settings(), json!({ "overrides": overrides }).to_string())
}

fn core(fs: &FakeFileSystem) -> Core {
    let env = FakeEnvironment::with_home(HOME).with_var("LOCALAPPDATA", LOCAL_APP_DATA);
    Core::with_extra_definitions(
        fake::ports(fs.clone(), env),
        &[("test-agent.toml", DEFINITION)],
    )
    .unwrap()
}

fn in_project() -> Context {
    Context {
        project: Some(PathBuf::from(PROJECT)),
    }
}

fn inventory(core: &Core) -> Inventory {
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    core.resolve(&snapshot, &in_project())
}

fn entry(inventory: &Inventory, name: &str) -> Entry {
    inventory
        .items
        .iter()
        .find(|item| item.agent == "test-agent" && item.name == name)
        .unwrap_or_else(|| panic!("no Item `{name}`"))
        .entries[0]
        .clone()
}

fn code(value: impl serde::Serialize) -> String {
    match serde_json::to_value(value).unwrap() {
        Value::String(code) => code,
        other => panic!("not a code: {other}"),
    }
}

fn plan(core: &Core, action: Action, selector: &str) -> Plan {
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    let targets = Targets {
        selectors: vec![selector.to_owned()],
        all: false,
        ..Targets::default()
    };
    core.plan(&snapshot, &in_project(), action, &targets)
        .unwrap()
}

fn settings_json(fs: &FakeFileSystem) -> Value {
    serde_json::from_slice(&fs.contents(settings()).unwrap()).unwrap()
}

fn local_pdf() -> String {
    format!("{}#servers.pdf", home_file().display())
}

fn synced_pdf() -> String {
    format!("{}#servers.pdf", synced_file().display())
}

/// The `wide-reach` warning of `plan`'s, its sentence.
fn wide_reach(plan: &Plan) -> Option<String> {
    plan.warnings()
        .iter()
        .find(|warning| warning.class == WarningClass::WideReach)
        .map(|warning| warning.sentence.clone())
}

#[test]
fn disable_writes_the_first_key_alone() {
    let fs = fixture(true, json!({}));
    let core = core(&fs);

    let disable = plan(&core, Action::Disable, "synced:pdf");
    assert_eq!(wide_reach(&disable), None);
    core.execute(disable, &[]).unwrap();

    assert_eq!(
        settings_json(&fs),
        json!({ "overrides": { "synced:pdf": "off" } })
    );
    let after = inventory(&core);
    assert_eq!(code(entry(&after, "synced:pdf").state), "disabled");
    assert_eq!(code(entry(&after, "pdf").state), "effective");
}

#[test]
fn enable_deletes_every_key_that_disables_the_entry() {
    let fs = fixture(false, json!({ "synced:pdf": "off", "pdf": "off" }));
    let core = core(&fs);

    let enable = plan(&core, Action::Enable, "synced:pdf");
    assert_eq!(wide_reach(&enable), None);
    core.execute(enable, &[]).unwrap();

    assert_eq!(settings_json(&fs), json!({ "overrides": {} }));
    assert_eq!(
        code(entry(&inventory(&core), "synced:pdf").state),
        "effective"
    );
}

#[test]
fn enable_through_the_own_name_key_warns_of_the_entry_it_also_turns_on() {
    let fs = fixture(true, json!({ "synced:pdf": "off", "pdf": "off" }));
    let core = core(&fs);
    let before = inventory(&core);
    assert_eq!(code(entry(&before, "pdf").state), "disabled");

    let enable = plan(&core, Action::Enable, "synced:pdf");

    let sentence = wide_reach(&enable).expect("a wide-reach warning");
    assert!(sentence.contains(&local_pdf()), "{sentence}");
    core.execute(enable, &[WarningClass::WideReach]).unwrap();
    assert_eq!(settings_json(&fs), json!({ "overrides": {} }));
    let after = inventory(&core);
    assert_eq!(code(entry(&after, "synced:pdf").state), "effective");
    assert_eq!(code(entry(&after, "pdf").state), "effective");
}

#[test]
fn disable_of_the_entry_named_as_another_s_own_name_warns_of_that_one_too() {
    let fs = fixture(true, json!({}));
    let core = core(&fs);

    let disable = plan(&core, Action::Disable, &local_pdf());

    let sentence = wide_reach(&disable).expect("a wide-reach warning");
    assert!(sentence.contains(&synced_pdf()), "{sentence}");
    core.execute(disable, &[WarningClass::WideReach]).unwrap();
    assert_eq!(settings_json(&fs), json!({ "overrides": { "pdf": "off" } }));
    let after = inventory(&core);
    assert_eq!(code(entry(&after, "pdf").state), "disabled");
    assert_eq!(code(entry(&after, "synced:pdf").state), "disabled");
}

#[test]
fn a_partial_value_under_the_own_name_key_limits_the_entry() {
    let fs = fixture(false, json!({ "pdf": "name-only" }));
    let core = core(&fs);

    let pdf = entry(&inventory(&core), "synced:pdf");

    assert_eq!(code(pdf.state), "effective");
    assert_eq!(pdf.notes.len(), 1);
    assert_eq!(code(pdf.notes[0].code), "partial_switch");
    assert_eq!(
        pdf.notes[0].switch,
        Some(format!("{}#overrides.pdf", settings().display()))
    );
}
