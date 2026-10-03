//! Disable and Enable of Claude Code MCP servers through the core `api`,
//! with fake ports over snapshot fixtures: the switches `/mcp` writes, the
//! Disabled State they give, its what-if and secondary reasons, and the
//! plans that write and lift them.

use config_digger_core::api::{
    Action, Context, Core, DisableOption, Entry, Error, Inventory, Plan, ScanOptions, Targets,
};
use config_digger_core::ports::RunningProcess;
use config_digger_core::ports::fake::{
    self, FakeEnvironment, FakeFileSystem, FakeGitIndex, FakeProcesses,
};
use serde_json::Value;
use std::path::PathBuf;

const HOME: &str = r"C:\Users\fixture";
const LOCAL_APP_DATA: &str = r"C:\Users\fixture\AppData\Local";
const PETME: &str = r"E:\projects\petme-app";
const CLAUDE_JSON: &[u8] = include_bytes!("fixtures/claude-code-disabled/home/.claude.json");
const MCP_JSON: &[u8] = include_bytes!("fixtures/claude-code-disabled/petme-app/.mcp.json");
const BUILT_IN_CLAUDE_CODE: &[u8] = include_bytes!("../definitions/claude-code.toml");
const SETTINGS_LOCAL: &[u8] =
    include_bytes!("fixtures/claude-code-disabled/petme-app/.claude/settings.local.json");
const SETTINGS: &[u8] =
    include_bytes!("fixtures/claude-code-disabled/petme-app/.claude/settings.json");

fn claude_json() -> PathBuf {
    PathBuf::from(HOME).join(".claude.json")
}

fn mcp_json() -> PathBuf {
    PathBuf::from(PETME).join(".mcp.json")
}

fn settings_local() -> PathBuf {
    PathBuf::from(PETME).join(r".claude\settings.local.json")
}

/// `~/.claude.json` Disables `linear` by name in petme-app, and its
/// `settings.local.json` Disables the `.mcp.json` server `github`. Its
/// `settings.json` approves every `.mcp.json` server.
fn fixture() -> FakeFileSystem {
    FakeFileSystem::new()
        .with_folder(PETME)
        .with_file(claude_json(), CLAUDE_JSON)
        .with_file(mcp_json(), MCP_JSON)
        .with_file(settings_local(), SETTINGS_LOCAL)
        .with_file(
            PathBuf::from(PETME).join(r".claude\settings.json"),
            SETTINGS,
        )
}

fn core(fs: &FakeFileSystem) -> Core {
    let env = FakeEnvironment::with_home(HOME).with_var("LOCALAPPDATA", LOCAL_APP_DATA);
    Core::new(fake::ports(fs.clone(), env)).unwrap()
}

fn in_petme() -> Context {
    Context {
        project: Some(PathBuf::from(PETME)),
    }
}

fn inventory(core: &Core, context: &Context) -> Inventory {
    let snapshot = core
        .scan(&ScanOptions {
            projects: context.project.iter().cloned().collect(),
        })
        .unwrap();
    core.resolve(&snapshot, context)
}

/// The Entries of the Item `name`, in precedence order.
fn entries(inventory: &Inventory, name: &str) -> Vec<Entry> {
    inventory
        .items
        .iter()
        .find(|item| item.name == name)
        .unwrap_or_else(|| panic!("no Item `{name}`"))
        .entries
        .clone()
}

/// A serialised value as its JSON string, such as a State's `"disabled"`.
fn code(value: impl serde::Serialize) -> String {
    match serde_json::to_value(value).unwrap() {
        Value::String(code) => code,
        other => panic!("not a code: {other}"),
    }
}

/// `(scope, state)` per Entry of the Item `name`.
fn states(inventory: &Inventory, name: &str) -> Vec<(String, String)> {
    entries(inventory, name)
        .iter()
        .map(|entry| (entry.scope.clone(), code(entry.state)))
        .collect()
}

fn pairs(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
        .collect()
}

#[test]
fn a_name_in_disabled_mcp_servers_disables_every_entry_of_the_item_in_that_project() {
    let fs = fixture();
    let inventory = inventory(&core(&fs), &in_petme());

    assert_eq!(
        states(&inventory, "linear"),
        pairs(&[("project", "disabled"), ("user", "disabled")])
    );
    for entry in entries(&inventory, "linear") {
        let reason = entry.reason.expect("a Disabled Entry has a reason");
        assert_eq!(code(reason.code), "disabled_by_name");
        assert_eq!(
            reason.switch.as_deref(),
            Some(
                r"C:\Users\fixture\.claude.json#projects.E:/projects/petme-app.disabledMcpServers"
            )
        );
        assert!(
            reason.sentence.contains("disabledMcpServers"),
            "{}",
            reason.sentence
        );
    }
    let item = inventory.items.iter().find(|i| i.name == "linear").unwrap();
    assert_eq!(code(item.state), "disabled");
}

#[test]
fn a_disabled_entry_says_what_it_would_be_once_enabled() {
    let fs = fixture();
    let inventory = inventory(&core(&fs), &in_petme());

    let linear = entries(&inventory, "linear");
    let project = linear[0].what_if.as_ref().unwrap();
    assert_eq!(code(project.state), "effective");
    assert_eq!(
        project.sentence,
        "Disabled: would be Effective once Enabled."
    );
    // Enable lifts the name for both, and the project Entry ranks higher.
    let user = linear[1].what_if.as_ref().unwrap();
    assert_eq!(code(user.state), "shadowed");
    let project_locator = format!(r"{}#mcpServers.linear", mcp_json().display());
    assert_eq!(user.effective.as_deref(), Some(project_locator.as_str()));
    assert_eq!(
        user.sentence,
        format!("Disabled: would be Shadowed by the project Entry {project_locator} once Enabled.")
    );
    assert!(entries(&inventory, "notes")[0].what_if.is_none());
}

#[test]
fn disabled_mcpjson_servers_disables_only_the_mcp_json_entry() {
    let fs = fixture();
    let inventory = inventory(&core(&fs), &in_petme());

    assert_eq!(
        states(&inventory, "github"),
        pairs(&[("local", "effective"), ("project", "disabled")])
    );
    let project = &entries(&inventory, "github")[1];
    let reason = project.reason.as_ref().unwrap();
    assert_eq!(code(reason.code), "disabled_by_name");
    let switch = format!("{}#disabledMcpjsonServers", settings_local().display());
    assert_eq!(reason.switch.as_deref(), Some(switch.as_str()));
}

#[test]
fn an_entry_failing_several_checks_lists_the_others_as_secondary_reasons() {
    // The `.mcp.json` `github` is Disabled, and a local `github` beats it.
    let fs = fixture();
    let inventory = inventory(&core(&fs), &in_petme());

    let project = &entries(&inventory, "github")[1];
    assert_eq!(code(project.state), "disabled");
    let codes: Vec<String> = project.secondary.iter().map(|r| code(r.code)).collect();
    assert_eq!(codes, ["shadowed_by_precedence"]);
    let local = format!(
        r"{}#projects.E:/projects/petme-app.mcpServers.github",
        claude_json().display()
    );
    assert_eq!(
        project.secondary[0].effective.as_deref(),
        Some(local.as_str())
    );
    assert_eq!(code(project.what_if.as_ref().unwrap().state), "shadowed");
}

#[test]
fn the_state_comes_from_the_first_failed_check_in_the_definitions_order() {
    // The same Agent with precedence checked before Disable.
    let fs = fixture();
    let definition = String::from_utf8(BUILT_IN_CLAUDE_CODE.to_vec())
        .unwrap()
        .replace(
            r#"checks = ["policy", "shape", "rules", "disable", "approval", "precedence"]"#,
            r#"checks = ["policy", "shape", "rules", "approval", "precedence", "disable"]"#,
        )
        .replace(r#"id = "claude-code""#, r#"id = "claude-code-reordered""#);
    let env = FakeEnvironment::with_home(HOME).with_var("LOCALAPPDATA", LOCAL_APP_DATA);
    let core = Core::with_extra_definitions(
        fake::ports(fs.clone(), env),
        &[("reordered.toml", &definition)],
    )
    .unwrap();
    let inventory = inventory(&core, &in_petme());

    let github = inventory
        .items
        .iter()
        .find(|item| item.agent == "claude-code-reordered" && item.name == "github")
        .unwrap();
    let project = &github.entries[1];
    assert_eq!(code(project.state), "shadowed");
    let codes: Vec<String> = project.secondary.iter().map(|r| code(r.code)).collect();
    assert_eq!(codes, ["disabled_by_name"]);
    assert!(project.what_if.is_none());
}

#[test]
fn without_a_project_per_project_switches_do_not_apply() {
    let fs = fixture();
    let inventory = inventory(&core(&fs), &Context::default());

    assert_eq!(
        states(&inventory, "linear"),
        pairs(&[("user", "effective")])
    );
}

#[test]
fn a_switch_under_another_spelling_of_the_project_key_applies() {
    let fs = fixture();
    let context = Context {
        project: Some(PathBuf::from(r"e:\PROJECTS\petme-app")),
    };
    let inventory = inventory(&core(&fs), &context);

    assert_eq!(
        states(&inventory, "linear"),
        pairs(&[("project", "disabled"), ("user", "disabled")])
    );
}

fn targets(selectors: &[&str], all: bool) -> Targets {
    Targets {
        selectors: selectors.iter().map(|s| (*s).to_owned()).collect(),
        all,
    }
}

fn plan(core: &Core, context: &Context, action: Action, targets: &Targets) -> Result<Plan, Error> {
    let snapshot = core
        .scan(&ScanOptions {
            projects: context.project.iter().cloned().collect(),
        })
        .unwrap();
    core.plan(&snapshot, context, action, targets)
}

fn json(fs: &FakeFileSystem, path: PathBuf) -> Value {
    serde_json::from_slice(&fs.contents(path).unwrap()).unwrap()
}

/// Rows of a preview's changes: `(locator, before, after)`.
fn changes(plan: &Plan) -> Vec<(String, String, String)> {
    plan.preview()
        .changes
        .iter()
        .map(|change| {
            (
                change.locator.clone(),
                code(change.before),
                code(change.after),
            )
        })
        .collect()
}

fn row(locator: &str, before: &str, after: &str) -> (String, String, String) {
    (locator.to_owned(), before.to_owned(), after.to_owned())
}

fn notes() -> String {
    format!("{}#mcpServers.notes", claude_json().display())
}

fn project_supabase() -> String {
    format!("{}#mcpServers.supabase", mcp_json().display())
}

fn user_supabase() -> String {
    format!("{}#mcpServers.supabase", claude_json().display())
}

/// [`fixture`] with a user `supabase` too, which the project one shadows.
fn with_user_supabase() -> FakeFileSystem {
    let claude_json_text = String::from_utf8(CLAUDE_JSON.to_vec()).unwrap().replace(
        r#""notes": {"#,
        r#""supabase": { "url": "https://user" },
    "notes": {"#,
    );
    fixture().with_file(claude_json(), claude_json_text)
}

/// The native Claude Code CLI, as it runs from `~/.local/bin`.
fn claude_code_cli() -> RunningProcess {
    RunningProcess {
        pid: 4242,
        image: "claude.exe".to_owned(),
        path: Some(PathBuf::from(r"C:\Users\fixture\.local\bin\claude.exe")),
        cmdline: r"C:\Users\fixture\.local\bin\claude.exe".to_owned(),
    }
}

/// A core over `fs` with `versioned` in the git index and `running` running,
/// from the native Installation of Claude Code that `fs` then holds.
fn core_with(fs: &FakeFileSystem, versioned: &[PathBuf], running: &[RunningProcess]) -> Core {
    if !running.is_empty() {
        fs.put_file(
            r"C:\Users\fixture\.local\bin\claude.exe",
            fake::executable(Some([2, 1, 286, 0])),
        );
    }
    let env = FakeEnvironment::with_home(HOME).with_var("LOCALAPPDATA", LOCAL_APP_DATA);
    let mut ports = fake::ports(fs.clone(), env);
    ports.git = Box::new(versioned.iter().fold(FakeGitIndex::default(), |git, path| {
        git.with_versioned(path)
    }));
    ports.processes = Box::new(
        running
            .iter()
            .fold(FakeProcesses::default(), |processes, process| {
                processes.with_process(process.clone())
            }),
    );
    Core::new(ports).unwrap()
}

#[test]
fn disable_adds_the_name_to_disabled_mcp_servers_under_the_projects_own_spelling() {
    // The context names petme-app in another spelling than Claude Code's.
    let fs = fixture();
    let core = core(&fs);
    let context = Context {
        project: Some(PathBuf::from(r"e:\projects\PETME-APP")),
    };

    let plan = plan(
        &core,
        &context,
        Action::Disable,
        &targets(&["notes"], false),
    )
    .unwrap();
    assert_eq!(changes(&plan), [row(&notes(), "effective", "disabled")]);
    assert_eq!(plan.preview().files[0].path, claude_json());
    let outcome = core.execute(plan, &[]).unwrap();

    assert_eq!(code(outcome.verify), "verified");
    let after = json(&fs, claude_json());
    assert_eq!(
        after["projects"]["E:/projects/petme-app"]["disabledMcpServers"],
        serde_json::json!(["linear", "notes"])
    );
    assert_eq!(after["projects"].as_object().unwrap().len(), 2);
    let inventory = inventory(&core, &in_petme());
    assert_eq!(states(&inventory, "notes"), pairs(&[("user", "disabled")]));
}

#[test]
fn disable_of_one_entry_by_name_lists_the_others_it_reaches_and_warns_of_wide_reach() {
    let fs = with_user_supabase();
    let core = core(&fs);

    let plan = plan(
        &core,
        &in_petme(),
        Action::Disable,
        &targets(&[&project_supabase()], false),
    )
    .unwrap();

    assert_eq!(
        changes(&plan),
        [
            row(&project_supabase(), "effective", "disabled"),
            row(&user_supabase(), "shadowed", "disabled"),
        ]
    );
    let warning = plan
        .warnings()
        .iter()
        .find(|warning| code(warning.class) == "wide-reach")
        .expect("a wide-reach warning");
    assert!(
        warning.sentence.contains(&user_supabase()),
        "{}",
        warning.sentence
    );
    let refused = core.execute(plan, &[]).unwrap_err();
    assert_eq!(code(refused.code), "ack_required");
    assert!(
        refused.message.contains("wide-reach"),
        "{}",
        refused.message
    );
}

#[test]
fn disable_of_every_entry_of_the_item_reaches_nothing_else() {
    let fs = with_user_supabase();
    let core = core(&fs);

    let plan = plan(
        &core,
        &in_petme(),
        Action::Disable,
        &targets(&["supabase"], true),
    )
    .unwrap();

    assert_eq!(changes(&plan).len(), 2);
    assert!(plan.warnings().is_empty(), "{:?}", plan.warnings());
    // Both Entries share one switch, written once.
    assert_eq!(plan.preview().files[0].edits.len(), 1);
    let effective = &plan.preview().effective[0];
    assert_eq!(effective.after, None);
    assert!(
        effective.sentence.contains("No Entry is Effective"),
        "{}",
        effective.sentence
    );
}

#[test]
fn disable_of_a_disabled_entry_is_blocked() {
    let fs = fixture();
    let core = core(&fs);
    let user = format!("{}#mcpServers.linear", claude_json().display());

    let error = plan(
        &core,
        &in_petme(),
        Action::Disable,
        &targets(&[&user], false),
    )
    .err()
    .unwrap();

    assert_eq!(code(error.code), "blocked");
    assert!(
        error.message.contains("Disabled already"),
        "{}",
        error.message
    );
}

#[test]
fn the_switch_store_is_rewritten_while_running_so_a_running_claude_code_warns() {
    let fs = fixture();
    let core = core_with(&fs, &[], &[claude_code_cli()]);

    let plan = plan(
        &core,
        &in_petme(),
        Action::Disable,
        &targets(&["notes"], false),
    )
    .unwrap();

    let warning = &plan.warnings()[0];
    assert_eq!(code(warning.class), "running-agent");
    assert!(
        warning.sentence.contains("rewrites"),
        "{}",
        warning.sentence
    );
}

#[test]
fn a_switch_file_no_location_reads_still_warns_while_its_agent_runs() {
    // `~/.claude.json` is versioned, so Disable writes `settings.local.json`.
    let fs = fixture();
    let core = core_with(&fs, &[claude_json()], &[claude_code_cli()]);

    let plan = plan(
        &core,
        &in_petme(),
        Action::Disable,
        &targets(&[&project_supabase()], false),
    )
    .unwrap();

    assert_eq!(plan.preview().files[0].path, settings_local());
    let warning = &plan.warnings()[0];
    assert_eq!(code(warning.class), "running-agent");
    assert!(
        warning
            .sentence
            .contains(&settings_local().display().to_string()),
        "{}",
        warning.sentence
    );
}

#[test]
fn disable_creates_the_switch_file_and_its_folder_when_missing() {
    let fs = FakeFileSystem::new()
        .with_folder(PETME)
        .with_file(claude_json(), CLAUDE_JSON)
        .with_file(mcp_json(), MCP_JSON);
    let core = core_with(&fs, &[claude_json()], &[]);

    let plan = plan(
        &core,
        &in_petme(),
        Action::Disable,
        &targets(&[&project_supabase()], false),
    )
    .unwrap();
    core.execute(plan, &[]).unwrap();

    assert_eq!(
        json(&fs, settings_local()),
        serde_json::json!({ "disabledMcpjsonServers": ["supabase"] })
    );
}

#[test]
fn enable_removes_the_name_the_agent_itself_listed() {
    // `linear` was Disabled through `/mcp`, not through config-digger.
    let fs = fixture();
    let core = core(&fs);

    let plan = plan(
        &core,
        &in_petme(),
        Action::Enable,
        &targets(&["linear"], true),
    )
    .unwrap();
    assert_eq!(plan.preview().title, "Enable 2 Entries");
    let outcome = core.execute(plan, &[]).unwrap();

    assert_eq!(code(outcome.verify), "verified");
    assert_eq!(
        json(&fs, claude_json())["projects"]["E:/projects/petme-app"]["disabledMcpServers"],
        serde_json::json!([])
    );
    assert_eq!(
        states(&inventory(&core, &in_petme()), "linear"),
        pairs(&[("project", "effective"), ("user", "shadowed")])
    );
}

#[test]
fn enable_of_a_mcp_json_server_lifts_disabled_mcpjson_servers() {
    let fs = fixture();
    let core = core(&fs);
    let github = format!("{}#mcpServers.github", mcp_json().display());

    let plan = plan(
        &core,
        &in_petme(),
        Action::Enable,
        &targets(&[&github], false),
    )
    .unwrap();

    // A local `github` still beats it once Enabled.
    assert_eq!(changes(&plan), [row(&github, "disabled", "shadowed")]);
    assert_eq!(plan.preview().files[0].path, settings_local());
    core.execute(plan, &[]).unwrap();
    assert_eq!(
        json(&fs, settings_local())["disabledMcpjsonServers"],
        serde_json::json!([])
    );
}

#[test]
fn enable_of_one_entry_by_name_warns_of_wide_reach() {
    let fs = fixture();
    let core = core(&fs);
    let project = format!("{}#mcpServers.linear", mcp_json().display());
    let user = format!("{}#mcpServers.linear", claude_json().display());

    let plan = plan(
        &core,
        &in_petme(),
        Action::Enable,
        &targets(&[&project], false),
    )
    .unwrap();

    assert_eq!(
        changes(&plan),
        [
            row(&project, "disabled", "effective"),
            row(&user, "disabled", "shadowed"),
        ]
    );
    let classes: Vec<String> = plan.warnings().iter().map(|w| code(w.class)).collect();
    assert_eq!(classes, ["wide-reach"]);
}

#[test]
fn enable_of_an_entry_that_is_not_disabled_is_blocked() {
    let fs = fixture();
    let core = core(&fs);

    let error = plan(
        &core,
        &in_petme(),
        Action::Enable,
        &targets(&["notes"], false),
    )
    .err()
    .unwrap();

    assert_eq!(code(error.code), "blocked");
    assert!(error.message.contains("not Disabled"), "{}", error.message);
}

#[test]
fn enable_lifts_the_name_under_every_spelling_of_the_project_key() {
    let claude_json_text = String::from_utf8(CLAUDE_JSON.to_vec()).unwrap().replace(
        r#""E:/projects/other": {"#,
        r#""e:\\projects\\petme-app": { "disabledMcpServers": ["notes"] },
    "E:/projects/other": {"#,
    );
    let fs = fixture().with_file(claude_json(), claude_json_text);
    let core = core(&fs);
    let inventory = inventory(&core, &in_petme());
    assert_eq!(states(&inventory, "notes"), pairs(&[("user", "disabled")]));
    let notes_entry = &entries(&inventory, "notes")[0];
    assert!(
        notes_entry
            .enable
            .as_deref()
            .unwrap()
            .contains("disabledMcpServers"),
        "{:?}",
        notes_entry.enable
    );

    let plan = plan(
        &core,
        &in_petme(),
        Action::Enable,
        &targets(&["notes"], false),
    )
    .unwrap();
    core.execute(plan, &[]).unwrap();

    let after = json(&fs, claude_json());
    assert_eq!(
        after["projects"][r"e:\projects\petme-app"]["disabledMcpServers"],
        serde_json::json!([])
    );
    assert_eq!(
        states(&inventory_of(&core), "notes"),
        pairs(&[("user", "effective")])
    );
}

fn inventory_of(core: &Core) -> Inventory {
    inventory(core, &in_petme())
}

#[test]
fn a_disable_then_its_enable_round_trips_and_each_is_its_own_operation() {
    let fs = fixture();
    let core = core(&fs);
    let before = fs.contents(claude_json()).unwrap();

    let disable = plan(
        &core,
        &in_petme(),
        Action::Disable,
        &targets(&["notes"], false),
    )
    .unwrap();
    core.execute(disable, &[]).unwrap();
    let enable = plan(
        &core,
        &in_petme(),
        Action::Enable,
        &targets(&["notes"], false),
    )
    .unwrap();
    core.execute(enable, &[]).unwrap();

    assert_eq!(
        json(&fs, claude_json()),
        serde_json::from_slice::<Value>(&before).unwrap()
    );
    let history = core.history().unwrap();
    let actions: Vec<String> = history.iter().map(|op| code(op.action)).collect();
    assert_eq!(actions, ["enable", "disable"]);
    // The Enable is undone on its own; the Disable it lifted no longer applies.
    assert!(history[0].undoable);
    assert!(!history[1].undoable);
}

#[test]
fn the_undo_of_a_disable_enables_the_entry_again() {
    let fs = fixture();
    let core = core(&fs);
    let disable = plan(
        &core,
        &in_petme(),
        Action::Disable,
        &targets(&["notes"], false),
    )
    .unwrap();
    let outcome = core.execute(disable, &[]).unwrap();

    let undo = core.plan_undo(&outcome.operation).unwrap();
    assert_eq!(changes(&undo), [row(&notes(), "disabled", "effective")]);
    core.execute(undo, &[]).unwrap();

    assert_eq!(
        states(&inventory_of(&core), "notes"),
        pairs(&[("user", "effective")])
    );
}

#[test]
fn a_switch_file_that_does_not_parse_disables_nothing_and_is_never_written() {
    // `~/.claude.json` is versioned, so the local settings are the only switch.
    let fs = fixture().with_file(
        settings_local(),
        "{ \"disabledMcpjsonServers\": [\"github\"], ",
    );
    let core = core_with(&fs, &[claude_json()], &[]);

    let inventory = inventory(&core, &in_petme());

    let github = &entries(&inventory, "github")[1];
    assert_eq!(code(github.state), "shadowed");
    let supabase = &entries(&inventory, "supabase")[0];
    match &supabase.disable {
        DisableOption::NotOffered {
            code: reason,
            sentence,
        } => {
            assert_eq!(code(reason), "unreadable_switch");
            assert!(
                sentence.contains(&settings_local().display().to_string()),
                "{sentence}"
            );
        }
        offered => panic!("Disable offered: {offered:?}"),
    }
}

#[test]
fn disable_of_two_names_into_a_missing_list_keeps_both() {
    let fs = FakeFileSystem::new()
        .with_folder(PETME)
        .with_file(claude_json(), CLAUDE_JSON)
        .with_file(mcp_json(), MCP_JSON);
    let core = core_with(&fs, &[claude_json()], &[]);
    let github = format!("{}#mcpServers.github", mcp_json().display());

    let plan = plan(
        &core,
        &in_petme(),
        Action::Disable,
        &targets(&[&project_supabase(), &github], false),
    )
    .unwrap();
    core.execute(plan, &[]).unwrap();

    assert_eq!(
        json(&fs, settings_local()),
        serde_json::json!({ "disabledMcpjsonServers": ["supabase", "github"] })
    );
}

#[test]
fn enable_removes_every_occurrence_of_a_name_listed_twice() {
    let claude_json_text = String::from_utf8(CLAUDE_JSON.to_vec()).unwrap().replace(
        r#""disabledMcpServers": ["linear"]"#,
        r#""disabledMcpServers": ["notes", "linear", "notes"]"#,
    );
    let fs = fixture().with_file(claude_json(), claude_json_text);
    let core = core(&fs);

    let plan = plan(
        &core,
        &in_petme(),
        Action::Enable,
        &targets(&["notes"], false),
    )
    .unwrap();
    core.execute(plan, &[]).unwrap();

    assert_eq!(
        json(&fs, claude_json())["projects"]["E:/projects/petme-app"]["disabledMcpServers"],
        serde_json::json!(["linear"])
    );
    assert_eq!(
        states(&inventory_of(&core), "notes"),
        pairs(&[("user", "effective")])
    );
}

#[test]
fn an_entry_that_wins_once_the_target_is_disabled_is_not_wide_reach() {
    // `~/.claude.json` is versioned, so the switch covers `.mcp.json` only.
    let fs = with_user_supabase();
    let core = core_with(&fs, &[claude_json()], &[]);

    let plan = plan(
        &core,
        &in_petme(),
        Action::Disable,
        &targets(&[&project_supabase()], false),
    )
    .unwrap();

    assert_eq!(
        changes(&plan),
        [
            row(&project_supabase(), "effective", "disabled"),
            row(&user_supabase(), "shadowed", "effective"),
        ]
    );
    assert!(plan.warnings().is_empty(), "{:?}", plan.warnings());
}

fn shared_settings() -> PathBuf {
    PathBuf::from(PETME).join(r".claude\settings.json")
}

fn user_settings() -> PathBuf {
    PathBuf::from(HOME).join(r".claude\settings.json")
}

#[test]
fn disabled_mcpjson_servers_is_read_from_every_file_claude_code_reads_it_from() {
    // The approval dialog keeps it in `~/.claude.json`, and any settings file can hold it.
    let claude_json_text = String::from_utf8(CLAUDE_JSON.to_vec()).unwrap().replace(
        r#""disabledMcpServers": ["linear"]"#,
        r#""disabledMcpServers": ["linear"],
      "disabledMcpjsonServers": ["supabase"]"#,
    );
    let fs = fixture()
        .with_file(claude_json(), claude_json_text)
        .with_file(settings_local(), "{}")
        .with_file(
            shared_settings(),
            r#"{ "disabledMcpjsonServers": ["github"], "enableAllProjectMcpServers": true }"#,
        )
        .with_file(
            user_settings(),
            r#"{ "disabledMcpjsonServers": ["linear"] }"#,
        );
    let inventory = inventory(&core(&fs), &in_petme());

    let switch = |name: &str| {
        entries(&inventory, name)
            .iter()
            .find(|entry| entry.scope == "project")
            .unwrap()
            .reason
            .clone()
            .unwrap()
            .switch
            .unwrap()
    };
    assert_eq!(
        switch("supabase"),
        format!(
            "{}#projects.E:/projects/petme-app.disabledMcpjsonServers",
            claude_json().display()
        )
    );
    assert_eq!(
        switch("github"),
        format!("{}#disabledMcpjsonServers", shared_settings().display())
    );
    let linear = &entries(&inventory, "linear")[0];
    let codes: Vec<String> = linear.secondary.iter().map(|r| code(r.code)).collect();
    assert_eq!(codes, ["disabled_by_name"]);
    assert_eq!(
        linear.secondary[0].switch.as_deref(),
        Some(format!("{}#disabledMcpjsonServers", user_settings().display()).as_str())
    );
}

#[test]
fn enable_lifts_the_switch_the_approval_dialog_keeps_in_claude_json() {
    let claude_json_text = String::from_utf8(CLAUDE_JSON.to_vec()).unwrap().replace(
        r#""disabledMcpServers": ["linear"]"#,
        r#""disabledMcpServers": ["linear"],
      "disabledMcpjsonServers": ["supabase"]"#,
    );
    let fs = fixture().with_file(claude_json(), claude_json_text);
    let core = core(&fs);

    let plan = plan(
        &core,
        &in_petme(),
        Action::Enable,
        &targets(&["supabase"], false),
    )
    .unwrap();
    core.execute(plan, &[]).unwrap();

    assert_eq!(
        json(&fs, claude_json())["projects"]["E:/projects/petme-app"]["disabledMcpjsonServers"],
        serde_json::json!([])
    );
}

#[test]
fn enable_is_not_offered_through_a_switch_config_digger_only_reads() {
    // The shared settings file belongs to the team.
    let fs = fixture().with_file(
        shared_settings(),
        r#"{ "disabledMcpjsonServers": ["supabase"] }"#,
    );
    let core = core(&fs);
    let supabase = &entries(&inventory(&core, &in_petme()), "supabase")[0];
    assert_eq!(code(supabase.state), "disabled");
    let enable = supabase.enable.as_deref().unwrap();
    assert!(enable.starts_with("Enable is not offered"), "{enable}");
    assert!(
        enable.contains(&shared_settings().display().to_string()),
        "{enable}"
    );

    let error = plan(
        &core,
        &in_petme(),
        Action::Enable,
        &targets(&["supabase"], false),
    )
    .err()
    .unwrap();

    assert_eq!(code(error.code), "blocked");
    assert!(error.message.contains(enable), "{}", error.message);
}
