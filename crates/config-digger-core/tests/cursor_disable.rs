//! Cursor's Disable and approvals through the core `api`, with fake ports
//! over fixture Cursor folders and fixture SQLite stores: the
//! `cursor/disabledMcpServers` list Cursor keeps in each workspace's
//! `state.vscdb`, or behind a Feature gate the profile's
//! `cursor/disabledGlobalMcpServers`, written only while `Cursor.exe` is not
//! running; the `enabled` of a Project's plugin; skills parked in
//! config-digger's store, since Cursor has no switch for them; and what
//! config-digger leaves alone: comments it never writes into Cursor's
//! `mcp.json`, and the approvals Cursor keeps of Project servers.

use config_digger_core::api::{
    Action, Context, Core, Error, Inventory, Item, Plan, ScanOptions, Targets, WarningClass,
};
use config_digger_core::ports::RunningProcess;
use config_digger_core::ports::fake::{
    self, FakeEnvironment, FakeFileSystem, FakeGitIndex, FakeProcesses,
};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const HOME: &str = r"C:\Users\fixture";
const APP_DATA: &str = r"C:\Users\fixture\AppData\Roaming";
const LOCAL_APP_DATA: &str = r"C:\Users\fixture\AppData\Local";

/// The folder where Cursor keeps the user's own data.
const USER: &str = r"C:\Users\fixture\AppData\Roaming\Cursor\User";

/// Where the fixture Projects go.
const PROJECTS: &str = r"E:\projects";

/// Cursor's store of application state, every window's.
fn global_state() -> PathBuf {
    Path::new(USER).join(r"globalStorage\state.vscdb")
}

/// The store Cursor keeps for the Project `app`.
fn app_state() -> PathBuf {
    Path::new(USER).join(r"workspaceStorage\a1b2\state.vscdb")
}

fn in_home(relative: &str) -> PathBuf {
    Path::new(HOME).join(relative)
}

fn project(name: &str) -> PathBuf {
    Path::new(PROJECTS).join(name)
}

/// The user's `~\.cursor\mcp.json`.
fn user_mcp() -> PathBuf {
    in_home(r".cursor\mcp.json")
}

/// The `.cursor\mcp.json` of the Project app.
fn app_mcp() -> PathBuf {
    project("app").join(r".cursor\mcp.json")
}

/// A Cursor that has run once, with the user's server `fetch`, and that
/// keeps a store for the Project app, which has its server `docs`, approved.
fn cursor() -> FakeFileSystem {
    FakeFileSystem::new()
        .with_file(Path::new(USER).join("settings.json"), "{}\n")
        .with_file(
            user_mcp(),
            "{\n  \"mcpServers\": {\n    \"fetch\": { \"command\": \"uvx\", \"args\": [\"mcp-server-fetch\"] }\n  }\n}\n",
        )
        .with_file(
            app_mcp(),
            json!({ "mcpServers": { "docs": { "command": "docs-mcp" } } }).to_string(),
        )
        .with_file(
            Path::new(USER).join(r"workspaceStorage\a1b2\workspace.json"),
            r#"{ "folder": "file:///e%3A/projects/app" }"#,
        )
        .with_sqlite_item(app_state(), "workbench.panel.chat", "{}")
        .with_sqlite_item(
            global_state(),
            "cursor/approvedProjectMcpServers",
            r#"["project-0-app-docs"]"#,
        )
}

/// The same, with the gate that keeps the Disables of servers outside
/// Projects in the profile cached off: the user's server then goes in the
/// workspace's list too.
fn cursor_gate_off() -> FakeFileSystem {
    profile_scoped(cursor(), false)
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

/// A core over `fs` whose git index holds `versioned`.
fn core_versioning(fs: &FakeFileSystem, versioned: &[PathBuf]) -> Core {
    let mut ports = fake::ports(fs.clone(), env());
    ports.git = Box::new(versioned.iter().fold(FakeGitIndex::default(), |git, path| {
        git.with_versioned(path)
    }));
    Core::new(ports).unwrap()
}

/// A core over `fs` while `Cursor.exe` runs from the user setup.
fn core_while_cursor_runs(fs: &FakeFileSystem) -> Core {
    let mut ports = fake::ports(fs.clone(), env());
    ports.processes = Box::new(FakeProcesses::default().with_process(RunningProcess {
        pid: 4242,
        image: "Cursor.exe".to_owned(),
        path: Some(PathBuf::from(format!(
            r"{LOCAL_APP_DATA}\Programs\cursor\Cursor.exe"
        ))),
        cmdline: "Cursor.exe".to_owned(),
    }));
    Core::new(ports).unwrap()
}

fn context(project: Option<&Path>) -> Context {
    Context {
        project: project.map(Path::to_path_buf),
    }
}

/// The context of the Project app.
fn in_app() -> Context {
    context(Some(&project("app")))
}

fn scan_options(context: &Context) -> ScanOptions {
    ScanOptions {
        projects: context.project.iter().cloned().collect(),
    }
}

fn inventory(core: &Core, context: &Context) -> Inventory {
    let snapshot = core.scan(&scan_options(context)).unwrap();
    core.resolve(&snapshot, context)
}

/// The Cursor Item named `name`.
fn item<'i>(inventory: &'i Inventory, name: &str) -> &'i Item {
    inventory
        .items
        .iter()
        .find(|item| item.agent == "cursor" && item.name == name)
        .unwrap_or_else(|| {
            let names: Vec<&str> = inventory
                .items
                .iter()
                .filter(|item| item.agent == "cursor")
                .map(|item| item.name.as_str())
                .collect();
            panic!("no Cursor Item `{name}` among {names:?}")
        })
}

fn json_of(value: impl serde::Serialize) -> Value {
    serde_json::to_value(value).unwrap()
}

fn code(value: impl serde::Serialize) -> String {
    json_of(value).as_str().unwrap().to_owned()
}

fn targets(selectors: &[&str]) -> Targets {
    Targets {
        selectors: selectors.iter().map(|s| (*s).to_owned()).collect(),
        all: false,
    }
}

fn plan(core: &Core, context: &Context, action: Action, selectors: &[&str]) -> Result<Plan, Error> {
    let snapshot = core.scan(&scan_options(context)).unwrap();
    core.plan(&snapshot, context, action, &targets(selectors))
}

/// What the store at `path` holds under `key`, as JSON.
fn item_value(fs: &FakeFileSystem, path: &Path, key: &str) -> Option<Value> {
    use config_digger_core::ports::FileSystem;
    fs.read_sqlite_item(path, key)
        .unwrap()
        .map(|text| serde_json::from_str(&text).unwrap())
}

fn locator(path: &Path, key_path: &str) -> String {
    format!("{}#{key_path}", path.display())
}

#[test]
fn a_read_failure_of_cursor_s_mcp_json_warns_it_may_drop_servers_and_for_a_project_their_sign_in() {
    // Cursor forgets the sign-in of the user's servers only when the file
    // stops parsing while it runs, never when it starts on such a file.
    for (path, context, risks) in [
        (user_mcp(), context(None), &["agent_may_drop_entries"][..]),
        (
            app_mcp(),
            in_app(),
            &["agent_may_drop_entries", "agent_may_clear_auth"][..],
        ),
    ] {
        let fs = cursor().with_file(&path, "{ \"mcpServers\": { ");

        let inventory = inventory(&core(&fs), &context);

        let failure = inventory
            .read_failures
            .iter()
            .find(|failure| failure.agent == "cursor" && failure.path == path)
            .unwrap_or_else(|| panic!("no Read failure of {}", path.display()));
        let codes: Vec<String> = failure
            .warnings
            .iter()
            .map(|warning| code(warning.code))
            .collect();
        assert_eq!(codes, risks, "{}", path.display());
        assert!(
            failure.warnings[0]
                .sentence
                .starts_with("Fix this before starting Cursor"),
            "{}",
            failure.warnings[0].sentence
        );
    }
}

/// The user's server `name`.
fn user_server(name: &str) -> String {
    locator(&user_mcp(), &format!("mcpServers.{name}"))
}

/// The Project app's server `name`.
fn app_server(name: &str) -> String {
    locator(&app_mcp(), &format!("mcpServers.{name}"))
}

#[test]
fn a_server_cursor_lists_in_the_workspace_s_disabled_servers_is_disabled_natively() {
    let fs = cursor_gate_off().with_sqlite_item(
        app_state(),
        "cursor/disabledMcpServers",
        r#"["user-fetch","project-0-app-docs"]"#,
    );

    let inventory = inventory(&core(&fs), &in_app());

    for name in ["user-fetch", "project-0-app-docs"] {
        let entry = &item(&inventory, name).entries[0];
        assert_eq!(code(entry.state), "disabled", "{name}");
        let reason = entry.reason.as_ref().unwrap();
        assert_eq!(code(reason.code), "disabled_natively");
        assert_eq!(
            reason.switch.as_deref(),
            Some(locator(&app_state(), "cursor/disabledMcpServers").as_str())
        );
    }
    // The list is the Project's own: elsewhere the server runs.
    let elsewhere = inventory_without_project(&fs);
    assert_eq!(code(item(&elsewhere, "user-fetch").state), "effective");
}

fn inventory_without_project(fs: &FakeFileSystem) -> Inventory {
    inventory(&core(fs), &context(None))
}

#[test]
fn disable_in_a_project_adds_the_server_s_identifier_to_the_workspace_s_list() {
    let fs = cursor_gate_off();
    let core = core(&fs);

    for (server, name) in [
        (user_server("fetch"), "user-fetch"),
        (app_server("docs"), "project-0-app-docs"),
    ] {
        let plan = plan(&core, &in_app(), Action::Disable, &[&server]).unwrap();
        assert_eq!(plan.preview().files[0].path, app_state());
        assert!(plan.blocks().is_empty(), "{:?}", plan.blocks());
        let outcome = core.execute(plan, &[WarningClass::RunningAgent]).unwrap();
        assert_eq!(code(outcome.verify), "verified");
        let inventory = inventory(&core, &in_app());
        assert_eq!(code(item(&inventory, name).state), "disabled");
    }
    assert_eq!(
        item_value(&fs, &app_state(), "cursor/disabledMcpServers"),
        Some(json!(["user-fetch", "project-0-app-docs"]))
    );
    // The store's other values stay as they were.
    assert_eq!(
        item_value(&fs, &app_state(), "workbench.panel.chat"),
        Some(json!({}))
    );
}

#[test]
fn enable_takes_the_identifier_out_of_the_workspace_s_list() {
    let fs = cursor_gate_off().with_sqlite_item(
        app_state(),
        "cursor/disabledMcpServers",
        r#"["user-fetch","user-other"]"#,
    );
    let core = core(&fs);

    let plan = plan(&core, &in_app(), Action::Enable, &[&user_server("fetch")]).unwrap();
    core.execute(plan, &[]).unwrap();

    assert_eq!(
        item_value(&fs, &app_state(), "cursor/disabledMcpServers"),
        Some(json!(["user-other"]))
    );
    let inventory = inventory(&core, &in_app());
    assert_eq!(code(item(&inventory, "user-fetch").state), "effective");
}

#[test]
fn an_undo_of_a_disable_puts_the_workspace_s_list_back() {
    let fs = cursor_gate_off().with_sqlite_item(app_state(), "cursor/disabledMcpServers", "[]");
    let core = core(&fs);
    let plan = plan(&core, &in_app(), Action::Disable, &[&user_server("fetch")]).unwrap();
    let outcome = core.execute(plan, &[]).unwrap();

    let undo = core.plan_undo(&outcome.operation).unwrap();
    core.execute(undo, &[]).unwrap();

    assert_eq!(
        item_value(&fs, &app_state(), "cursor/disabledMcpServers"),
        Some(json!([]))
    );
}

#[test]
fn a_write_to_cursor_s_store_is_blocked_while_cursor_runs() {
    let fs = cursor().with_file(
        format!(r"{LOCAL_APP_DATA}\Programs\cursor\Cursor.exe"),
        fake::executable(Some([3, 22, 12, 0])),
    );
    let core = core_while_cursor_runs(&fs);

    let plan = plan(&core, &in_app(), Action::Disable, &[&user_server("fetch")]).unwrap();
    let block = &plan.blocks()[0];
    assert_eq!(code(block.code), "running_agent_sqlite");
    assert!(block.sentence.contains("Cursor"), "{}", block.sentence);
    let error = core
        .execute(plan, &[WarningClass::RunningAgent, WarningClass::WideReach])
        .unwrap_err();

    assert_eq!(code(error.code), "blocked");
    assert_eq!(
        item_value(&fs, &app_state(), "cursor/disabledMcpServers"),
        None
    );
}

#[test]
fn without_a_project_a_user_server_has_no_list_to_go_in() {
    let inventory = inventory_without_project(&cursor_gate_off());

    let entry = &item(&inventory, "user-fetch").entries[0];
    let disable = json_of(&entry.disable);
    assert_eq!(disable["code"], "no_mechanism", "{disable}");
}

#[test]
fn a_project_cursor_keeps_no_store_for_yet_offers_no_disable() {
    let fs = cursor().with_file(
        project("new").join(r".cursor\mcp.json"),
        json!({ "mcpServers": { "docs": { "command": "docs-mcp" } } }).to_string(),
    );

    let inventory = inventory(&core(&fs), &context(Some(&project("new"))));

    let entry = &item(&inventory, "project-0-new-docs").entries[0];
    let disable = json_of(&entry.disable);
    assert_eq!(disable["code"], "no_mechanism", "{disable}");
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

/// `fs` with the gate that keeps the Disables of servers outside Projects
/// in the profile cached as `on`.
fn profile_scoped(fs: FakeFileSystem, on: bool) -> FakeFileSystem {
    fs.with_sqlite_item(
        global_state(),
        "workbench.experiments.statsigBootstrap",
        &bootstrap(&[("mcp_profile_scoped_global_disabled_state", on)]),
    )
}

#[test]
fn while_the_profile_scoped_gate_is_on_a_user_server_is_disabled_in_the_profile_s_list() {
    let fs = profile_scoped(cursor(), true)
        .with_sqlite_item(
            global_state(),
            "cursor/disabledGlobalMcpServers",
            r#"["user-fetch"]"#,
        )
        .with_sqlite_item(
            app_state(),
            "cursor/disabledMcpServers",
            r#"["user-other"]"#,
        );

    for context in [context(None), in_app()] {
        let inventory = inventory(&core(&fs), &context);
        let entry = &item(&inventory, "user-fetch").entries[0];
        assert_eq!(code(entry.state), "disabled");
        assert_eq!(
            entry.reason.as_ref().unwrap().switch.as_deref(),
            Some(locator(&global_state(), "cursor/disabledGlobalMcpServers").as_str())
        );
    }
}

#[test]
fn while_the_profile_scoped_gate_is_on_the_workspace_s_list_holds_only_project_servers() {
    let fs = profile_scoped(cursor(), true).with_sqlite_item(
        app_state(),
        "cursor/disabledMcpServers",
        r#"["user-fetch","project-0-app-docs"]"#,
    );

    let inventory = inventory(&core(&fs), &in_app());

    assert_eq!(code(item(&inventory, "user-fetch").state), "effective");
    assert_eq!(
        code(item(&inventory, "project-0-app-docs").state),
        "disabled"
    );
}

#[test]
fn while_the_profile_scoped_gate_is_on_disable_of_a_user_server_writes_the_profile_s_list() {
    let fs = profile_scoped(cursor(), true);
    let core = core(&fs);

    let plan = plan(
        &core,
        &context(None),
        Action::Disable,
        &[&user_server("fetch")],
    )
    .unwrap();
    assert_eq!(plan.preview().files[0].path, global_state());
    // The profile's list applies in every Project.
    assert!(
        plan.warnings()
            .iter()
            .any(|warning| code(warning.class) == "wide-reach"),
        "{:?}",
        plan.warnings()
    );
    core.execute(plan, &[WarningClass::WideReach]).unwrap();

    assert_eq!(
        item_value(&fs, &global_state(), "cursor/disabledGlobalMcpServers"),
        Some(json!(["user-fetch"]))
    );
    let project_server = plan_in_app_disable(&core, &app_server("docs"));
    assert_eq!(project_server, app_state());
}

/// The file Disable of `server` in the Project app writes.
fn plan_in_app_disable(core: &Core, server: &str) -> PathBuf {
    let plan = plan(core, &in_app(), Action::Disable, &[server]).unwrap();
    plan.preview().files[0].path.clone()
}

#[test]
fn the_profile_scoped_gate_is_assumed_on() {
    let fs = cursor().with_sqlite_item(
        global_state(),
        "cursor/disabledGlobalMcpServers",
        r#"["user-fetch"]"#,
    );

    let inventory = inventory(&core(&fs), &in_app());

    let entry = &item(&inventory, "user-fetch").entries[0];
    assert_eq!(code(entry.state), "disabled");
    let caveat = entry
        .caveats
        .iter()
        .find(|caveat| {
            caveat
                .sentence
                .contains("mcp_profile_scoped_global_disabled_state")
        })
        .expect("a caveat for the gate");
    assert_eq!(code(caveat.code), "gate_assumed");
}

fn skill_md(name: &str) -> String {
    format!("---\nname: {name}\ndescription: The {name} skill.\n---\n\nDo the {name} thing.\n")
}

/// `fs` with a skill folder at `folder`, named by its last component.
fn with_skill(fs: FakeFileSystem, folder: &Path) -> FakeFileSystem {
    let name = folder.file_name().unwrap().to_string_lossy().into_owned();
    fs.with_file(folder.join("SKILL.md"), skill_md(&name))
}

/// The `.cursor\settings.json` of the Project app.
fn app_settings() -> PathBuf {
    project("app").join(r".cursor\settings.json")
}

/// `fs` with the Marketplace plugin `review-kit` in Cursor's cache, with a
/// server and a skill, which the Project app installs.
fn review_kit_in_app(fs: FakeFileSystem) -> FakeFileSystem {
    let folder = in_home(r".cursor\plugins\cache\cursor-public\review-kit\2eb7ed46");
    let fs = fs
        .with_file(folder.join(".cache-complete"), "")
        .with_file(
            folder.join("mcp.json"),
            json!({ "mcpServers": { "review": { "url": "https://review.example.com/mcp" } } })
                .to_string(),
        )
        .with_file(
            app_settings(),
            json!({ "plugins": { "review-kit": { "enabled": true } } }).to_string(),
        );
    with_skill(fs, &folder.join(r"skills\review-skill"))
}

#[test]
fn a_project_plugin_in_a_versioned_settings_file_offers_no_disable() {
    let fs = review_kit_in_app(cursor());
    let core = core_versioning(&fs, &[app_settings()]);

    let inventory = inventory(&core, &in_app());

    for name in ["plugin-review-kit-review", "review-skill"] {
        let entry = &item(&inventory, name).entries[0];
        let disable = json_of(&entry.disable);
        assert_eq!(disable["code"], "git_versioned", "{name}: {disable}");
        let sentence = disable["sentence"].as_str().unwrap();
        assert!(
            sentence.contains(&app_settings().display().to_string()),
            "{sentence}"
        );
    }
    let Err(error) = plan(&core, &in_app(), Action::Disable, &["review-skill"]) else {
        panic!("Disable was planned");
    };
    assert_eq!(code(error.code), "blocked");
}

#[test]
fn a_project_plugin_in_a_settings_file_of_no_repo_is_disabled_by_its_enabled() {
    let fs = review_kit_in_app(cursor());
    let core = core(&fs);

    let plan = plan(&core, &in_app(), Action::Disable, &["review-skill"]).unwrap();
    assert_eq!(plan.preview().files[0].path, app_settings());
    core.execute(plan, &[WarningClass::WideReach]).unwrap();

    let text = String::from_utf8(fs.contents(app_settings()).unwrap()).unwrap();
    let settings: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(settings["plugins"]["review-kit"]["enabled"], json!(false));
}

/// The manifest of config-digger's parked-Entry store.
fn parked_store() -> PathBuf {
    PathBuf::from(LOCAL_APP_DATA).join(r"config-digger\parked\manifest.json")
}

/// The folder where config-digger's store keeps the skill parked as `id`.
fn parked_folder(id: &str) -> PathBuf {
    PathBuf::from(LOCAL_APP_DATA)
        .join(r"config-digger\parked\folders")
        .join(id)
}

fn read_json(fs: &FakeFileSystem, path: &Path) -> Value {
    serde_json::from_slice(&fs.contents(path).unwrap()).unwrap()
}

/// The user's Cursor skill `name`.
fn user_skill(name: &str) -> PathBuf {
    in_home(r".cursor\skills").join(name)
}

/// `fs` with the user's Cursor skill `review`.
fn with_review(fs: FakeFileSystem) -> FakeFileSystem {
    with_skill(fs, &user_skill("review"))
}

/// Disables the user's skill `review`, outside any Project.
fn park_review(fs: &FakeFileSystem) -> String {
    let core = core(fs);
    let selector = user_skill("review").display().to_string();
    let plan = plan(&core, &context(None), Action::Disable, &[&selector]).unwrap();
    core.execute(plan, EVERY_PROJECT).unwrap().operation
}

#[test]
fn disable_of_a_user_skill_parks_its_folder_in_config_digger_s_store() {
    let fs = with_review(cursor());
    let core = core(&fs);
    let selector = user_skill("review").display().to_string();

    let plan = plan(&core, &context(None), Action::Disable, &[&selector]).unwrap();
    let folder = &plan.preview().folders[0];
    assert_eq!(folder.path, user_skill("review"));
    assert_eq!(code(folder.change), "parked");
    assert_eq!(plan.preview().files[0].path, parked_store());
    let outcome = core.execute(plan, EVERY_PROJECT).unwrap();
    assert_eq!(code(outcome.verify), "verified");

    assert!(fs.contents(user_skill(r"review\SKILL.md")).is_none());
    assert_eq!(
        fs.contents(parked_folder(r"review\SKILL.md")),
        Some(skill_md("review").into_bytes())
    );
    let parked = &read_json(&fs, &parked_store())["entries"]["review"];
    assert_eq!(parked["agent"], "cursor");
    assert_eq!(parked["location"], "user-cursor-skills");
    assert_eq!(parked["path"], json!(user_skill("review")));
    assert_eq!(parked["folder"], "review");
}

#[test]
fn parking_a_skill_while_cursor_runs_warns_that_it_reads_the_folder() {
    let fs = with_review(cursor()).with_file(
        format!(r"{LOCAL_APP_DATA}\Programs\cursor\Cursor.exe"),
        fake::executable(Some([3, 22, 12, 0])),
    );
    let core = core_while_cursor_runs(&fs);
    let selector = user_skill("review").display().to_string();

    let plan = plan(&core, &context(None), Action::Disable, &[&selector]).unwrap();

    let warning = plan
        .warnings()
        .iter()
        .find(|warning| warning.class == WarningClass::RunningAgent)
        .expect("a running-agent warning");
    assert!(warning.sentence.contains("Cursor"), "{}", warning.sentence);
    assert!(
        warning.sentence.contains(r".cursor\skills\review"),
        "{}",
        warning.sentence
    );
}

#[test]
fn a_parked_skill_is_listed_as_disabled_where_it_was() {
    let fs = with_review(cursor());
    park_review(&fs);

    let inventory = inventory_without_project(&fs);

    let review = item(&inventory, "review");
    let entry = &review.entries[0];
    assert_eq!(code(entry.state), "disabled");
    assert_eq!(code(entry.reason.as_ref().unwrap().code), "parked");
    let parked = entry.parked.as_ref().unwrap();
    assert_eq!(parked.path, user_skill("review"));
    assert_eq!(parked.locator, user_skill("review").display().to_string());
    assert!(entry.enable.is_some());
}

#[test]
fn enable_of_a_parked_skill_moves_its_folder_back() {
    let fs = with_review(cursor());
    park_review(&fs);
    let core = core(&fs);
    let parked = locator(&parked_store(), "entries.review");

    let plan = plan(&core, &context(None), Action::Enable, &[&parked]).unwrap();
    assert_eq!(code(plan.preview().folders[0].change), "put_back");
    core.execute(plan, EVERY_PROJECT).unwrap();

    assert_eq!(
        fs.contents(user_skill(r"review\SKILL.md")),
        Some(skill_md("review").into_bytes())
    );
    assert!(!exists(&fs, &parked_folder("review")));
    assert_eq!(read_json(&fs, &parked_store())["entries"], json!({}));
    let inventory = inventory_without_project(&fs);
    assert_eq!(code(item(&inventory, "review").state), "effective");
}

#[test]
fn an_undo_of_a_park_moves_the_folder_back_and_an_undo_of_that_parks_it_again() {
    let fs = with_review(cursor());
    let operation = park_review(&fs);
    let core = core(&fs);

    let undo = core.plan_undo(&operation).unwrap();
    let undone = core.execute(undo, EVERY_PROJECT).unwrap();
    assert!(fs.contents(user_skill(r"review\SKILL.md")).is_some());
    assert!(!exists(&fs, &parked_folder("review")));

    let undo_of_undo = core.plan_undo(&undone.operation).unwrap();
    core.execute(undo_of_undo, EVERY_PROJECT).unwrap();
    assert!(fs.contents(user_skill(r"review\SKILL.md")).is_none());
    assert!(fs.contents(parked_folder(r"review\SKILL.md")).is_some());
    let inventory = inventory_without_project(&fs);
    assert_eq!(code(item(&inventory, "review").state), "disabled");
}

#[test]
fn enable_of_a_parked_skill_is_refused_when_a_skill_is_at_its_folder_again() {
    let fs = with_review(cursor());
    park_review(&fs);
    let fs = with_review(fs);
    let core = core(&fs);
    let parked = locator(&parked_store(), "entries.review");

    let Err(error) = plan(&core, &context(None), Action::Enable, &[&parked]) else {
        panic!("Enable was planned");
    };

    assert_eq!(code(error.code), "blocked");
    assert!(
        error
            .message
            .contains(&user_skill("review").display().to_string()),
        "{}",
        error.message
    );
}

#[test]
fn remove_of_a_parked_skill_takes_it_out_of_the_store() {
    let fs = with_review(cursor());
    park_review(&fs);
    let core = core(&fs);
    let parked = locator(&parked_store(), "entries.review");

    let plan = plan(&core, &context(None), Action::Remove, &[&parked]).unwrap();
    assert_eq!(plan.preview().folders[0].path, parked_folder("review"));
    let outcome = core.execute(plan, EVERY_PROJECT).unwrap();

    assert!(!exists(&fs, &parked_folder("review")));
    assert_eq!(read_json(&fs, &parked_store())["entries"], json!({}));
    let inventory = inventory_without_project(&fs);
    assert!(
        !inventory
            .items
            .iter()
            .any(|item| item.agent == "cursor" && item.name == "review")
    );

    // Its Undo puts it back in the store, still parked.
    let undo = core.plan_undo(&outcome.operation).unwrap();
    core.execute(undo, EVERY_PROJECT).unwrap();
    assert!(fs.contents(parked_folder(r"review\SKILL.md")).is_some());
    let inventory = inventory_without_project(&fs);
    assert_eq!(code(item(&inventory, "review").state), "disabled");
}

/// The skill `name` of the Project app's `.cursor\skills`.
fn app_skill(name: &str) -> PathBuf {
    project("app").join(r".cursor\skills").join(name)
}

#[test]
fn a_project_skill_outside_any_repo_is_parked() {
    let fs = with_skill(cursor(), &app_skill("lint"));
    let core = core(&fs);
    let selector = app_skill("lint").display().to_string();

    let plan = plan(&core, &in_app(), Action::Disable, &[&selector]).unwrap();
    core.execute(plan, &[]).unwrap();

    assert!(fs.contents(parked_folder(r"lint\SKILL.md")).is_some());
    let inventory = inventory(&core, &in_app());
    assert_eq!(code(item(&inventory, "lint").state), "disabled");
}

#[test]
fn a_project_skill_in_a_versioned_folder_offers_no_disable() {
    let fs = with_skill(cursor(), &app_skill("lint"));
    let core = core_versioning(&fs, &[app_skill("lint")]);

    let inventory = inventory(&core, &in_app());

    let entry = &item(&inventory, "lint").entries[0];
    let disable = json_of(&entry.disable);
    assert_eq!(disable["code"], "git_versioned", "{disable}");
    let sentence = disable["sentence"].as_str().unwrap();
    assert!(
        sentence.contains(&app_skill("lint").display().to_string()),
        "{sentence}"
    );
}

/// Whether anything is at `path` in `fs`.
fn exists(fs: &FakeFileSystem, path: &Path) -> bool {
    use config_digger_core::ports::FileSystem;
    fs.exists(path)
}

/// What an Operation on a user skill acknowledges: the skill applies in
/// every Project, the Project app Cursor records among them.
const EVERY_PROJECT: &[WarningClass] = &[WarningClass::WideReach];

#[test]
fn disable_of_a_skill_reached_through_a_link_takes_the_link_away_and_enable_creates_it_again() {
    let target = PathBuf::from(r"E:\skills\shared");
    let link = user_skill("shared");
    let fs = with_skill(cursor(), &target)
        .with_folder(in_home(r".cursor\skills"))
        .with_link(&link, &target);
    let core = core(&fs);
    let selector = link.display().to_string();

    let disable = plan(&core, &context(None), Action::Disable, &[&selector]).unwrap();
    assert_eq!(code(disable.preview().folders[0].change), "link_removed");
    core.execute(disable, EVERY_PROJECT).unwrap();

    assert!(!exists(&fs, &link));
    assert!(fs.contents(target.join("SKILL.md")).is_some());
    let parked = &read_json(&fs, &parked_store())["entries"]["shared"];
    assert_eq!(parked["link"], json!(target));
    let inventory = inventory_without_project(&fs);
    assert_eq!(code(item(&inventory, "shared").state), "disabled");

    let parked = locator(&parked_store(), "entries.shared");
    let enable = plan(&core, &context(None), Action::Enable, &[&parked]).unwrap();
    assert_eq!(code(enable.preview().folders[0].change), "link_created");
    core.execute(enable, EVERY_PROJECT).unwrap();

    use config_digger_core::ports::FileSystem;
    assert_eq!(fs.link_target(&link), Some(target));
    let inventory = inventory_without_project(&fs);
    assert_eq!(code(item(&inventory, "shared").state), "effective");
}

#[test]
fn an_undo_of_a_remove_puts_a_server_back_into_cursor_s_mcp_json_without_its_comments() {
    let fs = cursor().with_file(
        user_mcp(),
        "{\n  \"mcpServers\": {\n    // Fetches pages.\n    \"fetch\": { \"command\": \"uvx\" },\n    \"docs\": { \"command\": \"docs-mcp\" }\n  }\n}\n",
    );
    let core = core(&fs);
    let plan = plan(
        &core,
        &context(None),
        Action::Remove,
        &[&user_server("fetch")],
    )
    .unwrap();
    let outcome = core.execute(plan, &[WarningClass::RunningAgent]).unwrap();

    let undo = core.plan_undo(&outcome.operation).unwrap();
    core.execute(undo, &[WarningClass::RunningAgent]).unwrap();

    let text = String::from_utf8(fs.contents(user_mcp()).unwrap()).unwrap();
    assert!(!text.contains("//"), "{text}");
    let servers: Value = serde_json::from_str(&text).unwrap();
    assert_eq!(servers["mcpServers"]["fetch"], json!({ "command": "uvx" }));
}

#[test]
fn remove_of_an_approved_project_server_says_its_approval_is_left_as_it_is() {
    let fs = cursor();
    let core = core(&fs);

    let plan = plan(&core, &in_app(), Action::Remove, &[&app_server("docs")]).unwrap();
    let info = plan
        .preview()
        .info
        .iter()
        .find(|info| code(info.code) == "approval_kept")
        .expect("a note on the approval");
    assert_eq!(info.path, app_mcp());
    assert!(
        info.sentence.contains("project-0-app-docs"),
        "{}",
        info.sentence
    );
    assert!(info.sentence.contains("Cursor"), "{}", info.sentence);
    // config-digger writes nothing but the server's file.
    let written: Vec<&Path> = plan
        .preview()
        .files
        .iter()
        .map(|file| file.path.as_path())
        .collect();
    assert_eq!(written, [app_mcp().as_path()]);
    core.execute(plan, &[WarningClass::RunningAgent]).unwrap();

    assert_eq!(
        item_value(&fs, &global_state(), "cursor/approvedProjectMcpServers"),
        Some(json!(["project-0-app-docs"]))
    );
}

#[test]
fn remove_of_a_project_server_cursor_never_approved_has_no_approval_to_leave() {
    let fs = cursor().with_sqlite_item(global_state(), "cursor/approvedProjectMcpServers", "[]");

    let plan = plan(
        &core(&fs),
        &in_app(),
        Action::Remove,
        &[&app_server("docs")],
    )
    .unwrap();

    assert!(
        !plan
            .preview()
            .info
            .iter()
            .any(|info| code(info.code) == "approval_kept"),
        "{:?}",
        plan.preview().info
    );
}
