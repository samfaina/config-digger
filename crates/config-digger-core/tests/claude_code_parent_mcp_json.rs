//! Claude Code's `.mcp.json` in the Project folder and in every folder above
//! it, through the core `api`, with fake ports: the folders
//! `E:\outer\repo\sub\deep`, `repo` holding `.git`, and the Project at
//! `deep`, as the release gate's probe builds them.

use config_digger_core::api::{
    Action, Context, Core, Entry, Inventory, Plan, ScanOptions, Targets,
};
use config_digger_core::ports::fake::{self, FakeEnvironment, FakeFileSystem};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const HOME: &str = r"C:\Users\fixture";
const OUTER: &str = r"E:\outer";
const REPO: &str = r"E:\outer\repo";
const SUB: &str = r"E:\outer\repo\sub";
const DEEP: &str = r"E:\outer\repo\sub\deep";

fn mcp_json(folder: &str) -> PathBuf {
    Path::new(folder).join(".mcp.json")
}

/// A `.mcp.json` declaring `cd-<tag>` and `cd-same`, each running `<tag>`.
fn servers(tag: &str) -> String {
    json!({ "mcpServers": {
        format!("cd-{tag}"): { "command": tag },
        "cd-same": { "command": tag },
    } })
    .to_string()
}

fn claude_json() -> PathBuf {
    Path::new(HOME).join(".claude.json")
}

/// A `~/.claude.json` in which the user trusts the Project `deep`, so that
/// its own approvals count.
const TRUSTED: &str =
    r#"{ "projects": { "E:/outer/repo/sub/deep": { "hasTrustDialogAccepted": true } } }"#;

/// `.mcp.json` in `outer`, `repo` and `sub`, with no approval yet.
fn unapproved() -> FakeFileSystem {
    FakeFileSystem::new()
        .with_file(claude_json(), "{}")
        .with_folder(DEEP)
        .with_file(mcp_json(OUTER), servers("outer"))
        .with_file(mcp_json(REPO), servers("repo"))
        .with_file(mcp_json(SUB), servers("sub"))
}

/// `fs` with every `.mcp.json` server approved in the settings of the
/// Project `project`, which the user trusts, so that they count.
fn approved(fs: FakeFileSystem, project: &str) -> FakeFileSystem {
    let mut state: Value = serde_json::from_slice(&fs.contents(claude_json()).unwrap()).unwrap();
    state["projects"][project.replace('\\', "/")] = json!({ "hasTrustDialogAccepted": true });
    fs.with_file(claude_json(), state.to_string()).with_file(
        Path::new(project).join(r".claude\settings.json"),
        r#"{ "enableAllProjectMcpServers": true }"#,
    )
}

/// [`unapproved`] with `repo` a git repo, every server [`approved`] in
/// `deep`.
fn fixture() -> FakeFileSystem {
    approved(unapproved().with_folder(Path::new(REPO).join(".git")), DEEP)
}

fn core(fs: &FakeFileSystem) -> Core {
    Core::new(fake::ports(fs.clone(), FakeEnvironment::with_home(HOME))).unwrap()
}

/// The inventory in the Project `project`, the Projects `scanned` scanned.
fn inventory_in(fs: &FakeFileSystem, scanned: &[&str], project: &str) -> Inventory {
    let core = core(fs);
    let snapshot = core
        .scan(&ScanOptions {
            projects: scanned.iter().map(PathBuf::from).collect(),
        })
        .unwrap();
    core.resolve(
        &snapshot,
        &Context {
            project: Some(PathBuf::from(project)),
        },
    )
}

fn inventory(fs: &FakeFileSystem) -> Inventory {
    inventory_in(fs, &[DEEP], DEEP)
}

fn entries(inventory: &Inventory, name: &str) -> Vec<Entry> {
    inventory
        .items
        .iter()
        .find(|item| item.name == name)
        .unwrap_or_else(|| panic!("no Item `{name}`"))
        .entries
        .clone()
}

/// A serialised value as its JSON string, such as a State's `"shadowed"`.
fn code(value: impl serde::Serialize) -> String {
    match serde_json::to_value(value).unwrap() {
        Value::String(code) => code,
        other => panic!("not a code: {other}"),
    }
}

/// `(locator, scope, state)` per Entry of the Item `name`.
fn rows(inventory: &Inventory, name: &str) -> Vec<(String, String, String)> {
    entries(inventory, name)
        .iter()
        .map(|entry| {
            (
                entry.locator.clone(),
                entry.scope.clone(),
                code(entry.state),
            )
        })
        .collect()
}

fn row(folder: &str, name: &str, state: &str) -> (String, String, String) {
    (
        format!("{}#mcpServers.{name}", mcp_json(folder).display()),
        "project".to_owned(),
        state.to_owned(),
    )
}

/// The probe's facts: every parent's servers are the Project's, and of the
/// server they all declare, the nearest folder's wins.
fn assert_read_as_the_probe_found(inventory: &Inventory) {
    assert_eq!(
        rows(inventory, "cd-outer"),
        [row(OUTER, "cd-outer", "effective")]
    );
    assert_eq!(
        rows(inventory, "cd-repo"),
        [row(REPO, "cd-repo", "effective")]
    );
    assert_eq!(rows(inventory, "cd-sub"), [row(SUB, "cd-sub", "effective")]);
    assert_eq!(
        rows(inventory, "cd-same"),
        [
            row(SUB, "cd-same", "effective"),
            row(REPO, "cd-same", "shadowed"),
            row(OUTER, "cd-same", "shadowed"),
        ]
    );
}

#[test]
fn every_parent_s_mcp_json_is_read_past_the_repo_root_and_the_nearest_wins() {
    let fs = fixture();

    assert_read_as_the_probe_found(&inventory(&fs));
}

#[test]
fn the_walk_does_not_need_a_git_repo() {
    let fs = approved(unapproved(), DEEP);

    assert_read_as_the_probe_found(&inventory(&fs));
}

fn settings_local() -> PathBuf {
    Path::new(DEEP).join(r".claude\settings.local.json")
}

/// The State and the Reason's code of the Entry of `cd-outer`.
fn outer_state(fs: &FakeFileSystem) -> (String, Option<String>) {
    let entry = &entries(&inventory(fs), "cd-outer")[0];
    (
        code(entry.state),
        entry.reason.as_ref().map(|reason| code(reason.code)),
    )
}

#[test]
fn a_parent_s_server_waits_for_the_project_s_own_approval() {
    let fs = unapproved();
    assert_eq!(
        outer_state(&fs),
        ("blocked".to_owned(), Some("not_approved".to_owned()))
    );

    // Approved in the Project's state in `~/.claude.json`.
    fs.put_file(
        claude_json(),
        r#"{ "projects": { "E:/outer/repo/sub/deep": { "hasTrustDialogAccepted": true, "enabledMcpjsonServers": ["cd-outer"] } } }"#,
    );
    assert_eq!(outer_state(&fs), ("effective".to_owned(), None));

    // Approved in the Project's local settings.
    fs.put_file(claude_json(), TRUSTED);
    fs.put_file(
        settings_local(),
        r#"{ "enabledMcpjsonServers": ["cd-outer"] }"#,
    );
    assert_eq!(outer_state(&fs), ("effective".to_owned(), None));

    // A parent folder's own settings approve nothing in the Project.
    fs.put_file(settings_local(), "{}");
    fs.put_file(
        Path::new(OUTER).join(r".claude\settings.json"),
        r#"{ "enableAllProjectMcpServers": true }"#,
    );
    assert_eq!(
        outer_state(&fs),
        ("blocked".to_owned(), Some("not_approved".to_owned()))
    );
}

#[test]
fn disabled_mcpjson_servers_in_the_project_s_settings_disables_a_parent_s_server() {
    let fs = fixture().with_file(
        settings_local(),
        r#"{ "disabledMcpjsonServers": ["cd-outer"] }"#,
    );

    let entry = &entries(&inventory(&fs), "cd-outer")[0];

    assert_eq!(code(entry.state), "disabled");
    let reason = entry.reason.as_ref().unwrap();
    let switch = format!("{}#disabledMcpjsonServers", settings_local().display());
    assert_eq!(reason.switch.as_deref(), Some(switch.as_str()));
}

#[test]
fn disable_of_a_parent_s_server_writes_the_project_s_local_settings() {
    // Approved in the user's settings: Claude Code has not recorded the
    // Project, so the `/mcp` toggle in its state is skipped.
    let fs = unapproved()
        .with_folder(Path::new(REPO).join(".git"))
        .with_file(
            Path::new(HOME).join(r".claude\settings.json"),
            r#"{ "enableAllProjectMcpServers": true }"#,
        )
        .with_file(mcp_json(DEEP), servers("deep"));
    let core = core(&fs);
    let disable = |folder: &str, name: &str| {
        let snapshot = core
            .scan(&ScanOptions {
                projects: vec![PathBuf::from(DEEP)],
            })
            .unwrap();
        let targets = Targets {
            selectors: vec![format!("{}#mcpServers.{name}", mcp_json(folder).display())],
            all: false,
        };
        let context = Context {
            project: Some(PathBuf::from(DEEP)),
        };
        core.plan(&snapshot, &context, Action::Disable, &targets)
            .unwrap()
    };
    let files = |plan: &Plan| -> Vec<PathBuf> {
        plan.preview()
            .files
            .iter()
            .map(|file| file.path.clone())
            .collect()
    };

    // The file Disable writes for a server of the Project's own `.mcp.json`.
    assert_eq!(files(&disable(DEEP, "cd-deep")), [settings_local()]);
    let plan = disable(OUTER, "cd-outer");
    assert_eq!(files(&plan), [settings_local()]);
    core.execute(plan, &[]).unwrap();

    let written: Value = serde_json::from_slice(&fs.contents(settings_local()).unwrap()).unwrap();
    assert_eq!(written, json!({ "disabledMcpjsonServers": ["cd-outer"] }));
    assert_eq!(
        code(entries(&inventory(&fs), "cd-outer")[0].state),
        "disabled"
    );
}

#[test]
fn a_parent_s_mcp_json_that_does_not_parse_is_skipped_alone() {
    let fs = fixture()
        .with_file(mcp_json(OUTER), r#"{ "mcpServers": { broken"#)
        .with_file(mcp_json(DEEP), servers("deep"));

    let inventory = inventory(&fs);

    let failures: Vec<_> = inventory
        .read_failures
        .iter()
        .map(|failure| (failure.path.clone(), code(failure.reaction)))
        .collect();
    assert_eq!(failures, [(mcp_json(OUTER), "ignores_file".to_owned())]);
    assert_eq!(
        rows(&inventory, "cd-deep"),
        [row(DEEP, "cd-deep", "effective")]
    );
    assert_eq!(
        rows(&inventory, "cd-same"),
        [
            row(DEEP, "cd-same", "effective"),
            row(SUB, "cd-same", "shadowed"),
            row(REPO, "cd-same", "shadowed"),
        ]
    );
}

#[test]
fn a_project_inside_another_reads_the_parent_s_servers_as_its_own_too() {
    let fs = approved(fixture(), REPO);

    for project in [REPO, DEEP] {
        let inventory = inventory_in(&fs, &[REPO, DEEP], project);
        assert_eq!(
            rows(&inventory, "cd-outer"),
            [row(OUTER, "cd-outer", "effective")],
            "{project}"
        );
    }
    let in_repo = inventory_in(&fs, &[REPO, DEEP], REPO);
    assert!(in_repo.items.iter().all(|item| item.name != "cd-sub"));
    assert_eq!(
        rows(&in_repo, "cd-same"),
        [
            row(REPO, "cd-same", "effective"),
            row(OUTER, "cd-same", "shadowed"),
        ]
    );
}

#[test]
fn the_mcp_json_at_the_root_of_the_drive_is_not_read() {
    let fs = fixture().with_file(mcp_json(r"E:\"), servers("root"));

    let inventory = inventory(&fs);

    assert!(inventory.items.iter().all(|item| item.name != "cd-root"));
    assert_read_as_the_probe_found(&inventory);
}
