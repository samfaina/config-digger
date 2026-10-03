//! Claude Code plugins through the core `api`, with fake ports over a
//! fixture plugin cache: which plugins Claude Code loads, the servers and
//! skills they carry, how their servers compete by endpoint, and Disable
//! and Enable of a whole plugin through `enabledPlugins`.

use config_digger_core::api::{
    Action, Context, Core, DisableOption, Entry, Error, Inventory, Item, Plan, ScanOptions,
    Targets, WarningClass,
};
use config_digger_core::ports::fake::{self, FakeEnvironment, FakeFileSystem};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const HOME: &str = r"C:\Users\fixture";
const LOCAL_APP_DATA: &str = r"C:\Users\fixture\AppData\Local";
const PETME: &str = r"E:\projects\petme-app";
const CACHE: &str = r"C:\Users\fixture\.claude\plugins\cache";

/// The fixture folder, whose `home` stands for the user's home folder and
/// `petme-app` for the Project.
fn fixture_folder() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join(r"tests\fixtures\claude-code-plugins")
}

/// Every file below `folder` on disk, at the same place below `at`.
fn load(fs: FakeFileSystem, folder: &Path, at: &Path) -> FakeFileSystem {
    let mut fs = fs.with_folder(at);
    for child in std::fs::read_dir(folder).unwrap() {
        let child = child.unwrap().path();
        let name = child.file_name().unwrap();
        fs = if child.is_dir() {
            load(fs, &child, &at.join(name))
        } else {
            fs.with_file(at.join(name), std::fs::read(&child).unwrap())
        };
    }
    fs
}

/// The fixture plugin cache, with a Claude Code home and the Project petme-app.
fn fixture() -> FakeFileSystem {
    let fs = load(
        FakeFileSystem::new(),
        &fixture_folder().join("home"),
        Path::new(HOME),
    );
    load(fs, &fixture_folder().join("petme-app"), Path::new(PETME))
}

fn in_cache(relative: &str) -> PathBuf {
    PathBuf::from(CACHE).join(relative)
}

fn core(fs: &FakeFileSystem) -> Core {
    let env = FakeEnvironment::with_home(HOME).with_var("LOCALAPPDATA", LOCAL_APP_DATA);
    Core::new(fake::ports(fs.clone(), env)).unwrap()
}

fn inventory(core: &Core, context: &Context) -> Inventory {
    let snapshot = core
        .scan(&ScanOptions {
            projects: context.project.iter().cloned().collect(),
        })
        .unwrap();
    core.resolve(&snapshot, context)
}

fn no_project(fs: &FakeFileSystem) -> Inventory {
    inventory(&core(fs), &Context::default())
}

fn in_petme() -> Context {
    Context {
        project: Some(PathBuf::from(PETME)),
    }
}

fn item<'i>(inventory: &'i Inventory, name: &str) -> &'i Item {
    inventory
        .items
        .iter()
        .find(|item| item.name == name)
        .unwrap_or_else(|| panic!("no Item `{name}`"))
}

/// The Entry at `path` with the key path `key_path`, among every Item.
fn entry_at<'i>(inventory: &'i Inventory, path: &Path, key_path: &[&str]) -> &'i Entry {
    inventory
        .items
        .iter()
        .flat_map(|item| &item.entries)
        .find(|entry| entry.path == path && entry.key_path == key_path)
        .unwrap_or_else(|| panic!("no Entry at {}#{}", path.display(), key_path.join(".")))
}

/// A serialised value as its JSON string, such as a State's `"disabled"`.
fn code(value: impl serde::Serialize) -> String {
    match serde_json::to_value(value).unwrap() {
        Value::String(code) => code,
        other => panic!("not a code: {other}"),
    }
}

fn entry_json(entry: &Entry) -> Value {
    serde_json::to_value(entry).unwrap()
}

#[test]
fn a_server_in_a_plugins_mcp_json_is_listed_with_plugin_provenance() {
    let inventory = no_project(&fixture());

    let path = in_cache(r"claude-plugins-official\context7\1.0.0\.mcp.json");
    let entry = entry_json(entry_at(&inventory, &path, &["mcpServers", "context7"]));
    assert_eq!(entry["scope"], "plugin");
    assert_eq!(entry["provenance"], "plugin");
    assert_eq!(entry["plugin"]["id"], "context7@claude-plugins-official");
    assert_eq!(entry["plugin"]["name"], "context7");
    assert_eq!(entry["plugin"]["scope"], "user");
    assert_eq!(entry["plugin"]["version"], "1.0.0");
}

#[test]
fn a_plugin_skill_is_namespaced_by_its_plugin() {
    let inventory = no_project(&fixture());

    let skill = item(&inventory, "supabase:supabase-postgres");
    assert_eq!(code(skill.kind), "skill");
    assert_eq!(code(skill.state), "effective");
    let entry = entry_json(&skill.entries[0]);
    assert_eq!(
        entry["path"],
        in_cache(r"claude-plugins-official\supabase\0.1.4\skills\supabase-postgres")
            .display()
            .to_string()
    );
    assert_eq!(entry["provenance"], "plugin");
    assert_eq!(entry["plugin"]["id"], "supabase@claude-plugins-official");
    assert!(
        !inventory
            .items
            .iter()
            .any(|item| item.name == "supabase-postgres"),
        "a plugin skill answers to its namespaced name"
    );
}

#[test]
fn a_plugin_folder_marked_superseded_is_not_read() {
    let inventory = no_project(&fixture());

    let names: Vec<&str> = inventory
        .items
        .iter()
        .map(|item| item.name.as_str())
        .collect();
    assert!(!names.contains(&"retired:retired"), "{names:?}");
    let brainstorming = item(&inventory, "superpowers:brainstorming");
    let paths: Vec<&Path> = brainstorming
        .entries
        .iter()
        .map(|entry| entry.path.as_path())
        .collect();
    assert_eq!(
        paths,
        [in_cache(
            r"claude-plugins-official\superpowers\6.4.1\skills\brainstorming"
        )]
    );
}

#[test]
fn the_manifests_mcp_servers_can_name_a_file_of_the_plugin() {
    let inventory = no_project(&fixture());

    // supabase's manifest points to `./agents/claude/.mcp.json`. Its server
    // does not compete with the user's same-named one, which runs another
    // command.
    let path = in_cache(r"claude-plugins-official\supabase\0.1.4\agents\claude\.mcp.json");
    let plugin = item(&inventory, "plugin:supabase:supabase");
    assert_eq!(plugin.entries.len(), 1);
    assert_eq!(plugin.entries[0].path, path);
    assert_eq!(plugin.entries[0].key_path, ["mcpServers", "supabase"]);
    assert_eq!(code(plugin.state), "effective");
    let user = item(&inventory, "supabase");
    assert_eq!(user.entries.len(), 1);
    assert_eq!(code(user.state), "effective");
}

#[test]
fn the_manifests_mcp_servers_can_be_inline() {
    let inventory = no_project(&fixture());

    let memory = item(&inventory, "plugin:superpowers:episodic-memory");
    assert_eq!(
        memory.entries[0].path,
        in_cache(r"claude-plugins-official\superpowers\6.4.1\.claude-plugin\plugin.json")
    );
    assert_eq!(
        memory.entries[0].key_path,
        ["mcpServers", "episodic-memory"]
    );
}

#[test]
fn the_manifests_mcp_servers_can_be_a_list_of_files_and_inline_servers() {
    let inventory = inventory(&core(&fixture()), &in_petme());

    let folder = in_cache(r"ponytail\ponytail\0.2.0");
    let pony = item(&inventory, "plugin:ponytail:pony");
    assert_eq!(pony.entries[0].path, folder.join("servers.json"));
    assert_eq!(pony.entries[0].key_path, ["mcpServers", "pony"]);
    let tail = item(&inventory, "plugin:ponytail:tail");
    assert_eq!(
        tail.entries[0].path,
        folder.join(r".claude-plugin\plugin.json")
    );
    assert_eq!(tail.entries[0].key_path, ["mcpServers", "1", "tail"]);
}

#[test]
fn a_plugin_installed_for_one_project_applies_only_there() {
    let fs = fixture();

    let elsewhere = no_project(&fs);
    assert!(
        !elsewhere
            .items
            .iter()
            .any(|item| item.name.starts_with("plugin:ponytail:")),
        "ponytail is installed for petme-app only"
    );
    let in_petme = inventory(&core(&fs), &in_petme());
    let pony = item(&in_petme, "plugin:ponytail:pony");
    let plugin = pony.entries[0].plugin.as_ref().unwrap();
    assert_eq!(plugin.id, "ponytail@ponytail");
    assert_eq!(plugin.scope.as_deref(), Some("local"));
}

#[test]
fn a_plugin_server_with_the_endpoint_of_another_server_is_shadowed_by_it() {
    let inventory = no_project(&fixture());

    // The user's `docs` and context7's server have the same URL, but for
    // the host's case, the default port and a trailing slash.
    let docs = item(&inventory, "docs");
    let locators: Vec<&str> = docs
        .entries
        .iter()
        .map(|entry| entry.locator.as_str())
        .collect();
    let user = format!(r"{HOME}\.claude.json#mcpServers.docs");
    let plugin = format!(
        r"{}#mcpServers.context7",
        in_cache(r"claude-plugins-official\context7\1.0.0\.mcp.json").display()
    );
    assert_eq!(locators, [user.as_str(), plugin.as_str()]);
    assert_eq!(code(docs.entries[0].state), "effective");
    assert_eq!(code(docs.entries[1].state), "shadowed");
    let reason = docs.entries[1].reason.as_ref().unwrap();
    assert_eq!(code(reason.code), "shadowed_by_precedence");
    assert_eq!(reason.effective.as_deref(), Some(user.as_str()));
    assert_eq!(
        reason.sentence,
        format!(
            "Shadowed by the user Entry {user}, which has the same endpoint and Claude Code ranks higher."
        )
    );
    assert!(
        !inventory
            .items
            .iter()
            .any(|item| item.name == "plugin:context7:context7"),
        "the plugin server is no Item of its own"
    );
}

#[test]
fn a_plugin_turned_off_in_enabled_plugins_is_disabled_with_everything_it_carries() {
    let inventory = no_project(&fixture());

    let switch =
        format!(r"{HOME}\.claude\settings.json#enabledPlugins.superpowers@claude-plugins-official");
    for name in [
        "superpowers:brainstorming",
        "plugin:superpowers:episodic-memory",
    ] {
        let superpowers = item(&inventory, name);
        assert_eq!(code(superpowers.state), "disabled", "{name}");
        let reason = superpowers.entries[0].reason.as_ref().unwrap();
        assert_eq!(code(reason.code), "plugin_disabled");
        assert_eq!(reason.switch.as_deref(), Some(switch.as_str()));
        assert_eq!(
            reason.sentence,
            format!(
                r"Disabled with its plugin superpowers@claude-plugins-official: `enabledPlugins.superpowers@claude-plugins-official` is false in {HOME}\.claude\settings.json, so Claude Code skips every server and skill of the plugin."
            )
        );
    }
    assert_eq!(
        code(item(&inventory, "supabase:supabase-postgres").state),
        "effective"
    );
}

fn user_settings() -> PathBuf {
    PathBuf::from(HOME).join(r".claude\settings.json")
}

fn petme_local_settings() -> PathBuf {
    PathBuf::from(PETME).join(r".claude\settings.local.json")
}

fn json_of(fs: &FakeFileSystem, path: impl AsRef<Path>) -> Value {
    serde_json::from_slice(&fs.contents(path).expect("the file exists")).unwrap()
}

fn plan(core: &Core, context: &Context, action: Action, selectors: &[&str]) -> Result<Plan, Error> {
    let snapshot = core
        .scan(&ScanOptions {
            projects: context.project.iter().cloned().collect(),
        })
        .unwrap();
    let targets = Targets {
        selectors: selectors
            .iter()
            .map(|selector| (*selector).to_owned())
            .collect(),
        all: false,
    };
    core.plan(&snapshot, context, action, &targets)
}

/// `(Item, State before, State after)` per change of a plan in the Project
/// `project`, or with no Project, sorted.
fn changes(plan: &Plan, project: Option<&Path>) -> Vec<(String, String, String)> {
    let mut changes: Vec<(String, String, String)> = plan
        .preview()
        .changes
        .iter()
        .filter(|change| change.project.as_deref() == project)
        .map(|change| (change.item.clone(), code(change.before), code(change.after)))
        .collect();
    changes.sort();
    changes
}

fn triples(triples: &[(&str, &str, &str)]) -> Vec<(String, String, String)> {
    triples
        .iter()
        .map(|(a, b, c)| ((*a).to_owned(), (*b).to_owned(), (*c).to_owned()))
        .collect()
}

#[test]
fn disable_of_a_plugin_entry_turns_the_whole_plugin_off_in_the_user_settings() {
    let fs = fixture();
    let core = core(&fs);
    let before = no_project(&fs);
    match &item(&before, "plugin:supabase:supabase").entries[0].disable {
        DisableOption::Offered { path, sentence } => {
            assert_eq!(*path, user_settings());
            assert_eq!(
                sentence,
                &format!(
                    r"Disable sets `enabledPlugins.supabase@claude-plugins-official` to false in {HOME}\.claude\settings.json. This Disables the whole plugin supabase@claude-plugins-official, with every server and skill it carries."
                )
            );
        }
        other => panic!("Disable is not offered: {other:?}"),
    }

    let plan = plan(
        &core,
        &Context::default(),
        Action::Disable,
        &["plugin:supabase:supabase"],
    )
    .unwrap();

    // The preview lists every Item the plugin takes with it, here and in
    // the Project, where the plugin applies too.
    let expected = triples(&[
        ("plugin:supabase:supabase", "effective", "disabled"),
        ("supabase:supabase-postgres", "effective", "disabled"),
    ]);
    assert_eq!(changes(&plan, None), expected);
    assert_eq!(changes(&plan, Some(Path::new(PETME))), expected);
    let classes: Vec<String> = plan.warnings().iter().map(|w| code(w.class)).collect();
    assert_eq!(classes, ["wide-reach"]);

    core.execute(plan, &[WarningClass::WideReach]).unwrap();

    assert_eq!(
        json_of(&fs, user_settings()),
        json!({
            "enabledPlugins": {
                "supabase@claude-plugins-official": false,
                "context7@claude-plugins-official": true,
                "superpowers@claude-plugins-official": false
            }
        })
    );
    let after = no_project(&fs);
    for name in ["plugin:supabase:supabase", "supabase:supabase-postgres"] {
        let entry = &item(&after, name).entries[0];
        assert_eq!(code(entry.state), "disabled", "{name}");
        assert_eq!(code(entry.reason.as_ref().unwrap().code), "plugin_disabled");
    }
}

#[test]
fn enable_of_a_plugin_entry_turns_the_whole_plugin_back_on() {
    let fs = fixture();
    let core = core(&fs);

    let plan = plan(
        &core,
        &Context::default(),
        Action::Enable,
        &["superpowers:brainstorming"],
    )
    .unwrap();
    assert_eq!(
        changes(&plan, None),
        triples(&[
            (
                "plugin:superpowers:episodic-memory",
                "disabled",
                "effective"
            ),
            ("superpowers:brainstorming", "disabled", "effective"),
        ])
    );
    core.execute(plan, &[WarningClass::WideReach]).unwrap();

    assert_eq!(
        json_of(&fs, user_settings())["enabledPlugins"]["superpowers@claude-plugins-official"],
        json!(true)
    );
    let after = no_project(&fs);
    assert_eq!(
        code(item(&after, "superpowers:brainstorming").state),
        "effective"
    );
    assert_eq!(
        code(item(&after, "plugin:superpowers:episodic-memory").state),
        "effective"
    );
}

#[test]
fn disable_of_a_plugin_entry_in_a_project_writes_its_local_settings() {
    let fs = fixture();
    let core = core(&fs);

    let plan = plan(
        &core,
        &in_petme(),
        Action::Disable,
        &["plugin:ponytail:pony"],
    )
    .unwrap();
    assert_eq!(
        changes(&plan, Some(Path::new(PETME))),
        triples(&[
            ("plugin:ponytail:pony", "effective", "disabled"),
            ("plugin:ponytail:tail", "effective", "disabled"),
        ])
    );
    core.execute(plan, &[WarningClass::WideReach]).unwrap();

    assert_eq!(
        json_of(&fs, petme_local_settings()),
        json!({ "enabledPlugins": { "ponytail@ponytail": false } })
    );
}

#[test]
fn a_plugin_entry_shows_how_to_remove_it_in_place_of_remove() {
    let fs = fixture();
    let core = core(&fs);
    let snapshot = core.scan(&ScanOptions::default()).unwrap();

    let skill = core
        .show(&snapshot, &Context::default(), "supabase:supabase-postgres")
        .unwrap();
    let command = "claude plugin uninstall supabase@claude-plugins-official --scope user";
    assert_eq!(
        entry_json(&skill.entries[0])["remove"],
        json!({
            "status": "not_offered",
            "code": "plugin",
            "sentence": "Not in v0.1: uninstall the plugin supabase@claude-plugins-official in Claude Code.",
            "how_to_remove": {
                "command": command,
                "sentence": format!("How to remove: uninstall the plugin supabase@claude-plugins-official with `{command}`. It takes every server and skill of the plugin with it."),
            }
        })
    );
    // Disable is offered instead.
    assert!(matches!(
        skill.entries[0].disable,
        DisableOption::Offered { .. }
    ));

    let Err(error) = plan(
        &core,
        &Context::default(),
        Action::Remove,
        &["supabase:supabase-postgres"],
    ) else {
        panic!("Remove of a plugin Entry is refused");
    };
    assert_eq!(code(error.code), "blocked");
    assert!(error.message.contains(command), "{}", error.message);
}

#[test]
fn an_entry_of_a_file_can_be_removed() {
    let inventory = no_project(&fixture());

    let docs = item(&inventory, "docs");
    assert_eq!(
        entry_json(&docs.entries[0])["remove"],
        json!({ "status": "offered" })
    );
}

/// An Agent that imports Claude Code's plugins, as Cursor does: it reads
/// their servers and Claude Code's `enabledPlugins`.
const PLUGIN_IMPORTER: &str = r#"
format_version = 1

[agent]
id = "plugin-importer"
name = "Plugin Importer"

[[homes]]
id = "default"
folder = { block = "path", path = "~" }

[homes.roots]
plugins = ".claude/plugins"
settings_file = ".claude/settings.json"

[[scopes]]
id = "plugin"

[[plugins]]
id = "claude-code"
registry = { block = "json-installs", anchor = { block = "home", root = "plugins", path = "installed_plugins.json" }, root = ["plugins"], folder = "installPath", scope = "scope", project = "projectPath" }
name = { file = ".claude-plugin/plugin.json", key_path = ["name"] }
uninstall = "claude plugin uninstall {id}"

[[locations]]
id = "plugin-manifest-mcp"
anchor = { block = "plugin", plugins = "claude-code", path = ".claude-plugin/plugin.json" }
name_template = "{plugin}-{name}"
references = { root = ["mcpServers"] }
format = { block = "json", dialect = "strict" }
root = ["mcpServers"]
kind = "mcp"
scope = "plugin"
provenance = "plugin"
store_class = "plain"
on_read_failure = "ignores_file"

[items.mcp]
precedence = ["plugin-manifest-mcp"]
competition = { block = "exact-name" }
checks = ["disable", "precedence"]

[[items.mcp.disable]]
block = "json-field"
switch = { anchor = { block = "home", root = "settings_file" }, key_path = ["enabledPlugins", "{plugin}"], store_class = "plain" }
target = "plugin"
context = "all"
enable_action = "set"
enabled_value = true
disabled_value = false
reason = "plugin_disabled"
locations = ["plugin-manifest-mcp"]
"#;

#[test]
fn disable_of_a_plugin_reaches_every_agent_that_reads_its_switch() {
    let fs = fixture();
    let env = FakeEnvironment::with_home(HOME).with_var("LOCALAPPDATA", LOCAL_APP_DATA);
    let core = Core::with_extra_definitions(
        fake::ports(fs.clone(), env),
        &[("plugin-importer.toml", PLUGIN_IMPORTER)],
    )
    .unwrap();

    let plan = plan(
        &core,
        &Context::default(),
        Action::Disable,
        &["plugin:supabase:supabase"],
    )
    .unwrap();

    let reached: Vec<(&str, &str, String)> = plan
        .preview()
        .changes
        .iter()
        .filter(|change| change.project.is_none())
        .map(|change| {
            (
                change.agent.as_str(),
                change.item.as_str(),
                code(change.after),
            )
        })
        .collect();
    assert!(
        reached.contains(&(
            "plugin-importer",
            "supabase-supabase",
            "disabled".to_owned()
        )),
        "{reached:?}"
    );
    assert!(
        reached.contains(&(
            "claude-code",
            "plugin:supabase:supabase",
            "disabled".to_owned()
        )),
        "{reached:?}"
    );
    assert_eq!(plan.preview().files.len(), 1);
}

#[test]
fn a_plugin_installed_for_a_missing_project_leaves_no_leftovers() {
    let fs = fixture();
    fs.delete_folder(PETME);
    let core = core(&fs);
    let snapshot = core.scan(&ScanOptions::default()).unwrap();

    let leftovers = core.leftovers(&snapshot);

    let names: Vec<&str> = leftovers
        .iter()
        .flat_map(|leftovers| &leftovers.items)
        .map(|item| item.name.as_str())
        .collect();
    assert!(
        !names
            .iter()
            .any(|name| name.starts_with("plugin:ponytail:")),
        "a plugin's Entries are no leftovers: {names:?}"
    );
}

#[test]
fn skill_overrides_does_not_turn_off_a_plugin_skill() {
    let fs = fixture().with_file(
        petme_local_settings(),
        r#"{ "skillOverrides": { "supabase:supabase-postgres": "off" } }"#,
    );

    let inventory = inventory(&core(&fs), &in_petme());

    assert_eq!(
        code(item(&inventory, "supabase:supabase-postgres").state),
        "effective"
    );
}
