//! History and Undo through the core `api`, with fake ports: Operations
//! listed and shown, undone after the files changed or not, and pruned.

use config_digger_core::api::{
    Action, Context, Core, Error, ErrorCode, Outcome, ScanOptions, Targets,
};
use config_digger_core::ports::fake::{self, FakeClock, FakeEnvironment, FakeFileSystem};
use std::path::PathBuf;
use std::time::Duration;

const HOME: &str = r"C:\Users\fixture";
const LOCAL_APP_DATA: &str = r"C:\Users\fixture\AppData\Local";

fn claude_json() -> PathBuf {
    PathBuf::from(HOME).join(".claude.json")
}

fn backups() -> PathBuf {
    PathBuf::from(LOCAL_APP_DATA).join(r"config-digger\backups")
}

/// A core over `fs`, as a new process would start it, with its clock at `clock`.
fn core_at(fs: &FakeFileSystem, clock: &FakeClock) -> Core {
    let env = FakeEnvironment::with_home(HOME).with_var("LOCALAPPDATA", LOCAL_APP_DATA);
    let mut ports = fake::ports(fs.clone(), env);
    ports.clock = Box::new(clock.clone());
    Core::new(ports).unwrap()
}

fn core(fs: &FakeFileSystem) -> Core {
    core_at(fs, &FakeClock::default())
}

fn text(fs: &FakeFileSystem, path: impl Into<PathBuf>) -> String {
    String::from_utf8(fs.contents(path.into()).expect("the file exists")).unwrap()
}

fn remove_with(core: &Core, selector: &str) -> Result<Outcome, Error> {
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    let targets = Targets {
        selectors: vec![selector.to_owned()],
        all: false,
    };
    let plan = core.plan(&snapshot, &Context::default(), Action::Remove, &targets)?;
    core.execute(plan, &[])
}

fn remove(fs: &FakeFileSystem, selector: &str) -> Outcome {
    remove_with(&core(fs), selector).unwrap()
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
fn history_lists_operations_newest_first() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);
    let clock = FakeClock::default();
    let first = remove_with(&core_at(&fs, &clock), "linear").unwrap();
    clock.advance(Duration::from_secs(60));
    let second = remove_with(&core_at(&fs, &clock), "notes").unwrap();

    let history = core(&fs).history().unwrap();

    let ids: Vec<&str> = history.iter().map(|op| op.id.as_str()).collect();
    assert_eq!(ids, [second.operation.as_str(), first.operation.as_str()]);
    assert_eq!(history[0].created_at, "2026-10-01T12:01:00Z");
    assert_eq!(history[1].created_at, "2026-10-01T12:00:00Z");
    assert_eq!(history[0].title, "Remove 1 Entry");
    assert_eq!(history[0].status_text, "Applied");
    assert!(history[0].undoable, "{:?}", history[0]);
    assert_eq!(history[0].not_undoable, None);
}

#[test]
fn history_names_the_entries_each_operation_changed() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);
    let removed = remove(&fs, "linear");
    undo_with(&core(&fs), &removed.operation).unwrap();

    let history = core(&fs).history().unwrap();

    assert_eq!(history[0].entries, ["linear"]);
    assert_eq!(history[1].entries, ["linear"]);
}

fn undo_with(core: &Core, operation: &str) -> Result<Outcome, Error> {
    let plan = core.plan_undo(operation)?;
    core.execute(plan, &[])
}

#[test]
fn undo_puts_a_removed_entry_back_where_it_was() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);
    let removed = remove(&fs, "linear");

    // Another core, as after a restart: History is read from disk.
    undo_with(&core(&fs), &removed.operation).unwrap();

    assert_eq!(text(&fs, claude_json()), TWO_USER_SERVERS);
}

#[test]
fn history_shows_which_operation_undid_which() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);
    let removed = remove(&fs, "linear");
    let undo = undo_with(&core(&fs), &removed.operation).unwrap();

    let history = core(&fs).history().unwrap();

    assert_eq!(history[0].id, undo.operation);
    assert_eq!(
        history[0].status_text,
        format!("Undo of {}", removed.operation)
    );
    assert_eq!(
        history[0].undo_of.as_deref(),
        Some(removed.operation.as_str())
    );
    assert!(history[0].undoable, "{:?}", history[0]);
    assert_eq!(history[1].id, removed.operation);
    assert_eq!(
        history[1].status_text,
        format!("Undone by {}", undo.operation)
    );
    assert!(!history[1].undoable);
    assert!(
        history[1]
            .not_undoable
            .as_deref()
            .unwrap()
            .contains(&undo.operation),
        "{:?}",
        history[1]
    );
}

#[test]
fn an_operation_already_undone_cannot_be_undone_again() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);
    let removed = remove(&fs, "linear");
    undo_with(&core(&fs), &removed.operation).unwrap();

    let error = core(&fs).plan_undo(&removed.operation).err().unwrap();

    assert_eq!(error.code, ErrorCode::Blocked);
    assert_eq!(text(&fs, claude_json()), TWO_USER_SERVERS);
}

#[test]
fn undoing_the_undo_applies_the_operation_again() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);
    let removed = remove(&fs, "linear");
    let after_remove = text(&fs, claude_json());
    let undo = undo_with(&core(&fs), &removed.operation).unwrap();

    undo_with(&core(&fs), &undo.operation).unwrap();

    assert_eq!(text(&fs, claude_json()), after_remove);
    let history = core(&fs).history().unwrap();
    assert_eq!(
        history[1].status_text,
        format!("Undo of {}, undone by {}", removed.operation, history[0].id)
    );
    // The Undo that put it back is no longer in effect, so the Remove is undoable again.
    assert!(history[2].undoable, "{:?}", history[2]);
}

#[test]
fn undo_outcome_names_the_operation_it_undid() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);
    let removed = remove(&fs, "linear");

    let undo = undo_with(&core(&fs), &removed.operation).unwrap();

    assert_eq!(
        undo.sentence,
        format!(
            "Undid {}. Operation {}: verified.",
            removed.operation, undo.operation
        )
    );
}

#[test]
fn an_unknown_operation_cannot_be_undone() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);

    let error = core(&fs).plan_undo("20261001-120000-abc").err().unwrap();

    assert_eq!(error.code, ErrorCode::NoMatch);
}

#[test]
fn unrelated_changes_the_agent_made_survive_an_undo() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);
    let removed = remove(&fs, "linear");
    let rewritten = text(&fs, claude_json()).replace("\"numStartups\": 3", "\"numStartups\": 4");
    fs.put_file(claude_json(), rewritten);

    undo_with(&core(&fs), &removed.operation).unwrap();

    assert_eq!(
        text(&fs, claude_json()),
        TWO_USER_SERVERS.replace("\"numStartups\": 3", "\"numStartups\": 4")
    );
}

#[test]
fn the_undo_preview_shows_the_entry_coming_back() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);
    let removed = remove(&fs, "linear");

    let plan = core(&fs).plan_undo(&removed.operation).unwrap();

    let preview = serde_json::to_value(plan.preview()).unwrap();
    assert_eq!(preview["action"], "undo");
    assert_eq!(preview["undo_of"], removed.operation.as_str());
    assert_eq!(
        preview["title"],
        format!("Undo {} \"Remove 1 Entry\"", removed.operation)
    );
    assert_eq!(
        preview["changes"][0]["locator"],
        format!(r"{}#mcpServers.linear", claude_json().display())
    );
    assert_eq!(preview["changes"][0]["before"], "removed");
    assert_eq!(preview["changes"][0]["after"], "effective");
    assert_eq!(preview["files"][0]["edits"][0]["op"], "insert");
    let diff = preview["files"][0]["diff"].as_str().unwrap();
    assert!(diff.contains("+    \"linear\": {\n"), "{diff}");
    // Planning writes nothing.
    assert_eq!(core(&fs).history().unwrap().len(), 1);
}

#[test]
fn an_undo_is_refused_when_what_the_operation_touched_changed_since() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);
    let removed = remove(&fs, "linear");
    let undo = undo_with(&core(&fs), &removed.operation).unwrap();
    // The Agent rotates the URL of the server the Undo put back.
    let rotated = TWO_USER_SERVERS.replace("https://mcp.linear.app/mcp", "https://rotated");
    fs.put_file(claude_json(), rotated.clone());

    let error = core(&fs).plan_undo(&undo.operation).err().unwrap();

    assert_eq!(error.code, ErrorCode::UndoRefused);
    assert!(
        error.message.contains("mcpServers.linear"),
        "{}",
        error.message
    );
    assert_eq!(error.backup, Some(backups().join(&undo.operation)));
    let diff = error.diffs[0].diff.clone().unwrap();
    assert!(
        diff.contains("+      \"url\": \"https://rotated\""),
        "{diff}"
    );
    assert_eq!(text(&fs, claude_json()), rotated);
}

#[test]
fn undo_of_a_remove_refuses_to_overwrite_a_same_named_entry_that_appeared_since() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);
    let removed = remove(&fs, "linear");
    let readded = TWO_USER_SERVERS.replace("https://mcp.linear.app/mcp", "https://readded");
    fs.put_file(claude_json(), readded.clone());

    let error = core(&fs).plan_undo(&removed.operation).err().unwrap();

    assert_eq!(error.code, ErrorCode::Blocked);
    assert!(
        error.message.contains("mcpServers.linear"),
        "{}",
        error.message
    );
    assert_eq!(text(&fs, claude_json()), readded);
}

#[test]
fn undo_of_a_remove_recreates_a_missing_file_whose_folder_exists() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);
    let removed = remove(&fs, "linear");
    fs.delete_file(claude_json());

    undo_with(&core(&fs), &removed.operation).unwrap();

    assert_eq!(
        text(&fs, claude_json()),
        "{\n  \"mcpServers\": {\n    \"linear\": {\n      \"url\": \"https://mcp.linear.app/mcp\"\n    }\n  }\n}\n"
    );
}

#[test]
fn undo_of_a_remove_is_refused_when_the_location_is_gone() {
    let project = r"E:\projects\gone";
    let mcp_json = PathBuf::from(project).join(".mcp.json");
    let fs = FakeFileSystem::new()
        .with_file(claude_json(), "{}")
        .with_file(
            &mcp_json,
            r#"{"mcpServers": {"supabase": {"url": "https://p"}}}"#,
        );
    let core_now = core(&fs);
    let snapshot = core_now
        .scan(&ScanOptions {
            projects: vec![PathBuf::from(project)],
        })
        .unwrap();
    let targets = Targets {
        selectors: vec!["supabase".to_owned()],
        all: false,
    };
    let context = Context {
        project: Some(PathBuf::from(project)),
    };
    let plan = core_now
        .plan(&snapshot, &context, Action::Remove, &targets)
        .unwrap();
    let removed = core_now.execute(plan, &[]).unwrap();
    fs.delete_folder(project);

    let error = core(&fs).plan_undo(&removed.operation).err().unwrap();

    assert_eq!(error.code, ErrorCode::Blocked);
    assert!(error.message.contains(project), "{}", error.message);
    assert_eq!(fs.contents(&mcp_json), None);
}

#[test]
fn history_show_lists_the_edits_and_the_files_copied_to_the_backup() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);
    let removed = remove(&fs, "linear");

    let operation = core(&fs).history_show(&removed.operation).unwrap();

    assert_eq!(operation.summary.id, removed.operation);
    assert_eq!(operation.backup, backups().join(&removed.operation));
    let location = &operation.locations[0];
    assert_eq!(location.path, claude_json());
    let backup = backups().join(&removed.operation).join("1-.claude.json");
    assert_eq!(location.backup.as_ref(), Some(&backup));
    assert_eq!(text(&fs, backup), TWO_USER_SERVERS);
    let edits = serde_json::to_value(&location.edits).unwrap();
    assert_eq!(
        edits,
        serde_json::json!([{ "op": "remove", "key_path": ["mcpServers", "linear"] }])
    );
    assert_eq!(operation.changes[0].item, "linear");
}

#[test]
fn history_show_diffs_each_file_as_the_operation_changed_it() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);
    let removed = remove(&fs, "linear");
    // What the file holds now does not change what the Operation did.
    fs.put_file(
        claude_json(),
        TWO_USER_SERVERS.replace("notes-mcp", "notes-mcp-2"),
    );

    let operation = core(&fs).history_show(&removed.operation).unwrap();

    let diff = operation.locations[0].diff.as_ref().unwrap();
    assert_eq!(diff.path, claude_json());
    assert_eq!(
        diff.diff.as_deref().unwrap(),
        r#"--- a/C:\Users\fixture\.claude.json
+++ b/C:\Users\fixture\.claude.json
@@ -1,9 +1,6 @@
 {
   "numStartups": 3,
   "mcpServers": {
-    "linear": {
-      "url": "https://mcp.linear.app/mcp"
-    },
     "notes": {
       "command": "notes-mcp"
     }
"#
    );
}

#[test]
fn backups_of_a_file_list_each_copy_history_holds_newest_first() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);
    let clock = FakeClock::default();
    let first = remove_with(&core_at(&fs, &clock), "linear").unwrap();
    clock.advance(Duration::from_secs(60));
    let second = remove_with(&core_at(&fs, &clock), "notes").unwrap();

    // A Read failure names the file in its own spelling.
    let spelled = PathBuf::from(r"c:\users\FIXTURE\.claude.json");
    let backups_of = core(&fs).backups_of(&spelled).unwrap();

    let operations: Vec<&str> = backups_of.iter().map(|b| b.operation.as_str()).collect();
    assert_eq!(
        operations,
        [second.operation.as_str(), first.operation.as_str()]
    );
    assert_eq!(backups_of[0].created_at, "2026-10-01T12:01:00Z");
    assert_eq!(backups_of[0].title, "Remove 1 Entry");
    let copy = backups().join(&first.operation).join("1-.claude.json");
    assert_eq!(backups_of[1].copy, copy);
    assert_eq!(text(&fs, copy), TWO_USER_SERVERS);
}

#[test]
fn backups_of_a_file_leave_out_deleted_backups_and_other_files() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);
    let removed = remove(&fs, "linear");
    let core = core(&fs);
    assert!(
        core.backups_of(&PathBuf::from(HOME).join(".mcp.json"))
            .unwrap()
            .is_empty()
    );

    let plan = core.plan_delete_backup(&removed.operation).unwrap();
    core.execute(plan, &[]).unwrap();

    assert!(core.backups_of(&claude_json()).unwrap().is_empty());
}

#[test]
fn backups_of_a_file_are_none_before_any_operation() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);

    assert!(core(&fs).backups_of(&claude_json()).unwrap().is_empty());
}

#[test]
fn history_show_of_an_unknown_operation_fails() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);

    let error = core(&fs).history_show("20261001-120000-abc").err().unwrap();

    assert_eq!(error.code, ErrorCode::NoMatch);
}

const DAY: Duration = Duration::from_secs(24 * 60 * 60);

/// `count` Operations a minute apart: a Remove of `notes`, then each one an
/// Undo of the one before. Oldest first.
fn operations(fs: &FakeFileSystem, clock: &FakeClock, count: usize) -> Vec<String> {
    let mut ids = vec![remove_with(&core_at(fs, clock), "notes").unwrap().operation];
    while ids.len() < count {
        clock.advance(Duration::from_secs(60));
        let last = ids.last().unwrap();
        ids.push(undo_with(&core_at(fs, clock), last).unwrap().operation);
    }
    ids
}

fn history_ids(fs: &FakeFileSystem) -> Vec<String> {
    let mut ids: Vec<String> = core(fs)
        .history()
        .unwrap()
        .into_iter()
        .map(|op| op.id)
        .collect();
    ids.reverse();
    ids
}

#[test]
fn pruning_keeps_the_last_50_operations_and_removes_older_ones_with_their_backup() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);
    let clock = FakeClock::default();
    let ids = operations(&fs, &clock, 52);
    clock.advance(40 * DAY);

    let pruned = core_at(&fs, &clock).prune_history().unwrap();

    assert_eq!(pruned, ids[..2]);
    assert_eq!(history_ids(&fs), ids[2..]);
    assert_eq!(
        fs.files_below(backups().join(&ids[0])),
        Vec::<PathBuf>::new()
    );
    assert!(!fs.files_below(backups().join(&ids[2])).is_empty());
}

#[test]
fn pruning_keeps_everything_from_the_last_30_days() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);
    let clock = FakeClock::default();
    let ids = operations(&fs, &clock, 52);
    clock.advance(29 * DAY);

    let pruned = core_at(&fs, &clock).prune_history().unwrap();

    assert_eq!(pruned, Vec::<String>::new());
    assert_eq!(history_ids(&fs), ids);
}

#[test]
fn pruning_fails_at_once_while_another_operation_holds_the_lock() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);
    let clock = FakeClock::default();
    let ids = operations(&fs, &clock, 51);
    clock.advance(40 * DAY);
    fs.lock_elsewhere(PathBuf::from(LOCAL_APP_DATA).join(r"config-digger\lock"));

    let error = core_at(&fs, &clock).prune_history().err().unwrap();

    assert_eq!(error.code, ErrorCode::Locked);
    assert_eq!(history_ids(&fs), ids);
}

#[test]
fn history_says_why_an_undo_would_be_refused() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);
    let removed = remove(&fs, "linear");
    let readded = TWO_USER_SERVERS.replace("https://mcp.linear.app/mcp", "https://readded");
    fs.put_file(claude_json(), readded);

    let history = core(&fs).history().unwrap();
    let operation = core(&fs).history_show(&removed.operation).unwrap();

    assert!(!history[0].undoable, "{:?}", history[0]);
    let reason = history[0].not_undoable.as_deref().unwrap();
    assert!(reason.contains("mcpServers.linear"), "{reason}");
    assert_eq!(operation.summary.not_undoable.as_deref(), Some(reason));
}

#[test]
fn the_detail_of_an_operation_holds_the_refusal_its_undo_would_meet() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);
    let removed = remove(&fs, "linear");
    let undo = undo_with(&core(&fs), &removed.operation).unwrap();
    let rotated = TWO_USER_SERVERS.replace("https://mcp.linear.app/mcp", "https://rotated");
    fs.put_file(claude_json(), rotated);

    let operation = core(&fs).history_show(&undo.operation).unwrap();

    let refused = core(&fs).plan_undo(&undo.operation).err().unwrap();
    assert_eq!(operation.undo_refusal, Some(refused));
    // One History does not offer to undo, since it was undone, has none.
    assert_eq!(
        core(&fs)
            .history_show(&removed.operation)
            .unwrap()
            .undo_refusal,
        None
    );
}

#[test]
fn the_detail_of_an_operation_holds_the_block_its_undo_would_meet() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);
    let removed = remove(&fs, "linear");
    let readded = TWO_USER_SERVERS.replace("https://mcp.linear.app/mcp", "https://readded");
    fs.put_file(claude_json(), readded);

    let operation = core(&fs).history_show(&removed.operation).unwrap();

    let blocked = core(&fs).plan_undo(&removed.operation).err().unwrap();
    assert_eq!(blocked.code, ErrorCode::Blocked);
    assert_eq!(operation.undo_refusal, Some(blocked));
}

#[test]
fn an_undo_is_refused_when_the_file_it_changed_is_gone() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);
    let removed = remove(&fs, "linear");
    let undo = undo_with(&core(&fs), &removed.operation).unwrap();
    fs.delete_file(claude_json());

    let error = core(&fs).plan_undo(&undo.operation).err().unwrap();

    assert_eq!(error.code, ErrorCode::UndoRefused);
    assert!(error.message.contains("was deleted"), "{}", error.message);
    assert_eq!(fs.contents(claude_json()), None);
}

#[test]
fn a_second_undo_planned_before_the_first_ran_writes_nothing() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);
    let removed = remove(&fs, "linear");
    let core = core(&fs);
    let first = core.plan_undo(&removed.operation).unwrap();
    let second = core.plan_undo(&removed.operation).unwrap();
    core.execute(first, &[]).unwrap();

    let error = core.execute(second, &[]).err().unwrap();

    assert_eq!(error.code, ErrorCode::Stale);
    assert_eq!(text(&fs, claude_json()), TWO_USER_SERVERS);
    assert_eq!(core.history().unwrap().len(), 2);
}
