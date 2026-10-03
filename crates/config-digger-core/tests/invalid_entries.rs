//! Invalid Entries through the core `api`, with fake ports: a server missing
//! what the Agent requires is Invalid with `malformed_entry`, checked
//! against the minimal shape its location declares, and whether it still
//! competes is definition data.

use config_digger_core::api::{Context, Core, ScanOptions};
use config_digger_core::ports::fake::{self, FakeEnvironment, FakeFileSystem};
use serde_json::{Value, json};
use std::path::PathBuf;

const HOME: &str = r"C:\Users\fixture";
const PROJECT: &str = r"E:\work\app";

fn in_home(relative: &str) -> PathBuf {
    PathBuf::from(HOME).join(relative)
}

fn mcp_json() -> PathBuf {
    PathBuf::from(PROJECT).join(".mcp.json")
}

/// The inventory in the Project, with `definitions` loaded besides the built-in ones.
fn inventory(fs: FakeFileSystem, definitions: &[(&str, &str)]) -> Value {
    let core = Core::with_extra_definitions(
        fake::ports(fs, FakeEnvironment::with_home(HOME)),
        definitions,
    )
    .unwrap();
    let options = ScanOptions {
        projects: vec![PathBuf::from(PROJECT)],
    };
    let snapshot = core.scan(&options).unwrap();
    let context = Context {
        project: Some(PathBuf::from(PROJECT)),
    };
    serde_json::to_value(core.resolve(&snapshot, &context)).unwrap()
}

/// Claude Code with `claude_json` as `~/.claude.json` and `mcp_json` as the
/// Project's `.mcp.json`.
fn claude_code(claude_json: &str, mcp_json: &str) -> Value {
    inventory(
        FakeFileSystem::new()
            .with_file(in_home(".claude.json"), claude_json)
            .with_file(self::mcp_json(), mcp_json),
        &[],
    )
}

/// `(state, reason code)` of each Entry of the only Item.
fn entries(inventory: &Value) -> Vec<(String, Value)> {
    let items = inventory["items"].as_array().unwrap();
    assert_eq!(items.len(), 1, "{items:?}");
    items[0]["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            (
                entry["state"].as_str().unwrap().to_owned(),
                entry["reason"]["code"].clone(),
            )
        })
        .collect()
}

fn entry(state: &str, reason: Value) -> (String, Value) {
    (state.to_owned(), reason)
}

#[test]
fn a_server_without_command_or_url_is_invalid_and_does_not_compete() {
    let inventory = claude_code(
        r#"{"mcpServers": {"github": {"command": "gh"}}}"#,
        r#"{"mcpServers": {"github": {"env": {"A": "b"}}}}"#,
    );

    assert_eq!(
        entries(&inventory),
        [
            entry("invalid", json!("malformed_entry")),
            entry("effective", Value::Null)
        ]
    );
    assert_eq!(inventory["items"][0]["state"], "effective");
    assert_eq!(
        inventory["items"][0]["entries"][0]["reason"]["sentence"],
        "Claude Code skips this Entry: it needs `command` or `url`."
    );
}

#[test]
fn a_field_of_the_wrong_type_makes_a_server_invalid() {
    let inventory = claude_code(
        r#"{"mcpServers": {"github": {"command": "gh", "args": "--stdio"}}}"#,
        "{}",
    );

    assert_eq!(
        entries(&inventory),
        [entry("invalid", json!("malformed_entry"))]
    );
    assert_eq!(inventory["items"][0]["state"], "invalid");
    assert_eq!(
        inventory["items"][0]["entries"][0]["reason"]["sentence"],
        "Claude Code skips this Entry: `args` must be a list of strings."
    );
}

#[test]
fn a_server_that_is_not_an_object_is_invalid() {
    let inventory = claude_code(r#"{"mcpServers": {"github": "gh"}}"#, "{}");

    assert_eq!(
        entries(&inventory),
        [entry("invalid", json!("malformed_entry"))]
    );
    assert_eq!(
        inventory["items"][0]["entries"][0]["reason"]["sentence"],
        "Claude Code skips this Entry: it is not an object."
    );
}

#[test]
fn unknown_keys_are_not_flagged() {
    let inventory = claude_code(
        r#"{"mcpServers": {"github": {"command": "gh", "colour": "red", "timeout": 5}}}"#,
        "{}",
    );

    assert_eq!(entries(&inventory), [entry("effective", Value::Null)]);
}

/// An Agent whose malformed servers still compete by name, as Gemini CLI's
/// do: it lists them, then fails to connect.
const COMPETING_AGENT: &str = r#"
format_version = 1

[agent]
id = "competing-agent"
name = "Competing Agent"

[[homes]]
id = "default"
folder = { block = "path", path = "~" }
roots.config = "agent.json"

[[scopes]]
id = "project"

[[scopes]]
id = "user"

[[locations]]
id = "project-mcp"
anchor = { block = "project", path = "agent.json" }
format = { block = "json", dialect = "strict" }
root = ["servers"]
kind = "mcp"
scope = "project"
provenance = "file"
store_class = "plain"
on_read_failure = "stops_agent"
shape = { block = "fields", one_of = ["command", "url"], types = { command = "string", url = "string" }, still_competes = true }

[[locations]]
id = "user-mcp"
anchor = { block = "home", root = "config" }
format = { block = "json", dialect = "strict" }
root = ["servers"]
kind = "mcp"
scope = "user"
provenance = "file"
store_class = "plain"
on_read_failure = "stops_agent"
shape = { block = "fields", one_of = ["command", "url"], types = { command = "string", url = "string" }, still_competes = true }

[items.mcp]
precedence = ["project-mcp", "user-mcp"]
competition = { block = "exact-name" }
checks = ["shape", "disable", "precedence"]
"#;

#[test]
fn an_invalid_server_that_still_competes_shadows_the_ones_below_it() {
    let inventory = inventory(
        FakeFileSystem::new()
            .with_file(
                in_home("agent.json"),
                r#"{"servers": {"a": {"command": "a"}}}"#,
            )
            .with_file(
                PathBuf::from(PROJECT).join("agent.json"),
                r#"{"servers": {"a": {"command": 1}}}"#,
            ),
        &[("competing-agent.toml", COMPETING_AGENT)],
    );

    assert_eq!(
        entries(&inventory),
        [
            entry("invalid", json!("malformed_entry")),
            entry("shadowed", json!("shadowed_by_precedence"))
        ]
    );
    assert_eq!(inventory["items"][0]["state"], "invalid");
    assert_eq!(
        inventory["items"][0]["entries"][0]["reason"]["sentence"],
        "Competing Agent cannot use this Entry: `command` must be a string. It still competes by name."
    );
}

#[test]
fn a_disabled_malformed_server_would_be_invalid_once_enabled() {
    let inventory = claude_code(
        r#"{"mcpServers": {"github": {"args": ["x"]}}, "projects": {"E:/work/app": {"disabledMcpServers": ["github"]}}}"#,
        "{}",
    );

    let entry = &inventory["items"][0]["entries"][0];
    assert_eq!(entry["state"], "invalid");
    assert_eq!(entry["secondary"][0]["code"], "disabled_by_name");

    // Disable checked first, as an Agent may skip a Disabled Entry before
    // looking at it.
    let reordered = String::from_utf8(
        std::fs::read(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/definitions/claude-code.toml"
        ))
        .unwrap(),
    )
    .unwrap()
    .replace(
        r#"checks = ["policy", "shape", "rules", "disable", "approval", "precedence"]"#,
        r#"checks = ["policy", "rules", "disable", "shape", "approval", "precedence"]"#,
    )
    .replace(r#"id = "claude-code""#, r#"id = "claude-code-reordered""#);
    let inventory = inventory_of(
        FakeFileSystem::new()
            .with_file(
                in_home(".claude.json"),
                r#"{"mcpServers": {"github": {"args": ["x"]}}, "projects": {"E:/work/app": {"disabledMcpServers": ["github"]}}}"#,
            ),
        &[("reordered.toml", &reordered)],
        "claude-code-reordered",
    );
    let entry = &inventory["entries"][0];
    assert_eq!(entry["state"], "disabled");
    assert_eq!(
        entry["what_if"],
        json!({
            "state": "invalid",
            "effective": null,
            "sentence": "Disabled: would be Invalid once Enabled."
        })
    );
}

/// The one Item of the Agent `agent`.
fn inventory_of(fs: FakeFileSystem, definitions: &[(&str, &str)], agent: &str) -> Value {
    let inventory = inventory(fs, definitions);
    let items: Vec<&Value> = inventory["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["agent"] == agent)
        .collect();
    assert_eq!(items.len(), 1, "{items:?}");
    items[0].clone()
}
