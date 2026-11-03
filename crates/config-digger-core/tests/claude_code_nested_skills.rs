//! Claude Code's nested skills through the core `api`, with fake ports over
//! fixture Projects: the `.claude\skills` folders below the Project folder,
//! which Claude Code loads once it works on a file under them, the folders
//! git leaves out, how they are named, and Disable through
//! `skillOverrides`.

use config_digger_core::api::{Action, Context, Core, Inventory, Item, ScanOptions, Targets};
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

/// A Claude Code home in the user's home folder.
fn claude_code_home() -> FakeFileSystem {
    FakeFileSystem::new().with_file(PathBuf::from(HOME).join(".claude.json"), "{}")
}

/// A Claude Code home, and the Project as a git repo.
fn repo() -> FakeFileSystem {
    claude_code_home().with_folder(Path::new(PROJECT).join(".git"))
}

fn inventory(fs: &FakeFileSystem) -> Inventory {
    inventory_at(fs, PROJECT)
}

/// The inventory with the Project at `project` as the context.
fn inventory_at(fs: &FakeFileSystem, project: &str) -> Inventory {
    let env = FakeEnvironment::with_home(HOME);
    let core = Core::new(fake::ports(fs.clone(), env)).unwrap();
    let snapshot = core
        .scan(&ScanOptions {
            projects: vec![PathBuf::from(project)],
        })
        .unwrap();
    core.resolve(
        &snapshot,
        &Context {
            project: Some(PathBuf::from(project)),
        },
    )
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
fn a_nested_skill_on_under_its_prefixed_name_is_effective_while_its_folder_name_turns_the_project_s_skill_off()
 {
    let fs = with_skill(repo(), ".", "deploy");
    let fs = with_skill(fs, r"apps\web", "deploy");

    for overrides in [
        json!({ "apps/web:deploy": "on", "deploy": "off" }),
        json!({ "deploy": "off", "apps/web:deploy": "on" }),
    ] {
        let inventory = inventory(&with_overrides(fs.clone(), overrides.clone()));

        assert_eq!(
            ["apps/web:deploy", "deploy"].map(|name| state(&inventory, name)),
            ["effective", "disabled"],
            "{overrides}"
        );
    }
}

/// A Claude Code home, and the Project folder in no git work tree.
fn outside_git() -> FakeFileSystem {
    claude_code_home().with_folder(PROJECT)
}

/// The notes of the one Project of `inventory`.
fn project_notes(inventory: &Inventory) -> Value {
    assert_eq!(inventory.projects.len(), 1, "{:?}", inventory.projects);
    json!(inventory.projects[0].notes)
}

#[test]
fn outside_a_git_repo_a_skill_below_the_project_is_read_as_in_a_git_work_tree() {
    // The Project's own skill of its name makes the nested one go by
    // `{subfolder}:{name}`, as in a git work tree.
    let fs = with_skill(outside_git(), ".", "web");
    let fs = with_skill(fs, r"apps\web", "web");

    let inventory = inventory(&fs);

    assert_eq!(names(&inventory), ["apps/web:web", "web"]);
    let entry = entry_json(skill(&inventory, "apps/web:web"));
    assert_eq!(
        entry["path"],
        format!(r"{PROJECT}\apps\web\.claude\skills\web")
    );
    assert_eq!(entry["scope"], "project");
    assert_eq!(entry["state"], "effective");
    assert_eq!(
        entry["notes"],
        json!([{
            "code": "loaded_on_file_under_folder",
            "switch": null,
            "sentence": "Claude Code loads it only once it reads or edits a file under `apps\\web` in the Project.",
        }])
    );
    assert_eq!(project_notes(&inventory), json!([]));
}

#[test]
fn outside_a_git_repo_a_gitignore_in_the_project_folder_hides_a_folder_as_in_a_git_work_tree() {
    let fs = with_skill(outside_git(), r"apps\web", "web");
    let fs = with_skill(fs, r"tools\lint", "lint");
    let fs = fs.with_file(Path::new(PROJECT).join(".gitignore"), "apps\n");

    let inventory = inventory(&fs);

    assert_eq!(names(&inventory), ["lint"]);
}

/// The note of a Project whose nested skills folders Claude Code loads and
/// config-digger does not read.
fn left_out_note() -> Value {
    json!([{
        "agent": "claude-code",
        "code": "walk_down_left_out",
        "sentence": "Claude Code also loads the `.claude\\skills` folders below this Project, but config-digger does not read them here: outside a git repo, that would walk every folder below a home folder or a drive.",
    }])
}

/// A Claude Code home, and a skill in `apps\web\.claude\skills` below the
/// folder `project`, in no git work tree.
fn skill_below(project: &str) -> FakeFileSystem {
    claude_code_home().with_file(
        Path::new(project).join(r"apps\web\.claude\skills\web\SKILL.md"),
        skill_md("web"),
    )
}

#[test]
fn outside_a_git_repo_the_user_home_folder_as_a_project_has_no_nested_skill_and_a_note_saying_why()
{
    let inventory = inventory_at(&skill_below(HOME), HOME);

    assert_eq!(names(&inventory), Vec::<&str>::new());
    assert_eq!(project_notes(&inventory), left_out_note());
}

#[test]
fn outside_a_git_repo_the_user_home_folder_spelled_in_another_case_is_left_out_too() {
    let inventory = inventory_at(&skill_below(HOME), r"c:\users\FIXTURE");

    assert_eq!(names(&inventory), Vec::<&str>::new());
    assert_eq!(project_notes(&inventory), left_out_note());
}

#[test]
fn outside_a_git_repo_a_folder_above_the_user_home_folder_as_a_project_is_left_out() {
    let inventory = inventory_at(&skill_below(r"C:\Users"), r"C:\Users");

    assert_eq!(names(&inventory), Vec::<&str>::new());
    assert_eq!(project_notes(&inventory), left_out_note());
}

#[test]
fn outside_a_git_repo_a_drive_root_as_a_project_is_left_out() {
    let inventory = inventory_at(&skill_below(r"E:\"), r"E:\");

    assert_eq!(names(&inventory), Vec::<&str>::new());
    assert_eq!(project_notes(&inventory), left_out_note());
}

#[test]
fn the_user_home_folder_in_a_git_work_tree_walks_as_any_project_and_has_no_note() {
    let fs = skill_below(HOME).with_folder(Path::new(HOME).join(".git"));

    let inventory = inventory_at(&fs, HOME);

    assert_eq!(names(&inventory), ["web"]);
    assert_eq!(project_notes(&inventory), json!([]));
}

#[test]
fn outside_a_git_repo_a_project_that_is_not_left_out_has_no_note_though_it_holds_no_nested_skill() {
    let inventory = inventory(&outside_git());

    assert_eq!(names(&inventory), Vec::<&str>::new());
    assert_eq!(project_notes(&inventory), json!([]));
}

// `skillOverrides` merged across the settings files (#530).

fn local_settings() -> PathBuf {
    Path::new(PROJECT).join(r".claude\settings.local.json")
}

fn user_settings() -> PathBuf {
    PathBuf::from(HOME).join(r".claude\settings.json")
}

/// `fs` with the user settings holding `overrides` as their
/// `skillOverrides`.
fn with_user_overrides(fs: FakeFileSystem, overrides: Value) -> FakeFileSystem {
    fs.with_file(
        user_settings(),
        json!({ "skillOverrides": overrides }).to_string(),
    )
}

/// The Project's skill `deploy` and the nested one listed as
/// `apps/web:deploy`.
fn two_deploys() -> FakeFileSystem {
    with_skill(with_skill(repo(), ".", "deploy"), r"apps\web", "deploy")
}

#[test]
fn of_a_nested_skill_s_two_keys_in_two_settings_files_its_prefixed_name_decides() {
    let cases = [
        // (local, user, the nested skill's State)
        (
            json!({ "deploy": "off" }),
            json!({ "apps/web:deploy": "on" }),
            "effective",
        ),
        (
            json!({ "apps/web:deploy": "on" }),
            json!({ "deploy": "off" }),
            "effective",
        ),
        (
            json!({ "deploy": "on" }),
            json!({ "apps/web:deploy": "off" }),
            "disabled",
        ),
        (
            json!({ "apps/web:deploy": "off" }),
            json!({ "deploy": "on" }),
            "disabled",
        ),
    ];
    for (local, user, expected) in cases {
        let fs = with_user_overrides(with_overrides(two_deploys(), local.clone()), user.clone());
        assert_eq!(
            state(&inventory(&fs), "apps/web:deploy"),
            expected,
            "local {local}, user {user}"
        );
    }
}

#[test]
fn disable_of_a_nested_skill_on_in_the_user_settings_beside_its_folder_name_off_locally_writes_its_listed_name_off_and_undo_restores_it()
 {
    let fs = with_user_overrides(
        with_overrides(two_deploys(), json!({ "deploy": "off" })),
        json!({ "apps/web:deploy": "on" }),
    );
    assert_eq!(state(&inventory(&fs), "apps/web:deploy"), "effective");
    let core = Core::new(fake::ports(fs.clone(), FakeEnvironment::with_home(HOME))).unwrap();
    let context = Context {
        project: Some(PathBuf::from(PROJECT)),
    };
    let snapshot = core
        .scan(&ScanOptions {
            projects: vec![PathBuf::from(PROJECT)],
        })
        .unwrap();
    let targets = Targets {
        selectors: vec!["apps/web:deploy".to_owned()],
        ..Targets::default()
    };

    let before = settings_values(&fs);

    let disable = core
        .plan(&snapshot, &context, Action::Disable, &targets)
        .unwrap();
    let outcome = core.execute(disable, &[]).unwrap();

    let local: Value = serde_json::from_slice(&fs.contents(local_settings()).unwrap()).unwrap();
    assert_eq!(
        local,
        json!({ "skillOverrides": { "deploy": "off", "apps/web:deploy": "off" } })
    );
    let after = inventory(&fs);
    assert_eq!(state(&after, "apps/web:deploy"), "disabled");
    assert_eq!(
        entry_json(skill(&after, "apps/web:deploy"))["reason"]["switch"],
        format!(
            r"{}#skillOverrides.apps/web:deploy",
            local_settings().display()
        )
    );

    core.execute(core.plan_undo(&outcome.operation).unwrap(), &[])
        .unwrap();
    assert_eq!(settings_values(&fs), before);
}

#[test]
fn enable_of_a_nested_skill_off_by_its_folder_name_in_the_user_settings_writes_its_listed_name_on_locally_and_undo_restores_it()
 {
    let fs = with_user_overrides(
        with_overrides(two_deploys(), json!({})),
        json!({ "deploy": "off" }),
    );
    assert_eq!(state(&inventory(&fs), "apps/web:deploy"), "disabled");
    let core = Core::new(fake::ports(fs.clone(), FakeEnvironment::with_home(HOME))).unwrap();
    let context = Context {
        project: Some(PathBuf::from(PROJECT)),
    };
    let snapshot = core
        .scan(&ScanOptions {
            projects: vec![PathBuf::from(PROJECT)],
        })
        .unwrap();
    let targets = Targets {
        selectors: vec!["apps/web:deploy".to_owned()],
        ..Targets::default()
    };

    let before = settings_values(&fs);

    let enable = core
        .plan(&snapshot, &context, Action::Enable, &targets)
        .unwrap();
    let outcome = core.execute(enable, &[]).unwrap();

    let local: Value = serde_json::from_slice(&fs.contents(local_settings()).unwrap()).unwrap();
    assert_eq!(
        local,
        json!({ "skillOverrides": { "apps/web:deploy": "on" } })
    );
    let after = inventory(&fs);
    assert_eq!(
        ["apps/web:deploy", "deploy"].map(|name| state(&after, name)),
        ["effective", "disabled"]
    );

    core.execute(core.plan_undo(&outcome.operation).unwrap(), &[])
        .unwrap();
    assert_eq!(settings_values(&fs), before);
}

/// What the Project's local settings and the user settings hold, as values:
/// Undo restores values, not bytes.
fn settings_values(fs: &FakeFileSystem) -> [Value; 2] {
    [local_settings(), user_settings()]
        .map(|path| serde_json::from_slice(&fs.contents(path).unwrap()).unwrap())
}
