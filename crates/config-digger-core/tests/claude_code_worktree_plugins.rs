//! Claude Code's project-scope plugins in a linked git worktree, through
//! the core `api`, with fake ports: a main checkout at `E:\outer\main` and
//! linked worktrees of it at `E:\outer\wt` and `E:\outer\wt2`, whose `.git`
//! files name `E:\outer\main\.git\worktrees\<name>`, as the release gate's
//! probe builds them with `git worktree add`. The plugin `demo@acme`, with
//! the server `docs` and the skill `notes`, is installed at project scope.

use config_digger_core::api::{
    Action, Context, Core, Inventory, ScanOptions, Targets, WarningClass,
};
use config_digger_core::ports::fake::{self, FakeEnvironment, FakeFileSystem};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const HOME: &str = r"C:\Users\fixture";
const MAIN: &str = r"E:\outer\main";
const WORKTREE: &str = r"E:\outer\wt";
const OTHER_WORKTREE: &str = r"E:\outer\wt2";
const PLUGIN: &str = r"C:\Users\fixture\.claude\plugins\cache\acme\demo\1.0.0";

/// The Items `demo@acme` carries.
const CARRIED: [&str; 2] = ["plugin:demo:docs", "demo:notes"];

/// `relative` in the fixture's home folder.
fn in_home(relative: &str) -> PathBuf {
    Path::new(HOME).join(relative)
}

/// `fs` with the linked worktree `folder` of the main checkout, named
/// `name` in its `.git\worktrees`.
fn with_worktree(fs: FakeFileSystem, folder: &str, name: &str) -> FakeFileSystem {
    fs.with_folder(Path::new(MAIN).join(r".git\worktrees").join(name))
        .with_file(
            Path::new(folder).join(".git"),
            format!("gitdir: E:/outer/main/.git/worktrees/{name}\n"),
        )
}

/// `fs` with `demo@acme` recorded at project scope, bound to `project`.
fn installed_for(fs: FakeFileSystem, project: &str) -> FakeFileSystem {
    let records = json!({
        "version": 2,
        "plugins": { "demo@acme": [{
            "scope": "project",
            "installPath": PLUGIN,
            "version": "1.0.0",
            "projectPath": project,
        }] },
    });
    fs.with_file(
        in_home(r".claude\plugins\installed_plugins.json"),
        records.to_string(),
    )
}

/// The main checkout, its linked worktree [`WORKTREE`], and `demo@acme`
/// installed from the main checkout, on in neither's settings.
fn repo() -> FakeFileSystem {
    let fs = FakeFileSystem::new()
        .with_file(in_home(".claude.json"), "{}")
        .with_folder(Path::new(MAIN).join(".git"))
        .with_file(
            Path::new(PLUGIN).join(r".claude-plugin\plugin.json"),
            json!({ "name": "demo", "version": "1.0.0" }).to_string(),
        )
        .with_file(
            Path::new(PLUGIN).join(".mcp.json"),
            json!({ "mcpServers": { "docs": { "command": "docs-server" } } }).to_string(),
        )
        .with_file(
            Path::new(PLUGIN).join(r"skills\notes\SKILL.md"),
            "---\nname: notes\ndescription: Take notes.\n---\n\nDo notes.\n",
        );
    installed_for(with_worktree(fs, WORKTREE, "wt"), MAIN)
}

/// A settings file of a folder, by the folder and the file's name in its
/// `.claude`, turning `demo@acme` on or off.
type Switch<'s> = (&'s str, &'s str, bool);

/// `fs` whose `file` in `.claude` of `folder` turns `demo@acme` on or off.
fn switched(fs: FakeFileSystem, folder: &str, file: &str, on: bool) -> FakeFileSystem {
    fs.with_file(
        Path::new(folder).join(".claude").join(file),
        json!({ "enabledPlugins": { "demo@acme": on } }).to_string(),
    )
}

/// The Inventory of `fs` in the context of the Project `project`, every
/// Project of `projects` scanned.
fn inventory_at(fs: &FakeFileSystem, project: &str, projects: &[&str]) -> Inventory {
    let env = FakeEnvironment::with_home(HOME);
    let core = Core::new(fake::ports(fs.clone(), env)).unwrap();
    let snapshot = core
        .scan(&ScanOptions {
            projects: projects.iter().map(PathBuf::from).collect(),
        })
        .unwrap();
    core.resolve(
        &snapshot,
        &Context {
            project: Some(PathBuf::from(project)),
        },
    )
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

/// What [`carried`] gives for `demo@acme` in the State `state` with the
/// reason code `code`.
fn demo(state: &str, code: &str) -> Vec<(String, Vec<[String; 4]>)> {
    let row = |path: &str| {
        [
            Path::new(PLUGIN).join(path).display().to_string(),
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

#[test]
fn a_worktree_lists_the_plugin_its_main_checkout_installed_on_by_its_own_settings() {
    let fs = switched(repo(), WORKTREE, "settings.json", true);

    let inventory = inventory_at(&fs, WORKTREE, &[WORKTREE]);

    assert_eq!(carried(&inventory), demo("effective", ""));
}

#[test]
fn only_the_worktree_s_own_settings_turn_the_plugin_on_or_off_there() {
    let off = demo("disabled", "plugin_disabled");
    let on = demo("effective", "");
    // Each case: the switches of the main checkout's and the worktree's
    // settings files, and what the worktree lists.
    let cases: [(&[Switch], _); 6] = [
        (&[(MAIN, "settings.json", true)], &off),
        (&[(MAIN, "settings.local.json", true)], &off),
        (
            &[
                (MAIN, "settings.json", false),
                (WORKTREE, "settings.json", true),
            ],
            &on,
        ),
        (
            &[
                (MAIN, "settings.json", true),
                (WORKTREE, "settings.json", false),
            ],
            &off,
        ),
        (
            &[
                (WORKTREE, "settings.json", true),
                (WORKTREE, "settings.local.json", false),
            ],
            &off,
        ),
        (
            &[
                (WORKTREE, "settings.json", false),
                (WORKTREE, "settings.local.json", true),
            ],
            &on,
        ),
    ];
    for (switches, expected) in cases {
        let fs = switches.iter().fold(repo(), |fs, (folder, file, on)| {
            switched(fs, folder, file, *on)
        });

        let inventory = inventory_at(&fs, WORKTREE, &[WORKTREE]);

        assert_eq!(&carried(&inventory), expected, "{switches:?}");
    }
}

#[test]
fn the_main_checkout_still_reads_its_own_settings() {
    let fs = switched(
        switched(repo(), MAIN, "settings.json", true),
        WORKTREE,
        "settings.json",
        false,
    );

    let inventory = inventory_at(&fs, MAIN, &[WORKTREE]);

    assert_eq!(carried(&inventory), demo("effective", ""));
}

#[test]
fn a_plugin_installed_from_a_worktree_is_listed_in_every_checkout_of_its_repository() {
    let other_repo = r"E:\other";
    let below_main = r"E:\outer\main\sub";
    let fs = with_worktree(installed_for(repo(), WORKTREE), OTHER_WORKTREE, "wt2")
        .with_folder(Path::new(other_repo).join(".git"))
        .with_folder(below_main);
    let fs = [MAIN, OTHER_WORKTREE, other_repo, below_main]
        .iter()
        .fold(fs, |fs, folder| switched(fs, folder, "settings.json", true));
    let projects = [MAIN, WORKTREE, OTHER_WORKTREE, other_repo, below_main];
    // A plugin read from its copy in the cache, as here, loads in no folder
    // outside the checkouts of its repository; one read from its
    // marketplace's folder does (#547, `claude_code_named_plugins`).

    for (project, expected) in [
        (WORKTREE, demo("disabled", "plugin_disabled")),
        (MAIN, demo("effective", "")),
        (OTHER_WORKTREE, demo("effective", "")),
        (other_repo, Vec::new()),
        (below_main, Vec::new()),
    ] {
        let inventory = inventory_at(&fs, project, &projects);

        assert_eq!(carried(&inventory), expected, "{project}");
    }
}

#[test]
fn checkouts_of_a_bare_repo_a_separate_git_folder_or_a_submodule_share_the_plugin() {
    // Each case: the `.git` of each checkout root, the root the plugin is
    // installed from, the root it is listed in, and a root it is not.
    let bare = [
        (r"E:\bare\a", "gitdir: E:/bare/x.git/worktrees/a"),
        (r"E:\bare\b", "gitdir: E:/bare/x.git/worktrees/b"),
    ];
    let separate = [
        (r"E:\separate\main", "gitdir: E:/gitdirs/main"),
        (r"E:\separate\wt", "gitdir: E:/gitdirs/main/worktrees/wt"),
    ];
    let submodule = [
        (r"E:\super\sub", "gitdir: ../.git/modules/sub"),
        (
            r"E:\sub-wt",
            "gitdir: E:/super/.git/modules/sub/worktrees/sub-wt",
        ),
    ];
    for (roots, outside) in [
        (bare, None),
        (separate, None),
        (submodule, Some(r"E:\super")),
    ] {
        let [(installed_from, _), (listed_in, _)] = roots;
        let mut fs = installed_for(repo(), installed_from)
            .with_folder(r"E:\bare\x.git\worktrees\a")
            .with_folder(r"E:\bare\x.git\worktrees\b")
            .with_folder(r"E:\gitdirs\main\worktrees\wt")
            .with_folder(r"E:\super\.git\modules\sub\worktrees\sub-wt");
        for (root, git) in roots {
            fs = switched(fs, root, "settings.json", true)
                .with_file(Path::new(root).join(".git"), format!("{git}\n"));
        }
        if let Some(outside) = outside {
            fs = switched(fs, outside, "settings.json", true);
        }
        let projects: Vec<&str> = [installed_from, listed_in]
            .into_iter()
            .chain(outside)
            .collect();

        let inventory = inventory_at(&fs, listed_in, &projects);
        assert_eq!(carried(&inventory), demo("effective", ""), "{listed_in}");
        if let Some(outside) = outside {
            let inventory = inventory_at(&fs, outside, &projects);
            assert_eq!(carried(&inventory), Vec::new(), "{outside}");
        }
    }
}

#[test]
fn disable_in_a_worktree_writes_the_worktree_s_local_settings_alone() {
    let fs = switched(
        switched(repo(), MAIN, "settings.json", true),
        WORKTREE,
        "settings.json",
        true,
    );
    let core = Core::new(fake::ports(fs.clone(), FakeEnvironment::with_home(HOME))).unwrap();
    let context = Context {
        project: Some(PathBuf::from(WORKTREE)),
    };
    let snapshot = core
        .scan(&ScanOptions {
            projects: vec![PathBuf::from(WORKTREE)],
        })
        .unwrap();
    let targets = Targets {
        selectors: vec!["plugin:demo:docs".to_owned()],
        ..Targets::default()
    };

    let plan = core
        .plan(&snapshot, &context, Action::Disable, &targets)
        .unwrap();
    core.execute(plan, &[WarningClass::WideReach]).unwrap();

    let local = Path::new(WORKTREE).join(r".claude\settings.local.json");
    let written: Value = serde_json::from_slice(&fs.contents(local).unwrap()).unwrap();
    assert_eq!(written, json!({ "enabledPlugins": { "demo@acme": false } }));
    assert_eq!(
        carried(&inventory_at(&fs, WORKTREE, &[WORKTREE])),
        demo("disabled", "plugin_disabled")
    );
    assert!(
        fs.contents(Path::new(MAIN).join(r".claude\settings.local.json"))
            .is_none()
    );
    assert_eq!(
        carried(&inventory_at(&fs, MAIN, &[WORKTREE])),
        demo("effective", "")
    );
}
