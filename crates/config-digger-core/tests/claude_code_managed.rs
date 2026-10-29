//! Claude Code's managed configuration and approval gates through the core
//! `api`, with fake ports: `managed-mcp.json` and `managedMcpServers` in the
//! managed settings (file, `managed-settings.d`, `HKLM` and `HKCU`) read as
//! managed and read-only, an exclusive `managed-mcp.json`, allow and deny
//! lists in managed and user settings, and `.mcp.json` servers that are not
//! approved.

use config_digger_core::api::{
    Action, Context, Core, Entry, Inventory, Item, ScanOptions, Targets,
};
use config_digger_core::ports::Hive;
use config_digger_core::ports::fake::{self, FakeEnvironment, FakeFileSystem};
use serde_json::{Value, json};
use std::path::PathBuf;

const HOME: &str = r"C:\Users\fixture";
const PROGRAM_FILES: &str = r"C:\Program Files";
const PROJECT: &str = r"E:\projects\petme-app";

fn in_home(relative: &str) -> PathBuf {
    PathBuf::from(HOME).join(relative)
}

fn in_program_files(relative: &str) -> PathBuf {
    PathBuf::from(PROGRAM_FILES).join(relative)
}

fn in_project(relative: &str) -> PathBuf {
    PathBuf::from(PROJECT).join(relative)
}

fn env() -> FakeEnvironment {
    FakeEnvironment::with_home(HOME).with_var("ProgramFiles", PROGRAM_FILES)
}

/// A Claude Code home whose `~/.claude.json` is `state`.
fn claude_home(state: &Value) -> FakeFileSystem {
    FakeFileSystem::new().with_file(in_home(".claude.json"), state.to_string())
}

fn inventory_with(fs: &FakeFileSystem, env: FakeEnvironment, context: &Context) -> Inventory {
    let core = Core::new(fake::ports(fs.clone(), env)).unwrap();
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    core.resolve(&snapshot, context)
}

fn inventory(fs: &FakeFileSystem) -> Inventory {
    inventory_with(fs, env(), &Context::default())
}

fn server<'i>(inventory: &'i Inventory, name: &str) -> &'i Item {
    inventory
        .items
        .iter()
        .find(|item| item.name == name)
        .unwrap_or_else(|| panic!("no server `{name}`"))
}

fn entry_json(entry: &Entry) -> Value {
    serde_json::to_value(entry).unwrap()
}

#[test]
fn managed_mcp_json_servers_are_managed_and_read_only() {
    let fs = claude_home(&json!({})).with_file(
        in_program_files(r"ClaudeCode\managed-mcp.json"),
        json!({ "mcpServers": { "corp-search": { "type": "http", "url": "https://mcp.corp.example/search" } } })
            .to_string(),
    );

    let inventory = inventory(&fs);

    let entry = entry_json(&server(&inventory, "corp-search").entries[0]);
    assert_eq!(
        entry["locator"],
        r"C:\Program Files\ClaudeCode\managed-mcp.json#mcpServers.corp-search"
    );
    assert_eq!(entry["scope"], "managed");
    assert_eq!(entry["provenance"], "managed");
    assert_eq!(entry["state"], "effective");
    assert_eq!(entry["disable"]["code"], "read_only");
}

/// A Claude Code home with user servers `notes` and `corp-search`, and a
/// `managed-mcp.json` declaring `corp-search`.
fn exclusive_home() -> FakeFileSystem {
    claude_home(&json!({ "mcpServers": {
        "notes": { "command": "notes-mcp" },
        "corp-search": { "command": "my-search" },
    } }))
    .with_file(
        in_program_files(r"ClaudeCode\managed-mcp.json"),
        json!({ "mcpServers": { "corp-search": { "type": "http", "url": "https://mcp.corp.example/search" } } })
            .to_string(),
    )
}

#[test]
fn an_exclusive_managed_mcp_json_blocks_every_other_source_by_policy() {
    let inventory = inventory(&exclusive_home());

    let notes = server(&inventory, "notes");
    assert_eq!(serde_json::to_value(notes.state).unwrap(), "blocked");
    let entry = entry_json(&notes.entries[0]);
    assert_eq!(entry["state"], "blocked");
    assert_eq!(
        entry["reason"],
        json!({
            "code": "policy",
            "effective": null,
            "switch": r"C:\Program Files\ClaudeCode\managed-mcp.json",
            "sentence": r"Blocked by policy: C:\Program Files\ClaudeCode\managed-mcp.json takes exclusive control, so Claude Code loads only the servers its administrator manages.",
        })
    );
    assert_eq!(entry["enable"], Value::Null);
    assert_eq!(
        entry["what_if"],
        json!({
            "state": "effective",
            "effective": null,
            "sentence": "Blocked: would otherwise be Effective.",
        })
    );
}

#[test]
fn a_server_blocked_by_an_exclusive_managed_mcp_json_is_not_shadowed() {
    let inventory = inventory(&exclusive_home());

    let corp = server(&inventory, "corp-search");
    let states: Vec<Value> = corp
        .entries
        .iter()
        .map(|entry| entry_json(entry)["state"].clone())
        .collect();
    assert_eq!(states, [json!("effective"), json!("blocked")]);
    let user = entry_json(&corp.entries[1]);
    assert_eq!(user["reason"]["code"], "policy");
    // It fails precedence too, which is only a secondary reason.
    assert_eq!(user["secondary"][0]["code"], "shadowed_by_precedence");
    assert_eq!(
        user["what_if"]["sentence"],
        r"Blocked: would otherwise be Shadowed by the managed Entry C:\Program Files\ClaudeCode\managed-mcp.json#mcpServers.corp-search."
    );
}

#[test]
fn an_empty_managed_mcp_json_still_takes_exclusive_control() {
    let fs = claude_home(&json!({ "mcpServers": { "notes": { "command": "notes-mcp" } } }))
        .with_file(in_program_files(r"ClaudeCode\managed-mcp.json"), "{}");

    let inventory = inventory(&fs);

    let entry = entry_json(&server(&inventory, "notes").entries[0]);
    assert_eq!(entry["state"], "blocked");
    assert_eq!(entry["reason"]["code"], "policy");
}

const POLICIES: &str = r"SOFTWARE\Policies\ClaudeCode";

/// Managed settings declaring the server `name`, as JSON text.
fn managed_server(name: &str, url: &str) -> String {
    json!({ "managedMcpServers": { name: { "type": "http", "url": url } } }).to_string()
}

fn states(item: &Item) -> Vec<(String, String)> {
    item.entries
        .iter()
        .map(|entry| {
            let entry = entry_json(entry);
            (
                entry["locator"].as_str().unwrap().to_owned(),
                entry["state"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

#[test]
fn managed_settings_files_declare_managed_servers() {
    let fs = claude_home(&json!({}))
        .with_file(
            in_program_files(r"ClaudeCode\managed-settings.json"),
            managed_server("wiki", "https://mcp.corp.example/wiki"),
        )
        .with_file(
            in_program_files(r"ClaudeCode\managed-settings.d\20-tickets.json"),
            managed_server("tickets", "https://mcp.corp.example/tickets"),
        )
        .with_file(
            in_program_files(r"ClaudeCode\managed-settings.d\notes.txt"),
            managed_server("ignored", "https://mcp.corp.example/ignored"),
        );

    let inventory = inventory(&fs);

    for (name, locator) in [
        (
            "wiki",
            r"C:\Program Files\ClaudeCode\managed-settings.json#managedMcpServers.wiki",
        ),
        (
            "tickets",
            r"C:\Program Files\ClaudeCode\managed-settings.d\20-tickets.json#managedMcpServers.tickets",
        ),
    ] {
        let entry = entry_json(&server(&inventory, name).entries[0]);
        assert_eq!(entry["locator"], locator);
        assert_eq!(entry["scope"], "managed");
        assert_eq!(entry["provenance"], "managed");
        assert_eq!(entry["state"], "effective");
        assert_eq!(entry["disable"]["code"], "read_only");
    }
    assert!(!inventory.items.iter().any(|item| item.name == "ignored"));
}

#[test]
fn the_registry_policy_declares_managed_servers() {
    let env = env().with_registry_value(
        Hive::LocalMachine,
        POLICIES,
        "Settings",
        &managed_server("wiki", "https://mcp.corp.example/wiki"),
    );

    let inventory = inventory_with(&claude_home(&json!({})), env, &Context::default());

    let entry = entry_json(&server(&inventory, "wiki").entries[0]);
    assert_eq!(
        entry["locator"],
        r"HKLM\SOFTWARE\Policies\ClaudeCode\Settings#managedMcpServers.wiki"
    );
    assert_eq!(entry["provenance"], "managed");
    assert_eq!(entry["state"], "effective");
}

const USER_WRITABLE_SOURCE_REASON: &str = r"Claude Code never reads `managedMcpServers` from HKCU\SOFTWARE\Policies\ClaudeCode\Settings, which the user can write.";

#[test]
fn only_the_first_managed_source_holding_settings_applies() {
    let fs = claude_home(&json!({})).with_file(
        in_program_files(r"ClaudeCode\managed-settings.json"),
        managed_server("wiki", "https://mcp.corp.example/wiki"),
    );
    let env = env()
        .with_registry_value(
            Hive::LocalMachine,
            POLICIES,
            "Settings",
            &managed_server("wiki", "https://mcp.corp.example/wiki-v2"),
        )
        .with_registry_value(
            Hive::CurrentUser,
            POLICIES,
            "Settings",
            &managed_server("mine", "https://mcp.example/mine"),
        );

    let inventory = inventory_with(&fs, env, &Context::default());

    let wiki = server(&inventory, "wiki");
    assert_eq!(
        states(wiki),
        [
            (
                r"HKLM\SOFTWARE\Policies\ClaudeCode\Settings#managedMcpServers.wiki".to_owned(),
                "effective".to_owned()
            ),
            (
                r"C:\Program Files\ClaudeCode\managed-settings.json#managedMcpServers.wiki"
                    .to_owned(),
                "unread".to_owned()
            ),
        ]
    );
    let skipped = entry_json(&wiki.entries[1]);
    assert_eq!(skipped["reason"]["code"], "source_skipped");
    assert_eq!(
        skipped["reason"]["sentence"],
        r"Claude Code never reads it: it reads its managed settings only from the first source that holds any, which is HKLM\SOFTWARE\Policies\ClaudeCode\Settings."
    );
    // Skipped as the source after the one Claude Code reads, it is never
    // read from the user's source in any case, which the reason says.
    let mine = entry_json(&server(&inventory, "mine").entries[0]);
    assert_eq!(mine["state"], "unread");
    assert_eq!(mine["reason"]["code"], "user_writable_source");
    assert_eq!(mine["reason"]["sentence"], USER_WRITABLE_SOURCE_REASON);
}

#[test]
fn managed_servers_of_the_user_registry_policy_are_never_read() {
    let fs = claude_home(&json!({}))
        .with_file(in_program_files(r"ClaudeCode\managed-settings.json"), "{}");
    let env = env().with_registry_value(
        Hive::CurrentUser,
        POLICIES,
        "Settings",
        &managed_server("mine", "https://mcp.example/mine"),
    );

    let inventory = inventory_with(&fs, env, &Context::default());

    let entry = entry_json(&server(&inventory, "mine").entries[0]);
    assert_eq!(
        entry["locator"],
        r"HKCU\SOFTWARE\Policies\ClaudeCode\Settings#managedMcpServers.mine"
    );
    assert_eq!(entry["state"], "unread");
    assert_eq!(entry["reason"]["code"], "user_writable_source");
    assert_eq!(entry["reason"]["sentence"], USER_WRITABLE_SOURCE_REASON);
}

#[test]
fn an_exclusive_managed_mcp_json_leaves_managed_settings_servers_alone() {
    let fs = exclusive_home().with_file(
        in_program_files(r"ClaudeCode\managed-settings.json"),
        managed_server("wiki", "https://mcp.corp.example/wiki"),
    );

    let inventory = inventory(&fs);

    assert_eq!(
        entry_json(&server(&inventory, "wiki").entries[0])["state"],
        "effective"
    );
}

#[test]
fn a_registry_policy_that_does_not_parse_is_a_read_failure() {
    let env = env().with_registry_value(Hive::LocalMachine, POLICIES, "Settings", "{ not json");

    let inventory = inventory_with(&claude_home(&json!({})), env, &Context::default());

    let failure = serde_json::to_value(&inventory.read_failures).unwrap();
    assert_eq!(
        failure[0]["path"],
        r"HKLM\SOFTWARE\Policies\ClaudeCode\Settings"
    );
    assert_eq!(failure[0]["kind"], "parse");
}

/// The reaction of each Read failure, and the codes of the Agent flags.
fn reactions(inventory: &Inventory) -> (Vec<String>, Vec<String>) {
    let inventory = serde_json::to_value(inventory).unwrap();
    let strings = |list: &Value, field: &str| {
        list.as_array()
            .unwrap()
            .iter()
            .map(|value| value[field].as_str().unwrap().to_owned())
            .collect()
    };
    (
        strings(&inventory["read_failures"], "reaction"),
        strings(&inventory["agent_flags"], "code"),
    )
}

#[test]
fn a_managed_mcp_json_that_does_not_parse_is_skipped_and_keeps_exclusive_control() {
    let fs = claude_home(&json!({ "mcpServers": { "notes": { "command": "notes-mcp" } } }))
        .with_file(
            in_program_files(r"ClaudeCode\managed-mcp.json"),
            "{ not json",
        )
        .with_file(
            in_program_files(r"ClaudeCode\managed-settings.json"),
            managed_server("wiki", "https://mcp.corp.example/wiki"),
        );

    let inventory = inventory(&fs);

    assert_eq!(
        reactions(&inventory),
        (vec!["ignores_file".to_owned()], vec![])
    );
    assert_eq!(
        entry_json(&server(&inventory, "notes").entries[0])["state"],
        "blocked"
    );
    assert_eq!(
        entry_json(&server(&inventory, "wiki").entries[0])["state"],
        "effective"
    );
}

#[test]
fn an_admin_managed_settings_source_that_does_not_parse_stops_claude_code() {
    let unparsable = || {
        claude_home(&json!({})).with_file(
            in_program_files(r"ClaudeCode\managed-settings.json"),
            "{ not json",
        )
    };
    let drop_in = claude_home(&json!({})).with_file(
        in_program_files(r"ClaudeCode\managed-settings.d\10-corp.json"),
        "{ not json",
    );
    let hklm = env().with_registry_value(Hive::LocalMachine, POLICIES, "Settings", "{ not json");

    let stopped = (
        vec!["stops_agent".to_owned()],
        vec!["cannot_start".to_owned()],
    );
    assert_eq!(reactions(&inventory(&unparsable())), stopped);
    assert_eq!(reactions(&inventory(&drop_in)), stopped);
    assert_eq!(
        reactions(&inventory_with(
            &claude_home(&json!({})),
            hklm,
            &Context::default()
        )),
        stopped
    );
}

#[test]
fn managed_servers_of_another_shape_do_not_stop_claude_code() {
    let managed = |text: &str| {
        claude_home(&json!({}))
            .with_file(in_program_files(r"ClaudeCode\managed-settings.json"), text)
    };

    let unknown = (vec!["unknown".to_owned()], vec![]);
    assert_eq!(
        reactions(&inventory(&managed(r#"{"managedMcpServers": []}"#))),
        unknown
    );
    assert_eq!(reactions(&inventory(&managed("[]"))), unknown);
}

#[test]
fn a_user_registry_policy_that_does_not_parse_is_skipped() {
    let env = env().with_registry_value(Hive::CurrentUser, POLICIES, "Settings", "{ not json");

    let inventory = inventory_with(&claude_home(&json!({})), env, &Context::default());

    assert_eq!(
        reactions(&inventory),
        (vec!["ignores_file".to_owned()], vec![])
    );
}

/// A Claude Code home with the user servers `notes`, a stdio server, and
/// `linear`, a remote one.
fn servers_home() -> FakeFileSystem {
    claude_home(&json!({ "mcpServers": {
        "notes": { "command": "npx", "args": ["-y", "notes-mcp"] },
        "linear": { "type": "http", "url": "https://mcp.linear.app/mcp" },
    } }))
}

fn managed_settings(settings: Value) -> (PathBuf, String) {
    (
        in_program_files(r"ClaudeCode\managed-settings.json"),
        settings.to_string(),
    )
}

fn user_settings(settings: Value) -> (PathBuf, String) {
    (in_home(r".claude\settings.json"), settings.to_string())
}

fn with(fs: FakeFileSystem, (path, text): (PathBuf, String)) -> FakeFileSystem {
    fs.with_file(path, text)
}

fn state_and_code(inventory: &Inventory, name: &str) -> (String, String) {
    let entry = entry_json(&server(inventory, name).entries[0]);
    (
        entry["state"].as_str().unwrap().to_owned(),
        entry["reason"]["code"].as_str().unwrap_or("").to_owned(),
    )
}

fn pair(state: &str, code: &str) -> (String, String) {
    (state.to_owned(), code.to_owned())
}

#[test]
fn a_managed_deny_list_blocks_by_policy() {
    let fs = with(
        servers_home(),
        managed_settings(json!({ "deniedMcpServers": [{ "serverName": "notes" }] })),
    );

    let inventory = inventory(&fs);

    let entry = entry_json(&server(&inventory, "notes").entries[0]);
    assert_eq!(
        entry["reason"],
        json!({
            "code": "policy",
            "effective": null,
            "switch": r"C:\Program Files\ClaudeCode\managed-settings.json#deniedMcpServers",
            "sentence": r"Blocked by policy: `deniedMcpServers` in C:\Program Files\ClaudeCode\managed-settings.json names it by name, so Claude Code does not load it.",
        })
    );
    assert_eq!(entry["state"], "blocked");
    assert_eq!(entry["enable"], Value::Null);
    assert_eq!(state_and_code(&inventory, "linear"), pair("effective", ""));
}

#[test]
fn the_user_registry_policy_still_applies_its_other_settings() {
    let env = env().with_registry_value(
        Hive::CurrentUser,
        POLICIES,
        "Settings",
        &json!({
            "deniedMcpServers": [{ "serverName": "notes" }],
            "managedMcpServers": { "mine": { "type": "http", "url": "https://mcp.example/mine" } },
        })
        .to_string(),
    );

    let inventory = inventory_with(&servers_home(), env, &Context::default());

    let notes = entry_json(&server(&inventory, "notes").entries[0]);
    assert_eq!(notes["state"], "blocked");
    assert_eq!(
        notes["reason"]["switch"],
        r"HKCU\SOFTWARE\Policies\ClaudeCode\Settings#deniedMcpServers"
    );
    assert_eq!(state_and_code(&inventory, "linear"), pair("effective", ""));
    assert_eq!(
        state_and_code(&inventory, "mine"),
        pair("unread", "user_writable_source")
    );
}

#[test]
fn a_deny_list_in_a_user_file_disables_by_rule_without_enable() {
    let fs = with(
        servers_home(),
        user_settings(json!({ "deniedMcpServers": [{ "serverName": "notes" }] })),
    );

    let inventory = inventory(&fs);

    let entry = entry_json(&server(&inventory, "notes").entries[0]);
    assert_eq!(entry["state"], "disabled");
    assert_eq!(
        entry["reason"],
        json!({
            "code": "disabled_by_rule",
            "effective": null,
            "switch": r"C:\Users\fixture\.claude\settings.json#deniedMcpServers",
            "sentence": r"Disabled by rule: `deniedMcpServers` in C:\Users\fixture\.claude\settings.json names it by name, so Claude Code skips it.",
        })
    );
    assert_eq!(entry["enable"]["status"], "not_offered");
    assert_eq!(
        entry["enable"]["sentence"],
        r"Enable is not offered: `deniedMcpServers` in C:\Users\fixture\.claude\settings.json is a rule config-digger never lifts; change it there by hand."
    );
    assert_eq!(entry["disable"]["code"], "already_disabled");
    assert_eq!(
        entry["what_if"]["sentence"],
        "Disabled: would otherwise be Effective."
    );
}

#[test]
fn enable_of_an_entry_disabled_by_rule_is_refused() {
    let fs = with(
        servers_home(),
        user_settings(json!({ "deniedMcpServers": [{ "serverName": "notes" }] })),
    );
    let core = Core::new(fake::ports(fs, env())).unwrap();
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    let targets = Targets {
        selectors: vec!["notes".to_owned()],
        all: false,
        ..Targets::default()
    };

    let error = core
        .plan(&snapshot, &Context::default(), Action::Enable, &targets)
        .err()
        .expect("Enable is refused");

    assert_eq!(serde_json::to_value(error.code).unwrap(), "blocked");
    assert!(
        error
            .message
            .contains("is a rule config-digger never lifts"),
        "{}",
        error.message
    );
}

#[test]
fn a_deny_list_matches_by_command_and_by_url_pattern() {
    let fs = with(
        servers_home(),
        managed_settings(json!({ "deniedMcpServers": [
            { "serverCommand": ["npx", "-y", "notes-mcp"] },
            { "serverUrl": "https://*.linear.app/*" },
        ] })),
    );

    let inventory = inventory(&fs);

    assert_eq!(
        state_and_code(&inventory, "notes"),
        pair("blocked", "policy")
    );
    assert_eq!(
        state_and_code(&inventory, "linear"),
        pair("blocked", "policy")
    );
    let linear = entry_json(&server(&inventory, "linear").entries[0]);
    assert!(
        linear["reason"]["sentence"]
            .as_str()
            .unwrap()
            .contains("names it by URL"),
        "{linear}"
    );
}

#[test]
fn a_deny_list_item_matches_only_the_whole_command() {
    let fs = with(
        servers_home(),
        managed_settings(json!({ "deniedMcpServers": [
            { "serverCommand": ["npx", "notes-mcp"] },
            { "serverUrl": "https://mcp.linear.app" },
        ] })),
    );

    let inventory = inventory(&fs);

    assert_eq!(state_and_code(&inventory, "notes"), pair("effective", ""));
    assert_eq!(state_and_code(&inventory, "linear"), pair("effective", ""));
}

#[test]
fn a_managed_allow_list_blocks_what_it_does_not_name_by_policy() {
    let fs = with(
        servers_home(),
        managed_settings(json!({ "allowedMcpServers": [{ "serverName": "linear" }] })),
    );

    let inventory = inventory(&fs);

    assert_eq!(state_and_code(&inventory, "linear"), pair("effective", ""));
    let notes = entry_json(&server(&inventory, "notes").entries[0]);
    assert_eq!(notes["state"], "blocked");
    assert_eq!(
        notes["reason"]["sentence"],
        r"Blocked by policy: `allowedMcpServers` in C:\Program Files\ClaudeCode\managed-settings.json names nothing that matches it, so Claude Code does not load it."
    );
}

#[test]
fn an_allow_list_in_a_user_file_disables_by_rule() {
    let fs = with(
        servers_home(),
        user_settings(json!({ "allowedMcpServers": [{ "serverName": "linear" }] })),
    );

    let inventory = inventory(&fs);

    assert_eq!(
        state_and_code(&inventory, "notes"),
        pair("disabled", "disabled_by_rule")
    );
    assert_eq!(state_and_code(&inventory, "linear"), pair("effective", ""));
}

#[test]
fn allow_lists_merge_and_an_empty_one_allows_nothing() {
    let merged = with(
        with(
            servers_home(),
            managed_settings(json!({ "allowedMcpServers": [{ "serverName": "linear" }] })),
        ),
        user_settings(json!({ "allowedMcpServers": [{ "serverName": "notes" }] })),
    );
    let empty = with(
        servers_home(),
        managed_settings(json!({ "allowedMcpServers": [] })),
    );

    let merged = inventory(&merged);
    let empty = inventory(&empty);

    assert_eq!(state_and_code(&merged, "notes"), pair("effective", ""));
    assert_eq!(state_and_code(&merged, "linear"), pair("effective", ""));
    assert_eq!(state_and_code(&empty, "notes"), pair("blocked", "policy"));
    assert_eq!(state_and_code(&empty, "linear"), pair("blocked", "policy"));
}

#[test]
fn an_allow_list_naming_commands_allows_a_server_with_a_command_only_by_it() {
    let fs = with(
        servers_home(),
        managed_settings(json!({ "allowedMcpServers": [
            { "serverName": "notes" },
            { "serverName": "linear" },
            { "serverCommand": ["npx", "-y", "other-mcp"] },
        ] })),
    );

    let inventory = inventory(&fs);

    assert_eq!(
        state_and_code(&inventory, "notes"),
        pair("blocked", "policy")
    );
    // A remote server passes by name while no item names one by URL.
    assert_eq!(state_and_code(&inventory, "linear"), pair("effective", ""));
}

#[test]
fn a_deny_list_in_a_managed_source_claude_code_skips_does_not_apply() {
    let fs = with(
        servers_home(),
        managed_settings(json!({ "deniedMcpServers": [{ "serverName": "notes" }] })),
    );
    let env = env().with_registry_value(
        Hive::LocalMachine,
        POLICIES,
        "Settings",
        &json!({ "deniedMcpServers": [{ "serverName": "linear" }] }).to_string(),
    );

    let inventory = inventory_with(&fs, env, &Context::default());

    assert_eq!(state_and_code(&inventory, "notes"), pair("effective", ""));
    assert_eq!(
        state_and_code(&inventory, "linear"),
        pair("blocked", "policy")
    );
}

#[test]
fn a_rule_file_that_cannot_be_read_is_a_caveat() {
    let fs = servers_home().with_file(in_home(r".claude\settings.json"), "{ not json");

    let inventory = inventory(&fs);

    let entry = entry_json(&server(&inventory, "notes").entries[0]);
    assert_eq!(entry["state"], "effective");
    let caveats = entry["caveats"].as_array().unwrap();
    assert!(
        caveats.contains(&json!({
            "code": "setting_read_failure",
            "path": r"C:\Users\fixture\.claude\settings.json",
            "sentence": r"C:\Users\fixture\.claude\settings.json couldn't be read, so whether a rule or gate there keeps this Entry out is not known: it is shown as if none did.",
        })),
        "{entry}"
    );
}

/// A Claude Code home that recorded the Project as `E:/projects/petme-app`
/// with `project_state`, the user trusting it so that its own approvals
/// count, whose `.mcp.json` declares `supabase` and `notes`.
fn project_home(mut project_state: Value) -> FakeFileSystem {
    project_state["hasTrustDialogAccepted"] = json!(true);
    claude_home(&json!({
        "mcpServers": { "notes": { "command": "notes-mcp" } },
        "projects": { "E:/projects/petme-app": project_state },
    }))
    .with_file(
        in_project(".mcp.json"),
        json!({ "mcpServers": {
            "supabase": { "type": "http", "url": "https://mcp.supabase.com/mcp" },
            "notes": { "command": "notes-mcp" },
        } })
        .to_string(),
    )
}

fn in_petme(fs: &FakeFileSystem) -> Inventory {
    let context = Context {
        project: Some(PathBuf::from(PROJECT)),
    };
    inventory_with(fs, env(), &context)
}

fn project_entry(inventory: &Inventory, name: &str) -> Value {
    let item = server(inventory, name);
    let entry = item
        .entries
        .iter()
        .find(|entry| entry.path == in_project(".mcp.json"))
        .unwrap_or_else(|| panic!("no `.mcp.json` Entry of `{name}`"));
    entry_json(entry)
}

#[test]
fn a_project_server_that_is_not_approved_is_blocked() {
    let inventory = in_petme(&project_home(json!({})));

    let supabase = project_entry(&inventory, "supabase");
    assert_eq!(supabase["state"], "blocked");
    assert_eq!(
        supabase["reason"],
        json!({
            "code": "not_approved",
            "effective": null,
            "switch": null,
            "sentence": r"Blocked: Claude Code has not approved `supabase` from E:\projects\petme-app\.mcp.json. Approve it in Claude Code, which asks about it when it starts in this Project; config-digger never approves servers.",
        })
    );
    assert_eq!(supabase["enable"], Value::Null);
    // The user `notes` is Effective in its place, and has no approval to wait for.
    let notes = server(&inventory, "notes");
    let states: Vec<Value> = notes
        .entries
        .iter()
        .map(|entry| entry_json(entry)["state"].clone())
        .collect();
    assert_eq!(states, [json!("blocked"), json!("effective")]);
}

#[test]
fn a_project_server_is_approved_by_name_or_all_at_once_in_any_settings_file() {
    let approvals = [
        (
            in_project(r".claude\settings.local.json"),
            json!({ "enabledMcpjsonServers": ["supabase"] }),
        ),
        (
            in_project(r".claude\settings.json"),
            json!({ "enableAllProjectMcpServers": true }),
        ),
        (
            in_home(r".claude\settings.json"),
            json!({ "enabledMcpjsonServers": ["supabase"] }),
        ),
        (
            in_program_files(r"ClaudeCode\managed-settings.json"),
            json!({ "enableAllProjectMcpServers": true }),
        ),
    ];
    for (path, settings) in approvals {
        let fs = project_home(json!({})).with_file(&path, settings.to_string());

        let inventory = in_petme(&fs);

        assert_eq!(
            project_entry(&inventory, "supabase")["state"],
            "effective",
            "{}",
            path.display()
        );
    }
}

#[test]
fn a_project_server_is_approved_by_the_managed_settings_in_an_untrusted_project() {
    let fs = project_home(json!({}))
        .with_file(
            in_home(".claude.json"),
            json!({ "projects": { "E:/projects/petme-app": {} } }).to_string(),
        )
        .with_file(
            in_program_files(r"ClaudeCode\managed-settings.json"),
            json!({ "enabledMcpjsonServers": ["supabase"] }).to_string(),
        );

    let inventory = in_petme(&fs);

    assert_eq!(project_entry(&inventory, "supabase")["state"], "effective");
}

#[test]
fn a_project_server_is_approved_in_the_project_state_claude_code_keeps() {
    let fs = project_home(json!({ "enabledMcpjsonServers": ["supabase"] }));

    let inventory = in_petme(&fs);

    assert_eq!(project_entry(&inventory, "supabase")["state"], "effective");
    assert_eq!(project_entry(&inventory, "notes")["state"], "blocked");
}

#[test]
fn a_disabled_project_server_that_is_not_approved_would_be_blocked_once_enabled() {
    let fs = project_home(json!({})).with_file(
        in_project(r".claude\settings.local.json"),
        json!({ "disabledMcpjsonServers": ["supabase"] }).to_string(),
    );

    let inventory = in_petme(&fs);

    let supabase = project_entry(&inventory, "supabase");
    assert_eq!(supabase["state"], "disabled");
    assert_eq!(supabase["secondary"][0]["code"], "not_approved");
    assert_eq!(
        supabase["what_if"],
        json!({
            "state": "blocked",
            "effective": null,
            "sentence": "Disabled: would be Blocked once Enabled.",
        })
    );
}

/// A Claude Code home signed in to a claude.ai account, with the skill
/// `pdf` synced from it.
fn synced_home() -> FakeFileSystem {
    let account = json!({ "organizationUuid": "org1", "accountUuid": "acct1" });
    claude_home(&json!({ "oauthAccount": account })).with_file(
        in_home(r".claude\skills\synced\org1_acct1\pdf\SKILL.md"),
        "---\nname: pdf\ndescription: The pdf skill.\n---\n",
    )
}

#[test]
fn a_global_switch_in_a_user_file_disables_globally_without_enable() {
    let fs = with(
        synced_home(),
        user_settings(json!({ "syncClaudeAiSkills": false })),
    );

    let inventory = inventory(&fs);

    let pdf = entry_json(&server(&inventory, "anthropic-skills:pdf").entries[0]);
    assert_eq!(pdf["state"], "disabled");
    assert_eq!(
        pdf["reason"],
        json!({
            "code": "disabled_globally",
            "effective": null,
            "switch": r"C:\Users\fixture\.claude\settings.json#syncClaudeAiSkills",
            "sentence": r"Disabled globally: `syncClaudeAiSkills` is false in C:\Users\fixture\.claude\settings.json, so Claude Code skips it.",
        })
    );
    assert_eq!(pdf["enable"]["status"], "not_offered");
    assert_eq!(
        pdf["enable"]["sentence"],
        r"Enable is not offered: `syncClaudeAiSkills` in C:\Users\fixture\.claude\settings.json is a rule config-digger never lifts; change it there by hand."
    );
}

#[test]
fn the_highest_file_holding_a_global_switch_decides() {
    let managed = with(
        synced_home(),
        managed_settings(json!({ "syncClaudeAiSkills": false })),
    );
    let overridden = with(
        with(
            synced_home(),
            managed_settings(json!({ "syncClaudeAiSkills": true })),
        ),
        user_settings(json!({ "syncClaudeAiSkills": false })),
    );

    assert_eq!(
        state_and_code(&inventory(&managed), "anthropic-skills:pdf"),
        pair("blocked", "policy")
    );
    assert_eq!(
        state_and_code(&inventory(&overridden), "anthropic-skills:pdf"),
        pair("effective", "")
    );
}

#[test]
fn a_deny_list_applies_after_precedence() {
    // The local `notes` wins, and is denied by its command: Claude Code
    // loads neither, and the user `notes` stays Shadowed.
    let fs = with(
        project_home(json!({ "mcpServers": { "notes": { "command": "evil-notes" } } })),
        managed_settings(json!({ "deniedMcpServers": [{ "serverCommand": ["evil-notes"] }] })),
    );

    let inventory = in_petme(&fs);

    let notes = server(&inventory, "notes");
    assert_eq!(serde_json::to_value(notes.state).unwrap(), "blocked");
    let states: Vec<(String, String)> = notes
        .entries
        .iter()
        .map(|entry| {
            let entry = entry_json(entry);
            (
                entry["scope"].as_str().unwrap().to_owned(),
                entry["state"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    assert_eq!(
        states,
        [
            ("local".to_owned(), "blocked".to_owned()),
            ("project".to_owned(), "blocked".to_owned()),
            ("user".to_owned(), "shadowed".to_owned()),
        ]
    );
}

#[test]
fn a_later_drop_in_overrides_a_managed_server_of_the_same_name() {
    let fs = claude_home(&json!({}))
        .with_file(
            in_program_files(r"ClaudeCode\managed-settings.json"),
            managed_server("wiki", "https://mcp.corp.example/wiki"),
        )
        .with_file(
            in_program_files(r"ClaudeCode\managed-settings.d\10-wiki.json"),
            managed_server("wiki", "https://mcp.corp.example/wiki-v2"),
        );

    let inventory = inventory(&fs);

    let wiki = server(&inventory, "wiki");
    assert_eq!(
        states(wiki),
        [
            (
                r"C:\Program Files\ClaudeCode\managed-settings.d\10-wiki.json#managedMcpServers.wiki"
                    .to_owned(),
                "effective".to_owned()
            ),
            (
                r"C:\Program Files\ClaudeCode\managed-settings.json#managedMcpServers.wiki"
                    .to_owned(),
                "shadowed".to_owned()
            ),
        ]
    );
    assert_eq!(
        entry_json(&wiki.entries[1])["reason"]["sentence"],
        r"Shadowed by the managed Entry C:\Program Files\ClaudeCode\managed-settings.d\10-wiki.json#managedMcpServers.wiki, which Claude Code ranks higher."
    );
}

#[test]
fn a_later_drop_in_overrides_a_global_switch() {
    let fs = with(
        synced_home(),
        managed_settings(json!({ "syncClaudeAiSkills": false })),
    )
    .with_file(
        in_program_files(r"ClaudeCode\managed-settings.d\10-sync.json"),
        json!({ "syncClaudeAiSkills": true }).to_string(),
    );

    assert_eq!(
        state_and_code(&inventory(&fs), "anthropic-skills:pdf"),
        pair("effective", "")
    );
}

#[test]
fn allow_managed_mcp_servers_only_leaves_only_the_managed_allow_list() {
    let fs = with(
        with(
            servers_home(),
            managed_settings(json!({
                "allowedMcpServers": [{ "serverName": "linear" }],
                "allowManagedMcpServersOnly": true,
            })),
        ),
        user_settings(json!({ "allowedMcpServers": [{ "serverName": "notes" }] })),
    );

    let inventory = inventory(&fs);

    assert_eq!(
        state_and_code(&inventory, "notes"),
        pair("blocked", "policy")
    );
    assert_eq!(state_and_code(&inventory, "linear"), pair("effective", ""));
}

/// The cache of server-managed settings, in the user's `~/.claude`.
fn remote_settings(settings: Value) -> (PathBuf, String) {
    (
        in_home(r".claude\remote-settings.json"),
        settings.to_string(),
    )
}

#[test]
fn remote_settings_rank_above_every_other_managed_source() {
    let fs = with(
        claude_home(&json!({})),
        remote_settings(json!({ "managedMcpServers": {
            "wiki": { "type": "http", "url": "https://mcp.corp.example/wiki" },
        } })),
    );
    let env = env().with_registry_value(
        Hive::LocalMachine,
        POLICIES,
        "Settings",
        &managed_server("wiki", "https://mcp.corp.example/wiki-v2"),
    );

    let inventory = inventory_with(&fs, env, &Context::default());

    let wiki = server(&inventory, "wiki");
    assert_eq!(
        states(wiki),
        [
            (
                r"C:\Users\fixture\.claude\remote-settings.json#managedMcpServers.wiki".to_owned(),
                "effective".to_owned()
            ),
            (
                r"HKLM\SOFTWARE\Policies\ClaudeCode\Settings#managedMcpServers.wiki".to_owned(),
                "unread".to_owned()
            ),
        ]
    );
    let remote = entry_json(&wiki.entries[0]);
    assert_eq!(remote["scope"], "managed");
    assert_eq!(remote["provenance"], "managed");
    assert_eq!(remote["disable"]["code"], "read_only");
    assert_eq!(remote["remove"]["code"], "read_only");
}

#[test]
fn remote_settings_lists_apply_as_policy() {
    let fs = with(
        servers_home(),
        remote_settings(json!({
            "deniedMcpServers": [{ "serverName": "notes" }],
            "allowedMcpServers": [{ "serverName": "notes" }],
        })),
    );

    let inventory = inventory(&fs);

    assert_eq!(
        state_and_code(&inventory, "notes"),
        pair("blocked", "policy")
    );
    assert_eq!(
        state_and_code(&inventory, "linear"),
        pair("blocked", "policy")
    );
}

#[test]
fn a_source_set_to_merge_is_merged_with_the_admin_sources_below_it() {
    let fs = with(
        claude_home(&json!({})),
        remote_settings(json!({
            "managedSourcesBehavior": "merge",
            "managedMcpServers": { "wiki": { "type": "http", "url": "https://mcp.corp.example/wiki" } },
        })),
    )
    .with_file(
        in_program_files(r"ClaudeCode\managed-settings.json"),
        managed_server("tickets", "https://mcp.corp.example/tickets"),
    );
    let env = env()
        .with_registry_value(
            Hive::LocalMachine,
            POLICIES,
            "Settings",
            &managed_server("wiki", "https://mcp.corp.example/wiki-v2"),
        )
        .with_registry_value(
            Hive::CurrentUser,
            POLICIES,
            "Settings",
            &managed_server("mine", "https://mcp.example/mine"),
        );

    let inventory = inventory_with(&fs, env, &Context::default());

    let wiki = server(&inventory, "wiki");
    assert_eq!(
        states(wiki),
        [
            (
                r"C:\Users\fixture\.claude\remote-settings.json#managedMcpServers.wiki".to_owned(),
                "effective".to_owned()
            ),
            (
                r"HKLM\SOFTWARE\Policies\ClaudeCode\Settings#managedMcpServers.wiki".to_owned(),
                "shadowed".to_owned()
            ),
        ]
    );
    assert_eq!(state_and_code(&inventory, "tickets"), pair("effective", ""));
    // The user-writable registry value never merges with another source,
    // and its `managedMcpServers` are never read in any case.
    assert_eq!(
        state_and_code(&inventory, "mine"),
        pair("unread", "user_writable_source")
    );
}

#[test]
fn merging_lists_combines_them_from_every_merged_source() {
    let fs = with(
        with(
            servers_home(),
            remote_settings(json!({
                "managedSourcesBehavior": "merge",
                "deniedMcpServers": [{ "serverName": "notes" }],
            })),
        ),
        managed_settings(json!({ "deniedMcpServers": [{ "serverName": "linear" }] })),
    );

    let inventory = inventory(&fs);

    assert_eq!(
        state_and_code(&inventory, "notes"),
        pair("blocked", "policy")
    );
    assert_eq!(
        state_and_code(&inventory, "linear"),
        pair("blocked", "policy")
    );
}

#[test]
fn a_lower_source_cannot_opt_into_merging_with_the_source_above_it() {
    let fs = with(
        claude_home(&json!({})),
        remote_settings(json!({ "managedMcpServers": {
            "wiki": { "type": "http", "url": "https://mcp.corp.example/wiki" },
        } })),
    )
    .with_file(
        in_program_files(r"ClaudeCode\managed-settings.json"),
        json!({
            "managedSourcesBehavior": "merge",
            "managedMcpServers": { "tickets": { "type": "http", "url": "https://mcp.corp.example/tickets" } },
        })
        .to_string(),
    );

    let inventory = inventory(&fs);

    assert_eq!(
        state_and_code(&inventory, "tickets"),
        pair("unread", "source_skipped")
    );
}

#[test]
fn a_source_holding_only_the_merge_switch_decides_the_merge_but_supplies_no_policy() {
    let fs = with(
        claude_home(&json!({})),
        remote_settings(json!({ "managedSourcesBehavior": "merge" })),
    )
    .with_file(
        in_program_files(r"ClaudeCode\managed-settings.json"),
        managed_server("tickets", "https://mcp.corp.example/tickets"),
    );
    let env = env().with_registry_value(
        Hive::LocalMachine,
        POLICIES,
        "Settings",
        &managed_server("wiki", "https://mcp.corp.example/wiki"),
    );

    let inventory = inventory_with(&fs, env, &Context::default());

    assert_eq!(state_and_code(&inventory, "wiki"), pair("effective", ""));
    assert_eq!(state_and_code(&inventory, "tickets"), pair("effective", ""));
}

#[test]
fn merging_takes_an_allow_list_whole_from_the_highest_source_holding_one() {
    let fs = with(
        with(
            servers_home(),
            remote_settings(json!({
                "managedSourcesBehavior": "merge",
                "allowedMcpServers": [{ "serverName": "notes" }],
            })),
        ),
        managed_settings(json!({ "allowedMcpServers": [{ "serverName": "linear" }] })),
    );

    let inventory = inventory(&fs);

    assert_eq!(state_and_code(&inventory, "notes"), pair("effective", ""));
    assert_eq!(
        state_and_code(&inventory, "linear"),
        pair("blocked", "policy")
    );
}

#[test]
fn a_source_holding_only_control_keys_holds_no_setting() {
    let fs = with(
        claude_home(&json!({})),
        remote_settings(json!({ "wslInheritsWindowsSettings": true })),
    );
    let env = env().with_registry_value(
        Hive::LocalMachine,
        POLICIES,
        "Settings",
        &managed_server("wiki", "https://mcp.corp.example/wiki"),
    );

    let inventory = inventory_with(&fs, env, &Context::default());

    assert_eq!(state_and_code(&inventory, "wiki"), pair("effective", ""));
}

#[test]
fn merging_ranks_a_higher_source_over_the_drop_ins_below_it() {
    let fs = with(
        claude_home(&json!({})),
        remote_settings(json!({
            "managedSourcesBehavior": "merge",
            "managedMcpServers": { "wiki": { "type": "http", "url": "https://mcp.corp.example/wiki" } },
        })),
    )
    .with_file(
        in_program_files(r"ClaudeCode\managed-settings.d\10-wiki.json"),
        managed_server("wiki", "https://mcp.corp.example/wiki-v2"),
    );

    let inventory = inventory(&fs);

    let wiki = server(&inventory, "wiki");
    assert_eq!(
        states(wiki),
        [
            (
                r"C:\Users\fixture\.claude\remote-settings.json#managedMcpServers.wiki".to_owned(),
                "effective".to_owned()
            ),
            (
                r"C:\Program Files\ClaudeCode\managed-settings.d\10-wiki.json#managedMcpServers.wiki"
                    .to_owned(),
                "shadowed".to_owned()
            ),
        ]
    );
}
