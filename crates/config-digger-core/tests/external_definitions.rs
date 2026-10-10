//! Extra definitions loaded from a folder through the core `api`, as
//! `--definitions <dir>` loads them for development and tests (ADR 0005):
//! each is validated on load and rejected whole on any error, and the
//! others keep working.

use config_digger_core::api::{Context, Core, ScanOptions};
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

/// An Agent whose servers are in `~/.extra.json`.
fn agent(id: &str) -> String {
    format!(
        r#"
format_version = 1

[agent]
id = "{id}"
name = "{id}"

[[homes]]
id = "default"
folder = {{ block = "path", path = "~" }}
roots.config = ".{id}.json"

[[scopes]]
id = "user"

[[locations]]
id = "user-mcp"
anchor = {{ block = "home", root = "config" }}
format = {{ block = "json", dialect = "strict" }}
root = ["mcpServers"]
kind = "mcp"
scope = "user"
provenance = "file"
store_class = "plain"
on_read_failure = "unknown"

[items.mcp]
precedence = ["user-mcp"]
competition = {{ block = "exact-name" }}
checks = ["disable", "precedence"]
"#
    )
}

fn agent_ids(core: &Core) -> Vec<(String, bool)> {
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    let inventory = core.resolve(&snapshot, &Context::default());
    core.agents(&inventory)
        .into_iter()
        .map(|agent| (agent.id, agent.built_in))
        .collect()
}

#[test]
fn a_definitions_folder_adds_its_agents_after_the_built_in_ones() {
    let fs = FakeFileSystem::new()
        .with_file(definition_file("alpha.toml"), agent("alpha"))
        .with_file(definition_file("beta.toml"), agent("beta"));
    let ports = fake::ports(fs, FakeEnvironment::with_home(HOME));

    let loaded = Core::with_definitions_folder(ports, Path::new(DEFINITIONS)).unwrap();

    assert!(loaded.rejected.is_empty(), "{:?}", loaded.rejected);
    let agents = agent_ids(&loaded.core);
    let extra: Vec<_> = agents.iter().filter(|(_, built_in)| !built_in).collect();
    assert_eq!(
        extra,
        [&("alpha".to_owned(), false), &("beta".to_owned(), false)]
    );
    assert!(
        agents
            .iter()
            .any(|(id, built_in)| id == "claude-code" && *built_in)
    );
}

#[test]
fn an_invalid_definition_is_rejected_whole_and_the_others_keep_working() {
    let unknown_block = agent("gamma").replace("\"exact-name\"", "\"guess-name\"");
    let unknown_parameter = agent("delta").replace(
        "store_class = \"plain\"",
        "store_class = \"plain\"\nshiny = true",
    );
    let fs = FakeFileSystem::new()
        .with_file(
            in_home(".alpha.json"),
            r#"{"mcpServers": {"linear": {"command": "l"}}}"#,
        )
        .with_file(definition_file("alpha.toml"), agent("alpha"))
        .with_file(definition_file("gamma.toml"), unknown_block)
        .with_file(definition_file("delta.toml"), unknown_parameter);
    let ports = fake::ports(fs, FakeEnvironment::with_home(HOME));

    let loaded = Core::with_definitions_folder(ports, Path::new(DEFINITIONS)).unwrap();

    let rejected: Vec<(&Path, &str)> = loaded
        .rejected
        .iter()
        .map(|rejected| (rejected.file.as_path(), rejected.message.as_str()))
        .collect();
    assert_eq!(rejected.len(), 2, "{rejected:?}");
    assert_eq!(rejected[0].0, definition_file("delta.toml"));
    assert!(rejected[0].1.contains("shiny"), "{}", rejected[0].1);
    assert_eq!(rejected[1].0, definition_file("gamma.toml"));
    assert!(rejected[1].1.contains("guess-name"), "{}", rejected[1].1);
    let agents = agent_ids(&loaded.core);
    assert!(agents.iter().any(|(id, _)| id == "alpha"), "{agents:?}");
    assert!(!agents.iter().any(|(id, _)| id == "gamma" || id == "delta"));
    let snapshot = loaded.core.scan(&ScanOptions::default()).unwrap();
    let inventory = loaded.core.resolve(&snapshot, &Context::default());
    assert!(
        inventory
            .items
            .iter()
            .any(|item| item.name == "linear" && item.agent == "alpha"),
        "{:?}",
        inventory
            .items
            .iter()
            .map(|item| &item.name)
            .collect::<Vec<_>>()
    );
}

/// A skill manager that writes `.acme.json` into each skill it installs.
const ACME_MANAGER: &str = r#"
format_version = 1

[manager]
id = "acme"
name = "Acme skills"

[[detectors]]
block = "marker-file"
file = ".acme.json"
strength = "strong"
"#;

#[test]
fn a_skill_managers_folder_adds_skill_manager_definitions() {
    let broken = ACME_MANAGER
        .replace("\"acme\"", "\"broken\"")
        .replace("marker-file", "marker-glob");
    let skill = in_home(r".claude\skills\deploy");
    let fs = FakeFileSystem::new()
        .with_file(in_home(".claude.json"), "{}")
        .with_file(
            skill.join("SKILL.md"),
            "---\nname: deploy\ndescription: Deploys.\n---\n",
        )
        .with_file(skill.join(".acme.json"), "{}")
        .with_file(definition_file(r"skill-managers\acme.toml"), ACME_MANAGER)
        .with_file(definition_file(r"skill-managers\broken.toml"), broken);
    let ports = fake::ports(fs, FakeEnvironment::with_home(HOME));

    let loaded = Core::with_definitions_folder(ports, Path::new(DEFINITIONS)).unwrap();

    assert_eq!(loaded.rejected.len(), 1, "{:?}", loaded.rejected);
    assert_eq!(
        loaded.rejected[0].file,
        definition_file(r"skill-managers\broken.toml")
    );
    assert!(
        loaded.rejected[0]
            .message
            .starts_with("skill manager definition"),
        "{}",
        loaded.rejected[0].message
    );
    assert!(
        loaded.rejected[0].message.contains("marker-glob"),
        "{}",
        loaded.rejected[0].message
    );
    let snapshot = loaded.core.scan(&ScanOptions::default()).unwrap();
    let inventory = loaded.core.resolve(&snapshot, &Context::default());
    let deploy = inventory
        .items
        .iter()
        .find(|item| item.agent == "claude-code" && item.name == "deploy")
        .expect("Claude Code reads the skill");
    let entry = serde_json::to_value(&deploy.entries[0]).unwrap();
    assert_eq!(entry["provenance"], "skill_manager");
    assert_eq!(
        entry["skill_manager"],
        serde_json::json!({ "id": "acme", "name": "Acme skills" })
    );
}

#[test]
fn a_definition_whose_id_is_taken_is_rejected() {
    let acme_again = ACME_MANAGER.replace("Acme skills", "Acme again");
    let fs = FakeFileSystem::new()
        .with_file(definition_file("alpha.toml"), agent("alpha"))
        .with_file(definition_file("alpha-again.toml"), agent("alpha"))
        .with_file(definition_file("codex.toml"), agent("codex"))
        .with_file(definition_file(r"skill-managers\acme.toml"), ACME_MANAGER)
        .with_file(
            definition_file(r"skill-managers\acme-again.toml"),
            acme_again,
        )
        .with_file(
            definition_file(r"skill-managers\skillfish.toml"),
            ACME_MANAGER.replace("\"acme\"", "\"skillfish\""),
        );
    let ports = fake::ports(fs, FakeEnvironment::with_home(HOME));

    let loaded = Core::with_definitions_folder(ports, Path::new(DEFINITIONS)).unwrap();

    let rejected: Vec<(PathBuf, &str)> = loaded
        .rejected
        .iter()
        .map(|rejected| (rejected.file.clone(), rejected.message.as_str()))
        .collect();
    assert_eq!(
        rejected
            .iter()
            .map(|(file, _)| file.clone())
            .collect::<Vec<_>>(),
        [
            definition_file("alpha.toml"),
            definition_file("codex.toml"),
            definition_file(r"skill-managers\acme.toml"),
            definition_file(r"skill-managers\skillfish.toml"),
        ],
        "{rejected:#?}"
    );
    assert!(
        rejected
            .iter()
            .all(|(_, message)| message.contains("is taken")),
        "{rejected:#?}"
    );
    let agents = agent_ids(&loaded.core);
    assert_eq!(agents.iter().filter(|(id, _)| id == "alpha").count(), 1);
    assert_eq!(agents.iter().filter(|(id, _)| id == "codex").count(), 1);
}
