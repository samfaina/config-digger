//! Claude Code's project skills in a linked git worktree, through the core
//! `api`, with fake ports: a main checkout at `E:\outer\main` and a linked
//! worktree of it at `E:\outer\wt`, whose `.git` file names
//! `E:\outer\main\.git\worktrees\wt`, as the release gate's probe builds
//! them with `git worktree add`.

use config_digger_core::api::{Context, Core, Inventory, Item, ScanOptions};
use config_digger_core::ports::fake::{self, FakeEnvironment, FakeFileSystem};
use serde_json::Value;
use std::path::{Path, PathBuf};

const HOME: &str = r"C:\Users\fixture";
const MAIN: &str = r"E:\outer\main";
const WORKTREE: &str = r"E:\outer\wt";

/// `.claude\skills\<name>` in `folder`.
fn skill_in(folder: &str, name: &str) -> PathBuf {
    Path::new(folder).join(r".claude\skills").join(name)
}

/// `fs` with a skill folder `name` in `folder`'s `.claude\skills`.
fn with_skill(fs: FakeFileSystem, folder: &str, name: &str) -> FakeFileSystem {
    fs.with_file(
        skill_in(folder, name).join("SKILL.md"),
        format!("---\nname: {name}\ndescription: {name} in {folder}\n---\n\nDo {name}.\n"),
    )
}

/// The main checkout, holding `.claude\skills\main-only`, and the worktree,
/// whose root has no `.claude\skills`.
fn repo() -> FakeFileSystem {
    let fs = FakeFileSystem::new()
        .with_file(Path::new(HOME).join(".claude.json"), "{}")
        .with_folder(Path::new(MAIN).join(r".git\worktrees\wt"))
        .with_file(
            Path::new(WORKTREE).join(".git"),
            "gitdir: E:/outer/main/.git/worktrees/wt\n",
        );
    with_skill(fs, MAIN, "main-only")
}

fn inventory_at(fs: &FakeFileSystem, project: &str) -> Inventory {
    let env = FakeEnvironment::with_home(HOME);
    let core = Core::new(fake::ports(fs.clone(), env)).unwrap();
    let project = PathBuf::from(project);
    let snapshot = core
        .scan(&ScanOptions {
            projects: vec![project.clone()],
        })
        .unwrap();
    core.resolve(
        &snapshot,
        &Context {
            project: Some(project),
        },
    )
}

/// The names of the skill Items, sorted.
fn names(inventory: &Inventory) -> Vec<&str> {
    let mut names: Vec<&str> = skills(inventory).map(|item| item.name.as_str()).collect();
    names.sort_unstable();
    names
}

fn skills(inventory: &Inventory) -> impl Iterator<Item = &Item> {
    inventory
        .items
        .iter()
        .filter(|item| serde_json::to_value(item.kind).unwrap() == "skill")
}

fn skill<'i>(inventory: &'i Inventory, name: &str) -> &'i Item {
    skills(inventory)
        .find(|item| item.name == name)
        .unwrap_or_else(|| panic!("no skill `{name}`"))
}

/// The JSON of each Entry of the skill `name`, in precedence order.
fn entries(inventory: &Inventory, name: &str) -> Vec<Value> {
    skill(inventory, name)
        .entries
        .iter()
        .map(|entry| serde_json::to_value(entry).unwrap())
        .collect()
}

#[test]
fn the_main_checkout_s_skills_stand_in_when_the_worktree_root_has_no_skills_folder() {
    let inventory = inventory_at(&repo(), WORKTREE);

    assert_eq!(names(&inventory), ["main-only"]);
    let [entry] = &entries(&inventory, "main-only")[..] else {
        panic!("one Entry");
    };
    assert_eq!(entry["path"], r"E:\outer\main\.claude\skills\main-only");
    assert_eq!(entry["scope"], "project");
    assert_eq!(entry["state"], "effective");
}

#[test]
fn a_worktree_root_with_its_own_skills_folder_reads_only_its_own() {
    let fs = with_skill(repo(), WORKTREE, "wt-only");

    let inventory = inventory_at(&fs, WORKTREE);

    assert_eq!(names(&inventory), ["wt-only"]);
    assert_eq!(
        entries(&inventory, "wt-only")[0]["path"],
        r"E:\outer\wt\.claude\skills\wt-only"
    );
}

#[test]
fn an_empty_skills_folder_at_the_worktree_root_keeps_the_main_checkout_s_out() {
    let fs = repo().with_folder(Path::new(WORKTREE).join(r".claude\skills"));

    let inventory = inventory_at(&fs, WORKTREE);

    assert!(names(&inventory).is_empty(), "{:?}", names(&inventory));
}

#[test]
fn a_claude_folder_without_skills_at_the_worktree_root_lets_the_main_checkout_s_stand_in() {
    let fs = repo().with_folder(Path::new(WORKTREE).join(".claude"));

    let inventory = inventory_at(&fs, WORKTREE);

    assert_eq!(names(&inventory), ["main-only"]);
}

#[test]
fn a_skills_folder_between_the_project_and_the_worktree_root_adds_to_the_main_checkout_s_and_wins_a_shared_name()
 {
    let sub = r"E:\outer\wt\sub";
    let fs = with_skill(repo(), MAIN, "shared");
    let fs = with_skill(fs, sub, "between");
    let fs = with_skill(fs, sub, "shared");

    let inventory = inventory_at(&fs, sub);

    assert_eq!(names(&inventory), ["between", "main-only", "shared"]);
    let shared = entries(&inventory, "shared");
    let paths_and_states: Vec<(&Value, &Value)> = shared
        .iter()
        .map(|entry| (&entry["path"], &entry["state"]))
        .collect();
    assert_eq!(
        paths_and_states,
        [
            (
                &Value::from(r"E:\outer\wt\sub\.claude\skills\shared"),
                &Value::from("effective")
            ),
            (
                &Value::from(r"E:\outer\main\.claude\skills\shared"),
                &Value::from("shadowed")
            ),
        ]
    );
}

#[test]
fn only_the_main_checkout_root_s_skills_stand_in_not_those_of_the_matching_folder_below_it() {
    let fs = with_skill(repo(), r"E:\outer\main\sub", "main-sub").with_folder(r"E:\outer\wt\sub");

    let inventory = inventory_at(&fs, r"E:\outer\wt\sub");

    assert_eq!(names(&inventory), ["main-only"]);
}

#[test]
fn the_walk_stops_at_the_worktree_root() {
    let fs = with_skill(repo(), r"E:\outer", "outer");

    let inventory = inventory_at(&fs, WORKTREE);

    assert_eq!(names(&inventory), ["main-only"]);
}

#[test]
fn a_worktree_inside_the_main_checkout_with_its_own_skills_does_not_read_the_main_checkout_s() {
    let inside = r"E:\outer\main\.claude\worktrees\x";
    let fs = with_skill(repo(), inside, "wt-x")
        .with_folder(Path::new(MAIN).join(r".git\worktrees\x"))
        .with_file(
            Path::new(inside).join(".git"),
            "gitdir: E:/outer/main/.git/worktrees/x\n",
        );

    let inventory = inventory_at(&fs, inside);

    assert_eq!(names(&inventory), ["wt-x"]);
}

#[test]
fn a_project_in_the_main_checkout_reads_as_any_repo() {
    let fs = with_skill(repo(), WORKTREE, "wt-only").with_folder(Path::new(MAIN).join(r".git"));

    let inventory = inventory_at(&fs, MAIN);

    assert_eq!(names(&inventory), ["main-only"]);
}

#[test]
fn the_main_checkout_s_mcp_json_is_not_read_in_a_worktree_outside_it() {
    let fs = repo().with_file(
        Path::new(MAIN).join(".mcp.json"),
        r#"{ "mcpServers": { "cd-main": { "command": "main" } } }"#,
    );

    let inventory = inventory_at(&fs, WORKTREE);

    let servers: Vec<&str> = inventory
        .items
        .iter()
        .filter(|item| serde_json::to_value(item.kind).unwrap() == "mcp")
        .map(|item| item.name.as_str())
        .collect();
    assert!(servers.is_empty(), "{servers:?}");
}
