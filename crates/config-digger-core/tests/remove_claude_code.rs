//! Remove of Claude Code MCP server Entries through the core `api`, with fake
//! ports: plan, then execute, then the files, backups and History it leaves.

use config_digger_core::api::{Action, Context, Core, ErrorCode, ScanOptions, Targets};
use config_digger_core::ports::fake::{self, FakeEnvironment, FakeFileSystem};
use std::path::PathBuf;

const HOME: &str = r"C:\Users\fixture";
const LOCAL_APP_DATA: &str = r"C:\Users\fixture\AppData\Local";

fn claude_json() -> PathBuf {
    PathBuf::from(HOME).join(".claude.json")
}

fn core(fs: &FakeFileSystem) -> Core {
    let env = FakeEnvironment::with_home(HOME).with_var("LOCALAPPDATA", LOCAL_APP_DATA);
    Core::new(fake::ports(fs.clone(), env)).unwrap()
}

fn targets(selectors: &[&str]) -> Targets {
    Targets {
        selectors: selectors.iter().map(|s| (*s).to_owned()).collect(),
        all: false,
    }
}

fn text(fs: &FakeFileSystem, path: impl Into<PathBuf>) -> String {
    String::from_utf8(fs.contents(path.into()).expect("the file exists")).unwrap()
}

const TWO_USER_SERVERS: &str = r#"{
  "numStartups": 3,
  "mcpServers": {
    "linear": {
      "url": "https://mcp.linear.app/mcp"
    },
    "notes": {
      "command": "notes-mcp"
    }
  }
}
"#;

#[test]
fn remove_takes_out_only_the_entry_key() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);
    let core = core(&fs);
    let snapshot = core.scan(&ScanOptions::default()).unwrap();

    let plan = core
        .plan(
            &snapshot,
            &Context::default(),
            Action::Remove,
            &targets(&["linear"]),
        )
        .unwrap();
    core.execute(plan, &[]).unwrap();

    assert_eq!(
        text(&fs, claude_json()),
        r#"{
  "numStartups": 3,
  "mcpServers": {
    "notes": {
      "command": "notes-mcp"
    }
  }
}
"#
    );
}

const PROJECT: &str = r"E:\projects\petme-app";

/// User `linear` and `notes`, and a local `linear` in [`PROJECT`].
const LOCAL_AND_USER: &str = r#"{
  "mcpServers": {
    "linear": { "url": "https://user" },
    "notes": { "command": "notes-mcp" }
  },
  "projects": {
    "E:/projects/petme-app": {
      "mcpServers": {
        "linear": { "url": "https://local" }
      }
    }
  }
}
"#;

fn in_project() -> Context {
    Context {
        project: Some(PathBuf::from(PROJECT)),
    }
}

fn local_linear() -> String {
    format!(
        r"{}#projects.E:/projects/petme-app.mcpServers.linear",
        claude_json().display()
    )
}

fn user(name: &str) -> String {
    format!(r"{}#mcpServers.{name}", claude_json().display())
}

/// `(locator, before, after)` of each row of the preview.
fn rows(plan: &config_digger_core::api::Plan) -> Vec<(String, String, String)> {
    let preview = serde_json::to_value(plan.preview()).unwrap();
    preview["changes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|change| {
            (
                change["locator"].as_str().unwrap().to_owned(),
                change["before"].as_str().unwrap().to_owned(),
                change["after"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

fn row(locator: &str, before: &str, after: &str) -> (String, String, String) {
    (locator.to_owned(), before.to_owned(), after.to_owned())
}

#[test]
fn the_preview_names_the_entry_that_becomes_effective() {
    let fs = FakeFileSystem::new()
        .with_folder(PROJECT)
        .with_file(claude_json(), LOCAL_AND_USER);
    let core = core(&fs);
    let snapshot = core.scan(&ScanOptions::default()).unwrap();

    let plan = core
        .plan(
            &snapshot,
            &in_project(),
            Action::Remove,
            &targets(&[&local_linear()]),
        )
        .unwrap();

    assert_eq!(
        rows(&plan),
        [
            row(&local_linear(), "effective", "removed"),
            row(&user("linear"), "shadowed", "effective"),
        ]
    );
    let preview = serde_json::to_value(plan.preview()).unwrap();
    assert_eq!(preview["changes"][0]["project"], PROJECT);
    assert_eq!(preview["changes"][0]["agent"], "claude-code");
    assert_eq!(preview["effective"][0]["after"], user("linear"));
    assert_eq!(
        preview["effective"][0]["sentence"],
        format!(
            "{} becomes Effective for linear in Claude Code.",
            user("linear")
        )
    );
}

#[test]
fn the_preview_has_the_files_diff_and_the_backup_folder() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);
    let core = core(&fs);
    let snapshot = core.scan(&ScanOptions::default()).unwrap();

    let plan = core
        .plan(
            &snapshot,
            &Context::default(),
            Action::Remove,
            &targets(&["notes"]),
        )
        .unwrap();

    let preview = serde_json::to_value(plan.preview()).unwrap();
    assert_eq!(
        preview["files"][0]["path"],
        claude_json().display().to_string()
    );
    assert_eq!(
        preview["files"][0]["edits"],
        serde_json::json!([{ "op": "remove", "key_path": ["mcpServers", "notes"] }])
    );
    let diff = preview["files"][0]["diff"].as_str().unwrap();
    assert!(diff.contains("-    \"notes\": {\n"), "{diff}");
    assert!(
        diff.contains("-      \"command\": \"notes-mcp\"\n"),
        "{diff}"
    );
    assert_eq!(
        preview["backup_folder"],
        format!(r"{LOCAL_APP_DATA}\config-digger\backups")
    );
    assert_eq!(rows(&plan), [row(&user("notes"), "effective", "removed")]);
    assert_eq!(preview["effective"], serde_json::json!([]));
    assert_eq!(preview["title"], "Remove 1 Entry");
    assert_eq!(
        preview["backup_note"],
        format!(
            r"Each touched file is copied to {LOCAL_APP_DATA}\config-digger\backups before anything is written."
        )
    );
}

#[test]
fn planning_writes_nothing() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);
    let core = core(&fs);
    let snapshot = core.scan(&ScanOptions::default()).unwrap();

    core.plan(
        &snapshot,
        &Context::default(),
        Action::Remove,
        &targets(&["notes"]),
    )
    .unwrap();

    assert_eq!(text(&fs, claude_json()), TWO_USER_SERVERS);
    assert_eq!(fs.files_below(LOCAL_APP_DATA), Vec::<PathBuf>::new());
}

fn all(selectors: &[&str]) -> Targets {
    Targets {
        all: true,
        ..targets(selectors)
    }
}

#[test]
fn a_name_with_several_entries_is_ambiguous_and_lists_each_locator() {
    let fs = FakeFileSystem::new()
        .with_folder(PROJECT)
        .with_file(claude_json(), LOCAL_AND_USER);
    let core = core(&fs);
    let snapshot = core.scan(&ScanOptions::default()).unwrap();

    let error = core
        .plan(
            &snapshot,
            &in_project(),
            Action::Remove,
            &targets(&["linear"]),
        )
        .err()
        .unwrap();

    assert_eq!(error.code, ErrorCode::Ambiguous);
    assert_eq!(error.candidates, [local_linear(), user("linear")]);
}

#[test]
fn a_selector_that_matches_nothing_fails() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);
    let core = core(&fs);
    let snapshot = core.scan(&ScanOptions::default()).unwrap();

    let error = core
        .plan(
            &snapshot,
            &Context::default(),
            Action::Remove,
            &targets(&["nope"]),
        )
        .err()
        .unwrap();

    assert_eq!(error.code, ErrorCode::NoMatch);
}

#[test]
fn all_removes_every_entry_of_the_item_as_one_operation() {
    let fs = FakeFileSystem::new()
        .with_folder(PROJECT)
        .with_file(claude_json(), LOCAL_AND_USER);
    let core = core(&fs);
    let snapshot = core.scan(&ScanOptions::default()).unwrap();

    let plan = core
        .plan(&snapshot, &in_project(), Action::Remove, &all(&["linear"]))
        .unwrap();
    assert_eq!(
        rows(&plan),
        [
            row(&local_linear(), "effective", "removed"),
            row(&user("linear"), "shadowed", "removed"),
        ]
    );
    let outcome = core.execute(plan, &[]).unwrap();

    assert_eq!(
        text(&fs, claude_json()),
        r#"{
  "mcpServers": {
    "notes": { "command": "notes-mcp" }
  },
  "projects": {
    "E:/projects/petme-app": {
      "mcpServers": {}
    }
  }
}
"#
    );
    // One file, so one backup copy beside the manifest.
    assert_eq!(fs.files_below(&outcome.backup).len(), 2);
}

#[test]
fn several_selectors_form_one_operation() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);
    let core = core(&fs);
    let snapshot = core.scan(&ScanOptions::default()).unwrap();

    let plan = core
        .plan(
            &snapshot,
            &Context::default(),
            Action::Remove,
            &targets(&["linear", "notes"]),
        )
        .unwrap();
    core.execute(plan, &[]).unwrap();

    assert_eq!(
        text(&fs, claude_json()),
        "{\n  \"numStartups\": 3,\n  \"mcpServers\": {}\n}\n"
    );
}

#[test]
fn one_failing_selector_fails_the_whole_operation() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);
    let core = core(&fs);
    let snapshot = core.scan(&ScanOptions::default()).unwrap();

    let error = core
        .plan(
            &snapshot,
            &Context::default(),
            Action::Remove,
            &targets(&["linear", "nope"]),
        )
        .err()
        .unwrap();

    assert_eq!(error.code, ErrorCode::NoMatch);
    assert_eq!(text(&fs, claude_json()), TWO_USER_SERVERS);
}

#[test]
fn all_with_a_name_that_matches_no_item_fails() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);
    let core = core(&fs);
    let snapshot = core.scan(&ScanOptions::default()).unwrap();

    let error = core
        .plan(
            &snapshot,
            &Context::default(),
            Action::Remove,
            &all(&["nope"]),
        )
        .err()
        .unwrap();

    assert_eq!(error.code, ErrorCode::NoMatch);
}

fn backups() -> PathBuf {
    PathBuf::from(LOCAL_APP_DATA).join(r"config-digger\backups")
}

fn manifest(fs: &FakeFileSystem, outcome: &config_digger_core::api::Outcome) -> serde_json::Value {
    serde_json::from_str(&text(fs, outcome.backup.join("manifest.json"))).unwrap()
}

fn remove(
    fs: &FakeFileSystem,
    selector: &str,
) -> Result<config_digger_core::api::Outcome, config_digger_core::api::Error> {
    let core = core(fs);
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    let plan = core.plan(
        &snapshot,
        &Context::default(),
        Action::Remove,
        &targets(&[selector]),
    )?;
    core.execute(plan, &[])
}

#[test]
fn the_operation_gets_a_short_sortable_id_from_the_clock() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);

    let outcome = remove(&fs, "linear").unwrap();

    // The fake clock stands at 2026-10-01T12:00:00Z.
    let (stamp, random) = outcome.operation.split_at(16);
    assert_eq!(stamp, "20261001-120000-");
    assert!(
        random.len() == 3 && random.chars().all(|c| c.is_ascii_hexdigit()),
        "{}",
        outcome.operation
    );
    assert_eq!(outcome.backup, backups().join(&outcome.operation));
}

#[test]
fn each_touched_file_is_backed_up_before_it_is_written() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);

    let outcome = remove(&fs, "linear").unwrap();

    assert_eq!(
        text(&fs, outcome.backup.join("1-.claude.json")),
        TWO_USER_SERVERS
    );
}

#[test]
fn the_operation_is_recorded_with_its_manifest() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);

    let outcome = remove(&fs, "linear").unwrap();

    let manifest = manifest(&fs, &outcome);
    assert_eq!(manifest["id"], outcome.operation);
    assert_eq!(manifest["created_at"], "2026-10-01T12:00:00Z");
    assert_eq!(manifest["action"], "remove");
    assert_eq!(manifest["status"], "applied");
    assert_eq!(manifest["verify"]["verify"], "verified");
    let location = &manifest["locations"][0];
    assert_eq!(location["path"], claude_json().display().to_string());
    assert_eq!(location["backup"], "1-.claude.json");
    assert_eq!(
        location["edits"][0]["edit"],
        serde_json::json!({ "op": "remove", "key_path": ["mcpServers", "linear"] })
    );
    assert_eq!(location["edits"][0]["inverse"]["op"], "insert");
    assert_eq!(
        location["edits"][0]["depends_on"][0]["before"],
        serde_json::json!({ "url": "https://mcp.linear.app/mcp" })
    );
    assert_eq!(manifest["changes"][0]["after"], "removed");
}

#[test]
fn verify_reports_verified_when_every_state_is_as_predicted() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);

    let outcome = remove(&fs, "linear").unwrap();

    assert_eq!(outcome.verify, config_digger_core::api::Verify::Verified);
    assert!(outcome.differences.is_empty());
    assert_eq!(
        outcome.sentence,
        format!(
            "Removed 1 Entry. Operation {}: verified.",
            outcome.operation
        )
    );
}

#[test]
fn removing_the_last_entry_leaves_the_empty_structure() {
    let fs = FakeFileSystem::new().with_file(
        claude_json(),
        "{\n  \"mcpServers\": {\n    \"linear\": { \"url\": \"https://u\" }\n  }\n}\n",
    );

    remove(&fs, "linear").unwrap();

    assert_eq!(text(&fs, claude_json()), "{\n  \"mcpServers\": {}\n}\n");
}

#[test]
fn a_held_lock_fails_at_once_and_writes_nothing() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);
    fs.lock_elsewhere(PathBuf::from(LOCAL_APP_DATA).join(r"config-digger\lock"));

    let error = remove(&fs, "linear").err().unwrap();

    assert_eq!(error.code, ErrorCode::Locked);
    assert_eq!(text(&fs, claude_json()), TWO_USER_SERVERS);
    assert_eq!(fs.files_below(backups()), Vec::<PathBuf>::new());
}

#[test]
fn the_lock_is_released_once_the_operation_is_recorded() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);

    remove(&fs, "linear").unwrap();

    remove(&fs, "notes").unwrap();
}

#[test]
fn a_backup_that_does_not_match_the_bytes_read_aborts_before_any_write() {
    let fs = FakeFileSystem::new()
        .with_file(claude_json(), TWO_USER_SERVERS)
        .corrupting_writes_below(backups());

    let error = remove(&fs, "linear").err().unwrap();

    assert_eq!(error.code, ErrorCode::BackupFailed);
    assert_eq!(text(&fs, claude_json()), TWO_USER_SERVERS);
}

#[test]
fn a_failed_first_write_writes_nothing_and_is_recorded_as_failed() {
    let fs = FakeFileSystem::new()
        .with_file(claude_json(), TWO_USER_SERVERS)
        .failing_writes_to(claude_json());

    let error = remove(&fs, "linear").err().unwrap();

    assert_eq!(error.code, ErrorCode::WriteFailed);
    assert_eq!(text(&fs, claude_json()), TWO_USER_SERVERS);
    let manifests: Vec<PathBuf> = fs
        .files_below(backups())
        .into_iter()
        .filter(|file| file.ends_with("manifest.json"))
        .collect();
    let manifest: serde_json::Value =
        serde_json::from_str(&text(&fs, manifests[0].clone())).unwrap();
    assert_eq!(manifest["status"], "failed");
}

#[test]
fn a_change_to_the_entry_after_the_preview_writes_nothing() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);
    let core = core(&fs);
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    let plan = core
        .plan(
            &snapshot,
            &Context::default(),
            Action::Remove,
            &targets(&["linear"]),
        )
        .unwrap();
    let changed = TWO_USER_SERVERS.replace("https://mcp.linear.app/mcp", "https://elsewhere");
    fs.put_file(claude_json(), changed.clone());

    let error = core.execute(plan, &[]).err().unwrap();

    assert_eq!(error.code, ErrorCode::Stale);
    assert_eq!(text(&fs, claude_json()), changed);
}

#[test]
fn an_unrelated_change_after_the_preview_survives_the_write() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);
    let core = core(&fs);
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    let plan = core
        .plan(
            &snapshot,
            &Context::default(),
            Action::Remove,
            &targets(&["linear"]),
        )
        .unwrap();
    fs.put_file(
        claude_json(),
        TWO_USER_SERVERS.replace("\"numStartups\": 3", "\"numStartups\": 4"),
    );

    core.execute(plan, &[]).unwrap();

    assert_eq!(
        text(&fs, claude_json()),
        "{\n  \"numStartups\": 4,\n  \"mcpServers\": {\n    \"notes\": {\n      \"command\": \"notes-mcp\"\n    }\n  }\n}\n"
    );
}
