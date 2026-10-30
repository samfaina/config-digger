//! `unverified` marks in definitions loaded through the core `api`, as
//! `--definitions <dir>` loads them (ADR 0005): a mark names a fact taken
//! from inference, and changes nothing a scan, a plan or a write produces.

use config_digger_core::api::{Action, Context, Core, ScanOptions, Targets};
use config_digger_core::ports::fake::{self, FakeEnvironment, FakeFileSystem};
use std::path::{Path, PathBuf};

const HOME: &str = r"C:\Users\fixture";
const DEFINITIONS: &str = r"E:\dev\definitions";

fn in_home(relative: &str) -> PathBuf {
    PathBuf::from(HOME).join(relative)
}

fn definition_file(relative: &str) -> PathBuf {
    Path::new(DEFINITIONS).join(relative)
}

/// An Agent whose servers are in `~/.alpha.json`.
const ALPHA: &str = r#"
format_version = 1

[agent]
id = "alpha"
name = "Alpha"

[[homes]]
id = "default"
folder = { block = "path", path = "~" }
roots.config = ".alpha.json"

[[scopes]]
id = "user"

[[locations]]
id = "user-mcp"
anchor = { block = "home", root = "config" }
format = { block = "json", dialect = "strict" }
root = ["mcpServers"]
kind = "mcp"
scope = "user"
provenance = "file"
store_class = "plain"
on_read_failure = "unknown"

[items.mcp]
precedence = ["user-mcp"]
competition = { block = "exact-name" }
checks = ["disable", "precedence"]
"#;

/// [`ALPHA`] with a mark on the file, on a table, on an inline table and
/// on an entry of an array of tables.
fn marked_alpha() -> String {
    ALPHA
        .replacen(
            "format_version = 1",
            "format_version = 1\nunverified = \"Alpha keeps one home only.\"",
            1,
        )
        .replacen(
            r#"{ block = "home", root = "config" }"#,
            r#"{ block = "home", root = "config", unverified = "Alpha reads no other file." }"#,
            1,
        )
        .replacen(
            "on_read_failure = \"unknown\"",
            "on_read_failure = \"unknown\"\nunverified = [\"Alpha reads no folder above the home.\", \"Alpha's reaction to a file it cannot parse.\"]",
            1,
        )
        .replacen(
            "[items.mcp]",
            "[items.mcp]\nunverified = \"The first server of a name wins.\"",
            1,
        )
}

const SERVERS: &str = r#"{
  "mcpServers": {
    "linear": { "url": "https://mcp.linear.app/mcp" },
    "notes": { "command": "notes-mcp" }
  }
}
"#;

/// What a scan, a plan to Remove `linear` and its write produce with
/// `definition` loaded from the definitions folder: the inventory, the
/// preview and the file the write leaves.
fn scan_plan_and_write(definition: &str) -> (serde_json::Value, serde_json::Value, String) {
    let fs = FakeFileSystem::new()
        .with_file(in_home(".alpha.json"), SERVERS)
        .with_file(definition_file("alpha.toml"), definition);
    let ports = fake::ports(fs.clone(), FakeEnvironment::with_home(HOME));
    let loaded = Core::with_definitions_folder(ports, Path::new(DEFINITIONS)).unwrap();
    assert!(loaded.rejected.is_empty(), "{:?}", loaded.rejected);
    let core = loaded.core;

    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    let inventory = core.resolve(&snapshot, &Context::default());
    let plan = core
        .plan(
            &snapshot,
            &Context::default(),
            Action::Remove,
            &Targets {
                selectors: vec!["linear".to_owned()],
                all: false,
                ..Targets::default()
            },
        )
        .unwrap();
    let preview = serde_json::to_value(plan.preview()).unwrap();
    core.execute(plan, &[]).unwrap();
    let written = String::from_utf8(fs.contents(in_home(".alpha.json")).unwrap()).unwrap();

    let alpha: Vec<_> = inventory
        .items
        .iter()
        .filter(|item| item.agent == "alpha")
        .collect();
    (serde_json::to_value(alpha).unwrap(), preview, written)
}

#[test]
fn marks_change_nothing_a_scan_a_plan_or_a_write_produces() {
    let unmarked = scan_plan_and_write(ALPHA);

    let marked = scan_plan_and_write(&marked_alpha());

    assert_eq!(marked.0, unmarked.0);
    assert_eq!(marked.1, unmarked.1);
    assert_eq!(marked.2, unmarked.2);
    assert!(!marked.2.contains("linear"), "{}", marked.2);
}

#[test]
fn a_mark_without_a_sentence_rejects_the_definition() {
    let alpha = |id: &str| ALPHA.replace(r#"id = "alpha""#, &format!(r#"id = "{id}""#));
    let empty = alpha("empty").replacen("[items.mcp]", "[items.mcp]\nunverified = \"\"", 1);
    let blank = alpha("blank").replacen("[[scopes]]", "[[scopes]]\nunverified = \"  \"", 1);
    let not_text = alpha("not-text").replacen("[agent]", "[agent]\nunverified = true", 1);
    let no_sentences = alpha("no-sentences").replacen("[agent]", "[agent]\nunverified = []", 1);
    let one_empty = alpha("one-empty").replacen(
        "[agent]",
        "[agent]\nunverified = [\"Alpha has one name.\", \"\"]",
        1,
    );
    let fs = FakeFileSystem::new()
        .with_file(definition_file("blank.toml"), blank)
        .with_file(definition_file("empty.toml"), empty)
        .with_file(definition_file("no-sentences.toml"), no_sentences)
        .with_file(definition_file("not-text.toml"), not_text)
        .with_file(definition_file("one-empty.toml"), one_empty);
    let ports = fake::ports(fs, FakeEnvironment::with_home(HOME));

    let loaded = Core::with_definitions_folder(ports, Path::new(DEFINITIONS)).unwrap();

    let rejected: Vec<(&Path, &str)> = loaded
        .rejected
        .iter()
        .map(|rejected| (rejected.file.as_path(), rejected.message.as_str()))
        .collect();
    assert_eq!(rejected.len(), 5, "{rejected:#?}");
    assert_eq!(rejected[0].0, definition_file("blank.toml"));
    assert!(rejected[0].1.contains("scopes[user]"), "{}", rejected[0].1);
    assert_eq!(rejected[1].0, definition_file("empty.toml"));
    assert!(rejected[1].1.contains("items.mcp"), "{}", rejected[1].1);
    for (index, file) in [
        (2, "no-sentences.toml"),
        (3, "not-text.toml"),
        (4, "one-empty.toml"),
    ] {
        assert_eq!(rejected[index].0, definition_file(file));
        assert!(rejected[index].1.contains("agent"), "{}", rejected[index].1);
    }
    for (_, message) in &rejected {
        assert!(message.contains("unverified"), "{message}");
    }
}

#[test]
fn a_mark_under_the_exception_keeps_what_its_test_needs() {
    let alpha = ALPHA.replacen(
        "[items.mcp]",
        "[items.mcp]\nunverified = [\n  \"The first server of a name wins.\",\n  { sentence = \"Alpha's enterprise servers win.\", needs = \"an Alpha enterprise account\" },\n]",
        1,
    );

    let marks = config_digger_core::definitions::agent_marks("alpha.toml", &alpha).unwrap();

    let found: Vec<(&str, &str, Option<&str>)> = marks
        .iter()
        .map(|mark| {
            (
                mark.location.as_str(),
                mark.sentence.as_str(),
                mark.needs.as_deref(),
            )
        })
        .collect();
    assert_eq!(
        found,
        [
            ("items.mcp", "The first server of a name wins.", None),
            (
                "items.mcp",
                "Alpha's enterprise servers win.",
                Some("an Alpha enterprise account")
            ),
        ]
    );
    let line = alpha.lines().nth(marks[1].line - 1).unwrap();
    assert!(line.contains("enterprise servers"), "{line}");
    assert!(
        marks[1].to_string().ends_with(
            ": items.mcp: Alpha's enterprise servers win. Its test needs an Alpha enterprise account."
        ),
        "{}",
        marks[1]
    );
}

#[test]
fn a_mark_under_the_exception_without_a_sentence_or_a_need_rejects_the_definition() {
    let alpha = |mark: &str| ALPHA.replacen("[agent]", &format!("[agent]\nunverified = {mark}"), 1);
    for (mark, word) in [
        (r#"{ needs = "an Alpha enterprise account" }"#, "sentence"),
        (r#"{ sentence = "Alpha has one name." }"#, "needs"),
        (
            r#"{ sentence = "Alpha has one name.", needs = " " }"#,
            "needs",
        ),
        (
            r#"{ sentence = "Alpha has one name.", needs = 1 }"#,
            "needs",
        ),
        (
            r#"{ sentence = "Alpha has one name.", account = "Alpha" }"#,
            "account",
        ),
    ] {
        let error = config_digger_core::definitions::agent_marks("alpha.toml", &alpha(mark))
            .expect_err(mark);

        assert!(error.contains("agent.unverified"), "{mark}: {error}");
        assert!(error.contains(word), "{mark}: {error}");
    }
}

#[test]
fn a_skill_manager_definition_may_carry_marks() {
    let acme = r#"
format_version = 1
unverified = "Acme never moves a skill it installed."

[manager]
id = "acme"
name = "Acme skills"

[[detectors]]
block = "marker-file"
file = ".acme.json"
strength = "strong"
unverified = "Acme writes `.acme.json` into every skill."
"#;
    let fs = FakeFileSystem::new().with_file(definition_file(r"skill-managers\acme.toml"), acme);
    let ports = fake::ports(fs, FakeEnvironment::with_home(HOME));

    let loaded = Core::with_definitions_folder(ports, Path::new(DEFINITIONS)).unwrap();

    assert!(loaded.rejected.is_empty(), "{:?}", loaded.rejected);
}

/// Every object schema in `schema`, the root included.
fn objects(schema: &serde_json::Value) -> Vec<&serde_json::Value> {
    let mut found = Vec::new();
    match schema {
        serde_json::Value::Object(map) => {
            if map.get("type") == Some(&serde_json::json!("object")) {
                found.push(schema);
            }
            for value in map.values() {
                found.extend(objects(value));
            }
        }
        serde_json::Value::Array(values) => {
            for value in values {
                found.extend(objects(value));
            }
        }
        _ => {}
    }
    found
}

#[test]
fn both_schemas_give_every_table_a_mark_of_sentences() {
    for schema in [
        config_digger_core::definitions::json_schema(),
        config_digger_core::definitions::skill_managers::json_schema(),
    ] {
        let schema: serde_json::Value = serde_json::from_str(&schema).unwrap();

        // A mark, or a list of at least one. A mark is a sentence, not empty
        // nor only blanks, or a table of its sentence and what its test
        // needs.
        let sentence = serde_json::json!({ "type": "string", "pattern": r"\S" });
        let mark = serde_json::json!({
            "anyOf": [
                sentence,
                {
                    "type": "object",
                    "properties": { "sentence": sentence, "needs": sentence },
                    "required": ["sentence", "needs"],
                    "additionalProperties": false,
                },
            ],
        });
        let unverified = &schema["$defs"]["Unverified"];
        assert_eq!(
            unverified["anyOf"],
            serde_json::json!([
                mark["anyOf"][0],
                mark["anyOf"][1],
                { "type": "array", "items": mark, "minItems": 1 },
            ]),
            "{unverified:#}"
        );
        let mut schema = schema.clone();
        schema["$defs"]
            .as_object_mut()
            .unwrap()
            .remove("Unverified");
        let objects = objects(&schema);
        assert!(objects.len() > 3, "{}", objects.len());
        for object in objects {
            assert_eq!(
                object["properties"]["unverified"],
                serde_json::json!({ "$ref": "#/$defs/Unverified" }),
                "{object:#}"
            );
        }
    }
}

#[test]
fn the_built_in_marks_are_listed_with_their_file_location_and_sentence() {
    let marks = config_digger_core::definitions::unverified_marks().unwrap();

    // Facts of the built-in definitions no test has reached yet, by file,
    // table and a word of each.
    for (file, location, word) in [
        (
            "claude-desktop.toml",
            "items.mcp.rules[1]",
            "third-party provider",
        ),
        (
            "claude-code.toml",
            "locations[managed-settings-mcp]",
            "remote-settings.json",
        ),
        (
            "claude-code.toml",
            "locations[managed-settings-mcp]",
            "dropped `managedMcpServers` entry",
        ),
        ("claude-code.toml", "plugins[synced]", "`~g<generation>`"),
        ("claude-code.toml", "account.off", "interactive session"),
        (
            "claude-code.toml",
            "locations[plugin-skills]",
            "while `tengu_slate_wicket` is off",
        ),
        (
            "claude-code.toml",
            "locations[synced-plugin-skills]",
            "named as an installed plugin's are.",
        ),
    ] {
        let mark = marks
            .iter()
            .find(|mark| {
                mark.file == file && mark.location == location && mark.sentence.contains(word)
            })
            .unwrap_or_else(|| panic!("no mark at {file} {location}: {marks:#?}"));
        let text = match file {
            "claude-desktop.toml" => include_str!("../definitions/claude-desktop.toml"),
            _ => include_str!("../definitions/claude-code.toml"),
        };
        let line = text.lines().nth(mark.line - 1).unwrap();
        assert!(line.contains(word), "{file}:{}: {line}", mark.line);
    }
}

#[test]
fn the_built_in_marks_under_the_exception_say_what_their_test_needs() {
    let marks = config_digger_core::definitions::unverified_marks().unwrap();

    let under: Vec<(&str, &str, &str)> = marks
        .iter()
        .filter_map(|mark| {
            Some((
                mark.file.as_str(),
                mark.location.as_str(),
                mark.needs.as_deref()?,
            ))
        })
        .collect();

    // #80's exception: a test that needs an account or a deployment the
    // project does not have.
    let teams = "Claude Teams or Enterprise";
    let codex = "a Codex enterprise account with remote plugins";
    assert_eq!(
        under,
        [
            ("claude-code.toml", "settings[managed]", teams),
            ("claude-code.toml", "locations[managed-settings-mcp]", teams),
            (
                "claude-desktop.toml",
                "items.mcp.rules[1]",
                "a Claude Desktop deployment for a third-party provider"
            ),
            ("cursor.toml", "plugins[project]", "a Cursor team account"),
            ("codex.toml", "locations[cloud-mcp]", codex),
            ("codex.toml", "locations[cloud-mcp]", codex),
            ("codex.toml", "locations[remote-plugin-mcp]", codex),
            ("codex.toml", "items.mcp.disable[1]", codex),
            ("codex.toml", "locations[remote-plugin-skills]", codex),
        ]
    );
}
