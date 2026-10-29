//! Read failures through the core `api`, with fake ports: each location
//! parsed in each reader's dialect, fixture files that do not parse in each dialect, and
//! what each Agent does about a location it cannot read.

use config_digger_core::api::{
    Action, Context, Core, Error, ErrorCode, Info, InfoCode, Plan, ScanOptions, Targets,
};
use config_digger_core::ports::fake::{self, FakeEnvironment, FakeFileSystem};
use serde_json::{Value, json};
use std::path::PathBuf;

const HOME: &str = r"C:\Users\fixture";
const PROJECT: &str = r"E:\work\app";

/// An Agent that reads the Project's `.mcp.json` as JSONC with trailing
/// commas, next to Claude Code, which reads it as strict JSON.
const JSONC_AGENT: &str = r#"
format_version = 1

[agent]
id = "jsonc-agent"
name = "JSONC Agent"

[[homes]]
id = "default"
folder = { block = "path", path = "~" }
roots.state = ".jsonc-agent"

[[scopes]]
id = "project"

[[locations]]
id = "project-mcp"
anchor = { block = "project", path = ".mcp.json" }
format = { block = "json", dialect = "jsonc" }
root = ["mcpServers"]
kind = "mcp"
scope = "project"
provenance = "file"
store_class = "plain"
on_read_failure = "ignores_file"

[items.mcp]
precedence = ["project-mcp"]
competition = { block = "exact-name" }
checks = ["disable", "precedence"]
"#;

fn in_home(relative: &str) -> PathBuf {
    PathBuf::from(HOME).join(relative)
}

fn mcp_json() -> PathBuf {
    PathBuf::from(PROJECT).join(".mcp.json")
}

/// `fs` with the user's settings approving every `.mcp.json` server, which
/// Claude Code otherwise Blocks. The Project's own approvals would count
/// only in a Project its `~/.claude.json` trusts.
fn approved(fs: FakeFileSystem) -> FakeFileSystem {
    fs.with_file(
        in_home(r".claude\settings.json"),
        r#"{"enableAllProjectMcpServers": true}"#,
    )
}

/// Claude Code's home and the JSONC Agent's, with `.mcp.json` holding `text`.
fn with_mcp_json(text: &str) -> FakeFileSystem {
    FakeFileSystem::new()
        .with_file(in_home(".claude.json"), "{}")
        .with_folder(in_home(".jsonc-agent"))
        .with_file(mcp_json(), text)
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

/// `(agent, name, state)` of each Item.
fn items(inventory: &Value) -> Vec<(String, String, String)> {
    inventory["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            (
                item["agent"].as_str().unwrap().to_owned(),
                item["name"].as_str().unwrap().to_owned(),
                item["state"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

fn item(agent: &str, name: &str, state: &str) -> (String, String, String) {
    (agent.to_owned(), name.to_owned(), state.to_owned())
}

#[test]
fn a_file_one_reader_rejects_still_yields_entries_for_another() {
    let inventory = inventory(
        with_mcp_json(
            "{\n  // the team's servers\n  \"mcpServers\": { \"github\": { \"command\": \"gh\" } }\n}\n",
        ),
        &[("jsonc-agent.toml", JSONC_AGENT)],
    );

    assert_eq!(
        items(&inventory),
        [item("jsonc-agent", "github", "effective")]
    );
    let failures = inventory["read_failures"].as_array().unwrap();
    assert_eq!(failures.len(), 1, "{failures:?}");
    assert_eq!(failures[0]["agent"], "claude-code");
    assert_eq!(failures[0]["path"], json!(mcp_json()));
    assert_eq!(failures[0]["project"], json!(PROJECT));
    assert_eq!(failures[0]["kind"], "parse");
    assert_eq!(failures[0]["position"], json!({ "line": 2, "column": 3 }));
}

/// An Agent that reads its servers from `~/<file>` in `format`, doing
/// `reaction` when it cannot.
fn user_agent(file: &str, format: &str, reaction: &str) -> String {
    format!(
        r#"
format_version = 1

[agent]
id = "user-agent"
name = "User Agent"

[[homes]]
id = "default"
folder = {{ block = "path", path = "~" }}
roots.config = "{file}"

[[scopes]]
id = "user"

[[locations]]
id = "user-mcp"
anchor = {{ block = "home", root = "config" }}
format = {format}
root = ["servers"]
kind = "mcp"
scope = "user"
provenance = "file"
store_class = "plain"
on_read_failure = "{reaction}"

[items.mcp]
precedence = ["user-mcp"]
competition = {{ block = "exact-name" }}
checks = ["disable", "precedence"]
"#
    )
}

/// The inventory of the User Agent reading `text` from `~/<file>` in `format`.
fn user_inventory(file: &str, format: &str, text: &str) -> Value {
    let definition = user_agent(file, format, "ignores_file");
    inventory(
        FakeFileSystem::new().with_file(in_home(file), text),
        &[("user-agent.toml", &definition)],
    )
}

/// `(kind, position)` of each Read failure.
fn failures(inventory: &Value) -> Vec<(String, Value)> {
    inventory["read_failures"]
        .as_array()
        .unwrap()
        .iter()
        .map(|failure| {
            (
                failure["kind"].as_str().unwrap().to_owned(),
                failure["position"].clone(),
            )
        })
        .collect()
}

const STRICT: &str = r#"{ block = "json", dialect = "strict" }"#;
const WITH_COMMENTS: &str = r#"{ block = "json", dialect = "with-comments" }"#;
const JSONC: &str = r#"{ block = "json", dialect = "jsonc" }"#;
const TOML: &str = r#"{ block = "toml" }"#;

const COMMENTED_JSON: &str = "{\n  // mine\n  \"servers\": { \"a\": { \"command\": \"a\" } }\n}\n";
const TRAILING_COMMA_JSON: &str =
    "{\n  \"servers\": {\n    \"a\": { \"command\": \"a\" },\n  }\n}\n";

#[test]
fn strict_json_rejects_a_comment_with_its_position() {
    let inventory = user_inventory("agent.json", STRICT, COMMENTED_JSON);

    assert_eq!(items(&inventory), []);
    assert_eq!(
        failures(&inventory),
        [("parse".to_owned(), json!({ "line": 2, "column": 3 }))]
    );
    assert_eq!(inventory["read_failures"][0]["project"], Value::Null);
}

#[test]
fn json_with_comments_reads_comments_and_rejects_a_trailing_comma() {
    let commented = user_inventory("agent.json", WITH_COMMENTS, COMMENTED_JSON);
    let trailing = user_inventory("agent.json", WITH_COMMENTS, TRAILING_COMMA_JSON);

    assert_eq!(items(&commented), [item("user-agent", "a", "effective")]);
    assert_eq!(failures(&commented), []);
    assert_eq!(items(&trailing), []);
    assert_eq!(
        failures(&trailing),
        [("parse".to_owned(), json!({ "line": 3, "column": 28 }))]
    );
}

#[test]
fn jsonc_reads_comments_and_trailing_commas() {
    let inventory = user_inventory("agent.json", JSONC, TRAILING_COMMA_JSON);

    assert_eq!(items(&inventory), [item("user-agent", "a", "effective")]);
    assert_eq!(failures(&inventory), []);
}

#[test]
fn jsonc_still_rejects_what_is_not_json() {
    let inventory = user_inventory("agent.json", JSONC, "{ servers: { 'a': {} } }");

    assert_eq!(items(&inventory), []);
    assert_eq!(failures(&inventory)[0].0, "parse");
}

#[test]
fn toml_yields_entries() {
    let inventory = user_inventory(
        "agent.toml",
        TOML,
        "# mine\n[servers.a]\ncommand = \"a\"\nargs = [\"-y\"]\n",
    );

    assert_eq!(items(&inventory), [item("user-agent", "a", "effective")]);
    assert_eq!(
        inventory["items"][0]["entries"][0]["config"],
        json!({ "command": "a", "args": ["-y"] })
    );
}

#[test]
fn toml_that_does_not_parse_is_a_read_failure_with_its_position_and_no_entries() {
    let inventory = user_inventory(
        "agent.toml",
        TOML,
        "[servers.a]\ncommand = \"a\"\n\n[servers.b\ncommand = \"b\"\n",
    );

    assert_eq!(items(&inventory), []);
    assert_eq!(
        failures(&inventory),
        [("parse".to_owned(), json!({ "line": 4, "column": 11 }))]
    );
}

#[test]
fn toml_that_is_not_utf8_is_a_decode_read_failure() {
    let definition = user_agent("agent.toml", TOML, "ignores_file");
    let inventory = inventory(
        FakeFileSystem::new().with_file(
            in_home("agent.toml"),
            b"[servers.a]\ncommand = \"\xe9\"\n".to_vec(),
        ),
        &[("user-agent.toml", &definition)],
    );

    assert_eq!(failures(&inventory), [("decode".to_owned(), Value::Null)]);
}

#[test]
fn servers_that_are_not_an_object_are_a_shape_read_failure_of_their_location() {
    let inventory = user_inventory("agent.json", STRICT, r#"{"servers": [{"command": "a"}]}"#);

    assert_eq!(items(&inventory), []);
    assert_eq!(failures(&inventory), [("shape".to_owned(), Value::Null)]);
    let failure = &inventory["read_failures"][0];
    assert_eq!(failure["location"]["id"], "user-mcp");
    assert_eq!(failure["location"]["key_path"], json!(["servers"]));
}

#[test]
fn a_location_says_what_its_agent_does_about_a_shape() {
    let definition = user_agent("agent.json", STRICT, "stops_agent").replace(
        r#"on_read_failure = "stops_agent""#,
        "on_read_failure = \"stops_agent\"\non_shape_failure = \"ignores_file\"",
    );
    let inventory = inventory(
        FakeFileSystem::new().with_file(in_home("agent.json"), r#"{"servers": []}"#),
        &[("user-agent.toml", &definition)],
    );

    assert_eq!(inventory["read_failures"][0]["reaction"], "ignores_file");
    assert_eq!(inventory["agent_flags"], json!([]));
}

#[test]
fn a_shape_takes_none_of_the_reaction_to_a_file_that_does_not_parse() {
    let definition = user_agent("agent.json", STRICT, "stops_agent");
    // Servers that are a list, and a file that is valid JSON but not an object.
    for text in [r#"{"servers": []}"#, "[]"] {
        let inventory = inventory(
            FakeFileSystem::new().with_file(in_home("agent.json"), text),
            &[("user-agent.toml", &definition)],
        );

        assert_eq!(
            inventory["read_failures"][0]["reaction"], "unknown",
            "{text}"
        );
        assert_eq!(inventory["agent_flags"], json!([]), "{text}");
    }
}

#[test]
fn a_claude_json_that_is_not_an_object_carries_no_reset_warning() {
    let inventory = inventory(
        FakeFileSystem::new().with_file(in_home(".claude.json"), "[]"),
        &[],
    );

    let failure = &inventory["read_failures"][0];
    assert_eq!(failure["kind"], "shape");
    assert_eq!(failure["reaction"], "unknown");
    assert_eq!(failure["notes"], json!([]));
    assert_eq!(failure["warnings"], json!([]));
}

#[test]
fn an_unknown_reaction_to_a_shape_rejects_the_definition() {
    let definition = user_agent("agent.json", STRICT, "ignores_file").replace(
        r#"on_read_failure = "ignores_file""#,
        "on_read_failure = \"ignores_file\"\non_shape_failure = \"shrugs\"",
    );
    let loaded = Core::with_extra_definitions(
        fake::ports(FakeFileSystem::new(), FakeEnvironment::with_home(HOME)),
        &[("user-agent.toml", &definition)],
    );

    let error = loaded.err().unwrap();
    assert!(error.message.contains("shrugs"), "{}", error.message);
}

#[test]
fn a_document_that_is_not_an_object_fails_every_location_in_the_file() {
    let inventory = user_inventory("agent.json", STRICT, "[]");

    assert_eq!(failures(&inventory), [("shape".to_owned(), Value::Null)]);
    let failure = &inventory["read_failures"][0];
    assert_eq!(failure["location"], Value::Null);
    assert_eq!(
        failure["sentence"],
        r"User Agent cannot read C:\Users\fixture\agent.json: it is strict JSON but not an object, so it reads no Entries from it."
    );
}

#[test]
fn a_document_without_servers_is_no_read_failure() {
    let inventory = user_inventory("agent.json", STRICT, r#"{"other": 1}"#);

    assert_eq!(items(&inventory), []);
    assert_eq!(failures(&inventory), []);
}

#[test]
fn a_project_key_that_is_not_an_object_fails_only_its_projects_local_servers() {
    let inventory = inventory(
        FakeFileSystem::new().with_file(
            in_home(".claude.json"),
            r#"{"mcpServers": {"a": {"command": "a"}}, "projects": {"E:/work/app": "not an object"}}"#,
        ),
        &[],
    );

    assert_eq!(items(&inventory), [item("claude-code", "a", "effective")]);
    assert_eq!(failures(&inventory), [("shape".to_owned(), Value::Null)]);
    let failure = &inventory["read_failures"][0];
    assert_eq!(failure["location"]["id"], "local-mcp");
    assert_eq!(
        failure["location"]["key_path"],
        json!(["projects", "E:/work/app"])
    );
    assert_eq!(failure["project"], PROJECT);
}

#[test]
fn projects_that_are_not_an_object_fail_the_local_servers_of_every_project_and_not_the_registry() {
    let inventory = inventory(
        FakeFileSystem::new().with_file(
            in_home(".claude.json"),
            r#"{"mcpServers": {"a": {"command": "a"}}, "projects": "x"}"#,
        ),
        &[],
    );

    assert_eq!(items(&inventory), [item("claude-code", "a", "effective")]);
    assert_eq!(failures(&inventory), [("shape".to_owned(), Value::Null)]);
    let failure = &inventory["read_failures"][0];
    assert_eq!(failure["location"]["id"], "local-mcp");
    assert_eq!(failure["location"]["key_path"], json!(["projects"]));
    assert_eq!(failure["project"], Value::Null);
}

#[test]
fn user_servers_that_are_a_list_leave_the_local_servers_read() {
    let inventory = inventory(
        FakeFileSystem::new().with_file(
            in_home(".claude.json"),
            r#"{"mcpServers": [], "projects": {"E:/work/app": {"mcpServers": {"b": {"command": "b"}}}}}"#,
        ).with_folder(PROJECT),
        &[],
    );

    assert_eq!(items(&inventory), [item("claude-code", "b", "effective")]);
    assert_eq!(failures(&inventory), [("shape".to_owned(), Value::Null)]);
    let failure = &inventory["read_failures"][0];
    assert_eq!(failure["location"]["id"], "user-mcp");
    assert_eq!(failure["location"]["key_path"], json!(["mcpServers"]));
    assert_eq!(failure["project"], Value::Null);
    assert_eq!(failure["reaction"], "unknown");
    assert_eq!(failure["notes"], json!([]));
    assert_eq!(failure["warnings"], json!([]));
}

#[test]
fn a_shape_read_failure_drops_only_its_own_location_in_its_project() {
    let claude_json = in_home(".claude.json");
    let inventory = inventory(
        FakeFileSystem::new().with_file(
            &claude_json,
            r#"{"mcpServers": {"a": {"command": "a"}}, "projects": {"E:/work/app": {"mcpServers": []}}}"#,
        ),
        &[],
    );

    assert_eq!(items(&inventory), [item("claude-code", "a", "effective")]);
    assert_eq!(
        inventory["read_failures"],
        json!([{
            "agent": "claude-code",
            "agent_name": "Claude Code",
            "home": "default",
            "path": claude_json,
            "project": PROJECT,
            "location": {
                "id": "local-mcp",
                "key_path": ["projects", "E:/work/app", "mcpServers"]
            },
            "kind": "shape",
            "position": null,
            "reaction": "unknown",
            "sentence": r"Claude Code cannot read the MCP servers at C:\Users\fixture\.claude.json#projects.E:/work/app.mcpServers: it holds a list, not an object, so it reads no Entries from there.",
            "notes": [],
            "warnings": []
        }])
    );
    assert_eq!(
        inventory["items"][0]["caveats"],
        json!([{
            "code": "read_failure",
            "path": claude_json,
            "sentence": r"A location that could take part couldn't be read: C:\Users\fixture\.claude.json, so the State shown may not be the one Claude Code has."
        }])
    );
}

#[test]
fn a_file_that_cannot_be_opened_is_a_permission_read_failure() {
    let definition = user_agent("agent.json", STRICT, "ignores_file");
    let inventory = inventory(
        FakeFileSystem::new()
            .with_file(
                in_home("agent.json"),
                r#"{"servers": {"a": {"command": "a"}}}"#,
            )
            .denying_reads_of(in_home("agent.json")),
        &[("user-agent.toml", &definition)],
    );

    assert_eq!(items(&inventory), []);
    assert_eq!(
        failures(&inventory),
        [("permission".to_owned(), Value::Null)]
    );
}

#[test]
fn a_missing_location_is_no_read_failure() {
    let inventory = inventory(with_mcp_json("{}"), &[]);
    let fs = FakeFileSystem::new().with_file(in_home(".claude.json"), "{}");
    let without_mcp_json = self::inventory(fs, &[]);

    assert_eq!(failures(&inventory), []);
    assert_eq!(failures(&without_mcp_json), []);
}

/// The inventory in `project`, or with no Project.
fn inventory_in(fs: FakeFileSystem, definitions: &[(&str, &str)], project: Option<&str>) -> Value {
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
        project: project.map(PathBuf::from),
    };
    serde_json::to_value(core.resolve(&snapshot, &context)).unwrap()
}

/// The JSONC Agent, doing `reaction` when it cannot read `.mcp.json`.
fn project_agent(reaction: &str) -> String {
    JSONC_AGENT.replace(
        r#"on_read_failure = "ignores_file""#,
        &format!(r#"on_read_failure = "{reaction}""#),
    )
}

const UNPARSABLE_JSON: &str = r#"{"mcpServers": {"#;

#[test]
fn claude_code_asks_about_a_claude_json_it_cannot_parse_and_may_reset_it() {
    let fs = approved(
        FakeFileSystem::new()
            .with_file(in_home(".claude.json"), UNPARSABLE_JSON)
            .with_file(
                mcp_json(),
                r#"{"mcpServers": {"github": {"command": "gh"}}}"#,
            ),
    );
    let inventory = inventory(fs, &[]);

    assert_eq!(
        items(&inventory),
        [item("claude-code", "github", "effective")]
    );
    let failure = &inventory["read_failures"][0];
    assert_eq!(failure["reaction"], "asks_user");
    assert_eq!(
        failure["sentence"],
        r"Claude Code cannot parse C:\Users\fixture\.claude.json as strict JSON (line 1, column 16), so it reads no Entries from it."
    );
    assert_eq!(
        failure["notes"],
        json!(["Claude Code will ask about this file at startup."])
    );
    assert_eq!(
        failure["warnings"],
        json!([{
            "code": "agent_may_reset_file",
            "sentence": "Fix this before starting Claude Code: it may reset the file, losing what it holds."
        }])
    );
    assert_eq!(inventory["agent_flags"], json!([]));
    assert_eq!(inventory["items"][0]["caveats"], json!([]));
}

#[test]
fn an_agent_that_ignores_a_file_it_cannot_read_has_no_flag_or_caveat() {
    let fs = with_mcp_json(UNPARSABLE_JSON);
    let inventory = inventory(fs, &[("jsonc-agent.toml", &project_agent("ignores_file"))]);

    let failure = inventory["read_failures"]
        .as_array()
        .unwrap()
        .iter()
        .find(|failure| failure["agent"] == "jsonc-agent")
        .unwrap();
    assert_eq!(failure["reaction"], "ignores_file");
    assert_eq!(failure["notes"], json!([]));
    assert_eq!(failure["warnings"], json!([]));
    assert_eq!(inventory["agent_flags"], json!([]));
}

#[test]
fn a_user_file_that_stops_the_agent_flags_it_in_every_context() {
    let definition = user_agent("agent.json", STRICT, "stops_agent");
    let fs = || FakeFileSystem::new().with_file(in_home("agent.json"), UNPARSABLE_JSON);
    let in_project = inventory_in(fs(), &[("user-agent.toml", &definition)], Some(PROJECT));
    let without_project = inventory_in(fs(), &[("user-agent.toml", &definition)], None);

    let flag = json!([{
        "agent": "user-agent",
        "agent_name": "User Agent",
        "home": "default",
        "code": "cannot_start",
        "path": in_home("agent.json"),
        "sentence": r"User Agent won't start: it cannot read C:\Users\fixture\agent.json."
    }]);
    assert_eq!(in_project["agent_flags"], flag);
    assert_eq!(without_project["agent_flags"], flag);
    assert_eq!(in_project["read_failures"][0]["reaction"], "stops_agent");
}

#[test]
fn a_project_file_that_stops_the_agent_flags_it_in_that_project_only() {
    let definitions = [("jsonc-agent.toml", project_agent("stops_agent"))];
    let definitions: Vec<(&str, &str)> = definitions
        .iter()
        .map(|(file, text)| (*file, text.as_str()))
        .collect();
    let in_project = inventory_in(with_mcp_json(UNPARSABLE_JSON), &definitions, Some(PROJECT));
    let without_project = inventory_in(with_mcp_json(UNPARSABLE_JSON), &definitions, None);

    let flags = in_project["agent_flags"].as_array().unwrap();
    assert_eq!(flags.len(), 1, "{flags:?}");
    assert_eq!(flags[0]["agent"], "jsonc-agent");
    assert_eq!(flags[0]["code"], "cannot_start");
    assert_eq!(flags[0]["path"], json!(mcp_json()));
    assert_eq!(without_project["agent_flags"], json!([]));
}

#[test]
fn entries_keep_their_states_when_another_file_stops_the_agent() {
    let definition = user_agent("agent.json", STRICT, "stops_agent").replace(
        "[[locations]]",
        "[[locations]]\nid = \"extra-mcp\"\nanchor = { block = \"home\", root = \"extra\" }\nformat = { block = \"json\", dialect = \"strict\" }\nroot = [\"servers\"]\nkind = \"mcp\"\nscope = \"user\"\nprovenance = \"file\"\nstore_class = \"plain\"\non_read_failure = \"ignores_file\"\n\n[[locations]]",
    )
    .replace("roots.config = ", "roots.extra = \"extra.json\"\nroots.config = ")
    .replace(r#"precedence = ["user-mcp"]"#, r#"precedence = ["extra-mcp", "user-mcp"]"#);
    let fs = FakeFileSystem::new()
        .with_file(in_home("agent.json"), UNPARSABLE_JSON)
        .with_file(
            in_home("extra.json"),
            r#"{"servers": {"a": {"command": "a"}}}"#,
        );
    let inventory = inventory(fs, &[("user-agent.toml", &definition)]);

    assert_eq!(items(&inventory), [item("user-agent", "a", "effective")]);
    assert_eq!(inventory["agent_flags"][0]["code"], "cannot_start");
}

#[test]
fn a_location_whose_reaction_is_unknown_gives_every_item_in_its_contexts_a_caveat() {
    let definition = user_agent("agent.json", STRICT, "ignores_file")
        .replace(
            "[[locations]]",
            "[[scopes]]\nid = \"project\"\n\n[[locations]]\nid = \"project-mcp\"\nanchor = { block = \"project\", path = \".mcp.json\" }\nformat = { block = \"json\", dialect = \"strict\" }\nroot = [\"mcpServers\"]\nkind = \"mcp\"\nscope = \"project\"\nprovenance = \"file\"\nstore_class = \"plain\"\non_read_failure = \"unknown\"\n\n[[locations]]",
        )
        .replace(
            r#"precedence = ["user-mcp"]"#,
            r#"precedence = ["project-mcp", "user-mcp"]"#,
        );
    let fs = || {
        FakeFileSystem::new()
            .with_file(
                in_home("agent.json"),
                r#"{"servers": {"a": {"command": "a"}}}"#,
            )
            .with_file(mcp_json(), UNPARSABLE_JSON)
    };
    let definitions = [("user-agent.toml", definition.as_str())];
    let in_project = inventory_in(fs(), &definitions, Some(PROJECT));
    let without_project = inventory_in(fs(), &definitions, None);

    let caveats = |inventory: &Value| {
        inventory["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| item["agent"] == "user-agent")
            .unwrap()["caveats"]
            .clone()
    };
    let failure = in_project["read_failures"]
        .as_array()
        .unwrap()
        .iter()
        .find(|failure| failure["agent"] == "user-agent")
        .unwrap();
    assert_eq!(failure["reaction"], "unknown");
    assert_eq!(
        caveats(&in_project),
        json!([{
            "code": "read_failure",
            "path": mcp_json(),
            "sentence": r"A location that could take part couldn't be read: E:\work\app\.mcp.json, so the State shown may not be the one User Agent has."
        }])
    );
    assert_eq!(caveats(&without_project), json!([]));
    assert_eq!(in_project["agent_flags"], json!([]));
}

#[test]
fn claude_code_skips_a_mcp_json_it_cannot_parse_and_loads_its_other_servers() {
    let fs = FakeFileSystem::new()
        .with_file(
            in_home(".claude.json"),
            r#"{"mcpServers": {"linear": {"url": "https://mcp.linear.app/mcp"}}}"#,
        )
        .with_file(mcp_json(), UNPARSABLE_JSON);
    let inventory = inventory_in(fs, &[], Some(PROJECT));

    assert_eq!(inventory["read_failures"][0]["reaction"], "ignores_file");
    assert_eq!(
        items(&inventory),
        [item("claude-code", "linear", "effective")]
    );
    assert_eq!(inventory["items"][0]["caveats"], json!([]));
    assert_eq!(inventory["agent_flags"], json!([]));
}

#[test]
fn a_disable_store_that_is_a_read_failure_resolves_with_defaults_and_a_caveat() {
    let settings = PathBuf::from(PROJECT).join(r".claude\settings.local.json");
    let fs = approved(FakeFileSystem::new())
        .with_file(
            in_home(".claude.json"),
            r#"{"mcpServers": {"linear": {"url": "https://mcp.linear.app/mcp"}}, "projects": {"E:/work/app": {}}}"#,
        )
        .with_file(mcp_json(), r#"{"mcpServers": {"github": {"command": "gh"}}}"#)
        .with_file(&settings, r#"{"disabledMcpjsonServers": ["github"],"#);
    let inventory = inventory(fs, &[]);

    assert_eq!(
        items(&inventory),
        [
            item("claude-code", "github", "effective"),
            item("claude-code", "linear", "effective")
        ]
    );
    let github = &inventory["items"][0]["entries"][0];
    assert_eq!(
        github["caveats"],
        json!([{
            "code": "switch_read_failure",
            "path": settings,
            "sentence": r"E:\work\app\.claude\settings.local.json couldn't be read, so whether it Disables this Entry is not known: it is shown as if it did not."
        }])
    );
    // Disable goes through a switch config-digger can read.
    assert_eq!(github["disable"]["status"], "offered");
    assert_eq!(github["disable"]["path"], json!(in_home(".claude.json")));
    // The store only Disables `.mcp.json` servers, but the file can hold
    // allow and deny lists covering every server.
    assert_eq!(
        inventory["items"][1]["entries"][0]["caveats"],
        json!([{
            "code": "setting_read_failure",
            "path": settings,
            "sentence": r"E:\work\app\.claude\settings.local.json couldn't be read, so whether a rule or gate there keeps this Entry out is not known: it is shown as if none did."
        }])
    );
}

/// The core over `fs`, with `definitions` loaded besides the built-in ones.
fn core(fs: FakeFileSystem, definitions: &[(&str, &str)]) -> Core {
    let env = FakeEnvironment::with_home(HOME)
        .with_var("LOCALAPPDATA", r"C:\Users\fixture\AppData\Local");
    Core::with_extra_definitions(fake::ports(fs, env), definitions).unwrap()
}

/// Plans `action` on `selector` in the Project.
fn plan(core: &Core, action: Action, selector: &str) -> Result<Plan, Error> {
    let options = ScanOptions {
        projects: vec![PathBuf::from(PROJECT)],
    };
    let snapshot = core.scan(&options).unwrap();
    let context = Context {
        project: Some(PathBuf::from(PROJECT)),
    };
    let targets = Targets {
        selectors: vec![selector.to_owned()],
        all: false,
        ..Targets::default()
    };
    core.plan(&snapshot, &context, action, &targets)
}

#[test]
fn disable_is_refused_when_every_switch_store_is_a_read_failure() {
    let fs = with_mcp_json(r#"{"mcpServers": {"github": {"command": "gh"}}}"#)
        .with_file(in_home(".claude.json"), UNPARSABLE_JSON)
        .with_file(
            PathBuf::from(PROJECT).join(r".claude\settings.local.json"),
            UNPARSABLE_JSON,
        );

    let error = plan(&core(fs, &[]), Action::Disable, "github")
        .err()
        .unwrap();

    assert_eq!(error.code, ErrorCode::Blocked);
    assert!(error.message.contains("cannot read"), "{}", error.message);
}

/// The rows of a preview about Read failures, without those about versions.
fn read_failure_info(plan: &Plan) -> Vec<&Info> {
    plan.preview()
        .info
        .iter()
        .filter(|info| matches!(info.code, InfoCode::ReadFailure | InfoCode::StillRejected))
        .collect()
}

#[test]
fn a_read_failure_that_could_change_the_outcome_adds_an_info_row_with_no_ack() {
    let fs = FakeFileSystem::new()
        .with_file(
            in_home(".claude.json"),
            r#"{"mcpServers": {"linear": {"url": "https://mcp.linear.app/mcp"}}}"#,
        )
        .with_file(mcp_json(), UNPARSABLE_JSON);

    let plan = plan(&core(fs, &[]), Action::Remove, "linear").unwrap();

    assert_eq!(
        serde_json::to_value(read_failure_info(&plan)).unwrap(),
        json!([{
            "code": "read_failure",
            "path": mcp_json(),
            "sentence": r"Claude Code couldn't read E:\work\app\.mcp.json, so its States after this change may differ."
        }])
    );
    assert_eq!(plan.warnings(), []);
    assert_eq!(plan.blocks(), []);
}

#[test]
fn writing_a_file_another_reader_rejects_notes_that_it_still_rejects_it() {
    let fs = with_mcp_json(
        "{\n  // the team's servers\n  \"mcpServers\": { \"github\": { \"command\": \"gh\" } }\n}\n",
    );

    let plan = plan(
        &core(fs, &[("jsonc-agent.toml", JSONC_AGENT)]),
        Action::Remove,
        "github",
    )
    .unwrap();

    assert_eq!(
        serde_json::to_value(read_failure_info(&plan)).unwrap(),
        json!([{
            "code": "still_rejected",
            "path": mcp_json(),
            "sentence": r"Claude Code still rejects E:\work\app\.mcp.json after this change: Claude Code cannot parse E:\work\app\.mcp.json as strict JSON (line 2, column 3), so it reads no Entries from it."
        }])
    );
}

#[test]
fn switches_beside_a_shape_read_failure_still_disable_and_are_written() {
    // Valid JSON, but Claude Code cannot enumerate its user servers.
    let claude_json = in_home(".claude.json");
    let fs = approved(FakeFileSystem::new())
        .with_file(
            &claude_json,
            r#"{"mcpServers": [], "projects": {"E:/work/app": {"disabledMcpServers": ["github"]}}}"#,
        )
        .with_file(mcp_json(), r#"{"mcpServers": {"github": {"command": "gh"}}}"#);
    assert_eq!(
        items(&inventory(fs.clone(), &[])),
        [item("claude-code", "github", "disabled")]
    );

    let plan = plan(&core(fs, &[]), Action::Enable, "github").unwrap();

    assert_eq!(plan.preview().files[0].path, claude_json);
    assert_eq!(
        serde_json::to_value(read_failure_info(&plan)).unwrap(),
        json!([{
            "code": "still_rejected",
            "path": claude_json,
            "sentence": r"After this change too, Claude Code cannot read the MCP servers at C:\Users\fixture\.claude.json#mcpServers: it holds a list, not an object, so it reads no Entries from there."
        }])
    );
}

#[test]
fn switches_in_a_file_that_is_a_read_failure_for_their_agent_disable_nothing_and_are_not_written() {
    let claude_json = in_home(".claude.json");
    // Strict JSON takes no trailing comma.
    let fs = approved(FakeFileSystem::new())
        .with_file(
            &claude_json,
            r#"{"projects": {"E:/work/app": {"disabledMcpServers": ["github"]}},}"#,
        )
        .with_file(
            mcp_json(),
            r#"{"mcpServers": {"github": {"command": "gh"}}}"#,
        );
    let inventory = inventory(fs, &[]);

    assert_eq!(
        items(&inventory),
        [item("claude-code", "github", "effective")]
    );
    let github = &inventory["items"][0]["entries"][0];
    assert_eq!(github["caveats"][0]["path"], json!(claude_json));
    assert_eq!(
        github["disable"]["path"],
        json!(PathBuf::from(PROJECT).join(r".claude\settings.local.json"))
    );
}
