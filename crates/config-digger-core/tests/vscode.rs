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

    // Claude Code names the same folder by its frontmatter.
    assert!(
        inventory
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
    let fs = with_project(with_home(vscode()), "shared");
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
        .find(|item| item.agent == "claude-code" && item.name == "code-review")
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

/// The fixture agent plugin's folder.
fn review_kit() -> PathBuf {
    Path::new(HOME).join(r".vscode\agent-plugins\review-kit")
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
        ("review-kit", "review-kit")
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
    assert!(!json(&inventory).to_string().contains(SECRET));
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
    };
    let plan = core
        .plan(&snapshot, &context, Action::Remove, &targets)
        .unwrap();

    let preview = json(plan.preview()).to_string();
    assert!(preview.contains("clientSecret"), "{preview}");
    assert!(!preview.contains(SECRET), "{preview}");
}
