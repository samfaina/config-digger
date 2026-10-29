//! Delete backup through the core `api`, with fake ports: an Operation's
//! backup deleted on request, leaving the Operation in History but not
//! undoable, and the extra check when the backup is the only copy for manual
//! recovery.

use config_digger_core::api::{
    Action, Advice, Context, Core, DiffUnavailable, Error, ErrorCode, Outcome, ScanOptions,
    Targets, WarningClass,
};
use config_digger_core::ports::fake::{self, FakeEnvironment, FakeFileSystem};
use std::path::PathBuf;

const HOME: &str = r"C:\Users\fixture";
const LOCAL_APP_DATA: &str = r"C:\Users\fixture\AppData\Local";
const PROJECT: &str = r"E:\projects\petme-app";

fn claude_json() -> PathBuf {
    PathBuf::from(HOME).join(".claude.json")
}

fn mcp_json() -> PathBuf {
    PathBuf::from(PROJECT).join(".mcp.json")
}

fn data() -> PathBuf {
    PathBuf::from(LOCAL_APP_DATA).join("config-digger")
}

fn backups() -> PathBuf {
    data().join("backups")
}

fn core(fs: &FakeFileSystem) -> Core {
    let env = FakeEnvironment::with_home(HOME).with_var("LOCALAPPDATA", LOCAL_APP_DATA);
    Core::new(fake::ports(fs.clone(), env)).unwrap()
}

const TWO_USER_SERVERS: &str = r#"{
  "mcpServers": {
    "linear": {
      "url": "https://mcp.linear.app/mcp",
      "headers": { "Authorization": "Bearer lin_api_secret" }
    },
    "notes": {
      "command": "notes-mcp"
    }
  }
}
"#;

const PROJECT_LINEAR: &str = r#"{"mcpServers": {"linear": {"url": "https://p"}}}"#;

/// Removes `selector` with no Project and returns the Operation's id.
fn removed(fs: &FakeFileSystem, selector: &str) -> String {
    let core = core(fs);
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    let targets = Targets {
        selectors: vec![selector.to_owned()],
        all: false,
        ..Targets::default()
    };
    let plan = core
        .plan(&snapshot, &Context::default(), Action::Remove, &targets)
        .unwrap();
    core.execute(plan, &[]).unwrap().operation
}

/// Plans and executes the deletion of the backup of `id`, acknowledging `acks`.
fn delete_backup(fs: &FakeFileSystem, id: &str, acks: &[WarningClass]) -> Result<Outcome, Error> {
    let core = core(fs);
    let plan = core.plan_delete_backup(id)?;
    core.execute(plan, acks)
}

/// An Operation whose Rollback is incomplete: both `linear` Entries in
/// [`PROJECT`], where `.mcp.json` takes the write and refuses the restore,
/// and `~/.claude.json` refuses the write.
fn needs_attention(fs: &FakeFileSystem) -> String {
    let core = core(fs);
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
        ..Targets::default()
    };
    let plan = core
        .plan(&snapshot, &context, Action::Remove, &targets)
        .unwrap();
    let error = core.execute(plan, &[]).err().unwrap();
    assert_eq!(error.code, ErrorCode::RollbackIncomplete);
    fs.stop_failing_writes();
    error.failed.unwrap().operation
}

fn rollback_incomplete() -> FakeFileSystem {
    FakeFileSystem::new()
        .with_file(claude_json(), TWO_USER_SERVERS)
        .with_file(mcp_json(), PROJECT_LINEAR)
        .failing_writes_to_after(mcp_json(), 1)
        .failing_writes_to(claude_json())
}

#[test]
fn the_preview_lists_the_copies_it_deletes_and_deletes_nothing() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);
    let operation = removed(&fs, "linear");
    let copy = backups().join(&operation).join("1-.claude.json");

    let plan = core(&fs).plan_delete_backup(&operation).unwrap();

    let preview = plan.preview();
    assert_eq!(preview.action, Action::DeleteBackup);
    assert_eq!(preview.backup_of.as_deref(), Some(operation.as_str()));
    assert_eq!(
        preview.title,
        format!("Delete the backup of {operation} \"Remove 1 Entry\"")
    );
    assert_eq!(preview.deleted, std::slice::from_ref(&copy));
    assert_eq!(preview.changes, []);
    assert_eq!(preview.files, []);
    assert!(
        preview.backup_note.contains("can no longer be undone"),
        "{}",
        preview.backup_note
    );
    assert_eq!(plan.warnings(), []);
    assert!(fs.contents(&copy).is_some());
}

#[test]
fn deleting_a_backup_keeps_the_operation_in_history_as_not_undoable() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);
    let operation = removed(&fs, "linear");

    let outcome = delete_backup(&fs, &operation, &[]).unwrap();

    assert_eq!(outcome.operation, operation);
    assert_eq!(outcome.backup, backups().join(&operation));
    assert_eq!(
        outcome.sentence,
        format!("Deleted the backup of {operation}. It can no longer be undone.")
    );
    let history = core(&fs).history().unwrap();
    assert_eq!(history.len(), 1);
    assert_eq!(history[0].id, operation);
    assert_eq!(
        history[0].status_text,
        "Applied, backup deleted, not undoable"
    );
    assert!(history[0].backup_deleted);
    assert!(!history[0].undoable);
    let reason = history[0].not_undoable.as_deref().unwrap();
    assert!(reason.contains("backup was deleted"), "{reason}");
}

#[test]
fn deleting_a_backup_leaves_only_a_manifest_holding_no_value() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);
    let operation = removed(&fs, "linear");

    delete_backup(&fs, &operation, &[]).unwrap();

    let folder = backups().join(&operation);
    let manifest = folder.join("manifest.json");
    assert_eq!(fs.files_below(&folder), std::slice::from_ref(&manifest));
    let manifest = String::from_utf8(fs.contents(&manifest).unwrap()).unwrap();
    assert!(!manifest.contains("lin_api_secret"), "{manifest}");
    assert!(!manifest.contains("mcp.linear.app"), "{manifest}");
}

#[test]
fn history_show_still_lists_what_the_operation_did_without_its_backup() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);
    let operation = removed(&fs, "linear");
    delete_backup(&fs, &operation, &[]).unwrap();

    let detail = core(&fs).history_show(&operation).unwrap();

    assert_eq!(detail.locations.len(), 1);
    assert_eq!(detail.locations[0].path, claude_json());
    assert_eq!(detail.locations[0].backup, None);
    assert_eq!(
        detail.locations[0].edits[0].key_path,
        ["mcpServers", "linear"]
    );
    assert_eq!(detail.changes.len(), 1);
    assert!(detail.summary.backup_deleted);
}

#[test]
fn history_says_whether_a_backup_can_still_be_deleted() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);
    let operation = removed(&fs, "linear");
    let copy = backups().join(&operation).join("1-.claude.json");
    assert!(core(&fs).history().unwrap()[0].backup_deletable);

    delete_backup(&fs, &operation, &[]).unwrap();
    assert!(!core(&fs).history().unwrap()[0].backup_deletable);

    // A deletion that left a copy behind can be run again.
    fs.put_file(&copy, TWO_USER_SERVERS);
    let history = core(&fs).history().unwrap();
    assert!(history[0].backup_deleted);
    assert!(history[0].backup_deletable);
}

#[test]
fn history_shows_why_there_is_no_diff_once_the_backup_is_deleted() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);
    let operation = removed(&fs, "linear");
    delete_backup(&fs, &operation, &[]).unwrap();

    let detail = core(&fs).history_show(&operation).unwrap();

    let diff = detail.locations[0].diff.as_ref().unwrap();
    assert_eq!(diff.diff, None);
    assert_eq!(diff.diff_unavailable, Some(DiffUnavailable::BackupDeleted));
    assert_eq!(
        diff.diff_note.as_deref(),
        Some(
            format!(
                "Can't show a diff: the backup of Operation {operation} was deleted, with the values of its edits."
            )
            .as_str()
        )
    );
}

#[test]
fn an_operation_whose_backup_was_deleted_cannot_be_undone() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);
    let operation = removed(&fs, "linear");
    delete_backup(&fs, &operation, &[]).unwrap();

    let error = core(&fs).plan_undo(&operation).err().unwrap();

    assert_eq!(error.code, ErrorCode::Blocked);
    assert!(
        error.message.contains("backup was deleted"),
        "{}",
        error.message
    );
}

#[test]
fn an_undo_planned_before_the_backup_was_deleted_writes_nothing() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);
    let operation = removed(&fs, "linear");
    let after_remove = fs.contents(claude_json()).unwrap();
    let core = core(&fs);
    let undo = core.plan_undo(&operation).unwrap();
    delete_backup(&fs, &operation, &[]).unwrap();

    let error = core.execute(undo, &[]).err().unwrap();

    assert_eq!(error.code, ErrorCode::Stale);
    assert_eq!(fs.contents(claude_json()).unwrap(), after_remove);
}

#[test]
fn a_backup_already_deleted_cannot_be_deleted_again() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);
    let operation = removed(&fs, "linear");
    delete_backup(&fs, &operation, &[]).unwrap();

    let error = core(&fs).plan_delete_backup(&operation).err().unwrap();

    assert_eq!(error.code, ErrorCode::Blocked);
}

#[test]
fn a_backup_deleted_after_the_preview_is_not_deleted_again() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);
    let operation = removed(&fs, "linear");
    let core = core(&fs);
    let plan = core.plan_delete_backup(&operation).unwrap();
    delete_backup(&fs, &operation, &[]).unwrap();

    let error = core.execute(plan, &[]).err().unwrap();

    assert_eq!(error.code, ErrorCode::Stale);
}

#[test]
fn deleting_the_backup_of_an_unknown_operation_fails() {
    let fs = FakeFileSystem::new();

    let error = core(&fs)
        .plan_delete_backup("20261001-120000-abc")
        .err()
        .unwrap();

    assert_eq!(error.code, ErrorCode::NoMatch);
}

#[test]
fn only_the_operations_own_backup_is_deleted() {
    // Parked Entries are live state in their own store, not backups.
    let parked = data().join(r"parked\claude-desktop\linear.json");
    let fs = FakeFileSystem::new()
        .with_file(claude_json(), TWO_USER_SERVERS)
        .with_file(&parked, r#"{"command": "linear-mcp"}"#);
    let first = removed(&fs, "linear");
    let second = removed(&fs, "notes");

    delete_backup(&fs, &first, &[]).unwrap();

    assert!(fs.contents(&parked).is_some());
    assert_eq!(fs.files_below(backups().join(&second)).len(), 2);
    assert!(core(&fs).history().unwrap()[0].undoable);
}

#[test]
fn deleting_a_backup_while_another_operation_writes_fails() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);
    let operation = removed(&fs, "linear");
    let core = core(&fs);
    let plan = core.plan_delete_backup(&operation).unwrap();
    fs.lock_elsewhere(data().join("lock"));

    let error = core.execute(plan, &[]).err().unwrap();

    assert_eq!(error.code, ErrorCode::Locked);
    assert_eq!(fs.files_below(backups().join(&operation)).len(), 2);
}

// The backup of an Operation that needs attention.

#[test]
fn the_needs_attention_warning_says_first_to_copy_from_the_backup() {
    let fs = rollback_incomplete();
    let operation = needs_attention(&fs);

    let plan = core(&fs).plan_delete_backup(&operation).unwrap();

    let warning = &plan.warnings()[0];
    assert_eq!(
        warning.advice,
        [Advice {
            todo: "Copy what you need from this backup before deleting it.".to_owned(),
            why: warning.sentence.clone(),
            paths: Vec::new(),
        }]
    );
}

#[test]
fn the_backup_of_an_operation_that_needs_attention_needs_its_own_check() {
    let fs = rollback_incomplete();
    let operation = needs_attention(&fs);

    let plan = core(&fs).plan_delete_backup(&operation).unwrap();

    let warnings = plan.warnings();
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].class, WarningClass::NeedsAttention);
    assert!(
        warnings[0]
            .sentence
            .contains("only copy for manual recovery"),
        "{}",
        warnings[0].sentence
    );
}

#[test]
fn without_that_check_nothing_is_deleted() {
    let fs = rollback_incomplete();
    let operation = needs_attention(&fs);
    let files = fs.files_below(backups().join(&operation));

    let error = delete_backup(&fs, &operation, &[]).err().unwrap();

    assert_eq!(error.code, ErrorCode::AckRequired);
    assert!(
        error.message.contains("needs-attention"),
        "{}",
        error.message
    );
    assert_eq!(fs.files_below(backups().join(&operation)), files);
    assert!(core(&fs).history().unwrap()[0].needs_attention);
}

#[test]
fn deleting_the_backup_of_an_operation_that_needs_attention_dismisses_it() {
    let fs = rollback_incomplete();
    let operation = needs_attention(&fs);

    delete_backup(&fs, &operation, &[WarningClass::NeedsAttention]).unwrap();

    let history = core(&fs).history().unwrap();
    assert!(!history[0].needs_attention);
    assert_eq!(
        history[0].status_text,
        "Rollback incomplete, dismissed, backup deleted, not undoable"
    );
    let folder = backups().join(&operation);
    assert_eq!(fs.files_below(&folder), [folder.join("manifest.json")]);
}

#[test]
fn an_operation_that_needs_attention_only_since_the_preview_keeps_its_backup() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_USER_SERVERS);
    let operation = removed(&fs, "linear");
    let core = core(&fs);
    let plan = core.plan_delete_backup(&operation).unwrap();
    // Its status changes after the preview, which had no check to acknowledge.
    let manifest = backups().join(&operation).join("manifest.json");
    let mut recorded: serde_json::Value =
        serde_json::from_slice(&fs.contents(&manifest).unwrap()).unwrap();
    recorded["status"] = serde_json::json!("needs_attention");
    fs.put_file(&manifest, recorded.to_string());

    let error = core.execute(plan, &[]).err().unwrap();

    assert_eq!(error.code, ErrorCode::Stale);
    assert_eq!(fs.files_below(backups().join(&operation)).len(), 2);
}
