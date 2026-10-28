//! Gemini CLI through the core `api`, with fake ports over a fixture home
//! folder: the four `settings.json` Scopes and the variables that move
//! them, the extensions and their servers that a settings Entry of the same
//! name merges with, folder trust, skills, admin controls, and the Projects
//! Gemini CLI records.

use config_digger_core::api::{Context, Core, Entry, Inventory, Item, ScanOptions};
use config_digger_core::ports::fake::{self, FakeEnvironment, FakeFileSystem};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const HOME: &str = r"C:\Users\fixture";
const APP_DATA: &str = r"C:\Users\fixture\AppData\Roaming";
const PROGRAM_DATA: &str = r"C:\ProgramData";

/// Where the fixture Projects go.
const PROJECTS: &str = r"E:\projects";

/// `~\.gemini`.
fn gemini() -> PathBuf {
    Path::new(HOME).join(".gemini")
}

fn user_settings() -> PathBuf {
    gemini().join("settings.json")
}

fn project(name: &str) -> PathBuf {
    Path::new(PROJECTS).join(name)
}

/// The `.gemini\settings.json` of the Project folder `folder`.
fn project_settings(folder: &Path) -> PathBuf {
    folder.join(r".gemini\settings.json")
}

fn trusted_folders() -> PathBuf {
    gemini().join("trustedFolders.json")
}

/// The key Gemini CLI gives `folder` in `trustedFolders.json`: lower case,
/// with forward slashes.
fn trust_key(folder: &Path) -> String {
    folder
        .display()
        .to_string()
        .to_lowercase()
        .replace('\\', "/")
}

/// `fs` with `trustedFolders.json` trusting `folder`, as Gemini CLI writes it.
fn trusting(fs: FakeFileSystem, folder: &Path) -> FakeFileSystem {
    fs.with_file(
        trusted_folders(),
        json!({ trust_key(folder): "TRUST_FOLDER" }).to_string(),
    )
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

/// The States of an Item's Entries, highest first.
fn states(item: &Item) -> Vec<String> {
    item.entries.iter().map(|entry| code(entry.state)).collect()
}

/// The Gemini CLI Item named `name` in the home `home`, of the kind `kind`.
fn item_of<'i>(inventory: &'i Inventory, home: &str, kind: &str, name: &str) -> &'i Item {
    inventory
        .items
        .iter()
        .find(|item| {
            item.agent == "gemini-cli"
                && item.home == home
                && code(item.kind) == kind
                && item.name == name
        })
        .unwrap_or_else(|| {
            let names: Vec<(&str, &str)> = gemini_items(inventory)
                .map(|item| (item.home.as_str(), item.name.as_str()))
                .collect();
            panic!("no Gemini CLI {kind} Item `{name}` in home `{home}` among {names:?}")
        })
}

/// The Gemini CLI server named `name` in the default home.
fn server<'i>(inventory: &'i Inventory, name: &str) -> &'i Item {
    item_of(inventory, "default", "mcp", name)
}

fn gemini_items(inventory: &Inventory) -> impl Iterator<Item = &Item> {
    inventory
        .items
        .iter()
        .filter(|item| item.agent == "gemini-cli")
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

#[test]
fn the_user_settings_json_s_servers_are_items() {
    let fs = FakeFileSystem::new().with_file(
        user_settings(),
        "{\n  // Comments are allowed.\n  \"mcpServers\": {\n    \"fetch\": { \"command\": \"uvx\", \"args\": [\"mcp-server-fetch\"] }\n  }\n}\n",
    );

    let inventory = inventory(&fs);

    let fetch = server(&inventory, "fetch");
    assert_eq!(code(fetch.state), "effective");
    let entry = entry_json(&fetch.entries[0]);
    assert_eq!(entry["path"], user_settings().display().to_string());
    assert_eq!(entry["key_path"], json!(["mcpServers", "fetch"]));
    assert_eq!(entry["scope"], "user");
    assert_eq!(entry["provenance"], "file");
    assert_eq!(entry["config"]["args"], json!(["mcp-server-fetch"]));
}

#[test]
fn a_trailing_comma_in_settings_json_is_a_read_failure_that_stops_gemini_cli() {
    let fs = FakeFileSystem::new().with_file(
        user_settings(),
        "{ \"mcpServers\": { \"fetch\": { \"command\": \"uvx\" }, } }",
    );

    let inventory = inventory(&fs);

    let failure = inventory
        .read_failures
        .iter()
        .find(|failure| failure.agent == "gemini-cli")
        .expect("Gemini CLI cannot read the file");
    assert_eq!(failure.path, user_settings());
    assert_eq!(code(failure.kind), "parse");
    assert_eq!(code(failure.reaction), "stops_agent");
    assert!(
        inventory
            .agent_flags
            .iter()
            .any(|flag| flag.agent == "gemini-cli" && code(flag.code) == "cannot_start")
    );
    assert_eq!(gemini_items(&inventory).count(), 0);
}

#[test]
fn a_project_settings_entry_shadows_the_user_s_whole() {
    let folder = project("app");
    let fs = trusting(
        FakeFileSystem::new()
            .with_file(
                user_settings(),
                r#"{ "mcpServers": { "docs": { "command": "user-docs", "timeout": 5 } } }"#,
            )
            .with_file(
                project_settings(&folder),
                r#"{ "mcpServers": { "docs": { "command": "project-docs" } } }"#,
            ),
        &folder,
    );

    let inventory = inventory_at(&fs, &folder);

    let docs = server(&inventory, "docs");
    assert_eq!(states(docs), ["effective", "shadowed"]);
    let project_entry = entry_json(&docs.entries[0]);
    assert_eq!(project_entry["scope"], "project");
    assert_eq!(
        project_entry["path"],
        project_settings(&folder).display().to_string()
    );
    // Settings files are not merged field by field.
    assert!(docs.built.is_none());
}

#[test]
fn only_the_project_folder_s_own_settings_are_read_not_its_parents() {
    let folder = project(r"repo\app");
    let fs = trusting(
        FakeFileSystem::new()
            .with_folder(project("repo").join(".git"))
            .with_file(
                project_settings(&project("repo")),
                r#"{ "mcpServers": { "parent": { "command": "x" } } }"#,
            )
            .with_file(
                user_settings(),
                r#"{ "mcpServers": { "fetch": { "command": "uvx" } } }"#,
            )
            .with_folder(&folder),
        &folder,
    );

    let inventory = inventory_at(&fs, &folder);

    assert!(gemini_items(&inventory).all(|item| item.name != "parent"));
}

#[test]
fn the_user_s_own_settings_are_no_project_layer_in_the_home_folder() {
    let fs = trusting(
        FakeFileSystem::new().with_file(
            user_settings(),
            r#"{ "mcpServers": { "fetch": { "command": "uvx" } } }"#,
        ),
        Path::new(HOME),
    );

    let inventory = inventory_at(&fs, Path::new(HOME));

    let fetch = server(&inventory, "fetch");
    assert_eq!(states(fetch), ["effective"]);
    assert_eq!(entry_json(&fetch.entries[0])["scope"], "user");
}

fn system_settings() -> PathBuf {
    Path::new(PROGRAM_DATA).join(r"gemini-cli\settings.json")
}

fn system_defaults() -> PathBuf {
    Path::new(PROGRAM_DATA).join(r"gemini-cli\system-defaults.json")
}

/// A settings file declaring the server `docs` with the command `command`.
fn docs(command: &str) -> String {
    json!({ "mcpServers": { "docs": { "command": command } } }).to_string()
}

#[test]
fn system_override_beats_the_project_which_beats_the_user_who_beats_system_defaults() {
    let folder = project("app");
    let fs = trusting(
        FakeFileSystem::new()
            .with_file(system_defaults(), docs("defaults"))
            .with_file(user_settings(), docs("user"))
            .with_file(project_settings(&folder), docs("project"))
            .with_file(system_settings(), docs("system")),
        &folder,
    );

    let inventory = inventory_at(&fs, &folder);

    let docs = server(&inventory, "docs");
    let scopes: Vec<Value> = docs
        .entries
        .iter()
        .map(|entry| entry_json(entry)["scope"].clone())
        .collect();
    assert_eq!(scopes, ["managed", "project", "user", "managed"]);
    let paths: Vec<PathBuf> = docs
        .entries
        .iter()
        .map(|entry| entry.path.clone())
        .collect();
    assert_eq!(
        paths,
        [
            system_settings(),
            project_settings(&folder),
            user_settings(),
            system_defaults()
        ]
    );
    assert_eq!(
        states(docs),
        ["effective", "shadowed", "shadowed", "shadowed"]
    );
    assert_eq!(entry_json(&docs.entries[0])["provenance"], "managed");
}

#[test]
fn gemini_cli_system_settings_path_moves_the_system_file_and_the_defaults_beside_it() {
    let moved = Path::new(r"D:\org\gemini");
    let fs = FakeFileSystem::new()
        .with_file(user_settings(), docs("user"))
        .with_file(system_settings(), docs("not read"))
        .with_file(moved.join("override.json"), docs("system"))
        .with_file(moved.join("system-defaults.json"), docs("defaults"));

    let inventory = inventory_with(
        &fs,
        env().with_var(
            "GEMINI_CLI_SYSTEM_SETTINGS_PATH",
            r"D:\org\gemini\override.json",
        ),
    );

    let paths: Vec<PathBuf> = server(&inventory, "docs")
        .entries
        .iter()
        .map(|entry| entry.path.clone())
        .collect();
    assert_eq!(
        paths,
        [
            moved.join("override.json"),
            user_settings(),
            moved.join("system-defaults.json")
        ]
    );
}

#[test]
fn gemini_cli_system_defaults_path_moves_the_defaults_alone() {
    let fs = FakeFileSystem::new()
        .with_file(user_settings(), docs("user"))
        .with_file(system_defaults(), docs("not read"))
        .with_file(r"D:\defaults.json", docs("defaults"));

    let inventory = inventory_with(
        &fs,
        env().with_var("GEMINI_CLI_SYSTEM_DEFAULTS_PATH", r"D:\defaults.json"),
    );

    let paths: Vec<PathBuf> = server(&inventory, "docs")
        .entries
        .iter()
        .map(|entry| entry.path.clone())
        .collect();
    assert_eq!(paths, [user_settings(), PathBuf::from(r"D:\defaults.json")]);
}

#[test]
fn a_system_file_no_administrator_owns_is_unread() {
    let fs = FakeFileSystem::new()
        .with_file(user_settings(), docs("user"))
        .with_file(system_settings(), docs("system"))
        .writable_by_others(system_settings());

    let inventory = inventory(&fs);

    let docs = server(&inventory, "docs");
    let system = docs
        .entries
        .iter()
        .find(|entry| entry.path == system_settings())
        .unwrap();
    assert_eq!(code(system.state), "unread");
    let reason = system.reason.as_ref().unwrap();
    assert_eq!(code(reason.code), "system_file_not_admin_owned");
    assert!(
        reason.sentence.contains("administrator"),
        "{}",
        reason.sentence
    );
    let user = docs
        .entries
        .iter()
        .find(|entry| entry.path == user_settings())
        .unwrap();
    assert_eq!(code(user.state), "effective");
}

#[test]
fn a_folder_above_a_system_file_that_others_can_write_to_makes_it_unread() {
    let fs = FakeFileSystem::new()
        .with_file(user_settings(), "{}")
        .with_file(system_defaults(), docs("defaults"))
        .writable_by_others(Path::new(PROGRAM_DATA).join("gemini-cli"));

    let inventory = inventory(&fs);

    assert_eq!(code(server(&inventory, "docs").state), "unread");
}

#[test]
fn a_system_file_gemini_cli_skips_is_no_read_failure() {
    let fs = FakeFileSystem::new()
        .with_file(user_settings(), docs("user"))
        .with_file(system_settings(), "{ not json")
        .writable_by_others(system_settings());

    let inventory = inventory(&fs);

    assert!(
        inventory
            .read_failures
            .iter()
            .all(|failure| failure.agent != "gemini-cli")
    );
    assert!(
        inventory
            .agent_flags
            .iter()
            .all(|flag| flag.agent != "gemini-cli")
    );
}

/// The Gemini CLI home with the id `id`.
fn home<'i>(inventory: &'i Inventory, id: &str) -> &'i config_digger_core::api::Home {
    inventory
        .homes
        .iter()
        .find(|home| home.agent == "gemini-cli" && home.home == id)
        .unwrap_or_else(|| panic!("no Gemini CLI home `{id}`"))
}

#[test]
fn gemini_cli_home_names_another_home_folder_holding_a_gemini_folder() {
    let other = Path::new(r"D:\gemini-home");
    let fs = FakeFileSystem::new()
        .with_file(user_settings(), docs("default"))
        .with_file(other.join(r".gemini\settings.json"), docs("other"));

    let inventory = inventory_with(&fs, env().with_var("GEMINI_CLI_HOME", r"D:\gemini-home"));

    let env_home = home(&inventory, "gemini-cli-home");
    assert_eq!(env_home.folder, other);
    assert_eq!(
        json_of(&env_home.source),
        json!({ "kind": "environment", "variable": "GEMINI_CLI_HOME" })
    );
    let docs = item_of(&inventory, "gemini-cli-home", "mcp", "docs");
    assert_eq!(docs.entries[0].path, other.join(r".gemini\settings.json"));
    assert_eq!(states(docs), ["effective"]);
    assert_eq!(code(server(&inventory, "docs").state), "effective");
}

fn extensions() -> PathBuf {
    gemini().join("extensions")
}

/// `fs` with the extension `name` installed by copy, its manifest declaring
/// `servers`.
fn with_extension(fs: FakeFileSystem, name: &str, servers: Value) -> FakeFileSystem {
    let folder = extensions().join(name);
    fs.with_file(
        folder.join(".gemini-extension-install.json"),
        json!({ "source": "https://github.com/example/ext", "type": "git" }).to_string(),
    )
    .with_file(
        folder.join("gemini-extension.json"),
        json!({ "name": name, "version": "1.0.0", "mcpServers": servers }).to_string(),
    )
}

#[test]
fn an_extension_s_servers_are_items_it_uninstalls() {
    let fs = with_extension(
        FakeFileSystem::new().with_file(user_settings(), "{}"),
        "weather",
        json!({ "forecast": { "command": "node", "args": ["${extensionPath}${/}forecast.js"] } }),
    );

    let inventory = inventory(&fs);

    let forecast = server(&inventory, "forecast");
    assert_eq!(code(forecast.state), "effective");
    let entry = entry_json(&forecast.entries[0]);
    assert_eq!(
        entry["path"],
        extensions()
            .join(r"weather\gemini-extension.json")
            .display()
            .to_string()
    );
    assert_eq!(entry["scope"], "extension");
    assert_eq!(entry["provenance"], "extension");
    assert_eq!(entry["plugin"]["id"], "weather");
    assert_eq!(entry["plugin"]["name"], "weather");
    assert_eq!(entry["remove"]["status"], "not_offered");
    assert_eq!(
        entry["remove"]["how_to_remove"]["command"],
        "gemini extensions uninstall weather"
    );
}

#[test]
fn a_linked_extension_is_read_from_the_folder_it_links() {
    let source = Path::new(r"E:\src\weather-ext");
    let fs = FakeFileSystem::new()
        .with_file(user_settings(), "{}")
        .with_file(
            extensions().join(r"weather\.gemini-extension-install.json"),
            json!({ "source": source.display().to_string(), "type": "link" }).to_string(),
        )
        .with_file(
            source.join("gemini-extension.json"),
            json!({ "name": "weather", "mcpServers": { "forecast": { "command": "node" } } })
                .to_string(),
        );

    let inventory = inventory(&fs);

    let forecast = server(&inventory, "forecast");
    assert_eq!(
        forecast.entries[0].path,
        source.join("gemini-extension.json")
    );
    assert_eq!(
        entry_json(&forecast.entries[0])["plugin"]["folder"],
        source.display().to_string()
    );
}

#[test]
fn of_two_extensions_with_a_server_of_one_name_the_first_loaded_wins() {
    let fs = with_extension(
        with_extension(
            FakeFileSystem::new().with_file(user_settings(), "{}"),
            "beta",
            json!({ "search": { "command": "beta" } }),
        ),
        "alpha",
        json!({ "search": { "command": "alpha" } }),
    );

    let inventory = inventory(&fs);

    let search = server(&inventory, "search");
    assert_eq!(states(search), ["effective", "shadowed"]);
    assert_eq!(entry_json(&search.entries[0])["plugin"]["id"], "alpha");
    assert!(search.built.is_none());
}

/// The `<path>#<key path>` locator of the server `name` in the file `path`.
fn locator(path: &Path, name: &str) -> String {
    format!("{}#mcpServers.{name}", path.display())
}

#[test]
fn a_settings_entry_overlays_the_extension_server_of_its_name_field_by_field() {
    let fs = with_extension(
        FakeFileSystem::new().with_file(
            user_settings(),
            json!({ "mcpServers": { "docs": {
                "timeout": 10,
                "env": { "A": "user" },
                "excludeTools": ["delete"],
                "includeTools": ["read", "write"]
            } } })
            .to_string(),
        ),
        "kit",
        json!({ "docs": {
            "command": "node",
            "timeout": 5,
            "env": { "A": "extension", "B": "extension" },
            "excludeTools": ["drop"],
            "includeTools": ["read", "delete"]
        } }),
    );

    let inventory = inventory(&fs);

    let docs = server(&inventory, "docs");
    assert_eq!(states(docs), ["merged", "merged"]);
    assert_eq!(code(docs.state), "merged");
    let built = docs.built.as_ref().expect("the server Gemini CLI builds");
    assert_eq!(
        built.config,
        json!({
            "command": "node",
            "timeout": 10,
            // Masked, as every env value is.
            "env": { "A": "••••", "B": "••••" },
            "excludeTools": ["drop", "delete"],
            "includeTools": ["read"]
        })
    );
    let user = locator(&user_settings(), "docs");
    let extension = locator(&extensions().join(r"kit\gemini-extension.json"), "docs");
    let origin = |key_path: &[&str]| {
        built
            .origins
            .iter()
            .find(|origin| origin.key_path == key_path)
            .map(|origin| origin.entry.clone())
            .unwrap_or_else(|| panic!("no origin for {key_path:?}"))
    };
    assert_eq!(origin(&["command"]), extension);
    assert_eq!(origin(&["timeout"]), user);
    assert_eq!(origin(&["env", "A"]), user);
    assert_eq!(origin(&["env", "B"]), extension);
    assert_eq!(origin(&["excludeTools", "0"]), extension);
    assert_eq!(origin(&["excludeTools", "1"]), user);
    assert_eq!(origin(&["includeTools"]), user);
    let reason = docs.entries[0].reason.as_ref().unwrap();
    assert_eq!(code(reason.code), "merged");
}

#[test]
fn only_the_effective_settings_entry_overlays_an_extension_server() {
    let folder = project("app");
    let fs = trusting(
        with_extension(
            with_extension(
                FakeFileSystem::new()
                    .with_file(user_settings(), docs("user"))
                    .with_file(
                        project_settings(&folder),
                        r#"{ "mcpServers": { "docs": { "timeout": 1 } } }"#,
                    ),
                "a-kit",
                json!({ "docs": { "command": "first" } }),
            ),
            "b-kit",
            json!({ "docs": { "command": "second" } }),
        ),
        &folder,
    );

    let inventory = inventory_at(&fs, &folder);

    let docs = server(&inventory, "docs");
    assert_eq!(states(docs), ["merged", "shadowed", "merged", "shadowed"]);
    let built = docs.built.as_ref().unwrap();
    assert_eq!(built.config, json!({ "command": "first", "timeout": 1 }));
    // The user's Entry loses to the Project's, and the second extension to
    // the first.
    let shadowed_by = |index: usize| {
        docs.entries[index]
            .reason
            .as_ref()
            .unwrap()
            .sentence
            .clone()
    };
    assert!(
        shadowed_by(1).contains(&project_settings(&folder).display().to_string()),
        "{}",
        shadowed_by(1)
    );
    assert!(shadowed_by(3).contains("a-kit"), "{}", shadowed_by(3));
}

#[test]
fn gemini_mcp_list_hides_the_extension_half_of_a_merged_server() {
    let fs = with_extension(
        FakeFileSystem::new().with_file(
            user_settings(),
            r#"{ "mcpServers": { "docs": { "excludeTools": ["delete"] } } }"#,
        ),
        "kit",
        json!({ "docs": { "command": "node" } }),
    );

    let inventory = inventory(&fs);

    let docs = server(&inventory, "docs");
    let cli_note = |index: usize| {
        docs.entries[index]
            .notes
            .iter()
            .find(|note| code(note.code) == "cli_view_differs")
    };
    assert!(cli_note(0).is_none());
    let note = cli_note(1).expect("the extension's Entry notes it");
    assert!(
        note.sentence.contains("gemini mcp list"),
        "{}",
        note.sentence
    );
}

#[test]
fn two_extensions_whose_manifests_share_a_name_are_invalid() {
    let fs = with_extension(
        FakeFileSystem::new().with_file(user_settings(), "{}"),
        "kit",
        json!({ "first": { "command": "a" } }),
    )
    .with_file(
        extensions().join(r"kit-copy\.gemini-extension-install.json"),
        json!({ "source": r"E:\src\kit", "type": "local" }).to_string(),
    )
    .with_file(
        extensions().join(r"kit-copy\gemini-extension.json"),
        json!({ "name": "kit", "mcpServers": { "second": { "command": "b" } } }).to_string(),
    );
    let fs = with_extension(fs, "other", json!({ "third": { "command": "c" } }));

    let inventory = inventory(&fs);

    for name in ["first", "second"] {
        let item = server(&inventory, name);
        assert_eq!(code(item.state), "invalid", "{name}");
        let reason = item.entries[0].reason.as_ref().unwrap();
        assert_eq!(code(reason.code), "duplicate_plugin_name");
        assert!(reason.sentence.contains("kit"), "{}", reason.sentence);
    }
    assert_eq!(code(server(&inventory, "third").state), "effective");
}

/// `fs` with `trustedFolders.json` holding `rules`, by folder.
fn with_trust_rules(fs: FakeFileSystem, rules: &[(&Path, &str)]) -> FakeFileSystem {
    let rules: serde_json::Map<String, Value> = rules
        .iter()
        .map(|(folder, level)| (trust_key(folder), Value::from(*level)))
        .collect();
    fs.with_file(trusted_folders(), Value::Object(rules).to_string())
}

#[test]
fn in_a_folder_no_rule_trusts_every_server_is_blocked_user_ones_included() {
    let folder = project("app");
    let fs = with_extension(
        FakeFileSystem::new()
            .with_file(user_settings(), docs("user"))
            .with_file(
                project_settings(&folder),
                r#"{ "mcpServers": { "local": { "command": "x" } } }"#,
            ),
        "kit",
        json!({ "forecast": { "command": "node" } }),
    );

    let inventory = inventory_at(&fs, &folder);

    for name in ["docs", "local", "forecast"] {
        let item = server(&inventory, name);
        assert_eq!(states(item), ["blocked"], "{name}");
        let reason = item.entries[0].reason.as_ref().unwrap();
        assert_eq!(code(reason.code), "untrusted_folder", "{name}");
        assert!(
            reason.sentence.contains(&folder.display().to_string()),
            "{}",
            reason.sentence
        );
    }
}

#[test]
fn a_longer_rule_distrusting_a_folder_beats_one_trusting_the_folder_above() {
    let folder = project("app");
    let fs = with_trust_rules(
        FakeFileSystem::new().with_file(user_settings(), docs("user")),
        &[
            (Path::new(PROJECTS), "TRUST_FOLDER"),
            (&folder, "DO_NOT_TRUST"),
        ],
    );

    let inventory = inventory_at(&fs, &folder);

    assert_eq!(code(server(&inventory, "docs").state), "blocked");
}

#[test]
fn a_longer_rule_trusting_the_folder_above_its_own_beats_one_distrusting_further_up() {
    let folder = project(r"repo\app");
    let fs = with_trust_rules(
        FakeFileSystem::new()
            .with_file(user_settings(), docs("user"))
            .with_folder(&folder),
        &[
            (Path::new(r"C:\"), "DO_NOT_TRUST"),
            (Path::new(PROJECTS), "DO_NOT_TRUST"),
            (&project(r"repo\marker"), "TRUST_PARENT"),
        ],
    );

    let inventory = inventory_at(&fs, &folder);

    assert_eq!(code(server(&inventory, "docs").state), "effective");
}

#[test]
fn turning_folder_trust_off_trusts_every_folder() {
    let folder = project("app");
    let fs = FakeFileSystem::new().with_file(
        user_settings(),
        r#"{ "security": { "folderTrust": { "enabled": false } }, "mcpServers": { "docs": { "command": "x" } } }"#,
    );

    let inventory = inventory_at(&fs, &folder);

    assert_eq!(code(server(&inventory, "docs").state), "effective");
}

#[test]
fn with_no_project_user_servers_note_that_an_untrusted_folder_blocks_them() {
    let fs = with_extension(
        FakeFileSystem::new().with_file(user_settings(), docs("user")),
        "kit",
        json!({ "forecast": { "command": "node" } }),
    );

    let inventory = inventory(&fs);

    for name in ["docs", "forecast"] {
        let entry = &server(&inventory, name).entries[0];
        assert_eq!(code(entry.state), "effective");
        let note = entry
            .notes
            .iter()
            .find(|note| code(note.code) == "untrusted_folder_blocks")
            .unwrap_or_else(|| panic!("no trust note on {name}"));
        assert!(note.sentence.contains("trust"), "{}", note.sentence);
    }
}

/// A `SKILL.md` naming the skill `name` in its frontmatter.
fn skill_md(name: &str) -> String {
    format!("---\nname: {name}\ndescription: Does {name}.\n---\nBody.\n")
}

/// The `SKILL.md` of the skill folder `folder` in the folder of skills `skills`.
fn skill(skills: &Path, folder: &str) -> PathBuf {
    skills.join(folder).join("SKILL.md")
}

fn user_skills() -> PathBuf {
    gemini().join("skills")
}

fn agents_skills() -> PathBuf {
    Path::new(HOME).join(r".agents\skills")
}

/// The Gemini CLI skill named `name` in the default home.
fn skill_item<'i>(inventory: &'i Inventory, name: &str) -> &'i Item {
    item_of(inventory, "default", "skill", name)
}

#[test]
fn skills_are_named_by_their_frontmatter_and_the_last_loaded_wins() {
    let fs = FakeFileSystem::new()
        .with_file(user_settings(), "{}")
        .with_file(
            skill(&user_skills(), "jetpack-compose"),
            skill_md("android-jetpack-compose"),
        )
        .with_file(skill(&user_skills(), "review"), skill_md("review"))
        .with_file(skill(&agents_skills(), "review"), skill_md("review"));

    let inventory = inventory(&fs);

    let compose = skill_item(&inventory, "android-jetpack-compose");
    assert_eq!(states(compose), ["effective"]);
    assert_eq!(
        compose.entries[0].path,
        user_skills().join("jetpack-compose")
    );
    let review = skill_item(&inventory, "review");
    assert_eq!(states(review), ["effective", "shadowed"]);
    // `~\.agents\skills` loads after `~\.gemini\skills`.
    assert_eq!(review.entries[0].path, agents_skills().join("review"));
    assert_eq!(entry_json(&review.entries[0])["scope"], "user");
}

#[test]
fn a_skill_name_gemini_cli_cannot_use_as_a_file_name_is_rewritten_with_hyphens() {
    let fs = FakeFileSystem::new()
        .with_file(user_settings(), "{}")
        .with_file(skill(&user_skills(), "x"), skill_md("team:review"));

    let inventory = inventory(&fs);

    assert_eq!(states(skill_item(&inventory, "team-review")), ["effective"]);
}

#[test]
fn a_skill_without_frontmatter_is_invalid() {
    let fs = FakeFileSystem::new()
        .with_file(user_settings(), "{}")
        .with_file(skill(&user_skills(), "plain"), "Just a body.\n");

    let inventory = inventory(&fs);

    let plain = gemini_items(&inventory)
        .find(|item| code(item.kind) == "skill")
        .expect("the skill is listed");
    assert_eq!(states(plain), ["invalid"]);
    let reason = plain.entries[0].reason.as_ref().unwrap();
    assert_eq!(code(reason.code), "malformed_skill");
}

/// The reason of the one Gemini CLI skill of `inventory`, which is Invalid.
fn invalid_reason(inventory: &Inventory) -> String {
    let mut skills = gemini_items(inventory).filter(|item| code(item.kind) == "skill");
    let skill = skills.next().expect("the skill is listed");
    assert!(skills.next().is_none(), "one skill");
    assert_eq!(states(skill), ["invalid"]);
    let reason = skill.entries[0].reason.as_ref().unwrap();
    assert_eq!(code(reason.code), "malformed_skill");
    reason.sentence.clone()
}

/// The inventory of a Gemini CLI home whose one user skill's `SKILL.md`
/// holds `text`.
fn with_skill_md(text: &str) -> Inventory {
    inventory(
        &FakeFileSystem::new()
            .with_file(user_settings(), "{}")
            .with_file(skill(&user_skills(), "folder"), text),
    )
}

#[test]
fn a_description_holding_a_colon_and_space_is_read_line_by_line() {
    let inventory = with_skill_md("---\nname: review\ndescription: Use when: the user asks\n---\n");

    let review = skill_item(&inventory, "review");
    assert_eq!(states(review), ["effective"]);
    assert_eq!(
        entry_json(&review.entries[0])["config"],
        json!({ "name": "review", "description": "Use when: the user asks" })
    );
}

#[test]
fn a_frontmatter_with_a_stray_line_is_read_line_by_line() {
    let inventory = with_skill_md(
        "---\n  name: \"review\"\njust text\nname: 123\ndescription: Reviews\n  the diff.\n\n  Not this.\n---\n",
    );

    // A later `name` line wins, and a blank line ends the description.
    let review = skill_item(&inventory, "123");
    assert_eq!(states(review), ["effective"]);
    assert_eq!(
        entry_json(&review.entries[0])["config"],
        json!({ "name": "123", "description": "Reviews the diff." })
    );
}

#[test]
fn a_name_read_line_by_line_keeps_its_quotes_rewritten_with_hyphens() {
    let inventory = with_skill_md("---\nname: \"team:review\"\ndescription: Use when: x\n---\n");

    let review = skill_item(&inventory, "-team-review-");
    assert_eq!(states(review), ["effective"]);
    assert_eq!(
        entry_json(&review.entries[0])["config"]["name"],
        "\"team:review\""
    );
}

#[test]
fn a_name_yaml_reads_as_a_number_is_read_line_by_line() {
    let inventory = with_skill_md("---\nname: 123\ndescription: \"Reviews.\"\n---\n");

    let review = skill_item(&inventory, "123");
    assert_eq!(states(review), ["effective"]);
    assert_eq!(
        entry_json(&review.entries[0])["config"]["description"],
        "\"Reviews.\""
    );
}

#[test]
fn an_unclosed_bracket_names_the_skill_as_written() {
    let inventory = with_skill_md("---\nname: [unclosed\ndescription: Reviews.\n---\n");

    assert_eq!(states(skill_item(&inventory, "[unclosed")), ["effective"]);
}

#[test]
fn a_frontmatter_without_a_name_line_is_invalid() {
    let sentence = invalid_reason(&with_skill_md("---\ndescription: Use when: x\n---\n"));

    assert!(sentence.contains("has no `name`"), "{sentence}");
}

#[test]
fn a_frontmatter_without_a_description_line_is_invalid() {
    let sentence = invalid_reason(&with_skill_md("---\nname: review\n---\n"));

    assert!(sentence.contains("has no `description`"), "{sentence}");
}

#[test]
fn an_unclosed_frontmatter_is_no_frontmatter() {
    let sentence = invalid_reason(&with_skill_md("---\nname: review\ndescription: Reviews.\n"));

    assert!(
        sentence.contains("does not open with a `---` frontmatter"),
        "{sentence}"
    );
}

#[test]
fn skill_content_reads_the_frontmatter_as_the_inventory_does() {
    let fs = FakeFileSystem::new().with_file(
        skill(&user_skills(), "folder"),
        "---\nname: review\ndescription: Use when: x\n---\n",
    );

    let content = core_with(&fs, env())
        .skill_content(&user_skills().join("folder"), "gemini-cli")
        .unwrap();

    assert_eq!(
        (content.name.as_deref(), content.description.as_deref()),
        (Some("review"), Some("Use when: x"))
    );
}

#[test]
fn project_skills_beat_the_user_s_in_a_trusted_folder_and_are_blocked_in_another() {
    let trusted = project("trusted");
    let untrusted = project("untrusted");
    let with_project_skills = |fs: FakeFileSystem, folder: &Path| {
        fs.with_file(
            skill(&folder.join(r".gemini\skills"), "review"),
            skill_md("review"),
        )
        .with_file(
            skill(&folder.join(r".agents\skills"), "review"),
            skill_md("review"),
        )
    };
    let fs = trusting(
        with_project_skills(
            with_project_skills(
                FakeFileSystem::new()
                    .with_file(user_settings(), "{}")
                    .with_file(skill(&agents_skills(), "review"), skill_md("review")),
                &trusted,
            ),
            &untrusted,
        ),
        &trusted,
    );

    let inventory = inventory_at(&fs, &trusted);
    let review = skill_item(&inventory, "review");
    assert_eq!(states(review), ["effective", "shadowed", "shadowed"]);
    // `.agents\skills` loads after `.gemini\skills`.
    assert_eq!(
        review.entries[0].path,
        trusted.join(r".agents\skills\review")
    );
    assert_eq!(entry_json(&review.entries[0])["scope"], "project");

    let inventory = inventory_at(&fs, &untrusted);
    let review = skill_item(&inventory, "review");
    let by_path = |path: PathBuf| {
        let entry = review
            .entries
            .iter()
            .find(|entry| entry.path == path)
            .unwrap();
        code(entry.state)
    };
    assert_eq!(by_path(untrusted.join(r".agents\skills\review")), "blocked");
    assert_eq!(by_path(untrusted.join(r".gemini\skills\review")), "blocked");
    // The user's skills load in any folder.
    assert_eq!(by_path(agents_skills().join("review")), "effective");
}

#[test]
fn an_extension_s_skills_rank_below_the_user_s_and_built_ins_below_them() {
    let builtin = Path::new(APP_DATA).join(r"npm\node_modules\@google\gemini-cli\bundle\builtin");
    let fs = with_extension(
        FakeFileSystem::new()
            .with_file(user_settings(), "{}")
            .with_file(skill(&builtin, "skill-creator"), skill_md("skill-creator"))
            .with_file(
                skill(&builtin, "antigravity-support"),
                skill_md("antigravity-support"),
            )
            .with_file(
                skill(&user_skills(), "skill-creator"),
                skill_md("skill-creator"),
            ),
        "kit",
        json!({}),
    )
    .with_file(
        skill(&extensions().join(r"kit\skills"), "greeter"),
        skill_md("greeter"),
    )
    .with_file(
        skill(&extensions().join(r"kit\skills"), "antigravity-support"),
        skill_md("antigravity-support"),
    );

    let inventory = inventory(&fs);

    let greeter = entry_json(&skill_item(&inventory, "greeter").entries[0]);
    assert_eq!(greeter["scope"], "extension");
    assert_eq!(greeter["provenance"], "extension");
    assert_eq!(greeter["plugin"]["id"], "kit");
    let creator = skill_item(&inventory, "skill-creator");
    assert_eq!(states(creator), ["effective", "shadowed"]);
    assert_eq!(entry_json(&creator.entries[1])["provenance"], "built_in");
    assert_eq!(entry_json(&creator.entries[1])["scope"], "system");
    let support = skill_item(&inventory, "antigravity-support");
    assert_eq!(states(support), ["effective", "shadowed"]);
    assert_eq!(entry_json(&support.entries[0])["scope"], "extension");
}

#[test]
fn another_agent_s_agents_skills_folder_alone_finds_no_gemini_cli_home() {
    let fs = FakeFileSystem::new().with_file(skill(&agents_skills(), "review"), skill_md("review"));

    let inventory = inventory(&fs);

    assert!(
        inventory
            .homes
            .iter()
            .all(|home| home.agent != "gemini-cli")
    );
}

#[test]
fn projects_come_from_projects_json_and_trusted_folders_json() {
    let recorded = project("PetMe");
    let trusted = project(r"Scratch\Gemini");
    let fs = FakeFileSystem::new()
        .with_file(user_settings(), "{}")
        .with_folder(&recorded)
        .with_folder(&trusted)
        .with_file(
            gemini().join("projects.json"),
            json!({ "projects": {
                recorded.display().to_string().to_lowercase(): "petme",
                HOME.to_lowercase(): "fixture"
            } })
            .to_string(),
        )
        .with_file(
            trusted_folders(),
            json!({ trust_key(&trusted): "TRUST_FOLDER" }).to_string(),
        );

    let inventory = inventory(&fs);

    let registered: Vec<&Path> = inventory
        .projects
        .iter()
        .filter(|project| {
            project
                .registered_by
                .iter()
                .any(|agent| agent == "gemini-cli")
        })
        .map(|project| project.path.as_path())
        .collect();
    assert!(registered.contains(&recorded.as_path()), "{registered:?}");
    assert!(registered.contains(&trusted.as_path()), "{registered:?}");
    assert!(registered.contains(&Path::new(HOME)), "{registered:?}");
}

#[test]
fn antigravity_s_mcp_config_json_is_never_read() {
    let fs = FakeFileSystem::new()
        .with_file(user_settings(), "{}")
        .with_file(
            gemini().join(r"config\mcp_config.json"),
            r#"{ "mcpServers": { "dart-mcp-server": { "command": "dart" } } }"#,
        )
        .with_file(
            gemini().join(r"antigravity\mcp_config.json"),
            r#"{ "mcpServers": { "dart-mcp-server": { "command": "dart" } } }"#,
        );

    let inventory = inventory(&fs);

    assert!(
        inventory
            .items
            .iter()
            .all(|item| item.name != "dart-mcp-server")
    );
    assert!(inventory.read_failures.is_empty());
}

/// User settings signed in through `auth`, with the server `docs` and the
/// skill `review`.
fn signed_in(auth: &str) -> FakeFileSystem {
    FakeFileSystem::new()
        .with_file(
            user_settings(),
            json!({
                "security": { "auth": { "selectedType": auth } },
                "mcpServers": { "docs": { "command": "x" } }
            })
            .to_string(),
        )
        .with_file(skill(&user_skills(), "review"), skill_md("review"))
}

fn caveat_codes(entry: &Entry) -> Vec<String> {
    entry
        .caveats
        .iter()
        .map(|caveat| code(caveat.code))
        .collect()
}

#[test]
fn signed_in_with_google_or_adc_an_admin_may_turn_any_entry_off() {
    for auth in ["oauth-personal", "compute-default-credentials"] {
        let inventory = inventory(&signed_in(auth));

        for item in [server(&inventory, "docs"), skill_item(&inventory, "review")] {
            let entry = &item.entries[0];
            assert_eq!(caveat_codes(entry), ["gate_may_apply"], "{auth}");
            assert_eq!(code(entry.state), "effective");
            assert_eq!(entry.caveats[0].path, user_settings());
            assert!(
                entry.caveats[0].sentence.contains("admin"),
                "{}",
                entry.caveats[0].sentence
            );
        }
    }
}

#[test]
fn signed_in_with_an_api_key_or_vertex_ai_no_admin_control_applies() {
    for auth in ["gemini-api-key", "vertex-ai"] {
        let inventory = inventory(&signed_in(auth));

        assert!(
            caveat_codes(&server(&inventory, "docs").entries[0]).is_empty(),
            "{auth}"
        );
        assert!(
            caveat_codes(&skill_item(&inventory, "review").entries[0]).is_empty(),
            "{auth}"
        );
    }
}

/// The fake Secret every Secret field holds.
const SECRET: &str = "not-a-real-secret";

#[test]
fn secrets_are_masked_and_references_gemini_cli_expands_are_shown() {
    let server_config = json!({
        "httpUrl": format!("https://user:{SECRET}@mcp.example/mcp?key={SECRET}"),
        "url": format!("https://mcp.example/sse?token={SECRET}"),
        "headers": { "Authorization": format!("Bearer {SECRET}") },
        "env": {
            "LITERAL": SECRET,
            "BARE": "$GITHUB_TOKEN",
            "BRACED": "${GITHUB_TOKEN}",
            "DEFAULTED": "${GITHUB_TOKEN:-none}"
        },
        "oauth": { "clientId": "client", "clientSecret": SECRET }
    });
    let fs = with_extension(
        FakeFileSystem::new().with_file(
            user_settings(),
            json!({ "mcpServers": { "remote": server_config } }).to_string(),
        ),
        "kit",
        json!({ "kit-remote": server_config }),
    );

    let inventory = inventory(&fs);

    for name in ["remote", "kit-remote"] {
        let entry = &server(&inventory, name).entries[0];
        let text = serde_json::to_string(&entry_json(entry)).unwrap();
        assert!(!text.contains(SECRET), "{name}: {text}");
        let config = &entry.config;
        assert_eq!(
            config["httpUrl"], "https://••••@mcp.example/mcp?key=••••",
            "{name}"
        );
        assert_eq!(
            config["url"], "https://mcp.example/sse?token=••••",
            "{name}"
        );
        assert_eq!(config["headers"]["Authorization"], "Bearer ••••", "{name}");
        assert_eq!(config["env"]["LITERAL"], "••••", "{name}");
        assert_eq!(config["env"]["BARE"], "$GITHUB_TOKEN", "{name}");
        assert_eq!(config["env"]["BRACED"], "${GITHUB_TOKEN}", "{name}");
        assert_eq!(
            config["env"]["DEFAULTED"], "${GITHUB_TOKEN:-••••}",
            "{name}"
        );
        assert_eq!(config["oauth"]["clientSecret"], "••••", "{name}");
        assert_eq!(config["oauth"]["clientId"], "client", "{name}");
    }
}

#[test]
fn with_folder_trust_off_no_entry_notes_that_an_untrusted_folder_blocks_it() {
    let fs = FakeFileSystem::new().with_file(
        user_settings(),
        r#"{ "security": { "folderTrust": { "enabled": false } }, "mcpServers": { "docs": { "command": "x" } } }"#,
    );

    let inventory = inventory(&fs);

    let entry = &server(&inventory, "docs").entries[0];
    assert!(
        entry
            .notes
            .iter()
            .all(|note| code(note.code) != "untrusted_folder_blocks")
    );
}

#[test]
fn a_project_signed_in_with_google_gets_the_admin_controls_caveat() {
    let folder = project("app");
    let fs = trusting(
        FakeFileSystem::new()
            .with_file(user_settings(), docs("user"))
            .with_file(
                project_settings(&folder),
                r#"{ "security": { "auth": { "selectedType": "oauth-personal" } } }"#,
            ),
        &folder,
    );

    let inventory = inventory_at(&fs, &folder);

    let entry = &server(&inventory, "docs").entries[0];
    assert_eq!(caveat_codes(entry), ["gate_may_apply"]);
    assert_eq!(entry.caveats[0].path, project_settings(&folder));
}

#[test]
fn a_folder_without_an_install_record_is_no_extension() {
    let fs = FakeFileSystem::new()
        .with_file(user_settings(), "{}")
        .with_file(
            extensions().join(r"stray\gemini-extension.json"),
            json!({ "name": "stray", "mcpServers": { "stray": { "command": "x" } } }).to_string(),
        );

    let inventory = inventory(&fs);

    assert!(gemini_items(&inventory).all(|item| item.name != "stray"));
}
