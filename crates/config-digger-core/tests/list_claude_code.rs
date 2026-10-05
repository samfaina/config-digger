//! Listing Claude Code user MCP servers through the core `api`, with fake ports.

use config_digger_core::api::{Context, Core, ScanOptions};
use config_digger_core::ports::fake::{self, FakeEnvironment, FakeFileSystem};
use serde_json::{Value, json};
use std::path::PathBuf;

const HOME: &str = r"C:\Users\fixture";
const CLAUDE_JSON: &[u8] = include_bytes!("fixtures/claude-code-home/.claude.json");

fn in_home(relative: &str) -> PathBuf {
    PathBuf::from(HOME).join(relative)
}

fn list(fs: FakeFileSystem) -> Value {
    let core = Core::new(fake::ports(fs, FakeEnvironment::with_home(HOME))).unwrap();
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    serde_json::to_value(core.resolve(&snapshot, &Context::default())).unwrap()
}

/// What Disable says of a user server with no Project: Claude Code's switch
/// is per Project.
fn no_project_switch() -> Value {
    json!({
        "status": "not_offered",
        "code": "no_mechanism",
        "sentence": "Disable is not offered: Claude Code Disables this Entry per Project, and there is no Project in this context."
    })
}

#[test]
fn each_user_server_is_an_effective_item_of_the_default_home() {
    let inventory = list(FakeFileSystem::new().with_file(in_home(".claude.json"), CLAUDE_JSON));

    assert_eq!(
        inventory["homes"],
        json!([{
            "agent": "claude-code",
            "agent_name": "Claude Code",
            "home": "default",
            "folder": HOME,
            "source": {"kind": "default"},
            "hidden": false,
            "not_read": null,
            "mirror": null,
            "no_configuration": null,
            "scopes": ["managed", "local", "project", "user", "plugin"],
            "installations": [],
            "out_of_range": false,
            "last_ran": null,
            "notes": [{
                "code": "no_installation_found",
                "installation": null,
                "sentence": "No installation of Claude Code that reads this Agent home was found, so its version could not be checked."
            }]
        }])
    );
    assert_eq!(
        inventory["items"],
        json!([
            {
                "agent": "claude-code",
                "home": "default",
                "kind": "mcp",
                "name": "cd-throwaway-github",
                "state": "effective",
                "entries": [{
                    "path": r"C:\Users\fixture\.claude.json",
                    "key_path": ["mcpServers", "cd-throwaway-github"],
                    "locator": r"C:\Users\fixture\.claude.json#mcpServers.cd-throwaway-github",
                    "scope": "user",
                    "provenance": "file",
                    "skill_manager": null,
                    "plugin": null,
                    "git_versioned": false,
                    "link": null,
                    "parked": null,
                    "state": "effective",
                    "reason": null,
                    "secondary": [],
                    "what_if": null,
                    "notes": [],
                    "config": {
                        "type": "stdio",
                        "command": "npx",
                        "args": ["-y", "@modelcontextprotocol/server-github"],
                        "env": { "GITHUB_PERSONAL_ACCESS_TOKEN": "••••" }
                    },
                    "masked": [["env", "GITHUB_PERSONAL_ACCESS_TOKEN"]],
                    "disable": no_project_switch(),
                    "enable": null,
                    "remove": { "status": "offered" },
                    "caveats": [],
                    "also_read_by": [],
                    "copied_from": null
                }],
                "built": null,
                "caveats": []
            },
            {
                "agent": "claude-code",
                "home": "default",
                "kind": "mcp",
                "name": "linear",
                "state": "effective",
                "entries": [{
                    "path": r"C:\Users\fixture\.claude.json",
                    "key_path": ["mcpServers", "linear"],
                    "locator": r"C:\Users\fixture\.claude.json#mcpServers.linear",
                    "scope": "user",
                    "provenance": "file",
                    "skill_manager": null,
                    "plugin": null,
                    "git_versioned": false,
                    "link": null,
                    "parked": null,
                    "state": "effective",
                    "reason": null,
                    "secondary": [],
                    "what_if": null,
                    "notes": [],
                    "config": { "type": "http", "url": "https://mcp.linear.app/mcp" },
                    "masked": [],
                    "disable": no_project_switch(),
                    "enable": null,
                    "remove": { "status": "offered" },
                    "caveats": [],
                    "also_read_by": [],
                    "copied_from": null
                }],
                "built": null,
                "caveats": []
            }
        ])
    );
}

#[test]
fn servers_in_project_keys_are_not_user_servers() {
    let inventory = list(FakeFileSystem::new().with_file(in_home(".claude.json"), CLAUDE_JSON));

    let names: Vec<&str> = inventory["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["name"].as_str().unwrap())
        .collect();
    assert!(!names.contains(&"dart"), "{names:?}");
}

#[test]
fn without_claude_json_no_home_is_found() {
    let inventory = list(FakeFileSystem::new().with_file(in_home(r"Documents\notes.txt"), "x"));

    assert_eq!(inventory["homes"], json!([]));
    assert_eq!(inventory["items"], json!([]));
}

#[test]
fn a_home_without_mcp_servers_has_no_items() {
    let inventory =
        list(FakeFileSystem::new().with_file(in_home(".claude.json"), r#"{"numStartups": 1}"#));

    assert_eq!(inventory["homes"].as_array().unwrap().len(), 1);
    assert_eq!(inventory["items"], json!([]));
}

#[test]
fn read_time_comes_from_the_clock() {
    let inventory = list(FakeFileSystem::new().with_file(in_home(".claude.json"), CLAUDE_JSON));

    assert_eq!(inventory["read_at"], "2026-10-01T12:00:00Z");
}

#[test]
fn a_claude_json_that_does_not_parse_is_a_read_failure_with_no_entries() {
    let inventory =
        list(FakeFileSystem::new().with_file(in_home(".claude.json"), r#"{"mcpServers": {"#));

    assert_eq!(inventory["items"], json!([]));
    assert_eq!(
        inventory["read_failures"],
        json!([{
            "agent": "claude-code",
            "agent_name": "Claude Code",
            "home": "default",
            "path": r"C:\Users\fixture\.claude.json",
            "project": null,
            "kind": "parse",
            "position": { "line": 1, "column": 16 },
            "reaction": "asks_user",
            "sentence": r"Claude Code cannot parse C:\Users\fixture\.claude.json as strict JSON (line 1, column 16), so it reads no Entries from it.",
            "notes": ["Claude Code will ask about this file at startup."],
            "warnings": [{
                "code": "agent_may_reset_file",
                "sentence": "Fix this before starting Claude Code: it may reset the file, losing what it holds."
            }]
        }])
    );
}

#[test]
fn a_claude_json_that_is_not_utf8_is_a_decode_read_failure() {
    let inventory = list(
        FakeFileSystem::new().with_file(in_home(".claude.json"), b"{\"a\": \"\xff\"}".to_vec()),
    );

    assert_eq!(inventory["read_failures"][0]["kind"], "decode");
}

#[test]
fn a_locator_quotes_names_that_hold_dots_or_spaces() {
    let claude_json = r#"{"mcpServers": {"acme.tools": {}, "my server": {}, "say \"hi\"": {}}}"#;
    let inventory = list(FakeFileSystem::new().with_file(in_home(".claude.json"), claude_json));

    let locators: Vec<&str> = inventory["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["entries"][0]["locator"].as_str().unwrap())
        .collect();
    assert_eq!(
        locators,
        [
            r#"C:\Users\fixture\.claude.json#mcpServers."acme.tools""#,
            r#"C:\Users\fixture\.claude.json#mcpServers."my server""#,
            r#"C:\Users\fixture\.claude.json#mcpServers."say \"hi\"""#,
        ]
    );
}
