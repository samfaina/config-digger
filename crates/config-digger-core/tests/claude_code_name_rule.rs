//! Claude Code's skill name rule through the core `api`, with fake ports
//! over fixture skill folders: the skills whose folder name breaks it are
//! Invalid, a frontmatter `name` that breaks it counts as no `name`, and
//! the gate `tengu_slate_wicket` decides whether it applies.

use config_digger_core::api::{Context, Core, Inventory, Item, ScanOptions};
use config_digger_core::ports::fake::{self, FakeEnvironment, FakeFileSystem};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const HOME: &str = r"C:\Users\fixture";
const LOCAL_APP_DATA: &str = r"C:\Users\fixture\AppData\Local";

/// The reason of a skill whose folder name breaks the rule.
const FOLDER_BREAKS_RULE: &str = "Claude Code skips this skill: its folder's name breaks Claude Code's name rule, which refuses a space at either end and hidden, direction-changing or blank-like characters, `<` and `>`.";

fn in_home(relative: &str) -> PathBuf {
    PathBuf::from(HOME).join(relative)
}

/// A `SKILL.md` whose frontmatter names the skill `name`.
fn skill_md(name: &str) -> String {
    format!("---\nname: {name}\ndescription: The skill.\n---\n\nDo it.\n")
}

/// `fs` with a skill folder at `folder` whose `SKILL.md` is `text`.
fn with_skill(fs: FakeFileSystem, folder: impl AsRef<Path>, text: &str) -> FakeFileSystem {
    fs.with_file(folder.as_ref().join("SKILL.md"), text.to_owned())
}

/// `fs` with a user skill in the folder `folder`.
fn with_user_skill(fs: FakeFileSystem, folder: &str) -> FakeFileSystem {
    let path = in_home(r".claude\skills").join(folder);
    with_skill(fs, path, &skill_md("cd-throwaway-skill"))
}

/// A Claude Code home whose `~/.claude.json` holds `state`.
fn claude_home(state: Value) -> FakeFileSystem {
    FakeFileSystem::new().with_file(in_home(".claude.json"), state.to_string())
}

fn inventory(fs: &FakeFileSystem) -> Inventory {
    inventory_with(fs, FakeEnvironment::with_home(HOME))
}

fn inventory_with(fs: &FakeFileSystem, env: FakeEnvironment) -> Inventory {
    let env = env.with_var("LOCALAPPDATA", LOCAL_APP_DATA);
    let core = Core::new(fake::ports(fs.clone(), env)).unwrap();
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    core.resolve(&snapshot, &Context::default())
}

fn skill<'i>(inventory: &'i Inventory, name: &str) -> &'i Item {
    inventory
        .items
        .iter()
        .find(|item| code(item.kind) == "skill" && item.name == name)
        .unwrap_or_else(|| panic!("no skill `{name}`"))
}

/// A serialised value as its JSON string, such as a State's `"invalid"`.
fn code(value: impl serde::Serialize) -> String {
    match serde_json::to_value(value).unwrap() {
        Value::String(code) => code,
        other => panic!("not a code: {other}"),
    }
}

/// The State and reason of the only Entry of the skill `name`.
fn state(inventory: &Inventory, name: &str) -> (String, Value) {
    let item = skill(inventory, name);
    assert_eq!(item.entries.len(), 1, "{name}");
    let entry = serde_json::to_value(&item.entries[0]).unwrap();
    (code(item.state), entry["reason"]["sentence"].clone())
}

const REFUSED_FOLDERS: [&str; 6] = [
    " cd-throwaway-lead",
    "cd-throwaway-trail ",
    "cd-throwaway\u{200B}zwsp",
    "cd-throwaway\u{202E}rlo",
    "cd-throwaway\u{00A0}nbsp",
    "cd-throwaway\u{3000}ideo",
];

const KEPT_FOLDERS: [&str; 4] = [
    "cd-throwaway inner",
    "cd-throwaway.dot",
    "CD-Throwaway-Upper",
    "cd-throwaway-ñandú",
];

#[test]
fn a_user_skill_whose_folder_name_breaks_the_name_rule_is_invalid() {
    let mut fs = claude_home(json!({}));
    for folder in REFUSED_FOLDERS.iter().chain(&KEPT_FOLDERS) {
        fs = with_user_skill(fs, folder);
    }

    let inventory = inventory(&fs);

    for folder in REFUSED_FOLDERS {
        let (state, reason) = state(&inventory, folder);
        assert_eq!(state, "invalid", "{folder:?}");
        assert_eq!(reason, FOLDER_BREAKS_RULE, "{folder:?}");
    }
    for folder in KEPT_FOLDERS {
        assert_eq!(state(&inventory, folder).0, "effective", "{folder:?}");
    }
}

const DEFINITIONS: &str = r"E:\dev\definitions";

/// An Agent that reads skills from `~/.ruled/skills`, under the name rule,
/// above `~/.plain/skills`, under none.
const RULED_ABOVE_PLAIN: &str = r#"
format_version = 1

[agent]
id = "ruled"
name = "Ruled"

[[homes]]
id = "default"
folder = { block = "path", path = "~" }
roots.ruled = ".ruled"
roots.plain = ".plain"

[[scopes]]
id = "user"

[[locations]]
id = "ruled-skills"
anchor = { block = "home", root = "ruled", path = "skills" }
format = { block = "skill-folder", name_from = "folder", name_rule = "visible-chars", no_frontmatter = "folder-name", malformed_frontmatter = "folder-name", no_name = "folder-name" }
kind = "skill"
scope = "user"
provenance = "file"
store_class = "plain"
on_read_failure = "ignores_file"

[[locations]]
id = "plain-skills"
anchor = { block = "home", root = "plain", path = "skills" }
format = { block = "skill-folder", name_from = "folder", no_frontmatter = "folder-name", malformed_frontmatter = "folder-name", no_name = "folder-name" }
kind = "skill"
scope = "user"
provenance = "file"
store_class = "plain"
on_read_failure = "ignores_file"

[items.skill]
precedence = ["ruled-skills", "plain-skills"]
competition = { block = "exact-name" }
checks = ["frontmatter", "disable", "precedence"]
"#;

#[test]
fn a_skill_whose_folder_name_breaks_the_name_rule_competes_with_nothing() {
    let folder = "cd-throwaway\u{200B}zwsp";
    let fs = FakeFileSystem::new()
        .with_file(Path::new(DEFINITIONS).join("ruled.toml"), RULED_ABOVE_PLAIN);
    let fs = with_skill(fs, in_home(r".ruled\skills").join(folder), &skill_md("a"));
    let fs = with_skill(fs, in_home(r".plain\skills").join(folder), &skill_md("b"));
    let ports = fake::ports(fs, FakeEnvironment::with_home(HOME));
    let loaded = Core::with_definitions_folder(ports, Path::new(DEFINITIONS)).unwrap();
    assert!(loaded.rejected.is_empty(), "{:?}", loaded.rejected);
    let snapshot = loaded.core.scan(&ScanOptions::default()).unwrap();

    let inventory = loaded.core.resolve(&snapshot, &Context::default());

    let items: Vec<&Item> = inventory
        .items
        .iter()
        .filter(|item| item.agent == "ruled" && item.name == folder)
        .collect();
    let states: Vec<(String, String)> = items
        .iter()
        .flat_map(|item| &item.entries)
        .map(|entry| {
            let entry = serde_json::to_value(entry).unwrap();
            let path = entry["path"].as_str().unwrap().to_owned();
            (path, entry["state"].as_str().unwrap().to_owned())
        })
        .collect();
    assert_eq!(states.len(), 2, "{states:?}");
    assert!(states.contains(&(
        in_home(r".ruled\skills").join(folder).display().to_string(),
        "invalid".to_owned()
    )));
    assert!(states.contains(&(
        in_home(r".plain\skills").join(folder).display().to_string(),
        "effective".to_owned()
    )));
}

const PLUGIN: &str =
    r"C:\Users\fixture\.claude\plugins\cache\cd-throwaway-mkt\cd-throwaway-pk\1.0.0";

/// A Claude Code home whose `~/.claude.json` holds `state`, with the
/// plugin `cd-throwaway-pk` installed and on, whose manifest names one
/// skill folder `n<index>` for each frontmatter `name` of `names`.
fn with_plugin_skills(state: Value, names: &[String]) -> FakeFileSystem {
    let plugin = Path::new(PLUGIN);
    let installed = json!({
        "version": 2,
        "plugins": { "cd-throwaway-pk@cd-throwaway-mkt": [{
            "scope": "user",
            "installPath": PLUGIN,
            "version": "1.0.0",
            "installedAt": "2026-08-01T10:00:00.000Z",
            "lastUpdated": "2026-08-01T10:00:00.000Z"
        }] }
    });
    let folders: Vec<String> = (0..names.len())
        .map(|index| format!("./n{index}/"))
        .collect();
    let manifest = json!({ "name": "cd-throwaway-pk", "version": "1.0.0", "skills": folders });
    let mut fs = claude_home(state)
        .with_file(
            in_home(r".claude\plugins\installed_plugins.json"),
            installed.to_string(),
        )
        .with_file(
            in_home(r".claude\settings.json"),
            json!({ "enabledPlugins": { "cd-throwaway-pk@cd-throwaway-mkt": true } }).to_string(),
        )
        .with_file(
            plugin.join(r".claude-plugin\plugin.json"),
            manifest.to_string(),
        );
    for (index, name) in names.iter().enumerate() {
        let text = format!("---\nname: {name}\ndescription: d{index}\n---\n\nDo it.\n");
        fs = with_skill(fs, plugin.join(format!("n{index}")), &text);
    }
    fs
}

/// The frontmatter `name`s, as YAML writes them, that the rule refuses.
fn refused_names() -> Vec<String> {
    [
        "cd-throwaway spaced",
        r#""cd-throwaway\ttab""#,
        "cd-throwaway/slash",
        "cd-throwaway<lt",
        "cd-throwaway>gt",
        "cd-throwaway\u{200B}zwsp",
    ]
    .map(str::to_owned)
    .into_iter()
    .chain([format!("cd-throwaway-{}", "x".repeat(244))])
    .collect()
}

/// The note of a plugin skill whose `name` the rule refuses, in `folder`.
fn refused_name_note(folder: &str) -> String {
    format!(
        "Claude Code still loads this skill, as `{folder}`, its folder's name: its SKILL.md frontmatter `name` breaks Claude Code's name rule, which refuses blanks, `/`, `<`, `>`, hidden and direction-changing characters and names over 256 characters."
    )
}

#[test]
fn a_frontmatter_name_that_breaks_the_name_rule_counts_as_no_name() {
    let refused = refused_names();
    let fs = with_plugin_skills(json!({}), &refused);

    let inventory = inventory(&fs);

    for (index, name) in refused.iter().enumerate() {
        let folder = format!("n{index}");
        let item = skill(&inventory, &format!("cd-throwaway-pk:{folder}"));
        assert_eq!(code(item.state), "effective", "{name}");
        let entry = serde_json::to_value(&item.entries[0]).unwrap();
        assert_eq!(
            entry["notes"][0]["sentence"],
            refused_name_note(&folder),
            "{name}"
        );
    }
}

#[test]
fn a_frontmatter_name_of_256_characters_or_with_a_colon_at_either_end_is_kept() {
    let kept = [
        format!("cd-throwaway-{}", "x".repeat(243)),
        r#"":cd-throwaway-lead""#.to_owned(),
        r#""cd-throwaway-trail:""#.to_owned(),
    ];
    let fs = with_plugin_skills(json!({}), &kept);

    let inventory = inventory(&fs);

    for name in kept {
        let name = name.trim_matches('"');
        let item = skill(&inventory, &format!("cd-throwaway-pk:{name}"));
        assert_eq!(code(item.state), "effective", "{name}");
        let entry = serde_json::to_value(&item.entries[0]).unwrap();
        assert_eq!(entry["notes"], json!([]), "{name}");
    }
}

#[test]
fn a_frontmatter_name_that_breaks_the_name_rule_leaves_a_skill_named_by_its_folder_alone() {
    let mut fs = claude_home(json!({}));
    for (index, name) in refused_names().iter().enumerate() {
        let text = format!("---\nname: {name}\ndescription: d{index}\n---\n\nDo it.\n");
        fs = with_skill(
            fs,
            in_home(r".claude\skills").join(format!("n{index}")),
            &text,
        );
    }

    let inventory = inventory(&fs);

    for index in 0..refused_names().len() {
        let item = skill(&inventory, &format!("n{index}"));
        assert_eq!(code(item.state), "effective", "n{index}");
        let entry = serde_json::to_value(&item.entries[0]).unwrap();
        assert_eq!(entry["notes"], json!([]), "n{index}");
    }
}

const GATE: &str = "tengu_slate_wicket";

/// `~/.claude.json` with the gate cached as `on`.
fn gate_cached(on: bool) -> Value {
    json!({
        "cachedGrowthBookFeatures": { GATE: on },
        "cachedGrowthBookFeaturesAt": 1790930400000u64
    })
}

#[test]
fn with_the_gate_cached_off_no_name_rule_applies() {
    let refused = refused_names();
    let fs = with_user_skill(
        with_plugin_skills(gate_cached(false), &refused),
        " cd-throwaway-lead",
    );

    let inventory = inventory(&fs);

    assert_eq!(state(&inventory, " cd-throwaway-lead").0, "effective");
    let item = skill(&inventory, "cd-throwaway-pk:cd-throwaway spaced");
    assert_eq!(code(item.state), "effective");
    let entry = serde_json::to_value(&item.entries[0]).unwrap();
    assert_eq!(entry["notes"], json!([]));
    assert_eq!(
        entry["caveats"][0]["sentence"],
        "Depends on Claude Code gate `tengu_slate_wicket`: off as of 2 Oct 2026 08:40 UTC."
    );
}

#[test]
fn with_the_gate_cached_on_or_without_a_value_the_name_rule_applies() {
    for (state_file, caveat) in [
        (
            gate_cached(true),
            "Depends on Claude Code gate `tengu_slate_wicket`: on as of 2 Oct 2026 08:40 UTC.",
        ),
        (
            json!({}),
            "Depends on Claude Code gate `tengu_slate_wicket`: assumed on; cannot be read.",
        ),
    ] {
        let fs = with_user_skill(claude_home(state_file), " cd-throwaway-lead");
        let fs = with_user_skill(fs, "cd-throwaway-kept");

        let inventory = inventory(&fs);

        let (state, reason) = state(&inventory, " cd-throwaway-lead");
        assert_eq!(state, "invalid");
        assert_eq!(reason, FOLDER_BREAKS_RULE);
        let item = skill(&inventory, " cd-throwaway-lead");
        let entry = serde_json::to_value(&item.entries[0]).unwrap();
        assert_eq!(entry["caveats"][0]["sentence"], caveat);
        let kept =
            serde_json::to_value(&skill(&inventory, "cd-throwaway-kept").entries[0]).unwrap();
        assert_eq!(
            kept["caveats"],
            json!([]),
            "a skill the rule does not change"
        );
    }
}

#[test]
fn while_claude_code_skips_its_cached_gates_the_name_rule_applies() {
    let fs = with_user_skill(claude_home(gate_cached(false)), " cd-throwaway-lead");
    let env = FakeEnvironment::with_home(HOME).with_var("DISABLE_GROWTHBOOK", "1");

    let inventory = inventory_with(&fs, env);

    assert_eq!(state(&inventory, " cd-throwaway-lead").0, "invalid");
}
