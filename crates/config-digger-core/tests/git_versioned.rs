//! Git-versioned Entries through the core `api`, with a fake git index: the
//! tag on each Entry, the acknowledgement a Remove in a versioned file needs,
//! and the Disable mechanism a versioned target rules out.

use config_digger_core::api::{
    Action, Advice, Context, Core, DisableOption, Entry, ErrorCode, Inventory, ScanOptions,
    Targets, WarningClass,
};
use config_digger_core::ports::fake::{self, FakeEnvironment, FakeFileSystem, FakeGitIndex};
use std::path::PathBuf;

const HOME: &str = r"C:\Users\fixture";
const APP_DATA: &str = r"C:\Users\fixture\AppData\Roaming";
const LOCAL_APP_DATA: &str = r"C:\Users\fixture\AppData\Local";
const PROJECT: &str = r"E:\projects\petme-app";

fn claude_json() -> PathBuf {
    PathBuf::from(HOME).join(".claude.json")
}

fn mcp_json() -> PathBuf {
    PathBuf::from(PROJECT).join(".mcp.json")
}

fn settings_local() -> PathBuf {
    PathBuf::from(PROJECT).join(r".claude\settings.local.json")
}

/// A user `notes` in `~/.claude.json`, which also registers [`PROJECT`].
const CLAUDE_JSON: &str = r#"{
  "mcpServers": {
    "notes": { "command": "notes-mcp" }
  },
  "projects": {
    "E:/projects/petme-app": {}
  }
}
"#;

/// A project `linear` and `github` in [`PROJECT`]'s `.mcp.json`.
const MCP_JSON: &str = r#"{
  "mcpServers": {
    "linear": { "url": "https://mcp.linear.app/mcp" },
    "github": { "command": "github-mcp" }
  }
}
"#;

fn fixture() -> FakeFileSystem {
    FakeFileSystem::new()
        .with_folder(PROJECT)
        .with_file(claude_json(), CLAUDE_JSON)
        .with_file(mcp_json(), MCP_JSON)
}

/// A core over `fs` whose git index holds `versioned`.
fn core(fs: &FakeFileSystem, versioned: &[PathBuf]) -> Core {
    let env = FakeEnvironment::with_home(HOME).with_var("LOCALAPPDATA", LOCAL_APP_DATA);
    let mut ports = fake::ports(fs.clone(), env);
    ports.git = Box::new(versioned.iter().fold(FakeGitIndex::default(), |git, path| {
        git.with_versioned(path)
    }));
    Core::new(ports).unwrap()
}

fn in_project() -> Context {
    Context {
        project: Some(PathBuf::from(PROJECT)),
    }
}

fn targets(selectors: &[&str]) -> Targets {
    Targets {
        selectors: selectors.iter().map(|s| (*s).to_owned()).collect(),
        all: false,
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

/// The one Entry of the Item `name`.
fn entry(inventory: &Inventory, name: &str) -> Entry {
    let item = inventory
        .items
        .iter()
        .find(|item| item.name == name)
        .unwrap_or_else(|| panic!("no Item `{name}`"));
    assert_eq!(item.entries.len(), 1, "`{name}` has one Entry");
    item.entries[0].clone()
}

#[test]
fn an_entry_in_a_versioned_file_is_tagged_git_versioned() {
    let fs = fixture();
    let core = core(&fs, &[mcp_json()]);

    let inventory = inventory(&core, &in_project());

    assert!(entry(&inventory, "linear").git_versioned);
    assert!(entry(&inventory, "github").git_versioned);
    assert!(!entry(&inventory, "notes").git_versioned);
}

#[test]
fn an_untracked_or_ignored_file_is_an_ordinary_file() {
    // The fake index holds only tracked files, as the real one answers.
    let fs = fixture();
    let core = core(&fs, &[]);

    let inventory = inventory(&core, &in_project());

    assert!(!entry(&inventory, "linear").git_versioned);
}

#[test]
fn a_remove_in_a_versioned_file_warns_that_it_shows_in_the_git_diff() {
    let fs = fixture();
    let core = core(&fs, &[mcp_json()]);
    let snapshot = core
        .scan(&ScanOptions {
            projects: vec![PathBuf::from(PROJECT)],
        })
        .unwrap();

    let plan = core
        .plan(
            &snapshot,
            &in_project(),
            Action::Remove,
            &targets(&["linear"]),
        )
        .unwrap();

    let warnings = plan.warnings();
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].class, WarningClass::GitVersioned);
    let sentence = &warnings[0].sentence;
    assert!(
        sentence.contains(&mcp_json().display().to_string()),
        "{sentence}"
    );
    assert!(sentence.contains("git diff"), "{sentence}");
    assert!(plan.preview().files[0].git_versioned);
}

#[test]
fn the_git_versioned_warning_says_first_to_check_the_git_diff_and_gives_the_file_apart() {
    let fs = fixture();
    let core = core(&fs, &[mcp_json()]);
    let snapshot = core
        .scan(&ScanOptions {
            projects: vec![PathBuf::from(PROJECT)],
        })
        .unwrap();

    let plan = core
        .plan(
            &snapshot,
            &in_project(),
            Action::Remove,
            &targets(&["linear"]),
        )
        .unwrap();

    assert_eq!(
        plan.warnings()[0].advice,
        [Advice {
            todo: "Check the git diff before you commit.".to_owned(),
            why: "This location is git-versioned: this change will show in the git diff, and reaches every clone once committed. config-digger never stages or commits it.".to_owned(),
            paths: vec![mcp_json().display().to_string()],
        }]
    );
}

#[test]
fn a_remove_in_an_ordinary_file_has_no_git_versioned_warning() {
    let fs = fixture();
    let core = core(&fs, &[mcp_json()]);
    let snapshot = core
        .scan(&ScanOptions {
            projects: vec![PathBuf::from(PROJECT)],
        })
        .unwrap();

    let plan = core
        .plan(
            &snapshot,
            &in_project(),
            Action::Remove,
            &targets(&["notes"]),
        )
        .unwrap();

    assert!(plan.warnings().is_empty());
    assert!(!plan.preview().files[0].git_versioned);
}

#[test]
fn a_remove_in_a_versioned_file_writes_nothing_until_acknowledged() {
    let fs = fixture();
    let core = core(&fs, &[mcp_json()]);
    let plan = |core: &Core| {
        let snapshot = core
            .scan(&ScanOptions {
                projects: vec![PathBuf::from(PROJECT)],
            })
            .unwrap();
        core.plan(
            &snapshot,
            &in_project(),
            Action::Remove,
            &targets(&["linear"]),
        )
        .unwrap()
    };

    let refused = core.execute(plan(&core), &[]).unwrap_err();
    assert_eq!(refused.code, ErrorCode::AckRequired);
    assert!(
        refused.message.contains("git-versioned"),
        "{}",
        refused.message
    );
    assert_eq!(fs.contents(mcp_json()).unwrap(), MCP_JSON.as_bytes());

    core.execute(plan(&core), &[WarningClass::GitVersioned])
        .unwrap();
    let after = String::from_utf8(fs.contents(mcp_json()).unwrap()).unwrap();
    assert!(!after.contains("linear"), "{after}");
}

/// The file Disable would write for the Entry of `name`, or why it is not offered.
fn disable(inventory: &Inventory, name: &str) -> Result<PathBuf, (String, String)> {
    match entry(inventory, name).disable {
        DisableOption::Offered { path, .. } => Ok(path),
        DisableOption::NotOffered { code, sentence } => Err((
            serde_json::to_value(code)
                .unwrap()
                .as_str()
                .unwrap()
                .to_owned(),
            sentence,
        )),
    }
}

#[test]
fn disable_of_a_versioned_entry_uses_the_machine_local_switch() {
    // `/mcp` writes `disabledMcpServers` in `~/.claude.json`, outside the repo.
    let fs = fixture();
    let core = core(&fs, &[mcp_json()]);

    let inventory = inventory(&core, &in_project());

    assert_eq!(disable(&inventory, "linear"), Ok(claude_json()));
    assert_eq!(disable(&inventory, "notes"), Ok(claude_json()));
}

#[test]
fn disable_skips_a_versioned_target_for_the_next_machine_local_one() {
    // A home folder kept in a dotfiles repo versions `~/.claude.json`.
    let fs = fixture();
    let core = core(&fs, &[mcp_json(), claude_json()]);

    let inventory = inventory(&core, &in_project());

    assert_eq!(disable(&inventory, "linear"), Ok(settings_local()));
}

#[test]
fn disable_is_not_offered_when_every_target_is_versioned() {
    let fs = fixture().with_file(settings_local(), "{}\n");
    let core = core(&fs, &[mcp_json(), claude_json(), settings_local()]);

    let inventory = inventory(&core, &in_project());

    let (code, sentence) = disable(&inventory, "linear").unwrap_err();
    assert_eq!(code, "git_versioned");
    assert!(
        sentence.contains(&claude_json().display().to_string()),
        "{sentence}"
    );
    assert!(
        sentence.contains(&settings_local().display().to_string()),
        "{sentence}"
    );
    // A user server has no project-settings switch to fall back to.
    let (code, _) = disable(&inventory, "notes").unwrap_err();
    assert_eq!(code, "git_versioned");
}

#[test]
fn disable_is_not_offered_where_the_agent_has_no_switch() {
    // Claude Code's switch is per Project, so there is none without one.
    let fs = fixture();
    let core = core(&fs, &[]);

    let inventory = inventory(&core, &Context::default());

    let (code, sentence) = disable(&inventory, "notes").unwrap_err();
    assert_eq!(code, "no_mechanism");
    assert!(sentence.contains("Claude Code"), "{sentence}");
}

#[test]
fn a_disable_that_is_not_offered_is_blocked_with_the_reason() {
    let fs = fixture();
    let core = core(&fs, &[mcp_json(), claude_json()]);
    let snapshot = core
        .scan(&ScanOptions {
            projects: vec![PathBuf::from(PROJECT)],
        })
        .unwrap();
    let (_, sentence) = disable(&core.resolve(&snapshot, &in_project()), "notes").unwrap_err();

    let error = core
        .plan(
            &snapshot,
            &in_project(),
            Action::Disable,
            &targets(&["notes"]),
        )
        .err()
        .expect("Disable is not offered");

    assert_eq!(error.code, ErrorCode::Blocked);
    assert!(error.message.contains(&sentence), "{}", error.message);
}

#[test]
fn a_target_deleted_from_the_working_tree_but_in_the_index_is_still_versioned() {
    // Writing it would put it back, which shows in the git diff.
    let fs = fixture().with_folder(PathBuf::from(PROJECT).join(".claude"));
    let core = core(&fs, &[mcp_json(), claude_json(), settings_local()]);

    let inventory = inventory(&core, &in_project());

    let (code, _) = disable(&inventory, "linear").unwrap_err();
    assert_eq!(code, "git_versioned");
}

#[test]
fn the_undo_of_a_remove_in_a_versioned_file_warns_too() {
    let fs = fixture();
    let core = core(&fs, &[mcp_json()]);
    let snapshot = core
        .scan(&ScanOptions {
            projects: vec![PathBuf::from(PROJECT)],
        })
        .unwrap();
    let plan = core
        .plan(
            &snapshot,
            &in_project(),
            Action::Remove,
            &targets(&["linear"]),
        )
        .unwrap();
    let outcome = core.execute(plan, &[WarningClass::GitVersioned]).unwrap();

    let undo = core.plan_undo(&outcome.operation).unwrap();

    let classes: Vec<WarningClass> = undo.warnings().iter().map(|w| w.class).collect();
    assert_eq!(classes, [WarningClass::GitVersioned]);
}

#[test]
fn a_scan_asks_the_git_index_once_per_repository() {
    // `~/.claude.json` in a dotfiles repo at the home folder, and the
    // Project's files in its own.
    let fs = fixture().with_file(settings_local(), "{}\n");
    let env = FakeEnvironment::with_home(HOME).with_var("LOCALAPPDATA", LOCAL_APP_DATA);
    let mut ports = fake::ports(fs, env);
    let git = FakeGitIndex::default()
        .with_repository(HOME)
        .with_repository(PROJECT)
        .with_versioned(mcp_json())
        .with_versioned(claude_json());
    ports.git = Box::new(git.clone());
    let core = Core::new(ports).unwrap();

    let inventory = inventory(&core, &in_project());

    let mut asked = git.asked();
    asked.sort();
    assert_eq!(asked, [PathBuf::from(HOME), PathBuf::from(PROJECT)]);
    assert!(entry(&inventory, "linear").git_versioned);
    assert!(entry(&inventory, "notes").git_versioned);
}

/// The user's Cursor skill `name`.
fn cursor_skill(name: &str) -> PathBuf {
    PathBuf::from(HOME).join(r".cursor\skills").join(name)
}

/// A Cursor that has run once, with the user's skills `review` and `lint`
/// parked in config-digger's store, and the locator of each one's record
/// there.
fn parked_cursor_skills(env: &FakeEnvironment) -> (FakeFileSystem, Vec<String>) {
    let fs = ["review", "lint"].iter().fold(
        fixture().with_file(
            PathBuf::from(APP_DATA).join(r"Cursor\User\settings.json"),
            "{}\n",
        ),
        |fs, name| {
            fs.with_file(
                cursor_skill(name).join("SKILL.md"),
                format!("---\nname: {name}\n---\n"),
            )
        },
    );
    let core = Core::new(fake::ports(fs.clone(), env.clone())).unwrap();
    for name in ["review", "lint"] {
        let snapshot = core.scan(&ScanOptions { projects: vec![] }).unwrap();
        let selector = cursor_skill(name).display().to_string();
        let park = core
            .plan(
                &snapshot,
                &Context { project: None },
                Action::Disable,
                &targets(&[&selector]),
            )
            .unwrap();
        core.execute(park, &[WarningClass::WideReach]).unwrap();
    }
    let store = PathBuf::from(LOCAL_APP_DATA).join(r"config-digger\parked\manifest.json");
    let locators = ["review", "lint"]
        .iter()
        .map(|name| format!("{}#entries.{name}", store.display()))
        .collect();
    (fs, locators)
}

#[test]
fn an_enable_asks_the_git_index_once_per_repository_of_the_folders_it_brings_back() {
    // Both skills in a dotfiles repo at the home folder, which versions
    // `review` only.
    let env = FakeEnvironment::with_home(HOME)
        .with_var("APPDATA", APP_DATA)
        .with_var("LOCALAPPDATA", LOCAL_APP_DATA);
    let (fs, parked) = parked_cursor_skills(&env);
    let mut ports = fake::ports(fs, env);
    let git = FakeGitIndex::default()
        .with_repository(HOME)
        .with_versioned(cursor_skill("review"));
    ports.git = Box::new(git.clone());
    let core = Core::new(ports).unwrap();
    let snapshot = core.scan(&ScanOptions { projects: vec![] }).unwrap();
    let scanned = git.asked().len();

    let parked: Vec<&str> = parked.iter().map(String::as_str).collect();
    let plan = core
        .plan(
            &snapshot,
            &Context { project: None },
            Action::Enable,
            &targets(&parked),
        )
        .unwrap();

    // The folders it brings back in one call, then the plan's scan of what
    // it comes to. The fake index cannot tell that scan's asks apart, so
    // this counts on it asking what the scan before it did: here every
    // file and folder it reads is under the same two repositories.
    let asked = git.asked();
    let (scan, planned) = asked.split_at(scanned);
    assert_eq!(planned, [&[PathBuf::from(HOME)][..], scan].concat());
    let versioned = |name: &str| {
        let folder = plan
            .preview()
            .folders
            .iter()
            .find(|folder| folder.path == cursor_skill(name))
            .unwrap_or_else(|| panic!("no folder `{name}` brought back"));
        folder.git_versioned
    };
    assert!(versioned("review"));
    assert!(!versioned("lint"));
}

#[test]
fn counting_the_operations_asks_the_git_index_nothing() {
    // History holds the Disables that moved both skills out of a dotfiles
    // repo at the home folder.
    let env = FakeEnvironment::with_home(HOME)
        .with_var("APPDATA", APP_DATA)
        .with_var("LOCALAPPDATA", LOCAL_APP_DATA);
    let (fs, _) = parked_cursor_skills(&env);
    let mut ports = fake::ports(fs, env);
    let git = FakeGitIndex::default().with_repository(HOME);
    ports.git = Box::new(git.clone());
    let core = Core::new(ports).unwrap();

    let count = core.operation_count().unwrap();

    assert_eq!(count, 2);
    assert_eq!(git.asked(), Vec::<PathBuf>::new());
    core.history().unwrap();
    assert!(
        !git.asked().is_empty(),
        "History asks about the moved folders"
    );
}
