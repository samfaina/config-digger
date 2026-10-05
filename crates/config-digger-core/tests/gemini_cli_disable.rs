//! Gemini CLI's Disable through the core `api`, with fake ports over a
//! fixture home folder: `mcp-server-enablement.json`, which turns servers
//! off by lower-cased name in every Project, `skills.disabled` in the most
//! specific settings file, the path rules of `extension-enablement.json`,
//! read for each Project, and the extensions config-digger never removes.

use config_digger_core::api::{
    Action, Change, Context, Core, Error, Inventory, Item, Plan, ScanOptions, Targets, WarningClass,
};
use config_digger_core::ports::FileSystem;
use config_digger_core::ports::fake::{self, FakeEnvironment, FakeFileSystem, FakeGitIndex};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const HOME: &str = r"C:\Users\fixture";
const APP_DATA: &str = r"C:\Users\fixture\AppData\Roaming";
const PROGRAM_DATA: &str = r"C:\ProgramData";

/// Where the fixture Projects go.
const PROJECTS: &str = r"E:\projects";

/// `~\.gemini`.
fn gemini() -> PathBuf {
    Path::new(HOME).join(".gemini")
}

/// `~\.gemini\settings.json`.
fn user_settings() -> PathBuf {
    gemini().join("settings.json")
}

/// `~\.gemini\mcp-server-enablement.json`.
fn enablement() -> PathBuf {
    gemini().join("mcp-server-enablement.json")
}

/// The fixture Project folder `name`.
fn project(name: &str) -> PathBuf {
    Path::new(PROJECTS).join(name)
}

/// The `.gemini\settings.json` of the Project folder `folder`.
fn project_settings(folder: &Path) -> PathBuf {
    folder.join(r".gemini\settings.json")
}

/// The key Gemini CLI gives `folder` in `trustedFolders.json`: lower case,
/// with forward slashes.
fn trust_key(folder: &Path) -> String {
    folder
        .display()
        .to_string()
        .to_lowercase()
        .replace('\\', "/")
}

/// `fs` with `trustedFolders.json` trusting each of `folders`, as Gemini
/// CLI writes it.
fn trusting(fs: FakeFileSystem, folders: &[&Path]) -> FakeFileSystem {
    let rules: serde_json::Map<String, Value> = folders
        .iter()
        .map(|folder| (trust_key(folder), json!("TRUST_FOLDER")))
        .collect();
    folders
        .iter()
        .fold(fs, |fs, folder| fs.with_folder(*folder))
        .with_file(
            gemini().join("trustedFolders.json"),
            Value::from(rules).to_string(),
        )
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

fn inventory(core: &Core, context: &Context) -> Inventory {
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    core.resolve(&snapshot, context)
}

fn plan(core: &Core, context: &Context, action: Action, selectors: &[&str]) -> Result<Plan, Error> {
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    let targets = Targets {
        selectors: selectors.iter().map(|s| (*s).to_owned()).collect(),
        all: false,
    };
    core.plan(&snapshot, context, action, &targets)
}

/// The Gemini CLI Item of the kind `kind` named `name` in the default home.
fn item<'i>(inventory: &'i Inventory, kind: &str, name: &str) -> &'i Item {
    inventory
        .items
        .iter()
        .find(|item| {
            item.agent == "gemini-cli"
                && item.home == "default"
                && code(item.kind) == kind
                && item.name == name
        })
        .unwrap_or_else(|| panic!("no Gemini CLI {kind} Item `{name}`"))
}

fn server<'i>(inventory: &'i Inventory, name: &str) -> &'i Item {
    item(inventory, "mcp", name)
}

fn json_of(value: impl serde::Serialize) -> Value {
    serde_json::to_value(value).unwrap()
}

fn code(value: impl serde::Serialize) -> String {
    json_of(value).as_str().unwrap().to_owned()
}

fn text(fs: &FakeFileSystem, path: &Path) -> String {
    String::from_utf8(fs.read(path).unwrap()).unwrap()
}

/// The locator of the server `name` in the settings file `path`.
fn locator(path: &Path, name: &str) -> String {
    format!("{}#mcpServers.{name}", path.display())
}

/// The preview's rows, as (locator, Project, before, after).
fn changes(plan: &Plan) -> Vec<(String, Option<PathBuf>, String, String)> {
    plan.preview()
        .changes
        .iter()
        .map(|change: &Change| {
            (
                change.locator.clone(),
                change.project.clone(),
                code(change.before),
                code(change.after),
            )
        })
        .collect()
}

fn wide_reach(plan: &Plan) -> String {
    plan.warnings()
        .iter()
        .find(|warning| warning.class == WarningClass::WideReach)
        .map(|warning| warning.sentence.clone())
        .expect("a wide-reach warning")
}

#[test]
fn disable_of_a_server_turns_off_its_lower_cased_name_in_every_project() {
    let app = project("app");
    let fs = trusting(
        FakeFileSystem::new()
            .with_file(
                user_settings(),
                r#"{ "mcpServers": { "Docs": { "command": "user-docs" } } }"#,
            )
            .with_file(
                project_settings(&app),
                r#"{ "mcpServers": { "docs": { "command": "app-docs" } } }"#,
            ),
        &[&app],
    );
    let core = core(&fs);

    let plan = plan(
        &core,
        &context(None),
        Action::Disable,
        &[&locator(&user_settings(), "Docs")],
    )
    .unwrap();

    assert_eq!(plan.preview().files[0].path, enablement());
    let rows = changes(&plan);
    assert!(
        rows.contains(&(
            locator(&project_settings(&app), "docs"),
            Some(app.clone()),
            "effective".into(),
            "disabled".into()
        )),
        "{rows:?}"
    );
    let sentence = wide_reach(&plan);
    assert!(sentence.contains("all projects"), "{sentence}");
    assert!(
        sentence.contains(&locator(&project_settings(&app), "docs")),
        "{sentence}"
    );
    core.execute(plan, &[WarningClass::WideReach]).unwrap();

    let written: Value = serde_json::from_str(&text(&fs, &enablement())).unwrap();
    assert_eq!(written, json!({ "docs": { "enabled": false } }));
    let after = inventory(&core, &context(Some(&app)));
    let docs = server(&after, "docs");
    assert_eq!(code(docs.state), "disabled");
    assert!(
        docs.entries
            .iter()
            .all(|entry| code(entry.state) == "disabled")
    );
}

#[test]
fn enable_takes_the_lower_cased_name_out_of_the_enablement_file() {
    let fs = FakeFileSystem::new()
        .with_file(
            user_settings(),
            r#"{ "mcpServers": { "Docs": { "command": "user-docs" } } }"#,
        )
        .with_file(
            enablement(),
            r#"{ "docs": { "enabled": false }, "fetch": { "enabled": false } }"#,
        );
    let core = core(&fs);
    let before = inventory(&core, &context(None));
    let docs = server(&before, "Docs");
    assert_eq!(code(docs.state), "disabled");
    let reason = json_of(&docs.entries[0].reason);
    assert_eq!(reason["code"], "disabled_by_name", "{reason}");

    let plan = plan(
        &core,
        &context(None),
        Action::Enable,
        &[&locator(&user_settings(), "Docs")],
    )
    .unwrap();
    let sentence = wide_reach(&plan);
    assert!(sentence.contains("`Docs` in all projects"), "{sentence}");
    core.execute(plan, &[WarningClass::WideReach]).unwrap();

    let written: Value = serde_json::from_str(&text(&fs, &enablement())).unwrap();
    assert_eq!(written["fetch"], json!({ "enabled": false }));
    assert_ne!(written["docs"]["enabled"], json!(false), "{written}");
    let after = inventory(&core, &context(None));
    assert_eq!(code(server(&after, "Docs").state), "effective");
}

#[test]
fn an_unreadable_enablement_file_disables_nothing_with_a_caveat_and_takes_no_disable() {
    let fs = FakeFileSystem::new()
        .with_file(
            user_settings(),
            r#"{ "mcpServers": { "docs": { "command": "user-docs" } } }"#,
        )
        .with_file(enablement(), r#"{ "docs": { "enabled": false }, "#);
    let inventory = inventory(&core(&fs), &context(None));

    let docs = server(&inventory, "docs");
    assert_eq!(code(docs.state), "effective");
    let entry = json_of(&docs.entries[0]);
    let caveats: Vec<&str> = entry["caveats"]
        .as_array()
        .unwrap()
        .iter()
        .map(|caveat| caveat["code"].as_str().unwrap())
        .collect();
    assert!(caveats.contains(&"switch_read_failure"), "{caveats:?}");
    assert_eq!(
        entry["disable"]["status"], "not_offered",
        "{}",
        entry["disable"]
    );
    assert_eq!(
        entry["disable"]["code"], "unreadable_switch",
        "{}",
        entry["disable"]
    );
}

fn skill_md(name: &str) -> String {
    format!("---\nname: {name}\ndescription: Does {name}.\n---\n# {name}\n")
}

/// `fs` with the skill `name` in `~\.gemini\skills`.
fn with_user_skill(fs: FakeFileSystem, name: &str) -> FakeFileSystem {
    fs.with_file(
        gemini().join("skills").join(name).join("SKILL.md"),
        skill_md(name),
    )
}

/// The locator of the skill `name` in `~\.gemini\skills`.
fn user_skill(name: &str) -> String {
    gemini().join("skills").join(name).display().to_string()
}

fn skill<'i>(inventory: &'i Inventory, name: &str) -> &'i Item {
    item(inventory, "skill", name)
}

/// The Project app, trusted, with its own settings file, and the user's
/// skill `review`.
fn app_with_review() -> FakeFileSystem {
    let app = project("app");
    with_user_skill(
        trusting(
            FakeFileSystem::new()
                .with_file(user_settings(), "{}\n")
                .with_file(project_settings(&app), "{}\n"),
            &[&app],
        ),
        "review",
    )
}

#[test]
fn disable_of_a_skill_writes_skills_disabled_in_the_project_s_settings() {
    let app = project("app");
    let fs = app_with_review();
    let core = core(&fs);

    let plan = plan(
        &core,
        &context(Some(&app)),
        Action::Disable,
        &[&user_skill("review")],
    )
    .unwrap();
    assert_eq!(plan.preview().files[0].path, project_settings(&app));
    core.execute(plan, &[]).unwrap();

    let written: Value = serde_json::from_str(&text(&fs, &project_settings(&app))).unwrap();
    assert_eq!(written, json!({ "skills": { "disabled": ["review"] } }));
    assert_eq!(text(&fs, &user_settings()), "{}\n");
    let here = inventory(&core, &context(Some(&app)));
    assert_eq!(code(skill(&here, "review").state), "disabled");
    let elsewhere = inventory(&core, &context(None));
    assert_eq!(code(skill(&elsewhere, "review").state), "effective");
}

#[test]
fn a_skill_of_a_git_versioned_project_is_disabled_in_the_user_s_settings() {
    let app = project("app");
    let fs = app_with_review();
    let core = core_versioning(&fs, &[project_settings(&app)]);

    let plan = plan(
        &core,
        &context(Some(&app)),
        Action::Disable,
        &[&user_skill("review")],
    )
    .unwrap();
    assert_eq!(plan.preview().files[0].path, user_settings());
    core.execute(plan, &[WarningClass::WideReach]).unwrap();

    let written: Value = serde_json::from_str(&text(&fs, &user_settings())).unwrap();
    assert_eq!(written, json!({ "skills": { "disabled": ["review"] } }));
    assert_eq!(text(&fs, &project_settings(&app)), "{}\n");
}

#[test]
fn a_skill_is_disabled_in_the_user_s_settings_with_no_project() {
    let fs = app_with_review();
    let core = core(&fs);

    let plan = plan(
        &core,
        &context(None),
        Action::Disable,
        &[&user_skill("review")],
    )
    .unwrap();
    assert_eq!(plan.preview().files[0].path, user_settings());
    core.execute(plan, &[WarningClass::WideReach]).unwrap();

    let after = inventory(&core, &context(Some(&project("app"))));
    assert_eq!(code(skill(&after, "review").state), "disabled");
}

#[test]
fn skills_disabled_holds_names_without_regard_to_case() {
    let fs = with_user_skill(
        FakeFileSystem::new().with_file(
            user_settings(),
            r#"{ "skills": { "disabled": ["Review"] } }"#,
        ),
        "review",
    );
    let core = core(&fs);
    assert_eq!(
        code(skill(&inventory(&core, &context(None)), "review").state),
        "disabled"
    );

    let plan = plan(
        &core,
        &context(None),
        Action::Enable,
        &[&user_skill("review")],
    )
    .unwrap();
    core.execute(plan, &[WarningClass::WideReach]).unwrap();

    let written: Value = serde_json::from_str(&text(&fs, &user_settings())).unwrap();
    assert_eq!(written, json!({ "skills": { "disabled": [] } }));
}

#[test]
fn an_untrusted_project_s_settings_disable_no_skill_and_take_no_disable() {
    let app = project("app");
    let fs = with_user_skill(
        FakeFileSystem::new()
            .with_folder(&app)
            .with_file(
                gemini().join("projects.json"),
                json!({ "projects": { app.display().to_string().to_lowercase(): "app" } })
                    .to_string(),
            )
            .with_file(user_settings(), "{}\n")
            .with_file(
                project_settings(&app),
                r#"{ "skills": { "disabled": ["review"] } }"#,
            ),
        "review",
    );
    let core = core(&fs);
    assert_eq!(
        code(skill(&inventory(&core, &context(Some(&app))), "review").state),
        "effective"
    );

    let plan = plan(
        &core,
        &context(Some(&app)),
        Action::Disable,
        &[&user_skill("review")],
    )
    .unwrap();
    assert_eq!(plan.preview().files[0].path, user_settings());
}

fn extensions() -> PathBuf {
    gemini().join("extensions")
}

fn extension_enablement() -> PathBuf {
    extensions().join("extension-enablement.json")
}

/// `fs` with the extension `weather` installed by copy, with the server
/// `forecast` and the skill `radar`.
fn with_weather(fs: FakeFileSystem) -> FakeFileSystem {
    let folder = extensions().join("weather");
    fs.with_file(
        folder.join(".gemini-extension-install.json"),
        json!({ "source": "https://github.com/example/weather", "type": "git" }).to_string(),
    )
    .with_file(
        folder.join("gemini-extension.json"),
        json!({ "name": "weather", "mcpServers": { "forecast": { "command": "node" } } })
            .to_string(),
    )
    .with_file(folder.join(r"skills\radar\SKILL.md"), skill_md("radar"))
}

fn forecast() -> String {
    locator(
        &extensions().join(r"weather\gemini-extension.json"),
        "forecast",
    )
}

/// The Projects app, its subfolder api, and other, each trusted, with the
/// extension `weather`.
fn weather_and_projects() -> FakeFileSystem {
    with_weather(trusting(
        FakeFileSystem::new().with_file(user_settings(), "{}\n"),
        &[&project("app"), &project(r"app\api"), &project("other")],
    ))
}

#[test]
fn disable_of_an_extension_writes_a_rule_for_the_project_reaching_the_projects_below() {
    let app = project("app");
    let api = project(r"app\api");
    let fs = weather_and_projects();
    let core = core(&fs);

    let plan = plan(&core, &context(Some(&app)), Action::Disable, &[&forecast()]).unwrap();

    assert_eq!(plan.preview().files[0].path, extension_enablement());
    let rows = changes(&plan);
    assert!(
        rows.contains(&(
            forecast(),
            Some(api.clone()),
            "effective".into(),
            "disabled".into()
        )),
        "{rows:?}"
    );
    assert!(
        !rows.iter().any(|row| row.1 == Some(project("other"))),
        "{rows:?}"
    );
    let sentence = wide_reach(&plan);
    assert!(sentence.contains(&api.display().to_string()), "{sentence}");
    assert!(sentence.contains("radar"), "{sentence}");
    core.execute(plan, &[WarningClass::WideReach]).unwrap();

    let written: Value = serde_json::from_str(&text(&fs, &extension_enablement())).unwrap();
    assert_eq!(
        written,
        json!({ "weather": { "overrides": ["!/E:/projects/app/*"] } })
    );
    let here = inventory(&core, &context(Some(&app)));
    assert_eq!(code(server(&here, "forecast").state), "disabled");
    assert_eq!(code(skill(&here, "radar").state), "disabled");
    let reason = json_of(&server(&here, "forecast").entries[0].reason);
    assert_eq!(reason["code"], "plugin_disabled", "{reason}");
    let other = inventory(&core, &context(Some(&project("other"))));
    assert_eq!(code(server(&other, "forecast").state), "effective");
}

/// `fs` with `extension-enablement.json` holding `rules` for `weather`.
fn with_rules(fs: FakeFileSystem, rules: &[&str]) -> FakeFileSystem {
    fs.with_file(
        extension_enablement(),
        json!({ "weather": { "overrides": rules } }).to_string(),
    )
}

#[test]
fn a_rule_for_a_drive_s_root_disables_an_extension_in_every_project_on_that_drive() {
    let fs = with_rules(weather_and_projects(), &["/C:/Users/fixture/*", "!/E:/*"]);
    let core = core(&fs);

    let app = inventory(&core, &context(Some(&project("app"))));
    let forecast = server(&app, "forecast");
    assert_eq!(code(forecast.state), "disabled");
    let reason = json_of(&forecast.entries[0].reason);
    assert_eq!(reason["code"], "plugin_disabled", "{reason}");
    let sentence = reason["sentence"].as_str().unwrap();
    assert!(sentence.contains("!/E:/*"), "{sentence}");
    let other = inventory(&core, &context(Some(&project("other"))));
    assert_eq!(code(server(&other, "forecast").state), "disabled");
    let none = inventory(&core, &context(None));
    assert_eq!(code(server(&none, "forecast").state), "effective");
}

#[test]
fn enable_under_a_wider_rule_adds_a_rule_for_the_project_reaching_the_projects_below() {
    let app = project("app");
    let fs = with_rules(weather_and_projects(), &["!/E:/*"]);
    let core = core(&fs);

    let plan = plan(&core, &context(Some(&app)), Action::Enable, &[&forecast()]).unwrap();

    let rows = changes(&plan);
    assert!(
        rows.contains(&(
            forecast(),
            Some(project(r"app\api")),
            "disabled".into(),
            "effective".into()
        )),
        "{rows:?}"
    );
    assert!(
        !rows.iter().any(|row| row.1 == Some(project("other"))),
        "{rows:?}"
    );
    core.execute(plan, &[WarningClass::WideReach]).unwrap();

    let written: Value = serde_json::from_str(&text(&fs, &extension_enablement())).unwrap();
    assert_eq!(
        written,
        json!({ "weather": { "overrides": ["!/E:/*", "/E:/projects/app/*"] } })
    );
    let other = inventory(&core, &context(Some(&project("other"))));
    assert_eq!(code(server(&other, "forecast").state), "disabled");
}

#[test]
fn enable_takes_the_project_s_own_rule_out() {
    let app = project("app");
    let fs = with_rules(
        weather_and_projects(),
        &["/C:/Users/fixture/*", "!/e:/projects/app/*"],
    );
    let core = core(&fs);

    let plan = plan(&core, &context(Some(&app)), Action::Enable, &[&forecast()]).unwrap();
    core.execute(plan, &[WarningClass::WideReach]).unwrap();

    let written: Value = serde_json::from_str(&text(&fs, &extension_enablement())).unwrap();
    assert_eq!(
        written,
        json!({ "weather": { "overrides": ["/C:/Users/fixture/*"] } })
    );
    let after = inventory(&core, &context(Some(&app)));
    assert_eq!(code(server(&after, "forecast").state), "effective");
    assert_eq!(code(skill(&after, "radar").state), "effective");
}

#[test]
fn with_no_project_an_extension_takes_no_disable() {
    let fs = weather_and_projects();
    let inventory = inventory(&core(&fs), &context(None));

    for entry in [
        &server(&inventory, "forecast").entries[0],
        &skill(&inventory, "radar").entries[0],
    ] {
        let disable = json_of(&entry.disable);
        assert_eq!(disable["status"], "not_offered", "{disable}");
        assert_eq!(
            disable["sentence"],
            "Disable is not offered: Gemini CLI turns an extension off per folder; choose a Project."
        );
    }
}

#[test]
fn an_extension_s_entries_have_no_remove_and_say_how_to_uninstall_it() {
    let fs = weather_and_projects();
    let inventory = inventory(&core(&fs), &context(Some(&project("app"))));

    for entry in [
        &server(&inventory, "forecast").entries[0],
        &skill(&inventory, "radar").entries[0],
    ] {
        let entry = json_of(entry);
        assert_eq!(
            entry["disable"]["status"], "offered",
            "{}",
            entry["disable"]
        );
        assert_eq!(
            entry["remove"]["status"], "not_offered",
            "{}",
            entry["remove"]
        );
        assert_eq!(
            entry["remove"]["how_to_remove"]["command"],
            "gemini extensions uninstall weather"
        );
    }
}

#[test]
fn with_no_project_known_a_server_s_disable_still_warns_that_it_reaches_all_projects() {
    let fs = FakeFileSystem::new().with_file(
        user_settings(),
        r#"{ "mcpServers": { "docs": { "command": "user-docs" } } }"#,
    );
    let core = core(&fs);

    let plan = plan(
        &core,
        &context(None),
        Action::Disable,
        &[&locator(&user_settings(), "docs")],
    )
    .unwrap();

    let sentence = wide_reach(&plan);
    assert!(sentence.contains("`docs` in all projects"), "{sentence}");
    let refused = core.execute(plan, &[]).unwrap_err();
    assert_eq!(code(refused.code), "ack_required");
}

#[test]
fn a_rule_reaches_only_the_folder_spelled_with_its_case() {
    let fs = with_rules(weather_and_projects(), &["!/E:/Projects/App/*"]);
    let inventory = inventory(&core(&fs), &context(Some(&project("app"))));

    assert_eq!(code(server(&inventory, "forecast").state), "effective");
}
