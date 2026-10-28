//! Claude Code's plugins synced from claude.ai through the core `api`, with
//! fake ports: each folder of `plugins\synced\<org>_<account>` is a plugin
//! Claude Code keeps a copy of, read as a read-only cloud mirror with the
//! servers and skills it carries, on unless a settings file turns it off.

use config_digger_core::api::{Context, Core, Entry, Inventory, Item, ScanOptions};
use config_digger_core::ports::fake::{self, FakeEnvironment, FakeFileSystem};
use serde_json::{Value, json};
use std::path::PathBuf;

const HOME: &str = r"C:\Users\fixture";

/// The account folder synced plugins sit in.
const ACCOUNT: &str = r".claude\plugins\synced\org1_acct1";

fn in_home(relative: &str) -> PathBuf {
    PathBuf::from(HOME).join(relative)
}

fn in_account(relative: &str) -> PathBuf {
    in_home(ACCOUNT).join(relative)
}

fn skill_md(name: &str) -> String {
    format!("---\nname: {name}\ndescription: The {name} skill.\n---\n\nDo {name}.\n")
}

/// A Claude Code home signed in to the claude.ai account of [`ACCOUNT`],
/// with the synced plugin folder `demo~g3`, whose manifest names it `demo`,
/// with the skill `notes` and the server `docs`.
fn synced_home() -> FakeFileSystem {
    let account = json!({ "organizationUuid": "org1", "accountUuid": "acct1" });
    FakeFileSystem::new()
        .with_file(
            in_home(".claude.json"),
            json!({ "oauthAccount": account }).to_string(),
        )
        .with_file(
            in_account(r"demo~g3\.claude-plugin\plugin.json"),
            json!({ "name": "demo", "version": "1.2.0" }).to_string(),
        )
        .with_file(
            in_account(r"demo~g3\skills\notes\SKILL.md"),
            skill_md("notes"),
        )
        .with_file(
            in_account(r"demo~g3\.mcp.json"),
            json!({ "mcpServers": { "docs": { "url": "https://docs.example.com/mcp" } } })
                .to_string(),
        )
}

fn inventory_with(fs: &FakeFileSystem, env: FakeEnvironment) -> Inventory {
    let core = Core::new(fake::ports(fs.clone(), env)).unwrap();
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    core.resolve(&snapshot, &Context::default())
}

fn inventory(fs: &FakeFileSystem) -> Inventory {
    inventory_with(fs, FakeEnvironment::with_home(HOME))
}

fn item<'i>(inventory: &'i Inventory, name: &str) -> &'i Item {
    inventory
        .items
        .iter()
        .find(|item| item.name == name)
        .unwrap_or_else(|| panic!("no Item `{name}`"))
}

fn entry_json(entry: &Entry) -> Value {
    serde_json::to_value(entry).unwrap()
}

#[test]
fn a_synced_plugins_skill_is_a_read_only_cloud_mirror_named_by_its_manifest() {
    let inventory = inventory(&synced_home());

    let skill = item(&inventory, "demo:notes");
    let entry = entry_json(&skill.entries[0]);
    assert_eq!(
        entry["path"],
        in_account(r"demo~g3\skills\notes").display().to_string()
    );
    assert_eq!(entry["state"], "effective");
    assert_eq!(entry["scope"], "user");
    assert_eq!(entry["provenance"], "cloud_mirror");
    assert_eq!(entry["plugin"]["id"], "demo@synced");
    assert_eq!(entry["plugin"]["name"], "demo");
    assert_eq!(entry["disable"]["status"], "not_offered");
    assert_eq!(entry["disable"]["code"], "read_only");
    assert_eq!(entry["remove"]["status"], "not_offered");
    assert_eq!(entry["remove"]["code"], "read_only");
}

#[test]
fn a_synced_plugins_servers_are_read_only_cloud_mirrors_too() {
    let fs = synced_home().with_file(
        in_account(r"demo~g3\.claude-plugin\plugin.json"),
        json!({
            "name": "demo",
            "mcpServers": { "search": { "command": "demo-search" } },
        })
        .to_string(),
    );

    let inventory = inventory(&fs);

    for (name, path, key_path) in [
        (
            "plugin:demo:docs",
            in_account(r"demo~g3\.mcp.json"),
            json!(["mcpServers", "docs"]),
        ),
        (
            "plugin:demo:search",
            in_account(r"demo~g3\.claude-plugin\plugin.json"),
            json!(["mcpServers", "search"]),
        ),
    ] {
        let entry = entry_json(&item(&inventory, name).entries[0]);
        assert_eq!(entry["path"], path.display().to_string(), "{name}");
        assert_eq!(entry["key_path"], key_path, "{name}");
        assert_eq!(entry["state"], "effective", "{name}");
        assert_eq!(entry["scope"], "user", "{name}");
        assert_eq!(entry["provenance"], "cloud_mirror", "{name}");
        assert_eq!(entry["plugin"]["id"], "demo@synced", "{name}");
        assert_eq!(entry["disable"]["code"], "read_only", "{name}");
        assert_eq!(entry["remove"]["code"], "read_only", "{name}");
    }
}

/// The names of every Item of the inventory.
fn names(inventory: &Inventory) -> Vec<&str> {
    inventory
        .items
        .iter()
        .map(|item| item.name.as_str())
        .collect()
}

#[test]
fn the_files_claude_code_keeps_beside_synced_plugins_are_no_entries() {
    let marketplaces = json!({ "etag": "x", "rows": [{ "name": "acme" }], "parserVersion": 2 });
    let fs = synced_home()
        .with_file(
            in_home(r".claude\plugins\synced\.bucket-org1_acct1"),
            Vec::new(),
        )
        .with_file(
            in_account("manifest.json"),
            json!({ "lastUpdated": 1, "plugins": [{ "pluginId": "p1", "name": "demo" }] })
                .to_string(),
        )
        .with_file(in_account(".marketplaces.json"), marketplaces.to_string())
        .with_file(in_account(".last-complete-round"), "round-7")
        .with_file(
            in_account("demo~g3.meta.json"),
            json!({ "server_plugin_id": "p1", "marketplace_name": "acme", "installation_preference": "available" })
                .to_string(),
        )
        .with_file(in_account(r"demo~g3\.in_use\4242"), Vec::new());

    let inventory = inventory(&fs);

    assert_eq!(
        names(&inventory),
        ["plugin:demo:docs", "demo:notes"],
        "{:?}",
        inventory.items
    );
    assert_eq!(inventory.read_failures, []);
}

#[test]
fn a_synced_plugin_without_a_manifest_is_named_by_its_folder_up_to_its_generation() {
    let fs = synced_home();
    fs.delete_file(in_account(r"demo~g3\.claude-plugin\plugin.json"));

    let inventory = inventory(&fs);

    let skill = item(&inventory, "demo:notes");
    assert_eq!(entry_json(&skill.entries[0])["plugin"]["id"], "demo@synced");
    assert!(names(&inventory).contains(&"plugin:demo:docs"));
}

/// `fs` with the plugin `demo@acme` installed from a marketplace, which
/// the user settings turn on, carrying the skill `notes` and the server
/// `docs` at another URL than the synced plugin's.
fn with_installed_demo(fs: FakeFileSystem) -> FakeFileSystem {
    let folder = in_home(r".claude\plugins\cache\acme\demo\2.0.0");
    let records = json!({
        "version": 2,
        "plugins": { "demo@acme": [{ "scope": "user", "installPath": folder, "version": "2.0.0" }] },
    });
    let fs = fs
        .with_file(
            in_home(r".claude\plugins\installed_plugins.json"),
            records.to_string(),
        )
        .with_file(
            folder.join(r".claude-plugin\plugin.json"),
            json!({ "name": "demo" }).to_string(),
        )
        .with_file(folder.join(r"skills\notes\SKILL.md"), skill_md("notes"))
        .with_file(
            folder.join(".mcp.json"),
            json!({ "mcpServers": { "docs": { "url": "https://installed.example.com/mcp" } } })
                .to_string(),
        );
    with_user_settings(fs, &json!({ "enabledPlugins": { "demo@acme": true } }))
}

#[test]
fn an_installed_plugin_of_the_same_name_shadows_the_synced_one() {
    let inventory = inventory(&with_installed_demo(synced_home()));

    for name in ["plugin:demo:docs", "demo:notes"] {
        let provenances_and_states: Vec<(String, String)> = item(&inventory, name)
            .entries
            .iter()
            .map(|entry| {
                let entry = entry_json(entry);
                (
                    entry["provenance"].as_str().unwrap().to_owned(),
                    entry["state"].as_str().unwrap().to_owned(),
                )
            })
            .collect();
        assert_eq!(
            provenances_and_states,
            [
                ("plugin".to_owned(), "effective".to_owned()),
                ("cloud_mirror".to_owned(), "shadowed".to_owned()),
            ],
            "{name}"
        );
    }
}

/// A `~/.claude/settings.json` holding `settings`.
fn with_user_settings(fs: FakeFileSystem, settings: &Value) -> FakeFileSystem {
    fs.with_file(in_home(r".claude\settings.json"), settings.to_string())
}

/// The State and reason code of every Entry of `inventory`, by Item name.
fn states(inventory: &Inventory) -> Vec<(String, String, String)> {
    inventory
        .items
        .iter()
        .flat_map(|item| item.entries.iter().map(move |entry| (item, entry)))
        .map(|(item, entry)| {
            let entry = entry_json(entry);
            (
                item.name.clone(),
                entry["state"].as_str().unwrap().to_owned(),
                entry["reason"]["code"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
            )
        })
        .collect()
}

fn state_of(name: &str, state: &str, code: &str) -> (String, String, String) {
    (name.to_owned(), state.to_owned(), code.to_owned())
}

#[test]
fn enabled_plugins_off_for_name_at_synced_disables_the_plugin_and_all_it_carries() {
    let fs = with_user_settings(
        synced_home(),
        &json!({ "enabledPlugins": { "demo@synced": false } }),
    );

    let inventory = inventory(&fs);

    assert_eq!(
        states(&inventory),
        [
            state_of("plugin:demo:docs", "disabled", "plugin_disabled"),
            state_of("demo:notes", "disabled", "plugin_disabled"),
        ]
    );
    for item in &inventory.items {
        let entry = entry_json(&item.entries[0]);
        assert_eq!(entry["disable"]["status"], "not_offered", "{entry}");
        assert_eq!(entry["enable"]["status"], "not_offered", "{entry}");
    }
}

#[test]
fn a_synced_plugin_no_settings_file_names_is_on() {
    let fs = with_user_settings(
        synced_home(),
        &json!({ "enabledPlugins": { "other@synced": false, "demo@acme": false } }),
    );

    let inventory = inventory(&fs);

    assert_eq!(
        states(&inventory),
        [
            state_of("plugin:demo:docs", "effective", ""),
            state_of("demo:notes", "effective", ""),
        ]
    );
}

#[test]
fn sync_claude_ai_plugins_off_disables_every_synced_plugin_globally() {
    let fs = with_user_settings(synced_home(), &json!({ "syncClaudeAiPlugins": false }));

    let inventory = inventory(&fs);

    assert_eq!(
        states(&inventory),
        [
            state_of("plugin:demo:docs", "disabled", "disabled_globally"),
            state_of("demo:notes", "disabled", "disabled_globally"),
        ]
    );
    for item in &inventory.items {
        let entry = entry_json(&item.entries[0]);
        assert_eq!(entry["disable"]["status"], "not_offered", "{entry}");
        assert_eq!(entry["enable"]["status"], "not_offered", "{entry}");
    }
}

#[test]
fn every_claude_code_home_notes_that_cloud_connectors_are_not_on_disk() {
    let inventory = inventory(&synced_home());

    let notes: Vec<(String, String)> = inventory
        .homes
        .iter()
        .filter(|home| home.agent == "claude-code")
        .flat_map(|home| &home.notes)
        .map(|note| {
            let note = serde_json::to_value(note).unwrap();
            (
                note["code"].as_str().unwrap().to_owned(),
                note["sentence"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert!(
        notes.contains(&(
            "cloud_connectors_not_on_disk".to_owned(),
            "Claude Code's connectors live in your account, with no copy on disk, so they are not listed here.".to_owned()
        )),
        "{notes:?}"
    );
}

#[test]
fn a_synced_plugin_whose_manifest_fails_has_every_entry_invalid() {
    let manifest = in_account(r"demo~g3\.claude-plugin\plugin.json");
    for broken in [r#"{ "name": "#, r#"{ "name": "demo", "mcpServers": 42 }"#] {
        let fs = synced_home().with_file(&manifest, broken);

        let inventory = inventory(&fs);

        for name in ["plugin:demo:docs", "demo:notes"] {
            let entry = entry_json(&item(&inventory, name).entries[0]);
            assert_eq!(entry["state"], "invalid", "{broken}: {name}");
            assert_eq!(
                entry["reason"]["code"], "plugin_failed_to_load",
                "{broken}: {name}"
            );
        }
        let failure = inventory
            .read_failures
            .iter()
            .find(|failure| failure.path == manifest)
            .unwrap();
        assert_eq!(
            serde_json::to_value(failure.reaction).unwrap(),
            "fails_plugin",
            "{broken}"
        );
    }
}
