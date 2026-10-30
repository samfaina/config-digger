//! Cursor through the core `api`, with fake ports over fixture Cursor
//! folders and fixture SQLite stores: the MCP servers it reads from its own
//! files and from the Claude Code plugins it imports, each under its own
//! identifier, how it collapses servers with the same URL and keeps Project
//! servers out until they are approved, the skills it reads from five
//! Agents' folders, and the Projects it records.

use config_digger_core::api::{
    Action, Context, Core, EnableOption, Entry, Inventory, Item, Plan, ReadFailure, ScanOptions,
    Targets, WarningClass,
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
    let fs = with_skill(fs, in_home(r".claude\skills\a\b\c\d\e\f\g\deep"));

    let inventory = inventory(&fs);

    let unslop = skills_named(&inventory, "unslop");
    assert_eq!(unslop.len(), 2, "{unslop:?}");
    assert!(unslop.iter().all(|(state, _)| state == "effective"));
    assert!(skills_named(&inventory, "old").is_empty());
    let deep = skills_named(&inventory, "deep");
    assert_eq!(deep.len(), 1, "{deep:?}");
    assert_eq!(deep[0].0, "effective");
}

#[test]
fn a_project_skill_at_the_path_of_a_user_skill_loses_to_it() {
    let fs = with_skill(
        with_skill(
            with_skill(cursor(), in_home(r".cursor\skills\review")),
            project(r"app\.claude\skills\review"),
        ),
        project(r"app\.cursor\skills\team\review"),
    );

    let inventory = inventory_at(&fs, &project("app"));

    let review = skills_named(&inventory, "review");
    assert_eq!(
        review,
        [
            (
                "effective".to_owned(),
                vec![
                    (
                        "effective".to_owned(),
                        in_home(r".cursor\skills\review").display().to_string()
                    ),
                    (
                        "shadowed".to_owned(),
                        project(r"app\.claude\skills\review").display().to_string()
                    ),
                ]
            ),
            (
                "effective".to_owned(),
                vec![(
                    "effective".to_owned(),
                    project(r"app\.cursor\skills\team\review")
                        .display()
                        .to_string()
                )]
            ),
        ]
    );
}

#[test]
fn a_skill_whose_frontmatter_does_not_parse_is_read_under_its_folder_s_name() {
    let fs = cursor()
        .with_file(
            in_home(r".cursor\skills\unclosed\SKILL.md"),
            "---\nname: other\ndescription: Never closes.\n",
        )
        .with_file(
            in_home(r".claude\skills\no-key\SKILL.md"),
            "---\nname: other\njust text\n---\nbody\n",
        )
        .with_file(
            in_home(r".agents\skills\bare\SKILL.md"),
            "No frontmatter.\n",
        );

    let inventory = inventory(&fs);

    for name in ["unclosed", "no-key", "bare"] {
        assert_eq!(code(item(&inventory, name).state), "effective", "{name}");
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

/// The skill `name` whose `SKILL.md` holds `text`, in the fixture Project
/// `app`'s `.cursor\skills` and then in `~\.cursor\skills`, each checked to
/// be Effective.
fn in_project_and_user_folder(name: &str, text: &str) -> [Entry; 2] {
    let in_project = cursor().with_file(
        project(&format!(r"app\.cursor\skills\{name}\SKILL.md")),
        text,
    );
    let at_home = cursor().with_file(in_home(&format!(r".cursor\skills\{name}\SKILL.md")), text);
    let entries = [
        item(&inventory_at(&in_project, &project("app")), name).entries[0].clone(),
        item(&inventory(&at_home), name).entries[0].clone(),
    ];
    for (entry, scope) in entries.iter().zip(["project", "user"]) {
        assert_eq!(entry.scope, scope);
        assert_eq!(code(entry.state), "effective");
    }
    entries
}

/// The sentences of the `unlisted` notes of the skill `name` whose
/// `SKILL.md` holds `text`, in a Project and in the user's home folder, as
/// [`in_project_and_user_folder`] reads it.
fn unlisted_in_project_and_user_folder(name: &str, text: &str) -> Vec<Vec<String>> {
    in_project_and_user_folder(name, text)
        .iter()
        .map(|entry| unlisted(entry).into_iter().map(str::to_owned).collect())
        .collect()
}

/// The note of a skill Cursor leaves out of its model's list because its
/// `SKILL.md` has no `description`.
const NO_DESCRIPTION: &str = "The user can still run this skill in Cursor, but Cursor does not list it to its model: its SKILL.md has no `description`.";

#[test]
fn a_skill_with_no_frontmatter_is_one_cursor_leaves_out_of_its_model_s_list() {
    for bare in in_project_and_user_folder("cd-throwaway-bare", "A skill with no frontmatter.\n") {
        assert!(
            bare.notes
                .iter()
                .any(|note| code(note.code) == "skill_fallback")
        );
        assert_eq!(unlisted(&bare), [NO_DESCRIPTION]);
    }
}

/// The note of a skill Cursor leaves out of its model's list because it
/// cannot parse its `SKILL.md`'s frontmatter.
const MALFORMED: &str = "The user can still run this skill in Cursor, but Cursor does not list it to its model: its SKILL.md has a frontmatter that does not parse.";

#[test]
fn a_skill_whose_yaml_does_not_parse_is_unlisted_for_it() {
    assert_eq!(
        unlisted_in_project_and_user_folder(
            "cd-throwaway-yaml",
            "---\nname: [unclosed\ndescription: YAML that does not parse\n---\n"
        ),
        [[MALFORMED]; 2]
    );
}

/// Checks that the skill `name` whose `SKILL.md` holds `text`, in a
/// Project and in the user's home folder, keeps its `skill_fallback` note
/// and has the `unlisted` note of a frontmatter that does not parse.
fn assert_unlisted_for_not_parsing_with_its_fallback(name: &str, text: &str) {
    for entry in in_project_and_user_folder(name, text) {
        assert!(
            entry
                .notes
                .iter()
                .any(|note| code(note.code) == "skill_fallback")
        );
        assert_eq!(unlisted(&entry), [MALFORMED]);
    }
}

#[test]
fn a_frontmatter_that_never_closes_is_unlisted_for_not_parsing() {
    assert_unlisted_for_not_parsing_with_its_fallback(
        "cd-throwaway-unclosed",
        "---\nname: cd-throwaway-unclosed\ndescription: never closes\nbody\n",
    );
}

#[test]
fn a_frontmatter_with_a_line_that_is_no_key_is_unlisted_for_not_parsing() {
    assert_unlisted_for_not_parsing_with_its_fallback(
        "cd-throwaway-no-key",
        "---\nname: cd-throwaway-no-key\ndescription: probe\na line that is no key\n---\n",
    );
}

#[test]
fn a_yaml_frontmatter_with_a_name_and_no_description_is_unlisted_for_it() {
    assert_eq!(
        unlisted_in_project_and_user_folder(
            "cd-throwaway-no-description",
            "---\nname: cd-throwaway-no-description\n---\nbody\n"
        ),
        [[NO_DESCRIPTION]; 2]
    );
}

#[test]
fn a_skill_with_a_name_and_a_description_is_listed_whatever_disable_model_invocation() {
    for text in [
        "---\nname: cd-throwaway-ok\ndescription: Parses.\n---\n",
        "---\nname: cd-throwaway-ok\ndescription: Parses.\ndisable-model-invocation: true\n---\n",
    ] {
        assert_eq!(
            unlisted_in_project_and_user_folder("cd-throwaway-ok", text),
            [[""; 0]; 2],
            "{text}"
        );
    }
}

#[test]
fn a_shadowed_skill_that_does_not_parse_stays_shadowed_with_the_note() {
    let fs = with_skill(cursor(), in_home(r".cursor\skills\review")).with_file(
        in_home(r".claude\skills\review\SKILL.md"),
        "---\nname: [unclosed\ndescription: Reviews.\n---\n",
    );
    let inventory = inventory(&fs);

    let review = item(&inventory, "review");
    assert!(unlisted(&review.entries[0]).is_empty());
    let claude = &review.entries[1];
    assert_eq!(
        claude.locator,
        in_home(r".claude\skills\review").display().to_string()
    );
    assert_eq!(code(claude.state), "shadowed");
    assert_eq!(unlisted(claude), [MALFORMED]);
}

#[test]
fn a_nested_a_claude_and_a_cursor_plugin_s_skill_with_no_description_are_unlisted() {
    let plain = |name: &str| format!("---\nname: {name}\n---\n");
    let kit = local_plugin("kit");
    let fs = with_files(
        cursor().with_folder(project(r"app\.git")),
        &kit,
        &[(r".cursor-plugin\plugin.json", json!({ "name": "kit" }))],
    )
    .with_file(
        project(r"app\sub\.cursor\skills\nested\SKILL.md"),
        plain("nested"),
    )
    .with_file(in_home(r".claude\skills\claude\SKILL.md"), plain("claude"))
    .with_file(kit.join(r"skills\kit-review\SKILL.md"), plain("kit-review"));
    let inventory = inventory_at(&fs, &project("app"));

    for (name, provenance) in [
        ("nested", "file"),
        ("claude", "file"),
        ("kit-review", "plugin"),
    ] {
        let entry = &item(&inventory, name).entries[0];
        assert_eq!(code(entry.provenance), provenance, "{name}");
        assert_eq!(code(entry.state), "effective", "{name}");
        assert_eq!(unlisted(entry), [NO_DESCRIPTION], "{name}");
    }
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
        ..Targets::default()
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
            matches!(entry.enable, Some(EnableOption::NotOffered { .. })),
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

/// The Project `app` as a git repo, holding a skill at each of `skills`,
/// relative to its folder.
fn git_app_with_skills(skills: &[&str]) -> FakeFileSystem {
    let fs = cursor().with_folder(project(r"app\.git"));
    skills.iter().fold(fs, |fs, skill| {
        with_skill(fs, project(&format!(r"app\{skill}")))
    })
}

/// The locators of the Cursor skill Entries of `inventory`, sorted.
fn skill_locators(inventory: &Inventory) -> Vec<String> {
    let mut locators: Vec<String> = cursor_items(inventory)
        .filter(|item| code(item.kind) == "skill")
        .flat_map(|item| item.entries.iter().map(|entry| entry.locator.clone()))
        .collect();
    locators.sort();
    locators
}

#[test]
fn a_git_project_reads_the_skills_folders_below_its_folder() {
    let fs = git_app_with_skills(&[
        r"sub\.cursor\skills\a",
        r"sub\deep\er\.agents\skills\b",
        r"sub\.claude\skills\c",
    ]);

    let inventory = inventory_at(&fs, &project("app"));

    for (name, path) in [
        ("a", r"app\sub\.cursor\skills\a"),
        ("b", r"app\sub\deep\er\.agents\skills\b"),
        ("c", r"app\sub\.claude\skills\c"),
    ] {
        let skill = item(&inventory, name);
        assert_eq!(code(skill.state), "effective", "{name}");
        let entry = &skill.entries[0];
        assert_eq!(entry.scope, "project", "{name}");
        assert_eq!(entry.locator, project(path).display().to_string());
    }
}

#[test]
fn third_party_configs_off_turn_off_the_nested_claude_skills_too() {
    let fs = with_third_party(git_app_with_skills(&[r"sub\.claude\skills\c"]), false);

    let inventory = inventory_at(&fs, &project("app"));

    let entry = &item(&inventory, "c").entries[0];
    assert_eq!(code(entry.state), "disabled");
    assert_eq!(
        code(entry.reason.as_ref().unwrap().code),
        "disabled_globally"
    );
}

#[test]
fn nothing_is_read_below_a_skipped_folder_name_at_any_depth() {
    let fs = git_app_with_skills(&[
        r"node_modules\pkg\.cursor\skills\in-node-modules",
        r"sub\dist\.agents\skills\in-dist",
        r"sub\.cursor\skills\build\below-build",
        r"sub\.cursor\skills\kept",
    ]);

    let inventory = inventory_at(&fs, &project("app"));

    assert_eq!(
        skill_locators(&inventory),
        [project(r"app\sub\.cursor\skills\kept")
            .display()
            .to_string()]
    );
}

#[test]
fn a_project_outside_a_git_work_tree_reads_no_nested_skills() {
    let fs = git_app_with_skills(&[r".cursor\skills\root", r"sub\.cursor\skills\nested"]);
    let fs = {
        fs.delete_folder(project(r"app\.git"));
        fs
    };

    let inventory = inventory_at(&fs, &project("app"));

    assert_eq!(
        skill_locators(&inventory),
        [project(r"app\.cursor\skills\root").display().to_string()]
    );
}

#[test]
fn a_project_below_the_root_of_its_repo_reads_the_folders_below_it_only() {
    let fs = cursor().with_folder(project(r"repo\.git"));
    let fs = with_skill(fs, project(r"repo\other\.cursor\skills\beside"));
    let fs = with_skill(fs, project(r"repo\app\sub\.cursor\skills\below"));

    let inventory = inventory_at(&fs, &project(r"repo\app"));

    assert_eq!(
        skill_locators(&inventory),
        [project(r"repo\app\sub\.cursor\skills\below")
            .display()
            .to_string()]
    );
}

#[test]
fn a_nested_skill_wins_over_the_project_s_own_and_the_user_s_at_its_path() {
    let fs = git_app_with_skills(&[
        r".cursor\skills\review",
        r"sub\.cursor\skills\review",
        r"sub\.agents\skills\triage",
    ]);
    let fs = with_skill(fs, in_home(r".cursor\skills\triage"));

    let inventory = inventory_at(&fs, &project("app"));

    assert_eq!(
        skills_named(&inventory, "review")[0].1,
        [
            (
                "effective".to_owned(),
                project(r"app\sub\.cursor\skills\review")
                    .display()
                    .to_string()
            ),
            (
                "shadowed".to_owned(),
                project(r"app\.cursor\skills\review").display().to_string()
            ),
        ]
    );
    assert_eq!(
        skills_named(&inventory, "triage")[0].1,
        [
            (
                "effective".to_owned(),
                project(r"app\sub\.agents\skills\triage")
                    .display()
                    .to_string()
            ),
            (
                "shadowed".to_owned(),
                in_home(r".cursor\skills\triage").display().to_string()
            ),
        ]
    );
}

#[test]
fn of_two_nested_skills_at_one_path_the_first_folder_s_wins_and_in_one_folder_cursor_s() {
    let fs = git_app_with_skills(&[
        r"y\.cursor\skills\pair",
        r"x\.cursor\skills\pair",
        r"b\.cursor\skills\deep",
        r"a\b\c\.cursor\skills\deep",
        r"x\.agents\skills\holder",
        r"x\.cursor\skills\holder",
        r"a-b\.cursor\skills\separator",
        r"a\b\.cursor\skills\separator",
    ]);

    let inventory = inventory_at(&fs, &project("app"));

    for (name, effective, shadowed) in [
        (
            "pair",
            r"app\x\.cursor\skills\pair",
            r"app\y\.cursor\skills\pair",
        ),
        (
            "deep",
            r"app\a\b\c\.cursor\skills\deep",
            r"app\b\.cursor\skills\deep",
        ),
        (
            "holder",
            r"app\x\.cursor\skills\holder",
            r"app\x\.agents\skills\holder",
        ),
        (
            "separator",
            r"app\a\b\.cursor\skills\separator",
            r"app\a-b\.cursor\skills\separator",
        ),
    ] {
        assert_eq!(
            skills_named(&inventory, name)[0].1,
            [
                (
                    "effective".to_owned(),
                    project(effective).display().to_string()
                ),
                (
                    "shadowed".to_owned(),
                    project(shadowed).display().to_string()
                ),
            ],
            "{name}"
        );
    }
}

#[test]
fn of_two_nested_skills_at_one_path_the_first_folder_s_wins_whatever_their_markers() {
    let fs = git_app_with_skills(&[r"y\.cursor\skills\kinds", r"x\.agents\skills\kinds"]);

    let inventory = inventory_at(&fs, &project("app"));

    assert_eq!(
        skills_named(&inventory, "kinds")[0].1,
        [
            (
                "effective".to_owned(),
                project(r"app\x\.agents\skills\kinds").display().to_string()
            ),
            (
                "shadowed".to_owned(),
                project(r"app\y\.cursor\skills\kinds").display().to_string()
            ),
        ]
    );
}

#[test]
fn a_nested_claude_skill_in_the_first_folder_wins_while_third_party_configs_are_on() {
    let fs = git_app_with_skills(&[r"y\.cursor\skills\k", r"x\.claude\skills\k"]);
    let (x, y) = (
        project(r"app\x\.claude\skills\k").display().to_string(),
        project(r"app\y\.cursor\skills\k").display().to_string(),
    );

    let on = inventory_at(&with_third_party(fs.clone(), true), &project("app"));
    let off = inventory_at(&with_third_party(fs, false), &project("app"));

    assert_eq!(
        skills_named(&on, "k")[0].1,
        [
            ("effective".to_owned(), x.clone()),
            ("shadowed".to_owned(), y.clone())
        ]
    );
    let off = &skills_named(&off, "k")[0].1;
    assert!(
        off.contains(&("effective".to_owned(), y)) && off.contains(&("disabled".to_owned(), x)),
        "{off:?}"
    );
}

#[test]
fn ignore_files_leave_out_the_folders_down_to_the_marker_not_the_skills_folder() {
    let fs = git_app_with_skills(&[
        r"gi-folder\.cursor\skills\ignored-folder",
        r"gi-skills\.cursor\skills\ignored-skills",
        r"gi-marker\.cursor\skills\ignored-marker",
        r"ig-folder\.cursor\skills\dot-ignore",
        r"sub\kept\.cursor\skills\kept-again",
    ])
    .with_file(
        project(r"app\.gitignore"),
        "gi-folder/\ngi-skills/.cursor/skills/\ngi-marker/.cursor/\n# a comment\n",
    )
    .with_file(project(r"app\.ignore"), "ig-folder/\n")
    .with_file(project(r"app\sub\.gitignore"), "/*\n!/kept/\n");

    let inventory = inventory_at(&fs, &project("app"));

    assert_eq!(
        skill_locators(&inventory),
        [
            project(r"app\gi-skills\.cursor\skills\ignored-skills")
                .display()
                .to_string(),
            project(r"app\sub\kept\.cursor\skills\kept-again")
                .display()
                .to_string(),
        ]
    );
}

#[test]
fn the_ignore_files_above_the_project_folder_apply_too() {
    let fs = cursor()
        .with_folder(project(r"repo\.git"))
        .with_file(project(r"repo\.gitignore"), "app/generated/\n");
    let fs = with_skill(fs, project(r"repo\app\generated\.cursor\skills\left-out"));
    let fs = with_skill(fs, project(r"repo\app\sub\.cursor\skills\kept"));

    let inventory = inventory_at(&fs, &project(r"repo\app"));

    assert_eq!(
        skill_locators(&inventory),
        [project(r"repo\app\sub\.cursor\skills\kept")
            .display()
            .to_string()]
    );
}

#[test]
fn the_walk_follows_links_to_folders() {
    let fs = with_skill(
        git_app_with_skills(&[]),
        project(r"shared\.cursor\skills\linked"),
    )
    .with_link(project(r"app\shared"), project("shared"));

    let inventory = inventory_at(&fs, &project("app"));

    assert_eq!(code(item(&inventory, "linked").state), "effective");
}

#[test]
fn a_project_at_the_user_s_home_folder_reads_its_nested_skills_too() {
    let fs = with_skill(
        cursor().with_folder(in_home(".git")),
        in_home(r"dotfiles\.cursor\skills\nested"),
    );

    let inventory = inventory_at(&fs, Path::new(HOME));

    assert_eq!(
        skill_locators(&inventory),
        [in_home(r"dotfiles\.cursor\skills\nested")
            .display()
            .to_string()]
    );
}

#[test]
fn the_project_s_own_skills_folders_are_read_once() {
    let fs = git_app_with_skills(&[r".cursor\skills\root", r".agents\skills\agents-root"]);

    let inventory = inventory_at(&fs, &project("app"));

    assert_eq!(
        skill_locators(&inventory),
        [
            project(r"app\.agents\skills\agents-root")
                .display()
                .to_string(),
            project(r"app\.cursor\skills\root").display().to_string(),
        ]
    );
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
    let fs = cursor().with_file(&exe, fake::executable(Some([3, 27, 12, 0])));

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
    assert_eq!(versions, [(exe.as_path(), Some("3.27.12"))]);
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
        ..Targets::default()
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
fn a_manifest_whose_servers_are_neither_a_path_an_object_nor_a_list_of_either_has_its_skills_in_the_default_folder()
 {
    let mut fs = cursor();
    for (name, servers) in [
        ("number", json!(42)),
        ("list", json!([42])),
        ("null", json!(null)),
        ("kept", json!(["./servers.json", {}])),
    ] {
        let folder = local_plugin(name);
        fs = with_files(
            fs,
            &folder,
            &[(
                r".cursor-plugin\plugin.json",
                json!({ "name": name, "skills": "./custom", "mcpServers": servers }),
            )],
        );
        fs = with_skill(fs, folder.join(format!(r"custom\{name}-custom")));
        fs = with_skill(fs, folder.join(format!(r"skills\{name}-default")));
    }

    let inventory = inventory(&fs);

    for name in ["number", "list", "null"] {
        assert_eq!(
            code(item(&inventory, &format!("{name}-default")).state),
            "effective",
            "{name}"
        );
        assert!(
            cursor_items(&inventory).all(|item| item.name != format!("{name}-custom")),
            "{name}-custom is read"
        );
    }
    // Paths and objects, in a list or not, leave the manifest read.
    assert_eq!(code(item(&inventory, "kept-custom").state), "effective");
    assert!(cursor_items(&inventory).all(|item| item.name != "kept-default"));
}

#[test]
fn past_a_manifest_that_does_not_parse_or_is_rejected_the_next_manifest_s_skills_are_read() {
    let mut fs = cursor();
    for (name, first) in [
        ("broken", UNPARSABLE.to_owned()),
        (
            "rejected",
            json!({ "skills": "./custom", "mcpServers": 42 }).to_string(),
        ),
    ] {
        for (plugin, next) in [(name.to_owned(), true), (format!("{name}-alone"), false)] {
            let folder = local_plugin(&plugin);
            fs = fs.with_file(folder.join(r".cursor-plugin\plugin.json"), first.as_str());
            if next {
                fs = with_files(
                    fs,
                    &folder,
                    &[(
                        r".claude-plugin\plugin.json",
                        json!({ "name": plugin, "skills": "./other" }),
                    )],
                );
            }
            for skills in ["custom", "other", "skills"] {
                fs = with_skill(fs, folder.join(format!(r"{skills}\{plugin}-{skills}")));
            }
        }
    }

    let inventory = inventory(&fs);

    let listed = |plugin: &str, skills: &str| {
        cursor_items(&inventory).any(|item| item.name == format!("{plugin}-{skills}"))
    };
    for name in ["broken", "rejected"] {
        assert!(listed(name, "other"), "{name}");
        let alone = format!("{name}-alone");
        assert!(listed(&alone, "skills"), "{alone}");
        for (plugin, unread) in [
            (name, ["custom", "skills"]),
            (alone.as_str(), ["custom", "other"]),
        ] {
            for skills in unread {
                assert!(!listed(plugin, skills), "{plugin}-{skills} is listed");
            }
        }
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
fn a_key_naming_no_marketplace_is_read_in_cursor_public_s_cache_only() {
    let tracker = |fs, marketplace, command: &str| {
        with_cached(
            fs,
            marketplace,
            "tracker",
            &[(
                ".mcp.json",
                json!({ "mcpServers": { "issues": { "command": command } } }),
            )],
        )
    };
    // `aaa` comes before `cursor-public` by name; `acme` alone holds `kit`.
    let fs = tracker(cursor(), "aaa", "aaa-mcp");
    let fs = tracker(fs, "cursor-public", "tracker-mcp");
    let fs = with_cached(
        fs,
        "acme",
        "kit",
        &[(
            ".mcp.json",
            json!({ "mcpServers": { "lint": { "command": "kit-lint" } } }),
        )],
    );
    let fs = with_project_plugins(
        fs,
        json!({ "tracker": { "enabled": true }, "kit": { "enabled": true } }),
    );

    let inventory = inventory_at(&fs, &project("app"));

    let entries = &item(&inventory, "plugin-tracker-issues").entries;
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].config["command"], "tracker-mcp");
    assert!(cursor_items(&inventory).all(|item| item.name != "plugin-kit-lint"));
}

#[test]
fn a_project_plugin_is_named_by_its_key_and_a_dot_folder_is_no_local_plugin() {
    let fs = with_cached(
        cursor(),
        "cursor-public",
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

/// What Cursor does about the Read failure of `path`, as its definition says.
fn reaction(inventory: &Inventory, path: &Path) -> String {
    let failure = inventory
        .read_failures
        .iter()
        .find(|failure| failure.agent == "cursor" && failure.path == path)
        .unwrap_or_else(|| panic!("{} is no Read failure", path.display()));
    code(failure.reaction)
}

#[test]
fn cursor_skips_an_mcp_file_it_cannot_parse_or_reads_its_next_manifest_or_around_its_error() {
    let broken = r#"{ "mcpServers": { "a": { "command": "a" },, "b": { "command": "b" } } }"#;
    let kit = local_plugin("kit");
    let project_mcp = project(r"app\.cursor\mcp.json");
    let kit_manifest = kit.join(r".cursor-plugin\plugin.json");
    let kit_mcp = local_plugin("kit-mcp").join("mcp.json");
    let context7_manifest = plugin_cache(r"market\context7\1.0.0\.claude-plugin\plugin.json");
    let docs_mcp = plugin_cache(r"market\docs\1.0.0\.mcp.json");
    let files = [
        user_mcp(),
        project_mcp.clone(),
        kit_manifest.clone(),
        kit_mcp.clone(),
        context7_manifest.clone(),
        docs_mcp.clone(),
    ];
    let fs = with_installed(
        context7(cursor()),
        json!({
            "context7@market": user_install("context7"),
            "docs@market": user_install("docs"),
        }),
        json!({ "context7@market": true, "docs@market": true }),
    );
    let fs = files.iter().fold(fs, |fs, file| fs.with_file(file, broken));
    let fs = with_files(
        fs,
        &kit,
        &[(
            ".mcp.json",
            json!({ "mcpServers": { "lint": { "command": "kit-lint" } } }),
        )],
    );

    let inventory = inventory_at(&fs, &project("app"));

    assert_eq!(reaction(&inventory, &user_mcp()), "ignores_file");
    assert_eq!(reaction(&inventory, &project_mcp), "ignores_file");
    // Cursor reads the plugin's next manifest instead.
    for file in [&kit_manifest, &context7_manifest] {
        assert_eq!(
            reaction(&inventory, file),
            "reads_next_path",
            "{}",
            file.display()
        );
    }
    // It starts the servers around an `mcp.json`'s error.
    for file in [&kit_mcp, &docs_mcp] {
        assert_eq!(
            reaction(&inventory, file),
            "reads_around_error",
            "{}",
            file.display()
        );
    }
    // A plugin whose manifest does not parse still has its `.mcp.json` read.
    assert_eq!(code(item(&inventory, "plugin-kit-lint").state), "effective");
    assert_eq!(
        code(item(&inventory, "plugin-context7-context7").state),
        "effective"
    );
}

/// The Cursor Read failure of `path`.
fn read_failure<'i>(inventory: &'i Inventory, path: &Path) -> &'i ReadFailure {
    inventory
        .read_failures
        .iter()
        .find(|failure| failure.agent == "cursor" && failure.path == path)
        .unwrap_or_else(|| panic!("{} is no Read failure", path.display()))
}

#[test]
fn a_plugin_s_servers_file_with_an_error_lists_the_servers_around_it_with_no_caveat() {
    let broken = r#"{ "mcpServers": { "a": { "command": "a" },, "b": { "command": "b" } } }"#;
    let kit_mcp = local_plugin("kit").join("mcp.json");
    let docs_mcp = plugin_cache(r"market\docs\1.0.0\.mcp.json");
    let fs = with_installed(
        cursor(),
        json!({ "docs@market": user_install("docs") }),
        json!({ "docs@market": true }),
    );
    let fs = with_gates(fs, &[("enable_cc_plugin_import", true)])
        .with_file(&kit_mcp, broken)
        .with_file(&docs_mcp, broken);

    let inventory = inventory(&fs);

    for name in [
        "plugin-kit-a",
        "plugin-kit-b",
        "plugin-docs-a",
        "plugin-docs-b",
    ] {
        let item = item(&inventory, name);
        assert_eq!(code(item.state), "effective", "{name}");
        assert!(item.caveats.is_empty(), "{name}: {:?}", item.caveats);
    }
    for file in [&kit_mcp, &docs_mcp] {
        let failure = read_failure(&inventory, file);
        assert_eq!(code(failure.kind), "parse", "{}", file.display());
        assert_eq!(code(failure.reaction), "reads_around_error");
        let position = failure.position.as_ref().unwrap();
        assert_eq!((position.line, position.column), (1, 43));
        assert_eq!(
            failure.notes,
            ["Cursor reads what is around the error, and config-digger lists what it keeps."]
        );
    }
}

// Comments, trailing commas and four errors besides `,,`, in the texts
// `cursor.mjs` gives the real Cursor, each case asserting what it started
// (#438).

/// What config-digger reads of a servers file holding `text`, in a local
/// plugin's `mcp.json` and then in a Claude Code plugin's `.mcp.json`: the
/// servers it lists, and whether the file is a Read failure.
fn servers_read(text: &str) -> [(Vec<String>, bool); 2] {
    let kit_mcp = local_plugin("kit").join("mcp.json");
    let docs_mcp = plugin_cache(r"market\docs\1.0.0\.mcp.json");
    let fs = with_installed(
        cursor(),
        json!({ "docs@market": user_install("docs") }),
        json!({ "docs@market": true }),
    );
    let fs = fs.with_file(&kit_mcp, text).with_file(&docs_mcp, text);
    let inventory = inventory(&fs);
    [("kit", &kit_mcp), ("docs", &docs_mcp)].map(|(plugin, file)| {
        let prefix = format!("plugin-{plugin}-");
        let mut names: Vec<String> = cursor_items(&inventory)
            .filter_map(|item| item.name.strip_prefix(&prefix))
            .map(str::to_owned)
            .collect();
        names.sort();
        let failed = inventory
            .read_failures
            .iter()
            .any(|failure| failure.agent == "cursor" && &failure.path == file);
        (names, failed)
    })
}

/// `servers`, listed from a file that parses, in both kinds of plugin.
fn parsed(servers: &[&str]) -> [(Vec<String>, bool); 2] {
    let names: Vec<String> = servers.iter().map(|&name| name.to_owned()).collect();
    [(names.clone(), false), (names, false)]
}

/// `servers`, listed around the file's error, in both kinds of plugin.
fn around_error(servers: &[&str]) -> [(Vec<String>, bool); 2] {
    parsed(servers).map(|(names, _)| (names, true))
}

#[test]
fn a_servers_file_with_a_line_comment_between_servers_parses() {
    let text = r#"{ "mcpServers": {
  "a": {"command":"a"},
  // cd-throwaway
  "b": {"command":"b"}
} }"#;

    assert_eq!(servers_read(text), parsed(&["a", "b"]));
}

#[test]
fn a_servers_file_with_a_block_comment_between_servers_parses() {
    let text = r#"{ "mcpServers": {
  "a": {"command":"a"},
  /* cd-throwaway */
  "b": {"command":"b"}
} }"#;

    assert_eq!(servers_read(text), parsed(&["a", "b"]));
}

#[test]
fn a_servers_file_with_a_trailing_comma_after_the_last_server_parses() {
    let text = r#"{ "mcpServers": {
  "a": {"command":"a"},
  "b": {"command":"b"},
} }"#;

    assert_eq!(servers_read(text), parsed(&["a", "b"]));
}

#[test]
fn a_servers_file_with_a_trailing_comma_after_a_server_s_last_field_parses() {
    let text = r#"{ "mcpServers": {
  "a": {"command":"a"},
  "b": {"command":"b","args":["b"],}
} }"#;

    assert_eq!(servers_read(text), parsed(&["a", "b"]));
}

#[test]
fn a_servers_file_missing_a_comma_between_servers_lists_them_all() {
    let text = r#"{ "mcpServers": {
  "a": {"command":"a"}
  "b": {"command":"b"},
  "c": {"command":"c"}
} }"#;

    assert_eq!(servers_read(text), around_error(&["a", "b", "c"]));
}

#[test]
fn a_servers_file_key_with_no_value_lists_the_servers_after_it() {
    let text = r#"{ "mcpServers": {
  "a": ,
  "b": {"command":"b"},
  "c": {"command":"c"}
} }"#;

    assert_eq!(servers_read(text), around_error(&["b", "c"]));
}

#[test]
fn a_servers_file_left_open_at_its_end_lists_its_servers() {
    let text = r#"{ "mcpServers": {
  "a": {"command":"a"},
  "b": {"command":"b"}
"#;

    assert_eq!(servers_read(text), around_error(&["a", "b"]));
}

#[test]
fn a_string_left_open_in_a_server_takes_the_next_server_into_it() {
    // The string runs to the end of its line, swallowing the `}` and `,`,
    // so the next server is read as a member of `b`.
    let text = r#"{ "mcpServers": {
  "a": {"command":"a"},
  "b": {"command":"b", "note": "open },
  "c": {"command":"c"}
} }"#;

    assert_eq!(servers_read(text), around_error(&["a", "b"]));
}

/// The text of a manifest that does not parse.
const UNPARSABLE: &str = r#"{ "name": "broken",, "mcpServers": { "a": { "command": "a" } } }"#;

#[test]
fn of_a_local_plugin_whose_first_manifest_does_not_parse_cursor_reads_the_next() {
    let kit = local_plugin("kit");
    let broken = kit.join(r".cursor-plugin\plugin.json");
    let fs = with_files(
        cursor(),
        &kit,
        &[
            (
                r".claude-plugin\plugin.json",
                json!({ "name": "kit", "mcpServers": { "next": { "command": "kit-next" } } }),
            ),
            (
                ".mcp.json",
                json!({ "mcpServers": { "lint": { "command": "kit-lint" } } }),
            ),
        ],
    )
    .with_file(&broken, UNPARSABLE);

    let inventory = inventory(&fs);

    let next = &item(&inventory, "plugin-kit-next").entries[0];
    assert_eq!(code(next.state), "effective");
    assert_eq!(next.path, kit.join(r".claude-plugin\plugin.json"));
    assert_eq!(code(item(&inventory, "plugin-kit-lint").state), "effective");
    let failure = read_failure(&inventory, &broken);
    assert_eq!(code(failure.kind), "parse");
    assert_eq!(code(failure.reaction), "reads_next_path");
    assert_eq!(
        failure.notes,
        [
            "Cursor reads the first it can of the plugin's `.claude-plugin/plugin.json` or `plugin.json` instead of this file."
        ]
    );
    assert_eq!(
        inventory
            .read_failures
            .iter()
            .filter(|failure| failure.agent == "cursor")
            .count(),
        1
    );
}

/// The Cursor Read failures of files in `folder`.
fn read_failures_in<'i>(inventory: &'i Inventory, folder: &Path) -> Vec<&'i ReadFailure> {
    inventory
        .read_failures
        .iter()
        .filter(|failure| failure.agent == "cursor" && failure.path.starts_with(folder))
        .collect()
}

#[test]
fn a_manifest_whose_servers_are_neither_a_path_an_object_nor_a_list_of_either_is_rejected_whole() {
    let shapes = [
        ("number", json!(42)),
        ("list", json!([42])),
        ("null", json!(null)),
        ("mixed", json!(["./servers.json", 42])),
    ];
    let mut fs = cursor();
    for (name, servers) in &shapes {
        for (plugin, next) in [(name.to_string(), true), (format!("{name}-alone"), false)] {
            let folder = local_plugin(&plugin);
            let mut files = vec![
                (
                    r".cursor-plugin\plugin.json",
                    json!({ "name": plugin, "mcpServers": servers }),
                ),
                (
                    "servers.json",
                    json!({ "mcpServers": { "file": { "command": "file-mcp" } } }),
                ),
            ];
            if next {
                files.push((
                    r".claude-plugin\plugin.json",
                    json!({ "name": plugin, "mcpServers": { "next": { "command": "next-mcp" } } }),
                ));
            }
            fs = with_files(fs, &folder, &files);
        }
    }
    for (name, servers) in [
        ("path", json!("./servers.json")),
        ("path-and-object", json!(["./servers.json", {}])),
    ] {
        fs = with_files(
            fs,
            &local_plugin(name),
            &[
                (
                    r".cursor-plugin\plugin.json",
                    json!({ "name": name, "mcpServers": servers }),
                ),
                (
                    "servers.json",
                    json!({ "mcpServers": { "file": { "command": "file-mcp" } } }),
                ),
            ],
        );
    }

    let inventory = inventory(&fs);

    for (name, _) in &shapes {
        for plugin in [name.to_string(), format!("{name}-alone")] {
            let folder = local_plugin(&plugin);
            let failures = read_failures_in(&inventory, &folder);
            assert_eq!(failures.len(), 1, "{plugin}: {failures:?}");
            let failure = failures[0];
            assert_eq!(failure.path, folder.join(r".cursor-plugin\plugin.json"));
            assert_eq!(code(failure.kind), "shape", "{plugin}");
            assert_eq!(code(failure.reaction), "reads_next_path", "{plugin}");
            assert!(failure.location.is_none(), "{plugin}");
            assert!(
                failure.sentence.contains("mcpServers"),
                "{}",
                failure.sentence
            );
            assert!(
                cursor_items(&inventory).all(|item| item.name != format!("plugin-{plugin}-file")),
                "{plugin}: a server of the rejected manifest is read"
            );
        }
        assert_eq!(
            code(item(&inventory, &format!("plugin-{name}-next")).state),
            "effective",
            "{name}"
        );
    }
    for name in ["path", "path-and-object"] {
        assert_eq!(
            code(item(&inventory, &format!("plugin-{name}-file")).state),
            "effective",
            "{name}"
        );
        assert!(read_failures_in(&inventory, &local_plugin(name)).is_empty());
    }
}

#[test]
fn past_the_last_manifest_cursor_tries_no_other() {
    let kit = local_plugin("kit");
    let last = kit.join("plugin.json");
    let fs = with_skill(
        cursor().with_file(&last, UNPARSABLE),
        kit.join(r"skills\kit-default"),
    );

    let inventory = inventory(&fs);

    let failure = read_failure(&inventory, &last);
    assert_eq!(code(failure.reaction), "reads_next_path");
    assert_eq!(
        failure.notes,
        ["Cursor tries no file of the plugin's list after this one."]
    );
    assert_eq!(code(item(&inventory, "kit-default").state), "effective");
}

#[test]
fn a_file_a_manifest_points_to_is_no_path_cursor_reads_past() {
    let kit = local_plugin("kit");
    let servers = kit.join("servers.json");
    let fs = with_files(
        cursor(),
        &kit,
        &[(
            r".cursor-plugin\plugin.json",
            json!({ "name": "kit", "mcpServers": "./servers.json" }),
        )],
    )
    .with_file(&servers, UNPARSABLE);

    let inventory = inventory(&fs);

    // Ignored, as a file references name is when it does not parse.
    let failure = read_failure(&inventory, &servers);
    assert_eq!(code(failure.reaction), "ignores_file");
    assert!(failure.notes.is_empty(), "{:?}", failure.notes);
}

#[test]
fn of_a_claude_code_plugin_whose_first_manifest_does_not_parse_cursor_reads_the_next() {
    let broken = plugin_cache(r"market\kit\1.0.0\.cursor-plugin\plugin.json");
    let fs = with_plugin_files(
        cursor(),
        "kit",
        &[
            (
                r".claude-plugin\plugin.json",
                json!({ "name": "kit", "mcpServers": { "next": { "command": "kit-next" } } }),
            ),
            (
                ".mcp.json",
                json!({ "mcpServers": { "lint": { "command": "kit-lint" } } }),
            ),
        ],
    )
    .with_file(&broken, UNPARSABLE);
    let fs = with_installed(
        with_gates(fs, &[("enable_cc_plugin_import", true)]),
        json!({ "kit@market": user_install("kit") }),
        json!({ "kit@market": true }),
    );

    let inventory = inventory(&fs);

    let next = &item(&inventory, "plugin-kit-next").entries[0];
    assert_eq!(code(next.state), "effective");
    assert_eq!(
        next.path,
        plugin_cache(r"market\kit\1.0.0\.claude-plugin\plugin.json")
    );
    assert_eq!(code(item(&inventory, "plugin-kit-lint").state), "effective");
    let failure = read_failure(&inventory, &broken);
    assert_eq!(code(failure.reaction), "reads_next_path");
}

#[test]
fn a_claude_code_plugin_whose_first_manifest_is_rejected_is_named_by_the_next() {
    let fs = with_plugin_files(
        cursor(),
        "kit",
        &[
            (
                r".cursor-plugin\plugin.json",
                json!({ "name": "rejected-kit", "mcpServers": 42 }),
            ),
            (
                r".claude-plugin\plugin.json",
                json!({ "name": "next-kit", "mcpServers": { "next": { "command": "kit-next" } } }),
            ),
        ],
    );
    let fs = with_installed(
        fs,
        json!({ "kit@market": user_install("kit") }),
        json!({ "kit@market": true }),
    );

    let inventory = inventory(&fs);

    assert_eq!(
        code(item(&inventory, "plugin-next-kit-next").state),
        "effective"
    );
}

#[test]
fn of_a_server_a_claude_code_plugin_s_manifest_and_mcp_json_both_declare_the_manifest_s_wins() {
    let fs = with_plugin_files(
        plugins_installed(),
        "context7",
        &[(
            r".claude-plugin\plugin.json",
            json!({ "name": "context7", "mcpServers": {
                "context7": { "command": "context7-manifest" },
                "docs": { "command": "docs-mcp" },
            } }),
        )],
    );

    let inventory = inventory(&fs);

    let states: Vec<(String, String)> = item(&inventory, "plugin-context7-context7")
        .entries
        .iter()
        .map(|entry| (entry.config["command"].to_string(), code(entry.state)))
        .collect();
    assert_eq!(
        states,
        [
            ("\"context7-manifest\"".to_owned(), "effective".to_owned()),
            ("\"npx\"".to_owned(), "shadowed".to_owned()),
        ]
    );
    assert_eq!(
        code(item(&inventory, "plugin-context7-docs").state),
        "effective"
    );
}
