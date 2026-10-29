//! Claude Desktop through the core `api`, with fake ports over fixture
//! classic and MSIX data folders: where Claude Desktop keeps its homes and
//! which one it reads, the servers of its user file and of its extensions,
//! the policy and switches that keep them out, the skills it mirrors from
//! the account, and how a running Claude Desktop is told apart from Claude
//! Code.

use config_digger_core::api::{
    Action, Context, Core, EnableOption, Entry, Home, Inventory, Item, ScanOptions, Targets,
    WarningClass,
};
use config_digger_core::ports::fake::{self, FakeEnvironment, FakeFileSystem, FakeProcesses};
use config_digger_core::ports::{Hive, RegistryData, RunningProcess};
use serde_json::Value;
use std::path::{Path, PathBuf};

const HOME: &str = r"C:\Users\fixture";
const APP_DATA: &str = r"C:\Users\fixture\AppData\Roaming";
const LOCAL_APP_DATA: &str = r"C:\Users\fixture\AppData\Local";

/// The classic installer's data folder.
const CLASSIC: &str = r"C:\Users\fixture\AppData\Roaming\Claude";

/// The package family of the Store MSIX package, and of the one signed
/// as Anthropic PBC.
const STORE_FAMILY: &str = "Claude_pzs8sxrjxfjjc";
const ANTHROPIC_PBC_FAMILY: &str = "AnthropicPBC.Claude_fnn82j28hfe8t";

/// The fixture folder of each data folder layout.
fn fixture_folder(layout: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join(r"tests\fixtures\claude-desktop")
        .join(layout)
}

/// Every file below `folder` on disk, at the same place below `at`.
fn load(fs: FakeFileSystem, folder: &Path, at: &Path) -> FakeFileSystem {
    let mut fs = fs.with_folder(at);
    for child in std::fs::read_dir(folder).unwrap() {
        let child = child.unwrap().path();
        let name = child.file_name().unwrap();
        fs = if child.is_dir() {
            load(fs, &child, &at.join(name))
        } else {
            fs.with_file(at.join(name), std::fs::read(&child).unwrap())
        };
    }
    fs
}

/// Where the MSIX package of the family `family` keeps the data folder
/// that the app sees as `%APPDATA%\Claude`.
fn msix_folder(family: &str) -> PathBuf {
    PathBuf::from(LOCAL_APP_DATA)
        .join("Packages")
        .join(family)
        .join(r"LocalCache\Roaming\Claude")
}

/// The Store package's data folder.
fn msix() -> PathBuf {
    msix_folder(STORE_FAMILY)
}

/// The classic fixture data folder at `%APPDATA%\Claude`.
fn classic() -> FakeFileSystem {
    load(
        FakeFileSystem::new(),
        &fixture_folder("classic"),
        Path::new(CLASSIC),
    )
}

/// The MSIX fixture data folder in the package of the family `family`.
fn msix_of(family: &str) -> FakeFileSystem {
    load(
        FakeFileSystem::new(),
        &fixture_folder("msix"),
        &msix_folder(family),
    )
}

/// The MSIX fixture data folder of the Store package.
fn store() -> FakeFileSystem {
    msix_of(STORE_FAMILY)
}

/// The Claude Desktop home `home`.
fn home<'i>(inventory: &'i Inventory, home: &str) -> &'i Home {
    inventory
        .homes
        .iter()
        .find(|found| found.agent == "claude-desktop" && found.home == home)
        .unwrap_or_else(|| panic!("no Claude Desktop home `{home}`"))
}

fn env() -> FakeEnvironment {
    FakeEnvironment::with_home(HOME)
        .with_var("APPDATA", APP_DATA)
        .with_var("LOCALAPPDATA", LOCAL_APP_DATA)
}

fn core_with(fs: &FakeFileSystem, env: FakeEnvironment) -> Core {
    Core::new(fake::ports(fs.clone(), env)).unwrap()
}

fn inventory_with(fs: &FakeFileSystem, env: FakeEnvironment) -> Inventory {
    let core = core_with(fs, env);
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    core.resolve(&snapshot, &Context::default())
}

fn inventory(fs: &FakeFileSystem) -> Inventory {
    inventory_with(fs, env())
}

/// The Claude Desktop Item named `name`.
fn item<'i>(inventory: &'i Inventory, name: &str) -> &'i Item {
    inventory
        .items
        .iter()
        .find(|item| item.agent == "claude-desktop" && item.name == name)
        .unwrap_or_else(|| panic!("no Claude Desktop Item `{name}`"))
}

fn json(value: impl serde::Serialize) -> Value {
    serde_json::to_value(value).unwrap()
}

fn entry_json(entry: &Entry) -> Value {
    json(entry)
}

#[test]
fn a_classic_install_lists_the_servers_of_its_user_file() {
    let inventory = inventory(&classic());

    let filesystem = item(&inventory, "filesystem");
    assert_eq!(json(filesystem.kind), "mcp");
    assert_eq!(json(filesystem.state), "effective");
    assert_eq!(filesystem.home, "classic");
    let entry = entry_json(&filesystem.entries[0]);
    assert_eq!(
        entry["path"],
        format!(r"{CLASSIC}\claude_desktop_config.json")
    );
    assert_eq!(
        entry["key_path"],
        serde_json::json!(["mcpServers", "filesystem"])
    );
    assert_eq!(entry["scope"], "user");
    assert_eq!(entry["provenance"], "file");
    let home = home(&inventory, "classic");
    assert_eq!(home.folder, PathBuf::from(CLASSIC));
    assert_eq!(home.scopes, ["user"]);
}

#[test]
fn the_classic_installer_s_app_reads_the_classic_home_with_the_version_of_its_launcher() {
    let launcher = PathBuf::from(LOCAL_APP_DATA).join(r"AnthropicClaude\claude.exe");
    let fs = classic().with_file(&launcher, fake::executable(Some([3, 6556, 2, 0])));

    let inventory = inventory(&fs);

    let installed: Vec<(&str, Option<&str>)> = home(&inventory, "classic")
        .installations
        .iter()
        .map(|installation| (installation.id.as_str(), installation.version.as_deref()))
        .collect();
    assert_eq!(installed, [("classic", Some("3.6556.2"))]);
}

#[test]
fn a_store_package_keeps_its_data_folder_in_its_local_cache() {
    let inventory = inventory(&store());

    let notes = item(&inventory, "notes");
    assert_eq!(notes.home, "msix");
    assert_eq!(
        notes.entries[0].path,
        msix().join("claude_desktop_config.json")
    );
    assert_eq!(home(&inventory, "msix").folder, msix());
    assert_eq!(home(&inventory, "msix").not_read, None);
    assert!(
        !inventory
            .homes
            .iter()
            .any(|found| found.home == "msix-moved"),
        "a fresh install has moved nothing to `Claude-Data`"
    );
}

/// `fs` with the classic fixture data folder also at `folder`.
fn with_classic_at(fs: FakeFileSystem, folder: &Path) -> FakeFileSystem {
    load(fs, &fixture_folder("classic"), folder)
}

#[test]
fn claude_user_data_dir_names_no_home_since_signed_builds_ignore_it() {
    let custom = PathBuf::from(r"E:\claude-data");
    let fs = with_classic_at(store(), Path::new(CLASSIC));
    let fs = with_classic_at(fs, &custom);
    let env = env().with_var("CLAUDE_USER_DATA_DIR", r"E:\claude-data");

    let with_variable = inventory_with(&fs, env);

    let filesystem = item(&with_variable, "filesystem");
    assert_eq!(filesystem.home, "classic");
    assert_eq!(json(filesystem.state), "effective");
    for home in &with_variable.homes {
        assert_ne!(home.folder, custom, "{home:#?}");
        if let Some(not_read) = &home.not_read {
            assert!(
                !not_read.sentence.contains(r"E:\claude-data"),
                "{}",
                not_read.sentence
            );
        }
    }
    assert_eq!(with_variable, inventory(&fs));
}

/// The Claude Desktop Item named `name` in the home `home`, if any.
fn item_in<'i>(inventory: &'i Inventory, home: &str, name: &str) -> Option<&'i Item> {
    inventory
        .items
        .iter()
        .find(|item| item.agent == "claude-desktop" && item.home == home && item.name == name)
}

#[test]
fn a_package_beside_a_classic_folder_reads_its_own_user_file_and_the_classic_app_the_classic_one() {
    let fs = with_classic_at(store(), Path::new(CLASSIC));
    let inventory = inventory(&fs);

    assert_eq!(home(&inventory, "classic").not_read, None);
    assert_eq!(home(&inventory, "msix").not_read, None);
    let notes = item_in(&inventory, "msix", "notes").unwrap();
    assert_eq!(json(notes.state), "effective");
    assert_eq!(
        notes.entries[0].path,
        msix().join("claude_desktop_config.json")
    );
    assert!(item_in(&inventory, "classic", "notes").is_none());
    let filesystem = item_in(&inventory, "classic", "filesystem").unwrap();
    assert_eq!(json(filesystem.state), "effective");
    assert!(
        item_in(&inventory, "msix", "filesystem").is_none(),
        "the package's own file wins over the classic one"
    );
}

#[test]
fn a_package_beside_a_classic_folder_reads_the_classic_user_file_while_it_holds_none() {
    let fs = with_classic_at(store(), Path::new(CLASSIC));
    fs.delete_file(msix().join("claude_desktop_config.json"));
    let inventory = inventory(&fs);

    let classic_file = PathBuf::from(CLASSIC).join("claude_desktop_config.json");
    for read_by in ["classic", "msix"] {
        let filesystem = item_in(&inventory, read_by, "filesystem").unwrap();
        assert_eq!(json(filesystem.state), "effective", "{read_by}");
        assert_eq!(filesystem.entries[0].path, classic_file, "{read_by}");
    }
    assert!(item_in(&inventory, "msix", "notes").is_none());
}

/// `fs` with the extension `id`, named `name`, in the data folder
/// `folder`, switched on or off in its settings when `enabled` says.
fn with_extension(
    fs: FakeFileSystem,
    folder: &Path,
    id: &str,
    name: &str,
    enabled: Option<bool>,
) -> FakeFileSystem {
    let manifest = serde_json::json!({
        "manifest_version": "0.2",
        "name": name,
        "version": "1.0.0",
        "server": { "type": "binary", "mcp_config": { "command": format!("{name}.exe") } },
    });
    let fs = fs.with_file(
        folder
            .join("Claude Extensions")
            .join(id)
            .join("manifest.json"),
        manifest.to_string(),
    );
    match enabled {
        Some(enabled) => fs.with_file(
            folder
                .join("Claude Extensions Settings")
                .join(format!("{id}.json")),
            serde_json::json!({ "isEnabled": enabled }).to_string(),
        ),
        None => fs,
    }
}

#[test]
fn a_package_beside_a_classic_folder_sees_the_extensions_of_both_its_own_winning() {
    let fs = with_classic_at(store(), Path::new(CLASSIC));
    let classic_folder = Path::new(CLASSIC);
    let fs = with_extension(fs, classic_folder, "local.mcpb.acme.sky", "sky", Some(true));
    let fs = with_extension(
        fs,
        classic_folder,
        "local.mcpb.acme.both",
        "both",
        Some(false),
    );
    let fs = with_extension(fs, &msix(), "local.mcpb.acme.both", "both", Some(true));
    // The package's calendar has no settings of its own.
    let fs = fs.with_file(
        classic_folder.join(r"Claude Extensions Settings\local.dxt.acme.calendar.json"),
        r#"{ "isEnabled": true }"#,
    );
    let inventory = inventory(&fs);

    let classic_extension = |id: &str| {
        classic_folder
            .join("Claude Extensions")
            .join(id)
            .join("manifest.json")
    };
    for read_by in ["classic", "msix"] {
        let sky = item_in(&inventory, read_by, "sky").unwrap();
        assert_eq!(json(sky.state), "effective", "{read_by}");
        assert_eq!(
            sky.entries[0].path,
            classic_extension("local.mcpb.acme.sky")
        );
    }

    let both = item_in(&inventory, "msix", "both").unwrap();
    assert_eq!(json(both.state), "effective");
    assert_eq!(both.entries.len(), 1);
    assert_eq!(
        both.entries[0].path,
        extension("local.mcpb.acme.both").join("manifest.json")
    );
    let classic_both = item_in(&inventory, "classic", "both").unwrap();
    assert_eq!(json(classic_both.state), "disabled");
    assert_eq!(
        classic_both.entries[0].path,
        classic_extension("local.mcpb.acme.both")
    );

    let calendar = item_in(&inventory, "msix", "Calendar").unwrap();
    assert_eq!(json(calendar.state), "effective");
    assert!(item_in(&inventory, "classic", "Calendar").is_none());
    assert!(item_in(&inventory, "classic", "Files").is_none());
}

#[test]
fn a_package_beside_a_classic_folder_sees_the_skill_mirrors_of_both_its_own_winning() {
    let mirror = fixture_folder("msix").join(r"local-agent-mode-sessions\skills-plugin\org-0001");
    let classic_mirrors = Path::new(CLASSIC).join(r"local-agent-mode-sessions\skills-plugin");
    let fs = with_classic_at(store(), Path::new(CLASSIC));
    let fs = load(fs, &mirror, &classic_mirrors.join("org-0001"));
    let fs = load(fs, &mirror, &classic_mirrors.join("org-0002"));
    let inventory = inventory(&fs);

    let paths = |read_by: &str| -> Vec<PathBuf> {
        let pdf = item_in(&inventory, read_by, "pdf").unwrap();
        let mut paths: Vec<PathBuf> = pdf.entries.iter().map(|entry| entry.path.clone()).collect();
        paths.sort();
        paths
    };
    let skill = |mirrors: &Path, org: &str| mirrors.join(org).join(r"account-0001\skills\pdf");
    let own_mirrors = msix().join(r"local-agent-mode-sessions\skills-plugin");
    assert_eq!(
        paths("msix"),
        [
            skill(&own_mirrors, "org-0001"),
            skill(&classic_mirrors, "org-0002")
        ]
    );
    assert_eq!(
        paths("classic"),
        [
            skill(&classic_mirrors, "org-0001"),
            skill(&classic_mirrors, "org-0002")
        ]
    );
}

#[test]
fn a_package_alone_reads_its_own_user_file() {
    let inventory = inventory(&store());

    assert!(!inventory.homes.iter().any(|found| found.home == "classic"));
    let notes = item_in(&inventory, "msix", "notes").unwrap();
    assert_eq!(json(notes.state), "effective");
    assert_eq!(
        notes.entries[0].path,
        msix().join("claude_desktop_config.json")
    );
}

#[test]
fn a_classic_folder_alone_is_read_by_the_classic_home_alone() {
    let inventory = inventory(&classic());

    assert!(!inventory.homes.iter().any(|found| found.home == "msix"));
    let filesystem = item_in(&inventory, "classic", "filesystem").unwrap();
    assert_eq!(json(filesystem.state), "effective");
}

#[test]
fn claude_data_is_read_only_while_app_data_is_on_a_network_share() {
    let claude_data = PathBuf::from(LOCAL_APP_DATA).join("Claude-Data");
    let fs = with_classic_at(store(), &claude_data);

    let local = inventory(&fs);
    let not_read = home(&local, "claude-data").not_read.as_ref().unwrap();
    assert_eq!(not_read.read_instead.as_deref(), Some("msix"));
    assert_eq!(json(item(&local, "filesystem").state), "unread");
    assert_eq!(json(item(&local, "notes").state), "effective");

    let roaming = env().with_var("APPDATA", r"\\fileserver\profiles\fixture\AppData\Roaming");
    let on_share = inventory_with(&fs, roaming);
    assert_eq!(home(&on_share, "claude-data").not_read, None);
    assert_eq!(json(item(&on_share, "filesystem").state), "effective");
    assert_eq!(json(item(&on_share, "notes").state), "unread");
}

#[test]
fn third_party_inference_keeps_a_home_of_its_own_that_is_always_read() {
    let third_party = PathBuf::from(LOCAL_APP_DATA).join("Claude-3p");
    let fs = with_classic_at(store(), &third_party);
    let inventory = inventory(&fs);

    let filesystem = item(&inventory, "filesystem");
    assert_eq!(filesystem.home, "third-party");
    assert_eq!(json(filesystem.state), "effective");
    assert_eq!(json(item(&inventory, "notes").state), "effective");
    assert_eq!(home(&inventory, "third-party").not_read, None);
}

#[test]
fn each_home_says_whether_the_definition_default_or_a_known_location_found_it() {
    let fs = with_classic_at(store(), Path::new(CLASSIC));
    let fs = with_classic_at(fs, &PathBuf::from(LOCAL_APP_DATA).join("Claude-3p"));
    let inventory = inventory(&fs);

    assert_eq!(
        json(&home(&inventory, "classic").source),
        serde_json::json!({"kind": "default"})
    );
    assert_eq!(
        json(&home(&inventory, "msix").source),
        serde_json::json!({"kind": "known_location"})
    );
    assert_eq!(
        json(&home(&inventory, "third-party").source),
        serde_json::json!({"kind": "known_location"})
    );
}

#[test]
fn a_remote_server_in_the_user_file_is_invalid_since_the_file_is_stdio_only() {
    let inventory = inventory(&store());

    let remote = item(&inventory, "remote");
    assert_eq!(json(remote.state), "invalid");
    let reason = remote.entries[0].reason.as_ref().unwrap();
    assert_eq!(json(reason.code), "malformed_entry");
    assert!(reason.sentence.contains("`command`"), "{}", reason.sentence);
}

/// An extension's folder in the Store package's data folder.
fn extension(id: &str) -> PathBuf {
    msix().join("Claude Extensions").join(id)
}

#[test]
fn an_extension_s_server_comes_from_its_manifest_named_by_its_display_name() {
    let inventory = inventory(&store());

    let files = item(&inventory, "Files");
    assert_eq!(json(files.state), "effective");
    let entry = entry_json(&files.entries[0]);
    assert_eq!(
        entry["path"],
        json(extension("local.mcpb.acme.files").join("manifest.json"))
    );
    assert_eq!(
        entry["key_path"],
        serde_json::json!(["server", "mcp_config"])
    );
    assert_eq!(entry["scope"], "user");
    assert_eq!(entry["provenance"], "extension");
    assert_eq!(entry["config"]["command"], "node");
    assert_eq!(entry["plugin"]["id"], "local.mcpb.acme.files");
    assert_eq!(entry["plugin"]["name"], "Files");
    assert_eq!(
        entry["plugin"]["version"], "0.3.1",
        "from the install record"
    );
    assert_eq!(
        entry["plugin"]["folder"],
        json(extension("local.mcpb.acme.files"))
    );
    assert_eq!(entry["remove"]["code"], "extension");
    assert_eq!(entry["remove"]["how_to_remove"], Value::Null);
}

#[test]
fn an_extension_without_a_display_name_goes_by_its_name() {
    let inventory = inventory(&store().with_file(
        extension("local.mcpb.acme.weather").join("manifest.json"),
        r#"{ "name": "forecast", "server": { "mcp_config": { "command": "forecast" } } }"#,
    ));

    assert_eq!(
        item(&inventory, "forecast").entries[0].config["command"],
        "forecast"
    );
}

/// An extension's settings file in the Store package's data folder.
fn extension_settings(id: &str) -> PathBuf {
    msix()
        .join("Claude Extensions Settings")
        .join(format!("{id}.json"))
}

#[test]
fn an_extension_switched_off_in_its_settings_is_disabled_natively() {
    let inventory = inventory(&store());

    let todo = item(&inventory, "Todo");
    assert_eq!(json(todo.state), "disabled");
    let reason = todo.entries[0].reason.as_ref().unwrap();
    assert_eq!(json(reason.code), "disabled_natively");
    assert_eq!(
        reason.switch.as_deref(),
        Some(
            format!(
                "{}#isEnabled",
                extension_settings("local.mcpb.acme.todo").display()
            )
            .as_str()
        )
    );
    assert_eq!(json(item(&inventory, "Files").state), "effective");
}

#[test]
fn an_extension_without_a_settings_file_is_disabled() {
    let inventory = inventory(&store());

    let calendar = item(&inventory, "Calendar");
    assert_eq!(json(calendar.state), "disabled");
    let reason = calendar.entries[0].reason.as_ref().unwrap();
    assert_eq!(json(reason.code), "disabled_natively");
    assert!(reason.sentence.contains("missing"), "{}", reason.sentence);
}

#[test]
fn a_user_file_server_shadows_a_same_named_extension_server_whole() {
    let inventory = inventory(&store());

    let weather = item(&inventory, "weather");
    assert_eq!(json(weather.state), "effective");
    assert_eq!(json(weather.entries[0].provenance), "file");
    assert_eq!(json(weather.entries[1].provenance), "extension");
    assert_eq!(json(weather.entries[1].state), "shadowed");
    assert_eq!(
        json(weather.entries[1].reason.as_ref().unwrap().code),
        "shadowed_by_precedence"
    );
}

#[test]
fn an_extension_on_the_organisation_s_blocklist_is_blocked_by_policy() {
    let inventory = inventory(&store());

    let tomtom = item(&inventory, "TomTom");
    assert_eq!(json(tomtom.state), "blocked");
    let reason = tomtom.entries[0].reason.as_ref().unwrap();
    assert_eq!(json(reason.code), "policy");
    assert_eq!(
        reason.switch.as_deref(),
        Some(
            format!(
                "{}#orgBlockedReason",
                extension_settings("ant.dir.gh.tomtom.tomtom-mcp").display()
            )
            .as_str()
        )
    );
    let secondary: Vec<Value> = tomtom.entries[0]
        .secondary
        .iter()
        .map(|reason| json(reason.code))
        .collect();
    assert_eq!(secondary, ["disabled_natively"]);
}

/// Where Claude Desktop's device policy is, under either hive.
const POLICIES: &str = r"SOFTWARE\Policies\Claude";

#[test]
fn machine_policy_that_turns_off_user_added_servers_blocks_them() {
    let env = env().with_registry_dword(Hive::LocalMachine, POLICIES, "isLocalDevMcpEnabled", 0);
    let inventory = inventory_with(&store(), env);

    let notes = item(&inventory, "notes");
    assert_eq!(json(notes.state), "blocked");
    let reason = notes.entries[0].reason.as_ref().unwrap();
    assert_eq!(json(reason.code), "policy");
    assert_eq!(
        reason.switch.as_deref(),
        Some(r"HKLM\SOFTWARE\Policies\Claude#isLocalDevMcpEnabled")
    );
    assert_eq!(json(item(&inventory, "Files").state), "effective");
    assert_eq!(
        json(item(&inventory, "weather").state),
        "effective",
        "the extension's server is left to win the name"
    );
    assert_eq!(
        json(item(&inventory, "weather").entries[1].provenance),
        "extension"
    );
}

#[test]
fn user_policy_applies_only_while_the_machine_holds_none() {
    let user =
        env().with_registry_dword(Hive::CurrentUser, POLICIES, "isDesktopExtensionEnabled", 0);
    let inventory = inventory_with(&store(), user);
    let files = item(&inventory, "Files");
    assert_eq!(json(files.state), "blocked");
    assert_eq!(
        files.entries[0].reason.as_ref().unwrap().switch.as_deref(),
        Some(r"HKCU\SOFTWARE\Policies\Claude#isDesktopExtensionEnabled")
    );
    assert_eq!(json(item(&inventory, "notes").state), "effective");

    let both = env()
        .with_registry_dword(Hive::CurrentUser, POLICIES, "isDesktopExtensionEnabled", 0)
        .with_registry_dword(Hive::LocalMachine, POLICIES, "isLocalDevMcpEnabled", 1);
    let inventory = inventory_with(&store(), both);
    assert_eq!(json(item(&inventory, "Files").state), "effective");
}

#[test]
fn the_user_file_s_own_switch_disables_its_servers_globally_with_no_enable() {
    let fs = store().with_file(
        msix().join("claude_desktop_config.json"),
        r#"{ "mcpServers": { "notes": { "command": "notes-mcp" } }, "features": { "isLocalDevMcpEnabled": false } }"#,
    );
    let inventory = inventory(&fs);

    let notes = &item(&inventory, "notes").entries[0];
    assert_eq!(json(notes.state), "disabled");
    let reason = notes.reason.as_ref().unwrap();
    assert_eq!(json(reason.code), "disabled_globally");
    assert!(
        matches!(&notes.enable, Some(EnableOption::NotOffered { sentence }) if sentence.starts_with("Enable is not offered")),
        "{:?}",
        notes.enable
    );
    assert_eq!(json(item(&inventory, "Files").state), "effective");
}

#[test]
fn policy_that_allows_user_servers_does_not_lift_the_file_s_switch() {
    let fs = store().with_file(
        msix().join("claude_desktop_config.json"),
        r#"{ "mcpServers": { "notes": { "command": "notes-mcp" } }, "features": { "isLocalDevMcpEnabled": false } }"#,
    );
    let env = env().with_registry_dword(Hive::LocalMachine, POLICIES, "isLocalDevMcpEnabled", 1);
    let inventory = inventory_with(&fs, env);

    let reason = item(&inventory, "notes").entries[0].reason.clone().unwrap();
    assert_eq!(json(reason.code), "disabled_globally");
}

#[test]
fn the_user_file_s_extension_switch_disables_every_extension() {
    let fs = store().with_file(
        msix().join("claude_desktop_config.json"),
        r#"{ "mcpServers": {}, "features": { "isDxtEnabled": false } }"#,
    );
    let inventory = inventory(&fs);

    let files = &item(&inventory, "Files").entries[0];
    assert_eq!(json(files.state), "disabled");
    assert_eq!(
        json(files.reason.as_ref().unwrap().code),
        "disabled_globally"
    );
}

#[test]
fn a_value_claude_desktop_encrypted_is_a_secret_wherever_it_is() {
    let fs = store().with_file(
        msix().join("claude_desktop_config.json"),
        r#"{ "mcpServers": { "notes": { "command": "notes-mcp", "args": ["__encrypted__:djEwAAAAbm90LWEtcmVhbC1zZWNyZXQ="] } } }"#,
    );
    let inventory = inventory(&fs);

    let notes = &item(&inventory, "notes").entries[0];
    assert_eq!(notes.config["args"][0], "__encrypted__:••••");
    assert_eq!(notes.masked, [["args", "0"]]);
    let files = &item(&inventory, "Files").entries[0];
    assert_eq!(files.config["args"][2], "__encrypted__:••••");
    assert!(
        files
            .masked
            .contains(&vec!["args".to_owned(), "2".to_owned()])
    );
    assert_eq!(
        files.config["env"]["FILES_ROOT"], "${user_config.root}",
        "a reference to a value set in the extension's settings is no Secret"
    );
}

/// Where Claude Desktop mirrors the account's skills, for one account.
fn mirrored_skills() -> PathBuf {
    msix().join(r"local-agent-mode-sessions\skills-plugin\org-0001\account-0001")
}

#[test]
fn account_skills_mirrored_on_disk_are_cloud_mirror_and_read_only() {
    let inventory = inventory(&store());

    let pdf = item(&inventory, "pdf");
    assert_eq!(json(pdf.kind), "skill");
    assert_eq!(json(pdf.state), "effective");
    let entry = entry_json(&pdf.entries[0]);
    assert_eq!(entry["path"], json(mirrored_skills().join(r"skills\pdf")));
    assert_eq!(entry["scope"], "user");
    assert_eq!(entry["provenance"], "cloud_mirror");
    assert_eq!(entry["disable"]["code"], "read_only");
    assert_eq!(entry["remove"]["code"], "read_only");
}

/// The Store home with `contents` as the mirror's `SKILL.md` of `pdf`,
/// as a hand edit leaves it (#313).
fn store_with_pdf_skill_md(contents: &[u8]) -> FakeFileSystem {
    store().with_file(
        mirrored_skills().join(r"skills\pdf\SKILL.md"),
        contents.to_vec(),
    )
}

#[test]
fn a_mirrored_skill_whose_frontmatter_is_not_yaml_goes_by_its_folder() {
    let inventory = inventory(&store_with_pdf_skill_md(
        b"---\nname: [unclosed\ndescription: not: yaml\n---\nbody\n",
    ));

    let pdf = item(&inventory, "pdf");
    assert_eq!(json(pdf.state), "effective");
    assert!(inventory.read_failures.is_empty());
}

#[test]
fn a_mirrored_skill_md_that_is_not_utf_8_puts_a_caveat_on_every_mirrored_skill() {
    let inventory = inventory(&store_with_pdf_skill_md(
        b"---\nname: pdf\ndescription: d\n---\n\xff\xfe\n",
    ));

    let failure = &inventory.read_failures[0];
    assert_eq!(failure.path, mirrored_skills().join(r"skills\pdf\SKILL.md"));
    assert_eq!(json(failure.reaction), "unknown");
    let notes = item(&inventory, "my-notes");
    assert_eq!(
        json(&notes.caveats[0])["path"],
        json(mirrored_skills().join(r"skills\pdf\SKILL.md"))
    );
}

#[test]
fn a_mirrored_skill_takes_its_state_from_the_enabled_flag_it_mirrors() {
    let inventory = inventory(&store());

    let notes = &item(&inventory, "my-notes").entries[0];
    assert_eq!(json(notes.state), "disabled");
    let reason = notes.reason.as_ref().unwrap();
    assert_eq!(json(reason.code), "disabled_natively");
    assert_eq!(
        reason.switch.as_deref(),
        Some(
            format!(
                "{}#skills.1.enabled",
                mirrored_skills().join("manifest.json").display()
            )
            .as_str()
        )
    );
    assert!(
        matches!(&notes.enable, Some(EnableOption::NotOffered { sentence }) if sentence.starts_with("Enable is not offered")),
        "{:?}",
        notes.enable
    );
}

#[test]
fn every_home_notes_that_cloud_connectors_are_not_on_disk() {
    let inventory = inventory(&store());

    let notes: Vec<Value> = home(&inventory, "msix")
        .notes
        .iter()
        .map(|note| json(note.code))
        .collect();
    assert!(
        notes.contains(&json("cloud_connectors_not_on_disk")),
        "{notes:?}"
    );
}

/// The Store package's install folder, as Windows names it.
const PACKAGE: &str = r"C:\Program Files\WindowsApps\Claude_2.16120.0.0_x64__pzs8sxrjxfjjc";

/// The Store package's data folder and its install folder, whose manifest
/// gives its version.
fn installed_store() -> FakeFileSystem {
    store()
        .with_file(
            PathBuf::from(PACKAGE).join("AppxManifest.xml"),
            r#"<?xml version="1.0" encoding="utf-8"?>
<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10">
  <Identity Name="Claude" Publisher="CN=Anthropic" Version="2.16120.0.0" ProcessorArchitecture="x64" />
</Package>
"#,
        )
        .with_file(
            PathBuf::from(PACKAGE).join(r"app\claude.exe"),
            fake::executable(Some([2, 16120, 0, 0])),
        )
}

fn with_program_files(env: FakeEnvironment) -> FakeEnvironment {
    env.with_var("ProgramFiles", r"C:\Program Files")
}

#[test]
fn a_package_s_version_comes_from_its_manifest() {
    let inventory = inventory_with(&installed_store(), with_program_files(env()));

    let installations = &home(&inventory, "msix").installations;
    assert_eq!(installations.len(), 1, "{installations:?}");
    assert_eq!(
        installations[0].path,
        PathBuf::from(PACKAGE).join(r"app\claude.exe")
    );
    assert_eq!(installations[0].version.as_deref(), Some("2.16120.0.0"));
    assert!(!installations[0].out_of_range);
}

fn process(pid: u32, path: &str) -> RunningProcess {
    RunningProcess {
        pid,
        image: "claude.exe".to_owned(),
        path: Some(PathBuf::from(path)),
        cmdline: path.to_owned(),
    }
}

/// The running-agent warning of a Remove of `selector` while `running` run.
fn running_warning(running: &[RunningProcess], selector: &str) -> Option<String> {
    let mut ports = fake::ports(installed_store(), with_program_files(env()));
    ports.processes = Box::new(
        running
            .iter()
            .fold(FakeProcesses::default(), |processes, process| {
                processes.with_process(process.clone())
            }),
    );
    let core = Core::new(ports).unwrap();
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    let targets = Targets {
        selectors: vec![selector.to_owned()],
        all: false,
        ..Targets::default()
    };
    let plan = core
        .plan(&snapshot, &Context::default(), Action::Remove, &targets)
        .unwrap();
    plan.warnings()
        .iter()
        .find(|warning| warning.class == WarningClass::RunningAgent)
        .map(|warning| warning.sentence.clone())
}

#[test]
fn claude_desktop_runs_as_claude_exe_from_its_package() {
    let desktop = process(5151, &format!(r"{PACKAGE}\app\claude.exe"));

    let sentence = running_warning(&[desktop], "notes").expect("a running-agent warning");
    assert!(sentence.contains("Claude Desktop"), "{sentence}");
    assert!(
        sentence.contains("claude_desktop_config.json"),
        "{sentence}"
    );
}

#[test]
fn claude_code_is_not_claude_desktop_though_both_are_claude_exe() {
    let claude_code = process(4242, r"C:\Users\fixture\.local\bin\claude.exe");
    let code_tab = process(
        4343,
        &format!(r"{}\claude-code\2.1.286\claude.exe", msix().display()),
    );

    assert_eq!(running_warning(&[claude_code, code_tab], "notes"), None);
}

#[test]
fn a_second_extension_with_the_same_name_goes_by_its_id() {
    let inventory = inventory(&store().with_file(
        extension("local.mcpb.acme.weather").join("manifest.json"),
        r#"{ "name": "files-too", "display_name": "Files", "server": { "mcp_config": { "command": "files-too" } } }"#,
    ));

    let files = item(&inventory, "Files");
    assert_eq!(files.entries.len(), 1);
    assert_eq!(files.entries[0].config["command"], "node");
    let second = item(&inventory, "local.mcpb.acme.weather");
    assert_eq!(json(second.state), "effective");
    assert_eq!(second.entries[0].config["command"], "files-too");
}

#[test]
fn machine_policy_that_turns_off_extensions_blocks_them() {
    let env =
        env().with_registry_dword(Hive::LocalMachine, POLICIES, "isDesktopExtensionEnabled", 0);
    let inventory = inventory_with(&store(), env);

    let files = item(&inventory, "Files");
    assert_eq!(json(files.state), "blocked");
    assert_eq!(
        json(files.entries[0].reason.as_ref().unwrap().code),
        "policy"
    );
    assert_eq!(json(item(&inventory, "notes").state), "effective");
}

#[test]
fn a_machine_policy_value_of_any_type_hides_the_user_s_policy() {
    let env = env()
        .with_registry_dword(Hive::CurrentUser, POLICIES, "isDesktopExtensionEnabled", 0)
        .with_registry_data(
            Hive::LocalMachine,
            POLICIES,
            "skillBundles",
            RegistryData::Other,
        );
    let inventory = inventory_with(&store(), env);

    assert_eq!(json(item(&inventory, "Files").state), "effective");
}

#[test]
fn a_folder_without_a_manifest_is_no_extension() {
    let fs = store().with_file(
        msix().join(r"Claude Extensions\local.mcpb.acme.half\server\index.js"),
        "",
    );
    let inventory = inventory(&fs);

    assert!(
        !inventory
            .items
            .iter()
            .flat_map(|item| &item.entries)
            .any(|entry| entry.path.starts_with(extension("local.mcpb.acme.half")))
    );
}

#[test]
fn a_package_signed_as_anthropic_pbc_is_found_too() {
    let inventory = inventory(&msix_of(ANTHROPIC_PBC_FAMILY));

    let notes = item(&inventory, "notes");
    assert_eq!(notes.home, "msix-anthropicpbc");
    assert_eq!(
        home(&inventory, "msix-anthropicpbc").folder,
        msix_folder(ANTHROPIC_PBC_FAMILY)
    );
}

#[test]
fn each_package_reads_its_own_local_cache_when_both_are_installed() {
    let both = load(
        store(),
        &fixture_folder("msix"),
        &msix_folder(ANTHROPIC_PBC_FAMILY),
    );
    let packages = inventory(&both);

    assert_eq!(home(&packages, "msix").not_read, None);
    assert_eq!(home(&packages, "msix-anthropicpbc").not_read, None);
    let states: Vec<Value> = item(&packages, "notes")
        .entries
        .iter()
        .map(|entry| json(entry.state))
        .collect();
    assert_eq!(states, ["effective"]);
    let items = packages
        .items
        .iter()
        .filter(|item| item.name == "notes" && json(item.state) == "effective")
        .count();
    assert_eq!(items, 2, "one Item per home");

    let with_classic = with_classic_at(both, Path::new(CLASSIC));
    let classic_too = inventory(&with_classic);
    for read in ["classic", "msix", "msix-anthropicpbc"] {
        assert_eq!(home(&classic_too, read).not_read, None, "{read}");
    }
}

#[test]
fn read_instead_names_only_a_home_that_is_listed() {
    let fs = store().with_folder(moved_folder(STORE_FAMILY));
    let inventory = inventory(&fs);

    let not_read = home(&inventory, "msix").not_read.as_ref().unwrap();
    assert_eq!(not_read.read_instead, None);
    let moved = moved_folder(STORE_FAMILY).display().to_string();
    assert!(not_read.sentence.contains(&moved), "{}", not_read.sentence);
}

/// `%LOCALAPPDATA%\Claude-Data`.
fn claude_data() -> PathBuf {
    PathBuf::from(LOCAL_APP_DATA).join("Claude-Data")
}

/// Where the MSIX package of the family `family` moves its data folder:
/// `%LOCALAPPDATA%\Claude-Data\<family>`, the family in lower case.
fn moved_folder(family: &str) -> PathBuf {
    claude_data().join(family.to_lowercase())
}

/// The package of the family `family` with its data folder moved out of
/// its `LocalCache`, as on the reference machine: the MSIX fixture data
/// folder in `Claude-Data\<family>`, and only the marker
/// `.user-data-left.<family>` left in the `LocalCache`.
fn moved_of(fs: FakeFileSystem, family: &str) -> FakeFileSystem {
    let marker = format!(".user-data-left.{}", family.to_lowercase());
    load(fs, &fixture_folder("msix"), &moved_folder(family))
        .with_file(msix_folder(family).join(marker), "")
        .with_file(
            claude_data().join("README.txt"),
            "This folder holds the data Claude keeps for this Windows account.",
        )
}

/// The Store package with its data folder moved.
fn moved() -> FakeFileSystem {
    moved_of(FakeFileSystem::new(), STORE_FAMILY)
}

#[test]
fn a_store_package_that_moved_its_data_folder_reads_it_in_claude_data() {
    let inventory = inventory(&moved());

    let moved_home = home(&inventory, "msix-moved");
    assert_eq!(moved_home.folder, moved_folder(STORE_FAMILY));
    assert_eq!(moved_home.not_read, None);
    let notes = item(&inventory, "notes");
    assert_eq!(notes.home, "msix-moved");
    assert_eq!(json(notes.state), "effective");
    assert_eq!(
        notes.entries[0].path,
        moved_folder(STORE_FAMILY).join("claude_desktop_config.json")
    );
    let files = item(&inventory, "Files");
    assert_eq!(files.home, "msix-moved");
    assert_eq!(json(files.state), "effective");
    let pdf = item(&inventory, "pdf");
    assert_eq!(pdf.home, "msix-moved");
    assert_eq!(
        pdf.entries[0].path,
        moved_folder(STORE_FAMILY)
            .join(r"local-agent-mode-sessions\skills-plugin\org-0001\account-0001\skills\pdf")
    );
    assert!(
        !inventory.homes.iter().any(|found| found.home == "msix"),
        "the `LocalCache` holds only the marker, so it is no home"
    );
}

#[test]
fn a_package_signed_as_anthropic_pbc_that_moved_its_data_folder_is_found_too() {
    let inventory = inventory(&moved_of(FakeFileSystem::new(), ANTHROPIC_PBC_FAMILY));

    let notes = item(&inventory, "notes");
    assert_eq!(notes.home, "msix-anthropicpbc-moved");
    assert_eq!(json(notes.state), "effective");
    assert_eq!(
        home(&inventory, "msix-anthropicpbc-moved").folder,
        PathBuf::from(LOCAL_APP_DATA).join(r"Claude-Data\anthropicpbc.claude_fnn82j28hfe8t")
    );
}

#[test]
fn a_moved_data_folder_comes_before_a_classic_folder_beside_it() {
    let fs = with_classic_at(moved(), Path::new(CLASSIC));
    let inventory = inventory(&fs);

    assert_eq!(home(&inventory, "msix-moved").not_read, None);
    let notes = item(&inventory, "notes");
    assert_eq!(notes.home, "msix-moved");
    assert_eq!(json(notes.state), "effective");
    let filesystem = item(&inventory, "filesystem");
    assert_eq!(filesystem.home, "classic");
    assert_eq!(
        json(filesystem.state),
        "effective",
        "the classic app reads it"
    );
}

#[test]
fn claude_data_on_a_network_share_comes_before_a_moved_data_folder() {
    let fs = with_classic_at(moved(), &claude_data());
    let roaming = env().with_var("APPDATA", r"\\fileserver\profiles\fixture\AppData\Roaming");

    let on_share = inventory_with(&fs, roaming);

    assert_eq!(home(&on_share, "claude-data").not_read, None);
    assert_eq!(json(item(&on_share, "filesystem").state), "effective");
    let not_read = home(&on_share, "msix-moved").not_read.as_ref().unwrap();
    assert_eq!(not_read.read_instead.as_deref(), Some("claude-data"));
    assert_eq!(json(item(&on_share, "notes").state), "unread");

    let local = inventory(&fs);
    assert_eq!(home(&local, "msix-moved").not_read, None);
    assert_eq!(json(item(&local, "notes").state), "effective");
    let not_read = home(&local, "claude-data").not_read.as_ref().unwrap();
    assert_eq!(not_read.read_instead.as_deref(), Some("msix-moved"));
}

/// The Plan of `action` on the Item `name` of the Claude Desktop home `home`.
fn plan_in(core: &Core, action: Action, home: &str, name: &str) -> config_digger_core::api::Plan {
    let targets = Targets {
        selectors: vec![name.to_owned()],
        all: false,
        narrowing: core.narrowing(Some("claude-desktop"), Some(home)).unwrap(),
    };
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    core.plan(&snapshot, &Context::default(), action, &targets)
        .unwrap()
}

#[test]
fn disable_through_a_package_parks_a_classic_server_from_the_classic_file_and_enable_puts_it_back_there()
 {
    let fs = with_classic_at(store(), Path::new(CLASSIC));
    fs.delete_file(msix().join("claude_desktop_config.json"));
    let core = core_with(&fs, env());
    let classic_file = PathBuf::from(CLASSIC).join("claude_desktop_config.json");
    let original = fs.contents(&classic_file).unwrap();
    let in_package = fs.files_below(msix());

    // The Claude Desktop files a plan writes, beside the parked-Entry store.
    let written = |plan: &config_digger_core::api::Plan| -> Vec<PathBuf> {
        plan.preview()
            .files
            .iter()
            .map(|file| file.path.clone())
            .filter(|path| path.starts_with(CLASSIC) || path.starts_with(msix()))
            .collect()
    };
    let disable = plan_in(&core, Action::Disable, "msix", "filesystem");
    assert_eq!(written(&disable), std::slice::from_ref(&classic_file));
    let preview = disable.preview();
    let mut changed: Vec<(&str, Value)> = preview
        .changes
        .iter()
        .map(|change| (change.home.as_str(), json(change.after)))
        .collect();
    changed.sort_by(|a, b| a.0.cmp(b.0));
    assert_eq!(
        changed,
        [("classic", json("disabled")), ("msix", json("disabled"))]
    );
    core.execute(disable, &[]).unwrap();

    let parked = fs.contents(&classic_file).unwrap();
    assert!(!String::from_utf8_lossy(&parked).contains("filesystem"));
    assert_eq!(
        fs.files_below(msix()),
        in_package,
        "nothing is written to the package"
    );
    let disabled = inventory(&fs);
    for read_by in ["classic", "msix"] {
        let filesystem = item_in(&disabled, read_by, "filesystem").unwrap();
        assert_eq!(json(filesystem.state), "disabled", "{read_by}");
    }

    // The package gets a file of its own, which it reads from then on.
    fs.put_file(
        msix().join("claude_desktop_config.json"),
        r#"{ "mcpServers": { "notes": { "command": "notes-mcp" } } }"#,
    );
    let copied = inventory(&fs);
    assert!(item_in(&copied, "msix", "filesystem").is_none());
    assert_eq!(
        json(item_in(&copied, "classic", "filesystem").unwrap().state),
        "disabled"
    );

    let enable = plan_in(&core, Action::Enable, "classic", "filesystem");
    assert_eq!(written(&enable), std::slice::from_ref(&classic_file));
    core.execute(enable, &[]).unwrap();

    assert_eq!(
        serde_json::from_slice::<Value>(&fs.contents(&classic_file).unwrap()).unwrap(),
        serde_json::from_slice::<Value>(&original).unwrap()
    );
    let enabled = inventory(&fs);
    assert_eq!(
        json(item_in(&enabled, "classic", "filesystem").unwrap().state),
        "effective"
    );
    assert!(item_in(&enabled, "msix", "filesystem").is_none());
}

#[test]
fn disable_through_a_package_of_a_classic_extension_writes_the_settings_the_package_reads() {
    let classic_folder = Path::new(CLASSIC);
    let fs = with_classic_at(store(), classic_folder);
    let fs = with_extension(fs, classic_folder, "local.mcpb.acme.sky", "sky", Some(true));
    let own_settings = msix().join(r"Claude Extensions Settings\local.mcpb.acme.sky.json");
    fs.put_file(&own_settings, r#"{ "isEnabled": true }"#);
    let core = core_with(&fs, env());

    let disable = plan_in(&core, Action::Disable, "msix", "sky");
    let written: Vec<&Path> = disable
        .preview()
        .files
        .iter()
        .map(|file| file.path.as_path())
        .collect();
    assert_eq!(written, [own_settings.as_path()]);
    core.execute(disable, &[]).unwrap();

    let after = inventory(&fs);
    assert_eq!(
        json(item_in(&after, "msix", "sky").unwrap().state),
        "disabled"
    );
    assert_eq!(
        json(item_in(&after, "classic", "sky").unwrap().state),
        "effective"
    );
}

#[test]
fn remove_through_a_package_of_a_classic_server_writes_the_classic_file_alone() {
    let fs = with_classic_at(store(), Path::new(CLASSIC));
    fs.delete_file(msix().join("claude_desktop_config.json"));
    let core = core_with(&fs, env());
    let classic_file = PathBuf::from(CLASSIC).join("claude_desktop_config.json");
    let in_package = fs.files_below(msix());

    let remove = plan_in(&core, Action::Remove, "msix", "filesystem");
    let mut changed: Vec<&str> = remove
        .preview()
        .changes
        .iter()
        .map(|change| change.home.as_str())
        .collect();
    changed.sort();
    assert_eq!(changed, ["classic", "msix"]);
    core.execute(remove, &[]).unwrap();

    let removed = fs.contents(&classic_file).unwrap();
    assert!(!String::from_utf8_lossy(&removed).contains("filesystem"));
    assert_eq!(
        fs.files_below(msix()),
        in_package,
        "nothing is written to the package"
    );
    let after = inventory(&fs);
    assert!(item_in(&after, "classic", "filesystem").is_none());
    assert!(item_in(&after, "msix", "filesystem").is_none());
}
