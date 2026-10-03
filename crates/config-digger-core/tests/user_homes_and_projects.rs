//! Agent homes and Projects the user adds, hides and forgets, through the
//! core `api` with fake ports.

use config_digger_core::api::{Context, Core, ErrorCode, Inventory, ScanOptions};
use config_digger_core::ports::fake::{self, FakeEnvironment, FakeFileSystem};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const HOME: &str = r"C:\Users\fixture";
const APPDATA: &str = r"C:\Users\fixture\AppData\Roaming";
const ALT: &str = r"D:\alt-claude";

fn in_home(relative: &str) -> PathBuf {
    PathBuf::from(HOME).join(relative)
}

fn core(fs: &FakeFileSystem) -> Core {
    let env = FakeEnvironment::with_home(HOME).with_var("APPDATA", APPDATA);
    Core::new(fake::ports(fs.clone(), env)).unwrap()
}

/// What the core lists with no Project, hidden homes and Projects included or not.
fn inventory(core: &Core, include_hidden: bool) -> Inventory {
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    let inventory = core.resolve(&snapshot, &Context::default());
    if include_hidden {
        inventory
    } else {
        inventory.without_hidden()
    }
}

fn json(value: &impl serde::Serialize) -> Value {
    serde_json::to_value(value).unwrap()
}

/// `(agent, home, source)` of each home listed.
fn homes(inventory: &Inventory) -> Vec<(String, String, Value)> {
    inventory
        .homes
        .iter()
        .map(|home| (home.agent.clone(), home.home.clone(), json(&home.source)))
        .collect()
}

/// `(name, home)` of each Item listed.
fn items(inventory: &Inventory) -> Vec<(String, String)> {
    inventory
        .items
        .iter()
        .map(|item| (item.name.clone(), item.home.clone()))
        .collect()
}

fn claude_code_home() -> FakeFileSystem {
    FakeFileSystem::new().with_file(
        in_home(".claude.json"),
        r#"{"mcpServers": {"linear": {"url": "https://user"}}}"#,
    )
}

#[test]
fn an_added_home_is_read_like_a_discovered_one() {
    let fs = claude_code_home().with_file(
        Path::new(ALT).join(".claude.json"),
        r#"{"mcpServers": {"notes": {"command": "notes"}}}"#,
    );
    let core = core(&fs);

    let home = core.add_home(Path::new(ALT), "claude-code").unwrap();

    assert_eq!(home.home, ALT);
    assert_eq!(home.folder, PathBuf::from(ALT));
    let inventory = inventory(&core, false);
    assert_eq!(
        homes(&inventory),
        vec![
            (
                "claude-code".into(),
                "default".into(),
                json!({"kind": "default"})
            ),
            ("claude-code".into(), ALT.into(), json!({"kind": "added"})),
        ]
    );
    assert_eq!(
        items(&inventory),
        vec![
            ("notes".into(), ALT.into()),
            ("linear".into(), "default".into()),
        ]
    );
}

#[test]
fn an_added_home_without_the_expected_files_says_so() {
    let fs = claude_code_home().with_folder(ALT);
    let core = core(&fs);

    let home = core.add_home(Path::new(ALT), "claude-code").unwrap();

    assert_eq!(
        home.no_configuration.as_deref(),
        Some("No configuration found for Claude Code")
    );
    let inventory = inventory(&core, false);
    let discovered = &inventory.homes[0];
    assert_eq!(discovered.home, "default");
    assert_eq!(discovered.no_configuration, None);
}

#[test]
fn a_home_is_added_only_at_an_existing_folder_for_a_known_agent() {
    let fs = claude_code_home().with_file(Path::new(ALT).join("notes.txt"), "x");
    let core = core(&fs);

    let missing = core
        .add_home(Path::new(r"D:\nowhere"), "claude-code")
        .unwrap_err();
    let file = core
        .add_home(&Path::new(ALT).join("notes.txt"), "claude-code")
        .unwrap_err();
    let unknown = core.add_home(Path::new(ALT), "zed").unwrap_err();

    assert_eq!(missing.code, ErrorCode::InvalidPath);
    assert_eq!(missing.message, r"D:\nowhere is not an existing folder");
    assert_eq!(file.code, ErrorCode::InvalidPath);
    assert_eq!(unknown.code, ErrorCode::NoMatch);
    assert_eq!(homes(&inventory(&core, true)).len(), 1);
}

const PETME: &str = r"E:\projects\petme-app";
const SCRATCH: &str = r"E:\scratch";
const WORKSPACE: &str = r"E:\work\team.code-workspace";

/// Claude Code's home, whose registry records `PETME`, plus `SCRATCH` and
/// `WORKSPACE`, which no registry records.
fn with_projects() -> FakeFileSystem {
    FakeFileSystem::new()
        .with_file(
            in_home(".claude.json"),
            r#"{"projects": {"E:/projects/petme-app": {}}}"#,
        )
        .with_folder(PETME)
        .with_folder(SCRATCH)
        .with_file(WORKSPACE, r#"{"folders": []}"#)
}

/// `(path, registered_by, added, workspace)` of each Project listed.
fn projects(inventory: &Inventory) -> Vec<(String, Vec<String>, bool, bool)> {
    inventory
        .projects
        .iter()
        .map(|project| {
            (
                project.path.display().to_string(),
                project.registered_by.clone(),
                project.added,
                project.workspace_file,
            )
        })
        .collect()
}

#[test]
fn a_project_is_added_as_a_folder_or_a_workspace_file() {
    let fs = with_projects();
    let core = core(&fs);

    let scratch = core.add_project(Path::new("e:/scratch/")).unwrap();
    core.add_project(Path::new(WORKSPACE)).unwrap();
    core.add_project(Path::new(PETME)).unwrap();

    assert_eq!(scratch.path, PathBuf::from(SCRATCH));
    assert!(scratch.added);
    let claude_code = vec!["claude-code".to_owned()];
    assert_eq!(
        projects(&inventory(&core, false)),
        vec![
            (PETME.into(), claude_code, true, false),
            (SCRATCH.into(), vec![], true, false),
            (WORKSPACE.into(), vec![], true, true),
        ]
    );
}

#[test]
fn a_project_is_added_only_at_an_existing_folder_or_workspace_file() {
    let fs = with_projects().with_file(Path::new(SCRATCH).join("notes.txt"), "x");
    let core = core(&fs);

    let missing = core.add_project(Path::new(r"E:\gone")).unwrap_err();
    let file = core
        .add_project(&Path::new(SCRATCH).join("notes.txt"))
        .unwrap_err();

    assert_eq!(missing.code, ErrorCode::InvalidPath);
    assert_eq!(
        file.message,
        r"E:\scratch\notes.txt is not an existing folder or .code-workspace file"
    );
    assert_eq!(projects(&inventory(&core, true)).len(), 1);
}

#[test]
fn hidden_homes_and_their_items_are_listed_only_when_asked_for() {
    let fs = claude_code_home().with_file(
        Path::new(ALT).join(".claude.json"),
        r#"{"mcpServers": {"notes": {"command": "notes"}}}"#,
    );
    let core = core(&fs);
    core.add_home(Path::new(ALT), "claude-code").unwrap();

    let default = core.hide_home("default", Some("claude-code")).unwrap();
    let added = core.hide_home("d:/ALT-claude/", None).unwrap();

    assert!(default.hidden && added.hidden);
    let visible = inventory(&core, false);
    assert_eq!(homes(&visible), vec![]);
    assert_eq!(items(&visible), vec![]);
    let all = inventory(&core, true);
    assert_eq!(items(&all).len(), 2);
    assert!(all.homes.iter().all(|home| home.hidden));

    core.unhide_home("default", None).unwrap();
    assert_eq!(
        items(&inventory(&core, false)),
        vec![("linear".into(), "default".into())]
    );
}

#[test]
fn hidden_projects_are_listed_only_when_asked_for_and_still_evaluate_when_picked() {
    let fs = with_projects().with_file(
        in_home(".claude.json"),
        r#"{"projects": {"E:/projects/petme-app": {"mcpServers": {"local": {"command": "l"}}}}}"#,
    );
    let core = core(&fs);
    core.add_project(Path::new(SCRATCH)).unwrap();

    let petme = core
        .hide_project(Path::new("e:/projects/petme-app"))
        .unwrap();
    core.hide_project(Path::new(SCRATCH)).unwrap();

    assert!(petme.hidden);
    assert_eq!(projects(&inventory(&core, false)), vec![]);
    assert_eq!(projects(&inventory(&core, true)).len(), 2);
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    let picked = core
        .resolve(
            &snapshot,
            &Context {
                project: Some(PathBuf::from(PETME)),
            },
        )
        .without_hidden();
    assert_eq!(picked.project, Some(PathBuf::from(PETME)));
    assert_eq!(items(&picked), vec![("local".into(), "default".into())]);

    core.unhide_project(Path::new(PETME)).unwrap();
    assert_eq!(projects(&inventory(&core, false)).len(), 1);
}

#[test]
fn only_what_is_listed_is_hidden_and_only_what_is_hidden_is_unhidden() {
    let fs = with_projects();
    let core = core(&fs);

    let home = core.hide_home("other", None).unwrap_err();
    let project = core.hide_project(Path::new(SCRATCH)).unwrap_err();
    let not_hidden = core.unhide_project(Path::new(PETME)).unwrap_err();

    assert_eq!(home.code, ErrorCode::NoMatch);
    assert_eq!(project.code, ErrorCode::NoMatch);
    assert_eq!(not_hidden.code, ErrorCode::NoMatch);
}

#[test]
fn forgetting_removes_what_the_user_added() {
    let fs = with_projects().with_folder(ALT);
    let core = core(&fs);
    core.add_home(Path::new(ALT), "claude-code").unwrap();
    core.add_project(Path::new(SCRATCH)).unwrap();
    core.hide_project(Path::new(SCRATCH)).unwrap();

    let home = core.forget_home(ALT, None).unwrap();
    let project = core.forget_project(Path::new(SCRATCH)).unwrap();

    assert_eq!(home.home, ALT);
    assert_eq!(project.path, PathBuf::from(SCRATCH));
    let all = inventory(&core, true);
    assert_eq!(homes(&all).len(), 1);
    assert_eq!(projects(&all).len(), 1);
    // Added again, it is not hidden any more.
    assert!(!core.add_project(Path::new(SCRATCH)).unwrap().hidden);
}

#[test]
fn forgetting_what_is_discovered_is_refused_in_favour_of_hiding() {
    let fs = with_projects();
    let core = core(&fs);
    // Added as well, a Project a registry records stays on the list.
    core.add_project(Path::new(PETME)).unwrap();

    let home = core.forget_home("default", None).unwrap_err();
    let project = core.forget_project(Path::new(PETME)).unwrap_err();

    assert_eq!(home.code, ErrorCode::Blocked);
    assert_eq!(
        home.message,
        "Agent home default of Claude Code is found without you, so forgetting it would not take it off the list; hide it with `config-digger homes hide`"
    );
    assert_eq!(project.code, ErrorCode::Blocked);
    assert_eq!(
        project.message,
        r"Project E:\projects\petme-app is recorded by Claude Code, so forgetting it would not take it off the list; hide it with `config-digger projects hide`"
    );
    assert_eq!(projects(&inventory(&core, false)).len(), 1);
}

#[test]
fn additions_and_hides_live_in_the_roaming_data_folder_and_need_no_lock() {
    let fs = with_projects().with_folder(ALT);
    let claude_json = fs.contents(in_home(".claude.json"));
    // An Operation holds the global write lock.
    fs.lock_elsewhere(in_home(r".local\share\config-digger\lock"));
    let core = core(&fs);

    core.add_home(Path::new(ALT), "claude-code").unwrap();
    core.add_project(Path::new(SCRATCH)).unwrap();
    core.hide_project(Path::new(PETME)).unwrap();
    core.hide_home("default", None).unwrap();

    let list = Path::new(APPDATA).join(r"config-digger\homes-and-projects.json");
    assert_eq!(fs.files_below(APPDATA), vec![list.clone()]);
    let written: Value = serde_json::from_slice(&fs.contents(&list).unwrap()).unwrap();
    assert_eq!(
        written,
        json!({
            "version": 1,
            "homes": [{"agent": "claude-code", "folder": ALT}],
            "hidden_homes": [{"agent": "claude-code", "home": "default"}],
            "projects": [SCRATCH],
            "hidden_projects": [PETME],
        })
    );
    // Nothing writes Agent configuration.
    assert_eq!(fs.contents(in_home(".claude.json")), claude_json);
    assert_eq!(fs.files_below(ALT), Vec::<PathBuf>::new());
}

#[test]
fn projects_come_from_registries_and_the_user_never_from_scanning_the_disk() {
    let fs = with_projects().with_file(
        in_home(r"code\unrecorded\.mcp.json"),
        r#"{"mcpServers": {"x": {"command": "x"}}}"#,
    );
    let core = core(&fs);

    let listed = projects(&inventory(&core, true));

    assert_eq!(
        listed,
        vec![(PETME.into(), vec!["claude-code".to_owned()], false, false)]
    );
}

/// An Agent definition loaded besides the built-in ones, whose home is not
/// on this machine.
const OTHER_AGENT: &str = r#"
format_version = 1

[agent]
id = "other-agent"
name = "Other Agent"

[[homes]]
id = "default"
folder = { block = "path", path = "~/.other" }
roots.config = "config.json"

[[scopes]]
id = "user"

[[locations]]
id = "user-mcp"
anchor = { block = "home", root = "config" }
format = { block = "json", dialect = "strict" }
root = ["servers"]
kind = "mcp"
scope = "user"
provenance = "file"
store_class = "plain"
on_read_failure = "ignores_file"

[items.mcp]
precedence = ["user-mcp"]
competition = { block = "exact-name" }
checks = ["disable", "precedence"]
"#;

#[test]
fn agents_lists_every_loaded_definition_with_the_homes_found() {
    let fs = claude_code_home().with_folder(ALT);
    let env = FakeEnvironment::with_home(HOME).with_var("APPDATA", APPDATA);
    let core = Core::with_extra_definitions(
        fake::ports(fs.clone(), env),
        &[("other-agent.toml", OTHER_AGENT)],
    )
    .unwrap();
    core.add_home(Path::new(ALT), "claude-code").unwrap();
    core.hide_home(ALT, None).unwrap();

    let agents = core.agents(&inventory(&core, false));
    let with_hidden = core.agents(&inventory(&core, true));

    let summary = |agents: &[config_digger_core::api::Agent]| -> Vec<(String, bool, Vec<String>)> {
        agents
            .iter()
            .map(|agent| {
                let homes = agent.homes.iter().map(|home| home.home.clone()).collect();
                (agent.name.clone(), agent.built_in, homes)
            })
            .collect()
    };
    assert_eq!(
        summary(&agents),
        vec![
            ("Claude Code".into(), true, vec!["default".into()]),
            ("Other Agent".into(), false, vec![]),
        ]
    );
    assert_eq!(
        summary(&with_hidden)[0].2,
        vec!["default".to_owned(), ALT.to_owned()]
    );
}

#[test]
fn a_definition_home_the_user_adds_keeps_its_id_before_and_after_its_files_appear() {
    let fs = FakeFileSystem::new().with_folder(HOME);
    let core = core(&fs);

    let added = core.add_home(Path::new(HOME), "claude-code").unwrap();
    core.hide_home("default", None).unwrap();
    fs.put_file(in_home(".claude.json"), "{}");
    let found = &inventory(&core, true).homes;

    assert_eq!(added.home, "default");
    assert_eq!(json(&added.source), json!({"kind": "added"}));
    assert_eq!(
        added.no_configuration.as_deref(),
        Some("No configuration found for Claude Code")
    );
    assert_eq!(found.len(), 1);
    assert_eq!(found[0].home, "default");
    assert_eq!(json(&found[0].source), json!({"kind": "default"}));
    assert!(found[0].hidden);
}

#[test]
fn a_list_that_cannot_be_read_stops_the_scan_rather_than_show_hidden_homes() {
    let fs = claude_code_home().with_file(
        Path::new(APPDATA).join(r"config-digger\homes-and-projects.json"),
        "{ not json",
    );
    let core = core(&fs);

    let error = core.scan(&ScanOptions::default()).err().unwrap();

    assert_eq!(error.code, ErrorCode::Unexpected);
    assert!(
        error.message.contains("fix or delete it"),
        "{}",
        error.message
    );
}

#[test]
fn a_folder_named_like_a_workspace_file_is_a_folder_project() {
    let fs = with_projects().with_folder(r"E:\odd.code-workspace");
    let core = core(&fs);

    let project = core
        .add_project(Path::new(r"E:\odd.code-workspace"))
        .unwrap();

    assert!(!project.workspace_file);
}
