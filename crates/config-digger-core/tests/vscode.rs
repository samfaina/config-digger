//! VS Code through the core `api`, with fake ports over fixture VS Code
//! folders and a fixture SQLite store: the collections VS Code reads MCP
//! servers from and how same-named servers compete, its profiles, the
//! `.code-workspace` Projects it opens, the extension servers it caches in
//! `state.vscdb`, the skills it reads, and the files it shares with Claude
//! Code.

use config_digger_core::api::{
    Action, Context, Core, Entry, Home, Inventory, Item, Project, ScanOptions, Targets,
};
use config_digger_core::ports::fake::{self, FakeEnvironment, FakeFileSystem};
use serde_json::Value;
use std::path::{Path, PathBuf};

const HOME: &str = r"C:\Users\fixture";
const APP_DATA: &str = r"C:\Users\fixture\AppData\Roaming";
const LOCAL_APP_DATA: &str = r"C:\Users\fixture\AppData\Local";

/// The default profile's folder, where VS Code keeps the user's own files.
const USER: &str = r"C:\Users\fixture\AppData\Roaming\Code\User";

/// The fixture folder `name`.
fn fixture_folder(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(r"tests\fixtures\vscode")
        .join(name)
}

/// Every file below `folder` on disk, at the same place below `at`. A
/// `<store>.items.json` is the fixture of a SQLite store: an object of each
/// key of its `ItemTable` and the JSON value it holds there, which the store
/// at `<store>` holds as text.
fn load(fs: FakeFileSystem, folder: &Path, at: &Path) -> FakeFileSystem {
    let mut fs = fs.with_folder(at);
    for child in std::fs::read_dir(folder).unwrap() {
        let child = child.unwrap().path();
        let name = child.file_name().unwrap();
        let store = name.to_str().unwrap().strip_suffix(".items.json");
        fs = if child.is_dir() {
            load(fs, &child, &at.join(name))
        } else if let Some(store) = store {
            let items: serde_json::Map<String, Value> =
                serde_json::from_slice(&std::fs::read(&child).unwrap()).unwrap();
            items.into_iter().fold(fs, |fs, (key, value)| {
                fs.with_sqlite_item(at.join(store), &key, &value.to_string())
            })
        } else {
            fs.with_file(at.join(name), std::fs::read(&child).unwrap())
        };
    }
    fs
}

/// The fixture default profile at `%APPDATA%\Code\User`.
fn vscode() -> FakeFileSystem {
    load(
        FakeFileSystem::new(),
        &fixture_folder("user"),
        Path::new(USER),
    )
}

fn env() -> FakeEnvironment {
    FakeEnvironment::with_home(HOME)
        .with_var("APPDATA", APP_DATA)
        .with_var("LOCALAPPDATA", LOCAL_APP_DATA)
        .with_var("USERPROFILE", HOME)
}

fn core(fs: &FakeFileSystem) -> Core {
    Core::new(fake::ports(fs.clone(), env())).unwrap()
}

fn inventory_in(fs: &FakeFileSystem, context: &Context) -> Inventory {
    let core = core(fs);
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    core.resolve(&snapshot, context)
}

fn inventory(fs: &FakeFileSystem) -> Inventory {
    inventory_in(fs, &Context::default())
}

/// The VS Code Item named `name` in the home `home`.
fn item_in<'i>(inventory: &'i Inventory, home: &str, name: &str) -> &'i Item {
    inventory
        .items
        .iter()
        .find(|item| item.agent == "vscode" && item.home == home && item.name == name)
        .unwrap_or_else(|| panic!("no VS Code Item `{name}` in home `{home}`"))
}

/// The VS Code Item named `name` in the default profile.
fn item<'i>(inventory: &'i Inventory, name: &str) -> &'i Item {
    item_in(inventory, "default", name)
}

/// The VS Code home `home`.
fn home<'i>(inventory: &'i Inventory, home: &str) -> &'i Home {
    inventory
        .homes
        .iter()
        .find(|found| found.agent == "vscode" && found.home == home)
        .unwrap_or_else(|| panic!("no VS Code home `{home}`"))
}

fn json(value: impl serde::Serialize) -> Value {
    serde_json::to_value(value).unwrap()
}

fn entry_json(entry: &Entry) -> Value {
    json(entry)
}

/// Where the fixture Projects go.
const PROJECTS: &str = r"E:\projects";

/// The fixture Project folder `name`.
fn project(name: &str) -> PathBuf {
    Path::new(PROJECTS).join(name)
}

/// `fs` with the fixture Project `name` at [`project`].
fn with_project(fs: FakeFileSystem, name: &str) -> FakeFileSystem {
    load(
        fs,
        &fixture_folder(&format!(r"projects\{name}")),
        &project(name),
    )
}

/// The inventory in the context of the Project at `path`, which the scan is
/// asked for whether or not a registry records it.
fn inventory_at(fs: &FakeFileSystem, path: &Path) -> Inventory {
    let core = core(fs);
    let options = ScanOptions {
        projects: vec![path.to_path_buf()],
    };
    let snapshot = core.scan(&options).unwrap();
    let context = Context {
        project: Some(path.to_path_buf()),
    };
    core.resolve(&snapshot, &context)
}

/// Each Entry of `item` as its State and locator, in precedence order.
fn states(item: &Item) -> Vec<(String, String)> {
    item.entries
        .iter()
        .map(|entry| (code(entry.state), entry.locator.clone()))
        .collect()
}

fn code(value: impl serde::Serialize) -> String {
    json(value).as_str().unwrap().to_owned()
}

#[test]
fn the_servers_of_the_user_mcp_json_are_listed_under_its_servers_key() {
    let inventory = inventory(&vscode());

    let fetch = item(&inventory, "fetch");
    assert_eq!(json(fetch.kind), "mcp");
    assert_eq!(json(fetch.state), "effective");
    let entry = entry_json(&fetch.entries[0]);
    assert_eq!(entry["path"], format!(r"{USER}\mcp.json"));
    assert_eq!(entry["key_path"], serde_json::json!(["servers", "fetch"]));
    assert_eq!(entry["scope"], "user");
    assert_eq!(entry["provenance"], "file");
    assert_eq!(json(item(&inventory, "github").state), "effective");
    assert_eq!(home(&inventory, "default").folder, PathBuf::from(USER));
}

#[test]
fn a_folder_s_vscode_mcp_json_and_mcp_json_are_collections_of_the_project() {
    let fs = with_project(vscode(), "app");
    let inventory = inventory_at(&fs, &project("app"));

    let supabase = item(&inventory, "supabase");
    assert_eq!(json(supabase.state), "effective");
    let entry = entry_json(&supabase.entries[0]);
    assert_eq!(
        entry["key_path"],
        serde_json::json!(["mcpServers", "supabase"])
    );
    assert_eq!(entry["scope"], "workspace-folder");
    let github = &item(&inventory, "GitHub").entries[0];
    assert_eq!(
        github.locator,
        format!(r"{PROJECTS}\app\.vscode\mcp.json#servers.GitHub")
    );
    assert_eq!(github.scope, "workspace-folder");
}

#[test]
fn a_mcp_json_may_hold_its_servers_at_the_top_level() {
    let fs = with_project(vscode(), "bare");
    let inventory = inventory_at(&fs, &project("bare"));

    let webstorm = item(&inventory, "webstorm");
    assert_eq!(json(webstorm.state), "effective");
    assert_eq!(
        webstorm.entries[0].locator,
        format!(r"{PROJECTS}\bare\.mcp.json#webstorm")
    );
}

#[test]
fn same_named_servers_compete_without_regard_to_case_by_collection_order() {
    let fs = with_project(vscode(), "app");
    let inventory = inventory_at(&fs, &project("app"));

    let github = item(&inventory, "GitHub");
    assert_eq!(
        states(github)[..2],
        [
            (
                "effective".to_owned(),
                format!(r"{PROJECTS}\app\.vscode\mcp.json#servers.GitHub")
            ),
            (
                "shadowed".to_owned(),
                format!(r"{USER}\mcp.json#servers.github")
            ),
        ]
    );
    let reason = github.entries[1].reason.as_ref().unwrap();
    assert_eq!(code(reason.code), "shadowed_by_precedence");
}

#[test]
fn with_collision_behavior_suffix_same_named_servers_are_separate_items() {
    let fs = with_project(vscode(), "app").with_file(
        format!(r"{USER}\settings.json"),
        "{\n\t// Number the tools of same-named servers.\n\t\"chat.mcp.collisionBehavior\": \"suffix\",\n}\n",
    );
    let inventory = inventory_at(&fs, &project("app"));

    let named: Vec<(&str, String)> = inventory
        .items
        .iter()
        .filter(|item| {
            item.agent == "vscode"
                && item.home == "default"
                && item.name.eq_ignore_ascii_case("github")
        })
        .map(|item| (item.name.as_str(), code(item.state)))
        .collect();
    assert_eq!(
        named,
        [
            ("GitHub", "effective".to_owned()),
            ("github", "effective".to_owned()),
            ("github", "effective".to_owned()),
        ]
    );
}

#[test]
fn a_folder_s_own_settings_set_the_collision_behavior_in_that_project() {
    let fs = with_project(vscode(), "app")
        .with_file(
            format!(r"{USER}\settings.json"),
            r#"{ "chat.mcp.collisionBehavior": "suffix" }"#,
        )
        .with_file(
            project("app").join(r".vscode\settings.json"),
            r#"{ "chat.mcp.collisionBehavior": "disable" }"#,
        );
    let inventory = inventory_at(&fs, &project("app"));

    assert_eq!(item(&inventory, "GitHub").entries.len(), 3);
}

#[test]
fn a_member_folder_s_settings_set_no_collision_behavior_in_a_workspace_file() {
    let fs = with_project(vscode(), "multi").with_file(
        project(r"multi\api\.vscode\settings.json"),
        r#"{ "chat.mcp.collisionBehavior": "suffix" }"#,
    );
    let inventory = inventory_at(&fs, &workspace());

    // The two member folders' `shared` still compete, as with `"disable"`.
    assert_eq!(
        states(item(&inventory, "shared")),
        [
            (
                "effective".to_owned(),
                format!(r"{PROJECTS}\multi\api\.vscode\mcp.json#servers.shared")
            ),
            (
                "shadowed".to_owned(),
                format!(r"{PROJECTS}\multi\web\.vscode\mcp.json#servers.shared")
            ),
        ]
    );
}

/// The folder of the profile at `location` below `User\profiles`.
fn profile(location: &str) -> String {
    format!(r"{USER}\profiles\{location}")
}

#[test]
fn a_profile_that_does_not_share_mcp_is_an_agent_home_of_its_own() {
    let inventory = inventory(&vscode());

    let homes: Vec<&str> = inventory
        .homes
        .iter()
        .filter(|home| home.agent == "vscode")
        .map(|home| home.home.as_str())
        .collect();
    let agents = profile(r"builtin\agents");
    let work = profile("-5a1b2c3d");
    assert_eq!(homes, ["default", agents.as_str(), work.as_str()]);

    let agents_only = item_in(&inventory, &agents, "agents-only");
    assert_eq!(json(agents_only.state), "effective");
    assert_eq!(
        agents_only.entries[0].path,
        PathBuf::from(format!(r"{agents}\mcp.json"))
    );
    assert_eq!(home(&inventory, &agents).folder, PathBuf::from(&agents));
    assert_eq!(
        json(&home(&inventory, &agents).source),
        serde_json::json!({"kind": "known_location"})
    );
    assert_eq!(
        json(&home(&inventory, "default").source),
        serde_json::json!({"kind": "default"})
    );
    assert_eq!(
        json(item_in(&inventory, &work, "work-only").state),
        "effective"
    );
    // Profiles share no Item: the default profile's servers are its own.
    assert!(
        !inventory
            .items
            .iter()
            .any(|item| item.home == work && item.name == "fetch")
    );
}

#[test]
fn a_profile_that_shares_mcp_reads_the_default_profile_s_servers() {
    let inventory = inventory(&vscode());

    assert!(
        !inventory.items.iter().any(|item| item.name == "never-read"),
        "the shared profile's own mcp.json is not read"
    );
}

/// The Project at `path`, as the inventory lists it.
fn listed<'i>(inventory: &'i Inventory, path: &Path) -> Option<&'i Project> {
    inventory
        .projects
        .iter()
        .find(|project| project.path == path)
}

#[test]
fn projects_come_from_workspace_storage_and_the_profiles_workspaces() {
    let fs = with_project(vscode(), "app")
        .with_folder(project("registered"))
        .with_file(project(r"multi\multi.code-workspace"), "{}");
    let inventory = inventory(&fs);

    let app = listed(&inventory, &project("app")).expect("workspace.json names app");
    assert_eq!(app.registered_by, ["vscode"]);
    assert!(!app.workspace_file);
    let registered =
        listed(&inventory, &project("registered")).expect("profileAssociations names the folder");
    assert_eq!(registered.registered_by, ["vscode"]);
    let workspace = listed(&inventory, &project(r"multi\multi.code-workspace"))
        .expect("workspace.json names the workspace file");
    assert!(workspace.workspace_file);
    assert!(
        !inventory
            .projects
            .iter()
            .any(|project| project.path.to_string_lossy().contains("remote")),
        "a remote folder is no Project on this machine"
    );
}

/// Each Agent home VS Code last opened the Project at `path` in, as the
/// inventory lists it.
fn last_opened(inventory: &Inventory, path: &Path) -> Value {
    let project = listed(inventory, path).expect("the Project is listed");
    json(&project.last_opened)
}

#[test]
fn a_workspace_last_opened_in_a_profile_of_its_own_is_last_opened_in_that_profile_s_home() {
    let fs = with_project(vscode(), "app");
    let inventory = inventory(&fs);

    assert_eq!(
        last_opened(&inventory, &project("app")),
        serde_json::json!([{
            "agent": "vscode",
            "home": profile("-5a1b2c3d"),
            "name": "Work",
            "shares_home": false,
            "hidden": false,
            "sentence": "VS Code last opened this Project in Work.",
        }])
    );
}

#[test]
fn a_hidden_last_opened_home_is_still_the_project_s_and_says_it_is_hidden() {
    let fs = with_project(vscode(), "app");
    let core = core(&fs);
    core.hide_home(&profile("-5a1b2c3d"), Some("vscode"))
        .unwrap();
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    let inventory = core
        .resolve(&snapshot, &Context::default())
        .without_hidden();

    let opened = &last_opened(&inventory, &project("app"))[0];
    assert_eq!(opened["home"], profile("-5a1b2c3d"));
    assert_eq!(opened["hidden"], true);
    assert_eq!(
        opened["sentence"],
        "VS Code last opened this Project in Work, which is hidden."
    );
}

#[test]
fn a_workspace_last_opened_in_the_default_profile_is_last_opened_in_default() {
    let fs = vscode().with_folder(project("registered"));
    let inventory = inventory(&fs);

    assert_eq!(
        last_opened(&inventory, &project("registered")),
        serde_json::json!([{
            "agent": "vscode",
            "home": "default",
            "name": null,
            "shares_home": false,
            "hidden": false,
            "sentence": "VS Code last opened this Project in Default.",
        }])
    );
}

#[test]
fn a_workspace_last_opened_in_a_profile_that_shares_mcp_is_last_opened_in_default_under_its_name() {
    let fs = with_project(vscode(), "shared");
    let inventory = inventory(&fs);

    assert_eq!(
        last_opened(&inventory, &project("shared")),
        serde_json::json!([{
            "agent": "vscode",
            "home": "default",
            "name": "Shared",
            "shares_home": true,
            "hidden": false,
            "sentence": "VS Code last opened this Project in Shared, which reads Default's MCP servers.",
        }])
    );
}

#[test]
fn a_workspace_associated_with_no_profile_or_only_in_workspace_storage_has_no_last_opened_home() {
    let fs = with_project(with_project(vscode(), "bare"), "multi");
    let inventory = inventory(&fs);

    assert_eq!(
        last_opened(&inventory, &project("bare")),
        serde_json::json!([])
    );
    assert_eq!(last_opened(&inventory, &workspace()), serde_json::json!([]));
}

#[test]
fn a_last_opened_home_changes_no_state_in_any_home_or_project() {
    let fs = ["app", "shared", "bare", "multi"]
        .into_iter()
        .fold(vscode(), with_project)
        .with_folder(project("registered"));
    // The same workspaces, associated with no profile.
    let storage = Path::new(USER).join(r"globalStorage\storage.json");
    let mut associated: Value = serde_json::from_slice(
        &std::fs::read(fixture_folder(r"user\globalStorage\storage.json")).unwrap(),
    )
    .unwrap();
    for value in associated["profileAssociations"]["workspaces"]
        .as_object_mut()
        .unwrap()
        .values_mut()
    {
        *value = Value::from("no-such-profile");
    }
    let unassociated = fs.clone().with_file(&storage, associated.to_string());
    assert!(
        inventory(&unassociated)
            .projects
            .iter()
            .all(|project| project.last_opened.is_empty())
    );

    let contexts = [
        None,
        Some(project("app")),
        Some(project("shared")),
        Some(project("bare")),
        Some(project("registered")),
        Some(workspace()),
    ];
    for project in contexts {
        let context = Context { project };
        assert_eq!(
            inventory_in(&fs, &context).items,
            inventory_in(&unassociated, &context).items,
            "{context:?}"
        );
    }
}

/// The fixture multi-root workspace file.
fn workspace() -> PathBuf {
    project(r"multi\multi.code-workspace")
}

#[test]
fn a_code_workspace_contributes_its_own_servers_and_each_member_folder_s() {
    let fs = with_project(vscode(), "multi");
    let inventory = inventory_at(&fs, &workspace());

    let docs = &item(&inventory, "docs").entries[0];
    assert_eq!(
        docs.locator,
        format!(r"{PROJECTS}\multi\multi.code-workspace#settings.mcp.servers.docs")
    );
    assert_eq!(docs.scope, "workspace");
    assert_eq!(json(item(&inventory, "api-only").state), "effective");
    assert_eq!(json(item(&inventory, "web-only").state), "effective");
    // The first member folder ranks above the second.
    assert_eq!(
        states(item(&inventory, "shared")),
        [
            (
                "effective".to_owned(),
                format!(r"{PROJECTS}\multi\api\.vscode\mcp.json#servers.shared")
            ),
            (
                "shadowed".to_owned(),
                format!(r"{PROJECTS}\multi\web\.vscode\mcp.json#servers.shared")
            ),
        ]
    );
}

#[test]
fn a_workspace_s_servers_rank_below_its_folders_and_above_the_user_s() {
    let fs = with_project(vscode(), "multi")
        .with_file(
            project(r"multi\web\.vscode\mcp.json"),
            r#"{ "servers": { "Docs": { "command": "web-docs" } } }"#,
        )
        .with_file(
            format!(r"{USER}\mcp.json"),
            r#"{ "servers": { "docs": { "command": "user-docs" } } }"#,
        );
    let inventory = inventory_at(&fs, &workspace());

    let docs = item(&inventory, "Docs");
    let scopes: Vec<(String, &str)> = docs
        .entries
        .iter()
        .map(|entry| (code(entry.state), entry.scope.as_str()))
        .collect();
    assert_eq!(
        scopes,
        [
            ("effective".to_owned(), "workspace-folder"),
            ("shadowed".to_owned(), "workspace"),
            ("shadowed".to_owned(), "user"),
        ]
    );
}

#[test]
fn a_member_folder_alone_is_another_project() {
    let fs = with_project(vscode(), "multi");
    let inventory = inventory_at(&fs, &project(r"multi\api"));

    assert_eq!(json(item(&inventory, "api-only").state), "effective");
    assert!(
        !inventory
            .items
            .iter()
            .any(|item| item.name == "web-only" || item.name == "docs")
    );
}

/// The fixture workspace storage of the Project `app`.
fn app_storage() -> String {
    format!(r"{USER}\workspaceStorage\0a1b\state.vscdb")
}

#[test]
fn extension_servers_are_listed_from_the_cache_in_the_workspace_s_state_vscdb() {
    let fs = with_project(vscode(), "app");
    let inventory = inventory_at(&fs, &project("app"));

    let dart = item(&inventory, "Dart SDK MCP Server");
    assert_eq!(json(dart.state), "effective");
    let entry = &dart.entries[0];
    assert_eq!(entry.path, PathBuf::from(app_storage()));
    assert_eq!(
        entry.key_path,
        [
            "mcp.extCachedServers",
            "dart-code.dart-code/dart-sdk-mcp-servers",
            "servers",
            "0"
        ]
    );
    assert_eq!(entry.scope, "extension");
    assert_eq!(code(entry.provenance), "extension");
    assert_eq!(entry.config["launch"]["command"], "dart.exe");
    assert_eq!(json(&entry.remove)["code"], "extension");
    // The extension's server ranks below the user's own of the same name.
    assert_eq!(
        states(item(&inventory, "GitHub")),
        [
            (
                "effective".to_owned(),
                format!(r"{PROJECTS}\app\.vscode\mcp.json#servers.GitHub")
            ),
            (
                "shadowed".to_owned(),
                format!(r"{USER}\mcp.json#servers.github")
            ),
            (
                "shadowed".to_owned(),
                format!(
                    r#"{}#"mcp.extCachedServers"."github.copilot-chat/github".servers.0"#,
                    app_storage()
                )
            ),
        ]
    );
}

#[test]
fn a_workspace_s_cached_extension_servers_take_no_part_elsewhere() {
    let fs = with_project(vscode(), "app").with_folder(project("other"));
    let inventory = inventory_at(&fs, &project("other"));

    assert!(
        !inventory
            .items
            .iter()
            .any(|item| item.name == "Dart SDK MCP Server")
    );
}

#[test]
fn a_state_vscdb_that_cannot_be_read_is_a_sqlite_read_failure() {
    let fs = with_project(vscode(), "app").denying_reads_of(app_storage());
    let inventory = inventory_at(&fs, &project("app"));

    let failure = inventory
        .read_failures
        .iter()
        .find(|failure| failure.path == Path::new(&app_storage()))
        .expect("the store is a Read failure");
    assert_eq!(failure.agent, "vscode");
    assert_eq!(code(failure.kind), "sqlite");
    assert_eq!(code(failure.reaction), "ignores_file");
    assert_eq!(failure.project.as_deref(), Some(project("app").as_path()));
    assert!(
        !inventory
            .items
            .iter()
            .any(|item| item.name == "Dart SDK MCP Server")
    );
    assert_eq!(json(item(&inventory, "supabase").state), "effective");
}

/// `fs` with the fixture user's home folder.
fn with_home(fs: FakeFileSystem) -> FakeFileSystem {
    load(fs, &fixture_folder("home"), Path::new(HOME))
}

/// The VS Code skill named `name` in the default profile.
fn skill<'i>(inventory: &'i Inventory, name: &str) -> &'i Item {
    let found = item(inventory, name);
    assert_eq!(json(found.kind), "skill");
    found
}

#[test]
fn skills_rank_the_workspace_s_folders_above_the_personal_ones() {
    let fs = with_project(with_home(vscode()), "app");
    let inventory = inventory_at(&fs, &project("app"));

    assert_eq!(
        states(skill(&inventory, "review"))[..3],
        [
            (
                "effective".to_owned(),
                format!(r"{PROJECTS}\app\.github\skills\review")
            ),
            (
                "shadowed".to_owned(),
                format!(r"{HOME}\.agents\skills\review")
            ),
            (
                "shadowed".to_owned(),
                format!(r"{HOME}\.claude\skills\review")
            ),
        ]
    );
    let scopes: Vec<&str> = skill(&inventory, "review")
        .entries
        .iter()
        .map(|entry| entry.scope.as_str())
        .collect();
    assert_eq!(
        scopes,
        ["workspace-folder", "user", "user", "plugin", "extension"]
    );
}

#[test]
fn vs_code_names_a_skill_by_its_folder() {
    let inventory = inventory(&with_home(vscode()));

    // Claude Code names the same folder by its folder too, not by its
    // frontmatter `code-review`.
    assert!(
        inventory
            .items
            .iter()
            .any(|item| item.agent == "claude-code" && item.name == "review")
    );
    assert!(
        !inventory
            .items
            .iter()
            .any(|item| item.agent == "claude-code" && item.name == "code-review")
    );
    let review = skill(&inventory, "review");
    let folders: Vec<&Path> = review
        .entries
        .iter()
        .map(|entry| entry.path.as_path())
        .collect();
    assert_eq!(
        folders,
        [
            Path::new(HOME).join(r".agents\skills\review"),
            Path::new(HOME).join(r".claude\skills\review"),
            review_kit().join(r"skills\review"),
            Path::new(HOME).join(r".vscode\extensions\acme.helper-1.0.0\skills\review"),
        ]
    );
    assert!(
        !inventory
            .items
            .iter()
            .any(|item| item.agent == "vscode" && item.name == "code-review")
    );
}

#[test]
fn vs_code_loads_a_skill_whose_frontmatter_is_not_yaml_under_its_folder_s_name() {
    let fs = with_home(vscode()).with_file(
        format!(r"{HOME}\.agents\skills\broken\SKILL.md"),
        "---\ndescription: probe\nnot a key\n---\nbody\n",
    );
    let inventory = inventory(&fs);

    assert_eq!(json(skill(&inventory, "broken").state), "effective");
}

#[test]
fn vs_code_keeps_the_values_it_can_read_from_a_frontmatter_that_is_not_yaml_or_never_closes() {
    // The probe's skills, and the description `chat.getSkills` gives each.
    for (text, description) in [
        ("---\ndescription: [unclosed\n---\n", Value::Null),
        (
            "---\ndescription: probe\nargument-hint: [unclosed\n---\n",
            "probe".into(),
        ),
        ("---\ndescription: probe\nnot a key\n---\n", "probe".into()),
        ("---\ndescription: a: b\n---\n", "a: b".into()),
        ("---\ndescription: probe\nbody\n", "probe".into()),
    ] {
        for entry in in_project_and_user_folder("broken", text) {
            assert_eq!(
                entry_json(&entry)["config"]["description"],
                description,
                "{text} in {}",
                entry.scope
            );
        }
    }
}

#[test]
fn vs_code_reads_every_frontmatter_with_its_own_parser() {
    for (text, description) in [
        // YAML would refuse these, though config-digger's own reading takes
        // them as a description.
        ("---\ndescription: - item\n---\n", Value::Null),
        ("---\ndescription:x\n---\n", Value::Null),
        // A deeper key is one of the mapping, and a key with no value an
        // empty one.
        ("---\na:\n  b: c\n description: d\n---\n", "d".into()),
        // A comment, an escaped quote, an escape and a block scalar.
        ("---\ndescription: foo # c\n---\n", "foo".into()),
        ("---\ndescription: 'it''s'\n---\n", "it's".into()),
        ("---\ndescription: \"a\\nb\"\n---\n", "a\nb".into()),
        // A line break inside quotes folds into a space.
        ("---\ndescription: \"a\nb\"\n---\n", "a b".into()),
        (
            "---\ndescription: |\n  one\n  two\n---\n",
            "one\ntwo\n".into(),
        ),
    ] {
        for entry in in_project_and_user_folder("parsed", text) {
            assert_eq!(
                entry_json(&entry)["config"]["description"],
                description,
                "{text} in {}",
                entry.scope
            );
        }
    }
}

/// The sentences of the `unlisted` notes of `entry`, each checked to name
/// no switch.
fn unlisted(entry: &Entry) -> Vec<&str> {
    entry
        .notes
        .iter()
        .filter(|note| code(note.code) == "unlisted")
        .inspect(|note| assert!(note.switch.is_none()))
        .map(|note| note.sentence.as_str())
        .collect()
}

/// The fixture home with a personal skill `name` whose `SKILL.md` holds
/// `text`.
fn with_personal_skill(name: &str, text: &str) -> FakeFileSystem {
    with_home(vscode()).with_file(format!(r"{HOME}\.copilot\skills\{name}\SKILL.md"), text)
}

/// The skill `name` whose `SKILL.md` holds `text`, in the fixture Project
/// `app`'s `.agents\skills` and then in the personal `~\.copilot\skills`,
/// each checked to be Effective.
fn in_project_and_user_folder(name: &str, text: &str) -> [Entry; 2] {
    let fs = with_project(with_home(vscode()), "app").with_file(
        project("app").join(format!(r".agents\skills\{name}\SKILL.md")),
        text,
    );
    let in_project = skill(&inventory_at(&fs, &project("app")), name).entries[0].clone();
    let in_user_folder =
        skill(&inventory(&with_personal_skill(name, text)), name).entries[0].clone();
    let entries = [in_project, in_user_folder];
    for (entry, scope) in entries.iter().zip(["workspace-folder", "user"]) {
        assert_eq!(entry.scope, scope);
        assert_eq!(code(entry.state), "effective");
    }
    entries
}

/// The sentences of the `unlisted` notes of the skill `name` whose
/// `SKILL.md` holds `text`, in a workspace folder and in a user folder, as
/// [`in_project_and_user_folder`] reads it.
fn unlisted_in_project_and_user_folder(name: &str, text: &str) -> Vec<Vec<String>> {
    in_project_and_user_folder(name, text)
        .iter()
        .map(|entry| unlisted(entry).into_iter().map(str::to_owned).collect())
        .collect()
}

/// The note of a skill VS Code leaves out of its model's list because its
/// `SKILL.md` has no `description`.
const NO_DESCRIPTION: &str = "The user can still run this skill in VS Code, but VS Code does not list it to its model: its SKILL.md has no `description`.";

/// The note of a skill VS Code leaves out of its model's list because its
/// `SKILL.md` has no `description` and sets `disable-model-invocation: true`.
const NO_DESCRIPTION_AND_DISABLED: &str = "The user can still run this skill in VS Code, but VS Code does not list it to its model: its SKILL.md has no `description` and sets `disable-model-invocation: true`.";

#[test]
fn a_skill_with_no_frontmatter_is_one_vs_code_leaves_out_of_its_model_s_list() {
    for bare in in_project_and_user_folder("bare", "A skill with no frontmatter.\n") {
        assert!(
            bare.notes
                .iter()
                .any(|note| code(note.code) == "skill_fallback")
        );
        assert_eq!(unlisted(&bare), [NO_DESCRIPTION]);
    }
}

#[test]
fn a_skill_whose_yaml_frontmatter_has_no_description_is_unlisted() {
    assert_eq!(
        unlisted_in_project_and_user_folder("plain", "---\nname: plain\n---\nbody\n"),
        [[NO_DESCRIPTION]; 2]
    );
}

#[test]
fn a_skill_that_sets_disable_model_invocation_to_true_is_unlisted() {
    assert_eq!(
        unlisted_in_project_and_user_folder(
            "manual",
            "---\ndescription: Deploys.\ndisable-model-invocation: true\n---\n"
        ),
        [[
            "The user can still run this skill in VS Code, but VS Code does not list it to its model: its SKILL.md sets `disable-model-invocation: true`."
        ]; 2]
    );
}

#[test]
fn a_skill_unlisted_for_both_reasons_has_one_note_naming_both() {
    assert_eq!(
        unlisted_in_project_and_user_folder("manual", "---\ndisable-model-invocation: true\n---\n"),
        [[NO_DESCRIPTION_AND_DISABLED]; 2]
    );
}

#[test]
fn a_skill_with_a_description_that_leaves_model_invocation_on_is_listed() {
    for text in [
        "---\ndescription: Deploys.\ndisable-model-invocation: false\n---\n",
        "---\ndescription: Deploys.\n---\n",
        // VS Code reads only the value `true` as the boolean.
        "---\ndescription: Deploys.\ndisable-model-invocation: True\n---\n",
    ] {
        assert_eq!(
            unlisted_in_project_and_user_folder("deploy", text),
            [[""; 0]; 2],
            "{text}"
        );
    }
}

#[test]
fn a_skill_is_unlisted_for_what_vs_code_s_parser_reads_from_its_frontmatter() {
    const DISABLED: &str = "The user can still run this skill in VS Code, but VS Code does not list it to its model: its SKILL.md sets `disable-model-invocation: true`.";
    for (text, note) in [
        // A collection that never closes is no description.
        ("---\ndescription: [unclosed\n---\n", NO_DESCRIPTION),
        (
            "---\nname: broken\nargument-hint: {unclosed\n---\n",
            NO_DESCRIPTION,
        ),
        // An empty one is none.
        ("---\ndescription:\nnot a key\n---\n", NO_DESCRIPTION),
        ("---\ndescription: \"\"\n---\n", NO_DESCRIPTION),
        ("---\ndescription: [a, b]\n---\n", NO_DESCRIPTION),
        ("---\ndescription: - item\n---\n", NO_DESCRIPTION),
        (
            "---\ndisable-model-invocation: true\nnot a key\n---\n",
            NO_DESCRIPTION_AND_DISABLED,
        ),
        (
            "---\ndisable-model-invocation: true\nbody\n",
            NO_DESCRIPTION_AND_DISABLED,
        ),
        (
            "---\ndescription: probe\ndisable-model-invocation: true\nnot a key\n---\n",
            DISABLED,
        ),
        // VS Code reads only the value `true` as the boolean, quoted or not.
        (
            "---\ndescription: a: b\ndisable-model-invocation: \"true\"\n---\n",
            DISABLED,
        ),
        (
            "---\ndescription: Deploys.\ndisable-model-invocation: \"true\"\n---\n",
            DISABLED,
        ),
    ] {
        assert_eq!(
            unlisted_in_project_and_user_folder("broken", text),
            [[note]; 2],
            "{text}"
        );
    }
}

#[test]
fn a_frontmatter_that_is_not_yaml_with_a_description_vs_code_reads_is_listed() {
    for text in [
        // The collection that never closes takes in the line after it.
        "---\ndescription: probe\nargument-hint: [unclosed\ndisable-model-invocation: true\n---\n",
        // A line that is no key ends the frontmatter.
        "---\ndescription: probe\nnot a key\ndisable-model-invocation: true\n---\n",
        // Of a key given twice, the first wins.
        "---\ndescription: probe\ndisable-model-invocation: false\ndisable-model-invocation: true\n---\n",
        "---\ndescription: probe\ndisable-model-invocation: True\nnot a key\n---\n",
    ] {
        assert_eq!(
            unlisted_in_project_and_user_folder("broken", text),
            [[""; 0]; 2],
            "{text}"
        );
    }
}

#[test]
fn a_shadowed_skill_with_no_description_stays_shadowed_with_the_note() {
    let fs = with_project(
        with_personal_skill("plain", "---\nname: plain\n---\n"),
        "app",
    )
    .with_file(
        project("app").join(r".agents\skills\plain\SKILL.md"),
        "---\ndescription: Plain.\n---\n",
    );
    let inventory = inventory_at(&fs, &project("app"));

    let plain = skill(&inventory, "plain");
    assert!(unlisted(&plain.entries[0]).is_empty());
    let personal = &plain.entries[1];
    assert_eq!(code(personal.state), "shadowed");
    assert_eq!(unlisted(personal), [NO_DESCRIPTION]);
}

#[test]
fn an_extension_s_or_an_agent_plugin_s_skill_with_no_description_is_unlisted() {
    let fs = with_home(vscode())
        .with_file(
            Path::new(HOME)
                .join(r".vscode\extensions\acme.helper-1.0.0\skills\troubleshoot\SKILL.md"),
            "---\nname: troubleshoot\n---\n",
        )
        .with_file(
            review_kit().join(r"skills\triage\SKILL.md"),
            "---\nname: triage\n---\n",
        );
    let inventory = inventory(&fs);

    for (name, provenance) in [("troubleshoot", "extension"), ("triage", "plugin")] {
        let entry = &skill(&inventory, name).entries[0];
        assert_eq!(code(entry.provenance), provenance);
        assert_eq!(code(entry.state), "effective");
        assert_eq!(unlisted(entry), [NO_DESCRIPTION], "{name}");
    }
}

#[test]
fn an_extension_s_or_an_agent_plugin_s_frontmatter_that_is_not_yaml_keeps_its_values() {
    let extension_skill =
        Path::new(HOME).join(r".vscode\extensions\acme.helper-1.0.0\skills\troubleshoot");
    let plugin_skill = review_kit().join(r"skills\triage");
    let fs = with_home(vscode())
        .with_file(
            extension_skill.join("SKILL.md"),
            "---\ndescription: probe\nnot a key\n---\n",
        )
        .with_file(
            plugin_skill.join("SKILL.md"),
            "---\ndescription: [unclosed\n---\n",
        );
    let inventory = inventory(&fs);

    let troubleshoot = &skill(&inventory, "troubleshoot").entries[0];
    assert_eq!(code(troubleshoot.provenance), "extension");
    assert_eq!(entry_json(troubleshoot)["config"]["description"], "probe");
    assert!(unlisted(troubleshoot).is_empty());
    let triage = &skill(&inventory, "triage").entries[0];
    assert_eq!(code(triage.provenance), "plugin");
    assert_eq!(code(triage.state), "effective");
    assert_eq!(unlisted(triage), [NO_DESCRIPTION]);
    // The skill's content shows the description VS Code reads too.
    let content = core(&fs).skill_content(&extension_skill, "vscode").unwrap();
    assert_eq!(content.description.as_deref(), Some("probe"));
}

#[test]
fn another_agent_reading_the_same_skill_gives_it_no_unlisted_note() {
    let fs = with_home(vscode()).with_file(
        format!(r"{HOME}\.claude\skills\plain\SKILL.md"),
        "---\nname: plain\ndisable-model-invocation: true\n---\n",
    );
    let inventory = inventory(&fs);

    assert_eq!(
        unlisted(&skill(&inventory, "plain").entries[0]),
        [NO_DESCRIPTION_AND_DISABLED]
    );
    let claude = inventory
        .items
        .iter()
        .find(|item| item.agent == "claude-code" && item.name == "plain")
        .unwrap();
    assert!(unlisted(&claude.entries[0]).is_empty());
}

#[test]
fn a_personal_skill_s_skill_manager_is_its_provenance() {
    let inventory = inventory(&with_home(vscode()));

    let compose = &skill(&inventory, "jetpack-compose").entries[0];
    assert_eq!(
        compose.path,
        Path::new(HOME).join(r".copilot\skills\jetpack-compose")
    );
    assert_eq!(code(compose.provenance), "skill_manager");
    assert_eq!(compose.skill_manager.as_ref().unwrap().id, "skillfish");
}

#[test]
fn a_shared_file_is_also_read_by_another_agent_with_its_own_state() {
    // Claude Code counts the Project's own approval once the user trusts it.
    let fs = with_project(with_home(vscode()), "shared").with_file(
        Path::new(HOME).join(".claude.json"),
        r#"{ "projects": { "E:/projects/shared": { "hasTrustDialogAccepted": true } } }"#,
    );
    let inventory = inventory_at(&fs, &project("shared"));

    let dot_mcp = format!(r"{PROJECTS}\shared\.mcp.json#mcpServers.supabase");
    let vscode_entry = item(&inventory, "Supabase")
        .entries
        .iter()
        .find(|entry| entry.locator == dot_mcp)
        .unwrap();
    assert_eq!(code(vscode_entry.state), "shadowed");
    let also: Vec<(&str, &str, String)> = vscode_entry
        .also_read_by
        .iter()
        .map(|reader| {
            (
                reader.agent.as_str(),
                reader.home.as_str(),
                code(reader.state),
            )
        })
        .collect();
    assert_eq!(also, [("claude-code", "default", "effective".to_owned())]);
    assert!(
        vscode_entry.also_read_by[0]
            .sentence
            .contains("Claude Code"),
        "{}",
        vscode_entry.also_read_by[0].sentence
    );

    let claude_entry = &inventory
        .items
        .iter()
        .find(|item| item.agent == "claude-code" && item.name == "supabase")
        .unwrap()
        .entries[0];
    assert_eq!(code(claude_entry.state), "effective");
    let also: Vec<(&str, String)> = claude_entry
        .also_read_by
        .iter()
        .map(|reader| (reader.home.as_str(), code(reader.state)))
        .collect();
    // Each VS Code profile that reads the folder is another reader.
    assert!(
        also.contains(&("default", "shadowed".to_owned())),
        "{also:?}"
    );
    assert!(
        claude_entry
            .also_read_by
            .iter()
            .all(|reader| reader.agent == "vscode")
    );
}

#[test]
fn a_skill_folder_two_agents_read_names_each_one_s_state() {
    let inventory = inventory(&with_home(vscode()));

    let claude = inventory
        .items
        .iter()
        .find(|item| item.agent == "claude-code" && item.name == "review")
        .unwrap();
    let readers: Vec<(&str, &str)> = claude.entries[0]
        .also_read_by
        .iter()
        .filter(|reader| reader.home == "default")
        .map(|reader| (reader.agent.as_str(), reader.home.as_str()))
        .collect();
    assert_eq!(readers, [("vscode", "default")]);
    assert_eq!(code(claude.entries[0].also_read_by[0].state), "shadowed");
    // An Entry no other Agent reads has none.
    assert!(item(&inventory, "fetch").entries[0].also_read_by.is_empty());
}

/// The folder of the fixture agent plugin `installed.json` lists, in VS
/// Code's clone of its marketplace.
fn review_kit() -> PathBuf {
    Path::new(HOME).join(r".vscode\agent-plugins\github.com\acme\review-market\plugins\review-kit")
}

#[test]
fn an_agent_plugin_s_servers_rank_below_the_user_s() {
    let inventory = inventory(&with_home(vscode()));

    let linear = &item(&inventory, "linear").entries[0];
    assert_eq!(code(linear.state), "effective");
    assert_eq!(linear.path, review_kit().join(".mcp.json"));
    assert_eq!(code(linear.provenance), "plugin");
    assert_eq!(linear.scope, "plugin");
    let plugin = linear.plugin.as_ref().unwrap();
    assert_eq!(
        (plugin.id.as_str(), plugin.name.as_str()),
        (
            "github.com/acme/review-market/plugins/review-kit",
            "review-kit"
        )
    );
    assert_eq!(json(&linear.remove)["code"], "plugin");
    assert_eq!(
        states(item(&inventory, "fetch")),
        [
            (
                "effective".to_owned(),
                format!(r"{USER}\mcp.json#servers.fetch")
            ),
            (
                "shadowed".to_owned(),
                format!(r"{}\.mcp.json#mcpServers.fetch", review_kit().display())
            ),
        ]
    );
}

/// VS Code's `installed.json`, which lists its agent plugins.
fn installed_json() -> PathBuf {
    Path::new(HOME).join(r".vscode\agent-plugins\installed.json")
}

/// `fs` with `installed.json` listing only the plugins `plugins`, each a
/// folder in `plugins` of the fixture marketplace's clone and its files.
fn with_listed(fs: FakeFileSystem, plugins: &[(&str, &[(&str, &str)])]) -> FakeFileSystem {
    let market = "file:///c%3A/Users/fixture/.vscode/agent-plugins/github.com/acme/review-market";
    let installed: Vec<Value> = plugins
        .iter()
        .map(|(folder, _)| {
            serde_json::json!({
                "pluginUri": format!("{market}/plugins/{folder}"),
                "marketplace": "https://github.com/acme/review-market.git",
                "name": folder,
            })
        })
        .collect();
    let listed = serde_json::json!({ "version": 1, "installed": installed });
    let fs = fs.with_file(installed_json(), listed.to_string());
    plugins.iter().fold(fs, |fs, (folder, files)| {
        let folder = review_kit().with_file_name(folder);
        files.iter().fold(fs, |fs, (file, text)| {
            fs.with_file(folder.join(file), *text)
        })
    })
}

/// The name of the plugin of the VS Code server Item named `server`.
fn plugin_of(inventory: &Inventory, server: &str) -> String {
    let entry = &item(inventory, server).entries[0];
    entry.plugin.as_ref().unwrap().name.clone()
}

#[test]
fn a_listed_plugin_is_named_by_its_manifest_wherever_it_lies_or_else_by_its_folder() {
    let manifest = |name: &str| format!(r#"{{ "name": "{name}" }}"#);
    let servers =
        |name: &str| format!(r#"{{ "mcpServers": {{ "{name}": {{ "command": "x" }} }} }}"#);
    let (root, github, claude, open) = (
        manifest("named-root"),
        manifest("named-github"),
        manifest("named-claude"),
        manifest("named-open"),
    );
    let servers: Vec<String> = ["s-root", "s-github", "s-claude", "s-open", "s-bare"]
        .into_iter()
        .map(servers)
        .collect();
    let fs = with_listed(
        with_home(vscode()),
        &[
            (
                "root",
                &[("plugin.json", &root), (".mcp.json", &servers[0])],
            ),
            (
                "github",
                &[
                    (".github/plugin/plugin.json", &github),
                    (".mcp.json", &servers[1]),
                ],
            ),
            (
                "claude",
                &[
                    (".claude-plugin/plugin.json", &claude),
                    (".mcp.json", &servers[2]),
                ],
            ),
            (
                "open",
                &[(".plugin/plugin.json", &open), (".mcp.json", &servers[3])],
            ),
            ("bare", &[(".mcp.json", &servers[4])]),
        ],
    );
    let inventory = inventory(&fs);

    let names: Vec<String> = ["s-root", "s-github", "s-claude", "s-open", "s-bare"]
        .into_iter()
        .map(|server| plugin_of(&inventory, server))
        .collect();
    assert_eq!(
        names,
        [
            "named-root",
            "named-github",
            "named-claude",
            "named-open",
            "bare"
        ]
    );
}

#[test]
fn a_listed_plugin_is_named_by_a_manifest_with_comments_and_trailing_commas_wherever_it_lies() {
    let manifest = |name: &str| {
        format!(
            "{{\n  // The plugin's name.\n  /* VS Code reads it as JSONC. */\n  \"name\": \"{name}\",\n}}\n"
        )
    };
    let servers =
        |name: &str| format!(r#"{{ "mcpServers": {{ "{name}": {{ "command": "x" }} }} }}"#);
    let (root, github, claude, open) = (
        manifest("named-root"),
        manifest("named-github"),
        manifest("named-claude"),
        manifest("named-open"),
    );
    let servers: Vec<String> = ["s-root", "s-github", "s-claude", "s-open"]
        .into_iter()
        .map(servers)
        .collect();
    let fs = with_listed(
        with_home(vscode()),
        &[
            (
                "root",
                &[("plugin.json", &root), (".mcp.json", &servers[0])],
            ),
            (
                "github",
                &[
                    (".github/plugin/plugin.json", &github),
                    (".mcp.json", &servers[1]),
                ],
            ),
            (
                "claude",
                &[
                    (".claude-plugin/plugin.json", &claude),
                    (".mcp.json", &servers[2]),
                ],
            ),
            (
                "open",
                &[(".plugin/plugin.json", &open), (".mcp.json", &servers[3])],
            ),
        ],
    );
    let inventory = inventory(&fs);

    let names: Vec<String> = ["s-root", "s-github", "s-claude", "s-open"]
        .into_iter()
        .map(|server| plugin_of(&inventory, server))
        .collect();
    assert_eq!(
        names,
        ["named-root", "named-github", "named-claude", "named-open"]
    );
}

/// An Agent that lists its plugins in `~\.lister\installed.json` as VS
/// Code does, with no folder fallback, and reads each one's `.mcp.json`.
const PLUGIN_LISTER: &str = r#"
format_version = 1

[agent]
id = "plugin-lister"
name = "Plugin Lister"

[[homes]]
id = "default"
folder = { block = "path", path = "~" }
roots.lister = ".lister"

[[scopes]]
id = "plugin"

[[plugins]]
id = "listed"
registry = { block = "json-plugin-list", anchor = { block = "machine", path = "~/.lister/installed.json" }, list = ["installed"], folder = "path" }
name = { file = "plugin.json", key_path = ["name"] }

[[locations]]
id = "plugin-mcp"
anchor = { block = "plugin", plugins = "listed", path = ".mcp.json" }
format = { block = "json", dialect = "strict" }
root = ["mcpServers"]
kind = "mcp"
scope = "plugin"
provenance = "plugin"
store_class = "plain"
on_read_failure = "ignores_file"

[items.mcp]
precedence = ["plugin-mcp"]
competition = { block = "exact-name" }
checks = ["disable", "precedence"]
"#;

#[test]
fn a_plugin_list_without_the_folder_fallback_names_an_unnamed_plugin_by_its_id() {
    let lister = Path::new(HOME).join(".lister");
    let bare = lister.join(r"plugins\bare");
    let listed = serde_json::json!({ "installed": [{ "path": bare.display().to_string() }] });
    let fs = FakeFileSystem::new()
        .with_file(lister.join("installed.json"), listed.to_string())
        .with_file(
            bare.join(".mcp.json"),
            r#"{ "mcpServers": { "lister-server": { "command": "x" } } }"#,
        );
    let core = Core::with_extra_definitions(
        fake::ports(fs, env()),
        &[("plugin-lister.toml", PLUGIN_LISTER)],
    )
    .unwrap();
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    let inventory = core.resolve(&snapshot, &Context::default());

    let server = inventory
        .items
        .iter()
        .find(|item| item.agent == "plugin-lister" && item.name == "lister-server")
        .unwrap();
    let plugin = server.entries[0].plugin.as_ref().unwrap();
    assert_eq!(
        (plugin.id.as_str(), plugin.name.as_str()),
        ("plugins/bare", "plugins/bare")
    );
}

#[test]
fn a_manifest_s_mcp_servers_file_adds_its_servers_to_the_plugin_s_mcp_json() {
    let fs = with_listed(
        with_home(vscode()),
        &[(
            "named",
            &[
                (
                    "plugin.json",
                    r#"{ "name": "named", "mcpServers": "./servers.json" }"#,
                ),
                (
                    ".mcp.json",
                    r#"{ "mcpServers": { "dot": { "command": "dot" } } }"#,
                ),
                (
                    "servers.json",
                    r#"{ "mcpServers": { "file": { "command": "file" } } }"#,
                ),
            ],
        )],
    );
    let inventory = inventory(&fs);

    let folder = review_kit().with_file_name("named");
    for (server, file) in [("dot", ".mcp.json"), ("file", "servers.json")] {
        let entry = &item(&inventory, server).entries[0];
        assert_eq!(entry.path, folder.join(file));
        assert_eq!(code(entry.state), "effective");
        assert_eq!(entry.plugin.as_ref().unwrap().name, "named");
    }
}

#[test]
fn a_plugin_vs_code_installed_from_a_marketplace_is_read_where_installed_json_says() {
    // `installed.json` as VS Code 1.143.1 writes it once it installs a
    // plugin from a marketplace, with a `relativePath` beside its
    // `pluginUri`, and the plugin in VS Code's clone of the marketplace.
    let folder = Path::new(HOME)
        .join(r".vscode\agent-plugins\cd-throwaway.invalid\cd-throwaway\market\plugins\installed");
    let listed = r#"{"version":1,"installed":[{
        "pluginUri":"file:///c%3A/Users/fixture/.vscode/agent-plugins/cd-throwaway.invalid/cd-throwaway/market/plugins/installed",
        "relativePath":"cd-throwaway.invalid/cd-throwaway/market/plugins/installed",
        "marketplace":"https://cd-throwaway.invalid/cd-throwaway/market.git",
        "name":"cd-throwaway-installed"}]}"#;
    let fs = with_home(vscode())
        .with_file(installed_json(), listed)
        .with_file(
            folder.join("plugin.json"),
            r#"{ "name": "cd-throwaway-installed", "version": "1.0.0" }"#,
        )
        .with_file(
            folder.join(".mcp.json"),
            r#"{ "mcpServers": { "cd-throwaway-installed": { "command": "x" } } }"#,
        );
    let inventory = inventory(&fs);

    let entry = &item(&inventory, "cd-throwaway-installed").entries[0];
    assert_eq!(code(entry.state), "effective");
    assert_eq!(entry.path, folder.join(".mcp.json"));
    let plugin = entry.plugin.as_ref().unwrap();
    assert_eq!(
        (plugin.id.as_str(), plugin.name.as_str()),
        (
            "cd-throwaway.invalid/cd-throwaway/market/plugins/installed",
            "cd-throwaway-installed"
        )
    );
}

/// The default profile's server Entries of agent plugins in `inventory`,
/// each `<name> <state> <file name>`, sorted.
fn plugin_server_states(inventory: &Inventory) -> Vec<String> {
    let mut servers: Vec<String> = inventory
        .items
        .iter()
        .filter(|item| item.agent == "vscode" && item.home == "default" && json(item.kind) == "mcp")
        .flat_map(|item| item.entries.iter().map(move |entry| (&item.name, entry)))
        .filter(|(_, entry)| entry.scope == "plugin")
        .map(|(name, entry)| {
            let file = entry.path.file_name().unwrap().to_string_lossy();
            format!("{name} {} {file}", code(entry.state))
        })
        .collect();
    servers.sort();
    servers
}

#[test]
fn a_manifest_names_a_file_of_servers_from_claude_plugin_or_plugin_not_github_plugin() {
    // The probe's `manifests` launch: each manifest names `servers.json`.
    // VS Code 1.129.1 to 1.141.0 never take `.github/plugin/plugin.json` for
    // a plugin's manifest.
    let manifest = |tag: &str| {
        format!(r#"{{ "name": "cd-throwaway-{tag}", "mcpServers": "./servers.json" }}"#)
    };
    let servers = |tag: &str| {
        format!(r#"{{ "mcpServers": {{ "cd-throwaway-{tag}": {{ "command": "x" }} }} }}"#)
    };
    let [github, claude, open] = ["github", "claude", "open"].map(|tag| {
        [
            manifest(tag),
            servers(&format!("{tag}-dot")),
            servers(&format!("{tag}-file")),
        ]
    });
    let fs = with_listed(
        with_home(vscode()),
        &[
            (
                "github",
                &[
                    (".github/plugin/plugin.json", &github[0]),
                    (".mcp.json", &github[1]),
                    ("servers.json", &github[2]),
                ],
            ),
            (
                "claude",
                &[
                    (".claude-plugin/plugin.json", &claude[0]),
                    (".mcp.json", &claude[1]),
                    ("servers.json", &claude[2]),
                ],
            ),
            (
                "open",
                &[
                    (".plugin/plugin.json", &open[0]),
                    (".mcp.json", &open[1]),
                    ("servers.json", &open[2]),
                ],
            ),
        ],
    );

    assert_eq!(
        plugin_server_states(&inventory(&fs)),
        [
            "cd-throwaway-claude-dot effective .mcp.json",
            "cd-throwaway-claude-file effective servers.json",
            "cd-throwaway-github-dot effective .mcp.json",
            "cd-throwaway-open-dot effective .mcp.json",
            "cd-throwaway-open-file effective servers.json",
        ]
    );
}

#[test]
fn a_plugin_s_mcp_json_server_shadows_the_one_of_its_name_in_the_file_its_manifest_names() {
    let fs = with_listed(
        with_home(vscode()),
        &[(
            "clash",
            &[
                (
                    "plugin.json",
                    r#"{ "name": "clash", "mcpServers": "./servers.json" }"#,
                ),
                (
                    ".mcp.json",
                    r#"{ "mcpServers": { "cd-throwaway-clash": { "command": "dot" } } }"#,
                ),
                (
                    "servers.json",
                    r#"{ "mcpServers": { "cd-throwaway-clash": { "command": "file" } } }"#,
                ),
            ],
        )],
    );
    let inventory = inventory(&fs);

    let folder = review_kit().with_file_name("clash");
    assert_eq!(
        states(item(&inventory, "cd-throwaway-clash")),
        [
            (
                "effective".to_owned(),
                format!(
                    r"{}\.mcp.json#mcpServers.cd-throwaway-clash",
                    folder.display()
                )
            ),
            (
                "shadowed".to_owned(),
                format!(
                    r"{}\servers.json#mcpServers.cd-throwaway-clash",
                    folder.display()
                )
            ),
        ]
    );
}

/// Whether a VS Code server Item of an agent plugin is listed.
fn any_plugin_server(inventory: &Inventory) -> bool {
    inventory
        .items
        .iter()
        .filter(|item| item.agent == "vscode" && json(item.kind) == "mcp")
        .flat_map(|item| &item.entries)
        .any(|entry| entry.scope == "plugin")
}

#[test]
fn an_installed_json_that_does_not_parse_is_a_read_failure_and_lists_no_plugin() {
    let fs = with_home(vscode()).with_file(installed_json(), r#"{ "installed": [,] }"#);
    let inventory = inventory(&fs);

    let failure = inventory
        .read_failures
        .iter()
        .find(|failure| failure.agent == "vscode" && failure.path == installed_json())
        .expect("installed.json is a Read failure");
    assert_eq!(json(failure.kind), "parse");
    assert!(!any_plugin_server(&inventory));
}

#[test]
fn without_installed_json_vs_code_has_no_agent_plugin() {
    // The fixture's plugin folders, without the `installed.json` beside them.
    let plugins = fixture_folder(r"home\.vscode\agent-plugins");
    let fs = load(
        vscode(),
        &plugins.join("github.com"),
        &review_kit().join(r"..\..\..\.."),
    )
    .with_folder(installed_json().with_file_name("stray"));
    let fs = load(
        fs,
        &plugins.join("stray"),
        &installed_json().with_file_name("stray"),
    );
    let inventory = inventory(&fs);

    assert!(!any_plugin_server(&inventory));
    assert!(
        !inventory
            .read_failures
            .iter()
            .any(|failure| failure.path == installed_json())
    );
}

#[test]
fn an_item_whose_plugin_uri_is_not_a_file_uri_or_that_has_none_is_skipped() {
    let listed = serde_json::json!({
        "version": 1,
        "installed": [
            { "pluginUri": "vscode-remote://ssh-remote+box/home/me/review-kit", "name": "remote" },
            { "name": "no-uri" },
            {
                "pluginUri": "file:///c%3A/Users/fixture/.vscode/agent-plugins/github.com/acme/review-market/plugins/review-kit",
                "name": "review-kit"
            },
        ],
    });
    let fs = with_home(vscode()).with_file(installed_json(), listed.to_string());
    let inventory = inventory(&fs);

    let plugins: Vec<&str> = inventory
        .items
        .iter()
        .filter(|item| item.agent == "vscode")
        .flat_map(|item| &item.entries)
        .filter(|entry| entry.scope == "plugin")
        .filter_map(|entry| entry.plugin.as_ref())
        .map(|plugin| plugin.id.as_str())
        .collect();
    assert!(!plugins.is_empty());
    assert!(
        plugins
            .iter()
            .all(|id| *id == "github.com/acme/review-market/plugins/review-kit"),
        "{plugins:?}"
    );
}

#[test]
fn two_marketplaces_plugins_of_one_name_are_two_plugins() {
    let plugins = "file:///c%3A/Users/fixture/.vscode/agent-plugins/github.com";
    let listed = serde_json::json!({
        "version": 1,
        "installed": [
            { "pluginUri": format!("{plugins}/acme/one/plugins/kit"), "name": "kit" },
            { "pluginUri": format!("{plugins}/other/two/plugins/kit"), "name": "kit" },
        ],
    });
    let folder = |market: &str| {
        Path::new(HOME).join(format!(
            r".vscode\agent-plugins\github.com\{market}\plugins\kit"
        ))
    };
    let fs = with_home(vscode())
        .with_file(installed_json(), listed.to_string())
        .with_file(
            folder(r"acme\one").join(".mcp.json"),
            r#"{ "mcpServers": { "one": { "command": "one" } } }"#,
        )
        .with_file(
            folder(r"other\two").join(".mcp.json"),
            r#"{ "mcpServers": { "two": { "command": "two" } } }"#,
        );
    let inventory = inventory(&fs);

    let plugin = |server: &str| {
        let plugin = item(&inventory, server).entries[0].plugin.clone().unwrap();
        (plugin.id, plugin.name)
    };
    assert_eq!(
        plugin("one"),
        (
            "github.com/acme/one/plugins/kit".to_owned(),
            "kit".to_owned()
        )
    );
    assert_eq!(
        plugin("two"),
        (
            "github.com/other/two/plugins/kit".to_owned(),
            "kit".to_owned()
        )
    );
}

#[test]
fn a_folder_in_agent_plugins_that_installed_json_does_not_list_holds_no_plugin() {
    let inventory = inventory(&with_home(vscode()));

    assert!(
        !inventory
            .items
            .iter()
            .any(|item| item.agent == "vscode" && item.name == "stray-server"),
        "the unlisted folder's server is listed"
    );
    let entries = inventory.items.iter().flat_map(|item| &item.entries);
    assert!(
        !entries
            .filter_map(|entry| entry.plugin.as_ref())
            .any(|plugin| plugin.name == "stray"),
        "the unlisted folder is a plugin"
    );
}

#[test]
fn an_agent_plugin_s_skills_rank_below_the_personal_ones() {
    let inventory = inventory(&with_home(vscode()));

    let triage = &skill(&inventory, "triage").entries[0];
    assert_eq!(code(triage.state), "effective");
    assert_eq!(code(triage.provenance), "plugin");
    let review = skill(&inventory, "review");
    let plugin = review
        .entries
        .iter()
        .find(|entry| entry.path == review_kit().join(r"skills\review"))
        .unwrap();
    assert_eq!(code(plugin.state), "shadowed");
}

#[test]
fn an_extension_s_chat_skills_rank_last() {
    let inventory = inventory(&with_home(vscode()));

    let extension = Path::new(HOME).join(r".vscode\extensions\acme.helper-1.0.0");
    let troubleshoot = &skill(&inventory, "troubleshoot").entries[0];
    assert_eq!(troubleshoot.path, extension.join(r"skills\troubleshoot"));
    assert_eq!(code(troubleshoot.state), "effective");
    assert_eq!(code(troubleshoot.provenance), "extension");
    assert_eq!(troubleshoot.scope, "extension");
    assert_eq!(
        troubleshoot.plugin.as_ref().unwrap().id,
        "acme.helper-1.0.0"
    );
    assert_eq!(json(&troubleshoot.remove)["code"], "extension");
    let review = skill(&inventory, "review");
    let last = review.entries.last().unwrap();
    assert_eq!(last.path, extension.join(r"skills\review"));
    assert_eq!(code(last.state), "shadowed");
}

#[test]
fn an_extension_version_marked_obsolete_contributes_no_entries() {
    let inventory = inventory(&with_home(vscode()));

    let extension = Path::new(HOME).join(r".vscode\extensions\acme.helper-1.0.0");
    let troubleshoot = skill(&inventory, "troubleshoot");
    let paths: Vec<&Path> = troubleshoot
        .entries
        .iter()
        .map(|entry| entry.path.as_path())
        .collect();
    assert_eq!(paths, [extension.join(r"skills\troubleshoot")]);
    assert!(
        !inventory
            .items
            .iter()
            .any(|item| item.agent == "vscode" && item.name == "legacy"),
        "the obsolete version's own skill is listed"
    );
}

/// The folder of the fixture user setup of VS Code.
fn installation() -> PathBuf {
    Path::new(LOCAL_APP_DATA).join(r"Programs\Microsoft VS Code")
}

/// `fs` with the fixture user setup of VS Code, whose commit folder holds
/// the built-in extensions GitHub Copilot Chat and Git.
fn with_installation(fs: FakeFileSystem) -> FakeFileSystem {
    load(fs, &fixture_folder("installation"), &installation()).with_file(
        installation().join("Code.exe"),
        fake::executable(Some([1, 129, 1, 0])),
    )
}

/// The folder of the fixture built-in GitHub Copilot Chat.
fn copilot() -> PathBuf {
    installation().join(r"8a7abeba6e\resources\app\extensions\copilot")
}

#[test]
fn a_built_in_extension_s_chat_skills_are_extension_skills() {
    let inventory = inventory(&with_installation(with_home(vscode())));

    let create_skill = &skill(&inventory, "create-skill").entries[0];
    assert_eq!(
        create_skill.path,
        copilot().join(r"assets\prompts\skills\create-skill")
    );
    assert_eq!(code(create_skill.state), "effective");
    assert_eq!(code(create_skill.provenance), "extension");
    assert_eq!(create_skill.scope, "extension");
    let plugin = create_skill.plugin.as_ref().unwrap();
    assert_eq!(
        (plugin.id.as_str(), plugin.name.as_str()),
        ("copilot", "copilot-chat")
    );
    assert_eq!(code(skill(&inventory, "init").state), "effective");
}

#[test]
fn a_built_in_extension_s_skills_rank_among_extensions_before_installed_ones() {
    let fs = with_project(with_installation(with_home(vscode())), "app");
    let inventory = inventory_at(&fs, &project("app"));

    let review = skill(&inventory, "review");
    let entries: Vec<(&str, &Path)> = review
        .entries
        .iter()
        .map(|entry| (entry.scope.as_str(), entry.path.as_path()))
        .collect();
    let user_extension = Path::new(HOME).join(r".vscode\extensions\acme.helper-1.0.0");
    assert_eq!(
        entries[3..],
        [
            ("plugin", review_kit().join(r"skills\review").as_path()),
            (
                "extension",
                copilot().join(r"assets\prompts\skills\review").as_path()
            ),
            ("extension", user_extension.join(r"skills\review").as_path()),
        ]
    );
    assert_eq!(
        entries[..3]
            .iter()
            .map(|(scope, _)| *scope)
            .collect::<Vec<_>>(),
        ["workspace-folder", "user", "user"]
    );
    assert_eq!(code(review.entries[0].state), "effective");
    assert!(
        review.entries[1..]
            .iter()
            .all(|entry| code(entry.state) == "shadowed")
    );
}

#[test]
fn a_built_in_extension_s_skill_cannot_be_removed() {
    let inventory = inventory(&with_installation(with_home(vscode())));

    let remove = json(&skill(&inventory, "create-skill").entries[0].remove);
    assert_eq!(remove["status"], "not_offered");
    assert_eq!(remove["code"], "extension");
    assert_eq!(
        remove["sentence"],
        "copilot-chat is built into VS Code, which cannot uninstall it."
    );
    assert_eq!(remove["how_to_remove"], Value::Null);
}

/// `fs` with an extension in `folder` whose `package.json` holds `manifest`
/// and lists one `chatSkills` skill, `skill`.
fn with_extension(
    fs: FakeFileSystem,
    folder: &Path,
    mut manifest: Value,
    skill: &str,
) -> FakeFileSystem {
    manifest["contributes"] =
        serde_json::json!({ "chatSkills": [{ "path": format!("./skills/{skill}/SKILL.md") }] });
    fs.with_file(folder.join("package.json"), manifest.to_string())
        .with_file(
            folder.join(format!(r"skills\{skill}\SKILL.md")),
            format!("---\nname: {skill}\ndescription: Comes from {skill}.\n---\n"),
        )
}

/// The folder of a fixture built-in extension, `twin`.
fn built_in_twin() -> PathBuf {
    installation().join(r"8a7abeba6e\resources\app\extensions\twin")
}

/// The folder of a fixture installed extension, `acme.twin-<version>`.
fn installed_twin(version: &str) -> PathBuf {
    Path::new(HOME).join(format!(r".vscode\extensions\acme.twin-{version}"))
}

/// The fixture Installation and home, with a built-in extension whose
/// `package.json` holds `builtin` and whose skill is `twin-builtin`, and an
/// installed one whose `package.json` holds `installed` and whose skill is
/// `twin-installed`.
fn with_twins(builtin: Value, installed: Value) -> FakeFileSystem {
    let fs = with_extension(
        with_installation(with_home(vscode())),
        &built_in_twin(),
        builtin,
        "twin-builtin",
    );
    let version = installed["version"].as_str().unwrap_or("1.0.0").to_owned();
    with_extension(fs, &installed_twin(&version), installed, "twin-installed")
}

/// The manifest of the extension `acme.twin` at `version`.
fn twin(version: &str) -> Value {
    serde_json::json!({ "publisher": "acme", "name": "twin", "version": version })
}

/// The names of the VS Code skills `twin-builtin` and `twin-installed`
/// that are listed, in that order.
fn twin_skills(fs: &FakeFileSystem) -> Vec<&'static str> {
    let inventory = inventory(fs);
    ["twin-builtin", "twin-installed"]
        .into_iter()
        .filter(|name| {
            inventory
                .items
                .iter()
                .any(|item| item.agent == "vscode" && item.name == *name)
        })
        .collect()
}

#[test]
fn an_installed_extension_with_a_built_in_one_s_id_takes_its_place_at_a_higher_version() {
    let fs = with_twins(twin("1.0.0"), twin("2.0.0"));

    assert_eq!(twin_skills(&fs), ["twin-installed"]);
}

#[test]
fn an_installed_extension_with_a_built_in_one_s_id_is_skipped_at_the_same_or_a_lower_version() {
    for installed in ["1.0.0", "0.9.0"] {
        let fs = with_twins(twin("1.0.0"), twin(installed));

        assert_eq!(
            twin_skills(&fs),
            ["twin-builtin"],
            "installed at {installed}"
        );
    }
}

#[test]
fn extension_ids_that_differ_only_in_case_are_one_id() {
    let builtin = serde_json::json!({ "publisher": "Acme", "name": "Twin", "version": "1.0.0" });
    let fs = with_twins(builtin, twin("2.0.0"));

    assert_eq!(twin_skills(&fs), ["twin-installed"]);
}

#[test]
fn an_installed_extension_with_a_built_in_one_s_id_and_no_version_it_can_compare_is_skipped() {
    for installed in [
        serde_json::json!({ "publisher": "acme", "name": "twin" }),
        twin("next"),
    ] {
        let fs = with_twins(twin("1.0.0"), installed.clone());

        assert_eq!(twin_skills(&fs), ["twin-builtin"], "installed {installed}");
    }
    let builtin = serde_json::json!({ "publisher": "acme", "name": "twin" });
    let fs = with_twins(builtin, twin("2.0.0"));
    assert_eq!(
        twin_skills(&fs),
        ["twin-builtin"],
        "built in with no version"
    );
}

#[test]
fn extensions_whose_ids_differ_are_both_read() {
    let installed = serde_json::json!({ "publisher": "acme", "name": "other", "version": "2.0.0" });
    let fs = with_twins(twin("1.0.0"), installed);

    assert_eq!(twin_skills(&fs), ["twin-builtin", "twin-installed"]);
}

#[test]
fn an_extension_whose_manifest_gives_no_publisher_or_no_name_has_no_id_to_compare() {
    for (builtin, installed) in [
        (
            serde_json::json!({ "name": "twin", "version": "1.0.0" }),
            twin("0.9.0"),
        ),
        (
            serde_json::json!({ "publisher": "", "name": "twin", "version": "1.0.0" }),
            twin("0.9.0"),
        ),
        (
            twin("1.0.0"),
            serde_json::json!({ "publisher": "acme", "version": "0.9.0" }),
        ),
    ] {
        let fs = with_twins(builtin.clone(), installed.clone());

        assert_eq!(
            twin_skills(&fs),
            ["twin-builtin", "twin-installed"],
            "{builtin} and {installed}"
        );
    }
}

#[test]
fn an_installed_extension_takes_the_place_of_each_built_in_one_of_its_id_it_is_higher_than() {
    let fs = with_twins(twin("1.0.0"), twin("2.0.0"));
    // A commit folder left over from an update, with the same extension.
    let leftover = installation().join(r"0ld0c0mm1t\resources\app\extensions\twin");
    let fs = with_extension(fs, &leftover, twin("1.0.0"), "twin-builtin");

    assert_eq!(twin_skills(&fs), ["twin-installed"]);
}

#[test]
fn an_extension_version_marked_obsolete_takes_no_built_in_one_s_place() {
    let fs = with_twins(twin("1.0.0"), twin("2.0.0")).with_file(
        Path::new(HOME).join(r".vscode\extensions\.obsolete"),
        r#"{"acme.twin-2.0.0":true}"#,
    );

    assert_eq!(twin_skills(&fs), ["twin-builtin"]);
}

/// Claude Desktop's config file, which VS Code can discover.
const CLAUDE_DESKTOP: &str = r"C:\Users\fixture\AppData\Roaming\Claude\claude_desktop_config.json";

/// `fs` with a Claude Desktop config holding the server `filesystem`.
fn with_claude_desktop(fs: FakeFileSystem) -> FakeFileSystem {
    fs.with_file(
        CLAUDE_DESKTOP,
        r#"{ "mcpServers": { "filesystem": { "command": "npx", "args": ["-y", "@modelcontextprotocol/server-filesystem"] } } }"#,
    )
}

#[test]
fn another_app_s_servers_are_unread_until_discovery_is_turned_on_for_it() {
    let inventory = inventory(&with_claude_desktop(vscode()));

    let filesystem = item(&inventory, "filesystem");
    assert_eq!(json(filesystem.state), "unread");
    let entry = &filesystem.entries[0];
    assert_eq!(
        entry.locator,
        format!("{CLAUDE_DESKTOP}#mcpServers.filesystem")
    );
    assert_eq!(entry.scope, "discovered");
    let reason = entry.reason.as_ref().unwrap();
    assert_eq!(code(reason.code), "setting_value");
    assert!(
        reason.sentence.contains("chat.mcp.discovery.enabled"),
        "{}",
        reason.sentence
    );
    // Claude Desktop reads its own file all the same.
    let desktop: Vec<(&str, String)> = entry
        .also_read_by
        .iter()
        .map(|reader| (reader.agent.as_str(), code(reader.state)))
        .collect();
    assert_eq!(desktop, [("claude-desktop", "effective".to_owned())]);
}

/// The State of Claude Desktop's `filesystem` and the code of its reason.
fn discovered(inventory: &Inventory) -> (String, String) {
    let filesystem = item(inventory, "filesystem");
    let reason = filesystem.entries[0].reason.as_ref().unwrap();
    (code(filesystem.state), code(reason.code))
}

// VS Code reads `chat.mcp.discovery.enabled` for the window: in a
// workspace file, from its own `settings` and the user's, never from a
// member folder's `.vscode\settings.json`.

#[test]
fn a_member_folder_s_settings_turn_on_no_discovery_in_a_workspace_file() {
    let fs = with_claude_desktop(with_project(vscode(), "multi")).with_file(
        project(r"multi\api\.vscode\settings.json"),
        r#"{ "chat.mcp.discovery.enabled": { "claude-desktop": true } }"#,
    );
    let inventory = inventory_at(&fs, &workspace());

    assert_eq!(
        discovered(&inventory),
        ("unread".to_owned(), "setting_value".to_owned())
    );
}

#[test]
fn a_member_folder_s_settings_turn_off_no_discovery_in_a_workspace_file() {
    let fs = with_claude_desktop(with_project(vscode(), "multi"))
        .with_file(
            project(r"multi\api\.vscode\settings.json"),
            r#"{ "chat.mcp.discovery.enabled": { "claude-desktop": false } }"#,
        )
        .with_file(
            format!(r"{USER}\settings.json"),
            r#"{ "chat.mcp.discovery.enabled": { "claude-desktop": true } }"#,
        );
    let inventory = inventory_at(&fs, &workspace());

    // The user's settings turn the source on: only the user's trust in
    // each server keeps it from starting.
    assert_eq!(
        discovered(&inventory),
        ("blocked".to_owned(), "not_approved".to_owned())
    );
}

#[test]
fn a_folder_s_own_settings_turn_on_discovery_in_that_project() {
    let fs = with_claude_desktop(with_project(vscode(), "app")).with_file(
        project(r"app\.vscode\settings.json"),
        r#"{ "chat.mcp.discovery.enabled": { "claude-desktop": true } }"#,
    );
    let inventory = inventory_at(&fs, &project("app"));

    assert_eq!(
        discovered(&inventory),
        ("blocked".to_owned(), "not_approved".to_owned())
    );
}

#[test]
fn a_workspace_file_s_own_settings_turn_on_discovery_in_its_context() {
    let fs = with_claude_desktop(with_project(vscode(), "multi")).with_file(
        workspace(),
        r#"{
	"folders": [{ "path": "api" }, { "path": "web" }],
	"settings": { "chat.mcp.discovery.enabled": { "claude-desktop": true } }
}"#,
    );
    let inventory = inventory_at(&fs, &workspace());

    assert_eq!(
        discovered(&inventory),
        ("blocked".to_owned(), "not_approved".to_owned())
    );
}

#[test]
fn a_discovered_server_ranks_below_every_other_collection() {
    let fs = with_claude_desktop(vscode())
        .with_file(
            format!(r"{USER}\settings.json"),
            r#"{ "chat.mcp.discovery.enabled": { "claude-desktop": true } }"#,
        )
        .with_file(
            format!(r"{USER}\mcp.json"),
            r#"{ "servers": { "Filesystem": { "command": "fs-server" } } }"#,
        );
    let inventory = inventory(&fs);

    assert_eq!(
        states(item(&inventory, "Filesystem")),
        [
            (
                "effective".to_owned(),
                format!(r"{USER}\mcp.json#servers.Filesystem")
            ),
            (
                "shadowed".to_owned(),
                format!("{CLAUDE_DESKTOP}#mcpServers.filesystem")
            ),
        ]
    );
}

/// The fake Secret every fixture uses.
const SECRET: &str = "not-a-real-secret";

#[test]
fn an_mcp_json_shows_whole_references_and_masks_literal_secrets() {
    let fs = vscode().with_file(
        format!(r"{USER}\mcp.json"),
        format!(
            r#"{{ "servers": {{ "api": {{
                "command": "api-server",
                "envFile": "${{workspaceFolder}}/.env",
                "env": {{
                    "FROM_ENV": "${{env:API_TOKEN}}",
                    "FROM_INPUT": "${{input:api-token}}",
                    "HOME": "${{userHome}}",
                    "FOLDER": "${{workspaceFolder}}",
                    "NOT_VS_CODE": "${{API_TOKEN}}",
                    "LITERAL": "{SECRET}"
                }}
            }} }} }}"#
        ),
    );
    let inventory = inventory(&fs);

    let entry = &item(&inventory, "api").entries[0];
    let env = &entry.config["env"];
    assert_eq!(env["FROM_ENV"], "${env:API_TOKEN}");
    assert_eq!(env["FROM_INPUT"], "${input:api-token}");
    assert_eq!(env["HOME"], "${userHome}");
    assert_eq!(env["FOLDER"], "${workspaceFolder}");
    // VS Code replaces no `${NAME}` of another name.
    assert_eq!(env["NOT_VS_CODE"], "••••");
    assert_eq!(env["LITERAL"], "••••");
    assert_eq!(entry.config["envFile"], "${workspaceFolder}/.env");
    assert!(!json(&inventory).to_string().contains(SECRET));
}

#[test]
fn vs_code_takes_every_value_of_a_mcp_json_as_written() {
    let fs = with_home(with_project(vscode(), "shared")).with_file(
        project(r"shared\.mcp.json"),
        r#"{ "mcpServers": { "supabase": { "command": "npx", "env": { "TOKEN": "${SUPABASE_TOKEN}" } } } }"#,
    );
    let inventory = inventory_at(&fs, &project("shared"));

    let locator = format!(r"{PROJECTS}\shared\.mcp.json#mcpServers.supabase");
    let config_of = |agent: &str| {
        inventory
            .items
            .iter()
            .filter(|item| item.agent == agent && item.home == "default")
            .flat_map(|item| &item.entries)
            .find(|entry| entry.locator == locator)
            .unwrap()
            .config
            .clone()
    };
    assert_eq!(
        config_of("claude-code")["env"]["TOKEN"],
        "${SUPABASE_TOKEN}"
    );
    assert_eq!(config_of("vscode")["env"]["TOKEN"], "••••");
}

#[test]
fn a_cached_extension_server_s_launch_secrets_are_masked() {
    let fs = with_project(vscode(), "app");
    let inventory = inventory_at(&fs, &project("app"));

    let dart = &item(&inventory, "Dart SDK MCP Server").entries[0];
    assert_eq!(dart.config["launch"]["env"]["DART_TOKEN"], "••••");
    assert_eq!(
        dart.config["launch"]["args"],
        serde_json::json!(["••••", "••••", "••••"])
    );
    // The cache spells headers as `[name, value]` pairs, and a URL as a URI
    // object whose `external` encodes its query's `=`.
    let github = item(&inventory, "GitHub")
        .entries
        .iter()
        .find(|entry| entry.locator.contains("extCachedServers"))
        .unwrap();
    let launch = &github.config["launch"];
    assert_eq!(
        launch["headers"],
        serde_json::json!([["Authorization", "Bearer ••••"]])
    );
    assert_eq!(launch["uri"]["path"], "/mcp/");
    assert_eq!(launch["uri"]["scheme"], "https");
    for part in ["external", "authority", "query", "fragment"] {
        assert_eq!(launch["uri"][part], "••••", "{part}");
    }
    assert!(!json(&inventory).to_string().contains(SECRET));
}

/// What VS Code does, as its definition says, with the file at `path`,
/// which config-digger cannot read.
fn reaction(inventory: &Inventory, path: &Path) -> String {
    let failure = inventory
        .read_failures
        .iter()
        .find(|failure| failure.agent == "vscode" && failure.path == path)
        .expect("the file is a Read failure");
    code(failure.reaction)
}

#[test]
fn vs_code_skips_an_mcp_file_it_cannot_parse_but_reads_a_plugin_s_around_the_error() {
    let broken = r#"{ "servers": { "a": { "command": "a" },, "b": { "command": "b" } } }"#;
    let user_mcp = PathBuf::from(USER).join("mcp.json");
    let plugin_mcp = review_kit().join(".mcp.json");
    let files = [
        project(r"app\.vscode\mcp.json"),
        project(r"app\.mcp.json"),
        user_mcp.clone(),
        plugin_mcp.clone(),
    ];
    let fs = files
        .iter()
        .fold(with_home(with_project(vscode(), "app")), |fs, file| {
            fs.with_file(file, broken)
        });
    let inventory = inventory_at(&fs, &project("app"));

    let reaction = |path: &Path| reaction(&inventory, path);
    assert_eq!(reaction(&project(r"app\.vscode\mcp.json")), "ignores_file");
    assert_eq!(reaction(&project(r"app\.mcp.json")), "ignores_file");
    assert_eq!(reaction(&user_mcp), "ignores_file");
    assert_eq!(reaction(&plugin_mcp), "reads_around_error");
}

/// `fs` with the fixture plugin's `.mcp.json` holding `,,` between its two
/// servers.
fn with_broken_plugin_mcp(fs: FakeFileSystem) -> FakeFileSystem {
    fs.with_file(
        review_kit().join(".mcp.json"),
        "{\n  \"mcpServers\": {\n    \"linear\": { \"type\": \"http\", \"url\": \"https://mcp.linear.app/mcp\" },,\n    \"fetch\": { \"command\": \"uvx\" }\n  }\n}\n",
    )
}

#[test]
fn a_plugin_mcp_json_with_an_error_lists_the_servers_around_it_with_no_caveat() {
    let inventory = inventory(&with_broken_plugin_mcp(with_home(vscode())));

    let linear = item(&inventory, "linear");
    assert_eq!(json(linear.state), "effective");
    assert!(linear.caveats.is_empty(), "{:?}", linear.caveats);
    let fetch = item(&inventory, "fetch");
    assert_eq!(
        fetch.entries[1].locator,
        format!(r"{}\.mcp.json#mcpServers.fetch", review_kit().display())
    );
    assert!(fetch.caveats.is_empty(), "{:?}", fetch.caveats);

    let failures: Vec<_> = inventory
        .read_failures
        .iter()
        .filter(|failure| failure.agent == "vscode" && failure.home == "default")
        .collect();
    assert_eq!(failures.len(), 1, "{failures:?}");
    let failure = failures[0];
    assert_eq!(failure.path, review_kit().join(".mcp.json"));
    assert_eq!(code(failure.kind), "parse");
    assert!(
        !failure.sentence.contains("no Entries"),
        "{}",
        failure.sentence
    );
    assert_eq!(code(failure.reaction), "reads_around_error");
    let position = failure.position.as_ref().unwrap();
    assert_eq!((position.line, position.column), (3, 71));
    assert_eq!(
        failure.notes,
        ["VS Code reads what is around the error, and config-digger lists what it keeps."]
    );
}

// Four errors besides `,,`, in the texts `vscode.mjs` gives the real VS
// Code, each case asserting what it started or opened (#437).

/// The servers config-digger lists from the fixture plugin's `.mcp.json`
/// holding `text`, in the default profile.
fn plugin_servers(text: &str) -> Vec<String> {
    let fs = with_home(vscode()).with_file(review_kit().join(".mcp.json"), text);
    let inventory = inventory(&fs);
    let mut names: Vec<String> = inventory
        .items
        .iter()
        .filter(|item| {
            item.agent == "vscode"
                && item.home == "default"
                && item
                    .entries
                    .iter()
                    .any(|entry| entry.path == review_kit().join(".mcp.json"))
        })
        .map(|item| item.name.clone())
        .collect();
    names.sort();
    names
}

#[test]
fn a_plugin_mcp_json_missing_a_comma_between_servers_lists_them_all() {
    let text = r#"{ "mcpServers": {
  "cd-throwaway-plugin-a": {"command":"a"}
  "cd-throwaway-plugin-b": {"command":"b"},
  "cd-throwaway-plugin-c": {"command":"c"}
} }"#;

    assert_eq!(
        plugin_servers(text),
        [
            "cd-throwaway-plugin-a",
            "cd-throwaway-plugin-b",
            "cd-throwaway-plugin-c"
        ]
    );
}

#[test]
fn a_plugin_mcp_json_key_with_no_value_lists_the_servers_after_it() {
    let text = r#"{ "mcpServers": {
  "cd-throwaway-plugin-a": ,
  "cd-throwaway-plugin-b": {"command":"b"},
  "cd-throwaway-plugin-c": {"command":"c"}
} }"#;

    assert_eq!(
        plugin_servers(text),
        ["cd-throwaway-plugin-b", "cd-throwaway-plugin-c"]
    );
}

#[test]
fn a_plugin_mcp_json_left_open_at_its_end_lists_its_servers() {
    let text = r#"{ "mcpServers": {
  "cd-throwaway-plugin-a": {"command":"a"},
  "cd-throwaway-plugin-b": {"command":"b"}
"#;

    assert_eq!(
        plugin_servers(text),
        ["cd-throwaway-plugin-a", "cd-throwaway-plugin-b"]
    );
}

#[test]
fn a_string_left_open_in_a_plugin_s_server_takes_the_next_server_into_it() {
    // The string runs to the end of its line, swallowing the `}` and `,`,
    // so the next server is read as a member of `plugin-b`.
    let text = r#"{ "mcpServers": {
  "cd-throwaway-plugin-a": {"command":"a"},
  "cd-throwaway-plugin-b": {"command":"b", "note": "open },
  "cd-throwaway-plugin-c": {"command":"c"}
} }"#;

    assert_eq!(
        plugin_servers(text),
        ["cd-throwaway-plugin-a", "cd-throwaway-plugin-b"]
    );
}

/// What config-digger reads of a workspace file around its error.
#[derive(Debug, PartialEq)]
struct ReadAround {
    /// The member folders whose servers it lists.
    folders: Vec<&'static str>,
    /// Whether the file's `chat.mcp.discovery.enabled` turns Claude
    /// Desktop's servers on.
    discovery: bool,
    /// Whether it lists the file's own server.
    servers: bool,
}

/// What config-digger reads of the fixture workspace file holding `text`,
/// with Claude Desktop's config beside it, each case checked to be a Read
/// failure whose servers VS Code skips.
fn workspace_read_around(text: &str) -> ReadAround {
    let fs = with_claude_desktop(with_project(vscode(), "multi")).with_file(workspace(), text);
    let inventory = inventory_at(&fs, &workspace());
    assert_eq!(reaction(&inventory, &workspace()), "ignores_file");
    ReadAround {
        folders: [("api", "api-only"), ("web", "web-only")]
            .into_iter()
            .filter(|(_, server)| inventory.items.iter().any(|item| item.name == *server))
            .map(|(folder, _)| folder)
            .collect(),
        discovery: discovered(&inventory) == ("blocked".to_owned(), "not_approved".to_owned()),
        servers: inventory.items.iter().any(|item| item.name == "docs"),
    }
}

/// The fixture workspace's two member folders, as one member of its file.
const WORKSPACE_FOLDERS: &str = r#""folders": [{"path":"api"},{"path":"web"}]"#;
/// Settings turning on Claude Desktop's servers and adding the server
/// `docs`, as one member of a workspace file.
const WORKSPACE_SETTINGS: &str = r#""settings": {"chat.mcp.discovery.enabled":{"claude-desktop":true},"mcp":{"servers":{"docs":{"url":"https://docs.example.com/mcp"}}}}"#;

/// Both member folders read, discovery on, and the file's server skipped.
fn both_folders() -> ReadAround {
    ReadAround {
        folders: vec!["api", "web"],
        discovery: true,
        servers: false,
    }
}

#[test]
fn a_workspace_file_missing_a_comma_between_folders_and_settings_keeps_both() {
    let text = format!("{{\n  {WORKSPACE_FOLDERS}\n  {WORKSPACE_SETTINGS}\n}}");

    assert_eq!(workspace_read_around(&text), both_folders());
}

#[test]
fn a_workspace_file_key_with_no_value_between_folders_and_settings_keeps_both() {
    let text =
        format!("{{\n  {WORKSPACE_FOLDERS},\n  \"cd-throwaway-a\": ,\n  {WORKSPACE_SETTINGS}\n}}");

    assert_eq!(workspace_read_around(&text), both_folders());
}

#[test]
fn a_workspace_file_whose_settings_are_left_open_at_its_end_keeps_both() {
    let text = format!(
        r#"{{
  {WORKSPACE_FOLDERS},
  "settings": {{
    "chat.mcp.discovery.enabled": {{"claude-desktop":true}},
    "mcp": {{"servers":{{"docs":{{"url":"https://docs.example.com/mcp"}}}}}}
"#
    );

    assert_eq!(workspace_read_around(&text), both_folders());
}

#[test]
fn a_string_left_open_in_a_workspace_folder_drops_the_folder_after_it() {
    // The string swallows the rest of its line, so the object of the next
    // folder is skipped as a missing property name.
    let text = format!(
        r#"{{
  "folders": [
    {{ "path": "api", "name": "open }},
    {{ "path": "web" }}
  ],
  {WORKSPACE_SETTINGS}
}}"#
    );

    assert_eq!(
        workspace_read_around(&text),
        ReadAround {
            folders: vec!["api"],
            ..both_folders()
        }
    );
}

#[test]
fn vs_code_skips_the_servers_of_a_workspace_file_it_cannot_parse() {
    let file = workspace();
    let fs =
        with_project(vscode(), "multi").with_file(&file, r#"{ "folders": [],, "settings": {} }"#);
    let inventory = inventory_at(&fs, &file);

    assert_eq!(reaction(&inventory, &file), "ignores_file");
}

/// `fs` with the fixture workspace file holding `,,` after its `folders`,
/// and settings turning on Claude Desktop's servers and adding a server.
fn with_broken_workspace(fs: FakeFileSystem) -> FakeFileSystem {
    with_project(fs, "multi").with_file(
        workspace(),
        r#"{
	"folders": [
		{ "path": "api" },
		{ "path": "web" }
	],,
	"settings": {
		"chat.mcp.discovery.enabled": { "claude-desktop": true },
		"mcp": { "servers": { "docs": { "url": "https://docs.example.com/mcp" } } }
	}
}"#,
    )
}

#[test]
fn a_workspace_file_with_an_error_still_has_its_member_folders_but_not_its_servers() {
    let inventory = inventory_at(&with_broken_workspace(vscode()), &workspace());

    assert_eq!(json(item(&inventory, "api-only").state), "effective");
    assert_eq!(json(item(&inventory, "web-only").state), "effective");
    assert!(
        !inventory.items.iter().any(|item| item.name == "docs"),
        "the workspace file's own servers are skipped"
    );
    assert_eq!(reaction(&inventory, &workspace()), "ignores_file");
}

#[test]
fn a_workspace_file_with_an_error_still_turns_on_another_app_s_servers() {
    let fs = with_claude_desktop(with_broken_workspace(vscode()));
    let inventory = inventory_at(&fs, &workspace());

    // The setting turns them on: only the user's trust in each keeps it
    // from starting, as with the setting in the user's own settings.
    assert_eq!(
        discovered(&inventory),
        ("blocked".to_owned(), "not_approved".to_owned())
    );
}

#[test]
fn the_identity_provider_s_client_secret_in_a_workspace_file_never_shows() {
    let fs = with_project(vscode(), "multi");
    let core = core(&fs);
    let snapshot = core
        .scan(&ScanOptions {
            projects: vec![workspace()],
        })
        .unwrap();
    let context = Context {
        project: Some(workspace()),
    };
    let docs = format!(r"{PROJECTS}\multi\multi.code-workspace#settings.mcp.servers.docs");
    let plan = core
        .plan(
            &snapshot,
            &context,
            Action::Remove,
            &Targets {
                selectors: vec![docs],
                all: false,
                ..Targets::default()
            },
        )
        .unwrap();

    let preview = json(plan.preview()).to_string();
    assert!(preview.contains("clientSecret"), "{preview}");
    assert!(!preview.contains(SECRET), "{preview}");
}

#[test]
fn a_user_setup_of_vs_code_is_an_installation_of_every_home() {
    let code = Path::new(LOCAL_APP_DATA).join(r"Programs\Microsoft VS Code\Code.exe");
    let fs = vscode().with_file(&code, fake::executable(Some([1, 129, 1, 0])));
    let inventory = inventory(&fs);

    for home in inventory.homes.iter().filter(|home| home.agent == "vscode") {
        let versions: Vec<(&Path, Option<&str>)> = home
            .installations
            .iter()
            .map(|installation| (installation.path.as_path(), installation.version.as_deref()))
            .collect();
        assert_eq!(
            versions,
            [(code.as_path(), Some("1.129.1"))],
            "{}",
            home.home
        );
        assert!(!home.out_of_range);
    }
}

#[test]
fn every_profile_reads_the_workspace_storage_vs_code_keeps_once() {
    let fs = with_project(vscode(), "app");
    let inventory = inventory_at(&fs, &project("app"));

    let work = profile("-5a1b2c3d");
    let dart = item_in(&inventory, &work, "Dart SDK MCP Server");
    assert_eq!(dart.entries[0].path, PathBuf::from(app_storage()));
    assert_eq!(json(dart.state), "effective");
}

#[test]
fn a_client_secret_under_a_nested_mcp_setting_never_shows_either() {
    let fs = with_project(vscode(), "multi").with_file(
        workspace(),
        format!(
            r#"{{
	"folders": [{{ "path": "api" }}],
	"settings": {{
		"mcp": {{
			"enterpriseManagedAuth": {{ "idp": {{ "clientSecret": "{SECRET}" }} }},
			"servers": {{ "docs": {{ "url": "https://docs.example.com/mcp" }} }},
		}},
	}},
}}"#
        ),
    );
    let core = core(&fs);
    let snapshot = core
        .scan(&ScanOptions {
            projects: vec![workspace()],
        })
        .unwrap();
    let context = Context {
        project: Some(workspace()),
    };
    let docs = format!(r"{PROJECTS}\multi\multi.code-workspace#settings.mcp.servers.docs");
    let targets = Targets {
        selectors: vec![docs],
        all: false,
        ..Targets::default()
    };
    let plan = core
        .plan(&snapshot, &context, Action::Remove, &targets)
        .unwrap();

    let preview = json(plan.preview()).to_string();
    assert!(preview.contains("clientSecret"), "{preview}");
    assert!(!preview.contains(SECRET), "{preview}");
}
