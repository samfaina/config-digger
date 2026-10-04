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
fn a_store_package_keeps_its_data_folder_in_its_local_cache() {
    let inventory = inventory(&store());

    let notes = item(&inventory, "notes");
    assert_eq!(notes.home, "msix");
    assert_eq!(
        notes.entries[0].path,
        msix().join("claude_desktop_config.json")
    );
    assert_eq!(home(&inventory, "msix").folder, msix());
}

/// `fs` with the classic fixture data folder also at `folder`.
fn with_classic_at(fs: FakeFileSystem, folder: &Path) -> FakeFileSystem {
    load(fs, &fixture_folder("classic"), folder)
}

#[test]
fn claude_user_data_dir_names_the_home_claude_desktop_reads_and_skips_the_others() {
    let custom = PathBuf::from(r"E:\claude-data");
    let fs = with_classic_at(store(), &custom);
    let env = env().with_var("CLAUDE_USER_DATA_DIR", r"E:\claude-data");
    let inventory = inventory_with(&fs, env);

    let filesystem = item(&inventory, "filesystem");
    assert_eq!(filesystem.home, "user-data-dir");
    assert_eq!(json(filesystem.state), "effective");
    assert_eq!(home(&inventory, "user-data-dir").folder, custom);
    assert_eq!(home(&inventory, "user-data-dir").not_read, None);

    let skipped = home(&inventory, "msix");
    let not_read = skipped
        .not_read
        .as_ref()
        .expect("the MSIX home is not read");
    assert_eq!(not_read.read_instead.as_deref(), Some("user-data-dir"));
    assert!(
        not_read.sentence.contains(r"E:\claude-data"),
        "{}",
        not_read.sentence
    );
    let notes = item(&inventory, "notes");
    assert_eq!(json(notes.state), "unread");
    let reason = notes.entries[0].reason.as_ref().unwrap();
    assert_eq!(json(reason.code), "lookup_order_skips_home");
    assert_eq!(
        json(&notes.entries[0].disable)["code"],
        "unread",
        "an Unread Entry can only be Removed"
    );
}

#[test]
fn the_classic_folder_comes_before_a_package_s_local_cache() {
    let fs = with_classic_at(store(), Path::new(CLASSIC));
    let inventory = inventory(&fs);

    assert_eq!(json(item(&inventory, "filesystem").state), "effective");
    assert_eq!(home(&inventory, "classic").not_read, None);
    assert_eq!(json(item(&inventory, "notes").state), "unread");
    let not_read = home(&inventory, "msix").not_read.as_ref().unwrap();
    assert_eq!(not_read.read_instead.as_deref(), Some("classic"));
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
fn each_home_says_whether_the_definition_default_a_variable_or_a_known_location_found_it() {
    let fs = with_classic_at(store(), Path::new(CLASSIC));
    let fs = with_classic_at(fs, &PathBuf::from(LOCAL_APP_DATA).join("Claude-3p"));
    let fs = with_classic_at(fs, Path::new(r"E:\claude-data"));
    let env = env().with_var("CLAUDE_USER_DATA_DIR", r"E:\claude-data");
    let inventory = inventory_with(&fs, env);

    assert_eq!(
        json(&home(&inventory, "user-data-dir").source),
        serde_json::json!({"kind": "environment", "variable": "CLAUDE_USER_DATA_DIR"})
    );
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
fn a_home_the_user_adds_where_a_variable_finds_it_keeps_the_variable_as_its_source() {
    let custom = PathBuf::from(r"E:\claude-data");
    let fs = with_classic_at(FakeFileSystem::new(), &custom);
    let env = env().with_var("CLAUDE_USER_DATA_DIR", r"E:\claude-data");
    let core = core_with(&fs, env);

    core.add_home(&custom, "claude-desktop").unwrap();
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    let inventory = core.resolve(&snapshot, &Context::default());

    let homes: Vec<&Home> = inventory
        .homes
        .iter()
        .filter(|home| home.agent == "claude-desktop")
        .collect();
    assert_eq!(homes.len(), 1);
    assert_eq!(homes[0].home, "user-data-dir");
    assert_eq!(
        json(&homes[0].source),
        serde_json::json!({"kind": "environment", "variable": "CLAUDE_USER_DATA_DIR"})
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
    let claude_code_notes = inventory
        .homes
        .iter()
        .filter(|home| home.agent == "claude-code")
        .flat_map(|home| &home.notes)
        .any(|note| json(note.code) == "cloud_connectors_not_on_disk");
    assert!(!claude_code_notes);
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
    for skipped in ["msix", "msix-anthropicpbc"] {
        let not_read = home(&classic_too, skipped).not_read.as_ref().unwrap();
        assert_eq!(not_read.read_instead.as_deref(), Some("classic"));
    }
}

#[test]
fn read_instead_names_only_a_home_that_is_listed() {
    let fs = store().with_folder(CLASSIC);
    let inventory = inventory(&fs);

    let not_read = home(&inventory, "msix").not_read.as_ref().unwrap();
    assert_eq!(not_read.read_instead, None);
    assert!(not_read.sentence.contains(CLASSIC), "{}", not_read.sentence);
}
