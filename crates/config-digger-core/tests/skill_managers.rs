//! Skill managers through the core `api`, with a fake file system that
//! models junctions: the skill manager definitions that ship recognise
//! skills installed by `npx skills`, skillfish and openskills, the
//! strongest detector wins, a weak one notes that it rests on the name, and
//! Remove of one managed skill touches that folder alone.

use config_digger_core::api::{
    Action, Context, Core, Entry, Inventory, Item, NoteCode, ScanOptions, Targets,
};
use config_digger_core::ports::fake::{self, FakeEnvironment, FakeFileSystem};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const HOME: &str = r"C:\Users\fixture";
const LOCAL_APP_DATA: &str = r"C:\Users\fixture\AppData\Local";

fn in_home(relative: &str) -> PathBuf {
    PathBuf::from(HOME).join(relative)
}

fn claude_skill(name: &str) -> PathBuf {
    in_home(r".claude\skills").join(name)
}

fn agents_skill(name: &str) -> PathBuf {
    in_home(r".agents\skills").join(name)
}

fn skill_md(name: &str) -> String {
    format!("---\nname: {name}\ndescription: The {name} skill.\n---\n\nDo {name}.\n")
}

fn with_skill(fs: FakeFileSystem, folder: impl AsRef<Path>, name: &str) -> FakeFileSystem {
    fs.with_file(folder.as_ref().join("SKILL.md"), skill_md(name))
}

fn claude_home() -> FakeFileSystem {
    FakeFileSystem::new().with_file(in_home(".claude.json"), "{}")
}

/// An Agent that reads skills from `~/.agents/skills`, as Codex does.
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

fn skill<'i>(inventory: &'i Inventory, agent: &str, name: &str) -> &'i Item {
    inventory
        .items
        .iter()
        .find(|item| item.agent == agent && item.name == name)
        .unwrap_or_else(|| panic!("no skill `{name}` of {agent}"))
}

/// The only Entry of Claude Code's skill `name`.
fn claude_entry(fs: &FakeFileSystem, name: &str) -> Entry {
    skill(&inventory(&core(fs)), "claude-code", name).entries[0].clone()
}

fn entry_json(entry: &Entry) -> Value {
    serde_json::to_value(entry).unwrap()
}

/// `~/.agents/.skill-lock.json` as `npx skills` writes it, with `names`.
fn skill_lock(names: &[&str]) -> String {
    let skills: serde_json::Map<String, Value> = names
        .iter()
        .map(|name| {
            let entry = json!({ "source": "acme/skills", "sourceType": "github" });
            ((*name).to_owned(), entry)
        })
        .collect();
    json!({ "version": 3, "skills": skills }).to_string()
}

#[test]
fn a_junction_into_agents_skills_is_installed_by_npx_skills() {
    let fs = with_skill(claude_home(), agents_skill("deploy"), "deploy")
        .with_folder(in_home(r".claude\skills"))
        .with_link(claude_skill("deploy"), agents_skill("deploy"));

    let entry = claude_entry(&fs, "deploy");

    let json = entry_json(&entry);
    assert_eq!(json["provenance"], "skill_manager");
    assert_eq!(
        json["skill_manager"],
        json!({ "id": "npx-skills", "name": "npx skills" })
    );
    assert_eq!(json["notes"], json!([]));
}

#[test]
fn a_skillfish_or_openskills_marker_names_its_manager() {
    let fs = with_skill(claude_home(), claude_skill("compose"), "compose")
        .with_file(claude_skill("compose").join(".skillfish.json"), "{}");
    let fs = with_skill(fs, claude_skill("pdf"), "pdf")
        .with_file(claude_skill("pdf").join(".openskills.json"), "{}");

    let inventory = inventory(&core(&fs));

    let manager = |name: &str| {
        let entry = &skill(&inventory, "claude-code", name).entries[0];
        entry_json(entry)["skill_manager"]["id"].clone()
    };
    assert_eq!(manager("compose"), "skillfish");
    assert_eq!(manager("pdf"), "openskills");
}

#[test]
fn a_skill_named_in_the_lock_file_alone_is_npx_skills_by_name_only() {
    let fs = with_skill(claude_home(), claude_skill("ce-review"), "ce-review").with_file(
        in_home(r".agents\.skill-lock.json"),
        skill_lock(&["ce:review"]),
    );

    let entry = claude_entry(&fs, "ce-review");

    assert_eq!(entry_json(&entry)["skill_manager"]["id"], "npx-skills");
    let notes: Vec<(NoteCode, &str)> = entry
        .notes
        .iter()
        .map(|note| (note.code, note.sentence.as_str()))
        .collect();
    assert_eq!(
        notes,
        [(
            NoteCode::ProvenanceByNameOnly,
            "config-digger takes it to be installed by npx skills from its name or where it is alone, which a folder made by hand matches too."
        )]
    );
    assert_eq!(
        entry_json(&entry)["notes"][0]["code"],
        "provenance_by_name_only"
    );
}

#[test]
fn the_strongest_detector_wins() {
    // The lock names it, which is weak; the skillfish marker is strong.
    let fs = with_skill(claude_home(), claude_skill("compose"), "compose")
        .with_file(claude_skill("compose").join(".skillfish.json"), "{}")
        .with_file(
            in_home(r".agents\.skill-lock.json"),
            skill_lock(&["compose"]),
        );

    let entry = claude_entry(&fs, "compose");

    assert_eq!(entry_json(&entry)["skill_manager"]["id"], "skillfish");
    assert_eq!(entry_json(&entry)["notes"], json!([]));
}

#[test]
fn a_folder_in_agents_skills_is_npx_skills_by_location_only() {
    let fs = with_skill(FakeFileSystem::new(), agents_skill("deploy"), "deploy");

    let inventory = inventory(&core(&fs));

    let entry = &skill(&inventory, "agents-reader", "deploy").entries[0];
    assert_eq!(entry_json(entry)["skill_manager"]["id"], "npx-skills");
    assert_eq!(entry.notes[0].code, NoteCode::ProvenanceByNameOnly);
}

#[test]
fn a_skill_no_detector_matches_is_a_file_entry() {
    let fs = with_skill(claude_home(), claude_skill("deploy"), "deploy")
        .with_file(in_home(r".agents\.skill-lock.json"), skill_lock(&["other"]));

    let entry = claude_entry(&fs, "deploy");

    assert_eq!(entry_json(&entry)["provenance"], "file");
    assert_eq!(entry_json(&entry)["skill_manager"], Value::Null);
}

#[test]
fn remove_of_one_npx_skills_skill_touches_that_folder_alone() {
    // `skills remove deploy` would delete both folders and edit the lock.
    let lock = skill_lock(&["deploy"]);
    let fs = with_skill(FakeFileSystem::new(), agents_skill("deploy"), "deploy")
        .with_file(in_home(r".agents\.skill-lock.json"), lock.clone());
    let fs = with_skill(fs, claude_skill("deploy"), "deploy");
    let core = core(&fs);
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    let targets = Targets {
        selectors: vec![agents_skill("deploy").display().to_string()],
        all: false,
    };
    let plan = core
        .plan(&snapshot, &Context::default(), Action::Remove, &targets)
        .unwrap();

    assert_eq!(plan.preview().folders.len(), 1);
    assert_eq!(plan.preview().folders[0].path, agents_skill("deploy"));
    core.execute(plan, &[]).unwrap();

    assert!(
        fs.contents(agents_skill("deploy").join("SKILL.md"))
            .is_none()
    );
    assert!(
        fs.contents(claude_skill("deploy").join("SKILL.md"))
            .is_some()
    );
    assert_eq!(
        fs.contents(in_home(r".agents\.skill-lock.json")).unwrap(),
        lock.as_bytes()
    );
}
