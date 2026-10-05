//! Codex's Disable through the core `api`, with fake ports over fixture
//! Codex homes and layers: `enabled = false` in a server's own layer, or in
//! the user's `config.toml` for a server of a git-versioned layer, as long
//! as no higher layer sets `enabled`; `[[skills.config]]` path rules in the
//! user's `config.toml`; the `enabled` of a plugin and of a plugin's
//! server under `[plugins."<id>"]`; the server Codex builds before and
//! after a Remove; and actions on an Entry of Orca's Mirrored home, which go
//! to the Entry it was copied from.

use config_digger_core::api::{
    Action, Context, Core, Error, ErrorCode, Inventory, Item, Plan, ScanOptions, Targets,
    WarningClass,
};
use config_digger_core::ports::fake::{
    self, FakeEnvironment, FakeFileSystem, FakeGitIndex, FakeProcesses,
};
use config_digger_core::ports::{FileSystem, RunningProcess};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const HOME: &str = r"C:\Users\fixture";
const APP_DATA: &str = r"C:\Users\fixture\AppData\Roaming";
const PROGRAM_DATA: &str = r"C:\ProgramData";

/// The default Codex home, `~\.codex`.
const CODEX: &str = r"C:\Users\fixture\.codex";

/// Where the fixture Projects go.
const PROJECTS: &str = r"E:\projects";

fn user_config() -> PathBuf {
    Path::new(CODEX).join("config.toml")
}

fn project(name: &str) -> PathBuf {
    Path::new(PROJECTS).join(name)
}

/// The `.codex\config.toml` layer of the Project `name`.
fn layer(name: &str) -> PathBuf {
    project(name).join(r".codex\config.toml")
}

fn env() -> FakeEnvironment {
    FakeEnvironment::with_home(HOME)
        .with_var("APPDATA", APP_DATA)
        .with_var("USERPROFILE", HOME)
        .with_var("ProgramData", PROGRAM_DATA)
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

fn context(project: Option<&Path>) -> Context {
    Context {
        project: project.map(Path::to_path_buf),
    }
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

/// The Codex Item named `name` in the default home.
fn item<'i>(inventory: &'i Inventory, name: &str) -> &'i Item {
    item_in(inventory, "default", name)
}

/// The Codex Item named `name` in the home `home`.
fn item_in<'i>(inventory: &'i Inventory, home: &str, name: &str) -> &'i Item {
    inventory
        .items
        .iter()
        .find(|item| item.agent == "codex" && item.home == home && item.name == name)
        .unwrap_or_else(|| {
            let names: Vec<(&str, &str)> = inventory
                .items
                .iter()
                .filter(|item| item.agent == "codex")
                .map(|item| (item.home.as_str(), item.name.as_str()))
                .collect();
            panic!("no Codex Item `{name}` in home `{home}` among {names:?}")
        })
}

fn json_of(value: impl serde::Serialize) -> Value {
    serde_json::to_value(value).unwrap()
}

fn code(value: impl serde::Serialize) -> String {
    json_of(value).as_str().unwrap().to_owned()
}

fn plan(core: &Core, context: &Context, action: Action, selectors: &[&str]) -> Result<Plan, Error> {
    let snapshot = core.scan(&scan_options(context)).unwrap();
    let targets = Targets {
        selectors: selectors.iter().map(|s| (*s).to_owned()).collect(),
        all: false,
    };
    core.plan(&snapshot, context, action, &targets)
}

/// The locator of the server `name` in the file at `path`.
fn server(path: &Path, name: &str) -> String {
    format!("{}#mcp_servers.{name}", path.display())
}

fn has_warning(plan: &Plan, class: WarningClass) -> bool {
    plan.warnings().iter().any(|warning| warning.class == class)
}

fn text(fs: &FakeFileSystem, path: &Path) -> String {
    String::from_utf8(fs.read(path).unwrap()).unwrap()
}

#[test]
fn disable_of_a_server_writes_enabled_false_in_its_own_table() {
    let fs = FakeFileSystem::new().with_file(
        user_config(),
        "# My servers.\n[mcp_servers.fetch]\ncommand = \"uvx\"\n\n[mcp_servers.docs]\ncommand = \"docs-mcp\"\n",
    );
    let core = core(&fs);

    let before = inventory(&core, &context(None));
    let entry = json_of(&item(&before, "fetch").entries[0]);
    assert_eq!(
        entry["disable"]["status"], "offered",
        "{}",
        entry["disable"]
    );

    let plan = plan(
        &core,
        &context(None),
        Action::Disable,
        &[&server(&user_config(), "fetch")],
    )
    .unwrap();
    assert_eq!(plan.preview().files[0].path, user_config());
    core.execute(plan, &[]).unwrap();

    assert_eq!(
        text(&fs, &user_config()),
        "# My servers.\n[mcp_servers.fetch]\ncommand = \"uvx\"\nenabled = false\n\n[mcp_servers.docs]\ncommand = \"docs-mcp\"\n"
    );
    let after = inventory(&core, &context(None));
    assert_eq!(code(item(&after, "fetch").state), "disabled");
    assert_eq!(code(item(&after, "docs").state), "effective");
}

/// The Project app, a git repository the user trusts, whose
/// `.codex\config.toml` holds `layer`, under the user's `config.toml`
/// holding `user`.
fn app(user: &str, layer_text: &str) -> FakeFileSystem {
    FakeFileSystem::new()
        .with_folder(project(r"app\.git"))
        .with_file(
            user_config(),
            format!("{user}\n[projects.'e:\\projects\\app']\ntrust_level = \"trusted\"\n"),
        )
        .with_file(layer("app"), layer_text)
}

fn in_app() -> Context {
    context(Some(&project("app")))
}

#[test]
fn disable_writes_enabled_false_where_a_higher_layer_sets_enabled() {
    let fs = app(
        "[mcp_servers.docs]\ncommand = \"docs-mcp\"\n",
        "[mcp_servers.docs]\nenabled = true\n",
    );
    let core = core(&fs);

    let plan = plan(
        &core,
        &in_app(),
        Action::Disable,
        &[&server(&user_config(), "docs")],
    )
    .unwrap();
    assert_eq!(plan.preview().files[0].path, layer("app"));
    // It turns off the server built from every layer.
    assert!(has_warning(&plan, WarningClass::WideReach));
    core.execute(plan, &[WarningClass::WideReach]).unwrap();

    assert_eq!(
        text(&fs, &layer("app")),
        "[mcp_servers.docs]\nenabled = false\n"
    );
    let docs = inventory(&core, &in_app());
    assert_eq!(code(item(&docs, "docs").state), "disabled");
}

#[test]
fn a_server_of_a_git_versioned_layer_is_disabled_in_the_user_config_toml() {
    let fs = app("", "[mcp_servers.docs]\ncommand = \"docs-mcp\"\n");
    let core = core_versioning(&fs, &[layer("app")]);

    let plan = plan(
        &core,
        &in_app(),
        Action::Disable,
        &[&server(&layer("app"), "docs")],
    )
    .unwrap();
    assert_eq!(plan.preview().files[0].path, user_config());
    core.execute(plan, &[WarningClass::WideReach]).unwrap();

    let user = text(&fs, &user_config());
    assert_eq!(
        user,
        "\n[projects.'e:\\projects\\app']\ntrust_level = \"trusted\"\n\n[mcp_servers.docs]\nenabled = false\n"
    );
    assert_eq!(
        text(&fs, &layer("app")),
        "[mcp_servers.docs]\ncommand = \"docs-mcp\"\n"
    );
    let after = inventory(&core, &in_app());
    assert_eq!(code(item(&after, "docs").state), "disabled");
}

#[test]
fn disable_is_not_offered_when_a_git_versioned_project_layer_sets_enabled_true() {
    let fs = app(
        "[mcp_servers.docs]\ncommand = \"docs-mcp\"\n",
        "[mcp_servers.docs]\nenabled = true\n",
    );
    let core = core_versioning(&fs, &[layer("app")]);

    let inventory = inventory(&core, &in_app());
    for entry in &item(&inventory, "docs").entries {
        let disable = json_of(&entry.disable);
        assert_eq!(disable["status"], "not_offered", "{disable}");
        assert_eq!(disable["code"], "git_versioned", "{disable}");
        assert!(
            disable["sentence"].as_str().unwrap().contains("`enabled`"),
            "{disable}"
        );
    }

    let Err(error) = plan(
        &core,
        &in_app(),
        Action::Disable,
        &[&server(&user_config(), "docs")],
    ) else {
        panic!("a Disable the Project's `enabled = true` overrides is planned");
    };
    assert!(error.message.contains("`enabled`"), "{}", error.message);
}

/// A `SKILL.md` naming its skill `name`.
fn skill_md(name: &str) -> String {
    format!("---\nname: {name}\ndescription: Does {name}.\n---\nBody.\n")
}

/// The skill folder `name` in the user's `~\.agents\skills`.
fn agents_skill(name: &str) -> PathBuf {
    Path::new(HOME).join(r".agents\skills").join(name)
}

/// The skill folder `name` in the Project app's `.codex\skills`.
fn app_skill(name: &str) -> PathBuf {
    project("app").join(r".codex\skills").join(name)
}

/// What the user's `config.toml` holds under `skills.config`.
fn skill_rules(fs: &FakeFileSystem) -> Value {
    let parsed: toml::Value = toml::from_str(&text(fs, &user_config())).unwrap();
    json_of(parsed.get("skills").and_then(|skills| skills.get("config")))
}

fn skill_file(folder: &Path) -> String {
    folder.join("SKILL.md").display().to_string()
}

#[test]
fn disable_of_a_skill_adds_a_rule_by_its_skill_md_path_to_the_user_config_toml() {
    let fs = app("[mcp_servers.fetch]\ncommand = \"uvx\"\n", "")
        .with_file(agents_skill("review").join("SKILL.md"), skill_md("review"))
        .with_file(app_skill("lint").join("SKILL.md"), skill_md("lint"));
    let core = core(&fs);

    for folder in [agents_skill("review"), app_skill("lint")] {
        let plan = plan(
            &core,
            &in_app(),
            Action::Disable,
            &[&folder.display().to_string()],
        )
        .unwrap();
        // A Project's layer is no place for it: Codex reads these rules from
        // the user's file alone.
        assert_eq!(plan.preview().files[0].path, user_config());
        // The rule holds outside the Project too.
        core.execute(plan, &[WarningClass::WideReach]).unwrap();
    }

    assert_eq!(
        skill_rules(&fs),
        json!([
            { "path": skill_file(&agents_skill("review")), "enabled": false },
            { "path": skill_file(&app_skill("lint")), "enabled": false },
        ])
    );
    let user = text(&fs, &user_config());
    assert!(user.contains("\n[[skills.config]]\n"), "{user}");
    assert_eq!(text(&fs, &layer("app")), "");
    let after = inventory(&core, &in_app());
    for name in ["review", "lint"] {
        let entry = &item(&after, name).entries[0];
        assert_eq!(code(entry.state), "disabled", "{name}");
        assert_eq!(
            code(entry.reason.as_ref().unwrap().code),
            "disabled_natively"
        );
    }
}

#[test]
fn enable_of_a_skill_takes_its_path_rule_out_of_the_user_config_toml() {
    let rules = format!(
        "[mcp_servers.fetch]\ncommand = \"uvx\"\n\n# Off for now.\n[[skills.config]]\npath = '{}'\nenabled = false\n\n[[skills.config]]\nname = \"other\"\nenabled = false\n",
        skill_file(&agents_skill("review"))
    );
    let fs = FakeFileSystem::new()
        .with_file(user_config(), rules)
        .with_file(agents_skill("review").join("SKILL.md"), skill_md("review"));
    let core = core(&fs);
    let before = inventory(&core, &context(None));
    assert_eq!(code(item(&before, "review").state), "disabled");

    let plan = plan(
        &core,
        &context(None),
        Action::Enable,
        &[&agents_skill("review").display().to_string()],
    )
    .unwrap();
    core.execute(plan, &[WarningClass::WideReach]).unwrap();

    assert_eq!(
        text(&fs, &user_config()),
        "[mcp_servers.fetch]\ncommand = \"uvx\"\n\n[[skills.config]]\nname = \"other\"\nenabled = false\n"
    );
    let after = inventory(&core, &context(None));
    assert_eq!(code(item(&after, "review").state), "effective");
}

#[test]
fn a_rule_by_name_disables_every_skill_of_that_name_and_is_only_read() {
    let fs = FakeFileSystem::new()
        .with_file(
            user_config(),
            "[[skills.config]]\nname = \"review\"\nenabled = false\n",
        )
        .with_file(agents_skill("review").join("SKILL.md"), skill_md("review"))
        .with_file(
            Path::new(CODEX).join(r"skills\review\SKILL.md"),
            skill_md("review"),
        );

    let inventory = inventory(&core(&fs), &context(None));

    let entries: Vec<&config_digger_core::api::Entry> = inventory
        .items
        .iter()
        .filter(|item| item.agent == "codex" && item.name == "review")
        .flat_map(|item| &item.entries)
        .collect();
    assert_eq!(entries.len(), 2);
    for entry in entries {
        assert_eq!(code(entry.state), "disabled");
        assert_eq!(
            code(entry.reason.as_ref().unwrap().code),
            "disabled_by_name"
        );
        assert_eq!(json_of(&entry.enable)["status"], "not_offered");
    }
}

/// The folder of the version 1.0.0 of the plugin linear Codex installed
/// from openai-curated, with its server `linear` and its skill `triage`.
fn linear() -> PathBuf {
    Path::new(CODEX).join(r"plugins\cache\openai-curated\linear\1.0.0")
}

/// A Codex with the plugin linear, which `config.toml` holds `user` for.
fn with_linear(user: &str) -> FakeFileSystem {
    FakeFileSystem::new()
        .with_file(user_config(), user)
        .with_file(
            linear().join(".mcp.json"),
            json!({ "mcpServers": { "linear": { "url": "https://linear.example/mcp" } } })
                .to_string(),
        )
        .with_file(linear().join(r"skills\triage\SKILL.md"), skill_md("triage"))
}

fn linear_server() -> String {
    format!("{}#mcpServers.linear", linear().join(".mcp.json").display())
}

fn triage() -> String {
    linear().join(r"skills\triage").display().to_string()
}

#[test]
fn disable_of_a_plugin_s_skill_turns_the_whole_plugin_off() {
    let fs = with_linear("[plugins.\"linear@openai-curated\"]\nenabled = true\n");
    let core = core(&fs);

    let disable = plan(&core, &context(None), Action::Disable, &[&triage()]).unwrap();
    assert_eq!(disable.preview().files[0].path, user_config());
    // The plugin's server goes with it.
    assert!(has_warning(&disable, WarningClass::WideReach));
    core.execute(disable, &[WarningClass::WideReach]).unwrap();

    assert_eq!(
        text(&fs, &user_config()),
        "[plugins.\"linear@openai-curated\"]\nenabled = false\n"
    );
    let after = inventory(&core, &context(None));
    for name in ["linear", "linear:triage"] {
        let entry = &item(&after, name).entries[0];
        assert_eq!(code(entry.state), "disabled", "{name}");
        assert_eq!(code(entry.reason.as_ref().unwrap().code), "plugin_disabled");
    }

    // Enable turns it on again, as Codex's own `/plugins` does.
    let enable = plan(&core, &context(None), Action::Enable, &[&triage()]).unwrap();
    core.execute(enable, &[WarningClass::WideReach]).unwrap();
    assert_eq!(
        text(&fs, &user_config()),
        "[plugins.\"linear@openai-curated\"]\nenabled = true\n"
    );
}

#[test]
fn disable_of_a_plugin_s_server_turns_that_server_off_alone() {
    let fs = with_linear("[plugins.\"linear@openai-curated\"]\nenabled = true\n");
    let core = core(&fs);

    let plan = plan(&core, &context(None), Action::Disable, &[&linear_server()]).unwrap();
    assert_eq!(plan.preview().files[0].path, user_config());
    core.execute(plan, &[]).unwrap();

    let parsed: toml::Value = toml::from_str(&text(&fs, &user_config())).unwrap();
    assert_eq!(
        json_of(parsed),
        json!({ "plugins": { "linear@openai-curated": {
            "enabled": true,
            "mcp_servers": { "linear": { "enabled": false } },
        } } })
    );
    let after = inventory(&core, &context(None));
    let server = &item(&after, "linear").entries[0];
    assert_eq!(code(server.state), "disabled");
    assert_eq!(
        code(server.reason.as_ref().unwrap().code),
        "disabled_natively"
    );
    assert_eq!(code(item(&after, "linear:triage").state), "effective");
}

/// The Codex home Orca keeps in sync with `~\.codex`.
const ORCA: &str = r"C:\Users\fixture\AppData\Roaming\orca\codex-runtime-home\home";

fn orca_config() -> PathBuf {
    Path::new(ORCA).join("config.toml")
}

/// The user's `fetch` server, and Orca's copy of the user's `config.toml`
/// with a server of Orca's own, `orca-only`.
fn with_orca() -> FakeFileSystem {
    FakeFileSystem::new()
        .with_file(user_config(), "[mcp_servers.fetch]\ncommand = \"uvx\"\n")
        .with_file(
            orca_config(),
            "[mcp_servers.fetch]\ncommand = \"uvx\"\n\n[mcp_servers.orca-only]\ncommand = \"orca-mcp\"\n",
        )
}

#[test]
fn an_entry_of_the_mirrored_home_offers_the_actions_of_the_entry_it_was_copied_from() {
    let inventory = inventory(&core(&with_orca()), &context(None));

    let entry = json_of(&item_in(&inventory, "orca", "fetch").entries[0]);
    assert_eq!(entry["source"], server(&user_config(), "fetch"));
    assert_eq!(
        entry["disable"]["status"], "offered",
        "{}",
        entry["disable"]
    );
    assert_eq!(
        entry["disable"]["path"],
        user_config().display().to_string()
    );
    assert_eq!(entry["remove"]["status"], "offered", "{}", entry["remove"]);
}

#[test]
fn disable_of_an_entry_of_the_mirrored_home_goes_to_its_source_entry() {
    let fs = with_orca();
    let core = core(&fs);

    let plan = plan(
        &core,
        &context(None),
        Action::Disable,
        &[&server(&orca_config(), "fetch")],
    )
    .unwrap();

    let paths: Vec<&Path> = plan
        .preview()
        .files
        .iter()
        .map(|file| file.path.as_path())
        .collect();
    assert_eq!(paths, [user_config().as_path()]);
    let info = plan
        .preview()
        .info
        .iter()
        .find(|info| code(info.code) == "reaches_mirror")
        .expect("the preview says how the change reaches the mirror");
    assert_eq!(info.path, Path::new(ORCA));
    assert!(info.sentence.contains("Orca"), "{}", info.sentence);
    core.execute(plan, &[]).unwrap();

    assert_eq!(
        text(&fs, &user_config()),
        "[mcp_servers.fetch]\ncommand = \"uvx\"\nenabled = false\n"
    );
    assert!(!text(&fs, &orca_config()).contains("enabled"));
}

#[test]
fn remove_of_an_entry_of_the_mirrored_home_goes_to_its_source_entry() {
    let fs = with_orca();
    let core = core(&fs);

    let plan = plan(
        &core,
        &context(None),
        Action::Remove,
        &[&server(&orca_config(), "fetch")],
    )
    .unwrap();
    assert_eq!(plan.preview().files[0].path, user_config());
    assert!(
        plan.preview()
            .info
            .iter()
            .any(|info| code(info.code) == "reaches_mirror")
    );
    core.execute(plan, &[]).unwrap();

    assert_eq!(text(&fs, &user_config()), "");
}

#[test]
fn an_action_on_an_entry_only_the_mirrored_home_has_is_refused() {
    let fs = with_orca();
    let core = core(&fs);

    let inventory = inventory(&core, &context(None));
    let entry = json_of(&item_in(&inventory, "orca", "orca-only").entries[0]);
    assert_eq!(entry["source"], Value::Null);
    assert_eq!(entry["disable"]["code"], "mirrored");
    assert_eq!(entry["remove"]["code"], "mirrored");

    let Err(error) = plan(
        &core,
        &context(None),
        Action::Disable,
        &[&server(&orca_config(), "orca-only")],
    ) else {
        panic!("a Disable in the Mirrored home is planned");
    };
    assert!(error.message.contains("Orca"), "{}", error.message);
}

#[test]
fn remove_of_a_merged_entry_shows_the_server_codex_builds_before_and_after() {
    let fs = app(
        "[mcp_servers.docs]\ncommand = \"docs-mcp\"\nargs = [\"--user\"]\n",
        "[mcp_servers.docs]\nargs = [\"--app\"]\n",
    );
    let core = core(&fs);

    let plan = plan(
        &core,
        &in_app(),
        Action::Remove,
        &[&server(&layer("app"), "docs")],
    )
    .unwrap();

    let effective = plan
        .preview()
        .effective
        .iter()
        .find(|change| change.agent == "codex" && change.item == "docs")
        .expect("the preview says what becomes of docs");
    let built = |server: &Option<config_digger_core::api::BuiltServer>| {
        server.as_ref().map(|server| server.config.clone())
    };
    assert_eq!(
        built(&effective.built_before),
        Some(json!({ "command": "docs-mcp", "args": ["--app"] }))
    );
    assert_eq!(
        built(&effective.built_after),
        Some(json!({ "command": "docs-mcp", "args": ["--user"] }))
    );
    let origins: Vec<(Vec<String>, String)> = effective
        .built_after
        .as_ref()
        .unwrap()
        .origins
        .iter()
        .map(|origin| (origin.key_path.clone(), origin.entry.clone()))
        .collect();
    assert_eq!(
        origins,
        [
            (vec!["args".to_owned()], server(&user_config(), "docs")),
            (vec!["command".to_owned()], server(&user_config(), "docs")),
        ]
    );
}

/// A core over `fs` while `codex.exe` runs.
fn core_while_codex_runs(fs: &FakeFileSystem) -> Core {
    let mut ports = fake::ports(fs.clone(), env());
    ports.processes = Box::new(FakeProcesses::default().with_process(RunningProcess {
        pid: 4242,
        image: "codex.exe".to_owned(),
        path: Some(PathBuf::from(format!(
            r"{APP_DATA}\npm\node_modules\@openai\codex\bin\codex.exe"
        ))),
        cmdline: "codex.exe".to_owned(),
    }));
    Core::new(ports).unwrap()
}

#[test]
fn a_disable_while_codex_runs_warns_that_codex_rewrites_its_config_toml() {
    let fs = FakeFileSystem::new()
        .with_file(user_config(), "[mcp_servers.fetch]\ncommand = \"uvx\"\n")
        .with_file(
            Path::new(APP_DATA).join(r"npm\node_modules\@openai\codex\package.json"),
            r#"{ "version": "0.159.3" }"#,
        )
        .with_file(Path::new(APP_DATA).join(r"npm\codex.cmd"), "@echo off\n");
    let core = core_while_codex_runs(&fs);

    let plan = plan(
        &core,
        &context(None),
        Action::Disable,
        &[&server(&user_config(), "fetch")],
    )
    .unwrap();

    let warning = plan
        .warnings()
        .iter()
        .find(|warning| warning.class == WarningClass::RunningAgent)
        .expect("a running-agent warning");
    assert!(warning.sentence.contains("Codex"), "{}", warning.sentence);
    let error = core.execute(plan, &[]).unwrap_err();
    assert_eq!(error.code, ErrorCode::AckRequired);
}

#[test]
fn a_config_toml_codex_rewrote_after_the_preview_is_read_again_before_the_write() {
    let fs =
        FakeFileSystem::new().with_file(user_config(), "[mcp_servers.fetch]\ncommand = \"uvx\"\n");
    let core = core(&fs);
    let disable = |core: &Core| {
        plan(
            core,
            &context(None),
            Action::Disable,
            &[&server(&user_config(), "fetch")],
        )
        .unwrap()
    };

    // Codex writes the whole of `mcp_servers` again, adding a server: the
    // edit goes into the file as it is now, keeping the new server.
    let first = disable(&core);
    fs.put_file(
        user_config(),
        "[mcp_servers.fetch]\ncommand = \"uvx\"\n\n[mcp_servers.new]\ncommand = \"new\"\n",
    );
    core.execute(first, &[]).unwrap();
    assert_eq!(
        text(&fs, &user_config()),
        "[mcp_servers.fetch]\ncommand = \"uvx\"\nenabled = false\n\n[mcp_servers.new]\ncommand = \"new\"\n"
    );

    // Codex changes the key the edit writes: nothing is written.
    fs.put_file(user_config(), "[mcp_servers.fetch]\ncommand = \"uvx\"\n");
    let second = disable(&core);
    let changed = "[mcp_servers.fetch]\ncommand = \"uvx\"\nenabled = true\n";
    fs.put_file(user_config(), changed);
    let error = core.execute(second, &[]).unwrap_err();
    assert_eq!(error.code, ErrorCode::Stale);
    assert_eq!(text(&fs, &user_config()), changed);
}
