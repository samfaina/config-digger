//! Claude Code skills through the core `api`, with fake ports over fixture
//! skill folders: where Claude Code reads them, which one wins a name, the
//! skills it skips or loads through a fallback, and Disable and Enable
//! through `skillOverrides`.

use config_digger_core::api::{
    Action, Context, Core, DisableOption, EnableOption, Entry, Error, Inventory, Item, Plan,
    ScanOptions, Targets, WarningClass,
};
use config_digger_core::ports::fake::{self, FakeEnvironment, FakeFileSystem};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const HOME: &str = r"C:\Users\fixture";
const LOCAL_APP_DATA: &str = r"C:\Users\fixture\AppData\Local";

fn in_home(relative: &str) -> PathBuf {
    PathBuf::from(HOME).join(relative)
}

/// A `SKILL.md` whose frontmatter names the skill `name`.
fn skill_md(name: &str) -> String {
    format!("---\nname: {name}\ndescription: The {name} skill.\n---\n\nDo {name}.\n")
}

/// `fs` with a skill folder at `folder` whose `SKILL.md` is `text`.
fn with_skill(fs: FakeFileSystem, folder: impl AsRef<Path>, text: &str) -> FakeFileSystem {
    fs.with_file(folder.as_ref().join("SKILL.md"), text.to_owned())
}

/// A Claude Code home with no MCP servers.
fn claude_home() -> FakeFileSystem {
    FakeFileSystem::new().with_file(in_home(".claude.json"), "{}")
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

/// The skill Items, by name.
fn skills(inventory: &Inventory) -> Vec<&Item> {
    inventory
        .items
        .iter()
        .filter(|item| code(item.kind) == "skill")
        .collect()
}

fn skill<'i>(inventory: &'i Inventory, name: &str) -> &'i Item {
    skills(inventory)
        .into_iter()
        .find(|item| item.name == name)
        .unwrap_or_else(|| panic!("no skill `{name}`"))
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
fn a_personal_skill_folder_is_an_effective_skill_item() {
    let fs = with_skill(
        FakeFileSystem::new(),
        in_home(r".claude\skills\deploy"),
        &skill_md("deploy"),
    );

    let inventory = no_project(&fs);

    let deploy = skill(&inventory, "deploy");
    assert_eq!(code(deploy.state), "effective");
    assert_eq!(deploy.agent, "claude-code");
    assert_eq!(deploy.home, "default");
    let entry = entry_json(&deploy.entries[0]);
    assert_eq!(entry["path"], r"C:\Users\fixture\.claude\skills\deploy");
    assert_eq!(entry["key_path"], json!([]));
    assert_eq!(entry["locator"], r"C:\Users\fixture\.claude\skills\deploy");
    assert_eq!(entry["scope"], "user");
    assert_eq!(entry["provenance"], "file");
    assert_eq!(entry["state"], "effective");
    assert_eq!(
        entry["config"],
        json!({ "name": "deploy", "description": "The deploy skill." })
    );
    assert_eq!(entry["notes"], json!([]));
}

#[test]
fn a_folder_without_skill_md_and_a_dot_folder_are_not_entries() {
    let fs = with_skill(
        FakeFileSystem::new(),
        in_home(r".claude\skills\deploy"),
        &skill_md("deploy"),
    )
    .with_file(in_home(r".claude\skills\notes\README.md"), "not a skill")
    .with_file(in_home(r".claude\skills\stray.md"), "a file")
    .with_file(
        in_home(r".claude\skills\.trash\1-2-3\old\SKILL.md"),
        skill_md("old"),
    );
    let fs = with_skill(fs, in_home(r".claude\skills\.hidden"), &skill_md("hidden"));

    let inventory = no_project(&fs);

    let names: Vec<&str> = skills(&inventory)
        .iter()
        .map(|item| item.name.as_str())
        .collect();
    assert_eq!(names, ["deploy"]);
}

#[test]
fn a_skill_folder_is_read_one_level_down_only() {
    let fs = with_skill(
        FakeFileSystem::new(),
        in_home(r".claude\skills\group\deploy"),
        &skill_md("deploy"),
    );

    let inventory = no_project(&fs);

    assert!(skills(&inventory).is_empty());
}

#[test]
fn enterprise_skills_are_read_from_program_files_as_managed_and_read_only() {
    let program_files = r"C:\Program Files";
    let fs = with_skill(
        claude_home(),
        Path::new(program_files).join(r"ClaudeCode\.claude\skills\audit"),
        &skill_md("audit"),
    );
    let env = FakeEnvironment::with_home(HOME).with_var("ProgramFiles", program_files);
    let core = Core::new(fake::ports(fs.clone(), env)).unwrap();

    let inventory = inventory(&core, &Context::default());

    let audit = skill(&inventory, "audit");
    let entry = entry_json(&audit.entries[0]);
    assert_eq!(
        entry["path"],
        r"C:\Program Files\ClaudeCode\.claude\skills\audit"
    );
    assert_eq!(entry["scope"], "managed");
    assert_eq!(entry["provenance"], "managed");
    assert_eq!(entry["state"], "effective");
    assert_eq!(entry["disable"]["code"], "read_only");
}

#[test]
fn synced_skills_sit_below_an_account_folder_as_a_read_only_cloud_mirror() {
    let synced = in_home(r".claude\skills\synced\org1_acct1");
    let fs = with_skill(FakeFileSystem::new(), synced.join("pdf"), &skill_md("pdf"))
        .with_file(synced.join("manifest.json"), "{}")
        .with_file(
            in_home(r".claude\skills\synced\.staging\xlsx\SKILL.md"),
            skill_md("xlsx"),
        );

    let inventory = no_project(&fs);

    let names: Vec<&str> = skills(&inventory)
        .iter()
        .map(|item| item.name.as_str())
        .collect();
    assert_eq!(names, ["pdf"]);
    let entry = entry_json(&skill(&inventory, "pdf").entries[0]);
    assert_eq!(
        entry["path"],
        r"C:\Users\fixture\.claude\skills\synced\org1_acct1\pdf"
    );
    assert_eq!(entry["scope"], "user");
    assert_eq!(entry["provenance"], "cloud_mirror");
    assert_eq!(entry["disable"]["code"], "read_only");
}

#[test]
fn project_skills_are_read_in_the_project_and_its_parents_up_to_the_repo_root() {
    let repo = PathBuf::from(r"E:\repo");
    let project = repo.join(r"packages\frontend");
    let fs = claude_home()
        .with_folder(repo.join(".git"))
        .with_folder(&project);
    let fs = with_skill(fs, project.join(r".claude\skills\lint"), &skill_md("lint"));
    let fs = with_skill(
        fs,
        repo.join(r".claude\skills\release"),
        &skill_md("release"),
    );
    let fs = with_skill(fs, r"E:\.claude\skills\above", &skill_md("above"));
    let context = Context {
        project: Some(project.clone()),
    };

    let inventory = inventory(&core(&fs), &context);

    let mut names: Vec<&str> = skills(&inventory)
        .iter()
        .map(|item| item.name.as_str())
        .collect();
    names.sort();
    assert_eq!(names, ["lint", "release"]);
    let release = entry_json(&skill(&inventory, "release").entries[0]);
    assert_eq!(release["path"], r"E:\repo\.claude\skills\release");
    assert_eq!(release["scope"], "project");
}

#[test]
fn outside_a_repo_only_the_project_folder_is_read() {
    let project = PathBuf::from(r"E:\scratch\app");
    let fs = claude_home().with_folder(&project);
    let fs = with_skill(fs, project.join(r".claude\skills\lint"), &skill_md("lint"));
    let fs = with_skill(fs, r"E:\scratch\.claude\skills\parent", &skill_md("parent"));
    let context = Context {
        project: Some(project),
    };

    let inventory = inventory(&core(&fs), &context);

    let names: Vec<&str> = skills(&inventory)
        .iter()
        .map(|item| item.name.as_str())
        .collect();
    assert_eq!(names, ["lint"]);
}

#[test]
fn project_skills_take_no_part_without_a_project() {
    let project = PathBuf::from(r"E:\scratch\app");
    let fs = claude_home().with_folder(&project);
    let fs = with_skill(fs, project.join(r".claude\skills\lint"), &skill_md("lint"));

    let inventory = no_project(&fs);

    assert!(skills(&inventory).is_empty());
}

/// `(scope, state)` per Entry of the skill `name`, in precedence order.
fn states(inventory: &Inventory, name: &str) -> Vec<(String, String)> {
    skill(inventory, name)
        .entries
        .iter()
        .map(|entry| (entry.scope.clone(), code(entry.state)))
        .collect()
}

fn pairs(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
        .collect()
}

const PROJECT: &str = r"E:\projects\app";

fn in_project() -> Context {
    Context {
        project: Some(PathBuf::from(PROJECT)),
    }
}

fn project_skill(name: &str) -> PathBuf {
    Path::new(PROJECT).join(r".claude\skills").join(name)
}

#[test]
fn a_name_clash_is_won_by_enterprise_then_personal_then_project_then_synced() {
    let program_files = r"C:\Program Files";
    let mut fs = claude_home().with_folder(PROJECT);
    for folder in [
        Path::new(program_files).join(r"ClaudeCode\.claude\skills\deploy"),
        in_home(r".claude\skills\deploy"),
        project_skill("deploy"),
        in_home(r".claude\skills\synced\org_acct\deploy"),
    ] {
        fs = with_skill(fs, folder, &skill_md("deploy"));
    }
    let env = FakeEnvironment::with_home(HOME).with_var("ProgramFiles", program_files);
    let core = Core::new(fake::ports(fs.clone(), env)).unwrap();

    let inventory = inventory(&core, &in_project());

    assert_eq!(
        states(&inventory, "deploy"),
        pairs(&[
            ("managed", "effective"),
            ("user", "shadowed"),
            ("project", "shadowed"),
            ("user", "shadowed"),
        ])
    );
    let synced = &skill(&inventory, "deploy").entries[3];
    assert_eq!(code(synced.provenance), "cloud_mirror");
    assert_eq!(
        synced.reason.as_ref().unwrap().effective.as_deref(),
        Some(r"C:\Program Files\ClaudeCode\.claude\skills\deploy")
    );
}

#[test]
fn a_personal_skill_shadows_a_project_skill_of_the_same_name() {
    let fs = with_skill(
        claude_home().with_folder(PROJECT),
        in_home(r".claude\skills\deploy"),
        &skill_md("deploy"),
    );
    let fs = with_skill(fs, project_skill("deploy"), &skill_md("deploy"));

    let inventory = inventory(&core(&fs), &in_project());

    assert_eq!(
        states(&inventory, "deploy"),
        pairs(&[("user", "effective"), ("project", "shadowed")])
    );
}

#[test]
fn skills_compete_by_the_name_claude_code_gives_them_not_by_folder() {
    let fs = with_skill(
        claude_home().with_folder(PROJECT),
        in_home(r".claude\skills\ship-it"),
        &skill_md("deploy"),
    );
    let fs = with_skill(fs, project_skill("deploy"), &skill_md("deploy"));

    let inventory = inventory(&core(&fs), &in_project());

    assert_eq!(
        states(&inventory, "deploy"),
        pairs(&[("user", "effective"), ("project", "shadowed")])
    );
}

#[test]
fn a_skill_reached_through_a_linked_folder_is_one_entry_at_its_real_path() {
    let dotfiles = PathBuf::from(r"D:\dotfiles\skills");
    let fs = with_skill(
        claude_home().with_folder(PROJECT),
        dotfiles.join("deploy"),
        &skill_md("deploy"),
    )
    .with_link(in_home(r".claude\skills"), &dotfiles)
    .with_link(Path::new(PROJECT).join(r".claude\skills"), &dotfiles);

    let inventory = inventory(&core(&fs), &in_project());

    let deploy = skill(&inventory, "deploy");
    assert_eq!(deploy.entries.len(), 1, "{:?}", deploy.entries);
    assert_eq!(deploy.entries[0].path, dotfiles.join("deploy"));
    assert_eq!(deploy.entries[0].scope, "user");
    assert_eq!(code(deploy.entries[0].state), "effective");
}

#[test]
fn a_skill_is_selected_by_its_name_or_by_its_folder() {
    let fs = with_skill(
        claude_home(),
        in_home(r".claude\skills\deploy"),
        &skill_md("deploy"),
    );
    let inventory = no_project(&fs);

    let by_name = inventory.select_one("deploy").unwrap();
    let by_folder = inventory
        .select_one(r"c:/users/fixture/.claude/skills/deploy")
        .unwrap();

    assert_eq!(by_name, by_folder);
    assert_eq!(code(by_name.kind), "skill");
}

#[test]
fn a_skill_in_a_folder_claude_code_reserves_is_invalid_and_does_not_compete() {
    let fs = with_skill(
        claude_home().with_folder(PROJECT),
        in_home(r".claude\skills\Anthropic-Skills"),
        &skill_md("tools"),
    );
    let fs = with_skill(fs, project_skill("tools"), &skill_md("tools"));

    let inventory = inventory(&core(&fs), &in_project());

    assert_eq!(
        states(&inventory, "tools"),
        pairs(&[("user", "invalid"), ("project", "effective")])
    );
    let reason = skill(&inventory, "tools").entries[0]
        .reason
        .clone()
        .unwrap();
    assert_eq!(code(reason.code), "malformed_skill");
    assert_eq!(
        reason.sentence,
        "Claude Code skips this skill: Claude Code keeps its folder's name for itself."
    );
    assert_eq!(code(skill(&inventory, "tools").state), "effective");
}

#[test]
fn a_skill_md_without_frontmatter_loads_under_its_folder_name_with_a_note() {
    let fs = with_skill(
        claude_home(),
        in_home(r".claude\skills\deploy"),
        "# Deploy\n\nname: not-this\n",
    );

    let inventory = no_project(&fs);

    let deploy = skill(&inventory, "deploy");
    let entry = entry_json(&deploy.entries[0]);
    assert_eq!(entry["state"], "effective");
    assert_eq!(entry["config"], json!({}));
    assert_eq!(
        entry["notes"],
        json!([{
            "code": "skill_fallback",
            "switch": null,
            "sentence": "Claude Code still loads this skill, as `deploy`, its folder's name: its SKILL.md does not open with a `---` frontmatter."
        }])
    );
}

#[test]
fn a_frontmatter_that_is_not_yaml_loads_with_no_fields_under_the_folder_name() {
    let fs = with_skill(
        claude_home(),
        in_home(r".claude\skills\deploy"),
        "---\nname: ship\nthis is not yaml\n---\nBody.\n",
    );

    let inventory = no_project(&fs);

    let deploy = skill(&inventory, "deploy");
    assert_eq!(code(deploy.entries[0].state), "effective");
    assert_eq!(deploy.entries[0].config, json!({}));
    assert_eq!(code(deploy.entries[0].notes[0].code), "skill_fallback");
    assert!(
        deploy.entries[0].notes[0]
            .sentence
            .ends_with("its SKILL.md frontmatter is not valid YAML."),
        "{}",
        deploy.entries[0].notes[0].sentence
    );
}

#[test]
fn a_plain_value_holding_a_colon_and_space_is_not_yaml() {
    let fs = with_skill(
        claude_home(),
        in_home(r".claude\skills\deploy"),
        "---\nname: ship\ndescription: Use when: shipping\n---\n",
    );

    let inventory = no_project(&fs);

    let deploy = skill(&inventory, "deploy");
    assert_eq!(deploy.entries[0].config, json!({}));
    assert!(
        deploy.entries[0].notes[0]
            .sentence
            .ends_with("its SKILL.md frontmatter is not valid YAML."),
        "{}",
        deploy.entries[0].notes[0].sentence
    );
}

#[test]
fn a_quoted_value_may_hold_a_colon_and_space() {
    let fs = with_skill(
        claude_home(),
        in_home(r".claude\skills\deploy"),
        "---\nname: ship\ndescription: \"Use when: shipping\"\n---\n",
    );

    let inventory = no_project(&fs);

    let ship = skill(&inventory, "ship");
    assert_eq!(
        ship.entries[0].config,
        json!({ "name": "ship", "description": "Use when: shipping" })
    );
    assert_eq!(ship.entries[0].notes, []);
}

#[test]
fn a_frontmatter_may_nest_a_mapping_or_continue_a_value_on_the_next_lines() {
    let fs = with_skill(
        claude_home(),
        in_home(r".claude\skills\deploy"),
        "---\nname: ship\ndescription:\n  Ships the app\n  to staging.\nmetadata:\n  author: me\nallowed-tools:\n  - Bash\n---\n",
    );

    let inventory = no_project(&fs);

    let ship = skill(&inventory, "ship");
    assert_eq!(
        ship.entries[0].config,
        json!({ "name": "ship", "description": "Ships the app to staging." })
    );
    assert_eq!(ship.entries[0].notes, []);
}

#[test]
fn a_frontmatter_without_a_name_loads_under_the_folder_name_with_a_note() {
    let fs = with_skill(
        claude_home(),
        in_home(r".claude\skills\deploy"),
        "---\ndescription: Ships it.\n---\n",
    );

    let inventory = no_project(&fs);

    let deploy = skill(&inventory, "deploy");
    assert_eq!(code(deploy.entries[0].state), "effective");
    assert_eq!(
        deploy.entries[0].config,
        json!({ "description": "Ships it." })
    );
    assert!(
        deploy.entries[0].notes[0]
            .sentence
            .ends_with("its SKILL.md frontmatter has no `name`."),
        "{}",
        deploy.entries[0].notes[0].sentence
    );
}

/// An Agent that names skills by folder, sanitised, reads `~/.test/skills`
/// at any depth, and skips a skill without frontmatter.
const FOLDER_NAMED: &str = r#"
format_version = 1

[agent]
id = "test-agent"
name = "Test Agent"

[[homes]]
id = "default"
folder = { block = "path", path = "~" }
roots.skills = ".test/skills"

[[scopes]]
id = "user"

[[locations]]
id = "user-skills"
anchor = { block = "home", root = "skills" }
format = { block = "skill-folder", recursive = true, name_from = "folder", sanitize = "lowercase-hyphens", no_frontmatter = "skip", malformed_frontmatter = "folder-name", no_name = "skip" }
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

fn test_agent_skills(fs: &FakeFileSystem) -> Vec<Item> {
    let core = Core::with_extra_definitions(
        fake::ports(fs.clone(), FakeEnvironment::with_home(HOME)),
        &[("test-agent.toml", FOLDER_NAMED)],
    )
    .unwrap();
    inventory(&core, &Context::default())
        .items
        .into_iter()
        .filter(|item| item.agent == "test-agent")
        .collect()
}

#[test]
fn a_skill_folder_block_can_name_by_folder_sanitised_at_any_depth() {
    let fs = with_skill(
        FakeFileSystem::new(),
        in_home(r".test\skills\team\Deploy To Prod!"),
        &skill_md("ignored"),
    );

    let items = test_agent_skills(&fs);

    let names: Vec<&str> = items.iter().map(|item| item.name.as_str()).collect();
    assert_eq!(names, ["deploy-to-prod"]);
    assert_eq!(
        items[0].entries[0].path,
        in_home(r".test\skills\team\Deploy To Prod!")
    );
}

#[test]
fn a_skill_folder_block_can_skip_a_skill_without_frontmatter() {
    let fs = with_skill(
        FakeFileSystem::new(),
        in_home(r".test\skills\deploy"),
        "no frontmatter",
    );

    let items = test_agent_skills(&fs);

    assert_eq!(code(items[0].state), "invalid");
    let reason = items[0].entries[0].reason.clone().unwrap();
    assert_eq!(code(reason.code), "malformed_skill");
    assert_eq!(
        reason.sentence,
        "Test Agent skips this skill: its SKILL.md does not open with a `---` frontmatter."
    );
}

#[test]
fn a_skill_folder_inside_a_skill_folder_is_not_read() {
    let fs = with_skill(
        FakeFileSystem::new(),
        in_home(r".test\skills\deploy"),
        &skill_md("deploy"),
    );
    let fs = with_skill(
        fs,
        in_home(r".test\skills\deploy\nested"),
        &skill_md("nested"),
    );

    let items = test_agent_skills(&fs);

    let names: Vec<&str> = items.iter().map(|item| item.name.as_str()).collect();
    assert_eq!(names, ["deploy"]);
}

fn settings_local() -> PathBuf {
    Path::new(PROJECT).join(r".claude\settings.local.json")
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

/// A personal skill `deploy`, in a Project folder whose local settings
/// hold `local_settings`.
fn deploy_in_project(local_settings: &str) -> FakeFileSystem {
    with_skill(
        claude_home().with_folder(PROJECT),
        in_home(r".claude\skills\deploy"),
        &skill_md("deploy"),
    )
    .with_file(settings_local(), local_settings.to_owned())
}

#[test]
fn disable_writes_skill_overrides_off_in_the_projects_local_settings() {
    let fs = deploy_in_project(r#"{ "permissions": { "allow": [] } }"#);
    let core = core(&fs);
    let before = inventory(&core, &in_project());
    match &skill(&before, "deploy").entries[0].disable {
        DisableOption::Offered { path, sentence } => {
            assert_eq!(*path, settings_local());
            assert!(sentence.contains("skillOverrides"), "{sentence}");
        }
        other => panic!("Disable not offered: {other:?}"),
    }

    let plan = plan(&core, &in_project(), Action::Disable, &["deploy"]).unwrap();
    core.execute(plan, &[]).unwrap();

    assert_eq!(
        json_of(&fs, settings_local()),
        json!({ "permissions": { "allow": [] }, "skillOverrides": { "deploy": "off" } })
    );
    let after = inventory(&core, &in_project());
    let entry = &skill(&after, "deploy").entries[0];
    assert_eq!(code(entry.state), "disabled");
    let reason = entry.reason.as_ref().unwrap();
    assert_eq!(code(reason.code), "disabled_by_name");
    assert_eq!(
        reason.switch.as_deref(),
        Some(r"E:\projects\app\.claude\settings.local.json#skillOverrides.deploy")
    );
}

#[test]
fn enable_deletes_the_override_that_disable_wrote() {
    let fs = deploy_in_project("{}");
    let core = core(&fs);
    let disable = plan(&core, &in_project(), Action::Disable, &["deploy"]).unwrap();
    core.execute(disable, &[]).unwrap();

    let enable = plan(&core, &in_project(), Action::Enable, &["deploy"]).unwrap();
    core.execute(enable, &[]).unwrap();

    assert_eq!(
        json_of(&fs, settings_local()),
        json!({ "skillOverrides": {} })
    );
    let after = inventory(&core, &in_project());
    assert_eq!(code(skill(&after, "deploy").entries[0].state), "effective");
}

#[test]
fn disable_creates_the_local_settings_file_when_it_is_missing() {
    let fs = with_skill(
        claude_home().with_folder(PROJECT),
        in_home(r".claude\skills\deploy"),
        &skill_md("deploy"),
    );
    let core = core(&fs);

    let plan = plan(&core, &in_project(), Action::Disable, &["deploy"]).unwrap();
    core.execute(plan, &[]).unwrap();

    assert_eq!(
        json_of(&fs, settings_local()),
        json!({ "skillOverrides": { "deploy": "off" } })
    );
}

#[test]
fn only_off_disables_a_skill_and_the_other_values_are_notes() {
    let fs = deploy_in_project(
        r#"{ "skillOverrides": { "deploy": "name-only", "lint": "user-invocable-only", "docs": "on" } }"#,
    );
    let fs = with_skill(fs, in_home(r".claude\skills\lint"), &skill_md("lint"));
    let fs = with_skill(fs, in_home(r".claude\skills\docs"), &skill_md("docs"));

    let inventory = inventory(&core(&fs), &in_project());

    for name in ["deploy", "lint"] {
        let entry = &skill(&inventory, name).entries[0];
        assert_eq!(code(entry.state), "effective", "{name}");
        assert_eq!(entry.notes.len(), 1, "{name}");
        assert_eq!(code(entry.notes[0].code), "partial_switch");
        assert_eq!(
            entry.notes[0].switch,
            Some(format!(
                r"E:\projects\app\.claude\settings.local.json#skillOverrides.{name}"
            ))
        );
    }
    let docs = &skill(&inventory, "docs").entries[0];
    assert_eq!(code(docs.state), "effective");
    assert_eq!(docs.notes, []);
}

#[test]
fn off_in_the_user_settings_disables_the_skill_everywhere_and_is_only_read() {
    let fs = deploy_in_project("{}").with_file(
        in_home(r".claude\settings.json"),
        r#"{ "skillOverrides": { "deploy": "off" } }"#,
    );
    let core = core(&fs);

    for context in [Context::default(), in_project()] {
        let inventory = inventory(&core, &context);
        let entry = &skill(&inventory, "deploy").entries[0];
        assert_eq!(code(entry.state), "disabled");
        assert!(
            matches!(&entry.enable, Some(EnableOption::NotOffered { sentence }) if sentence.starts_with("Enable is not offered")),
            "{:?}",
            entry.enable
        );
    }
    let refused = plan(&core, &in_project(), Action::Enable, &["deploy"])
        .err()
        .unwrap();
    assert_eq!(code(refused.code), "blocked");
}

#[test]
fn disable_of_a_skill_without_a_project_is_not_offered() {
    let fs = deploy_in_project("{}");

    let inventory = no_project(&fs);

    let entry = entry_json(&skill(&inventory, "deploy").entries[0]);
    assert_eq!(entry["disable"]["status"], "not_offered");
    assert_eq!(entry["disable"]["code"], "no_mechanism");
}

#[test]
fn disable_by_name_reaches_every_skill_of_the_item_and_warns_of_wide_reach() {
    let fs = deploy_in_project("{}");
    let fs = with_skill(fs, project_skill("deploy"), &skill_md("deploy"));
    let core = core(&fs);
    let user_skill = in_home(r".claude\skills\deploy").display().to_string();

    let plan = plan(&core, &in_project(), Action::Disable, &[&user_skill]).unwrap();

    assert!(
        plan.warnings()
            .iter()
            .any(|warning| warning.class == WarningClass::WideReach),
        "{:?}",
        plan.warnings()
    );
    core.execute(plan, &[WarningClass::WideReach]).unwrap();
    assert_eq!(
        states(&inventory(&core, &in_project()), "deploy"),
        pairs(&[("user", "disabled"), ("project", "disabled")])
    );
}

#[test]
fn a_read_only_skill_cannot_be_disabled_or_removed() {
    let synced = in_home(r".claude\skills\synced\org_acct\pdf");
    let fs = with_skill(
        claude_home().with_folder(PROJECT),
        &synced,
        &skill_md("pdf"),
    );
    let core = core(&fs);

    let disable = plan(&core, &in_project(), Action::Disable, &["pdf"])
        .err()
        .unwrap();
    let remove = plan(&core, &in_project(), Action::Remove, &["pdf"])
        .err()
        .unwrap();

    assert_eq!(code(disable.code), "blocked");
    assert!(
        disable.message.contains("only reads it"),
        "{}",
        disable.message
    );
    assert_eq!(code(remove.code), "blocked");
    assert!(
        remove.message.contains("only reads it"),
        "{}",
        remove.message
    );
}
