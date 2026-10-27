//! Codex through the core `api`, with fake ports over fixture Codex homes
//! and fixture layers: the homes it reads, Orca's Mirrored home among them,
//! the `config.toml` layers from the machine's down to the Project's folder,
//! the trust its Project layers need, the servers it builds by merging
//! same-named Entries field by field, and the Projects it records.

use config_digger_core::api::{Context, Core, Entry, Inventory, Item, ScanOptions};
use config_digger_core::ports::fake::{self, FakeEnvironment, FakeFileSystem};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const HOME: &str = r"C:\Users\fixture";
const APP_DATA: &str = r"C:\Users\fixture\AppData\Roaming";
const PROGRAM_DATA: &str = r"C:\ProgramData";

/// The default Codex home, `~\.codex`.
const CODEX: &str = r"C:\Users\fixture\.codex";

/// The Codex home Orca keeps in sync with `~\.codex`.
const ORCA: &str = r"C:\Users\fixture\AppData\Roaming\orca\codex-runtime-home\home";

/// Where the fixture Projects go.
const PROJECTS: &str = r"E:\projects";

fn user_config() -> PathBuf {
    Path::new(CODEX).join("config.toml")
}

fn system_config() -> PathBuf {
    Path::new(PROGRAM_DATA).join(r"OpenAI\Codex\config.toml")
}

fn project(name: &str) -> PathBuf {
    Path::new(PROJECTS).join(name)
}

/// The `.codex\config.toml` layer of the folder `folder`.
fn layer(folder: &Path) -> PathBuf {
    folder.join(r".codex\config.toml")
}

fn env() -> FakeEnvironment {
    FakeEnvironment::with_home(HOME)
        .with_var("APPDATA", APP_DATA)
        .with_var("USERPROFILE", HOME)
        .with_var("ProgramData", PROGRAM_DATA)
}

fn core_with(fs: &FakeFileSystem, env: FakeEnvironment) -> Core {
    Core::new(fake::ports(fs.clone(), env)).unwrap()
}

fn inventory(fs: &FakeFileSystem) -> Inventory {
    inventory_with(fs, env())
}

fn inventory_with(fs: &FakeFileSystem, env: FakeEnvironment) -> Inventory {
    let core = core_with(fs, env);
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    core.resolve(&snapshot, &Context::default())
}

/// The inventory in the context of the Project at `path`, which the scan is
/// asked for whether or not a registry records it.
fn inventory_at(fs: &FakeFileSystem, path: &Path) -> Inventory {
    let core = core_with(fs, env());
    let options = ScanOptions {
        projects: vec![path.to_path_buf()],
    };
    let snapshot = core.scan(&options).unwrap();
    let context = Context {
        project: Some(path.to_path_buf()),
    };
    core.resolve(&snapshot, &context)
}

/// The Codex Item named `name` in the home `home`.
fn item_in<'i>(inventory: &'i Inventory, home: &str, name: &str) -> &'i Item {
    inventory
        .items
        .iter()
        .find(|item| item.agent == "codex" && item.home == home && item.name == name)
        .unwrap_or_else(|| {
            let names: Vec<(&str, &str)> = codex_items(inventory)
                .map(|item| (item.home.as_str(), item.name.as_str()))
                .collect();
            panic!("no Codex Item `{name}` in home `{home}` among {names:?}")
        })
}

/// The Codex Item named `name` in the default home.
fn item<'i>(inventory: &'i Inventory, name: &str) -> &'i Item {
    item_in(inventory, "default", name)
}

fn codex_items(inventory: &Inventory) -> impl Iterator<Item = &Item> {
    inventory.items.iter().filter(|item| item.agent == "codex")
}

fn json_of(value: impl serde::Serialize) -> Value {
    serde_json::to_value(value).unwrap()
}

fn entry_json(entry: &Entry) -> Value {
    json_of(entry)
}

fn code(value: impl serde::Serialize) -> String {
    json_of(value).as_str().unwrap().to_owned()
}

/// The States of an Item's Entries, highest first.
fn states(item: &Item) -> Vec<String> {
    item.entries.iter().map(|entry| code(entry.state)).collect()
}

#[test]
fn the_user_config_toml_s_servers_are_items() {
    let fs = FakeFileSystem::new().with_file(
        user_config(),
        "[mcp_servers.fetch]\ncommand = \"uvx\"\nargs = [\"mcp-server-fetch\"]\n",
    );

    let inventory = inventory(&fs);

    let fetch = item(&inventory, "fetch");
    assert_eq!(code(fetch.kind), "mcp");
    assert_eq!(code(fetch.state), "effective");
    let entry = entry_json(&fetch.entries[0]);
    assert_eq!(entry["path"], user_config().display().to_string());
    assert_eq!(entry["key_path"], json!(["mcp_servers", "fetch"]));
    assert_eq!(entry["scope"], "user");
    assert_eq!(entry["provenance"], "file");
    assert_eq!(entry["config"]["args"], json!(["mcp-server-fetch"]));
}

/// The Codex home with the id `id`.
fn home<'i>(inventory: &'i Inventory, id: &str) -> &'i config_digger_core::api::Home {
    inventory
        .homes
        .iter()
        .find(|home| home.agent == "codex" && home.home == id)
        .unwrap_or_else(|| {
            let ids: Vec<&str> = codex_homes(inventory)
                .map(|home| home.home.as_str())
                .collect();
            panic!("no Codex home `{id}` among {ids:?}")
        })
}

fn codex_homes(inventory: &Inventory) -> impl Iterator<Item = &config_digger_core::api::Home> {
    inventory.homes.iter().filter(|home| home.agent == "codex")
}

#[test]
fn codex_home_names_another_home_read_besides_the_default_one() {
    let other = r"D:\codex-home";
    let fs = FakeFileSystem::new()
        .with_file(user_config(), "[mcp_servers.fetch]\ncommand = \"uvx\"\n")
        .with_file(
            Path::new(other).join("config.toml"),
            "[mcp_servers.fetch]\ncommand = \"other\"\n",
        );

    let inventory = inventory_with(&fs, env().with_var("CODEX_HOME", other));

    assert_eq!(
        json_of(&home(&inventory, "default").source)["kind"],
        "default"
    );
    let env_home = home(&inventory, "codex-home");
    assert_eq!(
        json_of(&env_home.source),
        json!({ "kind": "environment", "variable": "CODEX_HOME" })
    );
    assert_eq!(env_home.folder, Path::new(other));
    // Entries in different homes never compete.
    assert_eq!(code(item(&inventory, "fetch").state), "effective");
    assert_eq!(
        code(item_in(&inventory, "codex-home", "fetch").state),
        "effective"
    );
}

#[test]
fn a_folder_codex_home_names_that_is_already_a_home_is_listed_once() {
    let fs =
        FakeFileSystem::new().with_file(user_config(), "[mcp_servers.fetch]\ncommand = \"uvx\"\n");

    let inventory = inventory_with(
        &fs,
        env().with_var("CODEX_HOME", r"C:\Users\fixture\.codex\"),
    );

    let ids: Vec<&str> = codex_homes(&inventory)
        .map(|home| home.home.as_str())
        .collect();
    assert_eq!(ids, ["default"]);
    assert_eq!(codex_items(&inventory).count(), 1);
}

#[test]
fn a_home_the_user_adds_where_codex_home_finds_it_keeps_the_variable_as_its_source() {
    let other = Path::new(r"D:\codex-home");
    let fs = FakeFileSystem::new().with_file(
        other.join("config.toml"),
        "[mcp_servers.fetch]\ncommand = \"uvx\"\n",
    );
    let core = core_with(&fs, env().with_var("CODEX_HOME", other.to_str().unwrap()));

    core.add_home(other, "codex").unwrap();
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    let inventory = core.resolve(&snapshot, &Context::default());

    let homes: Vec<_> = codex_homes(&inventory)
        .filter(|home| home.folder == other)
        .collect();
    assert_eq!(homes.len(), 1);
    assert_eq!(homes[0].home, "codex-home");
    assert_eq!(
        json_of(&homes[0].source),
        json!({ "kind": "environment", "variable": "CODEX_HOME" })
    );
}

/// Orca's Codex home, a copy of the user's `config.toml` next to links to
/// `~\.codex\plugins` and `~\.codex\skills`.
fn with_orca(fs: FakeFileSystem) -> FakeFileSystem {
    fs.with_file(
        Path::new(ORCA).join("config.toml"),
        "[mcp_servers.fetch]\ncommand = \"uvx\"\n",
    )
    .with_link(
        Path::new(ORCA).join("skills"),
        Path::new(CODEX).join("skills"),
    )
    .with_link(
        Path::new(ORCA).join("plugins"),
        Path::new(CODEX).join("plugins"),
    )
}

#[test]
fn orca_s_codex_home_is_a_read_only_mirrored_home_of_the_default_one() {
    let fs = with_orca(
        FakeFileSystem::new().with_file(user_config(), "[mcp_servers.fetch]\ncommand = \"uvx\"\n"),
    );

    let inventory = inventory(&fs);

    let orca = home(&inventory, "orca");
    assert_eq!(orca.folder, Path::new(ORCA));
    assert_eq!(json_of(&orca.source)["kind"], "known_location");
    let mirror = json_of(&orca.mirror);
    assert_eq!(mirror["owner"], "Orca");
    assert_eq!(mirror["label"], "Mirrored by Orca");
    assert_eq!(mirror["source"], "default");
    assert_eq!(mirror["source_folder"], CODEX);
    assert!(home(&inventory, "default").mirror.is_none());

    let mirrored = item_in(&inventory, "orca", "fetch");
    assert_eq!(code(mirrored.state), "effective");
    let entry = entry_json(&mirrored.entries[0]);
    assert_eq!(
        entry["path"],
        Path::new(ORCA).join("config.toml").display().to_string()
    );
    // Its actions are those of the Entry it was copied from, which they go to.
    assert_eq!(
        entry["copied_from"],
        format!("{}#mcp_servers.fetch", user_config().display())
    );
    assert_eq!(
        entry["disable"]["path"],
        user_config().display().to_string()
    );
}

#[test]
fn orca_s_codex_home_named_by_codex_home_is_still_its_mirrored_home() {
    let fs = with_orca(
        FakeFileSystem::new().with_file(user_config(), "[mcp_servers.fetch]\ncommand = \"uvx\"\n"),
    );

    let inventory = inventory_with(&fs, env().with_var("CODEX_HOME", ORCA));

    let ids: Vec<&str> = codex_homes(&inventory)
        .map(|home| home.home.as_str())
        .collect();
    assert_eq!(ids, ["default", "orca"]);
    assert!(home(&inventory, "orca").mirror.is_some());
}

/// A `SKILL.md` naming its skill `name`.
fn skill_md(name: &str) -> String {
    format!("---\nname: {name}\ndescription: Does {name}.\n---\nBody.\n")
}

#[test]
fn the_mirrored_home_s_linked_skills_are_the_same_entries_by_real_path() {
    let fs = with_orca(
        FakeFileSystem::new()
            .with_file(user_config(), "")
            .with_file(
                Path::new(CODEX).join(r"skills\review\SKILL.md"),
                skill_md("review"),
            ),
    );

    let inventory = inventory(&fs);

    let real = Path::new(CODEX).join(r"skills\review");
    let own = item(&inventory, "review");
    let mirrored = item_in(&inventory, "orca", "review");
    assert_eq!(own.entries[0].path, real);
    assert_eq!(mirrored.entries[0].path, real);
    assert_eq!(code(mirrored.kind), "skill");
    assert_eq!(code(mirrored.state), "effective");
}

#[test]
fn same_named_skills_never_compete() {
    let fs = FakeFileSystem::new()
        .with_file(
            Path::new(CODEX).join(r"skills\review\SKILL.md"),
            skill_md("review"),
        )
        .with_file(
            Path::new(CODEX).join(r"skills\team\review\SKILL.md"),
            skill_md("review"),
        );

    let inventory = inventory(&fs);

    let reviews: Vec<&Item> = codex_items(&inventory)
        .filter(|item| item.name == "review")
        .collect();
    assert_eq!(reviews.len(), 2);
    assert!(reviews.iter().all(|item| code(item.state) == "effective"));
}

/// The locator of the server `name` in the layer `path`.
fn server(path: &Path, name: &str) -> String {
    format!("{}#mcp_servers.{name}", path.display())
}

/// The locators of an Item's Entries, highest first.
fn locators(item: &Item) -> Vec<String> {
    item.entries
        .iter()
        .map(|entry| entry.locator.clone())
        .collect()
}

/// Where each field of the server an Item builds comes from, by its key
/// path joined with `.`.
fn origins(item: &Item) -> Vec<(String, String)> {
    let built = item.built.as_ref().expect("the Item builds a server");
    built
        .origins
        .iter()
        .map(|origin| (origin.key_path.join("."), origin.entry.clone()))
        .collect()
}

#[test]
fn same_named_servers_of_the_system_and_user_layers_merge_field_by_field() {
    let fs = FakeFileSystem::new()
        .with_file(
            system_config(),
            "[mcp_servers.docs]\ncommand = \"docs-mcp\"\nargs = [\"--system\"]\nstartup_timeout_sec = 20\n",
        )
        .with_file(user_config(), "[mcp_servers.docs]\nargs = [\"--user\"]\n");

    let inventory = inventory(&fs);

    let docs = item(&inventory, "docs");
    assert_eq!(code(docs.state), "merged");
    assert_eq!(
        locators(docs),
        [
            server(&user_config(), "docs"),
            server(&system_config(), "docs")
        ]
    );
    assert_eq!(states(docs), ["merged", "merged"]);
    let system = entry_json(&docs.entries[1]);
    assert_eq!(system["scope"], "managed");
    assert_eq!(system["provenance"], "managed");
    let reason = &docs.entries[0].reason.as_ref().unwrap();
    assert_eq!(code(reason.code), "merged");
    assert!(reason.sentence.contains("`args`"), "{}", reason.sentence);

    let built = docs.built.as_ref().unwrap();
    assert_eq!(
        built.config,
        json!({ "command": "docs-mcp", "args": ["--user"], "startup_timeout_sec": 20 })
    );
    let user = server(&user_config(), "docs");
    let system = server(&system_config(), "docs");
    assert_eq!(
        origins(docs),
        [
            ("args".to_owned(), user),
            ("command".to_owned(), system.clone()),
            ("startup_timeout_sec".to_owned(), system),
        ]
    );
}

#[test]
fn a_server_of_one_layer_alone_is_effective_and_builds_nothing_apart() {
    let fs = FakeFileSystem::new()
        .with_file(
            system_config(),
            "[mcp_servers.docs]\ncommand = \"docs-mcp\"\n",
        )
        .with_file(user_config(), "[mcp_servers.fetch]\ncommand = \"uvx\"\n");

    let inventory = inventory(&fs);

    assert_eq!(code(item(&inventory, "docs").state), "effective");
    assert!(item(&inventory, "docs").built.is_none());
    assert_eq!(code(item(&inventory, "fetch").state), "effective");
}

#[test]
fn managed_config_toml_is_not_read_on_windows() {
    let fs = FakeFileSystem::new()
        .with_file(user_config(), "")
        .with_file(
            Path::new(CODEX).join("managed_config.toml"),
            "[mcp_servers.legacy]\ncommand = \"legacy\"\n",
        );

    let inventory = inventory(&fs);

    assert!(codex_items(&inventory).all(|item| item.name != "legacy"));
}

/// A git repository at `E:\projects\mono`, trusted in the user's
/// `config.toml` when `trusted`, holding the Project `apps\web`, each with a
/// `.codex\config.toml` patching the user's `docs` server.
fn monorepo(trusted: bool) -> FakeFileSystem {
    let trust = if trusted {
        "\n[projects.'e:\\projects\\mono']\ntrust_level = \"trusted\"\n"
    } else {
        ""
    };
    FakeFileSystem::new()
        .with_folder(project(r"mono\.git"))
        .with_file(
            user_config(),
            format!("[mcp_servers.docs]\ncommand = \"docs-mcp\"\nargs = [\"--user\"]\n{trust}"),
        )
        .with_file(
            layer(&project("mono")),
            "[mcp_servers.docs]\nargs = [\"--root\"]\ncwd = \"E:/projects/mono\"\n",
        )
        .with_file(
            layer(&project(r"mono\apps\web")),
            "[mcp_servers.docs]\nargs = [\"--web\"]\n",
        )
}

#[test]
fn project_layers_are_read_from_the_repo_root_down_to_the_project_the_deepest_winning() {
    let fs = monorepo(true);

    let inventory = inventory_at(&fs, &project(r"mono\apps\web"));

    let docs = item(&inventory, "docs");
    assert_eq!(
        locators(docs),
        [
            server(&layer(&project(r"mono\apps\web")), "docs"),
            server(&layer(&project("mono")), "docs"),
            server(&user_config(), "docs"),
        ]
    );
    assert_eq!(states(docs), ["merged", "merged", "merged"]);
    assert_eq!(code(docs.state), "merged");
    assert_eq!(entry_json(&docs.entries[0])["scope"], "project");
    assert_eq!(
        docs.built.as_ref().unwrap().config,
        json!({ "command": "docs-mcp", "args": ["--web"], "cwd": "E:/projects/mono" })
    );
    // An Entry whose every field comes from a higher one still takes part.
    let user = docs.entries[2].reason.as_ref().unwrap();
    assert_eq!(code(user.code), "merged");
}

#[test]
fn project_layers_of_an_untrusted_project_are_blocked() {
    let fs = monorepo(false);

    let inventory = inventory_at(&fs, &project(r"mono\apps\web"));

    let docs = item(&inventory, "docs");
    assert_eq!(states(docs), ["blocked", "blocked", "effective"]);
    assert_eq!(code(docs.state), "effective");
    let reason = docs.entries[0].reason.as_ref().unwrap();
    assert_eq!(code(reason.code), "untrusted_folder");
    assert!(
        reason.sentence.contains(r"E:\projects\mono"),
        "{}",
        reason.sentence
    );
    // Blocked Entries take no part in the server Codex builds.
    assert!(docs.built.is_none());
    let what_if = docs.entries[0].what_if.as_ref().unwrap();
    assert_eq!(code(what_if.state), "merged");
}

#[test]
fn a_folder_s_own_trust_level_comes_before_its_project_root_s() {
    let fs = monorepo(true).with_file(
        user_config(),
        "[mcp_servers.docs]\ncommand = \"docs-mcp\"\n\n[projects.'e:\\projects\\mono']\ntrust_level = \"trusted\"\n\n[projects.'e:\\projects\\mono\\apps\\web']\ntrust_level = \"untrusted\"\n",
    );

    let inventory = inventory_at(&fs, &project(r"mono\apps\web"));

    let docs = item(&inventory, "docs");
    assert_eq!(states(docs), ["blocked", "merged", "merged"]);
}

#[test]
fn a_linked_worktree_is_trusted_through_its_main_checkout() {
    let worktree = Path::new(r"E:\worktrees\feature");
    let fs = FakeFileSystem::new()
        .with_folder(project(r"mono\.git\worktrees\feature"))
        .with_file(
            worktree.join(".git"),
            "gitdir: E:/projects/mono/.git/worktrees/feature\n",
        )
        .with_file(
            user_config(),
            "[mcp_servers.docs]\ncommand = \"docs-mcp\"\n\n[projects.'e:\\projects\\mono']\ntrust_level = \"trusted\"\n",
        )
        .with_file(layer(worktree), "[mcp_servers.docs]\nargs = [\"--feature\"]\n");

    let inventory = inventory_at(&fs, worktree);

    assert_eq!(states(item(&inventory, "docs")), ["merged", "merged"]);
}

#[test]
fn a_field_only_entry_that_turns_the_server_off_disables_every_entry_of_the_item() {
    let fs = monorepo(true).with_file(
        layer(&project(r"mono\apps\web")),
        "[mcp_servers.docs]\nenabled = false\n",
    );

    let inventory = inventory_at(&fs, &project(r"mono\apps\web"));

    let docs = item(&inventory, "docs");
    assert_eq!(code(docs.state), "disabled");
    assert_eq!(states(docs), ["disabled", "disabled", "disabled"]);
    let field_only = entry_json(&docs.entries[0]);
    assert_eq!(field_only["config"], json!({ "enabled": false }));
    let switch = format!(
        "{}#mcp_servers.docs.enabled",
        layer(&project(r"mono\apps\web")).display()
    );
    for entry in &docs.entries {
        let reason = entry.reason.as_ref().unwrap();
        assert_eq!(code(reason.code), "disabled_natively");
        assert_eq!(reason.switch.as_deref(), Some(switch.as_str()));
        assert_eq!(code(entry.what_if.as_ref().unwrap().state), "merged");
    }
    assert_eq!(
        entry_json(&docs.entries[2])["disable"]["code"],
        "already_disabled"
    );
}

#[test]
fn a_higher_layer_that_turns_the_server_on_again_wins_over_a_lower_one() {
    let fs = FakeFileSystem::new()
        .with_file(
            system_config(),
            "[mcp_servers.docs]\ncommand = \"docs-mcp\"\nenabled = false\n",
        )
        .with_file(user_config(), "[mcp_servers.docs]\nenabled = true\n");

    let inventory = inventory(&fs);

    assert_eq!(states(item(&inventory, "docs")), ["merged", "merged"]);
}

#[test]
fn layers_that_combine_command_and_url_make_every_entry_invalid_and_codex_unable_to_start() {
    let fs = monorepo(true).with_file(
        layer(&project(r"mono\apps\web")),
        "[mcp_servers.docs]\nurl = \"https://docs.example/mcp\"\n",
    );

    let in_project = inventory_at(&fs, &project(r"mono\apps\web"));

    let docs = item(&in_project, "docs");
    assert_eq!(code(docs.state), "invalid");
    assert_eq!(states(docs), ["invalid", "invalid", "invalid"]);
    let reason = docs.entries[2].reason.as_ref().unwrap();
    assert_eq!(code(reason.code), "transport_conflict");
    assert!(reason.sentence.contains("`command`"), "{}", reason.sentence);
    let flag = in_project
        .agent_flags
        .iter()
        .find(|flag| flag.agent == "codex")
        .expect("Codex is flagged");
    assert_eq!(code(flag.code), "cannot_start");
    assert!(flag.sentence.contains("docs"), "{}", flag.sentence);

    // Outside the Project, the user's server alone is fine.
    let outside = inventory(&fs);
    assert_eq!(code(item(&outside, "docs").state), "effective");
    assert!(outside.agent_flags.iter().all(|flag| flag.agent != "codex"));
}

#[test]
fn a_config_toml_codex_cannot_parse_stops_it() {
    let fs = FakeFileSystem::new().with_file(user_config(), "[mcp_servers.docs\ncommand = 1\n");

    let inventory = inventory(&fs);

    let failure = inventory
        .read_failures
        .iter()
        .find(|failure| failure.agent == "codex")
        .unwrap();
    assert_eq!(failure.path, user_config());
    assert_eq!(code(failure.reaction), "stops_agent");
    assert!(
        inventory
            .agent_flags
            .iter()
            .any(|flag| flag.agent == "codex" && code(flag.code) == "cannot_start")
    );
}

/// The first line Codex writes in a session's rollout file, naming the
/// folder it started in.
fn session_meta(cwd: &str) -> String {
    let meta = json!({
        "timestamp": "2026-10-01T10:00:00.000Z",
        "type": "session_meta",
        "payload": { "id": "0199", "cwd": cwd, "originator": "codex_cli_rs" },
    });
    format!("{meta}\n{}\n", json!({ "type": "response_item" }))
}

#[test]
fn projects_come_from_trust_entries_sessions_and_the_state_database() {
    let fs = FakeFileSystem::new()
        .with_file(
            user_config(),
            "[projects.'e:\\projects\\trusted']\ntrust_level = \"trusted\"\n",
        )
        .with_file(
            Path::new(CODEX).join(r"sessions\2026\10\01\rollout-2026-10-01T10-00-00-0199.jsonl"),
            session_meta(r"E:\projects\session"),
        )
        .with_file(
            Path::new(CODEX).join(r"archived_sessions\rollout-2026-09-01T10-00-00-0188.jsonl"),
            session_meta(r"E:\projects\archived"),
        )
        // Not a rollout file, nor one whose first line names a folder.
        .with_file(
            Path::new(CODEX).join(r"sessions\2026\10\01\notes.jsonl"),
            session_meta(r"E:\projects\not-a-session"),
        )
        .with_file(
            Path::new(CODEX).join(r"sessions\2026\10\02\rollout-2026-10-02T10-00-00-0200.jsonl"),
            "not json\n",
        )
        .with_sqlite_rows(
            Path::new(CODEX).join("state_5.sqlite"),
            "threads",
            "cwd",
            &[r"E:\projects\thread"],
        )
        .with_sqlite_rows(
            Path::new(CODEX).join("state_5.sqlite"),
            "project_roots",
            "path",
            &[r"E:\projects\named"],
        );

    let inventory = inventory(&fs);

    let registered: Vec<String> = inventory
        .projects
        .iter()
        .filter(|project| project.registered_by.iter().any(|agent| agent == "codex"))
        .map(|project| project.path.display().to_string())
        .collect();
    assert_eq!(
        registered,
        [
            r"E:\projects\archived",
            r"E:\projects\named",
            r"E:\projects\session",
            r"E:\projects\thread",
            r"E:\projects\trusted",
        ]
    );
}

#[test]
fn a_state_database_without_the_tables_records_no_projects() {
    let fs = FakeFileSystem::new()
        .with_file(user_config(), "")
        .with_sqlite_item(Path::new(CODEX).join("state_5.sqlite"), "unrelated", "{}");

    let inventory = inventory(&fs);

    assert!(inventory.projects.is_empty());
    assert!(inventory.read_failures.is_empty());
}

/// A remote server holding the fake Secret `not-a-real-secret` in every
/// field Codex can hold a Secret in, next to fields that name variables.
const REMOTE: &str = r#"[mcp_servers.remote]
url = "https://mcp.example/mcp"
bearer_token_env_var = "CD_FAKE_BEARER"
http_headers_helper = "print-headers --token not-a-real-secret"

[mcp_servers.remote.http_headers]
Authorization = "Bearer not-a-real-secret"
X-Team = "not-a-real-secret"

[mcp_servers.remote.env_http_headers]
X-Api-Key = "CD_FAKE_KEY"

[mcp_servers.remote.oauth]
client_id = "public-client"
client_secret = "not-a-real-secret"
"#;

#[test]
fn the_secret_fields_of_a_codex_server_are_masked_and_variable_names_shown() {
    let fs = FakeFileSystem::new().with_file(user_config(), REMOTE);

    let inventory = inventory(&fs);

    assert!(
        !json_of(&inventory)
            .to_string()
            .contains("not-a-real-secret")
    );
    let entry = &item(&inventory, "remote").entries[0];
    assert_eq!(
        entry.config,
        json!({
            "url": "https://mcp.example/mcp",
            "bearer_token_env_var": "CD_FAKE_BEARER",
            "http_headers_helper": "••••",
            "http_headers": { "Authorization": "Bearer ••••", "X-Team": "••••" },
            "env_http_headers": { "X-Api-Key": "CD_FAKE_KEY" },
            "oauth": { "client_id": "public-client", "client_secret": "••••" },
        })
    );
    let mut masked: Vec<String> = entry.masked.iter().map(|path| path.join(".")).collect();
    masked.sort();
    assert_eq!(
        masked,
        [
            "http_headers.Authorization",
            "http_headers.X-Team",
            "http_headers_helper",
            "oauth.client_secret",
        ]
    );
}

#[test]
fn the_server_codex_builds_keeps_the_secrets_of_its_entries_masked() {
    let fs = FakeFileSystem::new()
        .with_file(user_config(), REMOTE)
        .with_file(
            system_config(),
            "[mcp_servers.remote]\nstartup_timeout_sec = 5\n",
        );

    let inventory = inventory(&fs);

    assert!(
        !json_of(&inventory)
            .to_string()
            .contains("not-a-real-secret")
    );
    let built = item(&inventory, "remote").built.as_ref().unwrap();
    assert_eq!(built.config["http_headers"]["Authorization"], "Bearer ••••");
    assert_eq!(built.config["startup_timeout_sec"], 5);
    assert!(
        built
            .masked
            .contains(&vec!["http_headers".to_owned(), "Authorization".to_owned()])
    );
}

// Enable lifts the user's `enabled = false` from the user's Entry, as
// Disable writes it, never from one config-digger only reads.
#[test]
fn enable_of_a_managed_entry_a_merged_field_turns_off_is_refused_with_why() {
    use config_digger_core::api::{Action, Targets};
    let fs = FakeFileSystem::new()
        .with_file(
            system_config(),
            "[mcp_servers.docs]\ncommand = \"docs-mcp\"\n",
        )
        .with_file(user_config(), "[mcp_servers.docs]\nenabled = false\n");
    let core = core_with(&fs, env());
    let snapshot = core.scan(&ScanOptions::default()).unwrap();

    let Err(error) = core.plan(
        &snapshot,
        &Context::default(),
        Action::Enable,
        &Targets {
            selectors: vec![server(&system_config(), "docs")],
            all: false,
        },
    ) else {
        panic!("an Enable of a managed Entry is planned");
    };

    assert!(error.message.contains("only reads"), "{}", error.message);
}

#[test]
fn the_codex_home_s_own_config_toml_is_no_project_layer() {
    let fs = FakeFileSystem::new()
        .with_folder(Path::new(HOME).join(".git"))
        .with_file(user_config(), "[mcp_servers.fetch]\ncommand = \"uvx\"\n");

    for project in [Path::new(HOME).to_path_buf(), Path::new(HOME).join("notes")] {
        let inventory = inventory_at(&fs, &project);

        let fetch = item(&inventory, "fetch");
        assert_eq!(locators(fetch), [server(&user_config(), "fetch")]);
        assert_eq!(code(fetch.state), "effective");
        assert_eq!(entry_json(&fetch.entries[0])["scope"], "user");
    }
}
