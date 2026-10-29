//! Installations and tested version ranges through the core `api`, with fake
//! version files: each Installation that reads an Agent home and its
//! version, read from files at every scan and once per plan, and the Agent
//! notes when a version is outside the definition's tested range, cannot be
//! read, or there is nothing to compare.

use config_digger_core::api::{
    Action, Agent, AgentNoteCode, Context, Core, Home, InfoCode, Plan, ScanOptions, Targets,
    WarningClass,
};
use config_digger_core::ports::fake::{self, FakeEnvironment, FakeFileSystem, FakeProcesses};
use config_digger_core::ports::{Hive, RunningProcess};
use std::path::{Path, PathBuf};

const HOME: &str = r"C:\Users\fixture";

/// A user `notes` in `~/.claude.json`.
const CLAUDE_JSON: &str = r#"{
  "mcpServers": {
    "notes": { "command": "notes-mcp" }
  }
}
"#;

fn native_claude() -> PathBuf {
    PathBuf::from(HOME).join(r".local\bin\claude.exe")
}

fn fixture() -> FakeFileSystem {
    FakeFileSystem::new().with_file(PathBuf::from(HOME).join(".claude.json"), CLAUDE_JSON)
}

fn core(fs: &FakeFileSystem) -> Core {
    Core::new(fake::ports(fs.clone(), FakeEnvironment::with_home(HOME))).unwrap()
}

/// Every loaded Agent, with its homes, as a fresh scan finds them.
fn agents(core: &Core) -> Vec<Agent> {
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    core.agents(&core.resolve(&snapshot, &Context::default()))
}

/// The one home of the Agent `id`, with only the notes about its
/// Installations: the note that its connectors are not on disk, which
/// every Claude Code home carries, is left out.
fn home(core: &Core, id: &str) -> Home {
    let agents = agents(core);
    let agent = agents.iter().find(|agent| agent.id == id).unwrap();
    let [home] = agent.homes.as_slice() else {
        panic!("one home: {:?}", agent.homes);
    };
    let mut home = home.clone();
    home.notes
        .retain(|note| note.code != AgentNoteCode::CloudConnectorsNotOnDisk);
    home
}

#[test]
fn the_native_installation_shows_the_version_of_its_executable() {
    let fs = fixture().with_file(native_claude(), fake::executable(Some([2, 1, 299, 0])));

    let home = home(&core(&fs), "claude-code");

    let [installation] = home.installations.as_slice() else {
        panic!("one Installation: {:?}", home.installations);
    };
    assert_eq!(installation.path, native_claude());
    assert_eq!(installation.version.as_deref(), Some("2.1.299"));
    assert!(!installation.out_of_range);
    assert!(!home.out_of_range);
    assert_eq!(home.notes, []);
}

fn note_codes(home: &Home) -> Vec<AgentNoteCode> {
    home.notes.iter().map(|note| note.code).collect()
}

#[test]
fn a_newer_version_is_noted_as_outside_the_tested_range() {
    let fs = fixture().with_file(native_claude(), fake::executable(Some([2, 1, 310, 0])));

    let home = home(&core(&fs), "claude-code");

    assert!(home.out_of_range);
    assert!(home.installations[0].out_of_range);
    let [note] = home.notes.as_slice() else {
        panic!("one note: {:?}", home.notes);
    };
    assert_eq!(note.code, AgentNoteCode::VersionOutsideTestedRange);
    assert_eq!(
        note.installation.as_deref(),
        Some(native_claude().as_path())
    );
    assert_eq!(
        note.sentence,
        "Claude Code 2.1.310 is newer than the versions this definition was tested with (2.1.299 to 2.1.309)."
    );
}

#[test]
fn an_older_version_is_treated_like_a_newer_one() {
    let fs = fixture().with_file(native_claude(), fake::executable(Some([2, 0, 9, 0])));

    let home = home(&core(&fs), "claude-code");

    assert!(home.installations[0].out_of_range);
    assert_eq!(
        home.notes[0].sentence,
        "Claude Code 2.0.9 is older than the versions this definition was tested with (2.1.299 to 2.1.309)."
    );
}

#[test]
fn the_version_is_read_again_at_every_scan() {
    let fs = fixture().with_file(native_claude(), fake::executable(Some([2, 1, 299, 0])));
    let core = core(&fs);
    assert_eq!(note_codes(&home(&core, "claude-code")), []);

    fs.put_file(native_claude(), fake::executable(Some([2, 1, 311, 0])));

    let home = home(&core, "claude-code");
    assert_eq!(home.installations[0].version.as_deref(), Some("2.1.311"));
    assert_eq!(
        note_codes(&home),
        [AgentNoteCode::VersionOutsideTestedRange]
    );
}

#[test]
fn an_executable_without_a_version_resource_is_noted_as_unreadable() {
    let fs = fixture().with_file(native_claude(), fake::executable(None));

    let home = home(&core(&fs), "claude-code");

    let [installation] = home.installations.as_slice() else {
        panic!("one Installation: {:?}", home.installations);
    };
    assert_eq!(installation.version, None);
    assert!(!installation.out_of_range);
    assert_eq!(note_codes(&home), [AgentNoteCode::VersionUnreadable]);
    assert!(
        home.notes[0]
            .sentence
            .contains(&native_claude().display().to_string()),
        "{}",
        home.notes[0].sentence
    );
}

#[test]
fn a_file_that_is_not_an_executable_has_no_version() {
    let fs = fixture().with_file(native_claude(), "#!/bin/sh\n");

    let home = home(&core(&fs), "claude-code");

    assert_eq!(home.installations[0].version, None);
    assert_eq!(note_codes(&home), [AgentNoteCode::VersionUnreadable]);
}

#[test]
fn a_home_with_no_installation_found_says_so() {
    let home = home(&core(&fixture()), "claude-code");

    assert_eq!(home.installations, []);
    assert_eq!(note_codes(&home), [AgentNoteCode::NoInstallationFound]);
    assert_eq!(
        home.notes[0].sentence,
        "No installation of Claude Code that reads this Agent home was found, so its version could not be checked."
    );
}

#[test]
fn the_agent_lists_its_tested_range() {
    let agents = agents(&core(&fixture()));

    let tested = agents[0].tested.as_ref().expect("a tested range");
    assert_eq!(
        (tested.min.as_str(), tested.max.as_str()),
        ("2.1.299", "2.1.309")
    );
}

#[test]
fn the_version_that_last_ran_against_the_home_is_shown_beside_the_installations() {
    let fs = FakeFileSystem::new()
        .with_file(
            PathBuf::from(HOME).join(".claude.json"),
            r#"{ "lastReleaseNotesSeen": "2.1.284", "mcpServers": {} }"#,
        )
        .with_file(native_claude(), fake::executable(Some([2, 1, 299, 0])));

    let home = home(&core(&fs), "claude-code");

    assert_eq!(home.last_ran.as_deref(), Some("2.1.284"));
    assert_eq!(note_codes(&home), []);
}

const APP_DATA: &str = r"C:\Users\fixture\AppData\Roaming";
const LOCAL_APP_DATA: &str = r"C:\Users\fixture\AppData\Local";

fn core_with_app_data(fs: &FakeFileSystem) -> Core {
    let env = FakeEnvironment::with_home(HOME)
        .with_var("APPDATA", APP_DATA)
        .with_var("LOCALAPPDATA", LOCAL_APP_DATA);
    Core::new(fake::ports(fs.clone(), env)).unwrap()
}

#[test]
fn every_installation_that_reads_the_home_is_checked_and_any_out_of_range_flags_it() {
    let desktop = PathBuf::from(APP_DATA).join(r"Claude\claude-code\2.1.284\claude.exe");
    let msix = PathBuf::from(LOCAL_APP_DATA).join(
        r"Packages\Claude_pzs8sxrjxfjjc\LocalCache\Roaming\Claude\claude-code\2.1.299\635c1867224a\claude.exe",
    );
    let fs = fixture()
        .with_file(native_claude(), fake::executable(Some([2, 1, 299, 0])))
        .with_file(&desktop, fake::executable(Some([2, 1, 284, 0])))
        .with_file(&msix, fake::executable(Some([2, 1, 299, 0])));

    let home = home(&core_with_app_data(&fs), "claude-code");

    assert!(home.out_of_range, "one of them is out of range");
    let installed: Vec<(&str, Option<&str>, bool)> = home
        .installations
        .iter()
        .map(|installation| {
            (
                installation.id.as_str(),
                installation.version.as_deref(),
                installation.out_of_range,
            )
        })
        .collect();
    assert_eq!(
        installed,
        [
            ("native", Some("2.1.299"), false),
            ("claude-desktop", Some("2.1.284"), true),
            ("claude-desktop-msix", Some("2.1.299"), false),
        ]
    );
    let [note] = home.notes.as_slice() else {
        panic!("one note: {:?}", home.notes);
    };
    assert_eq!(note.installation.as_deref(), Some(desktop.as_path()));
}

#[test]
fn claude_desktop_s_copy_is_found_in_a_package_s_moved_data_folder() {
    let moved = PathBuf::from(LOCAL_APP_DATA)
        .join(r"Claude-Data\claude_pzs8sxrjxfjjc\claude-code\2.1.299\635c1867224a\claude.exe");
    let fs = fixture()
        .with_file(native_claude(), fake::executable(Some([2, 1, 299, 0])))
        .with_file(&moved, fake::executable(Some([2, 1, 299, 0])));

    let home = home(&core_with_app_data(&fs), "claude-code");

    let installed: Vec<(&str, &Path)> = home
        .installations
        .iter()
        .map(|installation| (installation.id.as_str(), installation.path.as_path()))
        .collect();
    assert_eq!(
        installed,
        [
            ("native", native_claude().as_path()),
            ("claude-desktop-msix", moved.as_path()),
        ]
    );
}

fn remove_notes(core: &Core) -> Plan {
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    let targets = Targets {
        selectors: vec!["notes".to_owned()],
        all: false,
        ..Targets::default()
    };
    core.plan(&snapshot, &Context::default(), Action::Remove, &targets)
        .unwrap()
}

#[test]
fn an_operation_on_the_agent_gets_the_note_as_an_informational_line_and_still_applies() {
    let fs = fixture().with_file(native_claude(), fake::executable(Some([2, 1, 310, 0])));
    let core = core(&fs);

    let plan = remove_notes(&core);

    let info: Vec<(InfoCode, &str)> = plan
        .preview()
        .info
        .iter()
        .map(|info| (info.code, info.sentence.as_str()))
        .collect();
    assert_eq!(
        info,
        [(
            InfoCode::VersionOutsideTestedRange,
            "Claude Code 2.1.310 is newer than the versions this definition was tested with (2.1.299 to 2.1.309)."
        )]
    );
    assert_eq!(plan.preview().info[0].path, native_claude());
    assert_eq!(plan.warnings(), []);
    assert_eq!(plan.blocks(), []);
    core.execute(plan, &[]).unwrap();
}

#[test]
fn every_note_of_the_agent_home_becomes_an_informational_line() {
    let plan = remove_notes(&core(&fixture()));

    let codes: Vec<InfoCode> = plan.preview().info.iter().map(|info| info.code).collect();
    assert_eq!(codes, [InfoCode::NoInstallationFound]);
    assert_eq!(
        plan.preview().info[0].path,
        PathBuf::from(HOME),
        "a note about no Installation is about the Agent home"
    );
}

#[test]
fn an_operation_on_an_agent_within_its_tested_range_has_no_version_line() {
    let fs = fixture().with_file(native_claude(), fake::executable(Some([2, 1, 299, 0])));

    let plan = remove_notes(&core(&fs));

    assert_eq!(plan.preview().info, []);
}

#[test]
fn an_undo_of_an_operation_on_the_agent_gets_the_line_too() {
    let fs = fixture().with_file(native_claude(), fake::executable(Some([2, 1, 310, 0])));
    let core = core(&fs);
    let outcome = core.execute(remove_notes(&core), &[]).unwrap();

    let undo = core.plan_undo(&outcome.operation).unwrap();

    let codes: Vec<InfoCode> = undo.preview().info.iter().map(|info| info.code).collect();
    assert_eq!(codes, [InfoCode::VersionOutsideTestedRange]);
}

/// How many times `fs` reads the native executable while `run` runs.
fn executable_reads(fs: &FakeFileSystem, run: impl FnOnce()) -> usize {
    let before = fs.reads_of(native_claude());
    run();
    fs.reads_of(native_claude()) - before
}

#[test]
fn an_undo_reads_the_versions_once_though_it_scans_twice() {
    let fs = fixture().with_file(native_claude(), fake::executable(Some([2, 1, 299, 0])));
    let core = core(&fs);
    let one_scan = executable_reads(&fs, || {
        core.scan(&ScanOptions::default()).unwrap();
    });
    let outcome = core.execute(remove_notes(&core), &[]).unwrap();

    let undo = executable_reads(&fs, || {
        core.plan_undo(&outcome.operation).unwrap();
    });

    assert!(one_scan > 0);
    assert_eq!(undo, one_scan);
}

#[test]
fn a_plan_predicting_with_another_scan_reads_no_version_again() {
    let project = PathBuf::from(r"E:\projects\app");
    let fs = FakeFileSystem::new()
        .with_file(
            PathBuf::from(HOME).join(".claude.json"),
            r#"{
  "mcpServers": { "notes": { "command": "notes-mcp" } },
  "projects": { "E:/projects/app": {} }
}
"#,
        )
        .with_folder(&project)
        .with_file(native_claude(), fake::executable(Some([2, 1, 299, 0])));
    let core = core(&fs);
    let snapshot = core
        .scan(&ScanOptions {
            projects: vec![project.clone()],
        })
        .unwrap();
    let context = Context {
        project: Some(project),
    };
    let targets = Targets {
        selectors: vec!["notes".to_owned()],
        all: false,
        ..Targets::default()
    };

    let reads = executable_reads(&fs, || {
        core.plan(&snapshot, &context, Action::Disable, &targets)
            .unwrap();
    });

    assert_eq!(reads, 0);
}

#[test]
fn each_of_two_scans_reads_the_versions() {
    let fs = fixture().with_file(native_claude(), fake::executable(Some([2, 1, 299, 0])));
    let core = core(&fs);
    let scan = || {
        core.scan(&ScanOptions::default()).unwrap();
    };
    let first = executable_reads(&fs, scan);

    let second = executable_reads(&fs, scan);

    assert!(first > 0);
    assert_eq!(second, first);
}

/// Claude Code's session file naming the native CLI's PID, and the CLI
/// running from `~/.local/bin`.
fn claude_code_running(fs: &FakeFileSystem) -> Core {
    let mut ports = fake::ports(fs.clone(), FakeEnvironment::with_home(HOME));
    ports.processes = Box::new(FakeProcesses::default().with_process(RunningProcess {
        pid: 4242,
        image: "claude.exe".to_owned(),
        path: Some(native_claude()),
        cmdline: native_claude().display().to_string(),
    }));
    Core::new(ports).unwrap()
}

#[test]
fn with_no_installation_found_the_running_agent_warning_never_fires() {
    let fs = fixture();

    let plan = remove_notes(&claude_code_running(&fs));

    assert_eq!(plan.warnings(), []);
}

#[test]
fn with_an_installation_found_the_running_agent_warning_fires() {
    let fs = fixture().with_file(native_claude(), fake::executable(Some([2, 1, 299, 0])));

    let plan = remove_notes(&claude_code_running(&fs));

    let classes: Vec<WarningClass> = plan.warnings().iter().map(|w| w.class).collect();
    assert_eq!(classes, [WarningClass::RunningAgent]);
}

#[test]
fn an_npm_installation_reads_its_package_json() {
    let npm = PathBuf::from(APP_DATA).join("npm");
    let fs = fixture()
        .with_file(npm.join("claude.cmd"), "@node cli.js %*\r\n")
        .with_file(
            npm.join(r"node_modules\@anthropic-ai\claude-code\package.json"),
            r#"{ "name": "@anthropic-ai/claude-code", "version": "2.1.299" }"#,
        );

    let home = home(&core_with_app_data(&fs), "claude-code");

    let [installation] = home.installations.as_slice() else {
        panic!("one Installation: {:?}", home.installations);
    };
    assert_eq!(
        (installation.id.as_str(), installation.version.as_deref()),
        ("npm", Some("2.1.299"))
    );
    assert_eq!(note_codes(&home), []);
}

/// A test Agent with two homes, `~/.test` and `~/.test-insiders`, tested
/// from 1.2 to 1.4.0, whose `[[installations]]` are `installations`.
fn test_agent(installations: &str) -> String {
    format!(
        r#"
format_version = 1
verified = {{ min = "1.2", max = "1.4.0" }}

[agent]
id = "test-agent"
name = "Test Agent"

{installations}

[[homes]]
id = "default"
folder = {{ block = "path", path = "~" }}
roots.state = ".test/state.json"

[[homes]]
id = "insiders"
folder = {{ block = "path", path = "~" }}
roots.state = ".test-insiders/state.json"

[[scopes]]
id = "user"

[[locations]]
id = "user-mcp"
anchor = {{ block = "home", root = "state" }}
format = {{ block = "json", dialect = "strict" }}
root = ["servers"]
kind = "mcp"
scope = "user"
provenance = "file"
store_class = "plain"
on_read_failure = "ignores_file"

[items.mcp]
precedence = ["user-mcp"]
competition = {{ block = "exact-name" }}
checks = ["disable", "precedence"]
"#
    )
}

const PROGRAMS: &str = r"C:\Users\fixture\AppData\Local\Programs";

/// The test Agent's homes, each with a state file, plus `files`.
fn test_fixture(files: &[(&str, Vec<u8>)]) -> FakeFileSystem {
    let fs = FakeFileSystem::new()
        .with_file(PathBuf::from(HOME).join(r".test\state.json"), "{}")
        .with_file(PathBuf::from(HOME).join(r".test-insiders\state.json"), "{}");
    for (path, contents) in files {
        fs.put_file(*path, contents.clone());
    }
    fs
}

/// A core with the test Agent and its `installations` loaded.
fn test_core(fs: &FakeFileSystem, env: FakeEnvironment, definition: &str) -> Core {
    let ports = fake::ports(fs.clone(), env.with_var("LOCALAPPDATA", LOCAL_APP_DATA));
    Core::with_extra_definitions(ports, &[("test.toml", definition)]).unwrap()
}

/// The test Agent, as a core with its `installations` finds it.
fn test_agent_found(fs: &FakeFileSystem, env: FakeEnvironment, installations: &str) -> Agent {
    let core = test_core(fs, env, &test_agent(installations));
    agents(&core)
        .into_iter()
        .find(|agent| agent.id == "test-agent")
        .unwrap()
}

fn versions(home: &Home) -> Vec<(String, Option<String>, Option<String>)> {
    home.installations
        .iter()
        .map(|installation| {
            (
                installation.id.clone(),
                installation.version.clone(),
                installation.version_from.clone(),
            )
        })
        .collect()
}

#[test]
fn a_json_value_reads_a_product_json_through_a_glob() {
    let product = format!(r"{PROGRAMS}\test\8a7abeba6e\resources\app\product.json");
    let fs = test_fixture(&[
        (&format!(r"{PROGRAMS}\test\Test.exe"), b"MZ".to_vec()),
        (
            &product,
            br#"{ "version": "1.3.0", "quality": "stable" }"#.to_vec(),
        ),
    ]);

    let agent = test_agent_found(
        &fs,
        FakeEnvironment::with_home(HOME),
        r#"[[installations]]
id = "user-setup"
folder = "%LOCALAPPDATA%/Programs/test"
executable = "Test.exe"
versions = [{ block = "exe-version" }, { block = "json-value", file = "*/resources/app/product.json", key_path = ["version"] }]"#,
    );

    assert_eq!(
        versions(&agent.homes[0]),
        [(
            "user-setup".to_owned(),
            Some("1.3.0".to_owned()),
            Some(product)
        )]
    );
    assert_eq!(note_codes(&agent.homes[0]), []);
}

#[test]
fn a_pre_release_suffix_counts_as_its_numeric_version() {
    let fs = test_fixture(&[
        (&format!(r"{PROGRAMS}\codex\bin\codex.exe"), b"MZ".to_vec()),
        (
            &format!(r"{PROGRAMS}\codex\vendor\codex-package.json"),
            br#"{ "layoutVersion": 1, "version": "1.4.0-insider" }"#.to_vec(),
        ),
    ]);

    let agent = test_agent_found(
        &fs,
        FakeEnvironment::with_home(HOME),
        r#"[[installations]]
id = "npm"
folder = "%LOCALAPPDATA%/Programs/codex"
executable = "bin/codex.exe"
versions = [{ block = "json-value", file = "vendor/codex-package.json", key_path = ["version"] }]"#,
    );

    let home = &agent.homes[0];
    assert_eq!(
        home.installations[0].version.as_deref(),
        Some("1.4.0-insider")
    );
    assert_eq!(note_codes(home), []);
}

#[test]
fn an_appx_identity_reads_the_package_manifest() {
    let package = r"C:\Program Files\WindowsApps\Test_1.5.0.0_x64__abc";
    let manifest = format!(r"{package}\AppxManifest.xml");
    let fs = test_fixture(&[
        (&format!(r"{package}\app\test.exe"), b"MZ".to_vec()),
        (
            &manifest,
            br#"<?xml version="1.0"?><Package><Identity Name="Test" ProcessorArchitecture="x64" Version="1.5.0.0" /><Dependencies><TargetDeviceFamily MinVersion="10.0.17763.0" /></Dependencies></Package>"#.to_vec(),
        ),
    ]);

    let agent = test_agent_found(
        &fs,
        FakeEnvironment::with_home(HOME),
        r#"[[installations]]
id = "msix"
folder = "C:/Program Files/WindowsApps/Test_*_x64__abc"
executable = "app/test.exe"
versions = [{ block = "appx-identity" }]"#,
    );

    let home = &agent.homes[0];
    assert_eq!(
        versions(home),
        [(
            "msix".to_owned(),
            Some("1.5.0.0".to_owned()),
            Some(manifest)
        )]
    );
    assert!(home.installations[0].out_of_range);
    assert_eq!(
        home.notes[0].sentence,
        "Test Agent 1.5.0.0 is newer than the versions this definition was tested with (1.2 to 1.4.0)."
    );
}

#[test]
fn an_uninstall_key_is_the_fallback_when_the_files_say_nothing() {
    let fs = test_fixture(&[(&format!(r"{PROGRAMS}\test\Test.exe"), b"MZ".to_vec())]);
    let key = r"Software\Microsoft\Windows\CurrentVersion\Uninstall\{DADADADA-ADAD}_is1";
    let env = FakeEnvironment::with_home(HOME).with_registry_value(
        Hive::CurrentUser,
        key,
        "DisplayVersion",
        "1.2.7",
    );

    let agent = test_agent_found(
        &fs,
        env,
        r#"[[installations]]
id = "user-setup"
folder = "%LOCALAPPDATA%/Programs/test"
executable = "Test.exe"
versions = [{ block = "exe-version" }, { block = "uninstall-key", hive = "current-user", key = "{DADADADA-ADAD}_is1" }]"#,
    );

    assert_eq!(
        versions(&agent.homes[0]),
        [(
            "user-setup".to_owned(),
            Some("1.2.7".to_owned()),
            Some(format!(r"HKCU\{key}"))
        )]
    );
}

#[test]
fn an_installation_reads_only_the_homes_it_names() {
    let fs = test_fixture(&[
        (
            &format!(r"{PROGRAMS}\test\Test.exe"),
            fake::executable(Some([1, 3, 0, 0])),
        ),
        (
            &format!(r"{PROGRAMS}\test-insiders\Test.exe"),
            fake::executable(Some([1, 5, 0, 0])),
        ),
    ]);

    let agent = test_agent_found(
        &fs,
        FakeEnvironment::with_home(HOME),
        r#"[[installations]]
id = "stable"
folder = "%LOCALAPPDATA%/Programs/test"
executable = "Test.exe"
versions = [{ block = "exe-version" }]
homes = ["default"]

[[installations]]
id = "insiders"
folder = "%LOCALAPPDATA%/Programs/test-insiders"
executable = "Test.exe"
versions = [{ block = "exe-version" }]
homes = ["insiders"]"#,
    );

    let ids = |home: &Home| -> Vec<String> {
        home.installations
            .iter()
            .map(|installation| installation.id.clone())
            .collect()
    };
    let [stable, insiders] = agent.homes.as_slice() else {
        panic!("two homes: {:?}", agent.homes);
    };
    assert_eq!(stable.home, "default");
    assert_eq!(ids(stable), ["stable"]);
    assert_eq!(note_codes(stable), []);
    assert_eq!(ids(insiders), ["insiders"]);
    assert_eq!(
        note_codes(insiders),
        [AgentNoteCode::VersionOutsideTestedRange]
    );
}

#[test]
fn a_definition_without_a_tested_range_is_noted_as_untested() {
    let fs = test_fixture(&[(
        &format!(r"{PROGRAMS}\test\Test.exe"),
        fake::executable(Some([9, 0, 0, 0])),
    )]);
    let definition = test_agent(
        r#"[[installations]]
id = "user-setup"
folder = "%LOCALAPPDATA%/Programs/test"
executable = "Test.exe"
versions = [{ block = "exe-version" }]"#,
    )
    .replace("verified = { min = \"1.2\", max = \"1.4.0\" }\n", "");
    let core = test_core(&fs, FakeEnvironment::with_home(HOME), &definition);

    let agents = agents(&core);
    let agent = agents
        .iter()
        .find(|agent| agent.id == "test-agent")
        .unwrap();

    assert_eq!(agent.tested, None);
    let home = &agent.homes[0];
    assert!(!home.installations[0].out_of_range);
    assert_eq!(note_codes(home), [AgentNoteCode::DefinitionUntested]);
    assert_eq!(
        home.notes[0].sentence,
        "This definition of Test Agent has no tested version range, so what it says may not match how Test Agent behaves."
    );
}

/// The test Agent with its last-ran version at the key `version` of each home's
/// `last.json`, read in the dialect `dialect` names, if any.
fn last_ran_agent(dialect: Option<&str>) -> String {
    let dialect = dialect.map_or(String::new(), |dialect| {
        format!("dialect = \"{dialect}\"\n")
    });
    test_agent("")
        .replace(
            "roots.state = \".test/state.json\"\n",
            "roots.state = \".test/state.json\"\nroots.last = \".test/last.json\"\n",
        )
        .replace(
            "roots.state = \".test-insiders/state.json\"\n",
            "roots.state = \".test-insiders/state.json\"\nroots.last = \".test-insiders/last.json\"\n",
        )
        .replace(
            "[[scopes]]",
            &format!(
                "[last_ran]\nanchor = {{ block = \"home\", root = \"last\" }}\nkey_path = [\"version\"]\n{dialect}\n[[scopes]]"
            ),
        )
}

/// The default home's last-ran file.
fn last_json() -> PathBuf {
    PathBuf::from(HOME).join(r".test\last.json")
}

/// The test Agent's homes, the default one's last-ran file holding `last`,
/// if anything.
fn last_ran_fixture(last: Option<&str>) -> FakeFileSystem {
    let fs = test_fixture(&[]);
    if let Some(last) = last {
        fs.put_file(last_json(), last.as_bytes().to_vec());
    }
    fs
}

/// The default home of the test Agent read with `definition`, its
/// last-ran file holding `last`, if anything.
fn last_ran_home(definition: &str, last: Option<&str>) -> Home {
    let fs = last_ran_fixture(last);
    let core = test_core(&fs, FakeEnvironment::with_home(HOME), definition);
    let agents = agents(&core);
    let agent = agents
        .iter()
        .find(|agent| agent.id == "test-agent")
        .unwrap();
    agent
        .homes
        .iter()
        .find(|home| home.home == "default")
        .unwrap()
        .clone()
}

/// A last-ran file with comments and a trailing comma.
const JSONC_LAST: &str = "{\n  // written by the Agent\n  \"version\": \"1.3.0\",\n}\n";

#[test]
fn the_last_ran_version_is_read_in_the_dialect_the_agent_reads_its_file_in() {
    let home = last_ran_home(&last_ran_agent(Some("jsonc")), Some(JSONC_LAST));

    assert_eq!(home.last_ran.as_deref(), Some("1.3.0"));
}

#[test]
fn a_last_ran_file_that_does_not_parse_in_the_agents_dialect_is_noted() {
    let home = last_ran_home(&last_ran_agent(None), Some(JSONC_LAST));

    assert_eq!(home.last_ran, None);
    let note = home
        .notes
        .iter()
        .find(|note| note.code == AgentNoteCode::LastRanUnreadable)
        .expect("a LastRanUnreadable note");
    assert_eq!(note.installation, None);
    assert!(
        note.sentence.contains(&last_json().display().to_string()),
        "{}",
        note.sentence
    );
}

#[test]
fn a_missing_last_ran_file_or_key_is_no_version_and_no_note() {
    for last in [None, Some("{ \"other\": \"1.3.0\" }")] {
        let home = last_ran_home(&last_ran_agent(None), last);

        assert_eq!(home.last_ran, None, "{last:?}");
        assert!(
            !note_codes(&home).contains(&AgentNoteCode::LastRanUnreadable),
            "{last:?}: {:?}",
            home.notes
        );
    }
}

#[test]
fn a_last_ran_value_that_is_not_a_string_is_noted() {
    let home = last_ran_home(&last_ran_agent(None), Some("{ \"version\": 13 }"));

    assert_eq!(home.last_ran, None);
    assert!(note_codes(&home).contains(&AgentNoteCode::LastRanUnreadable));
}

#[test]
fn the_last_ran_note_stays_out_of_operation_previews() {
    let fs = last_ran_fixture(Some(JSONC_LAST));
    fs.put_file(
        PathBuf::from(HOME).join(r".test\state.json"),
        br#"{ "servers": { "notes": { "command": "notes-mcp" } } }"#.to_vec(),
    );
    let core = test_core(&fs, FakeEnvironment::with_home(HOME), &last_ran_agent(None));

    let plan = remove_notes(&core);

    let codes: Vec<InfoCode> = plan.preview().info.iter().map(|info| info.code).collect();
    assert_eq!(codes, [InfoCode::NoInstallationFound]);
}
