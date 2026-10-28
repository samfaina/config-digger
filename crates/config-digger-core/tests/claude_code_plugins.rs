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
fn skills_in_every_folder_the_manifest_declares_are_read_besides_the_skills_folder() {
    let inventory = no_project(&fixture());

    // toolbox's manifest declares `./skills/` again, `./extra-skills/`, a
    // folder of skill folders, and `./solo/` and `.`, each one skill's
    // folder, which the skill's frontmatter `name` names.
    let folder = in_cache(r"claude-plugins-official\toolbox\2.0.0");
    for (name, path) in [
        ("toolbox:core", folder.join(r"skills\core")),
        ("toolbox:lint", folder.join(r"extra-skills\lint")),
        ("toolbox:solo-skill", folder.join("solo")),
        ("toolbox:guide", folder.clone()),
    ] {
        let skill = item(&inventory, name);
        let paths: Vec<&Path> = skill
            .entries
            .iter()
            .map(|entry| entry.path.as_path())
            .collect();
        assert_eq!(paths, [path], "{name}");
        assert_eq!(code(skill.state), "effective", "{name}");
        let entry = entry_json(&skill.entries[0]);
        assert_eq!(entry["scope"], "plugin", "{name}");
        assert_eq!(entry["provenance"], "plugin", "{name}");
        assert_eq!(entry["plugin"]["id"], "toolbox@claude-plugins-official");
    }
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

/// toolbox's server, as Claude Code runs it: `${CLAUDE_PLUGIN_ROOT}` is the
/// plugin's folder, with forward slashes.
const TOOLBOX_RUNNER: &str =
    "C:/Users/fixture/.claude/plugins/cache/claude-plugins-official/toolbox/2.0.0/runner.js";

#[test]
fn claude_plugin_root_expands_to_the_plugins_folder_in_its_servers() {
    let inventory = no_project(&fixture());

    let runner = item(&inventory, "plugin:toolbox:runner");
    assert_eq!(
        runner.entries[0].config,
        json!({
            "command": "node",
            "args": [TOOLBOX_RUNNER, "--data", "${CLAUDE_PLUGIN_DATA}"]
        })
    );
}

#[test]
fn servers_two_plugins_write_alike_with_claude_plugin_root_do_not_compete() {
    let fs = fixture().with_file(
        in_cache(r"claude-plugins-official\context7\1.0.0\.mcp.json"),
        json!({ "mcpServers": {
            "context7": { "type": "http", "url": "https://MCP.context7.com:443/mcp/" },
            "runner": {
                "command": "node",
                "args": ["${CLAUDE_PLUGIN_ROOT}/runner.js", "--data", "${CLAUDE_PLUGIN_DATA}"]
            },
        } })
        .to_string(),
    );

    let inventory = no_project(&fs);

    for name in ["plugin:toolbox:runner", "plugin:context7:runner"] {
        let runner = item(&inventory, name);
        assert_eq!(runner.entries.len(), 1, "{name}");
        assert_eq!(code(runner.state), "effective", "{name}");
    }
}

#[test]
fn a_server_running_the_expanded_command_of_a_plugin_server_shadows_it() {
    let fs = fixture();
    let mut state = json_of(&fs, PathBuf::from(HOME).join(".claude.json"));
    state["mcpServers"]["runner"] =
        json!({ "command": "node", "args": [TOOLBOX_RUNNER, "--data", "${CLAUDE_PLUGIN_DATA}"] });
    fs.put_file(PathBuf::from(HOME).join(".claude.json"), state.to_string());

    let inventory = no_project(&fs);

    let runner = item(&inventory, "runner");
    let states: Vec<String> = runner
        .entries
        .iter()
        .map(|entry| code(entry.state))
        .collect();
    assert_eq!(states, ["effective", "shadowed"]);
    assert_eq!(
        runner.entries[1].path,
        in_cache(r"claude-plugins-official\toolbox\2.0.0\.mcp.json")
    );
}

/// Where Claude Code extracted toolbox's `bundle.mcpb`.
fn toolbox_bundle() -> PathBuf {
    in_cache(r"claude-plugins-official\toolbox\2.0.0\.mcpb-cache\8739c76e681f9009")
}

/// The server of toolbox's bundle, as Claude Code runs it: `${__dirname}`
/// is the folder the bundle is extracted to.
fn weather_server() -> String {
    format!("{}/server/index.js", toolbox_bundle().display())
}

#[test]
fn a_bundle_the_manifest_declares_is_a_server_read_from_the_bundles_manifest() {
    let inventory = no_project(&fixture());

    let weather = item(&inventory, "plugin:toolbox:weather");
    assert_eq!(code(weather.state), "effective");
    let entry = &weather.entries[0];
    assert_eq!(entry.path, toolbox_bundle().join("manifest.json"));
    assert_eq!(entry.key_path, ["server", "mcp_config"]);
    assert_eq!(entry.config["command"], "node");
    assert_eq!(entry.config["args"], json!([weather_server()]));
    // A literal env value is a Secret, as in any other server.
    assert_eq!(entry.masked, [["env", "UNITS"]]);
    let entry = entry_json(entry);
    assert_eq!(entry["scope"], "plugin");
    assert_eq!(entry["provenance"], "plugin");
    assert_eq!(entry["plugin"]["id"], "toolbox@claude-plugins-official");
}

#[test]
fn a_bundle_claude_code_has_not_extracted_yet_has_no_entry() {
    let fs = fixture();
    fs.delete_folder(in_cache(
        r"claude-plugins-official\toolbox\2.0.0\.mcpb-cache",
    ));

    let inventory = no_project(&fs);

    assert!(
        !inventory
            .items
            .iter()
            .any(|item| item.name == "plugin:toolbox:weather")
    );
}

#[test]
fn a_bundles_server_competes_by_endpoint_like_other_plugin_servers() {
    let fs = fixture();
    let mut state = json_of(&fs, PathBuf::from(HOME).join(".claude.json"));
    state["mcpServers"]["forecast"] = json!({ "command": "node", "args": [weather_server()] });
    fs.put_file(PathBuf::from(HOME).join(".claude.json"), state.to_string());

    let inventory = no_project(&fs);

    let forecast = item(&inventory, "forecast");
    let states: Vec<(&Path, String)> = forecast
        .entries
        .iter()
        .map(|entry| (entry.path.as_path(), code(entry.state)))
        .collect();
    assert_eq!(
        states,
        [
            (
                PathBuf::from(HOME).join(".claude.json").as_path(),
                "effective".to_owned()
            ),
            (
                toolbox_bundle().join("manifest.json").as_path(),
                "shadowed".to_owned()
            ),
        ]
    );
}

#[test]
fn disable_of_a_plugin_lists_its_bundle_server_and_the_skills_of_every_declared_folder() {
    let fs = fixture();
    let core = core(&fs);

    let plan = plan(
        &core,
        &Context::default(),
        Action::Disable,
        &["toolbox:lint"],
    )
    .unwrap();

    assert_eq!(
        changes(&plan, None),
        triples(&[
            ("plugin:toolbox:runner", "effective", "disabled"),
            ("plugin:toolbox:weather", "effective", "disabled"),
            ("toolbox:core", "effective", "disabled"),
            ("toolbox:guide", "effective", "disabled"),
            ("toolbox:lint", "effective", "disabled"),
            ("toolbox:solo-skill", "effective", "disabled"),
        ])
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

#[test]
fn a_plugin_no_settings_file_names_is_off_with_everything_it_carries() {
    let fs = fixture();
    let mut settings = json_of(&fs, user_settings());
    settings["enabledPlugins"]
        .as_object_mut()
        .unwrap()
        .remove("supabase@claude-plugins-official");
    fs.put_file(user_settings(), settings.to_string());

    let inventory = no_project(&fs);

    let supabase = item(&inventory, "supabase:supabase-postgres");
    assert_eq!(code(supabase.state), "disabled");
    let reason = supabase.entries[0].reason.as_ref().unwrap();
    assert_eq!(code(reason.code), "plugin_disabled");
    assert!(
        reason.sentence.contains("is missing in") && reason.sentence.contains("counts as false"),
        "{}",
        reason.sentence
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
                "superpowers@claude-plugins-official": false,
                "toolbox@claude-plugins-official": true
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
registry = { block = "json-plugin-records", anchor = { block = "home", root = "plugins", path = "installed_plugins.json" }, root = ["plugins"], folder = "installPath", scope = "scope", project = "projectPath" }
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
switch = { anchor = { block = "home", root = "settings_file" }, key_path = ["enabledPlugins", "{plugin_id}"], store_class = "plain" }
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

#[test]
fn a_more_local_settings_file_turns_a_plugin_back_on() {
    // The user settings turn superpowers off; petme's local settings turn
    // it on, and Claude Code merges the key local over Project over user.
    let fs = fixture().with_file(
        petme_local_settings(),
        r#"{ "enabledPlugins": { "superpowers@claude-plugins-official": true } }"#,
    );

    let in_petme = inventory(&core(&fs), &in_petme());
    assert_eq!(
        code(item(&in_petme, "superpowers:brainstorming").state),
        "effective"
    );
    let elsewhere = no_project(&fs);
    assert_eq!(
        code(item(&elsewhere, "superpowers:brainstorming").state),
        "disabled"
    );
}

#[test]
fn a_server_the_manifest_declares_replaces_the_same_named_one_of_the_plugins_mcp_json() {
    let fs = fixture().with_file(
        in_cache(r"claude-plugins-official\context7\1.0.0\.claude-plugin\plugin.json"),
        r#"{ "name": "context7", "mcpServers": { "context7": { "url": "https://context7.example/v2" } } }"#,
    );

    let inventory = no_project(&fs);

    let manifest = in_cache(r"claude-plugins-official\context7\1.0.0\.claude-plugin\plugin.json");
    let context7 = item(&inventory, "plugin:context7:context7");
    let states: Vec<(&Path, String)> = context7
        .entries
        .iter()
        .map(|entry| (entry.path.as_path(), code(entry.state)))
        .collect();
    assert_eq!(
        states,
        [
            (manifest.as_path(), "effective".to_owned()),
            (
                in_cache(r"claude-plugins-official\context7\1.0.0\.mcp.json").as_path(),
                "shadowed".to_owned()
            ),
        ]
    );
    let reason = context7.entries[1].reason.as_ref().unwrap();
    assert!(
        !reason.sentence.contains("same endpoint"),
        "{}",
        reason.sentence
    );
}

const PROGRAM_FILES: &str = r"C:\Program Files";

fn managed_settings() -> PathBuf {
    PathBuf::from(PROGRAM_FILES).join(r"ClaudeCode\managed-settings.json")
}

/// The fixture, with `enabledPlugins` in the managed settings file.
fn with_managed_plugins(enabled: Value) -> Core {
    let fs = fixture().with_file(
        managed_settings(),
        json!({ "enabledPlugins": enabled }).to_string(),
    );
    let env = FakeEnvironment::with_home(HOME)
        .with_var("LOCALAPPDATA", LOCAL_APP_DATA)
        .with_var("ProgramFiles", PROGRAM_FILES);
    Core::new(fake::ports(fs, env)).unwrap()
}

#[test]
fn a_plugin_the_managed_settings_force_off_is_blocked_by_policy() {
    let core = with_managed_plugins(json!({ "supabase@claude-plugins-official": false }));

    let inventory = inventory(&core, &Context::default());

    let switch = format!(
        r"{PROGRAM_FILES}\ClaudeCode\managed-settings.json#enabledPlugins.supabase@claude-plugins-official"
    );
    for name in ["plugin:supabase:supabase", "supabase:supabase-postgres"] {
        let entry = entry_json(&item(&inventory, name).entries[0]);
        assert_eq!(entry["state"], "blocked", "{name}");
        assert_eq!(
            entry["reason"],
            json!({
                "code": "policy",
                "effective": null,
                "switch": switch,
                "sentence": format!(
                    r"Blocked by policy: `enabledPlugins.supabase@claude-plugins-official` is false in {PROGRAM_FILES}\ClaudeCode\managed-settings.json, so Claude Code skips every server and skill of the plugin supabase@claude-plugins-official."
                ),
            }),
            "{name}"
        );
        assert_eq!(entry["enable"], Value::Null, "{name}");
    }
}

#[test]
fn enable_of_a_plugin_the_managed_settings_force_off_is_refused() {
    let core = with_managed_plugins(json!({ "superpowers@claude-plugins-official": false }));

    let refused = plan(
        &core,
        &Context::default(),
        Action::Enable,
        &["superpowers:brainstorming"],
    );

    assert!(refused.is_err(), "Enable was planned");
}

#[test]
fn a_plugin_the_managed_settings_force_on_cannot_be_disabled() {
    let core = with_managed_plugins(json!({ "superpowers@claude-plugins-official": true }));

    let inventory = inventory(&core, &Context::default());

    // The user settings turn it off, which the managed value overrides.
    for name in [
        "superpowers:brainstorming",
        "plugin:superpowers:episodic-memory",
    ] {
        let entry = entry_json(&item(&inventory, name).entries[0]);
        assert_eq!(entry["state"], "effective", "{name}");
        assert_eq!(
            entry["disable"],
            json!({
                "status": "not_offered",
                "code": "policy",
                "sentence": format!(
                    r"Disable is not offered: `enabledPlugins.superpowers@claude-plugins-official` is true in {PROGRAM_FILES}\ClaudeCode\managed-settings.json, which forces the plugin on, and config-digger never overrides managed policy."
                ),
            }),
            "{name}"
        );
    }
}

#[test]
fn of_merged_managed_sources_the_highest_holding_a_plugin_decides() {
    let fs = fixture()
        .with_file(
            PathBuf::from(HOME).join(r".claude\remote-settings.json"),
            json!({
                "managedSourcesBehavior": "merge",
                "enabledPlugins": { "supabase@claude-plugins-official": true },
            })
            .to_string(),
        )
        .with_file(
            managed_settings(),
            json!({ "enabledPlugins": {
                "supabase@claude-plugins-official": false,
                "context7@claude-plugins-official": false,
            } })
            .to_string(),
        );
    let env = FakeEnvironment::with_home(HOME)
        .with_var("LOCALAPPDATA", LOCAL_APP_DATA)
        .with_var("ProgramFiles", PROGRAM_FILES);
    let core = Core::new(fake::ports(fs, env)).unwrap();

    let inventory = inventory(&core, &Context::default());

    assert_eq!(
        code(item(&inventory, "plugin:supabase:supabase").state),
        "effective"
    );
    let path = in_cache(r"claude-plugins-official\context7\1.0.0\.mcp.json");
    let context7 = entry_at(&inventory, &path, &["mcpServers", "context7"]);
    assert_eq!(code(context7.state), "blocked");
}

#[test]
fn disable_of_a_plugin_the_managed_settings_force_on_is_refused() {
    let core = with_managed_plugins(json!({ "superpowers@claude-plugins-official": true }));

    let refused = plan(
        &core,
        &Context::default(),
        Action::Disable,
        &["superpowers:brainstorming"],
    );

    assert!(refused.is_err(), "Disable was planned");
}

#[test]
fn a_plugin_forced_off_is_blocked_before_its_servers_shape_is_checked() {
    let path = in_cache(r"claude-plugins-official\context7\1.0.0\.mcp.json");
    let fs = fixture()
        .with_file(
            &path,
            json!({ "mcpServers": { "context7": { "type": "http" } } }).to_string(),
        )
        .with_file(
            managed_settings(),
            json!({ "enabledPlugins": { "context7@claude-plugins-official": false } }).to_string(),
        );
    let env = FakeEnvironment::with_home(HOME)
        .with_var("LOCALAPPDATA", LOCAL_APP_DATA)
        .with_var("ProgramFiles", PROGRAM_FILES);
    let core = Core::new(fake::ports(fs, env)).unwrap();

    let inventory = inventory(&core, &Context::default());

    let entry = entry_json(entry_at(&inventory, &path, &["mcpServers", "context7"]));
    assert_eq!(entry["state"], "blocked");
    assert_eq!(entry["reason"]["code"], "policy");
    assert_eq!(entry["secondary"][0]["code"], "malformed_entry");
}

#[test]
fn remote_settings_force_a_plugin_off_as_the_one_managed_source() {
    let fs = fixture().with_file(
        PathBuf::from(HOME).join(r".claude\remote-settings.json"),
        json!({ "enabledPlugins": { "supabase@claude-plugins-official": false } }).to_string(),
    );

    let inventory = no_project(&fs);

    let entry = entry_json(&item(&inventory, "plugin:supabase:supabase").entries[0]);
    assert_eq!(entry["state"], "blocked");
    assert_eq!(entry["reason"]["code"], "policy");
}

// --- Two plugins' servers with one endpoint -----------------------------------

const SUPABASE: &str = "supabase@claude-plugins-official";
const CONTEXT7: &str = "context7@claude-plugins-official";

/// The fixture, where supabase's and context7's `.mcp.json` each carry a
/// server at `https://other.example.com/mcp`.
fn one_endpoint() -> FakeFileSystem {
    let other = json!({ "type": "http", "url": "https://other.example.com/mcp" });
    fixture()
        .with_file(
            in_cache(r"claude-plugins-official\supabase\0.1.4\.mcp.json"),
            json!({ "mcpServers": { "supa": other } }).to_string(),
        )
        .with_file(
            in_cache(r"claude-plugins-official\context7\1.0.0\.mcp.json"),
            json!({ "mcpServers": {
                "context7": { "type": "http", "url": "https://MCP.context7.com:443/mcp/" },
                "c7": other,
            } })
            .to_string(),
        )
}

/// `enabledPlugins` holding `plugins`, each turned on or off, in that order.
fn enabled_plugins(plugins: &[(&str, bool)]) -> String {
    let enabled: serde_json::Map<String, Value> = plugins
        .iter()
        .map(|(id, on)| ((*id).to_owned(), Value::Bool(*on)))
        .collect();
    json!({ "enabledPlugins": enabled }).to_string()
}

/// The supabase and context7 servers of [`one_endpoint`], by their files.
fn supa() -> PathBuf {
    in_cache(r"claude-plugins-official\supabase\0.1.4\.mcp.json")
}

fn c7() -> PathBuf {
    in_cache(r"claude-plugins-official\context7\1.0.0\.mcp.json")
}

/// The Item of the servers of [`one_endpoint`], its name and the State of
/// each of the two, supabase's first.
fn one_endpoint_states(inventory: &Inventory) -> (String, String, String) {
    let supa = entry_at(inventory, &supa(), &["mcpServers", "supa"]);
    let c7 = entry_at(inventory, &c7(), &["mcpServers", "c7"]);
    let item = inventory
        .items
        .iter()
        .find(|item| item.entries.iter().any(|entry| entry.path == supa.path))
        .unwrap();
    assert!(
        item.entries.iter().any(|entry| entry.path == c7.path),
        "the two servers are one Item"
    );
    (item.name.clone(), code(supa.state), code(c7.state))
}

#[test]
fn of_two_plugins_servers_with_one_endpoint_the_plugin_first_in_enabled_plugins_wins() {
    let fs = one_endpoint().with_file(
        user_settings(),
        enabled_plugins(&[(SUPABASE, true), (CONTEXT7, true)]),
    );
    assert_eq!(
        one_endpoint_states(&no_project(&fs)),
        (
            "plugin:supabase:supa".to_owned(),
            "effective".to_owned(),
            "shadowed".to_owned()
        )
    );

    let fs = one_endpoint().with_file(
        user_settings(),
        enabled_plugins(&[(CONTEXT7, true), (SUPABASE, true)]),
    );
    assert_eq!(
        one_endpoint_states(&no_project(&fs)),
        (
            "plugin:context7:c7".to_owned(),
            "shadowed".to_owned(),
            "effective".to_owned()
        )
    );
}

const TOOLBOX: &str = "toolbox@claude-plugins-official";

fn tb() -> PathBuf {
    in_cache(r"claude-plugins-official\toolbox\2.0.0\.mcp.json")
}

/// [`one_endpoint`], where toolbox's `.mcp.json` carries a server at the
/// same endpoint too.
fn one_endpoint_of_three() -> FakeFileSystem {
    let fs = one_endpoint();
    let mut servers = json_of(&fs, tb());
    servers["mcpServers"]["tb"] = json!({ "type": "http", "url": "https://other.example.com/mcp" });
    fs.with_file(tb(), servers.to_string())
}

/// The files of the Entries of the Item holding supabase's server of
/// [`one_endpoint`], in the Item's order, each with its State.
fn one_endpoint_order(inventory: &Inventory) -> Vec<(PathBuf, String)> {
    inventory
        .items
        .iter()
        .find(|item| item.entries.iter().any(|entry| entry.path == supa()))
        .unwrap()
        .entries
        .iter()
        .map(|entry| (entry.path.clone(), code(entry.state)))
        .collect()
}

fn petme_settings() -> PathBuf {
    PathBuf::from(PETME).join(r".claude\settings.json")
}

#[test]
fn enabled_plugins_is_ordered_by_the_users_then_the_projects_then_the_local_settings() {
    let fs = one_endpoint_of_three()
        .with_file(user_settings(), enabled_plugins(&[(TOOLBOX, true)]))
        .with_file(petme_settings(), enabled_plugins(&[(CONTEXT7, true)]))
        .with_file(petme_local_settings(), enabled_plugins(&[(SUPABASE, true)]));

    let in_petme = inventory(&core(&fs), &in_petme());

    assert_eq!(
        one_endpoint_order(&in_petme),
        [
            (tb(), "effective".to_owned()),
            (c7(), "shadowed".to_owned()),
            (supa(), "shadowed".to_owned()),
        ]
    );
}

#[test]
fn a_plugin_the_local_settings_name_again_keeps_its_place_from_the_users() {
    let fs = one_endpoint_of_three()
        .with_file(
            user_settings(),
            enabled_plugins(&[(SUPABASE, true), (CONTEXT7, true)]),
        )
        .with_file(
            petme_local_settings(),
            enabled_plugins(&[(CONTEXT7, true), (TOOLBOX, true)]),
        );

    let in_petme = inventory(&core(&fs), &in_petme());

    assert_eq!(
        one_endpoint_order(&in_petme),
        [
            (supa(), "effective".to_owned()),
            (c7(), "shadowed".to_owned()),
            (tb(), "shadowed".to_owned()),
        ]
    );
}

#[test]
fn when_the_plugin_first_in_enabled_plugins_is_off_the_next_plugins_server_is_effective() {
    let fs = one_endpoint().with_file(
        user_settings(),
        enabled_plugins(&[(SUPABASE, false), (CONTEXT7, true)]),
    );

    let (_, supa, c7) = one_endpoint_states(&no_project(&fs));

    assert_eq!((supa.as_str(), c7.as_str()), ("disabled", "effective"));
}

#[test]
fn a_server_shadowed_by_another_plugins_says_that_plugin_comes_first_in_enabled_plugins() {
    let fs = one_endpoint().with_file(
        user_settings(),
        enabled_plugins(&[(SUPABASE, true), (CONTEXT7, true)]),
    );

    let inventory = no_project(&fs);

    let reason = entry_at(&inventory, &c7(), &["mcpServers", "c7"])
        .reason
        .as_ref()
        .unwrap();
    let winner = format!(r"{}#mcpServers.supa", supa().display());
    assert_eq!(code(reason.code), "shadowed_by_precedence");
    assert_eq!(reason.effective.as_deref(), Some(winner.as_str()));
    assert_eq!(
        reason.sentence,
        format!(
            "Shadowed by the plugin Entry {winner}, which has the same endpoint and Claude Code ranks first, since its plugin comes first in `enabledPlugins`."
        )
    );
}

#[test]
fn between_two_plugins_enabled_plugins_decides_over_the_file_a_server_comes_from() {
    // supabase's manifest declares its servers in `agents/claude/.mcp.json`,
    // which ranks above a plugin's own `.mcp.json` within one plugin.
    let manifest_servers =
        in_cache(r"claude-plugins-official\supabase\0.1.4\agents\claude\.mcp.json");
    let fs = one_endpoint()
        .with_file(
            &manifest_servers,
            json!({ "mcpServers": {
                "manifest-supa": { "type": "http", "url": "https://other.example.com/mcp" },
            } })
            .to_string(),
        )
        .with_file(
            user_settings(),
            enabled_plugins(&[(CONTEXT7, true), (SUPABASE, true)]),
        );

    let inventory = no_project(&fs);

    let c7 = entry_at(&inventory, &c7(), &["mcpServers", "c7"]);
    let manifest_supa = entry_at(
        &inventory,
        &manifest_servers,
        &["mcpServers", "manifest-supa"],
    );
    assert_eq!(code(c7.state), "effective");
    assert_eq!(code(manifest_supa.state), "shadowed");
    assert_eq!(item(&inventory, "plugin:context7:c7").entries.len(), 3);
}
