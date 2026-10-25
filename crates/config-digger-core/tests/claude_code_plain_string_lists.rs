//! Claude Code's `enabledMcpjsonServers` and `disabledMcpjsonServers`
//! written as one plain-string name, through the core `api`, with fake
//! ports: since 2.1.299 a settings file reads one as a one-item list, for
//! approval and for Disable, and the Project's state in `~/.claude.json`
//! approves nothing by one. Disable and Enable over one leave a list or no
//! key, and Undo puts the plain string back.

use config_digger_core::api::{Action, Context, Core, Inventory, Plan, ScanOptions, Targets};
use config_digger_core::ports::fake::{self, FakeEnvironment, FakeFileSystem};
use serde_json::{Value, json};
use std::path::PathBuf;

const HOME: &str = r"C:\Users\fixture";
const PROJECT: &str = r"E:\projects\petme-app";

fn in_home(relative: &str) -> PathBuf {
    PathBuf::from(HOME).join(relative)
}

fn in_project(relative: &str) -> PathBuf {
    PathBuf::from(PROJECT).join(relative)
}

fn mcp_json() -> PathBuf {
    in_project(".mcp.json")
}

/// A Claude Code home whose Project petme-app has the state
/// `project_state` in `~/.claude.json`, and whose `.mcp.json` declares the
/// servers `a`, `b`, `a.b` and `a b`.
fn project_home(project_state: Value) -> FakeFileSystem {
    let server = json!({ "command": "server-mcp" });
    FakeFileSystem::new()
        .with_file(
            in_home(".claude.json"),
            json!({ "projects": { "E:/projects/petme-app": project_state } }).to_string(),
        )
        .with_file(
            mcp_json(),
            json!({ "mcpServers": {
                "a": server, "b": server, "a.b": server, "a b": server,
            } })
            .to_string(),
        )
}

fn core(fs: &FakeFileSystem) -> Core {
    Core::new(fake::ports(fs.clone(), FakeEnvironment::with_home(HOME))).unwrap()
}

fn in_petme() -> Context {
    Context {
        project: Some(PathBuf::from(PROJECT)),
    }
}

fn scan_petme() -> ScanOptions {
    ScanOptions {
        projects: vec![PathBuf::from(PROJECT)],
    }
}

fn inventory(fs: &FakeFileSystem) -> Inventory {
    let core = core(fs);
    let snapshot = core.scan(&scan_petme()).unwrap();
    core.resolve(&snapshot, &in_petme())
}

/// The `.mcp.json` Entry of the server `name`, as JSON.
fn project_entry(inventory: &Inventory, name: &str) -> Value {
    let item = inventory
        .items
        .iter()
        .find(|item| item.name == name)
        .unwrap_or_else(|| panic!("no server `{name}`"));
    let entry = item
        .entries
        .iter()
        .find(|entry| entry.path == mcp_json())
        .unwrap_or_else(|| panic!("no `.mcp.json` Entry of `{name}`"));
    serde_json::to_value(entry).unwrap()
}

/// The settings files whose plain-string lists Claude Code reads.
fn settings_files() -> [PathBuf; 3] {
    [
        in_home(r".claude\settings.json"),
        in_project(r".claude\settings.json"),
        in_project(r".claude\settings.local.json"),
    ]
}

#[test]
fn a_plain_string_in_a_settings_file_approves_that_one_server() {
    for path in settings_files() {
        let fs = project_home(json!({}))
            .with_file(&path, json!({ "enabledMcpjsonServers": "a" }).to_string());

        let inventory = inventory(&fs);

        let shown = path.display();
        assert_eq!(
            project_entry(&inventory, "a")["state"],
            "effective",
            "{shown}"
        );
        let b = project_entry(&inventory, "b");
        assert_eq!(b["state"], "blocked", "{shown}");
        assert_eq!(b["reason"]["code"], "not_approved", "{shown}");
    }
}

#[test]
fn a_plain_string_in_the_project_state_approves_nothing() {
    let fs = project_home(json!({ "enabledMcpjsonServers": "a" }));

    let inventory = inventory(&fs);

    let a = project_entry(&inventory, "a");
    assert_eq!(a["state"], "blocked");
    assert_eq!(a["reason"]["code"], "not_approved");
}

#[test]
fn a_plain_string_with_any_other_character_approves_nothing() {
    for name in ["a.b", "a b"] {
        let fs = project_home(json!({})).with_file(
            in_project(r".claude\settings.local.json"),
            json!({ "enabledMcpjsonServers": name }).to_string(),
        );

        let inventory = inventory(&fs);

        let entry = project_entry(&inventory, name);
        assert_eq!(entry["state"], "blocked", "{name}");
        assert_eq!(entry["reason"]["code"], "not_approved", "{name}");
    }
}

/// The Project's state approves every `.mcp.json` server.
fn approved() -> Value {
    json!({ "enableAllProjectMcpServers": true })
}

#[test]
fn a_plain_string_in_a_settings_file_disables_that_one_server() {
    for path in settings_files() {
        let fs = project_home(approved())
            .with_file(&path, json!({ "disabledMcpjsonServers": "a" }).to_string());

        let inventory = inventory(&fs);

        let shown = path.display();
        let a = project_entry(&inventory, "a");
        assert_eq!(a["state"], "disabled", "{shown}");
        assert_eq!(a["reason"]["code"], "disabled_by_name", "{shown}");
        let switch = format!("{}#disabledMcpjsonServers", path.display());
        assert_eq!(a["reason"]["switch"], switch.as_str(), "{shown}");
        assert_eq!(
            project_entry(&inventory, "b")["state"],
            "effective",
            "{shown}"
        );
    }
}

#[test]
fn a_plain_string_with_any_other_character_disables_nothing() {
    for name in ["a.b", "a b"] {
        let fs = project_home(approved()).with_file(
            in_project(r".claude\settings.local.json"),
            json!({ "disabledMcpjsonServers": name }).to_string(),
        );

        let inventory = inventory(&fs);

        assert_eq!(
            project_entry(&inventory, name)["state"],
            "effective",
            "{name}"
        );
    }
}

fn settings_local() -> PathBuf {
    in_project(r".claude\settings.local.json")
}

/// The plan of `action` on the `.mcp.json` server `name`.
fn plan(core: &Core, action: Action, name: &str) -> Plan {
    let snapshot = core.scan(&scan_petme()).unwrap();
    let targets = Targets {
        selectors: vec![format!("{}#mcpServers.{name}", mcp_json().display())],
        all: false,
    };
    core.plan(&snapshot, &in_petme(), action, &targets).unwrap()
}

/// The local settings, holding `disabledMcpjsonServers` as the plain string
/// `"a"` after the permissions Claude Code keeps there. They hold another
/// key, as Claude Code's do: the JSONC editor writes a key put back in an
/// object a Remove left empty on the object's line.
const PLAIN_A: &str =
    "{\n  \"permissions\": { \"allow\": [] },\n  \"disabledMcpjsonServers\": \"a\"\n}\n";

/// A home whose `~/.claude.json` has not recorded petme-app, so that
/// Disable writes the local settings, which hold `local`, and whose user
/// settings approve every `.mcp.json` server.
fn unrecorded_home(local: &str) -> FakeFileSystem {
    let server = json!({ "command": "server-mcp" });
    FakeFileSystem::new()
        .with_folder(PROJECT)
        .with_file(in_home(".claude.json"), "{}")
        .with_file(
            in_home(r".claude\settings.json"),
            json!({ "enableAllProjectMcpServers": true }).to_string(),
        )
        .with_file(
            mcp_json(),
            json!({ "mcpServers": { "a": server, "b": server } }).to_string(),
        )
        .with_file(settings_local(), local)
}

fn local_value(fs: &FakeFileSystem) -> Value {
    serde_json::from_slice(&fs.contents(settings_local()).unwrap()).unwrap()
}

#[test]
fn disable_over_a_plain_string_writes_a_list_of_both_names_and_undo_puts_the_string_back() {
    let fs = unrecorded_home(PLAIN_A);
    let core = core(&fs);

    let outcome = core
        .execute(plan(&core, Action::Disable, "b"), &[])
        .unwrap();

    assert_eq!(
        local_value(&fs),
        json!({ "permissions": { "allow": [] }, "disabledMcpjsonServers": ["a", "b"] })
    );
    assert_eq!(project_entry(&inventory(&fs), "b")["state"], "disabled");
    core.execute(core.plan_undo(&outcome.operation).unwrap(), &[])
        .unwrap();
    assert_eq!(
        String::from_utf8(fs.contents(settings_local()).unwrap()).unwrap(),
        PLAIN_A
    );
}

#[test]
fn enable_of_the_name_a_plain_string_holds_deletes_the_key_and_undo_puts_it_back() {
    let fs = unrecorded_home(PLAIN_A);
    let core = core(&fs);

    let outcome = core.execute(plan(&core, Action::Enable, "a"), &[]).unwrap();

    assert_eq!(local_value(&fs), json!({ "permissions": { "allow": [] } }));
    assert_eq!(project_entry(&inventory(&fs), "a")["state"], "effective");
    core.execute(core.plan_undo(&outcome.operation).unwrap(), &[])
        .unwrap();
    assert_eq!(
        String::from_utf8(fs.contents(settings_local()).unwrap()).unwrap(),
        PLAIN_A
    );
}
