//! Claude Code's synced skills and plugins through the core `api`, with
//! fake ports: Claude Code reads only the folder of the claude.ai account it
//! is signed in to, `oauthAccount` in `~/.claude.json`, and none while a
//! settings file takes it off the account.

use config_digger_core::api::{Context, Core, Entry, Inventory, ScanOptions};
use config_digger_core::ports::fake::{self, FakeEnvironment, FakeFileSystem};
use serde_json::{Value, json};
use std::path::PathBuf;

const HOME: &str = r"C:\Users\fixture";

/// The folder of the account `oauthAccount` names in [`signed_in`].
const ACCOUNT: &str = "org-a_acct-a";

fn in_home(relative: &str) -> PathBuf {
    PathBuf::from(HOME).join(relative)
}

fn skill_md(name: &str) -> String {
    format!("---\nname: {name}\ndescription: The {name} skill.\n---\n\nDo {name}.\n")
}

/// `~/.claude.json` holding `oauthAccount`, or none for `Value::Null`.
fn with_oauth_account(fs: FakeFileSystem, account: Value) -> FakeFileSystem {
    let state = if account.is_null() {
        json!({})
    } else {
        json!({ "oauthAccount": account })
    };
    fs.with_file(in_home(".claude.json"), state.to_string())
}

/// Signed in to the account whose folder is [`ACCOUNT`].
fn signed_in(fs: FakeFileSystem) -> FakeFileSystem {
    with_oauth_account(
        fs,
        json!({
            "organizationUuid": "org-a",
            "accountUuid": "acct-a",
            "emailAddress": "someone@example.com",
        }),
    )
}

/// The synced skill `pdf` in the account folder `account`.
fn with_synced_skill(fs: FakeFileSystem, account: &str, name: &str) -> FakeFileSystem {
    fs.with_file(
        in_home(&format!(r".claude\skills\synced\{account}\{name}\SKILL.md")),
        skill_md(name),
    )
}

fn inventory_with(fs: &FakeFileSystem, env: FakeEnvironment, context: &Context) -> Inventory {
    let core = Core::new(fake::ports(fs.clone(), env)).unwrap();
    let snapshot = core
        .scan(&ScanOptions {
            projects: context.project.iter().cloned().collect(),
        })
        .unwrap();
    core.resolve(&snapshot, context)
}

fn inventory(fs: &FakeFileSystem) -> Inventory {
    inventory_with(fs, FakeEnvironment::with_home(HOME), &Context::default())
}

fn entry_json(entry: &Entry) -> Value {
    serde_json::to_value(entry).unwrap()
}

/// The one Entry of the Item `name`, as JSON.
fn entry(inventory: &Inventory, name: &str) -> Value {
    let item = inventory
        .items
        .iter()
        .find(|item| item.name == name)
        .unwrap_or_else(|| panic!("no Item `{name}`"));
    assert_eq!(item.entries.len(), 1, "{name}");
    entry_json(&item.entries[0])
}

#[test]
fn the_signed_in_accounts_synced_skill_is_effective_and_another_accounts_is_unread() {
    let fs = with_synced_skill(signed_in(FakeFileSystem::new()), ACCOUNT, "pdf");
    let fs = with_synced_skill(fs, "org-b_acct-b", "xlsx");

    let inventory = inventory(&fs);

    let ours = entry(&inventory, "anthropic-skills:pdf");
    assert_eq!(ours["state"], "effective", "{ours}");
    let theirs = entry(&inventory, "anthropic-skills:xlsx");
    assert_eq!(theirs["state"], "unread", "{theirs}");
    assert_eq!(theirs["reason"]["code"], "other_account", "{theirs}");
    assert_eq!(
        theirs["reason"]["sentence"],
        "Claude Code reads only the folder of the claude.ai account it is signed in to, org-a_acct-a, not this one."
    );
    assert_eq!(theirs["provenance"], "cloud_mirror");
    assert_eq!(theirs["remove"]["code"], "read_only", "{theirs}");
}

#[test]
fn signed_in_to_no_account_every_synced_skill_is_unread() {
    for account in [
        Value::Null,
        json!({ "organizationUuid": "org-a" }),
        json!({ "organizationUuid": "org-a", "accountUuid": "" }),
        json!({ "accountUuid": "acct-a", "organizationUuid": null }),
    ] {
        let fs = with_oauth_account(FakeFileSystem::new(), account.clone());
        let fs = with_synced_skill(fs, ACCOUNT, "pdf");

        let inventory = inventory(&fs);

        let entry = entry(&inventory, "anthropic-skills:pdf");
        assert_eq!(entry["state"], "unread", "{account}: {entry}");
        assert_eq!(entry["reason"]["code"], "not_signed_in", "{account}");
        assert_eq!(
            entry["reason"]["sentence"],
            "Claude Code is signed in to no claude.ai account, and reads this only for the account it is signed in to."
        );
    }
}

/// The synced plugin `<name>~g1` in the account folder `account`, whose
/// manifest names it `name`, with the skill `notes` and the server `docs`.
fn with_synced_plugin(fs: FakeFileSystem, account: &str, name: &str) -> FakeFileSystem {
    let folder = in_home(&format!(r".claude\plugins\synced\{account}\{name}~g1"));
    fs.with_file(
        folder.join(r".claude-plugin\plugin.json"),
        json!({ "name": name }).to_string(),
    )
    .with_file(folder.join(r"skills\notes\SKILL.md"), skill_md("notes"))
    .with_file(
        folder.join(".mcp.json"),
        json!({ "mcpServers": { "docs": { "url": format!("https://{name}.example.com/mcp") } } })
            .to_string(),
    )
}

/// The State and reason code of the one Entry of each Item `names` names.
fn states(inventory: &Inventory, names: &[&str]) -> Vec<(String, String)> {
    names
        .iter()
        .map(|name| {
            let entry = entry(inventory, name);
            (
                entry["state"].as_str().unwrap().to_owned(),
                entry["reason"]["code"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned(),
            )
        })
        .collect()
}

fn pairs(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(state, code)| ((*state).to_owned(), (*code).to_owned()))
        .collect()
}

#[test]
fn only_the_signed_in_accounts_synced_plugins_servers_and_skills_are_read() {
    let fs = with_synced_plugin(signed_in(FakeFileSystem::new()), ACCOUNT, "ours");
    let fs = with_synced_plugin(fs, "org-b_acct-b", "theirs");

    let inventory = inventory(&fs);

    assert_eq!(
        states(
            &inventory,
            &[
                "plugin:ours:docs",
                "ours:notes",
                "plugin:theirs:docs",
                "theirs:notes"
            ]
        ),
        pairs(&[
            ("effective", ""),
            ("effective", ""),
            ("unread", "other_account"),
            ("unread", "other_account"),
        ])
    );
    assert_eq!(
        entry(&inventory, "theirs:notes")["remove"]["code"],
        "read_only"
    );
}

#[test]
fn signed_in_to_no_account_no_synced_plugin_is_read() {
    let fs = with_synced_plugin(FakeFileSystem::new(), ACCOUNT, "ours");

    let inventory = inventory(&fs);

    assert_eq!(
        states(&inventory, &["plugin:ours:docs", "ours:notes"]),
        pairs(&[("unread", "not_signed_in"), ("unread", "not_signed_in")])
    );
}

const PROJECT: &str = r"E:\projects\app";

fn in_project() -> Context {
    Context {
        project: Some(PathBuf::from(PROJECT)),
    }
}

/// Signed in, with the account's synced skill `pdf` and synced plugin
/// `ours`, and the Project folder.
fn synced_home() -> FakeFileSystem {
    let fs = with_synced_skill(signed_in(FakeFileSystem::new()), ACCOUNT, "pdf");
    with_synced_plugin(fs, ACCOUNT, "ours").with_folder(PROJECT)
}

const SYNCED: [&str; 3] = ["anthropic-skills:pdf", "plugin:ours:docs", "ours:notes"];

/// The States of [`SYNCED`] in `context`.
fn synced_states(
    fs: &FakeFileSystem,
    env: FakeEnvironment,
    context: &Context,
) -> Vec<(String, String)> {
    states(&inventory_with(fs, env, context), &SYNCED)
}

fn home_env() -> FakeEnvironment {
    FakeEnvironment::with_home(HOME)
}

fn all(state: &str, code: &str) -> Vec<(String, String)> {
    pairs(&[(state, code); 3])
}

#[test]
fn an_api_key_helper_in_the_user_settings_takes_claude_code_off_the_account_everywhere() {
    let fs = synced_home().with_file(
        in_home(r".claude\settings.json"),
        json!({ "apiKeyHelper": "get-key.cmd" }).to_string(),
    );

    for context in [Context::default(), in_project()] {
        assert_eq!(
            synced_states(&fs, home_env(), &context),
            all("unread", "off_account"),
            "{context:?}"
        );
    }
    let inventory = inventory(&fs);
    assert_eq!(
        entry(&inventory, "anthropic-skills:pdf")["reason"]["sentence"],
        r"`apiKeyHelper` in C:\Users\fixture\.claude\settings.json takes Claude Code off its claude.ai account, and it reads this only while it runs on that account."
    );
}

#[test]
fn an_api_key_helper_in_a_projects_settings_takes_claude_code_off_the_account_there_alone() {
    let fs = synced_home().with_file(
        PathBuf::from(PROJECT).join(r".claude\settings.json"),
        json!({ "apiKeyHelper": "get-key.cmd" }).to_string(),
    );

    assert_eq!(
        synced_states(&fs, home_env(), &in_project()),
        all("unread", "off_account")
    );
    assert_eq!(
        synced_states(&fs, home_env(), &Context::default()),
        all("effective", "")
    );
}

/// The variables that take Claude Code off the account from a settings
/// file's `env`, each with a value that does.
const OFF_VARIABLES: [(&str, &str); 7] = [
    ("ANTHROPIC_AUTH_TOKEN", "not-a-real-secret"),
    ("CLAUDE_CODE_USE_BEDROCK", "1"),
    ("CLAUDE_CODE_USE_VERTEX", "true"),
    ("CLAUDE_CODE_USE_FOUNDRY", "yes"),
    ("CLAUDE_CODE_USE_MANTLE", "on"),
    ("CLAUDE_CODE_USE_ANTHROPIC_AWS", "1"),
    ("CLAUDE_CODE_USE_ANTHROPIC_GOOGLE_CLOUD", "TRUE"),
];

#[test]
fn an_auth_token_or_a_provider_in_a_settings_files_env_takes_claude_code_off_the_account() {
    for (name, value) in OFF_VARIABLES {
        let user = synced_home().with_file(
            in_home(r".claude\settings.json"),
            json!({ "env": { name: value } }).to_string(),
        );
        assert_eq!(
            synced_states(&user, home_env(), &Context::default()),
            all("unread", "off_account"),
            "{name} in the user settings"
        );
        let project = synced_home().with_file(
            PathBuf::from(PROJECT).join(r".claude\settings.json"),
            json!({ "env": { name: value } }).to_string(),
        );
        assert_eq!(
            synced_states(&project, home_env(), &in_project()),
            all("unread", "off_account"),
            "{name} in the Project's settings"
        );
        assert_eq!(
            synced_states(&project, home_env(), &Context::default()),
            all("effective", ""),
            "{name} in the Project's settings, outside it"
        );
    }
    let fs = synced_home().with_file(
        in_home(r".claude\settings.json"),
        json!({ "env": { "ANTHROPIC_AUTH_TOKEN": "not-a-real-secret" } }).to_string(),
    );
    assert_eq!(
        entry(&inventory(&fs), "anthropic-skills:pdf")["reason"]["sentence"],
        r"`ANTHROPIC_AUTH_TOKEN` in `env` of C:\Users\fixture\.claude\settings.json takes Claude Code off its claude.ai account, and it reads this only while it runs on that account."
    );
}

#[test]
fn an_empty_auth_token_or_a_provider_turned_off_leaves_claude_code_on_the_account() {
    let fs = synced_home().with_file(
        in_home(r".claude\settings.json"),
        json!({ "env": { "ANTHROPIC_AUTH_TOKEN": "", "CLAUDE_CODE_USE_BEDROCK": "0" } })
            .to_string(),
    );

    assert_eq!(
        synced_states(&fs, home_env(), &Context::default()),
        all("effective", "")
    );
}

/// The code and sentence of each caveat of [`SYNCED`]'s Entries in no
/// Project, which have one each.
fn account_caveats(fs: &FakeFileSystem, env: FakeEnvironment) -> Vec<(String, String)> {
    let inventory = inventory_with(fs, env, &Context::default());
    SYNCED
        .iter()
        .flat_map(|name| {
            let entry = entry(&inventory, name);
            let caveats = entry["caveats"].as_array().unwrap().clone();
            caveats
                .into_iter()
                .filter(|caveat| caveat["code"] == "may_run_off_account")
                .map(|caveat| {
                    (
                        caveat["code"].as_str().unwrap().to_owned(),
                        caveat["sentence"].as_str().unwrap().to_owned(),
                    )
                })
                .collect::<Vec<_>>()
        })
        .collect()
}

#[test]
fn a_variable_in_config_diggers_environment_may_take_claude_code_off_the_account() {
    for (name, value) in OFF_VARIABLES
        .into_iter()
        .chain([("ANTHROPIC_API_KEY", "not-a-real-secret")])
    {
        let env = home_env().with_var(name, value);

        assert_eq!(
            synced_states(&synced_home(), env.clone(), &Context::default()),
            all("effective", ""),
            "{name}"
        );
        let sentence = format!(
            "`{name}` is set in config-digger's environment: when Claude Code starts with it, it may run off its claude.ai account, and then does not read this."
        );
        assert_eq!(
            account_caveats(&synced_home(), env),
            vec![("may_run_off_account".to_owned(), sentence); 3],
            "{name}"
        );
    }
}

#[test]
fn an_api_key_in_a_settings_files_env_may_take_claude_code_off_the_account() {
    let fs = synced_home().with_file(
        in_home(r".claude\settings.json"),
        json!({ "env": { "ANTHROPIC_API_KEY": "not-a-real-secret" } }).to_string(),
    );

    assert_eq!(
        synced_states(&fs, home_env(), &Context::default()),
        all("effective", "")
    );
    let sentence = r"`ANTHROPIC_API_KEY` in `env` of C:\Users\fixture\.claude\settings.json may take Claude Code off its claude.ai account, and then it does not read this.";
    assert_eq!(
        account_caveats(&fs, home_env()),
        vec![("may_run_off_account".to_owned(), sentence.to_owned()); 3]
    );
}

#[test]
fn a_settings_files_env_replaces_a_variable_of_config_diggers_environment() {
    let fs = synced_home().with_file(
        in_home(r".claude\settings.json"),
        json!({ "env": { "CLAUDE_CODE_USE_BEDROCK": "0" } }).to_string(),
    );
    let env = home_env().with_var("CLAUDE_CODE_USE_BEDROCK", "1");

    assert_eq!(
        synced_states(&fs, env.clone(), &Context::default()),
        all("effective", "")
    );
    assert_eq!(account_caveats(&fs, env), []);
}

#[test]
fn a_primary_api_key_or_an_oauth_token_leaves_claude_code_on_the_account() {
    let fs = synced_home().with_file(
        in_home(".claude.json"),
        json!({
            "oauthAccount": { "organizationUuid": "org-a", "accountUuid": "acct-a" },
            "primaryApiKey": "not-a-real-secret",
        })
        .to_string(),
    );
    let env = home_env().with_var("CLAUDE_CODE_OAUTH_TOKEN", "not-a-real-secret");
    let user = fs.clone().with_file(
        in_home(r".claude\settings.json"),
        json!({ "env": { "CLAUDE_CODE_OAUTH_TOKEN": "not-a-real-secret" } }).to_string(),
    );

    for fs in [fs, user] {
        assert_eq!(
            synced_states(&fs, env.clone(), &Context::default()),
            all("effective", "")
        );
        assert_eq!(account_caveats(&fs, env.clone()), []);
    }
}

#[test]
fn an_unread_synced_entry_stays_read_only_whatever_keeps_it_unread() {
    let other = with_synced_skill(signed_in(FakeFileSystem::new()), "org-b_acct-b", "pdf");
    let signed_out = with_synced_skill(FakeFileSystem::new(), ACCOUNT, "pdf");
    let off = synced_home().with_file(
        in_home(r".claude\settings.json"),
        json!({ "apiKeyHelper": "get-key.cmd" }).to_string(),
    );

    for (fs, code) in [
        (other, "other_account"),
        (signed_out, "not_signed_in"),
        (off, "off_account"),
    ] {
        let entry = entry(&inventory(&fs), "anthropic-skills:pdf");
        assert_eq!(entry["reason"]["code"], code, "{entry}");
        assert_eq!(entry["remove"]["code"], "read_only", "{entry}");
        assert_eq!(entry["disable"]["code"], "unread", "{entry}");
    }
}

#[test]
fn an_api_key_helper_in_the_managed_or_the_local_settings_takes_claude_code_off_the_account() {
    let managed = synced_home().with_file(
        in_home(r".claude\remote-settings.json"),
        json!({ "apiKeyHelper": "get-key.cmd" }).to_string(),
    );
    let local = synced_home().with_file(
        PathBuf::from(PROJECT).join(r".claude\settings.local.json"),
        json!({ "apiKeyHelper": "get-key.cmd" }).to_string(),
    );

    assert_eq!(
        synced_states(&managed, home_env(), &Context::default()),
        all("unread", "off_account")
    );
    assert_eq!(
        synced_states(&local, home_env(), &in_project()),
        all("unread", "off_account")
    );
}

#[test]
fn a_higher_settings_files_variable_replaces_a_lower_ones() {
    let fs = synced_home()
        .with_file(
            PathBuf::from(PROJECT).join(r".claude\settings.local.json"),
            json!({ "env": { "CLAUDE_CODE_USE_BEDROCK": "0" } }).to_string(),
        )
        .with_file(
            in_home(r".claude\settings.json"),
            json!({ "env": { "CLAUDE_CODE_USE_BEDROCK": "1" } }).to_string(),
        );

    assert_eq!(
        synced_states(&fs, home_env(), &in_project()),
        all("effective", "")
    );
    assert_eq!(
        synced_states(&fs, home_env(), &Context::default()),
        all("unread", "off_account")
    );
}

#[test]
fn a_state_file_config_digger_cannot_read_says_so() {
    let broken = with_synced_skill(FakeFileSystem::new(), ACCOUNT, "pdf")
        .with_file(in_home(".claude.json"), "{ not json");
    let denied = with_synced_skill(signed_in(FakeFileSystem::new()), ACCOUNT, "pdf")
        .denying_reads_of(in_home(".claude.json"));

    for fs in [broken, denied] {
        let entry = entry(&inventory(&fs), "anthropic-skills:pdf");
        assert_eq!(entry["state"], "unread", "{entry}");
        assert_eq!(entry["reason"]["code"], "not_signed_in", "{entry}");
        assert_eq!(
            entry["reason"]["sentence"],
            r"config-digger cannot read C:\Users\fixture\.claude.json, which names the claude.ai account Claude Code is signed in to, so it takes Claude Code to be signed in to none, and then it does not read this."
        );
    }
}
