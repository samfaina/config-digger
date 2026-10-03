//! Read failures through the core `api`, with fake ports: each location
//! parsed in each reader's dialect, broken fixture files per dialect, and
//! what each Agent does about a location it cannot read.

use config_digger_core::api::{Context, Core, ScanOptions};
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
roots.state = { block = "path", path = "~/.jsonc-agent" }

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
roots.config = {{ block = "path", path = "~/{file}" }}

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
fn broken_toml_is_a_read_failure_with_its_position_and_no_entries() {
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
fn servers_that_are_not_an_object_are_a_read_failure() {
    let inventory = user_inventory("agent.json", STRICT, r#"{"servers": [{"command": "a"}]}"#);

    assert_eq!(items(&inventory), []);
    assert_eq!(failures(&inventory), [("parse".to_owned(), Value::Null)]);
}

#[test]
fn a_document_that_is_not_an_object_is_a_read_failure() {
    let inventory = user_inventory("agent.json", STRICT, "[]");

    assert_eq!(failures(&inventory), [("parse".to_owned(), Value::Null)]);
}

#[test]
fn a_document_without_servers_is_no_read_failure() {
    let inventory = user_inventory("agent.json", STRICT, r#"{"other": 1}"#);

    assert_eq!(items(&inventory), []);
    assert_eq!(failures(&inventory), []);
}

#[test]
fn a_project_key_that_is_not_an_object_is_a_read_failure() {
    let inventory = inventory(
        FakeFileSystem::new().with_file(
            in_home(".claude.json"),
            r#"{"mcpServers": {"a": {"command": "a"}}, "projects": {"E:/work/app": "broken"}}"#,
        ),
        &[],
    );

    assert_eq!(items(&inventory), []);
    assert_eq!(failures(&inventory), [("parse".to_owned(), Value::Null)]);
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

const BROKEN_JSON: &str = r#"{"mcpServers": {"#;

#[test]
fn claude_code_asks_about_a_claude_json_it_cannot_parse_and_may_reset_it() {
    let fs = FakeFileSystem::new()
        .with_file(in_home(".claude.json"), BROKEN_JSON)
        .with_file(
            mcp_json(),
            r#"{"mcpServers": {"github": {"command": "gh"}}}"#,
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
    let fs = with_mcp_json(BROKEN_JSON);
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
    let fs = || FakeFileSystem::new().with_file(in_home("agent.json"), BROKEN_JSON);
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
    let in_project = inventory_in(with_mcp_json(BROKEN_JSON), &definitions, Some(PROJECT));
    let without_project = inventory_in(with_mcp_json(BROKEN_JSON), &definitions, None);

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
    .replace("roots.config = ", "roots.extra = { block = \"path\", path = \"~/extra.json\" }\nroots.config = ")
    .replace(r#"precedence = ["user-mcp"]"#, r#"precedence = ["extra-mcp", "user-mcp"]"#);
    let fs = FakeFileSystem::new()
        .with_file(in_home("agent.json"), BROKEN_JSON)
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
    let fs = || {
        FakeFileSystem::new()
            .with_file(
                in_home(".claude.json"),
                r#"{"mcpServers": {"linear": {"url": "https://mcp.linear.app/mcp"}}}"#,
            )
            .with_file(mcp_json(), BROKEN_JSON)
    };
    let in_project = inventory_in(fs(), &[], Some(PROJECT));
    let without_project = inventory_in(fs(), &[], None);

    assert_eq!(in_project["read_failures"][0]["reaction"], "unknown");
    assert_eq!(
        in_project["items"][0]["caveats"],
        json!([{
            "code": "unreadable_location",
            "path": mcp_json(),
            "sentence": r"A location that could take part couldn't be read: E:\work\app\.mcp.json, so the State shown may not be the one Claude Code has."
        }])
    );
    assert_eq!(without_project["items"][0]["caveats"], json!([]));
    assert_eq!(in_project["agent_flags"], json!([]));
}
