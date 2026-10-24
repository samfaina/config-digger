//! VS Code's Disable and gates through the core `api`, with fake ports over
//! fixture VS Code folders and fixture SQLite stores: the `mcp.enablement`
//! and `chat.disabledPromptFiles.skill` switches VS Code keeps in
//! `state.vscdb`, the writes that set and lift them while `Code.exe` is not
//! running, and what keeps VS Code from using an Entry that Enable cannot
//! change: Workspace Trust, the trust of a discovered server, the account's
//! policy and the settings that turn every server or skill off.

use config_digger_core::api::{
    Action, Context, Core, Error, Inventory, Item, Plan, ScanOptions, Targets, WarningClass,
};
use config_digger_core::ports::RunningProcess;
use config_digger_core::ports::fake::{self, FakeEnvironment, FakeFileSystem, FakeProcesses};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const HOME: &str = r"C:\Users\fixture";
const APP_DATA: &str = r"C:\Users\fixture\AppData\Roaming";
const LOCAL_APP_DATA: &str = r"C:\Users\fixture\AppData\Local";

/// The default profile's folder, where VS Code keeps the user's own files.
const USER: &str = r"C:\Users\fixture\AppData\Roaming\Code\User";

/// Where the fixture Projects go.
const PROJECTS: &str = r"E:\projects";

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

/// The default profile's own store.
fn profile_store() -> PathBuf {
    PathBuf::from(format!(r"{USER}\globalStorage\state.vscdb"))
}

/// The store VS Code keeps for the Project `app`.
fn app_store() -> PathBuf {
    PathBuf::from(format!(r"{USER}\workspaceStorage\0a1b\state.vscdb"))
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

/// A core over `fs` while `Code.exe` runs from the user setup.
fn core_while_vs_code_runs(fs: &FakeFileSystem) -> Core {
    let mut ports = fake::ports(fs.clone(), env());
    ports.processes = Box::new(FakeProcesses::default().with_process(RunningProcess {
        pid: 5150,
        image: "Code.exe".to_owned(),
        path: Some(PathBuf::from(format!(
            r"{LOCAL_APP_DATA}\Programs\Microsoft VS Code\Code.exe"
        ))),
        cmdline: "Code.exe".to_owned(),
    }));
    Core::new(ports).unwrap()
}

fn context(project: Option<&Path>) -> Context {
    Context {
        project: project.map(Path::to_path_buf),
    }
}

fn inventory(core: &Core, context: &Context) -> Inventory {
    let options = ScanOptions {
        projects: context.project.iter().cloned().collect(),
    };
    let snapshot = core.scan(&options).unwrap();
    core.resolve(&snapshot, context)
}

/// The VS Code Item named `name` in the default profile.
fn item<'i>(inventory: &'i Inventory, name: &str) -> &'i Item {
    inventory
        .items
        .iter()
        .find(|item| item.agent == "vscode" && item.home == "default" && item.name == name)
        .unwrap_or_else(|| panic!("no VS Code Item `{name}`"))
}

fn code(value: impl serde::Serialize) -> String {
    serde_json::to_value(value)
        .unwrap()
        .as_str()
        .unwrap()
        .to_owned()
}

/// Each Entry of `item` as its State and locator, in precedence order.
fn states(item: &Item) -> Vec<(String, String)> {
    item.entries
        .iter()
        .map(|entry| (code(entry.state), entry.locator.clone()))
        .collect()
}

fn state(state: &str, locator: &str) -> (String, String) {
    (state.to_owned(), locator.to_owned())
}

/// The user `mcp.json`'s server `name`.
fn user_server(name: &str) -> String {
    format!(r"{USER}\mcp.json#servers.{name}")
}

fn targets(selectors: &[&str]) -> Targets {
    Targets {
        selectors: selectors.iter().map(|s| (*s).to_owned()).collect(),
        all: false,
    }
}

fn plan(core: &Core, context: &Context, action: Action, selectors: &[&str]) -> Result<Plan, Error> {
    let options = ScanOptions {
        projects: context.project.iter().cloned().collect(),
    };
    let snapshot = core.scan(&options).unwrap();
    core.plan(&snapshot, context, action, &targets(selectors))
}

/// What the store at `path` holds under `key`, as JSON.
fn item_value(fs: &FakeFileSystem, path: &Path, key: &str) -> Option<Value> {
    use config_digger_core::ports::FileSystem;
    fs.read_sqlite_item(path, key)
        .unwrap()
        .map(|text| serde_json::from_str(&text).unwrap())
}

#[test]
fn a_server_vs_code_disabled_in_the_profile_s_enablement_is_disabled_natively() {
    let fs = vscode().with_sqlite_item(
        profile_store(),
        "mcp.enablement",
        r#"[["mcp.config.usrlocal.fetch",false]]"#,
    );
    let inventory = inventory(&core(&fs), &context(None));

    let fetch = item(&inventory, "fetch");
    assert_eq!(states(fetch), [state("disabled", &user_server("fetch"))]);
    let reason = fetch.entries[0].reason.as_ref().unwrap();
    assert_eq!(code(reason.code), "disabled_natively");
    assert_eq!(
        reason.switch.as_deref(),
        Some(format!(r#"{}#"mcp.enablement".0"#, profile_store().display()).as_str())
    );
    assert_eq!(code(item(&inventory, "github").state), "effective");
}

/// The user `github`, which a workspace of the Project `app` names
/// `GitHub` too, in each store.
fn with_github_pairs(profile: &str, workspace: &str) -> FakeFileSystem {
    with_project(vscode(), "app")
        .with_sqlite_item(profile_store(), "mcp.enablement", profile)
        .with_sqlite_item(app_store(), "mcp.enablement", workspace)
}

#[test]
fn a_workspace_s_pair_decides_over_the_profile_s_even_when_it_is_true() {
    let fs = with_github_pairs(
        r#"[["mcp.config.usrlocal.github",false],["mcp.config.ws0.GitHub",false]]"#,
        r#"[["mcp.config.ws0.GitHub",true]]"#,
    );
    let inventory = inventory(&core(&fs), &context(Some(&project("app"))));

    let github = item(&inventory, "GitHub");
    assert_eq!(code(github.entries[0].state), "effective");
    assert_eq!(code(github.entries[1].state), "disabled");
    let reason = github.entries[1].reason.as_ref().unwrap();
    assert!(
        reason.sentence.contains("`mcp.enablement`"),
        "{}",
        reason.sentence
    );
}

#[test]
fn without_a_pair_in_the_workspace_s_store_the_profile_s_decides() {
    let fs = with_github_pairs(
        r#"[["mcp.config.ws0.GitHub",false]]"#,
        r#"[["mcp.config.usrlocal.github",true]]"#,
    );
    let inventory = inventory(&core(&fs), &context(Some(&project("app"))));

    // The Project's `GitHub` is off, so the user's wins.
    assert_eq!(
        states(item(&inventory, "GitHub"))[..2],
        [
            state(
                "disabled",
                &format!(r"{PROJECTS}\app\.vscode\mcp.json#servers.GitHub")
            ),
            state("effective", &user_server("github")),
        ]
    );
}

#[test]
fn disable_without_a_project_pairs_the_server_with_false_in_the_profile_s_store() {
    let fs = vscode().with_sqlite_item(profile_store(), "mcp.enablement", "[]");
    let core = core(&fs);

    let plan = plan(
        &core,
        &context(None),
        Action::Disable,
        &[&user_server("fetch")],
    )
    .unwrap();
    let file = &plan.preview().files[0];
    assert_eq!(file.path, profile_store());
    assert!(plan.blocks().is_empty(), "{:?}", plan.blocks());
    // The profile's switch applies in every Project.
    assert_eq!(code(plan.warnings()[0].class), "wide-reach");
    let outcome = core.execute(plan, &[WarningClass::WideReach]).unwrap();

    assert_eq!(code(outcome.verify), "verified");
    assert_eq!(
        item_value(&fs, &profile_store(), "mcp.enablement"),
        Some(json!([["mcp.config.usrlocal.fetch", false]]))
    );
    let inventory = inventory(&core, &context(None));
    assert_eq!(code(item(&inventory, "fetch").state), "disabled");
}

#[test]
fn disable_in_a_project_writes_the_workspace_s_store_and_the_next_server_wins() {
    let fs = with_project(vscode(), "app");
    let core = core(&fs);
    let app = context(Some(&project("app")));
    let github = format!(r"{PROJECTS}\app\.vscode\mcp.json#servers.GitHub");

    let plan = plan(&core, &app, Action::Disable, &[&github]).unwrap();
    assert_eq!(plan.preview().files[0].path, app_store());
    let effective = plan
        .preview()
        .effective
        .iter()
        .find(|effective| effective.home == "default")
        .unwrap();
    assert_eq!(
        effective.after.as_deref(),
        Some(user_server("github").as_str())
    );
    core.execute(plan, &[]).unwrap();

    assert_eq!(
        item_value(&fs, &app_store(), "mcp.enablement"),
        Some(json!([["mcp.config.ws0.GitHub", false]]))
    );
    // The store's other values stay as they were.
    assert!(item_value(&fs, &app_store(), "mcp.extCachedServers").is_some());
    let inventory = inventory(&core, &app);
    assert_eq!(
        states(item(&inventory, "GitHub"))[..2],
        [
            state("disabled", &github),
            state("effective", &user_server("github"))
        ]
    );
}

#[test]
fn an_undo_of_a_disable_puts_the_store_s_value_back() {
    let fs = vscode().with_sqlite_item(
        profile_store(),
        "mcp.enablement",
        r#"[["mcp.config.usrlocal.github",true]]"#,
    );
    let core = core(&fs);
    let plan = plan(
        &core,
        &context(None),
        Action::Disable,
        &[&user_server("fetch")],
    )
    .unwrap();
    let outcome = core.execute(plan, &[WarningClass::WideReach]).unwrap();

    let undo = core.plan_undo(&outcome.operation).unwrap();
    core.execute(undo, &[WarningClass::WideReach]).unwrap();

    assert_eq!(
        item_value(&fs, &profile_store(), "mcp.enablement"),
        Some(json!([["mcp.config.usrlocal.github", true]]))
    );
}

#[test]
fn enable_takes_the_server_s_pair_out_of_the_workspace_s_store_and_the_profile_s() {
    let fs = with_github_pairs(
        r#"[["mcp.config.ws0.GitHub",false],["mcp.config.usrlocal.github",true]]"#,
        r#"[["mcp.config.ws0.GitHub",false]]"#,
    );
    let core = core(&fs);
    let app = context(Some(&project("app")));
    let github = format!(r"{PROJECTS}\app\.vscode\mcp.json#servers.GitHub");

    let plan = plan(&core, &app, Action::Enable, &[&github]).unwrap();
    let mut written: Vec<&Path> = plan
        .preview()
        .files
        .iter()
        .map(|file| file.path.as_path())
        .collect();
    written.sort();
    assert_eq!(written, [profile_store().as_path(), app_store().as_path()]);
    core.execute(plan, &[WarningClass::WideReach]).unwrap();

    assert_eq!(
        item_value(&fs, &app_store(), "mcp.enablement"),
        Some(json!([]))
    );
    assert_eq!(
        item_value(&fs, &profile_store(), "mcp.enablement"),
        Some(json!([["mcp.config.usrlocal.github", true]]))
    );
    let inventory = inventory(&core, &app);
    assert_eq!(
        code(item(&inventory, "GitHub").entries[0].state),
        "effective"
    );
}

#[test]
fn a_write_to_a_store_is_blocked_while_vs_code_runs_and_no_ack_lifts_it() {
    let fs = with_project(vscode(), "app").with_file(
        format!(r"{LOCAL_APP_DATA}\Programs\Microsoft VS Code\Code.exe"),
        fake::executable(Some([1, 129, 1, 0])),
    );
    let core = core_while_vs_code_runs(&fs);
    let app = context(Some(&project("app")));
    let github = format!(r"{PROJECTS}\app\.vscode\mcp.json#servers.GitHub");

    let plan = plan(&core, &app, Action::Disable, &[&github]).unwrap();
    let block = &plan.blocks()[0];
    assert_eq!(code(block.code), "running_agent_sqlite");
    assert!(block.sentence.contains("VS Code"), "{}", block.sentence);
    let error = core
        .execute(
            plan,
            &[
                WarningClass::RunningAgent,
                WarningClass::WideReach,
                WarningClass::GitVersioned,
            ],
        )
        .unwrap_err();

    assert_eq!(code(error.code), "blocked");
    assert_eq!(item_value(&fs, &app_store(), "mcp.enablement"), None);
}

/// `fs` with the fixture user's home folder.
fn with_home(fs: FakeFileSystem) -> FakeFileSystem {
    load(fs, &fixture_folder("home"), Path::new(HOME))
}

/// The personal skill `name` in `~\.copilot\skills`.
fn copilot_skill(name: &str) -> String {
    format!(r"{HOME}\.copilot\skills\{name}")
}

#[test]
fn a_skill_vs_code_lists_in_its_disabled_skill_files_is_disabled() {
    // VS Code writes a URI with the drive letter as the folder holds it,
    // and reads it back without regard to case.
    let fs = with_home(vscode()).with_sqlite_item(
        profile_store(),
        "chat.disabledPromptFiles.skill",
        r#"[{"$mid":1,"external":"file:///c%3A/Users/fixture/.copilot/skills/jetpack-compose/SKILL.md","path":"/c:/Users/fixture/.copilot/skills/jetpack-compose/SKILL.md","scheme":"file"}]"#,
    );
    let inventory = inventory(&core(&fs), &context(None));

    let skill = item(&inventory, "jetpack-compose");
    assert_eq!(code(skill.state), "disabled");
    let reason = skill.entries[0].reason.as_ref().unwrap();
    assert_eq!(code(reason.code), "disabled_natively");
}

#[test]
fn disable_of_a_skill_adds_its_skill_file_to_the_profile_s_disabled_skill_files() {
    let fs = with_home(vscode()).with_sqlite_item(profile_store(), "other", "1");
    let core = core(&fs);

    let plan = plan(
        &core,
        &context(None),
        Action::Disable,
        &[&copilot_skill("jetpack-compose")],
    )
    .unwrap();
    core.execute(plan, &[WarningClass::WideReach]).unwrap();

    assert_eq!(
        item_value(&fs, &profile_store(), "chat.disabledPromptFiles.skill"),
        Some(json!([{
            "$mid": 1,
            "scheme": "file",
            "path": "/C:/Users/fixture/.copilot/skills/jetpack-compose/SKILL.md"
        }]))
    );
    let inventory = inventory(&core, &context(None));
    assert_eq!(code(item(&inventory, "jetpack-compose").state), "disabled");
}

/// `fs` whose application store trusts only the folders `uris` name.
fn trusting(fs: FakeFileSystem, uris: &[&str]) -> FakeFileSystem {
    let items: Vec<Value> = uris
        .iter()
        .map(|uri| json!({ "uri": uri, "trusted": true }))
        .collect();
    fs.with_sqlite_item(
        profile_store(),
        "content.trust.model.key",
        &json!({ "uriTrustInfo": items }).to_string(),
    )
}

#[test]
fn a_project_s_servers_are_blocked_until_its_folder_is_trusted() {
    let fs = trusting(with_project(vscode(), "app"), &["file:///e%3A/elsewhere"]);
    let inventory = inventory(&core(&fs), &context(Some(&project("app"))));

    let supabase = item(&inventory, "supabase");
    assert_eq!(code(supabase.state), "blocked");
    let reason = supabase.entries[0].reason.as_ref().unwrap();
    assert_eq!(code(reason.code), "untrusted_folder");
    assert!(
        reason.sentence.contains("once you trust"),
        "{}",
        reason.sentence
    );
    // An untrusted server still wins over the user's of the same name.
    assert_eq!(
        states(item(&inventory, "GitHub"))[..2],
        [
            state(
                "blocked",
                &format!(r"{PROJECTS}\app\.vscode\mcp.json#servers.GitHub")
            ),
            state("shadowed", &user_server("github")),
        ]
    );
    // The user's own servers need no trust.
    assert_eq!(code(item(&inventory, "fetch").state), "effective");
}

#[test]
fn trusting_a_parent_folder_trusts_the_project() {
    let fs = with_project(vscode(), "app");
    let inventory = inventory(&core(&fs), &context(Some(&project("app"))));

    assert_eq!(code(item(&inventory, "supabase").state), "effective");
}

#[test]
fn the_shared_store_s_trust_counts_too() {
    let fs = trusting(with_project(vscode(), "app"), &[]).with_sqlite_item(
        format!(r"{HOME}\.vscode-shared\sharedStorage\state.vscdb"),
        "content.trust.model.key",
        r#"{"uriTrustInfo":[{"uri":"file:///e%3A/projects/app","trusted":true}]}"#,
    );
    let inventory = inventory(&core(&fs), &context(Some(&project("app"))));

    assert_eq!(code(item(&inventory, "supabase").state), "effective");
}

#[test]
fn with_workspace_trust_turned_off_no_folder_needs_trust() {
    let fs = trusting(with_project(vscode(), "app"), &[]).with_file(
        format!(r"{USER}\settings.json"),
        r#"{ "security.workspace.trust.enabled": false }"#,
    );
    let inventory = inventory(&core(&fs), &context(Some(&project("app"))));

    assert_eq!(code(item(&inventory, "supabase").state), "effective");
}

/// `fs` with Claude Desktop's server `filesystem`, which VS Code discovers.
fn discovering_claude_desktop(fs: FakeFileSystem) -> FakeFileSystem {
    fs.with_file(
        format!(r"{APP_DATA}\Claude\claude_desktop_config.json"),
        r#"{ "mcpServers": { "filesystem": { "command": "npx" } } }"#,
    )
    .with_file(
        format!(r"{USER}\settings.json"),
        r#"{ "chat.mcp.discovery.enabled": { "claude-desktop": true } }"#,
    )
}

#[test]
fn a_discovered_server_is_blocked_until_the_user_trusts_it() {
    let fs = discovering_claude_desktop(vscode());
    let inventory = inventory(&core(&fs), &context(None));

    let filesystem = item(&inventory, "filesystem");
    assert_eq!(code(filesystem.state), "blocked");
    let reason = filesystem.entries[0].reason.as_ref().unwrap();
    assert_eq!(code(reason.code), "not_approved");
    assert!(reason.sentence.contains("trusted"), "{}", reason.sentence);
}

#[test]
fn a_discovered_server_trusted_at_the_nonce_vs_code_last_saw_is_effective() {
    let cache = |trusted_at: &str| {
        json!({ "serverTools": [["claude-desktop.null.filesystem", {
            "serverName": "filesystem", "nonce": "n2", "trustedAtNonce": trusted_at
        }]] })
        .to_string()
    };
    let trusted = discovering_claude_desktop(vscode()).with_sqlite_item(
        profile_store(),
        "mcpToolCache",
        &cache("n2"),
    );
    let changed = discovering_claude_desktop(vscode()).with_sqlite_item(
        profile_store(),
        "mcpToolCache",
        &cache("n1"),
    );

    let state_of = |fs: &FakeFileSystem| {
        let inventory = inventory(&core(fs), &context(None));
        code(item(&inventory, "filesystem").state)
    };
    assert_eq!(state_of(&trusted), "effective");
    assert_eq!(state_of(&changed), "blocked");
}

/// `fs` whose application store caches the account's policy as `policy`,
/// each part with the time of the request that brought it: `mcp` the
/// token's, fetched at 2026-09-21 14:13:20 UTC, and `mcpAccess` the MCP
/// registry's, at 2026-09-10 14:13:20 UTC, both older than the
/// entitlements VS Code fetched at its last start.
fn with_account_policy(fs: FakeFileSystem, policy: Value) -> FakeFileSystem {
    let cached = json!({ "accountPolicyData": {
        "accountId": "fixture",
        "policyData": policy,
        "entitlementsFetchedAt": 1_790_864_000_000_u64,
        "tokenEntitlementsFetchedAt": 1_790_000_000_000_u64,
        "mcpRegistryDataFetchedAt": 1_789_049_600_000_u64,
    } });
    fs.with_sqlite_item(
        profile_store(),
        "defaultAccount.cachedPolicyData",
        &cached.to_string(),
    )
}

#[test]
fn the_account_s_policy_without_mcp_blocks_every_server_as_of_the_token_s_fetch() {
    let fs = with_account_policy(with_project(vscode(), "app"), json!({ "mcp": false }));
    let inventory = inventory(&core(&fs), &context(Some(&project("app"))));

    for name in ["fetch", "supabase"] {
        let found = item(&inventory, name);
        assert_eq!(code(found.state), "blocked", "{name}");
        let reason = found.entries[0].reason.as_ref().unwrap();
        assert_eq!(code(reason.code), "policy");
        assert!(
            reason.sentence.contains("as of 21 Sep 2026"),
            "{}",
            reason.sentence
        );
    }
}

#[test]
fn a_registry_only_policy_blocks_all_but_the_installed_servers_as_of_the_registry_s_fetch() {
    let fs = with_account_policy(
        with_project(vscode(), "app"),
        json!({ "mcpAccess": "registry_only" }),
    );
    let inventory = inventory(&core(&fs), &context(Some(&project("app"))));

    let supabase = item(&inventory, "supabase");
    assert_eq!(code(supabase.state), "blocked");
    let reason = supabase.entries[0].reason.as_ref().unwrap();
    assert!(
        reason.sentence.contains("as of 10 Sep 2026"),
        "{}",
        reason.sentence
    );
    assert_eq!(code(item(&inventory, "fetch").state), "effective");
}

#[test]
fn mcp_access_none_in_the_user_settings_disables_every_server_with_no_enable() {
    let fs = vscode().with_file(
        format!(r"{USER}\settings.json"),
        r#"{ "chat.mcp.access": "none" }"#,
    );
    let core = core(&fs);
    let inventory = inventory(&core, &context(None));

    let fetch = item(&inventory, "fetch");
    assert_eq!(code(fetch.state), "disabled");
    let reason = fetch.entries[0].reason.as_ref().unwrap();
    assert_eq!(code(reason.code), "disabled_globally");
    let Err(refused) = plan(
        &core,
        &context(None),
        Action::Enable,
        &[&user_server("fetch")],
    ) else {
        panic!("Enable is offered");
    };
    assert_eq!(code(refused.code), "blocked");
}

#[test]
fn use_agent_skills_false_disables_every_skill() {
    let fs = with_home(vscode()).with_file(
        format!(r"{USER}\settings.json"),
        r#"{ "chat.useAgentSkills": false }"#,
    );
    let inventory = inventory(&core(&fs), &context(None));

    let skill = item(&inventory, "jetpack-compose");
    assert_eq!(code(skill.state), "disabled");
    let reason = skill.entries[0].reason.as_ref().unwrap();
    assert_eq!(code(reason.code), "disabled_globally");
}

/// `fs` whose application store caches the ExP assignment of `parameters`.
fn with_experiments(fs: FakeFileSystem, parameters: Value) -> FakeFileSystem {
    let memento = json!({ "VSCode.ABExp.FeatureData": {
        "features": [],
        "configs": [{ "Id": "vscode", "Parameters": parameters }],
    } });
    fs.with_sqlite_item(
        profile_store(),
        "memento/experiment.service.memento",
        &memento.to_string(),
    )
}

#[test]
fn an_exp_assignment_changes_none_of_the_settings_read_here() {
    let fs = with_experiments(
        with_project(vscode(), "app"),
        json!({
            "config.chat.mcp.access": "none",
            "config.chat.mcp.collisionBehavior": "suffix",
        }),
    );
    let inventory = inventory(&core(&fs), &context(Some(&project("app"))));

    // VS Code runs no experiment on these settings: every server may run,
    // and same-named servers still compete.
    assert_eq!(code(item(&inventory, "fetch").state), "effective");
    assert_eq!(
        code(item(&inventory, "GitHub").entries[1].state),
        "shadowed"
    );
}

#[test]
fn a_profile_store_that_cannot_be_read_disables_nothing_and_takes_no_disable() {
    let fs = vscode()
        .with_sqlite_item(
            profile_store(),
            "mcp.enablement",
            r#"[["mcp.config.usrlocal.fetch",false]]"#,
        )
        .denying_reads_of(profile_store());
    let core = core(&fs);
    let inventory = inventory(&core, &context(None));

    let fetch = item(&inventory, "fetch");
    assert_eq!(code(fetch.state), "effective");
    let disable = serde_json::to_value(&fetch.entries[0].disable).unwrap();
    assert_eq!(disable["code"], "unreadable_switch", "{disable}");
}

#[test]
fn the_account_s_policy_replaces_the_user_s_mcp_access() {
    let fs = with_account_policy(
        with_project(vscode(), "app"),
        json!({ "mcpAccess": "registry_only" }),
    )
    .with_file(
        format!(r"{USER}\settings.json"),
        r#"{ "chat.mcp.access": "none" }"#,
    );
    let inventory = inventory(&core(&fs), &context(Some(&project("app"))));

    // VS Code reads the policy's `registry`, not the user's `none`.
    assert_eq!(code(item(&inventory, "fetch").state), "effective");
    let supabase = item(&inventory, "supabase");
    assert_eq!(code(supabase.state), "blocked");
    assert_eq!(
        code(supabase.entries[0].reason.as_ref().unwrap().code),
        "policy"
    );
}

#[test]
fn enable_of_a_skill_takes_its_skill_file_out_of_the_disabled_ones() {
    let fs = with_home(vscode()).with_sqlite_item(
        profile_store(),
        "chat.disabledPromptFiles.skill",
        r#"[{"$mid":1,"path":"/c:/Users/fixture/.copilot/skills/jetpack-compose/SKILL.md","scheme":"file"},{"$mid":1,"path":"/c:/elsewhere/SKILL.md","scheme":"file"}]"#,
    );
    let core = core(&fs);

    let plan = plan(
        &core,
        &context(None),
        Action::Enable,
        &[&copilot_skill("jetpack-compose")],
    )
    .unwrap();
    core.execute(plan, &[WarningClass::WideReach]).unwrap();

    assert_eq!(
        item_value(&fs, &profile_store(), "chat.disabledPromptFiles.skill"),
        Some(json!([{ "$mid": 1, "path": "/c:/elsewhere/SKILL.md", "scheme": "file" }]))
    );
    let inventory = inventory(&core, &context(None));
    assert_eq!(code(item(&inventory, "jetpack-compose").state), "effective");
}

#[test]
fn a_profile_s_discovered_server_is_trusted_only_in_the_profile_s_store() {
    let cache = json!({ "serverTools": [["claude-desktop.null.filesystem", {
        "nonce": "n2", "trustedAtNonce": "n2"
    }]] })
    .to_string();
    let fs = with_project(discovering_claude_desktop(vscode()), "app").with_sqlite_item(
        app_store(),
        "mcpToolCache",
        &cache,
    );
    let inventory = inventory(&core(&fs), &context(Some(&project("app"))));

    assert_eq!(code(item(&inventory, "filesystem").state), "blocked");
}
