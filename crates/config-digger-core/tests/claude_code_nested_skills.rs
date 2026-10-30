//! Claude Code's nested skills through the core `api`, with fake ports over
//! fixture Projects: the `.claude\skills` folders below the Project folder,
//! which Claude Code loads once it works on a file under them, the folders
//! git leaves out, how they are named, and Disable through
//! `skillOverrides`.

use config_digger_core::api::{Context, Core, Inventory, Item, ScanOptions};
use config_digger_core::ports::fake::{self, FakeEnvironment, FakeFileSystem};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const HOME: &str = r"C:\Users\fixture";
const PROJECT: &str = r"E:\repo";

/// A `SKILL.md` whose frontmatter names the skill `name`.
fn skill_md(name: &str) -> String {
    format!("---\nname: {name}\ndescription: The {name} skill.\n---\n\nDo {name}.\n")
}

/// `fs` with a skill folder `name` in the `.claude\skills` of the folder
/// `folder`, relative to the Project.
fn with_skill(fs: FakeFileSystem, folder: &str, name: &str) -> FakeFileSystem {
    let path = Path::new(PROJECT)
        .join(folder)
        .join(r".claude\skills")
        .join(name)
        .join("SKILL.md");
    fs.with_file(path, skill_md(name))
}

/// A Claude Code home, and the Project as a git repo.
fn repo() -> FakeFileSystem {
    FakeFileSystem::new()
        .with_file(PathBuf::from(HOME).join(".claude.json"), "{}")
        .with_folder(Path::new(PROJECT).join(".git"))
}

fn inventory(fs: &FakeFileSystem) -> Inventory {
    let env = FakeEnvironment::with_home(HOME);
    let core = Core::new(fake::ports(fs.clone(), env)).unwrap();
    let context = Context {
        project: Some(PathBuf::from(PROJECT)),
    };
    let snapshot = core
        .scan(&ScanOptions {
            projects: vec![PathBuf::from(PROJECT)],
        })
        .unwrap();
    core.resolve(&snapshot, &context)
}

/// The skill Items, by name.
fn skills(inventory: &Inventory) -> Vec<&Item> {
    inventory
        .items
        .iter()
        .filter(|item| json!(item.kind) == "skill")
        .collect()
}

fn skill<'i>(inventory: &'i Inventory, name: &str) -> &'i Item {
    skills(inventory)
        .into_iter()
        .find(|item| item.name == name)
        .unwrap_or_else(|| panic!("no skill `{name}`"))
}

fn names(inventory: &Inventory) -> Vec<&str> {
    let mut names: Vec<&str> = skills(inventory)
        .iter()
        .map(|item| item.name.as_str())
        .collect();
    names.sort_unstable();
    names
}

fn entry_json(item: &Item) -> Value {
    assert_eq!(item.entries.len(), 1, "{}: {:?}", item.name, item.entries);
    serde_json::to_value(&item.entries[0]).unwrap()
}

#[test]
fn a_skill_below_the_project_is_an_effective_project_entry_noting_the_folder_it_waits_for() {
    let fs = with_skill(repo(), r"apps\web", "web");
    let fs = with_skill(fs, r"apps\web\src\deep", "deep");

    let inventory = inventory(&fs);

    assert_eq!(names(&inventory), ["deep", "web"]);
    for (name, folder) in [("web", r"apps\web"), ("deep", r"apps\web\src\deep")] {
        let entry = entry_json(skill(&inventory, name));
        assert_eq!(
            entry["path"],
            format!(r"{PROJECT}\{folder}\.claude\skills\{name}")
        );
        assert_eq!(entry["scope"], "project");
        assert_eq!(entry["state"], "effective");
        assert_eq!(
            entry["notes"],
            json!([{
                "code": "loaded_on_file_under_folder",
                "switch": null,
                "sentence": format!("Claude Code loads it only once it reads or edits a file under `{folder}` in the Project."),
            }])
        );
    }
}

#[test]
fn a_folder_the_gitignore_leaves_out_holds_no_entry_and_the_project_folder_own_skills_are_read_once()
 {
    let fs = with_skill(repo(), ".", "own");
    let fs = with_skill(fs, r"apps\gen", "generated");
    let fs = with_skill(fs, r"apps\web", "web");
    let fs = fs.with_file(Path::new(PROJECT).join(".gitignore"), "apps/gen/\n");

    let inventory = inventory(&fs);

    assert_eq!(names(&inventory), ["own", "web"]);
    assert_eq!(entry_json(skill(&inventory, "own"))["notes"], json!([]));
}

#[test]
fn only_a_gitignore_leaving_out_the_folder_that_holds_the_claude_folder_counts() {
    // Claude Code asks git whether it ignores the folder holding `.claude`:
    // an `.ignore` file, which git never reads, and a `.gitignore` leaving
    // out the `.claude` folder alone do not hide the skills.
    let fs = with_skill(repo(), r"apps\web", "web");
    let fs = with_skill(fs, r"apps\api", "api");
    let fs = fs
        .with_file(Path::new(PROJECT).join(r"apps\.ignore"), "web/\n")
        .with_file(
            Path::new(PROJECT).join(r"apps\api\.gitignore"),
            ".claude/\n",
        );

    let inventory = inventory(&fs);

    assert_eq!(names(&inventory), ["api", "web"]);
}

/// `(name, scope, state)` per skill Item, of its one Entry.
fn named_states(inventory: &Inventory) -> Vec<(String, String, String)> {
    let mut states: Vec<(String, String, String)> = skills(inventory)
        .iter()
        .map(|item| {
            let entry = entry_json(item);
            (
                item.name.clone(),
                entry["scope"].as_str().unwrap().to_owned(),
                entry["state"].as_str().unwrap().to_owned(),
            )
        })
        .collect();
    states.sort();
    states
}

fn triples(triples: &[(&str, &str, &str)]) -> Vec<(String, String, String)> {
    triples
        .iter()
        .map(|(a, b, c)| ((*a).to_owned(), (*b).to_owned(), (*c).to_owned()))
        .collect()
}

#[test]
fn a_nested_skill_competes_with_none_and_takes_its_folder_as_a_prefix_when_another_skill_has_its_name()
 {
    let fs = with_skill(repo(), ".", "deploy");
    let fs = with_skill(fs, r"apps\web", "deploy");
    let fs = fs.with_file(
        PathBuf::from(HOME).join(r".claude\skills\mine\SKILL.md"),
        skill_md("mine"),
    );
    let fs = with_skill(fs, r"apps\web", "mine");
    let fs = with_skill(fs, r"apps\web", "twin");
    let fs = with_skill(fs, r"apps\web\src\deep", "twin");
    // A frontmatter `name` is an alias, which no nested skill clashes with.
    let fs = fs.with_file(
        Path::new(PROJECT).join(r".claude\skills\ship\SKILL.md"),
        skill_md("alias"),
    );
    let fs = with_skill(fs, r"apps\web", "alias");

    let inventory = inventory(&fs);

    assert_eq!(
        named_states(&inventory),
        triples(&[
            ("alias", "project", "effective"),
            ("apps/web/src/deep:twin", "project", "effective"),
            ("apps/web:deploy", "project", "effective"),
            ("apps/web:mine", "project", "effective"),
            ("apps/web:twin", "project", "effective"),
            ("deploy", "project", "effective"),
            ("mine", "user", "effective"),
            ("ship", "project", "effective"),
        ])
    );
    let nested = entry_json(skill(&inventory, "apps/web:deploy"));
    assert_eq!(
        nested["path"],
        format!(r"{PROJECT}\apps\web\.claude\skills\deploy")
    );
    assert_eq!(nested["notes"][0]["code"], "loaded_on_file_under_folder");
}

/// `fs` with the Project's local settings holding `overrides` as its
/// `skillOverrides`.
fn with_overrides(fs: FakeFileSystem, overrides: Value) -> FakeFileSystem {
    fs.with_file(
        Path::new(PROJECT).join(r".claude\settings.local.json"),
        json!({ "skillOverrides": overrides }).to_string(),
    )
}

fn state(inventory: &Inventory, name: &str) -> String {
    entry_json(skill(inventory, name))["state"]
        .as_str()
        .unwrap()
        .to_owned()
}

#[test]
fn skill_overrides_turn_a_nested_skill_off_by_its_name_and_a_prefixed_one_by_its_folder_name_too() {
    let fs = with_skill(repo(), ".", "deploy");
    let fs = with_skill(fs, r"apps\web", "deploy");
    let fs = with_skill(fs, r"apps\web", "web");

    // The fake file system's clones share its files.
    let none = inventory(&fs);
    let by_name = inventory(&with_overrides(
        fs.clone(),
        json!({ "apps/web:deploy": "off", "web": "off" }),
    ));
    let by_folder = inventory(&with_overrides(fs, json!({ "deploy": "off" })));

    assert_eq!(
        [
            ("apps/web:deploy", &by_name),
            ("deploy", &by_name),
            ("web", &by_name)
        ]
        .map(|(name, inventory)| state(inventory, name)),
        ["disabled", "effective", "disabled"]
    );
    assert_eq!(
        ["apps/web:deploy", "deploy"].map(|name| state(&by_folder, name)),
        ["disabled", "disabled"]
    );
    let offered = entry_json(skill(&none, "apps/web:deploy"));
    assert_eq!(
        offered["disable"]["sentence"],
        format!(
            r#"Disable sets `skillOverrides.apps/web:deploy` to "off" in {PROJECT}\.claude\settings.local.json for this Project."#
        )
    );
}

#[test]
fn outside_a_git_repo_no_nested_skill_is_read() {
    // Claude Code loads them there too, but reading them would walk every
    // folder below a Project that may be a home folder or a drive.
    let fs = FakeFileSystem::new()
        .with_file(PathBuf::from(HOME).join(".claude.json"), "{}")
        .with_folder(PROJECT);
    let fs = with_skill(fs, ".", "own");
    let fs = with_skill(fs, r"apps\web", "web");

    let inventory = inventory(&fs);

    assert_eq!(names(&inventory), ["own"]);
}
