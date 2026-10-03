//! Writes when the world changes under them, through the core `api` with fake
//! ports: a file changed between preview and execute, a write that fails, an
//! Agent rewriting a file after the write, and a restore that fails.

use config_digger_core::api::{
    Action, Context, Core, Error, ErrorCode, FailedOperation, Outcome, RollbackOutcome,
    ScanOptions, Targets, Verify,
};
use config_digger_core::ports::fake::{self, FakeClock, FakeEnvironment, FakeFileSystem};
use std::path::PathBuf;
use std::time::Duration;

const HOME: &str = r"C:\Users\fixture";
const LOCAL_APP_DATA: &str = r"C:\Users\fixture\AppData\Local";
const PROJECT: &str = r"E:\projects\petme-app";

fn claude_json() -> PathBuf {
    PathBuf::from(HOME).join(".claude.json")
}

fn mcp_json() -> PathBuf {
    PathBuf::from(PROJECT).join(".mcp.json")
}

fn backups() -> PathBuf {
    PathBuf::from(LOCAL_APP_DATA).join(r"config-digger\backups")
}

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

const PROJECT_LINEAR: &str = r#"{"mcpServers": {"linear": {"url": "https://p"}}}"#;

/// Removes `selector` with no Project.
fn remove(fs: &FakeFileSystem, selector: &str) -> Result<Outcome, Error> {
    let core = core(fs);
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    let targets = Targets {
        selectors: vec![selector.to_owned()],
        all: false,
    };
    let plan = core.plan(&snapshot, &Context::default(), Action::Remove, &targets)?;
    core.execute(plan, &[])
}

/// Both `linear` Entries in [`PROJECT`]: `.mcp.json` is written first, then
/// `~/.claude.json`.
fn remove_both_linear(core: &Core) -> Result<Outcome, Error> {
    let options = ScanOptions {
        projects: vec![PathBuf::from(PROJECT)],
    };
    let snapshot = core.scan(&options).unwrap();
    let context = Context {
        project: Some(PathBuf::from(PROJECT)),
    };
    let targets = Targets {
        selectors: vec!["linear".to_owned()],
        all: true,
    };
    let plan = core.plan(&snapshot, &context, Action::Remove, &targets)?;
    core.execute(plan, &[])
}

fn two_files() -> FakeFileSystem {
    FakeFileSystem::new()
        .with_file(claude_json(), TWO_USER_SERVERS)
        .with_file(mcp_json(), PROJECT_LINEAR)
}

/// The Operation that failed with `error`.
fn failed(error: &Error) -> &FailedOperation {
    error.failed.as_deref().expect("the Operation was recorded")
}

/// `(path, outcome)` of each location the Rollback went through.
fn rollback(error: &Error) -> Vec<(PathBuf, RollbackOutcome)> {
    failed(error)
        .rollback
        .iter()
        .map(|location| (location.path.clone(), location.outcome))
        .collect()
}

// A file changed between preview and execute.

#[test]
fn an_entry_taken_away_after_the_preview_writes_nothing_and_records_nothing() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);
    let core = core(&fs);
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    let targets = Targets {
        selectors: vec!["linear".to_owned()],
        all: false,
    };
    let plan = core
        .plan(&snapshot, &Context::default(), Action::Remove, &targets)
        .unwrap();
    let only_notes = "{\n  \"mcpServers\": {\n    \"notes\": { \"command\": \"n\" }\n  }\n}\n";
    fs.put_file(claude_json(), only_notes);

    let error = core.execute(plan, &[]).err().unwrap();

    assert_eq!(error.code, ErrorCode::Stale);
    assert!(error.message.contains("plan again"), "{}", error.message);
    assert_eq!(text(&fs, claude_json()), only_notes);
    assert_eq!(core.history().unwrap(), []);
}

// A write that fails.

#[test]
fn a_failed_write_rolls_back_the_files_already_written() {
    let fs = two_files().failing_writes_to(claude_json());

    let error = remove_both_linear(&core(&fs)).err().unwrap();

    assert_eq!(error.code, ErrorCode::WriteFailed);
    assert_eq!(text(&fs, mcp_json()), PROJECT_LINEAR);
    assert_eq!(text(&fs, claude_json()), TWO_USER_SERVERS);
    assert_eq!(
        rollback(&error),
        [
            (mcp_json(), RollbackOutcome::Restored),
            (claude_json(), RollbackOutcome::NotWritten),
        ]
    );
    let operation = failed(&error).operation.clone();
    assert_eq!(error.backup, Some(backups().join(&operation)));
    assert!(error.message.contains("rolled back"), "{}", error.message);
}

#[test]
fn history_shows_a_rolled_back_operation_as_failed_with_its_cause_and_not_undoable() {
    let fs = two_files().failing_writes_to(claude_json());
    let error = remove_both_linear(&core(&fs)).err().unwrap();

    let history = core(&fs).history().unwrap();
    let operation = core(&fs).history_show(&history[0].id).unwrap();

    assert_eq!(history[0].id, failed(&error).operation);
    assert_eq!(history[0].status_text, "Failed, rolled back");
    let cause = history[0].cause.as_deref().unwrap();
    assert!(cause.contains(".claude.json"), "{cause}");
    assert!(!history[0].undoable);
    assert!(!history[0].needs_attention);
    assert_eq!(operation.rollback.len(), 2);
    assert_eq!(operation.rollback[0].outcome, RollbackOutcome::Restored);
}

#[test]
fn the_first_write_failing_is_rolled_back_with_nothing_to_restore() {
    let fs = FakeFileSystem::new()
        .with_file(claude_json(), TWO_USER_SERVERS)
        .failing_writes_to(claude_json());

    let error = remove(&fs, "linear").err().unwrap();

    assert_eq!(error.code, ErrorCode::WriteFailed);
    assert_eq!(
        rollback(&error),
        [(claude_json(), RollbackOutcome::NotWritten)]
    );
    assert_eq!(
        core(&fs).history().unwrap()[0].status_text,
        "Failed, rolled back"
    );
}

#[test]
fn a_file_the_operation_created_is_deleted_again_by_the_rollback() {
    let fs = two_files().with_folder(PROJECT);
    let removed = remove_both_linear(&core(&fs)).unwrap();
    // The project file goes, so the Undo recreates it, then fails on the user file.
    fs.delete_file(mcp_json());
    let fs = fs.failing_writes_to(claude_json());
    let core = core(&fs);
    let plan = core.plan_undo(&removed.operation).unwrap();

    let error = core.execute(plan, &[]).err().unwrap();

    assert_eq!(error.code, ErrorCode::WriteFailed);
    assert_eq!(fs.contents(mcp_json()), None);
    assert_eq!(rollback(&error)[0], (mcp_json(), RollbackOutcome::Restored));
}

// An Agent rewriting the file after the write.

#[test]
fn an_agent_writing_back_its_own_copy_after_the_write_fails_the_operation() {
    // Claude Code was running and writes back what it held, `linear` included.
    let agent_copy = TWO_USER_SERVERS.replace("\"numStartups\": 3", "\"numStartups\": 4");
    let fs = FakeFileSystem::new()
        .with_file(claude_json(), TWO_USER_SERVERS)
        .agent_rewrites_after_next_write(claude_json(), agent_copy.clone());

    let error = remove(&fs, "linear").err().unwrap();

    assert_eq!(error.code, ErrorCode::OverwrittenByAgent);
    assert!(
        error
            .message
            .contains("rewritten after the write; if Claude Code is running"),
        "{}",
        error.message
    );
    // What the Agent wrote is left alone: it holds none of the Operation's edits.
    assert_eq!(text(&fs, claude_json()), agent_copy);
    assert_eq!(
        rollback(&error),
        [(claude_json(), RollbackOutcome::LeftAlone)]
    );
    assert_eq!(
        core(&fs).history().unwrap()[0].status_text,
        "Failed, rolled back"
    );
}

#[test]
fn an_agent_rewrite_that_keeps_the_edit_is_verified() {
    let kept = "{\n  \"numStartups\": 4,\n  \"mcpServers\": {\n    \"notes\": {\n      \"command\": \"notes-mcp\"\n    }\n  }\n}\n";
    let fs = FakeFileSystem::new()
        .with_file(claude_json(), TWO_USER_SERVERS)
        .agent_rewrites_after_next_write(claude_json(), kept);

    let outcome = remove(&fs, "linear").unwrap();

    assert_eq!(outcome.verify, Verify::Verified);
    assert_eq!(text(&fs, claude_json()), kept);
}

#[test]
fn a_location_changed_since_the_write_that_still_holds_the_edit_is_not_restored() {
    // `.mcp.json` is written, then rewritten keeping the edit; `~/.claude.json` fails.
    let rewritten = r#"{"mcpServers": {"supabase": {"url": "https://s"}}}"#;
    let fs = two_files()
        .agent_rewrites_after_next_write(mcp_json(), rewritten)
        .failing_writes_to(claude_json());

    let error = remove_both_linear(&core(&fs)).err().unwrap();

    assert_eq!(error.code, ErrorCode::RollbackIncomplete);
    assert_eq!(failed(&error).failure, Some(ErrorCode::WriteFailed));
    assert_eq!(text(&fs, mcp_json()), rewritten);
    assert_eq!(
        rollback(&error),
        [
            (mcp_json(), RollbackOutcome::NotRestored),
            (claude_json(), RollbackOutcome::NotWritten),
        ]
    );
    let reason = failed(&error).rollback[0].reason.as_deref().unwrap();
    assert!(
        reason.contains("changed after config-digger wrote it"),
        "{reason}"
    );
}

// A restore that fails.

#[test]
fn a_restore_that_fails_leaves_the_rollback_incomplete() {
    // `.mcp.json` takes the write, then refuses the restore; `~/.claude.json` fails.
    let fs = two_files()
        .failing_writes_to_after(mcp_json(), 1)
        .failing_writes_to(claude_json());

    let error = remove_both_linear(&core(&fs)).err().unwrap();

    assert_eq!(error.code, ErrorCode::RollbackIncomplete);
    assert_eq!(failed(&error).failure, Some(ErrorCode::WriteFailed));
    assert_eq!(text(&fs, mcp_json()), r#"{"mcpServers": {}}"#);
    assert_eq!(
        rollback(&error),
        [
            (mcp_json(), RollbackOutcome::NotRestored),
            (claude_json(), RollbackOutcome::NotWritten),
        ]
    );
    assert!(failed(&error).rollback[0].reason.is_some());
    let operation = failed(&error).operation.clone();
    assert_eq!(error.backup, Some(backups().join(&operation)));
    let diff = error.diffs[0].diff.clone().unwrap();
    assert!(diff.contains("-{\"mcpServers\": {\"linear\""), "{diff}");
    assert!(diff.contains("+{\"mcpServers\": {}}"), "{diff}");
}

#[test]
fn history_shows_an_incomplete_rollback_as_needing_attention() {
    let fs = two_files()
        .failing_writes_to_after(mcp_json(), 1)
        .failing_writes_to(claude_json());
    remove_both_linear(&core(&fs)).err().unwrap();

    let history = core(&fs).history().unwrap();

    assert_eq!(history[0].status_text, "Needs attention");
    assert!(history[0].needs_attention);
    assert!(!history[0].undoable);
    assert!(history[0].cause.is_some());
}

const DAY: Duration = Duration::from_secs(24 * 60 * 60);

/// An Operation whose Rollback is incomplete, then 51 more, then 40 days.
fn needs_attention_among_old_operations(fs: &FakeFileSystem, clock: &FakeClock) -> String {
    let error = remove_both_linear(&core_at(fs, clock)).err().unwrap();
    assert_eq!(error.code, ErrorCode::RollbackIncomplete);
    fs.stop_failing_writes();
    let mut last = remove(fs, "notes").unwrap().operation;
    for _ in 0..50 {
        clock.advance(Duration::from_secs(60));
        let core = core_at(fs, clock);
        let plan = core.plan_undo(&last).unwrap();
        last = core.execute(plan, &[]).unwrap().operation;
    }
    clock.advance(40 * DAY);
    failed(&error).operation.clone()
}

#[test]
fn an_operation_that_needs_attention_is_never_pruned() {
    let fs = two_files()
        .failing_writes_to_after(mcp_json(), 1)
        .failing_writes_to(claude_json());
    let clock = FakeClock::default();
    let needs_attention = needs_attention_among_old_operations(&fs, &clock);

    let pruned = core_at(&fs, &clock).prune_history().unwrap();

    assert!(!pruned.contains(&needs_attention), "{pruned:?}");
    assert!(!fs.files_below(backups().join(&needs_attention)).is_empty());
}

#[test]
fn once_dismissed_an_operation_that_needed_attention_is_pruned_as_usual() {
    let fs = two_files()
        .failing_writes_to_after(mcp_json(), 1)
        .failing_writes_to(claude_json());
    let clock = FakeClock::default();
    let needs_attention = needs_attention_among_old_operations(&fs, &clock);

    let dismissed = core_at(&fs, &clock).dismiss(&needs_attention).unwrap();
    let pruned = core_at(&fs, &clock).prune_history().unwrap();

    assert!(!dismissed.needs_attention);
    assert_eq!(dismissed.status_text, "Rollback incomplete, dismissed");
    assert!(pruned.contains(&needs_attention), "{pruned:?}");
}

#[test]
fn only_an_operation_that_needs_attention_can_be_dismissed() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);
    let removed = remove(&fs, "linear").unwrap();

    let error = core(&fs).dismiss(&removed.operation).err().unwrap();

    assert_eq!(error.code, ErrorCode::Blocked);
}

#[test]
fn dismissing_an_unknown_operation_fails() {
    let fs = FakeFileSystem::new();

    let error = core(&fs).dismiss("20261001-120000-abc").err().unwrap();

    assert_eq!(error.code, ErrorCode::NoMatch);
}

// Undo goes through the same Verify and Rollback.

#[test]
fn a_failed_undo_returns_to_the_state_after_the_operation() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);
    let removed = remove(&fs, "linear").unwrap();
    let after_remove = text(&fs, claude_json());
    // Claude Code writes back what it held after the Remove, over the Undo.
    let fs = fs.agent_rewrites_after_next_write(claude_json(), after_remove.clone());
    let core = core(&fs);
    let plan = core.plan_undo(&removed.operation).unwrap();

    let error = core.execute(plan, &[]).err().unwrap();

    assert_eq!(error.code, ErrorCode::OverwrittenByAgent);
    assert_eq!(text(&fs, claude_json()), after_remove);
    // The failed Undo is not in effect, so the Remove is still undoable.
    let history = core.history().unwrap();
    assert_eq!(history[1].id, removed.operation);
    assert!(history[1].undoable, "{:?}", history[1]);
}

#[test]
fn a_failed_undo_write_restores_what_the_undo_had_written() {
    let fs = two_files();
    let removed = remove_both_linear(&core(&fs)).unwrap();
    let after_remove = text(&fs, mcp_json());
    let fs = fs.failing_writes_to(claude_json());
    let core = core(&fs);
    let plan = core.plan_undo(&removed.operation).unwrap();

    let error = core.execute(plan, &[]).err().unwrap();

    assert_eq!(error.code, ErrorCode::WriteFailed);
    assert_eq!(text(&fs, mcp_json()), after_remove);
}

// Verified with differences.

#[test]
fn states_after_that_differ_from_the_prediction_are_verified_with_differences() {
    // Removing the local `linear` should make the user one Effective, but
    // Claude Code drops the user one too right after the write.
    let local_and_user = r#"{
  "mcpServers": {
    "linear": { "url": "https://user" }
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
    let both_gone = r#"{
  "mcpServers": {},
  "projects": {
    "E:/projects/petme-app": {
      "mcpServers": {}
    }
  }
}
"#;
    let fs = FakeFileSystem::new()
        .with_folder(PROJECT)
        .with_file(claude_json(), local_and_user)
        .agent_rewrites_after_next_write(claude_json(), both_gone);
    let core = core(&fs);
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    let local = format!(
        r"{}#projects.E:/projects/petme-app.mcpServers.linear",
        claude_json().display()
    );
    let targets = Targets {
        selectors: vec![local],
        all: false,
    };
    let context = Context {
        project: Some(PathBuf::from(PROJECT)),
    };
    let plan = core
        .plan(&snapshot, &context, Action::Remove, &targets)
        .unwrap();

    let outcome = core.execute(plan, &[]).unwrap();

    assert_eq!(outcome.verify, Verify::VerifiedWithDifferences);
    let differences = serde_json::to_value(&outcome.differences).unwrap();
    assert_eq!(
        differences,
        serde_json::json!([{
            "agent": "claude-code",
            "home": "default",
            "locator": format!(r"{}#mcpServers.linear", claude_json().display()),
            "predicted": "effective",
            "actual": "removed",
        }])
    );
    assert!(
        outcome.sentence.ends_with("verified with differences."),
        "{}",
        outcome.sentence
    );
}
