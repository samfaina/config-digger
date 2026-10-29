//! Parking through the core `api`, with fake ports over a fixture Claude
//! Desktop data folder: a server of its user file, which has no switch of
//! its own, is Disabled by moving it into config-digger's parked-Entry
//! store, listed there as Disabled, and put back on Enable (ADR 0002).
//! Extension servers use their own `isEnabled` switch instead.

use config_digger_core::api::{
    Action, Advice, Context, Core, Error, ErrorCode, Inventory, Item, Outcome, Plan, ScanOptions,
    Targets, WarningClass,
};
use config_digger_core::ports::RunningProcess;
use config_digger_core::ports::fake::{
    self, FakeClock, FakeEnvironment, FakeFileSystem, FakeGitIndex, FakeProcesses,
};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::time::Duration;

const HOME: &str = r"C:\Users\fixture";
const APP_DATA: &str = r"C:\Users\fixture\AppData\Roaming";
const LOCAL_APP_DATA: &str = r"C:\Users\fixture\AppData\Local";

/// The Store package's data folder.
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

/// Every file below `folder` on disk, at the same place below `at`.
fn load(fs: FakeFileSystem, folder: &Path, at: &Path) -> FakeFileSystem {
    let mut fs = fs.with_folder(at);
    for child in std::fs::read_dir(folder).unwrap() {
        let child = child.unwrap().path();
        let name = child.file_name().unwrap();
        fs = if child.is_dir() {
            load(fs, &child, &at.join(name))
        } else {
            fs.with_file(at.join(name), std::fs::read(&child).unwrap())
        };
    }
    fs
}

/// The MSIX fixture data folder of the Store package.
fn desktop() -> FakeFileSystem {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR")).join(r"tests\fixtures\claude-desktop\msix");
    load(FakeFileSystem::new(), &fixture, &msix())
}

fn env() -> FakeEnvironment {
    FakeEnvironment::with_home(HOME)
        .with_var("APPDATA", APP_DATA)
        .with_var("LOCALAPPDATA", LOCAL_APP_DATA)
}

fn core(fs: &FakeFileSystem) -> Core {
    Core::new(fake::ports(fs.clone(), env())).unwrap()
}

fn inventory(fs: &FakeFileSystem) -> Inventory {
    let core = core(fs);
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    core.resolve(&snapshot, &Context::default())
}

/// The Claude Desktop Item named `name`.
fn item<'i>(inventory: &'i Inventory, name: &str) -> &'i Item {
    inventory
        .items
        .iter()
        .find(|item| item.agent == "claude-desktop" && item.name == name)
        .unwrap_or_else(|| panic!("no Claude Desktop Item `{name}`"))
}

fn plan_with(core: &Core, action: Action, selector: &str) -> Result<Plan, Error> {
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    let targets = Targets {
        selectors: vec![selector.to_owned()],
        all: false,
        ..Targets::default()
    };
    core.plan(&snapshot, &Context::default(), action, &targets)
}

fn act(fs: &FakeFileSystem, action: Action, selector: &str) -> Result<Outcome, Error> {
    let core = core(fs);
    let plan = plan_with(&core, action, selector)?;
    core.execute(plan, &[])
}

fn read_json(fs: &FakeFileSystem, path: impl AsRef<Path>) -> Value {
    serde_json::from_slice(&fs.contents(path).expect("the file exists")).unwrap()
}

fn to_json(value: impl serde::Serialize) -> Value {
    serde_json::to_value(value).unwrap()
}

#[test]
fn disable_parks_a_user_file_server_in_config_digger_s_store() {
    let fs = desktop();

    act(&fs, Action::Disable, "notes").unwrap();

    let servers = &read_json(&fs, config())["mcpServers"];
    assert_eq!(servers.get("notes"), None);
    assert_eq!(servers["weather"]["command"], "weather-local");
    let parked = &read_json(&fs, store())["entries"]["notes"];
    assert_eq!(parked["agent"], "claude-desktop");
    assert_eq!(parked["home"], "msix");
    assert_eq!(parked["location"], "user-mcp");
    assert_eq!(parked["name"], "notes");
    assert_eq!(parked["path"], to_json(config()));
    assert_eq!(parked["key_path"], json!(["mcpServers", "notes"]));
    assert_eq!(
        parked["entry"],
        json!({ "command": "notes-mcp", "args": ["--vault", r"C:\Users\fixture\notes"] })
    );
}

#[test]
fn a_parked_entry_is_listed_as_disabled_with_reason_parked() {
    let fs = desktop();
    act(&fs, Action::Disable, "notes").unwrap();

    let inventory = inventory(&fs);

    let notes = item(&inventory, "notes");
    assert_eq!(notes.entries.len(), 1);
    let entry = to_json(&notes.entries[0]);
    assert_eq!(entry["state"], "disabled");
    assert_eq!(entry["reason"]["code"], "parked");
    assert_eq!(entry["path"], to_json(store()));
    assert_eq!(entry["provenance"], "file");
    assert_eq!(entry["parked"]["path"], to_json(config()));
    assert_eq!(entry["parked"]["key_path"], json!(["mcpServers", "notes"]));
    assert_eq!(entry["what_if"]["state"], "effective");
    assert_eq!(entry["enable"]["status"], "offered", "{entry}");
    assert!(
        entry["enable"]["sentence"]
            .as_str()
            .unwrap()
            .contains("puts `notes` back"),
        "{entry}"
    );
    assert_eq!(entry["remove"]["status"], "offered", "{entry}");
    assert_eq!(entry["disable"]["code"], "already_disabled");
}

/// The fixture data folder with a server whose env holds a Secret.
fn with_secret() -> FakeFileSystem {
    desktop().with_file(
        config(),
        r#"{
  "mcpServers": {
    "github": {
      "command": "github-mcp",
      "env": { "GITHUB_TOKEN": "ghp_notarealsecret0000000000000000000000" }
    }
  }
}
"#,
    )
}

#[test]
fn the_preview_of_a_park_names_the_entry_where_it_was_and_masks_its_secrets() {
    let fs = with_secret();

    let plan = plan_with(&core(&fs), Action::Disable, "github").unwrap();

    let preview = plan.preview();
    assert_eq!(preview.changes.len(), 1, "{:?}", preview.changes);
    let change = &preview.changes[0];
    assert_eq!(
        change.locator,
        format!("{}#mcpServers.github", config().display())
    );
    assert_eq!(to_json(change.before), "effective");
    assert_eq!(to_json(change.after), "disabled");
    assert!(plan.warnings().is_empty(), "{:?}", plan.warnings());
    let paths: Vec<&PathBuf> = preview.files.iter().map(|file| &file.path).collect();
    assert_eq!(paths, [&config(), &store()]);
    for file in &preview.files {
        let diff = file.diff.as_deref().unwrap_or_else(|| panic!("{file:?}"));
        assert!(!diff.contains("notarealsecret"), "{diff}");
    }
    let stored = preview.files[1].diff.as_deref().unwrap();
    assert!(stored.contains("GITHUB_TOKEN"), "{stored}");
}

fn text(fs: &FakeFileSystem, path: impl AsRef<Path>) -> String {
    String::from_utf8(fs.contents(path).expect("the file exists")).unwrap()
}

/// The fixture user file, as it is on disk.
fn fixture_config() -> String {
    text(&desktop(), config())
}

#[test]
fn enable_puts_a_parked_entry_back_where_it_was() {
    let fs = desktop();
    act(&fs, Action::Disable, "notes").unwrap();

    act(&fs, Action::Enable, "notes").unwrap();

    assert_eq!(text(&fs, config()), fixture_config());
    assert_eq!(read_json(&fs, store())["entries"], json!({}));
    let inventory = inventory(&fs);
    let notes = item(&inventory, "notes");
    assert_eq!(to_json(notes.state), "effective");
    assert_eq!(notes.entries[0].path, config());
}

#[test]
fn enable_refuses_to_overwrite_a_same_named_entry_that_appeared_since() {
    let fs = desktop();
    act(&fs, Action::Disable, "notes").unwrap();
    let appeared = text(&fs, config()).replace(
        r#""mcpServers": {"#,
        r#""mcpServers": {
    "notes": { "command": "other-notes" },"#,
    );
    fs.put_file(config(), appeared.clone());
    let parked = format!("{}#entries.notes", store().display());

    let error = act(&fs, Action::Enable, &parked).unwrap_err();

    assert_eq!(error.code, ErrorCode::Blocked);
    assert!(error.message.contains("overwrite"), "{}", error.message);
    assert_eq!(text(&fs, config()), appeared);
    assert_eq!(read_json(&fs, store())["entries"]["notes"]["name"], "notes");
}

#[test]
fn enable_recreates_a_missing_file_whose_folder_exists() {
    let fs = desktop();
    act(&fs, Action::Disable, "notes").unwrap();
    fs.delete_file(config());

    act(&fs, Action::Enable, "notes").unwrap();

    assert_eq!(
        read_json(&fs, config()),
        json!({ "mcpServers": { "notes": {
            "command": "notes-mcp",
            "args": ["--vault", r"C:\Users\fixture\notes"]
        } } })
    );
}

const PROJECT: &str = r"E:\work\one";

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

/// The Parking Agent's home and Project, which declares `alpha`.
fn parking_fixture() -> FakeFileSystem {
    FakeFileSystem::new()
        .with_file(PathBuf::from(HOME).join(".parking-agent.json"), "{}")
        .with_file(
            project_file(),
            r#"{ "servers": { "alpha": { "command": "a" } } }"#,
        )
}

fn parking_core(fs: &FakeFileSystem, git: FakeGitIndex) -> Core {
    let mut ports = fake::ports(fs.clone(), env());
    ports.git = Box::new(git);
    Core::with_extra_definitions(ports, &[("parking-agent.toml", PARKING_AGENT)]).unwrap()
}

/// Plans `action` on `alpha` in the Parking Agent's Project.
fn plan_in_project(core: &Core, action: Action) -> Result<Plan, Error> {
    let options = ScanOptions {
        projects: vec![PathBuf::from(PROJECT)],
    };
    let snapshot = core.scan(&options).unwrap();
    let context = Context {
        project: Some(PathBuf::from(PROJECT)),
    };
    let targets = Targets {
        selectors: vec!["alpha".to_owned()],
        all: false,
        ..Targets::default()
    };
    core.plan(&snapshot, &context, action, &targets)
}

#[test]
fn enable_refuses_when_the_project_folder_is_gone() {
    let fs = parking_fixture();
    let core = parking_core(&fs, FakeGitIndex::default());
    core.execute(plan_in_project(&core, Action::Disable).unwrap(), &[])
        .unwrap();
    let store_before = text(&fs, store());
    fs.delete_folder(PROJECT);

    let error = plan_in_project(&core, Action::Enable).err().unwrap();

    assert_eq!(error.code, ErrorCode::Blocked);
    assert!(
        error.message.contains("only be Removed"),
        "{}",
        error.message
    );
    assert_eq!(text(&fs, store()), store_before);
}

#[test]
fn parking_never_applies_to_a_git_versioned_entry() {
    let fs = parking_fixture();
    let core = parking_core(&fs, FakeGitIndex::default().with_versioned(project_file()));

    let error = plan_in_project(&core, Action::Disable).err().unwrap();

    assert_eq!(error.code, ErrorCode::Blocked);
    assert!(error.message.contains("git-versioned"), "{}", error.message);
    assert_eq!(fs.contents(store()), None);
}

#[test]
fn remove_of_a_parked_entry_deletes_it_from_the_store() {
    let fs = desktop();
    act(&fs, Action::Disable, "notes").unwrap();
    let parked_config = text(&fs, config());

    act(&fs, Action::Remove, "notes").unwrap();

    assert_eq!(read_json(&fs, store())["entries"], json!({}));
    assert_eq!(text(&fs, config()), parked_config);
    let inventory = inventory(&fs);
    assert!(
        !inventory.items.iter().any(|item| item.name == "notes"),
        "{:?}",
        inventory.items
    );
}

fn undo(fs: &FakeFileSystem, operation: &str) -> Result<Outcome, Error> {
    let core = core(fs);
    let plan = core.plan_undo(operation)?;
    core.execute(plan, &[])
}

#[test]
fn undo_of_a_park_puts_the_entry_back() {
    let fs = desktop();
    let parked = act(&fs, Action::Disable, "notes").unwrap();

    undo(&fs, &parked.operation).unwrap();

    assert_eq!(text(&fs, config()), fixture_config());
    assert_eq!(read_json(&fs, store()), json!({}));
    let inventory = inventory(&fs);
    assert_eq!(to_json(item(&inventory, "notes").state), "effective");
}

#[test]
fn undo_of_a_park_refuses_to_overwrite_a_same_named_entry_that_appeared_since() {
    let fs = desktop();
    let parked = act(&fs, Action::Disable, "notes").unwrap();
    let appeared = text(&fs, config()).replace(
        r#""mcpServers": {"#,
        r#""mcpServers": {
    "notes": { "command": "other-notes" },"#,
    );
    fs.put_file(config(), appeared.clone());

    let error = undo(&fs, &parked.operation).unwrap_err();

    assert_eq!(error.code, ErrorCode::Blocked);
    assert!(error.message.contains("overwrite"), "{}", error.message);
    assert_eq!(text(&fs, config()), appeared);
}

#[test]
fn undo_of_an_enable_parks_the_entry_again() {
    let fs = desktop();
    act(&fs, Action::Disable, "notes").unwrap();
    let parked_config = text(&fs, config());
    let enabled = act(&fs, Action::Enable, "notes").unwrap();

    undo(&fs, &enabled.operation).unwrap();

    assert_eq!(text(&fs, config()), parked_config);
    let inventory = inventory(&fs);
    let notes = &item(&inventory, "notes").entries[0];
    assert_eq!(to_json(notes.reason.as_ref().unwrap().code), "parked");
}

#[test]
fn an_entry_parked_from_a_toml_file_is_removed_from_the_store() {
    let definition = PARKING_AGENT
        .replace(
            r#"path = ".parking-agent.json" }"#,
            r#"path = ".parking-agent.toml" }"#,
        )
        .replace(
            r#"format = { block = "json", dialect = "strict" }"#,
            r#"format = { block = "toml" }"#,
        );
    let toml = PathBuf::from(PROJECT).join(".parking-agent.toml");
    let fs = FakeFileSystem::new()
        .with_file(PathBuf::from(HOME).join(".parking-agent.json"), "{}")
        .with_file(&toml, "[servers.alpha]\ncommand = \"a\"\n");
    let ports = fake::ports(fs.clone(), env());
    let core = Core::with_extra_definitions(ports, &[("parking-agent.toml", &definition)]).unwrap();
    core.execute(plan_in_project(&core, Action::Disable).unwrap(), &[])
        .unwrap();
    assert_eq!(
        read_json(&fs, store())["entries"]["alpha"]["entry"],
        json!({ "command": "a" })
    );

    core.execute(plan_in_project(&core, Action::Remove).unwrap(), &[])
        .unwrap();

    assert_eq!(read_json(&fs, store())["entries"], json!({}));
}

/// The Store package's install folder, as Windows names it.
const PACKAGE: &str = r"C:\Program Files\WindowsApps\Claude_2.16120.0.0_x64__pzs8sxrjxfjjc";

/// A core over `fs` while Claude Desktop runs from its Store package.
fn core_while_desktop_runs(fs: &FakeFileSystem) -> Core {
    core_while_claude_runs(fs, Some(PathBuf::from(PACKAGE).join(r"app\claude.exe")))
}

/// A core over `fs` while a `claude.exe` runs from `path`, `None` when the
/// OS hides it.
fn core_while_claude_runs(fs: &FakeFileSystem, path: Option<PathBuf>) -> Core {
    core_while_claude_runs_with(fs, path, &[""])
}

#[test]
fn parking_is_blocked_while_claude_desktop_runs() {
    let fs = desktop();
    let core = core_while_desktop_runs(&fs);

    let plan = plan_with(&core, Action::Disable, "notes").unwrap();

    assert_parking_blocked(&plan);
    assert!(
        plan.blocks()[0].sentence.contains("Close Claude Desktop"),
        "{}",
        plan.blocks()[0].sentence
    );
    let acks = [WarningClass::RunningAgent, WarningClass::WideReach];
    let error = core.execute(plan, &acks).unwrap_err();
    assert_eq!(error.code, ErrorCode::Blocked);
    assert_eq!(text(&fs, config()), fixture_config());
    assert_eq!(fs.contents(store()), None);
}

#[test]
fn the_parking_block_says_first_to_close_claude_desktop_and_gives_its_file_apart() {
    let fs = desktop();
    let core = core_while_desktop_runs(&fs);

    let plan = plan_with(&core, Action::Disable, "notes").unwrap();

    assert_eq!(
        plan.blocks()[0].advice,
        [Advice {
            todo: "Close Claude Desktop and retry.".to_owned(),
            why: "Claude Desktop is running and rewrites this location from memory, which would undo moving an Entry in or out of it.".to_owned(),
            paths: vec![config().display().to_string()],
        }]
    );
}

#[test]
fn putting_a_parked_entry_back_is_blocked_while_claude_desktop_runs() {
    let fs = desktop();
    act(&fs, Action::Disable, "notes").unwrap();

    let plan = plan_with(&core_while_desktop_runs(&fs), Action::Enable, "notes").unwrap();

    assert_parking_blocked(&plan);
}

/// A Claude Desktop home the user added outside the definition's homes.
const ADDED: &str = r"E:\cd-throwaway\claude-desktop-home";

/// The Store package's data folder and its install folder, an Installation
/// that reads an added home, and a classic data folder at `ADDED`.
fn desktop_and_added() -> FakeFileSystem {
    desktop_and_added_at(ADDED)
}

/// [`desktop_and_added`] with the classic data folder at `added`.
fn desktop_and_added_at(added: &str) -> FakeFileSystem {
    let fixture =
        Path::new(env!("CARGO_MANIFEST_DIR")).join(r"tests\fixtures\claude-desktop\classic");
    let installed = desktop()
        .with_file(
            PathBuf::from(PACKAGE).join("AppxManifest.xml"),
            r#"<?xml version="1.0" encoding="utf-8"?>
<Package xmlns="http://schemas.microsoft.com/appx/manifest/foundation/windows10">
  <Identity Name="Claude" Publisher="CN=Anthropic" Version="2.16120.0.0" ProcessorArchitecture="x64" />
</Package>
"#,
        )
        .with_file(
            PathBuf::from(PACKAGE).join(r"app\claude.exe"),
            fake::executable(Some([2, 16120, 0, 0])),
        );
    load(installed, &fixture, Path::new(added))
}

fn assert_parking_blocked(plan: &Plan) {
    assert_eq!(plan.blocks().len(), 1, "{:?}", plan.blocks());
    assert_eq!(to_json(plan.blocks()[0].code), "running_agent_parking");
}

/// The running Claude Desktop reads no file of the plan: it neither blocks
/// nor warns.
fn assert_desktop_reads_none_of(plan: &Plan) {
    assert_eq!(plan.blocks(), &[], "{:?}", plan.blocks());
    assert!(
        plan.warnings()
            .iter()
            .all(|warning| warning.class != WarningClass::RunningAgent),
        "{:?}",
        plan.warnings()
    );
}

#[test]
fn parking_in_an_added_home_elsewhere_goes_ahead_while_claude_desktop_runs() {
    let fs = desktop_and_added();
    let core = core_while_desktop_runs(&fs);
    core.add_home(Path::new(ADDED), "claude-desktop").unwrap();

    let plan = plan_with(&core, Action::Disable, "filesystem").unwrap();

    assert_desktop_reads_none_of(&plan);
    core.execute(plan, &[WarningClass::WideReach]).unwrap();
    assert_eq!(
        read_json(&fs, Path::new(ADDED).join("claude_desktop_config.json"))["mcpServers"],
        json!({})
    );
}

#[test]
fn a_claude_exe_whose_path_is_hidden_blocks_parking_in_the_definition_s_home_only() {
    let fs = desktop_and_added();
    let core = core_while_claude_runs(&fs, None);
    core.add_home(Path::new(ADDED), "claude-desktop").unwrap();

    let in_definition_home = plan_with(&core, Action::Disable, "notes").unwrap();
    let in_added_home = plan_with(&core, Action::Disable, "filesystem").unwrap();

    assert_parking_blocked(&in_definition_home);
    assert_desktop_reads_none_of(&in_added_home);
}

/// A core over `fs` while each command line runs as a `claude.exe` from
/// `path`, `None` when the OS hides it.
fn core_while_claude_runs_with(
    fs: &FakeFileSystem,
    path: Option<PathBuf>,
    cmdlines: &[&str],
) -> Core {
    let mut ports = fake::ports(
        fs.clone(),
        env().with_var("ProgramFiles", r"C:\Program Files"),
    );
    let mut processes = FakeProcesses::default();
    for (pid, cmdline) in (7000..).zip(cmdlines) {
        processes = processes.with_process(RunningProcess {
            pid,
            image: "claude.exe".to_owned(),
            path: path.clone(),
            cmdline: (*cmdline).to_owned(),
        });
    }
    ports.processes = Box::new(processes);
    Core::new(ports).unwrap()
}

/// The command line of a Chromium child of the Store package's Claude
/// Desktop whose data folder switch is `switch`.
fn desktop_child(switch: &str) -> String {
    format!(
        r#""{PACKAGE}\app\claude.exe" --type=gpu-process {switch} --gpu-preferences=WAAAAAAAAAA --field-trial-handle=1,i,1,2,3"#
    )
}

/// A core over `fs` with `added` added as a Claude Desktop home, while the
/// Store package's Claude Desktop runs one process per command line.
fn added_while_desktop_runs(fs: &FakeFileSystem, added: &str, cmdlines: &[&str]) -> Core {
    let core = core_while_claude_runs_with(
        fs,
        Some(PathBuf::from(PACKAGE).join(r"app\claude.exe")),
        cmdlines,
    );
    core.add_home(Path::new(added), "claude-desktop").unwrap();
    core
}

#[test]
fn parking_in_an_added_home_is_blocked_while_a_claude_desktop_child_names_its_folder() {
    let fs = desktop_and_added();
    let child = desktop_child(&format!("--user-data-dir={ADDED}"));
    let core = added_while_desktop_runs(&fs, ADDED, &[&child]);

    let plan = plan_with(&core, Action::Disable, "filesystem").unwrap();

    assert_parking_blocked(&plan);
}

#[test]
fn a_claude_desktop_child_naming_the_added_folder_in_another_case_blocks_parking() {
    let fs = desktop_and_added();
    let child = desktop_child(&format!("--user-data-dir={}", ADDED.to_uppercase()));
    let core = added_while_desktop_runs(&fs, ADDED, &[&child]);

    assert_parking_blocked(&plan_with(&core, Action::Disable, "filesystem").unwrap());
}

#[test]
fn a_claude_desktop_child_naming_the_added_folder_quoted_blocks_parking() {
    let added = r"E:\cd-throwaway\claude desktop home";
    let fs = desktop_and_added_at(added);
    for switch in [
        format!(r#"--user-data-dir="{added}""#),
        format!(r#""--user-data-dir={added}""#),
    ] {
        let child = desktop_child(&switch);
        let core = added_while_desktop_runs(&fs, added, &[&child]);

        let plan = plan_with(&core, Action::Disable, "filesystem").unwrap();

        assert_parking_blocked(&plan);
    }
}

#[test]
fn a_remove_in_an_added_home_warns_while_a_claude_desktop_child_names_its_folder() {
    let fs = desktop_and_added();
    let child = desktop_child(&format!("--user-data-dir={ADDED}"));
    let core = added_while_desktop_runs(&fs, ADDED, &[&child]);

    let plan = plan_with(&core, Action::Remove, "filesystem").unwrap();

    assert_eq!(plan.blocks(), &[], "{:?}", plan.blocks());
    let warning = plan
        .warnings()
        .iter()
        .find(|warning| warning.class == WarningClass::RunningAgent)
        .unwrap_or_else(|| panic!("no running-agent warning: {:?}", plan.warnings()));
    let file = Path::new(ADDED).join("claude_desktop_config.json");
    assert_eq!(
        warning.advice[0].paths,
        [file.display().to_string()],
        "{warning:?}"
    );
}

#[test]
fn a_claude_desktop_child_naming_the_added_folder_with_a_hidden_path_is_judged_when_listed_again() {
    let fs = desktop_and_added();
    let child = RunningProcess {
        pid: 7000,
        image: "claude.exe".to_owned(),
        path: None,
        cmdline: desktop_child(&format!("--user-data-dir={ADDED}")),
    };
    let core_listing_again = |again: Vec<RunningProcess>| {
        let mut ports = fake::ports(
            fs.clone(),
            env().with_var("ProgramFiles", r"C:\Program Files"),
        );
        ports.processes = Box::new(
            FakeProcesses::default()
                .with_process(child.clone())
                .then_running(again),
        );
        let core = Core::new(ports).unwrap();
        core.add_home(Path::new(ADDED), "claude-desktop").unwrap();
        core
    };

    let gone = core_listing_again(vec![]);
    assert_desktop_reads_none_of(&plan_with(&gone, Action::Disable, "filesystem").unwrap());

    let elsewhere = RunningProcess {
        path: Some(PathBuf::from(r"D:\tools\claude.exe")),
        ..child.clone()
    };
    let read_elsewhere = core_listing_again(vec![elsewhere]);
    assert_desktop_reads_none_of(
        &plan_with(&read_elsewhere, Action::Disable, "filesystem").unwrap(),
    );

    let in_its_package = RunningProcess {
        path: Some(PathBuf::from(PACKAGE).join(r"app\claude.exe")),
        ..child.clone()
    };
    let read_in_its_package = core_listing_again(vec![in_its_package]);
    assert_parking_blocked(
        &plan_with(&read_in_its_package, Action::Disable, "filesystem").unwrap(),
    );

    let still_hidden = core_listing_again(vec![child.clone()]);
    assert_parking_blocked(&plan_with(&still_hidden, Action::Disable, "filesystem").unwrap());
}

#[test]
fn a_claude_desktop_child_naming_another_folder_does_not_block_parking_in_an_added_home() {
    let fs = desktop_and_added();
    let child = desktop_child(r"--user-data-dir=E:\cd-throwaway\claude-desktop-home-2");
    let core = added_while_desktop_runs(&fs, ADDED, &[&child]);

    assert_desktop_reads_none_of(&plan_with(&core, Action::Disable, "filesystem").unwrap());
}

#[test]
fn a_claude_desktop_whose_command_line_is_hidden_does_not_block_parking_in_an_added_home() {
    let fs = desktop_and_added();
    let core = added_while_desktop_runs(&fs, ADDED, &[""]);

    assert_desktop_reads_none_of(&plan_with(&core, Action::Disable, "filesystem").unwrap());
}

#[test]
fn a_claude_code_cli_naming_the_added_folder_does_not_block_parking_in_it() {
    let fs = desktop_and_added();
    let cli = PathBuf::from(HOME).join(r".local\bin\claude.exe");
    let cmdline = format!(r#""{}" --user-data-dir={ADDED}"#, cli.display());
    let core = core_while_claude_runs_with(&fs, Some(cli), &[&cmdline]);
    core.add_home(Path::new(ADDED), "claude-desktop").unwrap();

    assert_desktop_reads_none_of(&plan_with(&core, Action::Disable, "filesystem").unwrap());
}

// An added home in the folder of a definition's home is that home, so
// `notes` names one Entry.

#[test]
fn parking_in_an_added_home_in_another_case_of_a_definition_s_home_is_blocked() {
    let fs = desktop();
    let core = core_while_desktop_runs(&fs);
    let spelled = msix().display().to_string().to_uppercase();
    core.add_home(Path::new(&spelled), "claude-desktop")
        .unwrap();

    assert_parking_blocked(&plan_with(&core, Action::Disable, "notes").unwrap());
}

#[test]
fn parking_in_an_added_home_linked_to_a_definition_s_home_is_blocked() {
    let link = r"E:\cd-throwaway\desktop-link";
    let fs = desktop().with_link(link, msix());
    let core = core_while_desktop_runs(&fs);
    core.add_home(Path::new(link), "claude-desktop").unwrap();

    assert_parking_blocked(&plan_with(&core, Action::Disable, "notes").unwrap());
}

/// An Agent that picks no home by lookup order, whose servers in its home
/// file have no switch, and which rewrites that file from memory.
const HOME_PARKING_AGENT: &str = r#"
format_version = 1

[agent]
id = "home-parking-agent"
name = "Home Parking Agent"

[process]
images = ["home-parking-agent.exe"]

[[homes]]
id = "default"
folder = { block = "path", path = "~" }
roots.state = ".home-parking-agent.json"

[[scopes]]
id = "user"

[[locations]]
id = "user-mcp"
anchor = { block = "home", root = "state" }
format = { block = "json", dialect = "strict" }
root = ["servers"]
kind = "mcp"
scope = "user"
provenance = "file"
store_class = "rewritten-while-running"
on_read_failure = "ignores_file"

[items.mcp]
precedence = ["user-mcp"]
competition = { block = "exact-name" }
checks = ["disable", "precedence"]

[[items.mcp.disable]]
block = "park"
locations = ["user-mcp"]
"#;

#[test]
fn parking_in_an_added_home_of_an_agent_without_lookup_orders_is_blocked_while_it_runs() {
    let added = r"E:\cd-throwaway\home-parking-agent-home";
    let fs = FakeFileSystem::new()
        .with_file(PathBuf::from(HOME).join(".home-parking-agent.json"), "{}")
        .with_file(
            Path::new(added).join(".home-parking-agent.json"),
            r#"{ "servers": { "alpha": { "command": "a" } } }"#,
        );
    let mut ports = fake::ports(fs.clone(), env());
    ports.processes = Box::new(FakeProcesses::default().with_process(RunningProcess {
        pid: 6161,
        image: "home-parking-agent.exe".to_owned(),
        path: None,
        cmdline: String::new(),
    }));
    let core =
        Core::with_extra_definitions(ports, &[("home-parking-agent.toml", HOME_PARKING_AGENT)])
            .unwrap();
    core.add_home(Path::new(added), "home-parking-agent")
        .unwrap();

    assert_parking_blocked(&plan_with(&core, Action::Disable, "alpha").unwrap());
}

#[test]
fn verify_fails_a_park_when_the_entry_is_back_in_its_file() {
    // Claude Desktop writes its file back from memory, with the server.
    let fs = desktop().agent_rewrites_after_next_write(config(), fixture_config());

    let error = act(&fs, Action::Disable, "notes").unwrap_err();

    assert_eq!(error.code, ErrorCode::OverwrittenByAgent);
    assert_eq!(text(&fs, config()), fixture_config());
    assert_eq!(
        fs.contents(store()),
        None,
        "the store it created is rolled back"
    );
}

#[test]
fn verify_fails_a_park_when_the_entry_is_not_in_the_store() {
    let fs = desktop()
        .corrupting_writes_below(PathBuf::from(LOCAL_APP_DATA).join(r"config-digger\parked"));

    let error = act(&fs, Action::Disable, "notes").unwrap_err();

    // The store holds other bytes than those written, without the Entry,
    // so the Rollback leaves it for manual recovery.
    assert_eq!(error.code, ErrorCode::RollbackIncomplete);
    let failed = error.failed.as_ref().unwrap();
    assert_eq!(failed.failure, Some(ErrorCode::OverwrittenByAgent));
    let store_text = store().display().to_string();
    assert!(error.message.contains(&store_text), "{}", error.message);
    assert_eq!(text(&fs, config()), fixture_config());
}

#[test]
fn verify_fails_an_enable_when_the_entry_is_not_back_in_its_file() {
    let fs = desktop();
    act(&fs, Action::Disable, "notes").unwrap();
    let parked_config = text(&fs, config());
    let store_before = text(&fs, store());
    let fs = fs.agent_rewrites_after_next_write(config(), parked_config.clone());

    let error = act(&fs, Action::Enable, "notes").unwrap_err();

    assert_eq!(error.code, ErrorCode::OverwrittenByAgent);
    assert_eq!(text(&fs, config()), parked_config);
    assert_eq!(text(&fs, store()), store_before);
}

/// An extension's settings file in the Store package's data folder.
fn extension_settings(id: &str) -> PathBuf {
    msix()
        .join("Claude Extensions Settings")
        .join(format!("{id}.json"))
}

#[test]
fn extension_disable_switches_it_off_in_its_settings() {
    let fs = desktop();

    act(&fs, Action::Disable, "Files").unwrap();

    let settings = read_json(&fs, extension_settings("local.mcpb.acme.files"));
    assert_eq!(settings["isEnabled"], false);
    assert_eq!(
        settings["userConfig"]["root"],
        r"C:\Users\fixture\Documents"
    );
    assert_eq!(fs.contents(store()), None, "an extension is never parked");
    let inventory = inventory(&fs);
    let files = &item(&inventory, "Files").entries[0];
    assert_eq!(
        to_json(files.reason.as_ref().unwrap().code),
        "disabled_natively"
    );
}

#[test]
fn extension_enable_switches_it_on_in_its_settings() {
    let fs = desktop();

    act(&fs, Action::Enable, "Todo").unwrap();

    let settings = read_json(&fs, extension_settings("local.mcpb.acme.todo"));
    assert_eq!(settings, json!({ "isEnabled": true }));
    assert_eq!(to_json(item(&inventory(&fs), "Todo").state), "effective");
}

#[test]
fn extension_enable_creates_its_missing_settings_file() {
    let fs = desktop();

    act(&fs, Action::Enable, "Calendar").unwrap();

    let settings = read_json(&fs, extension_settings("local.dxt.acme.calendar"));
    assert_eq!(settings, json!({ "isEnabled": true }));
}

#[test]
fn pruning_history_never_prunes_the_parked_entry_store() {
    let fs = desktop();
    let clock = FakeClock::default();
    let core_at = |clock: &FakeClock| {
        let mut ports = fake::ports(fs.clone(), env());
        ports.clock = Box::new(clock.clone());
        Core::new(ports).unwrap()
    };
    let run = |action: Action, selector: &str| {
        let core = core_at(&clock);
        let plan = plan_with(&core, action, selector).unwrap();
        core.execute(plan, &[]).unwrap().operation
    };
    let parked = run(Action::Disable, "notes");
    for round in 0..51 {
        clock.advance(Duration::from_secs(60));
        let action = if round % 2 == 0 {
            Action::Disable
        } else {
            Action::Enable
        };
        run(action, "Files");
    }
    let store_before = text(&fs, store());
    clock.advance(Duration::from_secs(40 * 24 * 60 * 60));

    let pruned = core_at(&clock).prune_history().unwrap();

    assert_eq!(pruned.first(), Some(&parked));
    assert_eq!(text(&fs, store()), store_before);
    let inventory = inventory(&fs);
    let notes = &item(&inventory, "notes").entries[0];
    assert_eq!(to_json(notes.reason.as_ref().unwrap().code), "parked");
}

#[test]
fn a_parked_entry_s_secrets_are_masked_in_the_inventory() {
    let fs = with_secret();
    act(&fs, Action::Disable, "github").unwrap();

    let inventory = inventory(&fs);

    let github = &item(&inventory, "github").entries[0];
    let token = github.config["env"]["GITHUB_TOKEN"].as_str().unwrap();
    assert!(!token.contains("notarealsecret"), "{token}");
    assert_eq!(github.masked, [["env", "GITHUB_TOKEN"]]);
}

#[test]
fn disable_of_every_entry_of_an_item_parks_one_and_switches_the_extension_off() {
    let fs = desktop();
    let core = core(&fs);
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    let targets = Targets {
        selectors: vec!["weather".to_owned()],
        all: true,
        ..Targets::default()
    };
    let plan = core
        .plan(&snapshot, &Context::default(), Action::Disable, &targets)
        .unwrap();

    core.execute(plan, &[]).unwrap();

    assert_eq!(read_json(&fs, config())["mcpServers"].get("weather"), None);
    assert_eq!(
        read_json(&fs, store())["entries"]["weather"]["name"],
        "weather"
    );
    let settings = read_json(&fs, extension_settings("local.mcpb.acme.weather"));
    assert_eq!(settings["isEnabled"], false);
    let inventory = inventory(&fs);
    let states: Vec<Value> = item(&inventory, "weather")
        .entries
        .iter()
        .map(|entry| to_json(entry.reason.as_ref().unwrap().code))
        .collect();
    assert_eq!(states, ["parked", "disabled_natively"]);
}

#[test]
fn an_entry_whose_switch_cannot_be_read_is_not_parked_instead() {
    let definition = PARKING_AGENT.replace(
        "[[items.mcp.disable]]\nblock = \"park\"",
        r#"[[items.mcp.disable]]
block = "json-field"
switch = { anchor = { block = "home", root = "state" }, key_path = ["off", "{name}"], store_class = "plain" }
target = "entry"
context = "all"
enable_action = "delete-key"
disabled_value = true
reason = "disabled_natively"

[[items.mcp.disable]]
block = "park""#,
    );
    let fs = parking_fixture().with_file(
        PathBuf::from(HOME).join(".parking-agent.json"),
        "{ not json",
    );
    let ports = fake::ports(fs.clone(), env());
    let core = Core::with_extra_definitions(ports, &[("parking-agent.toml", &definition)]).unwrap();

    let error = plan_in_project(&core, Action::Disable).err().unwrap();

    assert_eq!(error.code, ErrorCode::Blocked);
    assert!(error.message.contains("cannot read"), "{}", error.message);
    assert_eq!(fs.contents(store()), None);
}

#[test]
fn enable_refuses_when_the_folder_of_the_file_is_gone() {
    let definition = PARKING_AGENT.replace(
        r#"path = ".parking-agent.json" }"#,
        r#"path = ".sub/parking-agent.json" }"#,
    );
    let folder = PathBuf::from(PROJECT).join(".sub");
    let fs = FakeFileSystem::new()
        .with_folder(PROJECT)
        .with_file(PathBuf::from(HOME).join(".parking-agent.json"), "{}")
        .with_file(
            folder.join("parking-agent.json"),
            r#"{ "servers": { "alpha": { "command": "a" } } }"#,
        );
    let ports = fake::ports(fs.clone(), env());
    let core = Core::with_extra_definitions(ports, &[("parking-agent.toml", &definition)]).unwrap();
    core.execute(plan_in_project(&core, Action::Disable).unwrap(), &[])
        .unwrap();
    let store_before = text(&fs, store());
    fs.delete_folder(&folder);

    let error = plan_in_project(&core, Action::Enable).err().unwrap();

    assert_eq!(error.code, ErrorCode::Blocked);
    assert!(error.message.contains("is gone"), "{}", error.message);
    assert_eq!(text(&fs, store()), store_before);
}
