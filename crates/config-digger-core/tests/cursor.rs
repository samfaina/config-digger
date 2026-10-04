//! Cursor through the core `api`, with fake ports over fixture Cursor
//! folders and fixture SQLite stores: the MCP servers it reads from its own
//! files and from the Claude Code plugins it imports, each under its own
//! identifier, how it collapses servers with the same URL and keeps Project
//! servers out until they are approved, the skills it reads from five
//! Agents' folders, and the Projects it records.

use config_digger_core::api::{
    Action, Context, Core, Entry, Inventory, Item, Plan, ScanOptions, Targets, WarningClass,
};
use config_digger_core::ports::fake::{self, FakeEnvironment, FakeFileSystem};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const HOME: &str = r"C:\Users\fixture";
const APP_DATA: &str = r"C:\Users\fixture\AppData\Roaming";
const LOCAL_APP_DATA: &str = r"C:\Users\fixture\AppData\Local";

/// The folder where Cursor keeps the user's own data.
const USER: &str = r"C:\Users\fixture\AppData\Roaming\Cursor\User";

/// Cursor's store of application state, every window's.
fn global_state() -> PathBuf {
    Path::new(USER).join(r"globalStorage\state.vscdb")
}

fn in_home(relative: &str) -> PathBuf {
    Path::new(HOME).join(relative)
}

/// The user's `~\.cursor\mcp.json`.
fn user_mcp() -> PathBuf {
    in_home(r".cursor\mcp.json")
}

/// A Cursor that has run once: its user data folder holds its settings.
fn cursor() -> FakeFileSystem {
    FakeFileSystem::new().with_file(Path::new(USER).join("settings.json"), "{}\n")
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

fn inventory(fs: &FakeFileSystem) -> Inventory {
    let core = core(fs);
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    core.resolve(&snapshot, &Context::default())
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

/// The Cursor Item named `name`.
fn item<'i>(inventory: &'i Inventory, name: &str) -> &'i Item {
    inventory
        .items
        .iter()
        .find(|item| item.agent == "cursor" && item.name == name)
        .unwrap_or_else(|| {
            let names: Vec<&str> = cursor_items(inventory)
                .map(|item| item.name.as_str())
                .collect();
            panic!("no Cursor Item `{name}` among {names:?}")
        })
}

fn cursor_items(inventory: &Inventory) -> impl Iterator<Item = &Item> {
    inventory.items.iter().filter(|item| item.agent == "cursor")
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
fn the_user_mcp_json_s_servers_are_items_under_their_user_identifiers() {
    let fs = cursor().with_file(
        user_mcp(),
        "{\n  // Comments are allowed, as are trailing commas.\n  \"mcpServers\": {\n    \"fetch\": { \"command\": \"uvx\", \"args\": [\"mcp-server-fetch\"], },\n  },\n}\n",
    );

    let inventory = inventory(&fs);

    let fetch = item(&inventory, "user-fetch");
    assert_eq!(code(fetch.kind), "mcp");
    assert_eq!(code(fetch.state), "effective");
    let entry = entry_json(&fetch.entries[0]);
    assert_eq!(entry["path"], user_mcp().display().to_string());
    assert_eq!(entry["key_path"], json!(["mcpServers", "fetch"]));
    assert_eq!(entry["scope"], "user");
    assert_eq!(entry["provenance"], "file");
}

/// Where the fixture Projects go.
const PROJECTS: &str = r"E:\projects";

fn project(name: &str) -> PathBuf {
    Path::new(PROJECTS).join(name)
}

/// `fs` with `servers` in the `.cursor\mcp.json` of the folder `folder`.
fn with_project_mcp(fs: FakeFileSystem, folder: &Path, servers: Value) -> FakeFileSystem {
    fs.with_file(
        folder.join(r".cursor\mcp.json"),
        json!({ "mcpServers": servers }).to_string(),
    )
}

#[test]
fn a_project_s_servers_are_items_under_identifiers_of_their_folder() {
    let fs = with_project_mcp(
        cursor(),
        &project("app"),
        json!({ "docs": { "command": "docs-mcp" } }),
    );

    let inventory = inventory_at(&fs, &project("app"));

    let docs = item(&inventory, "project-0-app-docs");
    let entry = entry_json(&docs.entries[0]);
    assert_eq!(entry["path"], r"E:\projects\app\.cursor\mcp.json");
    assert_eq!(entry["key_path"], json!(["mcpServers", "docs"]));
    assert_eq!(entry["scope"], "project");
}

#[test]
fn same_named_servers_of_the_user_and_a_project_never_compete() {
    let fs = with_project_mcp(
        cursor().with_file(
            user_mcp(),
            json!({ "mcpServers": { "docs": { "command": "docs-mcp" } } }).to_string(),
        ),
        &project("app"),
        json!({ "docs": { "command": "docs-mcp", "args": ["--project"] } }),
    );

    let inventory = inventory_at(&fs, &project("app"));

    let user = item(&inventory, "user-docs");
    let project = item(&inventory, "project-0-app-docs");
    assert_eq!(user.entries.len(), 1);
    assert_eq!(project.entries.len(), 1);
    assert_eq!(code(user.state), "effective");
}

#[test]
fn each_folder_of_a_workspace_file_names_its_servers_by_its_position() {
    let workspace = project(r"multi\multi.code-workspace");
    let fs = with_project_mcp(
        with_project_mcp(
            cursor().with_file(
                &workspace,
                r#"{ "folders": [{ "path": "api" }, { "path": "web" }] }"#,
            ),
            &project(r"multi\api"),
            json!({ "docs": { "command": "api-docs" } }),
        ),
        &project(r"multi\web"),
        json!({ "docs": { "command": "web-docs" } }),
    );

    let inventory = inventory_at(&fs, &workspace);

    assert_eq!(
        item(&inventory, "project-0-api-docs").entries[0].locator,
        r"E:\projects\multi\api\.cursor\mcp.json#mcpServers.docs"
    );
    assert_eq!(
        item(&inventory, "project-1-web-docs").entries[0].locator,
        r"E:\projects\multi\web\.cursor\mcp.json#mcpServers.docs"
    );
}

#[test]
fn a_server_with_neither_a_command_nor_a_url_is_invalid() {
    let fs = cursor().with_file(
        user_mcp(),
        json!({ "mcpServers": {
            "nothing": { "args": ["--port", "3000"] },
            "typed": { "type": "sse", "command": "docs-mcp" },
            "remote": { "url": "https://mcp.example.com/mcp" },
        } })
        .to_string(),
    );

    let inventory = inventory(&fs);

    let nothing = &item(&inventory, "user-nothing").entries[0];
    assert_eq!(code(nothing.state), "invalid");
    assert_eq!(
        code(nothing.reason.as_ref().unwrap().code),
        "malformed_entry"
    );
    // Cursor goes by `command` or `url`, whatever `type` says.
    assert_eq!(code(item(&inventory, "user-typed").state), "effective");
    assert_eq!(code(item(&inventory, "user-remote").state), "effective");
}

/// `fs` with `approved` as the Project servers Cursor approved.
fn with_approvals(fs: FakeFileSystem, approved: &[&str]) -> FakeFileSystem {
    fs.with_sqlite_item(
        global_state(),
        "cursor/approvedProjectMcpServers",
        &json!(approved).to_string(),
    )
}

/// The State and reason code of the only Entry of the Cursor Item `name`
/// in the Project app.
fn state_in_app(fs: &FakeFileSystem, name: &str) -> (String, Option<String>) {
    let inventory = inventory_at(fs, &project("app"));
    let entry = &item(&inventory, name).entries[0];
    (
        code(entry.state),
        entry.reason.as_ref().map(|reason| code(reason.code)),
    )
}

/// The playwright server Cursor 3.22.12 approved on the research machine
/// as `project-0-cursor-playwright:62809ed2`, here in the folder app.
fn playwright() -> Value {
    json!({ "playwright": { "command": "npx", "args": ["@playwright/mcp@latest"] } })
}

#[test]
fn a_project_server_cursor_has_not_approved_is_blocked() {
    let fs = with_project_mcp(cursor(), &project("app"), playwright());

    let inventory = inventory_at(&fs, &project("app"));

    let entry = &item(&inventory, "project-0-app-playwright").entries[0];
    assert_eq!(code(entry.state), "blocked");
    let reason = entry.reason.as_ref().unwrap();
    assert_eq!(code(reason.code), "not_approved");
    assert_eq!(
        reason.sentence,
        r"Blocked: Cursor has not approved `project-0-app-playwright` from E:\projects\app\.cursor\mcp.json. Approve it in Cursor, which asks about it in its MCP settings; config-digger never approves servers."
    );
}

#[test]
fn a_project_server_approved_with_the_hash_of_its_configuration_runs() {
    let fs = with_approvals(
        with_project_mcp(cursor(), &project("app"), playwright()),
        &["project-0-app-playwright:62809ed2"],
    );

    assert_eq!(
        state_in_app(&fs, "project-0-app-playwright"),
        ("effective".to_owned(), None)
    );
}

#[test]
fn a_project_server_changed_since_its_approval_is_blocked_again() {
    let changed = json!({ "playwright": { "command": "npx", "args": ["@playwright/mcp@next"] } });
    let fs = with_approvals(
        with_project_mcp(cursor(), &project("app"), changed),
        &["project-0-app-playwright:62809ed2"],
    );

    assert_eq!(
        state_in_app(&fs, "project-0-app-playwright"),
        ("blocked".to_owned(), Some("not_approved".to_owned()))
    );
}

#[test]
fn an_approval_of_the_bare_identifier_still_counts() {
    let fs = with_approvals(
        with_project_mcp(cursor(), &project("app"), playwright()),
        &["project-0-app-playwright"],
    );

    assert_eq!(state_in_app(&fs, "project-0-app-playwright").0, "effective");
}

#[test]
fn the_hash_covers_only_the_fields_of_the_server_s_transport() {
    // The expected hashes come from Cursor's own algorithm run in Node: a
    // server with a command hashes its `command`, `args`, `env` and
    // `envFile`, one without hashes its `url`, given `http://` when it
    // names no scheme, `headers` and `auth`, as JavaScript orders their keys.
    let servers = json!({
        "remote": { "url": "localhost:3000/mcp", "headers": { "X-Team": "core", "10": "ten" }, "command": "" },
        "local": { "command": "docs-mcp", "url": "https://ignored.example.com", "env": { "TOKEN": "${env:DOCS_TOKEN}" }, "envFile": ".env" },
    });
    let fs = with_approvals(
        with_project_mcp(cursor(), &project("app"), servers),
        &[
            "project-0-app-remote:-7357c1f2",
            "project-0-app-local:-2da029cc",
        ],
    );

    assert_eq!(state_in_app(&fs, "project-0-app-remote").0, "effective");
    assert_eq!(state_in_app(&fs, "project-0-app-local").0, "effective");
}

#[test]
fn the_user_s_servers_need_no_approval() {
    let fs = cursor().with_file(
        user_mcp(),
        json!({ "mcpServers": playwright() }).to_string(),
    );

    assert_eq!(
        code(item(&inventory(&fs), "user-playwright").state),
        "effective"
    );
}

fn skill_md(name: &str) -> String {
    format!("---\nname: {name}\ndescription: The {name} skill.\n---\n\nDo the {name} thing.\n")
}

/// `fs` with a skill folder at `folder`, named by its last component.
fn with_skill(fs: FakeFileSystem, folder: impl AsRef<Path>) -> FakeFileSystem {
    let folder = folder.as_ref();
    let name = folder.file_name().unwrap().to_string_lossy().into_owned();
    fs.with_file(folder.join("SKILL.md"), skill_md(&name))
}

/// The Cursor skill Items named `name`, each as its State and the locators
/// of its Entries.
fn skills_named(inventory: &Inventory, name: &str) -> Vec<(String, Vec<(String, String)>)> {
    cursor_items(inventory)
        .filter(|item| code(item.kind) == "skill" && item.name == name)
        .map(|item| {
            let entries = item
                .entries
                .iter()
                .map(|entry| (code(entry.state), entry.locator.clone()))
                .collect();
            (code(item.state), entries)
        })
        .collect()
}

#[test]
fn skills_at_the_same_path_compete_cursor_first_then_claude_codex_grok_agents() {
    let mut fs = cursor();
    for folder in [".agents", ".grok", ".codex", ".claude", ".cursor"] {
        fs = with_skill(fs, in_home(&format!(r"{folder}\skills\review")));
    }
    fs = with_skill(fs, in_home(r".grok\skills\triage"));
    fs = with_skill(fs, in_home(r".agents\skills\triage"));

    let inventory = inventory(&fs);

    let review = skills_named(&inventory, "review");
    assert_eq!(review.len(), 1, "{review:?}");
    let states: Vec<&str> = review[0]
        .1
        .iter()
        .map(|(state, _)| state.as_str())
        .collect();
    assert_eq!(
        states,
        ["effective", "shadowed", "shadowed", "shadowed", "shadowed"]
    );
    assert_eq!(
        review[0].1[0].1,
        in_home(r".cursor\skills\review").display().to_string()
    );
    assert_eq!(
        review[0].1[1].1,
        in_home(r".claude\skills\review").display().to_string()
    );
    assert_eq!(
        review[0].1[4].1,
        in_home(r".agents\skills\review").display().to_string()
    );
    let triage = skills_named(&inventory, "triage");
    assert_eq!(
        triage[0].1,
        [
            (
                "effective".to_owned(),
                in_home(r".grok\skills\triage").display().to_string()
            ),
            (
                "shadowed".to_owned(),
                in_home(r".agents\skills\triage").display().to_string()
            ),
        ]
    );
}

#[test]
fn skill_folders_are_read_at_any_depth_and_only_the_same_path_competes() {
    let fs = with_skill(
        with_skill(
            with_skill(cursor(), in_home(r".claude\skills\unslop")),
            in_home(r".claude\skills\synced\org-account\unslop"),
        ),
        in_home(r".claude\skills\.trash\old"),
    );

    let inventory = inventory(&fs);

    let unslop = skills_named(&inventory, "unslop");
    assert_eq!(unslop.len(), 2, "{unslop:?}");
    assert!(unslop.iter().all(|(state, _)| state == "effective"));
    assert!(skills_named(&inventory, "old").is_empty());
}

#[test]
fn a_user_skill_and_a_project_skill_of_the_same_name_are_separate_items() {
    let fs = with_skill(
        with_skill(cursor(), in_home(r".cursor\skills\review")),
        project(r"app\.claude\skills\review"),
    );

    let inventory = inventory_at(&fs, &project("app"));

    let review = skills_named(&inventory, "review");
    assert_eq!(review.len(), 2, "{review:?}");
    assert!(review.iter().all(|(state, _)| state == "effective"));
    let scopes: Vec<String> = cursor_items(&inventory)
        .filter(|item| item.name == "review")
        .map(|item| item.entries[0].scope.clone())
        .collect();
    assert!(scopes.contains(&"user".to_owned()) && scopes.contains(&"project".to_owned()));
}

#[test]
fn a_project_reads_the_skills_of_all_five_folders() {
    let mut fs = cursor();
    for folder in [".cursor", ".claude", ".codex", ".grok", ".agents"] {
        let name = format!("{}-skill", &folder[1..]);
        fs = with_skill(fs, project(&format!(r"app\{folder}\skills\{name}")));
    }

    let inventory = inventory_at(&fs, &project("app"));

    for name in [
        "cursor-skill",
        "claude-skill",
        "codex-skill",
        "grok-skill",
        "agents-skill",
    ] {
        let skill = item(&inventory, name);
        assert_eq!(code(skill.state), "effective", "{name}");
        assert_eq!(skill.entries[0].scope, "project", "{name}");
    }
}

#[test]
fn cursor_s_built_in_skills_are_read_only() {
    let fs = with_skill(cursor(), in_home(r".cursor\skills-cursor\create-rule"));

    let inventory = inventory(&fs);

    let entry = entry_json(&item(&inventory, "create-rule").entries[0]);
    assert_eq!(entry["state"], "effective");
    assert_eq!(entry["provenance"], "built_in");
    assert_eq!(entry["scope"], "built-in");
    assert_eq!(entry["remove"]["code"], "read_only");
    assert_eq!(entry["disable"]["code"], "read_only");
}

/// Where Claude Code keeps the files of the plugins it installs.
fn plugin_cache(relative: &str) -> PathBuf {
    in_home(r".claude\plugins\cache").join(relative)
}

/// `fs` with Claude Code's `installed_plugins.json` holding `plugins`, each
/// an id and its records, and `enabled` as its user settings'
/// `enabledPlugins`.
fn with_installed(fs: FakeFileSystem, plugins: Value, enabled: Value) -> FakeFileSystem {
    fs.with_file(
        in_home(r".claude\plugins\installed_plugins.json"),
        json!({ "version": 2, "plugins": plugins }).to_string(),
    )
    .with_file(
        in_home(r".claude\settings.json"),
        json!({ "enabledPlugins": enabled }).to_string(),
    )
}

/// A user install of the plugin `<name>@market` in the plugin cache.
fn user_install(name: &str) -> Value {
    json!([{
        "scope": "user",
        "installPath": plugin_cache(&format!(r"market\{name}\1.0.0")).display().to_string(),
        "version": "1.0.0",
    }])
}

/// `fs` with the files of the plugin `<name>@market`, each a path in its
/// folder and its JSON.
fn with_plugin_files(
    mut fs: FakeFileSystem,
    name: &str,
    files: &[(&str, Value)],
) -> FakeFileSystem {
    for (relative, value) in files {
        fs = fs.with_file(
            plugin_cache(&format!(r"market\{name}\1.0.0\{relative}")),
            value.to_string(),
        );
    }
    fs
}

/// Claude Code's supabase plugin, which ships a manifest for Cursor and one
/// for Claude Code, each naming its own file of servers, and a skill.
fn supabase(fs: FakeFileSystem) -> FakeFileSystem {
    let fs = with_plugin_files(
        fs,
        "supabase",
        &[
            (
                r".cursor-plugin\plugin.json",
                json!({ "name": "supabase", "mcpServers": "agents/cursor/mcp.json" }),
            ),
            (
                r".claude-plugin\plugin.json",
                json!({ "name": "supabase", "mcpServers": "agents/claude/.mcp.json" }),
            ),
            (
                r"agents\cursor\mcp.json",
                json!({ "mcpServers": { "supabase": { "url": "https://mcp.supabase.com/mcp?client=cursor" } } }),
            ),
            (
                r"agents\claude\.mcp.json",
                json!({ "mcpServers": { "supabase-claude": { "url": "https://mcp.supabase.com/mcp" } } }),
            ),
        ],
    );
    with_skill(
        fs,
        plugin_cache(r"market\supabase\1.0.0\skills\supabase-postgres"),
    )
}

/// Claude Code's context7 plugin, with no manifest and a `.mcp.json`.
fn context7(fs: FakeFileSystem) -> FakeFileSystem {
    with_plugin_files(
        fs,
        "context7",
        &[(
            ".mcp.json",
            json!({ "mcpServers": { "context7": { "command": "npx", "args": ["-y", "@upstash/context7-mcp"] } } }),
        )],
    )
}

fn plugins_installed() -> FakeFileSystem {
    with_installed(
        context7(supabase(cursor())),
        json!({
            "supabase@market": user_install("supabase"),
            "context7@market": user_install("context7"),
        }),
        json!({ "supabase@market": true, "context7@market": true }),
    )
}

#[test]
fn a_claude_code_plugin_s_servers_are_items_under_plugin_identifiers() {
    let inventory = inventory(&plugins_installed());

    let supabase = item(&inventory, "plugin-supabase-supabase");
    assert_eq!(code(supabase.state), "effective");
    let entry = entry_json(&supabase.entries[0]);
    assert_eq!(
        entry["path"],
        plugin_cache(r"market\supabase\1.0.0\agents\cursor\mcp.json")
            .display()
            .to_string()
    );
    assert_eq!(entry["scope"], "plugin");
    assert_eq!(entry["provenance"], "plugin");
    assert_eq!(entry["plugin"]["id"], "supabase@market");
    assert_eq!(
        entry["remove"]["how_to_remove"]["command"],
        "claude plugin uninstall supabase@market --scope user"
    );
    assert_eq!(
        code(item(&inventory, "plugin-context7-context7").state),
        "effective"
    );
}

#[test]
fn cursor_reads_only_the_first_manifest_of_a_plugin() {
    let inventory = inventory(&plugins_installed());

    assert!(
        cursor_items(&inventory).all(|item| item.name != "plugin-supabase-supabase-claude"),
        "the servers of the Claude Code manifest are read"
    );
}

#[test]
fn a_claude_code_plugin_s_skills_are_listed_with_the_plugin() {
    let inventory = inventory(&plugins_installed());

    let entry = entry_json(&item(&inventory, "supabase-postgres").entries[0]);
    assert_eq!(entry["state"], "effective");
    assert_eq!(entry["provenance"], "plugin");
    assert_eq!(entry["plugin"]["id"], "supabase@market");
}

#[test]
fn a_plugin_enabled_plugins_turns_off_or_leaves_out_is_disabled() {
    let fs = with_installed(
        context7(supabase(cursor())),
        json!({
            "supabase@market": user_install("supabase"),
            "context7@market": user_install("context7"),
        }),
        json!({ "supabase@market": false }),
    );

    let inventory = inventory(&fs);

    for name in [
        "plugin-supabase-supabase",
        "supabase-postgres",
        "plugin-context7-context7",
    ] {
        let entry = &item(&inventory, name).entries[0];
        assert_eq!(code(entry.state), "disabled", "{name}");
        assert_eq!(
            code(entry.reason.as_ref().unwrap().code),
            "plugin_disabled",
            "{name}"
        );
    }
    let switch = item(&inventory, "plugin-supabase-supabase").entries[0]
        .reason
        .as_ref()
        .unwrap()
        .switch
        .clone();
    assert_eq!(
        switch.as_deref(),
        Some(r"C:\Users\fixture\.claude\settings.json#enabledPlugins.supabase@market")
    );
}

#[test]
fn a_local_install_applies_in_its_own_project_only() {
    let fs = with_installed(
        context7(cursor()),
        json!({ "context7@market": [{
            "scope": "local",
            "projectPath": r"E:\projects\app",
            "installPath": plugin_cache(r"market\context7\1.0.0").display().to_string(),
        }] }),
        json!({}),
    )
    .with_folder(project("app"))
    .with_folder(project("other"))
    .with_file(
        project(r"app\.claude\settings.local.json"),
        json!({ "enabledPlugins": { "context7@market": true } }).to_string(),
    );

    let in_app = inventory_at(&fs, &project("app"));
    let in_other = inventory_at(&fs, &project("other"));

    assert_eq!(
        code(item(&in_app, "plugin-context7-context7").state),
        "effective"
    );
    assert!(cursor_items(&in_other).all(|item| item.name != "plugin-context7-context7"));
}

/// Cursor's cache of its Statsig gates, holding `gates`, each a gate's
/// name and value, keyed by their djb2 hashes.
fn bootstrap(gates: &[(&str, bool)]) -> String {
    let djb2 = |name: &str| {
        name.encode_utf16()
            .fold(0u32, |hash, unit| {
                hash.wrapping_mul(31).wrapping_add(u32::from(unit))
            })
            .to_string()
    };
    let gates: serde_json::Map<String, Value> = gates
        .iter()
        .map(|(name, value)| {
            let key = djb2(name);
            (key.clone(), json!({ "name": key, "value": value }))
        })
        .collect();
    json!({ "feature_gates": gates, "time": 1790867511105u64, "hash_used": "djb2" }).to_string()
}

fn with_gates(fs: FakeFileSystem, gates: &[(&str, bool)]) -> FakeFileSystem {
    fs.with_sqlite_item(
        global_state(),
        "workbench.experiments.statsigBootstrap",
        &bootstrap(gates),
    )
}

#[test]
fn a_plugin_s_servers_are_unread_while_claude_code_plugin_import_is_off() {
    let fs = with_gates(plugins_installed(), &[("enable_cc_plugin_import", false)]);

    let inventory = inventory(&fs);

    let entry = entry_json(&item(&inventory, "plugin-supabase-supabase").entries[0]);
    assert_eq!(entry["state"], "unread");
    assert_eq!(entry["caveats"][0]["code"], "gate_cached");
    assert_eq!(
        code(item(&inventory, "supabase-postgres").state),
        "effective"
    );
}

#[test]
fn claude_code_plugin_import_is_assumed_on() {
    let inventory = inventory(&plugins_installed());

    let entry = entry_json(&item(&inventory, "plugin-supabase-supabase").entries[0]);
    assert_eq!(entry["state"], "effective");
    assert_eq!(entry["caveats"][0]["code"], "gate_assumed");
}

#[test]
fn claude_code_s_disable_of_a_plugin_reaches_cursor_through_enabled_plugins() {
    let fs = plugins_installed();
    let core = core(&fs);
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    let targets = Targets {
        selectors: vec!["plugin:supabase:supabase-claude".to_owned()],
        all: false,
    };

    let plan = core
        .plan(&snapshot, &Context::default(), Action::Disable, &targets)
        .unwrap();

    let reached: Vec<(String, String, String)> = plan
        .preview()
        .changes
        .iter()
        .filter(|change| change.project.is_none())
        .map(|change| {
            (
                change.agent.clone(),
                change.item.clone(),
                code(change.after),
            )
        })
        .collect();
    for (agent, item) in [
        ("claude-code", "plugin:supabase:supabase-claude"),
        ("cursor", "plugin-supabase-supabase"),
        ("cursor", "supabase-postgres"),
    ] {
        assert!(
            reached.contains(&(agent.to_owned(), item.to_owned(), "disabled".to_owned())),
            "{agent} {item} not in {reached:?}"
        );
    }
    assert_eq!(plan.preview().files.len(), 1);
    assert_eq!(
        plan.preview().files[0].path,
        in_home(r".claude\settings.json")
    );
}

/// The installed plugins, with `servers` in the user's `mcp.json` and the
/// gate `dedupe_mcp_servers` cached as `dedupe`.
fn deduping(servers: Value, dedupe: Option<bool>) -> FakeFileSystem {
    let fs =
        plugins_installed().with_file(user_mcp(), json!({ "mcpServers": servers }).to_string());
    match dedupe {
        Some(on) => with_gates(fs, &[("dedupe_mcp_servers", on)]),
        None => fs,
    }
}

#[test]
fn remote_servers_with_the_same_url_collapse_while_dedupe_is_on() {
    let fs = deduping(
        json!({ "supa": { "url": "https://MCP.Supabase.com:443/mcp/?client=cursor#tools" } }),
        Some(true),
    );

    let inventory = inventory(&fs);

    let supa = item(&inventory, "user-supa");
    let states: Vec<(String, String)> = supa
        .entries
        .iter()
        .map(|entry| (code(entry.state), entry.locator.clone()))
        .collect();
    assert_eq!(
        states,
        [
            (
                "effective".to_owned(),
                format!(r"{}#mcpServers.supa", user_mcp().display())
            ),
            (
                "shadowed".to_owned(),
                format!(
                    r"{}#mcpServers.supabase",
                    plugin_cache(r"market\supabase\1.0.0\agents\cursor\mcp.json").display()
                )
            ),
        ]
    );
    assert_eq!(
        code(supa.entries[1].reason.as_ref().unwrap().code),
        "shadowed_by_precedence"
    );
    assert!(cursor_items(&inventory).all(|item| item.name != "plugin-supabase-supabase"));
}

#[test]
fn servers_with_the_same_url_stay_apart_while_dedupe_is_off() {
    let fs = deduping(
        json!({ "supa": { "url": "https://mcp.supabase.com/mcp?client=cursor" } }),
        Some(false),
    );

    let inventory = inventory(&fs);

    assert_eq!(code(item(&inventory, "user-supa").state), "effective");
    assert_eq!(
        code(item(&inventory, "plugin-supabase-supabase").state),
        "effective"
    );
    assert_eq!(item(&inventory, "user-supa").entries.len(), 1);
}

#[test]
fn dedupe_is_assumed_on() {
    let fs = deduping(
        json!({ "supa": { "url": "https://mcp.supabase.com/mcp?client=cursor" } }),
        None,
    );

    let inventory = inventory(&fs);

    assert_eq!(item(&inventory, "user-supa").entries.len(), 2);
}

#[test]
fn a_server_with_a_command_or_of_a_project_never_collapses_by_url() {
    let fs = with_project_mcp(
        deduping(
            json!({ "both": { "command": "supabase-mcp", "url": "https://mcp.supabase.com/mcp?client=cursor" } }),
            Some(true),
        ),
        &project("app"),
        json!({ "supa": { "url": "https://mcp.supabase.com/mcp?client=cursor" } }),
    );

    let inventory = inventory_at(&fs, &project("app"));

    for name in [
        "user-both",
        "project-0-app-supa",
        "plugin-supabase-supabase",
    ] {
        assert_eq!(item(&inventory, name).entries.len(), 1, "{name}");
    }
}

#[test]
fn a_server_that_collapses_by_url_says_what_config_digger_cannot_see() {
    let fs = deduping(
        json!({ "supa": { "url": "https://mcp.supabase.com/mcp?client=cursor" } }),
        Some(true),
    );

    let inventory = inventory(&fs);

    let caveats = &entry_json(&item(&inventory, "user-supa").entries[0])["caveats"];
    let unseen = caveats
        .as_array()
        .unwrap()
        .iter()
        .find(|caveat| caveat["code"] == "unseen_servers")
        .unwrap_or_else(|| panic!("no unseen_servers caveat in {caveats}"));
    assert_eq!(
        unseen["sentence"],
        "Cursor runs one server per URL, and servers config-digger cannot see take part too: those of extensions, of the team and of user Marketplace plugins. One of them, or one you picked in Cursor, may run instead."
    );
    assert!(
        caveats
            .as_array()
            .unwrap()
            .iter()
            .any(|caveat| caveat["code"] == "gate_cached")
    );
}

/// `fs` with Cursor's Include Third-Party Plugins, Skills, and Other
/// Configs switch at `on`.
fn with_third_party(fs: FakeFileSystem, on: bool) -> FakeFileSystem {
    fs.with_sqlite_item(
        global_state(),
        "cursor/thirdPartyExtensibilityEnabled",
        &on.to_string(),
    )
}

#[test]
fn turning_third_party_configs_off_disables_what_cursor_imports() {
    let mut fs = plugins_installed();
    for folder in [".cursor", ".claude", ".codex", ".grok", ".agents"] {
        let name = format!("{}-skill", &folder[1..]);
        fs = with_skill(fs, in_home(&format!(r"{folder}\skills\{name}")));
    }
    let fs = with_third_party(fs, false);

    let inventory = inventory(&fs);

    for name in [
        "claude-skill",
        "codex-skill",
        "grok-skill",
        "supabase-postgres",
        "plugin-supabase-supabase",
    ] {
        let entry = &item(&inventory, name).entries[0];
        assert_eq!(code(entry.state), "disabled", "{name}");
        assert_eq!(
            code(entry.reason.as_ref().unwrap().code),
            "disabled_globally",
            "{name}"
        );
        assert!(
            entry.enable.is_some(),
            "{name}: Enable not offered says why"
        );
    }
    for name in ["cursor-skill", "agents-skill"] {
        assert_eq!(code(item(&inventory, name).state), "effective", "{name}");
    }
}

#[test]
fn a_skill_turned_off_with_third_party_configs_leaves_the_next_folder_s_effective() {
    let fs = with_third_party(
        with_skill(
            with_skill(cursor(), in_home(r".claude\skills\review")),
            in_home(r".agents\skills\review"),
        ),
        false,
    );

    let inventory = inventory(&fs);

    let review = skills_named(&inventory, "review");
    assert_eq!(
        review[0].1,
        [
            (
                "disabled".to_owned(),
                in_home(r".claude\skills\review").display().to_string()
            ),
            (
                "effective".to_owned(),
                in_home(r".agents\skills\review").display().to_string()
            ),
        ]
    );
}

#[test]
fn third_party_configs_are_on_unless_turned_off() {
    let fs = with_skill(cursor(), in_home(r".claude\skills\review"));

    assert_eq!(code(item(&inventory(&fs), "review").state), "effective");
    let fs = with_third_party(fs, true);
    assert_eq!(code(item(&inventory(&fs), "review").state), "effective");
}

/// Whether the Projects of `inventory` hold `path`, recorded by Cursor.
fn recorded_by_cursor(inventory: &Inventory, path: &Path) -> bool {
    inventory
        .projects
        .iter()
        .any(|project| project.path == path && project.registered_by.contains(&"cursor".to_owned()))
}

#[test]
fn projects_come_from_cursor_s_workspace_storage() {
    let fs = cursor().with_folder(project("app")).with_file(
        Path::new(USER).join(r"workspaceStorage\ebf8269e\workspace.json"),
        r#"{"folder":"file:///e%3A/projects/app"}"#,
    );

    assert!(recorded_by_cursor(&inventory(&fs), &project("app")));
}

#[test]
fn projects_come_from_the_agents_window_s_list() {
    // As Cursor 3.22.12 keeps a Project of its Agents Window.
    let projects = json!([{
        "type": "workspace",
        "id": "workspace:ebf8269ee7453051eebf6d3b33565220",
        "name": "app",
        "lastUsedAt": 1790862531347u64,
        "displayPath": r"E:\projects\app",
        "workspaceIdentifier": {
            "id": "ebf8269ee7453051eebf6d3b33565220",
            "uri": {
                "$mid": 1,
                "fsPath": r"e:\projects\app",
                "_sep": 1,
                "external": "file:///e%3A/projects/app",
                "path": "/E:/projects/app",
                "scheme": "file",
            },
        },
    }]);
    let fs = cursor().with_folder(project("app")).with_sqlite_item(
        global_state(),
        "cursor/glass.additionalProjects",
        &projects.to_string(),
    );

    assert!(recorded_by_cursor(&inventory(&fs), &project("app")));
}

#[test]
fn projects_come_from_the_workspace_metadata() {
    let metadata = json!({ "entries": [{
        "workspaceId": "ebf8269ee7453051eebf6d3b33565220",
        "displayPath": r"E:\projects\web",
        "worktreeInfo": { "isWorktree": false },
        "folderUri": "file:///e%3A/projects/web",
    }] });
    let fs = cursor().with_folder(project("web")).with_sqlite_item(
        global_state(),
        "workspaceMetadata.entries",
        &metadata.to_string(),
    );

    assert!(recorded_by_cursor(&inventory(&fs), &project("web")));
}

#[test]
fn cursor_notes_the_plugins_it_keeps_in_the_account() {
    let inventory = inventory(&cursor());

    let home = inventory
        .homes
        .iter()
        .find(|home| home.agent == "cursor")
        .expect("Cursor's home");
    assert_eq!(home.folder, Path::new(USER));
    let note = home
        .notes
        .iter()
        .find(|note| code(note.code) == "marketplace_plugins_not_on_disk")
        .expect("the note");
    assert_eq!(
        note.sentence,
        "Cursor keeps the plugins you install for yourself from its Marketplace in your account, so they are not listed here. Those a Project installs are."
    );
}

#[test]
fn the_cursor_cli_s_own_files_are_not_read_as_cursor_s() {
    let slug = in_home(r".cursor\projects\e-projects-app");
    let fs = with_project_mcp(
        cursor().with_file(
            user_mcp(),
            json!({ "mcpServers": playwright() }).to_string(),
        ),
        &project("app"),
        playwright(),
    )
    .with_file(
        slug.join("mcp-approvals.json"),
        json!(["project-0-app-playwright"]).to_string(),
    )
    .with_file(
        slug.join("mcp-disabled.json"),
        json!(["user-playwright"]).to_string(),
    )
    .with_file(
        slug.join(r"mcps\user-other\SERVER_METADATA.json"),
        json!({ "serverIdentifier": "user-other", "serverName": "other" }).to_string(),
    );

    let inventory = inventory_at(&fs, &project("app"));

    assert_eq!(
        code(item(&inventory, "project-0-app-playwright").state),
        "blocked"
    );
    assert_eq!(code(item(&inventory, "user-playwright").state), "effective");
    assert!(
        cursor_items(&inventory)
            .flat_map(|item| &item.entries)
            .all(|entry| !entry.path.starts_with(in_home(r".cursor\projects")))
    );
    assert!(
        inventory
            .projects
            .iter()
            .all(|project| !project.path.starts_with(in_home(r".cursor\projects")))
    );
}

/// The deliberate fake Secret.
const SECRET: &str = "not-a-real-secret";

#[test]
fn a_cursor_server_shows_whole_references_and_masks_literal_secrets() {
    let fs = cursor().with_file(
        user_mcp(),
        format!(
            r#"{{ "mcpServers": {{
                "local": {{
                    "command": "${{userHome}}${{/}}bin${{pathSeparator}}docs-mcp",
                    "args": ["--folder", "${{workspaceFolder}}", "--name", "${{workspaceFolderBasename}}"],
                    "envFile": "C:/secrets/docs.env",
                    "env": {{
                        "FROM_ENV": "${{env:DOCS_TOKEN}}",
                        "NOT_CURSOR": "${{DOCS_TOKEN}}",
                        "LITERAL": "{SECRET}"
                    }}
                }},
                "remote": {{
                    "url": "https://mcp.example.com/mcp",
                    "headers": {{ "Authorization": "Bearer {SECRET}" }},
                    "auth": {{ "CLIENT_ID": "docs-client", "CLIENT_SECRET": "{SECRET}", "scopes": ["read"] }}
                }}
            }} }}"#
        ),
    );

    let inventory = inventory(&fs);

    let local = &item(&inventory, "user-local").entries[0];
    assert_eq!(local.config["env"]["FROM_ENV"], "${env:DOCS_TOKEN}");
    // Cursor replaces no `${NAME}` of another name.
    assert_eq!(local.config["env"]["NOT_CURSOR"], "••••");
    assert_eq!(local.config["env"]["LITERAL"], "••••");
    assert_eq!(local.config["envFile"], "C:/secrets/docs.env");
    assert_eq!(
        local.config["args"],
        json!([
            "--folder",
            "${workspaceFolder}",
            "--name",
            "${workspaceFolderBasename}"
        ])
    );
    assert_eq!(
        local.config["command"],
        "${userHome}${/}bin${pathSeparator}docs-mcp"
    );
    let remote = &item(&inventory, "user-remote").entries[0];
    assert_eq!(remote.config["headers"]["Authorization"], "Bearer ••••");
    assert_eq!(remote.config["auth"]["CLIENT_SECRET"], "••••");
    assert_eq!(remote.config["auth"]["CLIENT_ID"], "docs-client");
    assert!(
        remote
            .masked
            .contains(&vec!["auth".to_owned(), "CLIENT_SECRET".to_owned()])
    );
    assert!(!json_of(&inventory).to_string().contains(SECRET));
}

#[test]
fn a_plugin_s_servers_secrets_are_masked_too() {
    let fs = with_plugin_files(
        plugins_installed(),
        "context7",
        &[(
            ".mcp.json",
            json!({ "mcpServers": { "context7": {
                "url": "https://mcp.context7.com/mcp",
                "headers": { "CONTEXT7_API_KEY": SECRET },
                "auth": { "CLIENT_SECRET": SECRET },
            } } }),
        )],
    );

    let inventory = inventory(&fs);

    let entry = &item(&inventory, "plugin-context7-context7").entries[0];
    assert_eq!(entry.config["auth"]["CLIENT_SECRET"], "••••");
    let cursor: Vec<&Item> = cursor_items(&inventory).collect();
    assert!(!json_of(&cursor).to_string().contains(SECRET));
}

#[test]
fn a_user_setup_of_cursor_is_an_installation_of_its_home() {
    let exe = Path::new(LOCAL_APP_DATA).join(r"Programs\cursor\Cursor.exe");
    let fs = cursor().with_file(&exe, fake::executable(Some([3, 22, 12, 0])));

    let inventory = inventory(&fs);

    let home = inventory
        .homes
        .iter()
        .find(|home| home.agent == "cursor")
        .unwrap();
    let versions: Vec<(&Path, Option<&str>)> = home
        .installations
        .iter()
        .map(|installation| (installation.path.as_path(), installation.version.as_deref()))
        .collect();
    assert_eq!(versions, [(exe.as_path(), Some("3.22.12"))]);
    assert!(!home.out_of_range);
}

#[test]
fn a_user_install_runs_only_while_the_user_settings_turn_it_on() {
    let fs = with_installed(
        context7(cursor()),
        json!({ "context7@market": user_install("context7") }),
        json!({}),
    )
    .with_file(
        project(r"app\.claude\settings.local.json"),
        json!({ "enabledPlugins": { "context7@market": true } }).to_string(),
    );

    let inventory = inventory_at(&fs, &project("app"));

    // Claude Code takes the Project's `true`; Cursor reads the user's alone.
    let entry = &item(&inventory, "plugin-context7-context7").entries[0];
    assert_eq!(code(entry.state), "disabled");
    assert_eq!(
        entry.reason.as_ref().unwrap().switch.as_deref(),
        Some(r"C:\Users\fixture\.claude\settings.json#enabledPlugins.context7@market")
    );
}

#[test]
fn a_project_s_false_keeps_a_user_install_out_of_that_project() {
    let fs = with_installed(
        context7(cursor()),
        json!({ "context7@market": user_install("context7") }),
        json!({ "context7@market": true }),
    )
    .with_file(
        project(r"app\.claude\settings.json"),
        json!({ "enabledPlugins": { "context7@market": false } }).to_string(),
    );

    let inventory = inventory_at(&fs, &project("app"));

    assert_eq!(
        code(item(&inventory, "plugin-context7-context7").state),
        "disabled"
    );
}

#[test]
fn a_local_install_runs_only_while_its_project_s_settings_turn_it_on() {
    let fs = with_installed(
        context7(cursor()),
        json!({ "context7@market": [{
            "scope": "local",
            "projectPath": r"E:\projects\app",
            "installPath": plugin_cache(r"market\context7\1.0.0").display().to_string(),
        }] }),
        json!({ "context7@market": true }),
    )
    .with_folder(project("app"));

    let inventory = inventory_at(&fs, &project("app"));

    let entry = &item(&inventory, "plugin-context7-context7").entries[0];
    assert_eq!(code(entry.state), "disabled");
    assert_eq!(
        entry.reason.as_ref().unwrap().switch.as_deref(),
        Some(r"E:\projects\app\.claude\settings.json#enabledPlugins.context7@market")
    );
}

#[test]
fn a_workspace_file_cursor_opened_is_a_project() {
    let workspace = project(r"multi\multi.code-workspace");
    let fs = cursor()
        .with_file(&workspace, r#"{ "folders": [{ "path": "api" }] }"#)
        .with_file(
            Path::new(USER).join(r"workspaceStorage\4e5f\workspace.json"),
            r#"{"workspace":"file:///e%3A/projects/multi/multi.code-workspace"}"#,
        );

    assert!(recorded_by_cursor(&inventory(&fs), &workspace));
}

// --- Cursor's own plugins ---------------------------------------------------

/// Where Cursor keeps the files of the plugins it installs from a
/// marketplace, a folder per version.
fn cursor_plugin_cache(relative: &str) -> PathBuf {
    in_home(r".cursor\plugins\cache").join(relative)
}

/// The version folder of the plugin `name` of `marketplace` in Cursor's cache.
fn cached(marketplace: &str, name: &str) -> PathBuf {
    cursor_plugin_cache(&format!(
        r"{marketplace}\{name}\2eb7ed4613cfc8f098dfe464a23680ea44d84c5e"
    ))
}

/// `fs` with the files of a plugin in `folder`, each a path in it and its
/// JSON.
fn with_files(mut fs: FakeFileSystem, folder: &Path, files: &[(&str, Value)]) -> FakeFileSystem {
    for (relative, value) in files {
        fs = fs.with_file(folder.join(relative), value.to_string());
    }
    fs
}

/// `fs` with the plugin `name` of `marketplace` in Cursor's cache, complete,
/// holding `files` and the skill `<name>-skill`.
fn with_cached(
    fs: FakeFileSystem,
    marketplace: &str,
    name: &str,
    files: &[(&str, Value)],
) -> FakeFileSystem {
    let folder = cached(marketplace, name);
    let fs = with_files(fs, &folder, files).with_file(folder.join(".cache-complete"), "");
    with_skill(fs, folder.join(format!(r"skills\{name}-skill")))
}

/// PostHog's plugin, a manifest and an `mcp.json` with one remote server.
fn posthog(fs: FakeFileSystem) -> FakeFileSystem {
    with_cached(
        fs,
        "cursor-public",
        "posthog-mcp",
        &[
            (
                r".cursor-plugin\plugin.json",
                json!({ "name": "posthog-mcp" }),
            ),
            (
                "mcp.json",
                json!({ "mcpServers": { "posthog": { "url": "https://mcp.posthog.com/mcp" } } }),
            ),
        ],
    )
}

/// `fs` with `plugins` as the `plugins` of the Project `app`'s
/// `.cursor\settings.json`.
fn with_project_plugins(fs: FakeFileSystem, plugins: Value) -> FakeFileSystem {
    fs.with_file(
        project(r"app\.cursor\settings.json"),
        json!({ "plugins": plugins }).to_string(),
    )
}

/// PostHog's plugin, which the Project `app` installs.
fn posthog_in_app() -> FakeFileSystem {
    with_project_plugins(
        posthog(cursor()),
        json!({ "posthog-mcp": { "enabled": true } }),
    )
}

#[test]
fn a_project_s_marketplace_plugin_s_servers_and_skills_are_items_of_the_plugin() {
    let inventory = inventory_at(&posthog_in_app(), &project("app"));

    let server = item(&inventory, "plugin-posthog-mcp-posthog");
    assert_eq!(code(server.state), "effective");
    let entry = entry_json(&server.entries[0]);
    assert_eq!(
        entry["path"],
        cached("cursor-public", "posthog-mcp")
            .join("mcp.json")
            .display()
            .to_string()
    );
    assert_eq!(entry["scope"], "plugin");
    assert_eq!(entry["provenance"], "plugin");
    assert_eq!(entry["plugin"]["id"], "posthog-mcp");
    let skill = entry_json(&item(&inventory, "posthog-mcp-skill").entries[0]);
    assert_eq!(skill["state"], "effective");
    assert_eq!(skill["provenance"], "plugin");
    assert_eq!(skill["plugin"]["id"], "posthog-mcp");
}

#[test]
fn a_project_s_plugins_apply_in_that_project_only() {
    let fs = posthog_in_app().with_folder(project("other"));

    let in_other = inventory_at(&fs, &project("other"));

    assert!(cursor_items(&in_other).all(|item| item.name != "plugin-posthog-mcp-posthog"));
}

#[test]
fn a_key_naming_a_marketplace_is_read_in_that_marketplace_s_cache() {
    let fs = with_cached(
        cursor(),
        "acme",
        "tracker",
        &[(
            ".mcp.json",
            json!({ "mcpServers": { "issues": { "command": "tracker-mcp" } } }),
        )],
    );
    let fs = with_project_plugins(fs, json!({ "acme/tracker": { "enabled": true } }));

    let inventory = inventory_at(&fs, &project("app"));

    let entry = entry_json(&item(&inventory, "plugin-tracker-issues").entries[0]);
    assert_eq!(entry["state"], "effective");
    assert_eq!(entry["plugin"]["id"], "acme/tracker");
}

#[test]
fn a_plugin_whose_cache_is_incomplete_has_no_entries() {
    let fs = with_project_plugins(cursor(), json!({ "posthog-mcp": { "enabled": true } }))
        .with_file(
            cached("cursor-public", "posthog-mcp").join("mcp.json"),
            json!({ "mcpServers": { "posthog": { "url": "https://mcp.posthog.com/mcp" } } })
                .to_string(),
        );

    let inventory = inventory_at(&fs, &project("app"));

    assert!(cursor_items(&inventory).all(|item| item.name != "plugin-posthog-mcp-posthog"));
}

#[test]
fn a_project_plugin_not_enabled_is_disabled_by_its_enabled_value() {
    for enabled in [json!({ "enabled": false }), json!({})] {
        let fs = with_project_plugins(posthog(cursor()), json!({ "posthog-mcp": enabled }));

        let inventory = inventory_at(&fs, &project("app"));

        for name in ["plugin-posthog-mcp-posthog", "posthog-mcp-skill"] {
            let entry = &item(&inventory, name).entries[0];
            assert_eq!(code(entry.state), "disabled", "{name} with {enabled}");
            let reason = entry.reason.as_ref().unwrap();
            assert_eq!(code(reason.code), "plugin_disabled", "{name}");
            assert_eq!(
                reason.switch.as_deref(),
                Some(r"E:\projects\app\.cursor\settings.json#plugins.posthog-mcp.enabled")
            );
        }
    }
}

/// The plan of `action` on the Items `selectors` in the Project `app`.
fn plan_in_app(fs: &FakeFileSystem, action: Action, selectors: &[&str]) -> (Core, Plan) {
    let core = core(fs);
    let options = ScanOptions {
        projects: vec![project("app")],
    };
    let snapshot = core.scan(&options).unwrap();
    let context = Context {
        project: Some(project("app")),
    };
    let targets = Targets {
        selectors: selectors
            .iter()
            .map(|&selector| selector.to_owned())
            .collect(),
        all: false,
    };
    let plan = core.plan(&snapshot, &context, action, &targets).unwrap();
    (core, plan)
}

#[test]
fn disable_of_a_project_plugin_sets_its_enabled_to_false_keeping_comments() {
    let settings = project(r"app\.cursor\settings.json");
    let fs = posthog(cursor()).with_file(
        &settings,
        "{\n  // Installed from the Marketplace.\n  \"plugins\": {\n    \"posthog-mcp\": { \"enabled\": true, },\n  },\n}\n",
    );

    let (core, plan) = plan_in_app(&fs, Action::Disable, &["plugin-posthog-mcp-posthog"]);

    let reached: Vec<(String, String)> = plan
        .preview()
        .changes
        .iter()
        .map(|change| (change.item.clone(), code(change.after)))
        .collect();
    for name in ["plugin-posthog-mcp-posthog", "posthog-mcp-skill"] {
        assert!(
            reached.contains(&(name.to_owned(), "disabled".to_owned())),
            "{name} not in {reached:?}"
        );
    }
    assert_eq!(plan.preview().files.len(), 1);
    assert_eq!(plan.preview().files[0].path, settings);
    core.execute(plan, &[WarningClass::WideReach]).unwrap();
    let text = String::from_utf8(fs.contents(&settings).unwrap()).unwrap();
    assert!(
        text.contains("// Installed from the Marketplace."),
        "{text}"
    );
    let inventory = inventory_at(&fs, &project("app"));
    assert_eq!(
        code(item(&inventory, "plugin-posthog-mcp-posthog").state),
        "disabled"
    );
}

#[test]
fn enable_of_a_project_plugin_sets_its_enabled_to_true() {
    let settings = project(r"app\.cursor\settings.json");
    let fs = with_project_plugins(
        posthog(cursor()),
        json!({ "posthog-mcp": { "enabled": false } }),
    );

    let (core, plan) = plan_in_app(&fs, Action::Enable, &["posthog-mcp-skill"]);

    core.execute(plan, &[WarningClass::WideReach]).unwrap();
    let written: Value = serde_json::from_slice(&fs.contents(&settings).unwrap()).unwrap();
    assert_eq!(written["plugins"]["posthog-mcp"]["enabled"], true);
    let inventory = inventory_at(&fs, &project("app"));
    assert_eq!(
        code(item(&inventory, "plugin-posthog-mcp-posthog").state),
        "effective"
    );
}

/// The folder of the local plugin `name`.
fn local_plugin(name: &str) -> PathBuf {
    in_home(r".cursor\plugins\local").join(name)
}

#[test]
fn local_plugins_are_items_of_the_plugin_in_every_context() {
    let folder = local_plugin("kit");
    let fs = with_files(
        cursor(),
        &folder,
        &[
            (r".cursor-plugin\plugin.json", json!({ "name": "kit" })),
            (
                ".mcp.json",
                json!({ "mcpServers": { "lint": { "command": "kit-lint" } } }),
            ),
        ],
    );
    let fs = with_skill(fs, folder.join(r"skills\kit-review"));

    let inventory = inventory(&fs);

    let server = entry_json(&item(&inventory, "plugin-kit-lint").entries[0]);
    assert_eq!(server["state"], "effective");
    assert_eq!(server["provenance"], "plugin");
    assert_eq!(server["plugin"]["id"], "kit");
    let skill = entry_json(&item(&inventory, "kit-review").entries[0]);
    assert_eq!(skill["state"], "effective");
    assert_eq!(skill["plugin"]["id"], "kit");
}

#[test]
fn a_plugin_s_skills_in_a_folder_its_manifest_declares_are_read() {
    let folder = local_plugin("kit");
    let fs = with_files(
        cursor(),
        &folder,
        &[(
            r".cursor-plugin\plugin.json",
            json!({ "name": "kit", "skills": "./kit-skills/" }),
        )],
    );
    let fs = with_skill(fs, folder.join(r"kit-skills\kit-review"));

    let inventory = inventory(&fs);

    let skill = entry_json(&item(&inventory, "kit-review").entries[0]);
    assert_eq!(skill["state"], "effective");
    assert_eq!(skill["plugin"]["id"], "kit");
}

#[test]
fn a_manifest_s_list_names_skill_folders_or_folders_of_them() {
    let folder = local_plugin("kit");
    let fs = with_files(
        cursor(),
        &folder,
        &[(
            r".cursor-plugin\plugin.json",
            json!({ "name": "kit", "skills": ["./one/kit-solo", "./many/"] }),
        )],
    );
    let fs = with_skill(fs, folder.join(r"one\kit-solo"));
    let fs = with_skill(fs, folder.join(r"many\kit-review"));
    // Left out once the manifest declares where its skills are.
    let fs = with_skill(fs, folder.join(r"skills\kit-default"));
    let unset = local_plugin("unset");
    let fs = with_files(
        fs,
        &unset,
        &[(r".cursor-plugin\plugin.json", json!({ "skills": null }))],
    );
    let fs = with_skill(fs, unset.join(r"skills\unset-skill"));

    let inventory = inventory(&fs);

    for name in ["kit-solo", "kit-review"] {
        assert_eq!(code(item(&inventory, name).state), "effective", "{name}");
    }
    for name in ["kit-default", "unset-skill"] {
        assert!(
            cursor_items(&inventory).all(|item| item.name != name),
            "{name} is read"
        );
    }
}

#[test]
fn a_plugin_s_manifests_are_tried_in_cursor_s_order() {
    let folder = local_plugin("kit-folder");
    let fs = with_files(
        cursor(),
        &folder,
        &[
            (
                r".claude-plugin\plugin.json",
                json!({ "name": "kit-claude", "mcpServers": { "claude-only": { "command": "x" } } }),
            ),
            (
                "plugin.json",
                json!({ "name": "kit-plain", "mcpServers": { "plain-only": { "command": "y" } } }),
            ),
        ],
    );

    let inventory = inventory(&fs);

    // Named by its folder, whatever its manifest says.
    assert_eq!(
        code(item(&inventory, "plugin-kit-folder-claude-only").state),
        "effective"
    );
    assert!(cursor_items(&inventory).all(|item| !item.name.contains("plain-only")));
}

#[test]
fn a_project_plugin_is_named_by_its_key_and_a_dot_folder_is_no_local_plugin() {
    let fs = with_cached(
        cursor(),
        "acme",
        "tracker",
        &[
            (r".cursor-plugin\plugin.json", json!({ "name": "Tracker" })),
            (
                ".mcp.json",
                json!({ "mcpServers": { "issues": { "command": "tracker-mcp" } } }),
            ),
        ],
    );
    let fs = with_project_plugins(fs, json!({ "tracker": { "enabled": true } })).with_file(
        local_plugin(r".trash\.mcp.json"),
        json!({ "mcpServers": { "old": { "command": "old-mcp" } } }).to_string(),
    );

    let inventory = inventory_at(&fs, &project("app"));

    // A key naming no marketplace is found in any marketplace's cache.
    assert_eq!(
        code(item(&inventory, "plugin-tracker-issues").state),
        "effective"
    );
    assert!(cursor_items(&inventory).all(|item| item.name != "plugin-.trash-old"));
}

#[test]
fn a_project_plugin_s_server_loses_to_a_user_server_with_the_same_url() {
    let fs = posthog_in_app().with_file(
        user_mcp(),
        json!({ "mcpServers": { "posthog": { "url": "https://mcp.posthog.com/mcp/" } } })
            .to_string(),
    );

    let inventory = inventory_at(&fs, &project("app"));

    let user = item(&inventory, "user-posthog");
    let states: Vec<(String, String)> = user
        .entries
        .iter()
        .map(|entry| (entry.locator.clone(), code(entry.state)))
        .collect();
    assert_eq!(states.len(), 2, "{states:?}");
    assert_eq!(states[0].1, "effective");
    assert_eq!(states[1].1, "shadowed");
    assert!(states[1].0.contains("posthog-mcp"), "{states:?}");
}

#[test]
fn cursor_s_own_plugins_need_neither_the_import_gate_nor_third_party_configs() {
    let fs = with_gates(posthog_in_app(), &[("enable_cc_plugin_import", false)]);
    let fs = with_third_party(fs, false);

    let inventory = inventory_at(&fs, &project("app"));

    for name in ["plugin-posthog-mcp-posthog", "posthog-mcp-skill"] {
        assert_eq!(code(item(&inventory, name).state), "effective", "{name}");
    }
}

#[test]
fn a_cursor_plugin_s_servers_secrets_are_masked() {
    let fs = with_project_plugins(
        with_cached(
            cursor(),
            "cursor-public",
            "posthog-mcp",
            &[(
                "mcp.json",
                json!({ "mcpServers": { "posthog": {
                    "url": "https://mcp.posthog.com/mcp",
                    "auth": { "CLIENT_ID": "posthog-client", "CLIENT_SECRET": SECRET },
                } } }),
            )],
        ),
        json!({ "posthog-mcp": { "enabled": true } }),
    );

    let inventory = inventory_at(&fs, &project("app"));

    let entry = &item(&inventory, "plugin-posthog-mcp-posthog").entries[0];
    assert_eq!(entry.config["auth"]["CLIENT_SECRET"], "••••");
    assert_eq!(entry.config["auth"]["CLIENT_ID"], "posthog-client");
    let cursor: Vec<&Item> = cursor_items(&inventory).collect();
    assert!(!json_of(&cursor).to_string().contains(SECRET));
}
