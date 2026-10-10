//! Claude Code's local, project and user Scopes resolved per Project through
//! the core `api`, with fake ports: snapshot fixture in, States out.

use config_digger_core::api::{Context, Core, ErrorCode, ScanOptions, Snapshot};
use config_digger_core::ports::fake::{self, FakeEnvironment, FakeFileSystem};
use serde_json::{Value, json};
use std::path::PathBuf;

const HOME: &str = r"C:\Users\fixture";
const PETME: &str = r"E:\projects\petme-app";
const CLAUDE_JSON: &[u8] = include_bytes!("fixtures/claude-code-projects/home/.claude.json");
const MCP_JSON: &[u8] = include_bytes!("fixtures/claude-code-projects/petme-app/.mcp.json");
const SETTINGS: &[u8] =
    include_bytes!("fixtures/claude-code-projects/petme-app/.claude/settings.json");

/// petme-app approves every `.mcp.json` server in its `settings.json`.
fn fixture() -> FakeFileSystem {
    FakeFileSystem::new()
        .with_file(PathBuf::from(HOME).join(".claude.json"), CLAUDE_JSON)
        .with_file(PathBuf::from(PETME).join(".mcp.json"), MCP_JSON)
        .with_file(
            PathBuf::from(PETME).join(r".claude\settings.json"),
            SETTINGS,
        )
}

fn core(fs: FakeFileSystem) -> Core {
    Core::new(fake::ports(fs, FakeEnvironment::with_home(HOME))).unwrap()
}

fn context(project: Option<&str>) -> Context {
    Context {
        project: project.map(PathBuf::from),
    }
}

fn resolve(core: &Core, snapshot: &Snapshot, project: Option<&str>) -> Value {
    serde_json::to_value(core.resolve(snapshot, &context(project))).unwrap()
}

fn inventory(fs: FakeFileSystem, project: Option<&str>) -> Value {
    let core = core(fs);
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    resolve(&core, &snapshot, project)
}

/// `(name, [(scope, state)])` per Item, Entries in precedence order.
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
fn in_a_project_local_beats_project_beats_user() {
    let inventory = inventory(fixture(), Some(PETME));

    assert_eq!(
        states(&inventory),
        expected(&[
            ("dart", &[("local", "effective")]),
            ("flutter", &[("local", "effective")]),
            (
                "github",
                &[
                    ("local", "effective"),
                    ("project", "shadowed"),
                    ("user", "shadowed")
                ]
            ),
            ("linear", &[("project", "effective"), ("user", "shadowed")]),
            ("notes", &[("user", "effective")]),
            ("supabase", &[("project", "effective")]),
        ])
    );
}

#[test]
fn with_no_project_only_user_entries_take_part() {
    let inventory = inventory(fixture(), None);

    assert_eq!(
        states(&inventory),
        expected(&[
            ("github", &[("user", "effective")]),
            ("linear", &[("user", "effective")]),
            ("notes", &[("user", "effective")]),
        ])
    );
    assert_eq!(inventory["project"], Value::Null);
}

#[test]
fn a_shadowed_entry_points_at_the_effective_entry_with_a_reason() {
    let inventory = inventory(fixture(), Some(PETME));

    let linear = item(&inventory, "linear");
    assert_eq!(linear["entries"][0]["reason"], Value::Null);
    assert_eq!(
        linear["entries"][1]["reason"],
        json!({
            "code": "shadowed_by_precedence",
            "effective": r"E:\projects\petme-app\.mcp.json#mcpServers.linear",
            "switch": null,
            "sentence": r"Shadowed by the project Entry E:\projects\petme-app\.mcp.json#mcpServers.linear, which Claude Code ranks higher."
        })
    );
}

#[test]
fn local_entries_keep_the_agents_own_spelling_of_the_project_key() {
    let inventory = inventory(fixture(), Some(PETME));

    let github = item(&inventory, "github");
    assert_eq!(
        github["entries"][0]["key_path"],
        json!(["projects", "E:/projects/petme-app", "mcpServers", "github"])
    );
    assert_eq!(
        github["entries"][0]["locator"],
        r"C:\Users\fixture\.claude.json#projects.E:/projects/petme-app.mcpServers.github"
    );
    assert_eq!(
        item(&inventory, "flutter")["entries"][0]["key_path"],
        json!(["projects", "e:/projects/petme-app", "mcpServers", "flutter"])
    );
}

#[test]
fn entries_carry_their_provenance() {
    let inventory = inventory(fixture(), Some(PETME));

    for entry in item(&inventory, "github")["entries"].as_array().unwrap() {
        assert_eq!(entry["provenance"], "file", "{entry}");
    }
}

#[test]
fn every_spelling_of_one_folder_is_one_project() {
    let inventory = inventory(fixture(), None);

    assert_eq!(
        inventory["projects"],
        json!([
            { "path": r"E:\projects\gone", "missing": true, "workspace_file": false, "members": [], "registered_by": ["claude-code"], "added": false, "hidden": false, "entries": 1 },
            { "path": PETME, "missing": false, "workspace_file": false, "members": [], "registered_by": ["claude-code"], "added": false, "hidden": false, "entries": 6 },
        ])
    );
}

#[test]
fn the_context_project_matches_any_spelling() {
    for spelling in [
        "e:/projects/petme-app",
        r"E:\Projects\PETME-APP\",
        "/E:/projects/petme-app",
        r"E:\projects\other\..\petme-app\.",
    ] {
        let inventory = inventory(fixture(), Some(spelling));

        assert_eq!(inventory["project"], PETME, "{spelling}");
        assert_eq!(
            item(&inventory, "dart")["entries"][0]["state"],
            "effective",
            "{spelling}"
        );
    }
}

#[test]
fn an_existing_folder_is_compared_by_its_real_path() {
    let fs = fixture().with_link(r"E:\work\petme", PETME);
    let core = core(fs);
    let snapshot = core
        .scan(&ScanOptions {
            projects: vec![PathBuf::from(r"E:\work\petme")],
        })
        .unwrap();

    let inventory = resolve(&core, &snapshot, Some(r"E:\work\petme"));

    assert_eq!(inventory["project"], PETME);
    assert_eq!(item(&inventory, "dart")["entries"][0]["state"], "effective");
    assert_eq!(
        item(&inventory, "supabase")["entries"][0]["path"],
        r"E:\projects\petme-app\.mcp.json"
    );
    assert_eq!(inventory["projects"].as_array().unwrap().len(), 2);
}

#[test]
fn a_missing_project_is_matched_lexically() {
    let inventory = inventory(fixture(), Some(r"e:\projects\gone\"));

    assert_eq!(inventory["project"], r"E:\projects\gone");
    assert_eq!(
        states(&inventory),
        expected(&[
            ("github", &[("user", "effective")]),
            ("linear", &[("user", "effective")]),
            ("notes", &[("user", "effective")]),
            ("old", &[("local", "unread")]),
        ])
    );
}

#[test]
fn a_project_no_registry_knows_is_evaluated_when_asked_for() {
    let fs = fixture()
        .with_file(
            r"E:\scratch\.mcp.json",
            r#"{"mcpServers": {"scratchpad": {"command": "x"}}}"#,
        )
        .with_file(
            r"E:\scratch\.claude\settings.local.json",
            r#"{"enabledMcpjsonServers": ["scratchpad"]}"#,
        );
    let core = core(fs);
    let snapshot = core
        .scan(&ScanOptions {
            projects: vec![PathBuf::from(r"E:\scratch")],
        })
        .unwrap();

    let inventory = resolve(&core, &snapshot, Some(r"E:\scratch"));

    assert_eq!(inventory["project"], r"E:\scratch");
    assert_eq!(
        item(&inventory, "scratchpad")["entries"][0]["state"],
        "effective"
    );
    let scratch = inventory["projects"]
        .as_array()
        .unwrap()
        .iter()
        .find(|project| project["path"] == r"E:\scratch")
        .unwrap();
    assert_eq!(scratch["registered_by"], json!([]));
}

#[test]
fn a_project_that_was_not_scanned_still_resolves_its_user_entries() {
    let inventory = inventory(fixture(), Some(r"E:\elsewhere"));

    assert_eq!(inventory["project"], r"E:\elsewhere");
    assert_eq!(
        states(&inventory),
        expected(&[
            ("github", &[("user", "effective")]),
            ("linear", &[("user", "effective")]),
            ("notes", &[("user", "effective")]),
        ])
    );
}

#[test]
fn switching_context_does_not_read_the_disk() {
    let fs = fixture();
    let core = core(fs.clone());
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    let accesses = fs.accesses();

    for project in [Some(PETME), None, Some(r"E:\projects\gone"), Some(PETME)] {
        resolve(&core, &snapshot, project);
    }

    assert!(accesses > 0);
    assert_eq!(fs.accesses(), accesses);
}

#[test]
fn two_spellings_of_one_project_key_tie_and_the_reason_says_so() {
    let claude_json = r#"{"projects": {
        "E:/projects/petme-app": {"mcpServers": {"dart": {"command": "a"}}},
        "e:\\projects\\petme-app": {"mcpServers": {"dart": {"command": "b"}}}
    }}"#;
    let fs = FakeFileSystem::new()
        .with_file(PathBuf::from(HOME).join(".claude.json"), claude_json)
        .with_folder(PETME);

    let inventory = inventory(fs, Some(PETME));

    let dart = item(&inventory, "dart");
    assert_eq!(
        states(&inventory),
        expected(&[("dart", &[("local", "effective"), ("local", "shadowed")])])
    );
    assert_eq!(
        dart["entries"][1]["reason"]["sentence"],
        r"Shadowed by the local Entry C:\Users\fixture\.claude.json#projects.E:/projects/petme-app.mcpServers.dart, which has the same precedence in Claude Code; which of the two it uses is not known."
    );
}

#[test]
fn registry_keys_that_are_not_absolute_paths_are_not_projects() {
    let claude_json = r#"{"projects": {
        "": {"mcpServers": {"empty": {"command": "a"}}},
        "rel/x": {"mcpServers": {"relative": {"command": "b"}}},
        "E:/projects/petme-app": {}
    }}"#;
    let fs = FakeFileSystem::new()
        .with_file(PathBuf::from(HOME).join(".claude.json"), claude_json)
        .with_folder(PETME)
        .with_folder(r"rel\x");

    let inventory = inventory(fs, Some("rel/x"));

    assert_eq!(
        inventory["projects"],
        json!([{ "path": PETME, "missing": false, "workspace_file": false, "members": [], "registered_by": ["claude-code"], "added": false, "hidden": false, "entries": 0 }])
    );
    assert_eq!(inventory["items"], json!([]));
}

#[test]
fn a_file_read_by_several_locations_is_one_read_failure() {
    let fs = FakeFileSystem::new().with_file(PathBuf::from(HOME).join(".claude.json"), "{ nope");

    let inventory = inventory(fs, None);

    assert_eq!(inventory["read_failures"].as_array().unwrap().len(), 1);
}

#[test]
fn show_finds_an_item_by_name() {
    let core = core(fixture());
    let snapshot = core.scan(&ScanOptions::default()).unwrap();

    let github = core
        .show(&snapshot, &context(Some(PETME)), "github")
        .unwrap();

    assert_eq!(github.name, "github");
    assert_eq!(github.entries.len(), 3);
}

#[test]
fn show_finds_an_item_by_the_locator_of_any_entry() {
    let core = core(fixture());
    let snapshot = core.scan(&ScanOptions::default()).unwrap();

    for locator in [
        r"C:\Users\fixture\.claude.json#mcpServers.github",
        r"c:/users/FIXTURE/.claude.json#mcpServers.github",
        r"E:\projects\petme-app\.mcp.json#mcpServers.github",
        r"C:\Users\fixture\.claude.json#projects.E:/projects/petme-app.mcpServers.github",
        r#"C:\Users\fixture\.claude.json#"projects"."E:/projects/petme-app".mcpServers."github""#,
    ] {
        let item = core
            .show(&snapshot, &context(Some(PETME)), locator)
            .unwrap_or_else(|e| panic!("{locator}: {e}"));

        assert_eq!(item.name, "github", "{locator}");
    }
}

#[test]
fn show_of_an_unknown_selector_is_no_match() {
    let core = core(fixture());
    let snapshot = core.scan(&ScanOptions::default()).unwrap();

    for selector in [
        "nothing",
        // `dart` is a local Entry, which takes no part without a Project.
        "dart",
        r"C:\Users\fixture\.claude.json#mcpServers.nothing",
    ] {
        let error = core.show(&snapshot, &context(None), selector).unwrap_err();

        assert_eq!(error.code, ErrorCode::NoMatch, "{selector}");
        assert!(error.message.contains(selector), "{}", error.message);
    }
}
