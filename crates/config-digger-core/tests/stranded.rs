//! Stranded Entries through the core `api`, with fake ports: a parked Entry
//! whose Agent definition, Agent home or location is no longer found is
//! listed apart, with the first of those reasons, and can only be Removed
//! (ADR 0002). A parked-Entry store, or an Entry in it, that cannot be read
//! is a Read failure of the store.

use config_digger_core::api::{
    Action, Context, Core, Error, ErrorCode, Inventory, Outcome, Plan, ScanOptions, Stranded,
    Targets, WarningClass,
};
use config_digger_core::ports::RunningProcess;
use config_digger_core::ports::fake::{self, FakeEnvironment, FakeFileSystem, FakeProcesses};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const HOME: &str = r"C:\Users\fixture";
const APP_DATA: &str = r"C:\Users\fixture\AppData\Roaming";
const LOCAL_APP_DATA: &str = r"C:\Users\fixture\AppData\Local";
const PROJECT: &str = r"E:\work\one";

/// The Store package's data folder: Claude Desktop's `msix` home.
fn msix() -> PathBuf {
    PathBuf::from(LOCAL_APP_DATA).join(r"Packages\Claude_pzs8sxrjxfjjc\LocalCache\Roaming\Claude")
}

fn config() -> PathBuf {
    msix().join("claude_desktop_config.json")
}

/// The manifest of config-digger's parked-Entry store.
fn store() -> PathBuf {
    PathBuf::from(LOCAL_APP_DATA).join(r"config-digger\parked\manifest.json")
}

/// The folder where the store keeps the skill parked as `id`.
fn parked_folder(id: &str) -> PathBuf {
    PathBuf::from(LOCAL_APP_DATA)
        .join(r"config-digger\parked\folders")
        .join(id)
}

/// The locator of the Entry parked as `id`.
fn in_store(id: &str) -> String {
    format!("{}#entries.{id}", store().display())
}

/// Claude Desktop's `msix` home, with the user servers `notes` and `github`,
/// whose env holds a Secret.
fn desktop() -> FakeFileSystem {
    FakeFileSystem::new().with_file(
        config(),
        r#"{
  "mcpServers": {
    "notes": { "command": "notes-mcp", "args": ["--vault", "C:\\Users\\fixture\\notes"] },
    "github": {
      "command": "github-mcp",
      "env": { "GITHUB_TOKEN": "ghp_notarealsecret0000000000000000000000" }
    }
  }
}
"#,
    )
}

fn env() -> FakeEnvironment {
    FakeEnvironment::with_home(HOME)
        .with_var("APPDATA", APP_DATA)
        .with_var("LOCALAPPDATA", LOCAL_APP_DATA)
}

fn core(fs: &FakeFileSystem) -> Core {
    Core::new(fake::ports(fs.clone(), env())).unwrap()
}

fn scan(core: &Core) -> config_digger_core::api::Snapshot {
    core.scan(&ScanOptions {
        projects: vec![PathBuf::from(PROJECT)],
    })
    .unwrap()
}

fn stranded(core: &Core) -> Vec<Stranded> {
    core.stranded(&scan(core))
}

fn inventory(core: &Core) -> Inventory {
    core.resolve(&scan(core), &Context::default())
}

fn plan(core: &Core, action: Action, selectors: &[&str]) -> Result<Plan, Error> {
    let targets = Targets {
        selectors: selectors
            .iter()
            .map(|&selector| selector.to_owned())
            .collect(),
        all: false,
        ..Targets::default()
    };
    core.plan(&scan(core), &Context::default(), action, &targets)
}

fn act(core: &Core, action: Action, selectors: &[&str]) -> Result<Outcome, Error> {
    core.execute(plan(core, action, selectors)?, &[WarningClass::WideReach])
}

fn undo(core: &Core, operation: &str) -> Outcome {
    core.execute(core.plan_undo(operation).unwrap(), &[])
        .unwrap()
}

fn to_json(value: impl serde::Serialize) -> Value {
    serde_json::to_value(value).unwrap()
}

fn read_json(fs: &FakeFileSystem, path: impl AsRef<Path>) -> Value {
    serde_json::from_slice(&fs.contents(path).expect("the file exists")).unwrap()
}

/// Parks `notes` and `github` from Claude Desktop's `msix` home, then
/// deletes the home, as uninstalling the Store package does.
fn stranded_desktop() -> FakeFileSystem {
    let fs = desktop();
    act(&core(&fs), Action::Disable, &["notes"]).unwrap();
    act(&core(&fs), Action::Disable, &["github"]).unwrap();
    fs.delete_folder(msix());
    fs
}

#[test]
fn an_entry_parked_from_a_home_that_is_then_deleted_is_listed_as_stranded() {
    let fs = stranded_desktop();
    let core = core(&fs);

    let stranded = stranded(&core);

    let locators: Vec<&str> = stranded.iter().map(|one| one.locator.as_str()).collect();
    assert_eq!(locators, [in_store("github"), in_store("notes")]);
    let notes = to_json(&stranded[1]);
    assert_eq!(notes["name"], "notes");
    assert_eq!(notes["kind"], "mcp");
    assert_eq!(notes["agent"], "claude-desktop");
    assert_eq!(notes["agent_name"], "Claude Desktop");
    assert_eq!(notes["reason"], "home_not_found");
    assert_eq!(notes["origin"]["home"], "msix");
    assert_eq!(notes["origin"]["location"], "user-mcp");
    assert_eq!(notes["origin"]["project"], Value::Null);
    assert_eq!(notes["origin"]["path"], to_json(config()));
    assert_eq!(
        notes["origin"]["locator"],
        format!("{}#mcpServers.notes", config().display())
    );
    assert_eq!(notes["path"], to_json(store()));
    assert_eq!(notes["key_path"], json!(["entries", "notes"]));
    assert!(
        notes["parked_at"].as_str().unwrap().starts_with("20"),
        "{notes}"
    );
    let sentence = notes["sentence"].as_str().unwrap();
    assert!(
        sentence.contains("Claude Desktop home `msix` is not found"),
        "{sentence}"
    );
    let inventory = inventory(&core);
    assert!(
        !inventory.items.iter().any(|item| item.name == "notes"),
        "{:?}",
        inventory.items
    );
}

#[test]
fn a_stranded_entry_whose_location_is_loaded_is_masked_as_its_location_masks() {
    let fs = stranded_desktop();

    let github = to_json(&stranded(&core(&fs))[0]);

    assert_eq!(github["config"]["command"], "github-mcp");
    assert_eq!(github["config"]["env"]["GITHUB_TOKEN"], "ghp_••••");
    assert_eq!(github["masked"], json!([["env", "GITHUB_TOKEN"]]));
}

#[test]
fn enable_and_disable_of_a_stranded_entry_are_refused_naming_what_is_missing() {
    let fs = stranded_desktop();
    let core = core(&fs);
    let before = fs.contents(store());

    for action in [Action::Enable, Action::Disable] {
        let error = plan(&core, action, &[&in_store("notes")]).err().unwrap();

        assert_eq!(error.code, ErrorCode::Blocked);
        assert!(error.message.contains("Stranded"), "{}", error.message);
        assert!(
            error
                .message
                .contains("Claude Desktop home `msix` is not found"),
            "{}",
            error.message
        );
    }
    assert_eq!(fs.contents(store()), before);
}

#[test]
fn once_its_home_is_found_again_a_stranded_entry_is_parked_and_enable_puts_it_back() {
    let fs = desktop();
    let core = core(&fs);
    act(&core, Action::Disable, &["notes"]).unwrap();
    let parked_config = fs.contents(config()).unwrap();
    fs.delete_folder(msix());
    assert_eq!(stranded(&core).len(), 1);

    fs.put_file(config(), parked_config);

    assert_eq!(stranded(&core), []);
    act(&core, Action::Enable, &["notes"]).unwrap();
    assert_eq!(
        read_json(&fs, config())["mcpServers"]["notes"]["command"],
        "notes-mcp"
    );
    assert_eq!(read_json(&fs, store())["entries"], json!({}));
}

#[test]
fn remove_of_a_stranded_entry_by_its_locator_goes_through_preview_verify_and_undo() {
    let fs = stranded_desktop();
    let core = core(&fs);

    let plan = plan(&core, Action::Remove, &[&in_store("github")]).unwrap();
    assert_eq!(plan.preview().title, "Remove 1 Entry");
    let files: Vec<&PathBuf> = plan.preview().files.iter().map(|file| &file.path).collect();
    assert_eq!(files, [&store()]);
    assert_eq!(plan.preview().changes, []);
    let info = to_json(&plan.preview().info);
    assert_eq!(info[0]["code"], "removes_stranded", "{info}");
    let sentence = info[0]["sentence"].as_str().unwrap();
    assert!(
        sentence.contains("Stranded Entry `github` of Claude Desktop"),
        "{sentence}"
    );
    assert!(plan.blocks().is_empty(), "{:?}", plan.blocks());
    for file in &plan.preview().files {
        let diff = file.diff.as_deref().unwrap_or_else(|| panic!("{file:?}"));
        assert!(!diff.contains("notarealsecret"), "{diff}");
    }
    let outcome = core.execute(plan, &[]).unwrap();
    assert_eq!(to_json(outcome.verify), "verified");

    let entries = &read_json(&fs, store())["entries"];
    assert_eq!(entries.get("github"), None);
    assert_eq!(entries["notes"]["name"], "notes");
    assert_eq!(stranded(&core).len(), 1);

    undo(&core, &outcome.operation);

    let locators: Vec<String> = stranded(&core).into_iter().map(|one| one.locator).collect();
    assert_eq!(locators, [in_store("github"), in_store("notes")]);
}

#[test]
fn a_running_agent_does_not_block_the_remove_of_a_stranded_entry() {
    let fs = stranded_desktop();
    let mut ports = fake::ports(
        fs.clone(),
        env().with_var("ProgramFiles", r"C:\Program Files"),
    );
    let package = r"C:\Program Files\WindowsApps\Claude_2.16120.0.0_x64__pzs8sxrjxfjjc";
    ports.processes = Box::new(FakeProcesses::default().with_process(RunningProcess {
        pid: 5151,
        image: "claude.exe".to_owned(),
        path: Some(PathBuf::from(package).join(r"app\claude.exe")),
        cmdline: String::new(),
    }));
    let core = Core::new(ports).unwrap();

    let plan = plan(&core, Action::Remove, &[&in_store("notes")]).unwrap();

    assert!(plan.blocks().is_empty(), "{:?}", plan.blocks());
    assert!(plan.warnings().is_empty(), "{:?}", plan.warnings());
    core.execute(plan, &[]).unwrap();
}

#[test]
fn an_item_name_never_selects_a_stranded_entry_even_when_a_live_entry_has_its_name() {
    let fs = stranded_desktop();
    let classic = PathBuf::from(APP_DATA).join(r"Claude\claude_desktop_config.json");
    fs.put_file(
        &classic,
        r#"{ "mcpServers": { "notes": { "command": "other-notes" } } }"#,
    );
    let core = core(&fs);

    let plan = plan(&core, Action::Remove, &["notes"]).unwrap();

    let files: Vec<&PathBuf> = plan.preview().files.iter().map(|file| &file.path).collect();
    assert_eq!(files, [&classic]);
    let error = self::plan(&core, Action::Remove, &["github"])
        .err()
        .unwrap();
    assert_eq!(error.code, ErrorCode::NoMatch);
}

/// An Agent whose project servers have no switch, so that config-digger
/// parks them.
const PARKING_AGENT: &str = r#"
format_version = 1

[agent]
id = "parking-agent"
name = "Parking Agent"

[[homes]]
id = "default"
folder = { block = "path", path = "~" }
roots.state = ".parking-agent.json"

[[scopes]]
id = "project"

[[locations]]
id = "project-mcp"
anchor = { block = "project", path = ".parking-agent.json" }
format = { block = "json", dialect = "strict" }
root = ["servers"]
kind = "mcp"
scope = "project"
provenance = "file"
store_class = "plain"
on_read_failure = "ignores_file"

[items.mcp]
precedence = ["project-mcp"]
competition = { block = "exact-name" }
checks = ["disable", "precedence"]

[[items.mcp.disable]]
block = "park"
locations = ["project-mcp"]
"#;

fn project_file() -> PathBuf {
    PathBuf::from(PROJECT).join(".parking-agent.json")
}

fn parking_core(fs: &FakeFileSystem, definition: &str) -> Core {
    let ports = fake::ports(fs.clone(), env());
    Core::with_extra_definitions(ports, &[("parking-agent.toml", definition)]).unwrap()
}

/// Parks `alpha`, whose `command` is `secret-command`, from the Parking
/// Agent's Project.
fn parked_alpha() -> FakeFileSystem {
    let fs = FakeFileSystem::new()
        .with_file(PathBuf::from(HOME).join(".parking-agent.json"), "{}")
        .with_file(
            project_file(),
            r#"{ "servers": { "alpha": { "command": "secret-command", "timeout": 30 } } }"#,
        );
    let core = parking_core(&fs, PARKING_AGENT);
    let targets = Targets {
        selectors: vec!["alpha".to_owned()],
        all: false,
        ..Targets::default()
    };
    let context = Context {
        project: Some(PathBuf::from(PROJECT)),
    };
    let plan = core
        .plan(&scan(&core), &context, Action::Disable, &targets)
        .unwrap();
    core.execute(plan, &[]).unwrap();
    fs
}

#[test]
fn an_entry_whose_location_id_is_no_longer_in_the_definition_is_stranded() {
    let fs = parked_alpha();
    let renamed = PARKING_AGENT.replace("project-mcp", "project-servers");
    let core = parking_core(&fs, &renamed);

    let stranded = stranded(&core);

    assert_eq!(stranded.len(), 1, "{stranded:?}");
    let alpha = to_json(&stranded[0]);
    assert_eq!(alpha["reason"], "location_gone");
    assert_eq!(alpha["agent_name"], "Parking Agent");
    assert_eq!(alpha["kind"], "mcp");
    assert_eq!(alpha["origin"]["project"], to_json(PathBuf::from(PROJECT)));
    let sentence = alpha["sentence"].as_str().unwrap();
    assert!(
        sentence.contains("location `project-mcp` is no longer in the definition of Parking Agent"),
        "{sentence}"
    );
}

#[test]
fn an_entry_whose_agent_definition_is_no_longer_loaded_is_stranded_with_every_text_masked() {
    let fs = parked_alpha();
    let core = core(&fs);

    let stranded = stranded(&core);

    assert_eq!(stranded.len(), 1, "{stranded:?}");
    let alpha = to_json(&stranded[0]);
    assert_eq!(alpha["reason"], "no_definition");
    assert_eq!(alpha["agent"], "parking-agent");
    assert_eq!(alpha["agent_name"], "parking-agent");
    assert_eq!(alpha["config"], json!({ "command": "••••", "timeout": 30 }));
    assert_eq!(alpha["masked"], json!([["command"]]));
    let sentence = alpha["sentence"].as_str().unwrap();
    assert!(
        sentence.contains("no definition of the Agent `parking-agent` is loaded"),
        "{sentence}"
    );
    let error = plan(&core, Action::Enable, &[&in_store("alpha")])
        .err()
        .unwrap();
    assert_eq!(error.code, ErrorCode::Blocked);
    assert!(
        error.message.contains("`parking-agent`"),
        "{}",
        error.message
    );
}

/// A store holding a server and a skill folder parked from an Agent no
/// definition is loaded for.
fn store_of_a_gone_agent() -> FakeFileSystem {
    let origin = |name: &str, key_path: Value, path: &str| {
        json!({
            "agent": "gone-agent",
            "home": "default",
            "location": "user",
            "name": name,
            "path": path,
            "key_path": key_path,
            "parked_at": "2026-10-01T12:00:00Z",
        })
    };
    let mut server = origin(
        "linear",
        json!(["mcpServers", "linear"]),
        r"C:\Users\fixture\.gone\config.json",
    );
    server["entry"] = json!({ "url": "https://mcp.linear.app/sse" });
    let mut skill = origin("review", json!([]), r"C:\Users\fixture\.gone\skills\review");
    skill["entry"] = json!({ "name": "review" });
    skill["folder"] = json!("review");
    let manifest = json!({ "entries": { "linear": server, "review": skill } });
    FakeFileSystem::new()
        .with_file(store(), serde_json::to_vec_pretty(&manifest).unwrap())
        .with_file(
            parked_folder(r"review\SKILL.md"),
            "---\nname: review\ndescription: Reviews.\n---\n",
        )
}

#[test]
fn a_parked_skill_folder_is_stranded_as_a_skill() {
    let fs = store_of_a_gone_agent();

    let stranded = stranded(&core(&fs));

    let kinds: Vec<Value> = stranded.iter().map(|one| to_json(one.kind)).collect();
    assert_eq!(kinds, [json!("mcp"), json!("skill")]);
    assert_eq!(
        stranded[1].origin.locator,
        r"C:\Users\fixture\.gone\skills\review"
    );
}

#[test]
fn remove_stranded_removes_every_stranded_entry_in_one_operation_and_undo_puts_them_back() {
    let fs = store_of_a_gone_agent();
    let core = core(&fs);

    let plan = core.plan_remove_stranded(&scan(&core)).unwrap();

    assert_eq!(plan.preview().title, "Remove 2 Entries");
    let folders: Vec<&PathBuf> = plan
        .preview()
        .folders
        .iter()
        .map(|folder| &folder.path)
        .collect();
    assert_eq!(folders, [&parked_folder("review")]);
    let outcome = core.execute(plan, &[]).unwrap();
    assert_eq!(to_json(outcome.verify), "verified");
    assert_eq!(read_json(&fs, store())["entries"], json!({}));
    assert_eq!(fs.contents(parked_folder(r"review\SKILL.md")), None);
    assert_eq!(stranded(&core), []);

    undo(&core, &outcome.operation);

    assert_eq!(stranded(&core).len(), 2);
    assert!(fs.contents(parked_folder(r"review\SKILL.md")).is_some());
}

#[test]
fn remove_stranded_with_none_stranded_names_no_entry() {
    let fs = desktop();

    let error = core(&fs)
        .plan_remove_stranded(&scan(&core(&fs)))
        .err()
        .unwrap();

    assert_eq!(error.code, ErrorCode::NoMatch);
}

#[test]
fn a_manifest_entry_that_does_not_read_as_a_parked_entry_is_a_read_failure_of_the_store() {
    let fs = store_of_a_gone_agent();
    let mut manifest = read_json(&fs, store());
    manifest["entries"]["broken"] = json!({ "name": 3, "entry": { "token": "not-a-real-secret" } });
    let bytes = serde_json::to_vec_pretty(&manifest).unwrap();
    fs.put_file(store(), bytes.clone());
    let core = core(&fs);

    let inventory = inventory(&core);

    assert_eq!(fs.contents(store()), Some(bytes));
    let failures = to_json(&inventory.store_read_failures);
    assert_eq!(failures.as_array().unwrap().len(), 1, "{failures}");
    assert_eq!(failures[0]["path"], to_json(store()));
    assert_eq!(failures[0]["entry"], "broken");
    assert_eq!(failures[0]["kind"], "shape");
    let sentence = failures[0]["sentence"].as_str().unwrap();
    assert!(sentence.contains("`broken`"), "{sentence}");
    assert!(!sentence.contains("not-a-real-secret"), "{sentence}");
    assert_eq!(stranded(&core).len(), 2);
    core.execute(core.plan_remove_stranded(&scan(&core)).unwrap(), &[])
        .unwrap();
    let entries = &read_json(&fs, store())["entries"];
    assert_eq!(entries.as_object().unwrap().len(), 1, "{entries}");
    assert_eq!(entries["broken"]["name"], 3);
}

#[test]
fn an_unparsable_manifest_is_a_read_failure_of_the_store_left_as_it_is() {
    let fs = store_of_a_gone_agent();
    let broken = b"{ \"entries\": { \"linear\": ".to_vec();
    fs.put_file(store(), broken.clone());
    let core = core(&fs);

    let inventory = inventory(&core);

    let failures = to_json(&inventory.store_read_failures);
    assert_eq!(failures.as_array().unwrap().len(), 1, "{failures}");
    assert_eq!(failures[0]["entry"], Value::Null);
    assert_eq!(failures[0]["kind"], "parse");
    assert!(failures[0]["position"].is_object(), "{failures}");
    assert_eq!(stranded(&core), []);
    assert_eq!(fs.contents(store()), Some(broken));
}

#[test]
fn a_manifest_that_does_not_hold_its_entries_as_an_object_is_a_read_failure_of_the_store() {
    let fs = FakeFileSystem::new().with_file(store(), r#"{ "entries": [] }"#);

    let inventory = inventory(&core(&fs));

    let failures = to_json(&inventory.store_read_failures);
    assert_eq!(failures[0]["entry"], Value::Null);
    assert_eq!(failures[0]["kind"], "shape");
}

#[test]
fn a_manifest_that_cannot_be_opened_is_a_read_failure_of_the_store() {
    let fs = store_of_a_gone_agent().denying_reads_of(store());

    let inventory = inventory(&core(&fs));

    let failures = to_json(&inventory.store_read_failures);
    assert_eq!(failures.as_array().unwrap().len(), 1, "{failures}");
    assert_eq!(failures[0]["entry"], Value::Null);
    assert_eq!(failures[0]["kind"], "permission");
    let sentence = failures[0]["sentence"].as_str().unwrap();
    assert!(sentence.contains("no permission to read"), "{sentence}");
}
