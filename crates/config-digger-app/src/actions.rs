//! The plans the action dialog previews, and the History of the Operations
//! they record. A plan stays here, in Rust, under an id: the webview gets its
//! preview, warnings and blocks, then executes it by id with the warnings the
//! user acknowledged. A plan is dropped once it is executed or cancelled, and
//! with the app when it exits.

use crate::session;
use config_digger_core::api::{
    Action, Block, Context, Core, Error, ErrorCode, OperationDetail, OperationSummary, Outcome,
    Plan, Preview, ScanOptions, Targets, Warning, WarningClass,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// What the webview asks to plan.
#[derive(Debug, Clone, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ActionRequest {
    /// One action on one Entry, in the context of `project`, or of no Project.
    Entry {
        verb: Verb,
        /// The Entry's `<path>#<key path>` locator.
        locator: String,
        project: Option<PathBuf>,
    },
    /// The Remove of every leftover of the missing Project `project`, in
    /// its context.
    Leftovers { project: PathBuf },
    /// The Remove of every Stranded Entry, with no Project. One alone is
    /// Removed as an `Entry`, by its locator in the store.
    Stranded,
    /// The Undo of the Operation `operation` in History.
    Undo { operation: String },
    /// The deletion of the backup of the Operation `operation` in History.
    DeleteBackup { operation: String },
}

/// What the dialog does to one Entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Verb {
    /// Makes the Entry stop taking effect while keeping it.
    Disable,
    /// Makes a Disabled Entry take effect again.
    Enable,
    /// Deletes the Entry for every Agent that reads it.
    Remove,
}

impl From<Verb> for Action {
    fn from(verb: Verb) -> Self {
        match verb {
            Verb::Disable => Action::Disable,
            Verb::Enable => Action::Enable,
            Verb::Remove => Action::Remove,
        }
    }
}

/// A plan held under `plan_id`, as the dialog shows it.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct ActionPreview {
    /// The id the plan is held under, which executes or cancels it.
    pub plan_id: String,
    /// What the plan changes.
    pub preview: Preview,
    /// What the user must acknowledge, each on its own.
    pub warnings: Vec<Warning>,
    /// Why it cannot execute, whatever is acknowledged.
    pub blocks: Vec<Block>,
}

/// What executing a plan did.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Executed {
    /// Backed up, written and verified.
    Applied { outcome: Outcome },
    /// Something the plan touched changed after its preview, so nothing was
    /// written: this is a fresh preview of the same action, under a new id.
    Replanned {
        preview: Box<ActionPreview>,
        /// What changed, as a sentence rendered by the core.
        cause: String,
    },
}

/// The plans planned and not yet executed or cancelled.
pub struct Actions {
    core: Core,
    held: HashMap<String, Held>,
    /// The number of the last plan, from which each plan's id is made.
    planned: u64,
    /// The backup folders reported by an Operation or a refused Undo, which
    /// can be revealed.
    reported: HashSet<String>,
}

/// A plan, with the request it was planned from, to plan again.
struct Held {
    plan: Plan,
    request: ActionRequest,
}

impl Actions {
    /// No plans yet, planned and executed through `core`.
    pub fn new(core: Core) -> Self {
        Self {
            core,
            held: HashMap::new(),
            planned: 0,
            reported: HashSet::new(),
        }
    }

    /// Plans `request` against what the disk holds now, and holds the plan.
    pub fn plan(&mut self, request: &ActionRequest) -> Result<ActionPreview, Error> {
        let plan = self.planned_from(request).inspect_err(|error| {
            // A refused Undo points at the backup to recover from.
            self.report(error.backup.as_deref());
        })?;
        self.planned += 1;
        let plan_id = format!("plan-{}", self.planned);
        let preview = ActionPreview {
            plan_id: plan_id.clone(),
            preview: plan.preview().clone(),
            warnings: plan.warnings().to_vec(),
            blocks: plan.blocks().to_vec(),
        };
        let request = request.clone();
        self.held.insert(plan_id, Held { plan, request });
        Ok(preview)
    }

    fn planned_from(&self, request: &ActionRequest) -> Result<Plan, Error> {
        match request {
            ActionRequest::Entry {
                verb,
                locator,
                project,
            } => {
                let snapshot = self.core.scan(&ScanOptions {
                    projects: project.iter().cloned().collect(),
                })?;
                let context = Context {
                    project: project.clone(),
                };
                let targets = Targets {
                    selectors: vec![locator.clone()],
                    all: false,
                };
                self.core
                    .plan(&snapshot, &context, (*verb).into(), &targets)
            }
            ActionRequest::Leftovers { project } => {
                let snapshot = self.core.scan(&ScanOptions::default())?;
                self.core.plan_remove_leftovers(&snapshot, project)
            }
            ActionRequest::Stranded => {
                let snapshot = self.core.scan(&ScanOptions::default())?;
                self.core.plan_remove_stranded(&snapshot)
            }
            ActionRequest::Undo { operation } => self.core.plan_undo(operation),
            ActionRequest::DeleteBackup { operation } => self.core.plan_delete_backup(operation),
        }
    }

    /// Executes the plan held under `plan_id` with the warnings in
    /// `acknowledged`, and drops it. When something it touched changed since
    /// its preview, nothing is written and the same request is planned again.
    pub fn execute(
        &mut self,
        plan_id: &str,
        acknowledged: &[WarningClass],
    ) -> Result<Executed, Error> {
        let Held { plan, request } = self.held.remove(plan_id).ok_or_else(|| not_held(plan_id))?;
        let executed = self.core.execute(plan, acknowledged);
        match &executed {
            Ok(outcome) => self.report(Some(&outcome.backup)),
            Err(error) => self.report(error.backup.as_deref()),
        }
        match executed {
            Ok(outcome) => Ok(Executed::Applied { outcome }),
            Err(error) if error.code == ErrorCode::Stale => {
                // The core tells a script to plan again, which is done here.
                let cause = error.message.trim_end_matches("; plan again").to_owned();
                match self.plan(&request) {
                    Ok(preview) => Ok(Executed::Replanned {
                        preview: Box::new(preview),
                        cause,
                    }),
                    Err(replan) => Err(Error {
                        message: format!("Nothing was written: {cause}. {}", replan.message),
                        ..replan
                    }),
                }
            }
            Err(error) => Err(error),
        }
    }

    /// Drops the plan held under `plan_id`, if any.
    pub fn cancel(&mut self, plan_id: &str) {
        self.held.remove(plan_id);
    }

    /// Every Operation in History, newest first.
    pub fn history(&self) -> Result<Vec<OperationSummary>, Error> {
        self.core.history()
    }

    /// The Operation `id` in History, whose backup folder can then be
    /// revealed, unless its backup was deleted.
    pub fn detail(&mut self, id: &str) -> Result<OperationDetail, Error> {
        let detail = self.core.history_show(id)?;
        if !detail.summary.backup_deleted {
            self.report(Some(&detail.backup));
        }
        Ok(detail)
    }

    /// Dismisses the Operation `id`, which needs attention, once its files
    /// were recovered by hand.
    pub fn dismiss(&self, id: &str) -> Result<OperationSummary, Error> {
        self.core.dismiss(id)
    }

    /// Whether `path` is a backup folder an Operation, a refused Undo or a
    /// detail in History reported here, in any letter case: the webview
    /// reveals no other.
    pub fn can_reveal(&self, path: &Path) -> bool {
        self.reported.contains(&session::key(path))
    }

    fn report(&mut self, backup: Option<&Path>) {
        if let Some(backup) = backup {
            self.reported.insert(session::key(backup));
        }
    }
}

/// Why a plan id is not one held.
fn not_held(plan_id: &str) -> Error {
    Error::unexpected(format!(
        "{plan_id} is not a plan waiting to be applied; preview the action again"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use config_digger_core::ports::fake::{self, FakeEnvironment, FakeFileSystem};

    const HOME: &str = r"C:\Users\fixture";
    const PROJECT: &str = r"E:\projects\app";

    const CLAUDE_JSON: &str = r#"{ "mcpServers": { "notes": { "command": "notes-mcp" } }, "projects": { "E:/projects/app": {} } }"#;

    fn claude_json() -> PathBuf {
        PathBuf::from(HOME).join(".claude.json")
    }

    fn fixture() -> FakeFileSystem {
        FakeFileSystem::new()
            .with_file(claude_json(), CLAUDE_JSON)
            .with_folder(PROJECT)
    }

    fn actions(fs: &FakeFileSystem) -> Actions {
        let env = FakeEnvironment::with_home(HOME)
            .with_var("LOCALAPPDATA", r"C:\Users\fixture\AppData\Local");
        Actions::new(Core::new(fake::ports(fs.clone(), env)).unwrap())
    }

    fn remove_notes() -> ActionRequest {
        ActionRequest::Entry {
            verb: Verb::Remove,
            locator: format!("{}#mcpServers.notes", claude_json().display()),
            project: None,
        }
    }

    fn holds_notes(fs: &FakeFileSystem) -> bool {
        String::from_utf8(fs.contents(claude_json()).unwrap())
            .unwrap()
            .contains("notes-mcp")
    }

    #[test]
    fn a_plan_executes_by_its_id_once() {
        let fs = fixture();
        let mut actions = actions(&fs);

        let planned = actions.plan(&remove_notes()).unwrap();
        assert_eq!(planned.preview.action, Action::Remove);
        assert!(holds_notes(&fs), "the preview writes nothing");

        let Executed::Applied { outcome } = actions.execute(&planned.plan_id, &[]).unwrap() else {
            panic!("applied");
        };
        assert!(!outcome.operation.is_empty());
        assert!(!holds_notes(&fs));

        let again = actions.execute(&planned.plan_id, &[]).unwrap_err();
        assert!(
            again.message.contains("preview the action again"),
            "{}",
            again.message
        );
    }

    #[test]
    fn a_cancelled_plan_never_executes() {
        let fs = fixture();
        let mut actions = actions(&fs);
        let planned = actions.plan(&remove_notes()).unwrap();

        actions.cancel(&planned.plan_id);

        assert!(actions.execute(&planned.plan_id, &[]).is_err());
        assert!(holds_notes(&fs));
    }

    #[test]
    fn a_change_after_the_preview_writes_nothing_and_previews_again() {
        let fs = fixture();
        let mut actions = actions(&fs);
        let planned = actions.plan(&remove_notes()).unwrap();
        let changed = CLAUDE_JSON.replace("notes-mcp", "notes-mcp-2");
        fs.put_file(claude_json(), changed.as_str());

        let executed = actions.execute(&planned.plan_id, &[]).unwrap();

        let Executed::Replanned { preview, cause } = executed else {
            panic!("a fresh preview: {executed:?}");
        };
        assert_eq!(fs.contents(claude_json()).unwrap(), changed.as_bytes());
        assert_ne!(preview.plan_id, planned.plan_id);
        let diff = preview.preview.files[0].diff.as_deref().unwrap();
        assert!(diff.contains("notes-mcp-2"), "{diff}");
        assert!(cause.contains(".claude.json"), "{cause}");
        assert!(!cause.contains("plan again"), "{cause}");

        let Executed::Applied { .. } = actions.execute(&preview.plan_id, &[]).unwrap() else {
            panic!("the fresh preview applies");
        };
        assert!(!holds_notes(&fs));
    }

    #[test]
    fn an_operation_is_undone_through_a_plan_of_its_own() {
        let fs = fixture();
        let mut actions = actions(&fs);
        let planned = actions.plan(&remove_notes()).unwrap();
        let Executed::Applied { outcome } = actions.execute(&planned.plan_id, &[]).unwrap() else {
            panic!("applied");
        };

        let undo = actions
            .plan(&ActionRequest::Undo {
                operation: outcome.operation.clone(),
            })
            .unwrap();
        assert_eq!(undo.preview.action, Action::Undo);
        assert_eq!(undo.preview.undo_of, Some(outcome.operation));
        actions.execute(&undo.plan_id, &[]).unwrap();

        assert!(holds_notes(&fs));
    }

    #[test]
    fn only_the_backup_folder_an_operation_reported_can_be_revealed() {
        let fs = fixture();
        let mut actions = actions(&fs);
        let planned = actions.plan(&remove_notes()).unwrap();
        assert!(!actions.can_reveal(&planned.preview.backup_folder));

        let Executed::Applied { outcome } = actions.execute(&planned.plan_id, &[]).unwrap() else {
            panic!("applied");
        };

        assert!(actions.can_reveal(&outcome.backup));
        assert!(!actions.can_reveal(&planned.preview.backup_folder));
        assert!(!actions.can_reveal(Path::new(r"C:\Windows")));
    }

    #[test]
    fn a_refused_undo_reports_a_backup_folder_that_can_be_revealed() {
        let fs = fixture();
        let mut actions = actions(&fs);
        let planned = actions.plan(&remove_notes()).unwrap();
        let Executed::Applied { outcome } = actions.execute(&planned.plan_id, &[]).unwrap() else {
            panic!("applied");
        };
        let undo = actions
            .plan(&ActionRequest::Undo {
                operation: outcome.operation,
            })
            .unwrap();
        let Executed::Applied { outcome } = actions.execute(&undo.plan_id, &[]).unwrap() else {
            panic!("undone");
        };
        // The Entry the Undo put back changes since.
        fs.put_file(
            claude_json(),
            CLAUDE_JSON.replace("notes-mcp", "notes-mcp-2"),
        );
        // The Operation ran before the app restarted.
        let mut actions = self::actions(&fs);

        let refused = actions
            .plan(&ActionRequest::Undo {
                operation: outcome.operation,
            })
            .unwrap_err();

        assert_eq!(refused.code, ErrorCode::UndoRefused);
        assert!(actions.can_reveal(refused.backup.as_deref().unwrap()));
    }

    fn removed_notes(actions: &mut Actions) -> String {
        let planned = actions.plan(&remove_notes()).unwrap();
        let Executed::Applied { outcome } = actions.execute(&planned.plan_id, &[]).unwrap() else {
            panic!("applied");
        };
        outcome.operation
    }

    #[test]
    fn history_lists_each_operation_with_a_detail_whose_backup_folder_can_be_revealed() {
        let fs = fixture();
        let operation = removed_notes(&mut actions(&fs));
        // History is read after the app restarted.
        let mut actions = actions(&fs);

        let history = actions.history().unwrap();
        assert_eq!(history.len(), 1);
        assert_eq!(history[0].id, operation);

        let detail = actions.detail(&operation).unwrap();
        assert!(actions.can_reveal(&detail.backup));
        let diff = detail.locations[0].diff.as_ref().unwrap();
        let text = diff.diff.as_deref().unwrap();
        assert!(text.contains("-{ \"mcpServers\": { \"notes\""), "{text}");
    }

    #[test]
    fn a_backup_is_deleted_through_a_plan_of_its_own() {
        let fs = fixture();
        let mut actions = actions(&fs);
        let operation = removed_notes(&mut actions);

        let delete = actions
            .plan(&ActionRequest::DeleteBackup {
                operation: operation.clone(),
            })
            .unwrap();
        assert_eq!(delete.preview.action, Action::DeleteBackup);
        assert_eq!(
            delete.preview.backup_of.as_deref(),
            Some(operation.as_str())
        );
        assert_eq!(delete.preview.deleted.len(), 1);
        actions.execute(&delete.plan_id, &[]).unwrap();

        let history = actions.history().unwrap();
        assert!(history[0].backup_deleted);
        assert!(!history[0].undoable);
        // Its folder holds only its record now, so the detail offers nothing to reveal.
        let mut restarted = self::actions(&fs);
        let detail = restarted.detail(&operation).unwrap();
        assert!(!restarted.can_reveal(&detail.backup));
    }

    #[test]
    fn the_leftovers_of_a_missing_project_are_removed_through_one_plan() {
        let gone = r#"{ "projects": { "E:/projects/gone": { "mcpServers": { "a": { "command": "a" }, "b": { "command": "b" } } } } }"#;
        let fs = FakeFileSystem::new().with_file(claude_json(), gone);
        let mut actions = actions(&fs);

        let planned = actions
            .plan(&ActionRequest::Leftovers {
                project: PathBuf::from(r"e:\projects\gone"),
            })
            .unwrap();

        assert_eq!(planned.preview.action, Action::Remove);
        assert_eq!(planned.preview.changes.len(), 2);
        actions.execute(&planned.plan_id, &[]).unwrap();
        let text = String::from_utf8(fs.contents(claude_json()).unwrap()).unwrap();
        assert!(!text.contains("command"), "{text}");
    }

    #[test]
    fn every_stranded_entry_is_removed_through_one_plan() {
        let store =
            PathBuf::from(r"C:\Users\fixture\AppData\Local\config-digger\parked\manifest.json");
        let parked = |name: &str| {
            serde_json::json!({
                "agent": "gone-agent", "home": "default", "location": "user",
                "name": name, "path": r"C:\Users\fixture\.gone.json",
                "key_path": ["mcpServers", name],
                "parked_at": "2026-10-01T14:30:12Z", "entry": { "command": name }
            })
        };
        let manifest = serde_json::json!({ "entries": { "a": parked("a"), "b": parked("b") } });
        let fs = fixture().with_file(&store, manifest.to_string());
        let mut actions = actions(&fs);

        let planned = actions.plan(&ActionRequest::Stranded).unwrap();

        assert_eq!(planned.preview.action, Action::Remove);
        assert_eq!(planned.preview.title, "Remove 2 Entries");
        actions.execute(&planned.plan_id, &[]).unwrap();
        let left: serde_json::Value =
            serde_json::from_slice(&fs.contents(&store).unwrap()).unwrap();
        assert_eq!(left["entries"], serde_json::json!({}));
    }

    #[test]
    fn a_stale_preview_that_cannot_be_planned_again_says_nothing_was_written() {
        let fs = fixture();
        let mut actions = actions(&fs);
        let planned = actions.plan(&remove_notes()).unwrap();
        let gone = r#"{ "mcpServers": {}, "projects": { "E:/projects/app": {} } }"#;
        fs.put_file(claude_json(), gone);

        let error = actions.execute(&planned.plan_id, &[]).unwrap_err();

        assert_eq!(fs.contents(claude_json()).unwrap(), gone.as_bytes());
        assert!(
            error.message.starts_with("Nothing was written:"),
            "{}",
            error.message
        );
        assert!(error.message.contains(".claude.json"), "{}", error.message);
    }
}
