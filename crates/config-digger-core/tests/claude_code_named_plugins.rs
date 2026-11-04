//! Claude Code's project-scope plugins in a Project outside their record's
//! repository whose own settings files name them (#547), through the core
//! `api`, with fake ports. The plugin `demo@acme`, with the server `docs`
//! and the skill `notes`, comes from the `directory` marketplace `acme` at
//! `E:\market`, which Claude Code reads it from, and is recorded at project
//! scope from the repository `E:\repo`. `E:\other` is a folder of no
//! repository.

use config_digger_core::api::{
    Action, Context, Core, Inventory, ScanOptions, Targets, WarningClass,
};
use config_digger_core::ports::fake::{self, FakeEnvironment, FakeFileSystem};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const HOME: &str = r"C:\Users\fixture";
const REPO: &str = r"E:\repo";
const OTHER: &str = r"E:\other";
const MARKET: &str = r"E:\market";
const CACHE: &str = r"C:\Users\fixture\.claude\plugins\cache\acme\demo\1.0.0";

/// The Items `demo@acme` carries.
const CARRIED: [&str; 2] = ["plugin:demo:docs", "demo:notes"];

/// `relative` in the fixture's home folder.
fn in_home(relative: &str) -> PathBuf {
    Path::new(HOME).join(relative)
}

/// The plugin's folder in the marketplace.
fn in_market() -> PathBuf {
    Path::new(MARKET).join("demo")
}

/// `fs` with the plugin's files in `folder`.
fn plugin_files(fs: FakeFileSystem, folder: &Path) -> FakeFileSystem {
    fs.with_file(
        folder.join(r".claude-plugin\plugin.json"),
        json!({ "name": "demo", "version": "1.0.0" }).to_string(),
    )
    .with_file(
        folder.join(".mcp.json"),
        json!({ "mcpServers": { "docs": { "command": "docs-server" } } }).to_string(),
    )
    .with_file(
        folder.join(r"skills\notes\SKILL.md"),
        "---\nname: notes\ndescription: Take notes.\n---\n\nDo notes.\n",
    )
}

/// `fs` with `demo@acme`'s records `records`.
fn recorded(fs: FakeFileSystem, records: Value) -> FakeFileSystem {
    fs.with_file(
        in_home(r".claude\plugins\installed_plugins.json"),
        json!({ "version": 2, "plugins": { "demo@acme": records } }).to_string(),
    )
}

/// A record of `demo@acme` at `scope`, bound to `project` when given.
fn record(scope: &str, project: Option<&str>) -> Value {
    let mut record = json!({
        "scope": scope,
        "installPath": CACHE,
        "version": "1.0.0",
    });
    if let Some(project) = project {
        record["projectPath"] = json!(project);
    }
    record
}

/// The repository, the marketplace `acme` in its folder, with its copy of
/// the plugin in the cache too, and `demo@acme` installed at project scope
/// from the repository, on in its settings.
fn fixture() -> FakeFileSystem {
    let fs = FakeFileSystem::new()
        .with_file(in_home(".claude.json"), "{}")
        .with_folder(Path::new(REPO).join(".git"))
        .with_folder(OTHER)
        .with_file(
            in_home(r".claude\plugins\known_marketplaces.json"),
            json!({ "acme": {
                "source": { "source": "directory", "path": MARKET },
                "installLocation": MARKET,
            } })
            .to_string(),
        )
        .with_file(
            Path::new(MARKET).join(r".claude-plugin\marketplace.json"),
            json!({ "name": "acme", "plugins": [{ "name": "demo", "source": "./demo" }] })
                .to_string(),
        );
    let fs = plugin_files(plugin_files(fs, &in_market()), Path::new(CACHE));
    let fs = switched(fs, REPO, "settings.json", true);
    recorded(fs, json!([record("project", Some(REPO))]))
}

/// `fs` whose `file` in `.claude` of `folder` turns `demo@acme` on or off.
fn switched(fs: FakeFileSystem, folder: &str, file: &str, on: bool) -> FakeFileSystem {
    fs.with_file(
        Path::new(folder).join(".claude").join(file),
        json!({ "enabledPlugins": { "demo@acme": on } }).to_string(),
    )
}

fn core(fs: &FakeFileSystem) -> Core {
    Core::new(fake::ports(fs.clone(), FakeEnvironment::with_home(HOME))).unwrap()
}

fn in_project(project: &str) -> Context {
    Context {
        project: Some(PathBuf::from(project)),
    }
}

/// The Inventory of `fs` in the context of the Project `project`, both the
/// repository and [`OTHER`] scanned.
fn inventory_at(fs: &FakeFileSystem, project: &str) -> Inventory {
    let core = core(fs);
    let snapshot = core.scan(&scan_options()).unwrap();
    core.resolve(&snapshot, &in_project(project))
}

fn scan_options() -> ScanOptions {
    ScanOptions {
        projects: vec![PathBuf::from(REPO), PathBuf::from(OTHER)],
    }
}

/// The path, Scope, State and reason code of each Entry of each Item of
/// [`CARRIED`] that `inventory` lists, by Item name.
fn carried(inventory: &Inventory) -> Vec<(String, Vec<[String; 4]>)> {
    let text = |entry: &Value, key: &str| entry[key].as_str().unwrap_or_default().to_owned();
    CARRIED
        .iter()
        .filter_map(|name| inventory.items.iter().find(|item| item.name == *name))
        .map(|item| {
            let entries = item
                .entries
                .iter()
                .map(|entry| serde_json::to_value(entry).unwrap())
                .map(|entry| {
                    [
                        text(&entry, "path"),
                        text(&entry, "scope"),
                        text(&entry, "state"),
                        text(&entry["reason"], "code"),
                    ]
                })
                .collect();
            (item.name.clone(), entries)
        })
        .collect()
}

/// What [`carried`] gives for `demo@acme`, read from `folder`, in the State
/// `state` with the reason code `code`.
fn demo_in(folder: &Path, state: &str, code: &str) -> Vec<(String, Vec<[String; 4]>)> {
    let row = |path: &str| {
        [
            folder.join(path).display().to_string(),
            "plugin".to_owned(),
            state.to_owned(),
            code.to_owned(),
        ]
    };
    vec![
        ("plugin:demo:docs".to_owned(), vec![row(".mcp.json")]),
        ("demo:notes".to_owned(), vec![row(r"skills\notes")]),
    ]
}

/// [`demo_in`] the marketplace's folder.
fn demo(state: &str, code: &str) -> Vec<(String, Vec<[String; 4]>)> {
    demo_in(&in_market(), state, code)
}

#[test]
fn a_project_outside_the_repository_whose_settings_turn_the_plugin_on_lists_it() {
    let fs = switched(fixture(), OTHER, "settings.json", true);

    assert_eq!(carried(&inventory_at(&fs, OTHER)), demo("effective", ""));
}

#[test]
fn a_project_whose_local_settings_alone_turn_the_plugin_on_lists_it() {
    let fs = switched(fixture(), OTHER, "settings.local.json", true);

    assert_eq!(carried(&inventory_at(&fs, OTHER)), demo("effective", ""));
}

/// Carries out `action` on the plugin's server in the Project [`OTHER`].
fn act_in_other(fs: &FakeFileSystem, action: Action) {
    let core = core(fs);
    let snapshot = core.scan(&scan_options()).unwrap();
    let targets = Targets {
        selectors: vec!["plugin:demo:docs".to_owned()],
        ..Targets::default()
    };
    let plan = core
        .plan(&snapshot, &in_project(OTHER), action, &targets)
        .unwrap();
    core.execute(plan, &[WarningClass::WideReach]).unwrap();
}

/// What [`OTHER`]'s `.claude/settings.local.json` holds.
fn other_local(fs: &FakeFileSystem) -> Value {
    let local = Path::new(OTHER).join(r".claude\settings.local.json");
    serde_json::from_slice(&fs.contents(local).unwrap()).unwrap()
}

#[test]
fn a_project_whose_local_settings_turn_the_plugin_off_lists_it_disabled_and_enable_turns_it_on() {
    // Its `.claude/settings.json` turns the plugin on, and its local
    // settings, which win, off: Claude Code does not load it there.
    let fs = switched(
        switched(fixture(), OTHER, "settings.json", true),
        OTHER,
        "settings.local.json",
        false,
    );
    assert_eq!(
        carried(&inventory_at(&fs, OTHER)),
        demo("disabled", "plugin_disabled")
    );

    act_in_other(&fs, Action::Enable);

    assert_eq!(
        other_local(&fs),
        json!({ "enabledPlugins": { "demo@acme": true } })
    );
    assert_eq!(carried(&inventory_at(&fs, OTHER)), demo("effective", ""));
}

#[test]
fn disable_in_such_a_project_writes_its_local_settings_and_it_still_lists_the_plugin() {
    let fs = switched(fixture(), OTHER, "settings.json", true);

    act_in_other(&fs, Action::Disable);

    assert_eq!(
        other_local(&fs),
        json!({ "enabledPlugins": { "demo@acme": false } })
    );
    assert_eq!(
        carried(&inventory_at(&fs, OTHER)),
        demo("disabled", "plugin_disabled")
    );
    assert_eq!(carried(&inventory_at(&fs, REPO)), demo("effective", ""));
}

#[test]
fn a_project_whose_files_do_not_name_the_plugin_lists_nothing_for_it() {
    let fs = fixture().with_file(
        Path::new(OTHER).join(r".claude\settings.json"),
        json!({ "enabledPlugins": { "other@acme": true } }).to_string(),
    );

    assert_eq!(carried(&inventory_at(&fs, OTHER)), Vec::new());
    assert_eq!(carried(&inventory_at(&fs, REPO)), demo("effective", ""));
}

#[test]
fn a_plugin_read_from_its_copy_in_the_cache_is_not_listed_where_the_settings_alone_name_it() {
    // The marketplace's entry gives no path below its folder, so Claude
    // Code reads the copy in the cache, which it loads in no folder outside
    // the record's repository, as `claude-code.mjs` found.
    let fs = switched(fixture(), OTHER, "settings.json", true).with_file(
        Path::new(MARKET).join(r".claude-plugin\marketplace.json"),
        json!({ "name": "acme", "plugins": [{ "name": "demo", "source": "../elsewhere" }] })
            .to_string(),
    );

    assert_eq!(carried(&inventory_at(&fs, OTHER)), Vec::new());
    assert_eq!(
        carried(&inventory_at(&fs, REPO)),
        demo_in(Path::new(CACHE), "effective", "")
    );
}

#[test]
fn a_project_a_user_scope_record_of_the_id_applies_in_gets_no_second_copy() {
    let fs = recorded(
        switched(fixture(), OTHER, "settings.json", true),
        json!([record("user", None), record("project", Some(REPO))]),
    );

    assert_eq!(carried(&inventory_at(&fs, OTHER)), demo("effective", ""));
}

#[test]
fn of_two_records_bound_to_other_projects_one_is_listed_where_the_settings_name_it() {
    let third = r"E:\third";
    let fs = recorded(
        switched(fixture(), OTHER, "settings.json", true).with_folder(third),
        json!([
            record("project", Some(REPO)),
            record("project", Some(third))
        ]),
    );

    assert_eq!(carried(&inventory_at(&fs, OTHER)), demo("effective", ""));
}

#[test]
fn a_local_scope_record_is_listed_where_the_settings_name_it_too() {
    let fs = recorded(
        switched(fixture(), OTHER, "settings.local.json", true),
        json!([record("local", Some(REPO))]),
    );

    assert_eq!(carried(&inventory_at(&fs, OTHER)), demo("effective", ""));
}

/// `fs` whose user settings turn `demo@acme` on or off (#555).
fn user_switched(fs: FakeFileSystem, on: bool) -> FakeFileSystem {
    fs.with_file(
        in_home(r".claude\settings.json"),
        json!({ "enabledPlugins": { "demo@acme": on } }).to_string(),
    )
}

#[test]
fn a_project_none_of_whose_files_name_the_plugin_lists_it_when_the_user_settings_turn_it_on() {
    let fs = user_switched(fixture(), true);

    assert_eq!(carried(&inventory_at(&fs, OTHER)), demo("effective", ""));
    assert_eq!(carried(&inventory_at(&fs, REPO)), demo("effective", ""));
}

#[test]
fn a_project_none_of_whose_files_name_the_plugin_lists_it_disabled_when_the_user_settings_turn_it_off_and_enable_turns_it_on()
 {
    let fs = user_switched(fixture(), false);
    assert_eq!(
        carried(&inventory_at(&fs, OTHER)),
        demo("disabled", "plugin_disabled")
    );

    act_in_other(&fs, Action::Enable);

    assert_eq!(
        other_local(&fs),
        json!({ "enabledPlugins": { "demo@acme": true } })
    );
    assert_eq!(carried(&inventory_at(&fs, OTHER)), demo("effective", ""));
}

#[test]
fn disable_where_the_user_settings_alone_turn_the_plugin_on_writes_the_project_s_local_settings_and_it_still_lists_the_plugin()
 {
    let fs = user_switched(fixture(), true);

    act_in_other(&fs, Action::Disable);

    assert_eq!(
        other_local(&fs),
        json!({ "enabledPlugins": { "demo@acme": false } })
    );
    assert_eq!(
        carried(&inventory_at(&fs, OTHER)),
        demo("disabled", "plugin_disabled")
    );
    assert_eq!(carried(&inventory_at(&fs, REPO)), demo("effective", ""));
}

#[test]
fn a_project_whose_own_settings_and_the_user_settings_name_the_plugin_lists_it_once() {
    let fs = switched(user_switched(fixture(), true), OTHER, "settings.json", true);

    assert_eq!(carried(&inventory_at(&fs, OTHER)), demo("effective", ""));
    assert_eq!(carried(&inventory_at(&fs, REPO)), demo("effective", ""));
}

#[test]
fn where_the_user_settings_name_the_plugin_a_user_scope_record_leaves_one_copy_in_each_project() {
    // The record Claude Code writes in the first session the user settings
    // bring the plugin into, as `claude-code.mjs` found.
    let fs = recorded(
        user_switched(fixture(), true),
        json!([record("project", Some(REPO)), record("user", None)]),
    );

    assert_eq!(carried(&inventory_at(&fs, OTHER)), demo("effective", ""));
    assert_eq!(carried(&inventory_at(&fs, REPO)), demo("effective", ""));
}

#[test]
fn a_linked_worktree_of_the_record_s_repository_lists_the_plugin_once_where_the_user_settings_name_it()
 {
    let worktree = r"E:\repo-wt";
    let fs = user_switched(fixture(), true)
        .with_folder(Path::new(REPO).join(r".git\worktrees\wt"))
        .with_file(
            Path::new(worktree).join(".git"),
            "gitdir: E:/repo/.git/worktrees/wt\n",
        );
    let core = core(&fs);
    let options = ScanOptions {
        projects: vec![
            PathBuf::from(REPO),
            PathBuf::from(worktree),
            PathBuf::from(OTHER),
        ],
    };
    let snapshot = core.scan(&options).unwrap();

    for project in [REPO, worktree, OTHER] {
        let inventory = core.resolve(&snapshot, &in_project(project));
        assert_eq!(carried(&inventory), demo("effective", ""), "{project}");
    }
}

/// [`fixture`] whose marketplace's entry gives no path below its folder,
/// so that Claude Code reads the plugin's copy in the cache.
fn read_from_the_cache(fs: FakeFileSystem) -> FakeFileSystem {
    fs.with_file(
        Path::new(MARKET).join(r".claude-plugin\marketplace.json"),
        json!({ "name": "acme", "plugins": [{ "name": "demo", "source": "../elsewhere" }] })
            .to_string(),
    )
}

#[test]
fn a_plugin_read_from_its_copy_in_the_cache_is_not_listed_where_the_user_settings_alone_name_it() {
    // Claude Code does not load it in a folder no record applies in, as
    // `claude-code.mjs` found, until that session installs it at user scope.
    let fs = read_from_the_cache(user_switched(fixture(), true));

    assert_eq!(carried(&inventory_at(&fs, OTHER)), Vec::new());
    assert_eq!(
        carried(&inventory_at(&fs, REPO)),
        demo_in(Path::new(CACHE), "effective", "")
    );
}

#[test]
fn a_plugin_read_from_its_copy_in_the_cache_is_listed_everywhere_once_claude_code_installs_it_at_user_scope()
 {
    let fs = recorded(
        read_from_the_cache(user_switched(fixture(), true)),
        json!([record("project", Some(REPO)), record("user", None)]),
    );

    assert_eq!(
        carried(&inventory_at(&fs, OTHER)),
        demo_in(Path::new(CACHE), "effective", "")
    );
}

#[test]
fn a_local_scope_record_is_listed_where_the_user_settings_alone_name_it() {
    let fs = recorded(
        user_switched(fixture(), true),
        json!([record("local", Some(REPO))]),
    );

    assert_eq!(carried(&inventory_at(&fs, OTHER)), demo("effective", ""));
}

#[test]
fn a_project_whose_shared_settings_turn_the_plugin_off_enables_it_in_its_local_settings() {
    let fs = switched(fixture(), OTHER, "settings.json", false);
    assert_eq!(
        carried(&inventory_at(&fs, OTHER)),
        demo("disabled", "plugin_disabled")
    );

    act_in_other(&fs, Action::Enable);

    assert_eq!(
        other_local(&fs),
        json!({ "enabledPlugins": { "demo@acme": true } })
    );
    assert_eq!(carried(&inventory_at(&fs, OTHER)), demo("effective", ""));
}

#[test]
fn enable_of_the_plugin_s_skill_where_the_user_settings_turn_it_off_writes_the_project_s_local_settings()
 {
    let fs = user_switched(fixture(), false);
    let core = core(&fs);
    let snapshot = core.scan(&scan_options()).unwrap();
    let targets = Targets {
        selectors: vec!["demo:notes".to_owned()],
        ..Targets::default()
    };
    let plan = core
        .plan(&snapshot, &in_project(OTHER), Action::Enable, &targets)
        .unwrap();
    core.execute(plan, &[WarningClass::WideReach]).unwrap();

    assert_eq!(
        other_local(&fs),
        json!({ "enabledPlugins": { "demo@acme": true } })
    );
    assert_eq!(carried(&inventory_at(&fs, OTHER)), demo("effective", ""));
}

#[test]
fn a_user_scope_record_after_two_records_bound_to_projects_leaves_one_copy_in_each() {
    let third = r"E:\third";
    let fs = recorded(
        user_switched(fixture(), true).with_folder(third),
        json!([
            record("project", Some(REPO)),
            record("project", Some(third)),
            record("user", None)
        ]),
    );
    let core = core(&fs);
    let options = ScanOptions {
        projects: vec![
            PathBuf::from(REPO),
            PathBuf::from(third),
            PathBuf::from(OTHER),
        ],
    };
    let snapshot = core.scan(&options).unwrap();

    for project in [REPO, third, OTHER] {
        let inventory = core.resolve(&snapshot, &in_project(project));
        assert_eq!(carried(&inventory), demo("effective", ""), "{project}");
    }
}

#[test]
fn enable_in_a_project_of_a_user_scope_plugin_the_user_settings_turn_off_writes_its_local_settings()
{
    let fs = recorded(
        user_switched(fixture(), false),
        json!([record("user", None)]),
    );

    act_in_other(&fs, Action::Enable);

    assert_eq!(
        other_local(&fs),
        json!({ "enabledPlugins": { "demo@acme": true } })
    );
    assert_eq!(carried(&inventory_at(&fs, OTHER)), demo("effective", ""));
}
