//! Linked Entries and skill Remove through the core `api`, with a fake file
//! system that models junctions: a junction at a skill's own location is
//! its own Entry, Remove takes away the link or moves the folder into the
//! backup store, a link target warns of the links it breaks, and Undo puts
//! each back.

use config_digger_core::api::{
    Action, Advice, Context, Core, Entry, Error, Inventory, Item, Outcome, Plan, ScanOptions,
    Targets, WarningClass,
};
use config_digger_core::ports::fake::{self, FakeEnvironment, FakeFileSystem};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const HOME: &str = r"C:\Users\fixture";
const LOCAL_APP_DATA: &str = r"C:\Users\fixture\AppData\Local";
const BACKUPS: &str = r"C:\Users\fixture\AppData\Local\config-digger\backups";

fn in_home(relative: &str) -> PathBuf {
    PathBuf::from(HOME).join(relative)
}

/// Where Claude Code reads personal skills.
fn claude_skill(name: &str) -> PathBuf {
    in_home(r".claude\skills").join(name)
}

/// Where the Agents Reader reads skills, as `npx skills` keeps them.
fn agents_skill(name: &str) -> PathBuf {
    in_home(r".agents\skills").join(name)
}

/// A `SKILL.md` whose frontmatter names the skill `name`.
fn skill_md(name: &str) -> String {
    format!("---\nname: {name}\ndescription: The {name} skill.\n---\n\nDo {name}.\n")
}

/// `fs` with a skill folder at `folder` named `name`, holding a script too.
fn with_skill(fs: FakeFileSystem, folder: impl AsRef<Path>, name: &str) -> FakeFileSystem {
    let folder = folder.as_ref();
    fs.with_file(folder.join("SKILL.md"), skill_md(name))
        .with_file(folder.join(r"scripts\run.ps1"), "Write-Output run")
}

/// A Claude Code home with no MCP servers.
fn claude_home() -> FakeFileSystem {
    FakeFileSystem::new().with_file(in_home(".claude.json"), "{}")
}

/// An Agent that reads skills from `~/.agents/skills`, as Codex and Gemini
/// CLI do, so that a link target there is an Entry too.
const AGENTS_READER: &str = r#"
format_version = 1

[agent]
id = "agents-reader"
name = "Agents Reader"

[[homes]]
id = "default"
folder = { block = "path", path = "~" }
roots.skills = ".agents/skills"

[[scopes]]
id = "user"

[[locations]]
id = "user-skills"
anchor = { block = "home", root = "skills" }
format = { block = "skill-folder", name_from = "frontmatter", no_frontmatter = "folder-name", malformed_frontmatter = "folder-name", no_name = "folder-name" }
kind = "skill"
scope = "user"
provenance = "file"
store_class = "plain"
on_read_failure = "ignores_file"

[items.skill]
precedence = ["user-skills"]
competition = { block = "exact-name" }
checks = ["frontmatter", "disable", "precedence"]
"#;

fn core(fs: &FakeFileSystem) -> Core {
    let env = FakeEnvironment::with_home(HOME).with_var("LOCALAPPDATA", LOCAL_APP_DATA);
    Core::with_extra_definitions(
        fake::ports(fs.clone(), env),
        &[("agents-reader.toml", AGENTS_READER)],
    )
    .unwrap()
}

fn inventory(core: &Core) -> Inventory {
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    core.resolve(&snapshot, &Context::default())
}

/// The skill Item `name` of the Agent `agent`.
fn skill<'i>(inventory: &'i Inventory, agent: &str, name: &str) -> &'i Item {
    inventory
        .items
        .iter()
        .find(|item| item.agent == agent && item.name == name)
        .unwrap_or_else(|| panic!("no skill `{name}` of {agent}"))
}

/// A serialised value as its JSON string, such as a State's `"unread"`.
fn code(value: impl serde::Serialize) -> String {
    match serde_json::to_value(value).unwrap() {
        Value::String(code) => code,
        other => panic!("not a code: {other}"),
    }
}

fn entry_json(entry: &Entry) -> Value {
    serde_json::to_value(entry).unwrap()
}

/// `deploy` installed as `npx skills` does: the canonical folder in
/// `~/.agents/skills`, junctioned into `~/.claude/skills`.
fn junctioned_deploy() -> FakeFileSystem {
    with_skill(claude_home(), agents_skill("deploy"), "deploy")
        .with_folder(in_home(r".claude\skills"))
        .with_link(claude_skill("deploy"), agents_skill("deploy"))
}

#[test]
fn a_junction_at_a_skills_own_location_is_a_linked_entry_apart_from_its_target() {
    let fs = junctioned_deploy();

    let inventory = inventory(&core(&fs));

    let linked = skill(&inventory, "claude-code", "deploy");
    assert_eq!(linked.entries.len(), 1);
    let entry = entry_json(&linked.entries[0]);
    assert_eq!(entry["path"], r"C:\Users\fixture\.claude\skills\deploy");
    assert_eq!(entry["state"], "effective");
    assert_eq!(
        entry["link"],
        json!({ "target": r"C:\Users\fixture\.agents\skills\deploy", "broken": false })
    );
    assert_eq!(
        entry["config"],
        json!({ "name": "deploy", "description": "The deploy skill." })
    );
    let target = skill(&inventory, "agents-reader", "deploy");
    let entry = entry_json(&target.entries[0]);
    assert_eq!(entry["path"], r"C:\Users\fixture\.agents\skills\deploy");
    assert_eq!(entry["link"], Value::Null);
}

#[test]
fn a_link_whose_target_is_gone_is_an_unread_linked_entry_named_by_its_folder() {
    let fs = claude_home()
        .with_folder(in_home(r".claude\skills"))
        .with_link(claude_skill("deploy"), agents_skill("deploy"));

    let inventory = inventory(&core(&fs));

    let broken = skill(&inventory, "claude-code", "deploy");
    assert_eq!(code(broken.state), "unread");
    let entry = &broken.entries[0];
    assert_eq!(entry.path, claude_skill("deploy"));
    assert_eq!(code(entry.state), "unread");
    let reason = entry.reason.as_ref().unwrap();
    assert_eq!(code(reason.code), "link_broken");
    assert_eq!(
        reason.sentence,
        r"Claude Code reads nothing here: the link points to C:\Users\fixture\.agents\skills\deploy, which is gone. It can only be Removed."
    );
    assert_eq!(
        entry_json(entry)["link"],
        json!({ "target": r"C:\Users\fixture\.agents\skills\deploy", "broken": true })
    );
    assert_eq!(code(entry_json(entry)["disable"]["code"].clone()), "unread");
}

fn plan(core: &Core, action: Action, selectors: &[&str]) -> Result<Plan, Error> {
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    let targets = Targets {
        selectors: selectors
            .iter()
            .map(|selector| (*selector).to_owned())
            .collect(),
        all: false,
    };
    core.plan(&snapshot, &Context::default(), action, &targets)
}

fn remove(core: &Core, selector: &str) -> Plan {
    plan(core, Action::Remove, &[selector]).unwrap()
}

/// The size of a skill folder made by [`with_skill`] for `name`.
fn skill_size(name: &str) -> u64 {
    (skill_md(name).len() + "Write-Output run".len()) as u64
}

#[test]
fn remove_of_a_skill_folder_moves_it_into_the_backup_store() {
    let fs = with_skill(claude_home(), claude_skill("deploy"), "deploy");
    let core = core(&fs);

    let plan = remove(&core, &claude_skill("deploy").display().to_string());

    let preview = serde_json::to_value(plan.preview()).unwrap();
    assert_eq!(preview["title"], "Remove 1 Entry");
    assert_eq!(preview["files"], json!([]));
    assert_eq!(
        preview["folders"],
        json!([{
            "path": r"C:\Users\fixture\.claude\skills\deploy",
            "change": "moved_to_backup",
            "target": null,
            "files": 2,
            "size": skill_size("deploy"),
            "git_versioned": false,
            "sentence": r"Moves the skill folder C:\Users\fixture\.claude\skills\deploy (2 files, 80 bytes) into the backup."
        }])
    );
    assert_eq!(preview["changes"][0]["before"], "effective");
    assert_eq!(preview["changes"][0]["after"], "removed");

    let outcome = core.execute(plan, &[]).unwrap();

    assert_eq!(code(outcome.verify), "verified");
    assert_eq!(
        fs.files_below(claude_skill("deploy")),
        Vec::<PathBuf>::new()
    );
    let store = outcome.backup.join("1-deploy");
    assert_eq!(
        fs.contents(store.join("SKILL.md")).unwrap(),
        skill_md("deploy").as_bytes()
    );
    assert!(fs.contents(store.join(r"scripts\run.ps1")).is_some());
    let history = core.history_show(&outcome.operation).unwrap();
    assert_eq!(history.summary.locations, [claude_skill("deploy")]);
    assert_eq!(history.locations[0].backup, Some(store));
}

#[test]
fn remove_of_a_linked_entry_removes_the_link_and_never_its_target() {
    let fs = junctioned_deploy();
    let core = core(&fs);

    let plan = remove(&core, &claude_skill("deploy").display().to_string());

    let preview = serde_json::to_value(plan.preview()).unwrap();
    assert_eq!(preview["folders"][0]["change"], "link_removed");
    assert_eq!(
        preview["folders"][0]["target"],
        r"C:\Users\fixture\.agents\skills\deploy"
    );
    assert_eq!(
        preview["folders"][0]["sentence"],
        r"Removes the link C:\Users\fixture\.claude\skills\deploy only; its target C:\Users\fixture\.agents\skills\deploy (2 files, 80 bytes) stays as it is."
    );
    // Only the link's own Entry changes: its target stays Effective.
    let changed: Vec<(&str, &str)> = plan
        .preview()
        .changes
        .iter()
        .map(|change| (change.agent.as_str(), change.locator.as_str()))
        .collect();
    assert_eq!(
        changed,
        [("claude-code", r"C:\Users\fixture\.claude\skills\deploy")]
    );
    assert!(plan.warnings().is_empty(), "{:?}", plan.warnings());

    let outcome = core.execute(plan, &[]).unwrap();

    assert_eq!(code(outcome.verify), "verified");
    assert!(!fs.is_link(claude_skill("deploy")));
    assert_eq!(
        fs.contents(agents_skill("deploy").join("SKILL.md"))
            .unwrap(),
        skill_md("deploy").as_bytes()
    );
    assert_eq!(
        fs.files_below(outcome.backup.join("1-deploy")),
        Vec::<PathBuf>::new()
    );
    let after = inventory(&core);
    assert!(!after.items.iter().any(|item| item.agent == "claude-code"));
    assert_eq!(
        code(skill(&after, "agents-reader", "deploy").state),
        "effective"
    );
}

#[test]
fn the_broken_links_warning_says_first_to_remove_the_link_and_gives_the_folder_and_link_apart() {
    let fs = junctioned_deploy();
    let core = core(&fs);

    let plan = remove(&core, &agents_skill("deploy").display().to_string());

    assert_eq!(
        plan.warnings()[0].advice,
        [Advice {
            todo: "Remove the link on its own once this is applied.".to_owned(),
            why: "Removing the first location below leaves the link to it pointing to nothing, as an Unread Entry.".to_owned(),
            paths: vec![agents_skill("deploy"), claude_skill("deploy")],
        }]
    );
}

#[test]
fn remove_of_a_link_target_warns_of_the_links_it_leaves_broken() {
    let fs = junctioned_deploy();
    let core = core(&fs);

    let plan = remove(&core, &agents_skill("deploy").display().to_string());

    assert_eq!(plan.preview().title, "Remove 1 Entry");
    let warnings: Vec<(String, &str)> = plan
        .warnings()
        .iter()
        .map(|warning| (code(warning.class), warning.sentence.as_str()))
        .collect();
    assert_eq!(
        warnings,
        [(
            "breaks-links".to_owned(),
            r"Removing C:\Users\fixture\.agents\skills\deploy leaves the link C:\Users\fixture\.claude\skills\deploy pointing to nothing. It stays, as an Unread Entry, until it is Removed on its own."
        )]
    );
    // The link only changes State: it is not Removed with its target.
    let changes: Vec<(&str, String, String)> = plan
        .preview()
        .changes
        .iter()
        .map(|change| {
            (
                change.agent.as_str(),
                code(change.before),
                code(change.after),
            )
        })
        .collect();
    assert_eq!(
        changes,
        [
            ("claude-code", "effective".to_owned(), "unread".to_owned()),
            (
                "agents-reader",
                "effective".to_owned(),
                "removed".to_owned()
            ),
        ]
    );
    assert_eq!(plan.preview().folders.len(), 1);
    let refused = core.execute(plan, &[]).unwrap_err();
    assert_eq!(code(refused.code), "ack_required");

    let plan = remove(&core, &agents_skill("deploy").display().to_string());
    let outcome = core.execute(plan, &[WarningClass::BreaksLinks]).unwrap();

    assert_eq!(code(outcome.verify), "verified");
    assert!(fs.is_link(claude_skill("deploy")));
    let after = inventory(&core);
    let broken = skill(&after, "claude-code", "deploy");
    assert_eq!(code(broken.state), "unread");
    assert!(broken.entries[0].link.as_ref().unwrap().broken);
}

#[test]
fn a_broken_link_has_its_own_remove() {
    let fs = claude_home()
        .with_folder(in_home(r".claude\skills"))
        .with_link(claude_skill("deploy"), agents_skill("deploy"));
    let core = core(&fs);

    let plan = remove(&core, "deploy");

    assert_eq!(
        plan.preview().folders[0].change,
        config_digger_core::api::FolderChangeKind::LinkRemoved
    );
    assert_eq!(code(plan.preview().changes[0].before), "unread");
    let outcome = core.execute(plan, &[]).unwrap();
    assert_eq!(code(outcome.verify), "verified");
    assert!(!fs.is_link(claude_skill("deploy")));
}

#[test]
fn a_backup_copy_that_differs_fails_verify_and_the_folder_is_moved_back() {
    let fs = with_skill(claude_home(), claude_skill("deploy"), "deploy")
        .corrupting_writes_below(BACKUPS);
    let core = core(&fs);
    let plan = remove(&core, "deploy");

    let error = core.execute(plan, &[]).unwrap_err();

    assert_eq!(code(error.code), "verify_failed");
    assert!(
        error.message.contains(
            r"the copy of C:\Users\fixture\.claude\skills\deploy in the backup does not hold its 2 files, 80 bytes"
        ),
        "{}",
        error.message
    );
    let rollback = &error.failed.as_ref().unwrap().rollback;
    assert_eq!(rollback[0].path, claude_skill("deploy"));
    assert_eq!(code(rollback[0].outcome), "restored");
    assert!(
        fs.contents(claude_skill("deploy").join("SKILL.md"))
            .is_some()
    );
}

/// Executes `plan` with nothing to acknowledge.
fn execute(core: &Core, plan: Plan) -> Outcome {
    core.execute(plan, &[]).unwrap()
}

#[test]
fn undo_of_a_folder_remove_moves_the_folder_back_from_the_backup() {
    let fs = with_skill(claude_home(), claude_skill("deploy"), "deploy");
    let core = core(&fs);
    let removed = execute(&core, remove(&core, "deploy"));

    let undo = core.plan_undo(&removed.operation).unwrap();

    let preview = serde_json::to_value(undo.preview()).unwrap();
    assert_eq!(preview["folders"][0]["change"], "moved_back");
    assert_eq!(
        preview["folders"][0]["sentence"],
        r"Moves the skill folder C:\Users\fixture\.claude\skills\deploy (2 files, 80 bytes) back from the backup."
    );
    assert_eq!(preview["changes"][0]["before"], "removed");
    assert_eq!(preview["changes"][0]["after"], "effective");
    let undone = execute(&core, undo);
    assert_eq!(code(undone.verify), "verified");
    assert_eq!(
        fs.contents(claude_skill("deploy").join("SKILL.md"))
            .unwrap(),
        skill_md("deploy").as_bytes()
    );
    assert_eq!(
        fs.files_below(removed.backup.join("1-deploy")),
        Vec::<PathBuf>::new()
    );
    assert_eq!(
        code(skill(&inventory(&core), "claude-code", "deploy").state),
        "effective"
    );
    // The Undo is itself undone by moving the folder away again.
    let again = execute(&core, core.plan_undo(&undone.operation).unwrap());
    assert_eq!(code(again.verify), "verified");
    assert_eq!(
        fs.files_below(claude_skill("deploy")),
        Vec::<PathBuf>::new()
    );
}

#[test]
fn undo_of_a_linked_entry_remove_creates_the_link_again() {
    let fs = junctioned_deploy();
    let core = core(&fs);
    let removed = execute(
        &core,
        remove(&core, &claude_skill("deploy").display().to_string()),
    );

    let undo = core.plan_undo(&removed.operation).unwrap();

    assert_eq!(
        serde_json::to_value(undo.preview()).unwrap()["folders"][0]["sentence"],
        r"Creates the link C:\Users\fixture\.claude\skills\deploy to C:\Users\fixture\.agents\skills\deploy again."
    );
    let undone = execute(&core, undo);
    assert_eq!(code(undone.verify), "verified");
    assert!(fs.is_link(claude_skill("deploy")));
    let after = inventory(&core);
    assert_eq!(
        code(skill(&after, "claude-code", "deploy").state),
        "effective"
    );
}

#[test]
fn undo_of_a_folder_remove_is_refused_once_something_is_there_again() {
    let fs = with_skill(claude_home(), claude_skill("deploy"), "deploy");
    let core = core(&fs);
    let removed = execute(&core, remove(&core, "deploy"));
    fs.put_file(claude_skill("deploy").join("SKILL.md"), skill_md("deploy"));

    let refused = core.plan_undo(&removed.operation).err().unwrap();

    assert_eq!(code(refused.code), "undo_refused");
    assert!(
        refused.message.starts_with(
            r"something is at C:\Users\fixture\.claude\skills\deploy again since the Operation, so the Undo is refused"
        ),
        "{}",
        refused.message
    );
    let history = core.history().unwrap();
    assert!(!history[0].undoable);
}

#[test]
fn deleting_the_backup_of_a_folder_remove_deletes_the_moved_folder() {
    let fs = with_skill(claude_home(), claude_skill("deploy"), "deploy");
    let core = core(&fs);
    let removed = execute(&core, remove(&core, "deploy"));
    let store = removed.backup.join("1-deploy");

    let plan = core.plan_delete_backup(&removed.operation).unwrap();

    assert_eq!(plan.preview().deleted, std::slice::from_ref(&store));
    execute(&core, plan);
    assert_eq!(fs.files_below(&store), Vec::<PathBuf>::new());
    assert!(!core.history().unwrap()[0].undoable);
}

#[test]
fn a_link_to_a_link_is_removed_as_it_holds_it_and_undone_to_the_same_target() {
    // `~/.claude/skills/deploy` → `D:\dev\deploy` → `~/.agents/skills/deploy`.
    let dev = PathBuf::from(r"D:\dev\deploy");
    let fs = with_skill(claude_home(), agents_skill("deploy"), "deploy")
        .with_folder(in_home(r".claude\skills"))
        .with_folder(r"D:\dev")
        .with_link(&dev, agents_skill("deploy"))
        .with_link(claude_skill("deploy"), &dev);
    let core = core(&fs);
    let before = inventory(&core);
    let link = before
        .items
        .iter()
        .flat_map(|item| &item.entries)
        .find(|entry| entry.path == claude_skill("deploy"))
        .and_then(|entry| entry.link.clone())
        .unwrap();
    assert_eq!(link.target, dev);
    // Its real target is in `.agents/skills`, where `npx skills` keeps it.
    let manager = before
        .items
        .iter()
        .flat_map(|item| &item.entries)
        .find(|entry| entry.path == claude_skill("deploy"))
        .and_then(|entry| entry.skill_manager.clone())
        .unwrap();
    assert_eq!(manager.id, "npx-skills");

    let removed = execute(
        &core,
        remove(&core, &claude_skill("deploy").display().to_string()),
    );

    assert_eq!(code(removed.verify), "verified");
    assert!(!fs.is_link(claude_skill("deploy")));
    assert!(fs.is_link(&dev));
    execute(&core, core.plan_undo(&removed.operation).unwrap());
    assert!(fs.is_link(claude_skill("deploy")));
    let after = inventory(&core);
    let again = after
        .items
        .iter()
        .flat_map(|item| &item.entries)
        .find(|entry| entry.path == claude_skill("deploy"))
        .unwrap();
    assert_eq!(again.link.as_ref().unwrap().target, dev);
}
