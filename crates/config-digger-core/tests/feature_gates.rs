//! Feature gates through the core `api`, with fake ports: Claude Code's
//! claude.ai skill sync gate read from the value it last cached in
//! `~/.claude.json`, assumed when it cannot be read, and bypassed by the
//! variables that make Claude Code ignore its cache.

use config_digger_core::api::{Context, Core, Entry, Inventory, Item, ScanOptions};
use config_digger_core::ports::fake::{self, FakeEnvironment, FakeFileSystem};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const HOME: &str = r"C:\Users\fixture";

/// 2 Oct 2026 07:40 UTC, in milliseconds since the epoch.
const CACHED_AT: u64 = 1_790_926_800_000;

const SYNC_GATE: &str = "tengu_claudeai_skills_sync_enabled";

fn in_home(relative: &str) -> PathBuf {
    PathBuf::from(HOME).join(relative)
}

fn skill_md(name: &str) -> String {
    format!("---\nname: {name}\ndescription: The {name} skill.\n---\n\nDo {name}.\n")
}

fn with_skill(fs: FakeFileSystem, folder: impl AsRef<Path>, text: &str) -> FakeFileSystem {
    fs.with_file(folder.as_ref().join("SKILL.md"), text.to_owned())
}

/// A Claude Code home whose `~/.claude.json` is `state`, with a skill
/// `pdf` synced from claude.ai and a personal skill `deploy`.
fn claude_home(state: &Value) -> FakeFileSystem {
    let fs = FakeFileSystem::new().with_file(in_home(".claude.json"), state.to_string());
    let fs = with_skill(
        fs,
        in_home(r".claude\skills\synced\org1_acct1\pdf"),
        &skill_md("pdf"),
    );
    with_skill(fs, in_home(r".claude\skills\deploy"), &skill_md("deploy"))
}

/// A `~/.claude.json` that cached the sync gate as `value`.
fn cached(value: Value) -> Value {
    json!({
        "cachedGrowthBookFeatures": { SYNC_GATE: value, "tengu_other": false },
        "cachedGrowthBookFeaturesAt": CACHED_AT,
    })
}

fn inventory_with(fs: &FakeFileSystem, env: FakeEnvironment) -> Inventory {
    let core = Core::new(fake::ports(fs.clone(), env)).unwrap();
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    core.resolve(&snapshot, &Context::default())
}

fn inventory(fs: &FakeFileSystem) -> Inventory {
    inventory_with(fs, FakeEnvironment::with_home(HOME))
}

fn skill<'i>(inventory: &'i Inventory, name: &str) -> &'i Item {
    inventory
        .items
        .iter()
        .find(|item| item.name == name)
        .unwrap_or_else(|| panic!("no skill `{name}`"))
}

fn entry_json(entry: &Entry) -> Value {
    serde_json::to_value(entry).unwrap()
}

#[test]
fn a_cached_gate_value_is_a_caveat_on_the_entries_it_affects() {
    let fs = claude_home(&cached(json!(true)));

    let inventory = inventory(&fs);

    let pdf = skill(&inventory, "pdf");
    let entry = entry_json(&pdf.entries[0]);
    assert_eq!(entry["state"], "effective");
    assert_eq!(
        entry["caveats"],
        json!([{
            "code": "gate_cached",
            "path": r"C:\Users\fixture\.claude.json",
            "sentence": "Depends on Claude Code gate `tengu_claudeai_skills_sync_enabled`: on as of 2 Oct 2026 07:40 UTC.",
        }])
    );
    // Only the Entries the gate affects carry it, and the Item list does not.
    assert_eq!(pdf.caveats, []);
    assert_eq!(
        entry_json(&skill(&inventory, "deploy").entries[0])["caveats"],
        json!([])
    );
}

#[test]
fn a_location_whose_gate_is_cached_off_is_unread() {
    let fs = claude_home(&cached(json!(false)));

    let inventory = inventory(&fs);

    let pdf = skill(&inventory, "pdf");
    let entry = entry_json(&pdf.entries[0]);
    assert_eq!(entry["state"], "unread");
    assert_eq!(entry["reason"]["code"], "feature_gate");
    assert_eq!(
        entry["reason"]["sentence"],
        "Claude Code does not read it while its gate `tengu_claudeai_skills_sync_enabled` is off."
    );
    assert_eq!(entry["caveats"][0]["code"], "gate_cached");
    assert_eq!(
        entry["caveats"][0]["sentence"],
        "Depends on Claude Code gate `tengu_claudeai_skills_sync_enabled`: off as of 2 Oct 2026 07:40 UTC."
    );
    assert_eq!(serde_json::to_value(pdf.state).unwrap(), "unread");
}

#[test]
fn a_gate_cached_off_leaves_a_local_skill_of_the_same_name_effective() {
    let fs = with_skill(
        claude_home(&cached(json!(false))),
        in_home(r".claude\skills\pdf"),
        &skill_md("pdf"),
    );

    let inventory = inventory(&fs);

    let pdf = skill(&inventory, "pdf");
    let states: Vec<Value> = pdf
        .entries
        .iter()
        .map(|entry| entry_json(entry)["state"].clone())
        .collect();
    assert_eq!(states, [json!("effective"), json!("unread")]);
}

/// The sentence of the one caveat of the synced skill `pdf`, checking its code.
fn pdf_caveat(inventory: &Inventory, code: &str) -> String {
    let entry = entry_json(&skill(inventory, "pdf").entries[0]);
    assert_eq!(entry["state"], "effective", "{entry}");
    let caveats = entry["caveats"].as_array().unwrap();
    assert_eq!(caveats.len(), 1, "{entry}");
    assert_eq!(caveats[0]["code"], code, "{entry}");
    caveats[0]["sentence"].as_str().unwrap().to_owned()
}

const ASSUMED_UNREADABLE: &str =
    "Depends on Claude Code gate `tengu_claudeai_skills_sync_enabled`: assumed on; cannot be read.";

#[test]
fn a_gate_without_a_cached_value_is_assumed_on() {
    for state in [
        json!({}),
        json!({ "cachedGrowthBookFeatures": {}, "cachedGrowthBookFeaturesAt": CACHED_AT }),
        json!({ "cachedGrowthBookFeatures": { SYNC_GATE: "yes" } }),
    ] {
        let inventory = inventory(&claude_home(&state));

        assert_eq!(
            pdf_caveat(&inventory, "gate_assumed"),
            ASSUMED_UNREADABLE,
            "{state}"
        );
    }
}

#[test]
fn a_gate_whose_cache_file_is_missing_or_unreadable_is_assumed_on() {
    let missing = claude_home(&json!({}));
    missing.delete_file(in_home(".claude.json"));
    let broken = claude_home(&json!({})).with_file(in_home(".claude.json"), "{ not json");
    let denied = claude_home(&cached(json!(false))).denying_reads_of(in_home(".claude.json"));

    for fs in [missing, broken, denied] {
        let inventory = inventory(&fs);

        assert_eq!(pdf_caveat(&inventory, "gate_assumed"), ASSUMED_UNREADABLE);
    }
}

#[test]
fn a_cached_value_without_a_time_says_so() {
    let state = json!({ "cachedGrowthBookFeatures": { SYNC_GATE: true } });

    let inventory = inventory(&claude_home(&state));

    assert_eq!(
        pdf_caveat(&inventory, "gate_cached"),
        "Depends on Claude Code gate `tengu_claudeai_skills_sync_enabled`: on as Claude Code last cached it, at a time it does not record."
    );
}

#[test]
fn a_cached_value_has_no_age_limit() {
    let fs = claude_home(&cached(json!(false)));
    let core = Core::new(config_digger_core::ports::Ports {
        clock: Box::new(fake::FakeClock::at(
            std::time::UNIX_EPOCH + std::time::Duration::from_secs(4_000_000_000),
        )),
        ..fake::ports(fs, FakeEnvironment::with_home(HOME))
    })
    .unwrap();
    let snapshot = core.scan(&ScanOptions::default()).unwrap();

    let inventory = core.resolve(&snapshot, &Context::default());

    let entry = entry_json(&skill(&inventory, "pdf").entries[0]);
    assert_eq!(entry["state"], "unread");
    assert_eq!(entry["caveats"][0]["code"], "gate_cached");
}

/// The inventory of a home that cached the sync gate off, read with the
/// environment variables `vars`.
fn cached_off_with(vars: &[(&str, &str)]) -> Inventory {
    let env = vars
        .iter()
        .fold(FakeEnvironment::with_home(HOME), |env, (name, value)| {
            env.with_var(name, value)
        });
    inventory_with(&claude_home(&cached(json!(false))), env)
}

fn bypassed_by(variable: &str) -> String {
    format!(
        "Depends on Claude Code gate `tengu_claudeai_skills_sync_enabled`: assumed on; Claude Code ignores its cached value while `{variable}` is set."
    )
}

#[test]
fn the_variables_that_make_claude_code_ignore_its_cache_bypass_the_cached_value() {
    for (name, value) in [
        ("DISABLE_GROWTHBOOK", "1"),
        ("DISABLE_GROWTHBOOK", "TRUE"),
        ("DISABLE_TELEMETRY", "1"),
        ("DISABLE_TELEMETRY", "0"),
        ("CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC", "false"),
        ("DO_NOT_TRACK", "1"),
        ("CLAUDE_CODE_USE_BEDROCK", "1"),
        ("CLAUDE_CODE_USE_VERTEX", "true"),
        ("CLAUDE_CODE_USE_FOUNDRY", "yes"),
        ("CLAUDE_CODE_USE_ANTHROPIC_AWS", "on"),
        ("CLAUDE_CODE_USE_MANTLE", "1"),
    ] {
        let inventory = cached_off_with(&[(name, value)]);

        assert_eq!(
            pdf_caveat(&inventory, "gate_assumed"),
            bypassed_by(name),
            "{name}={value}"
        );
    }
}

#[test]
fn values_that_leave_fetching_on_keep_the_cached_value() {
    for vars in [
        vec![("DISABLE_GROWTHBOOK", "0")],
        vec![("DISABLE_GROWTHBOOK", "false")],
        vec![("DO_NOT_TRACK", "0")],
        vec![("DISABLE_TELEMETRY", "")],
        vec![("CLAUDE_CODE_USE_BEDROCK", "0")],
        // A host that embeds Claude Code keeps fetching on a provider.
        vec![
            ("CLAUDE_CODE_USE_BEDROCK", "1"),
            ("CLAUDE_CODE_PROVIDER_MANAGED_BY_HOST", "1"),
        ],
    ] {
        let inventory = cached_off_with(&vars);

        let entry = entry_json(&skill(&inventory, "pdf").entries[0]);
        assert_eq!(entry["state"], "unread", "{vars:?}");
        assert_eq!(entry["caveats"][0]["code"], "gate_cached", "{vars:?}");
    }
}

#[test]
fn a_variable_in_the_user_settings_env_bypasses_the_cache_too() {
    let fs = claude_home(&cached(json!(false))).with_file(
        in_home(r".claude\settings.json"),
        json!({ "env": { "DISABLE_GROWTHBOOK": "1" } }).to_string(),
    );

    let inventory = inventory(&fs);

    assert_eq!(
        pdf_caveat(&inventory, "gate_assumed"),
        bypassed_by("DISABLE_GROWTHBOOK")
    );
}

#[test]
fn an_empty_value_in_the_user_settings_env_replaces_the_one_in_the_environment() {
    let fs = claude_home(&cached(json!(false))).with_file(
        in_home(r".claude\settings.json"),
        json!({ "env": { "CLAUDE_CODE_USE_VERTEX": "" } }).to_string(),
    );
    let env = FakeEnvironment::with_home(HOME).with_var("CLAUDE_CODE_USE_VERTEX", "1");

    let inventory = inventory_with(&fs, env);

    let entry = entry_json(&skill(&inventory, "pdf").entries[0]);
    assert_eq!(entry["state"], "unread");
    assert_eq!(entry["caveats"][0]["code"], "gate_cached");
}

/// An Agent that reads `.test/mcp.json` only while its gate
/// `dedupe_mcp_servers` is off, cached as Cursor caches Statsig gates: by
/// djb2 hash, in the JSON under a key of a SQLite store.
const SQLITE_GATED: &str = r#"
format_version = 1

[agent]
id = "test-agent"
name = "Test Agent"

[[homes]]
id = "default"
folder = { block = "path", path = "~" }
roots.state = ".test/state.vscdb"
roots.config = ".test/mcp.json"

[[scopes]]
id = "user"

[[locations]]
id = "user-mcp"
anchor = { block = "home", root = "config" }
format = { block = "json", dialect = "strict" }
root = ["servers"]
kind = "mcp"
scope = "user"
provenance = "file"
store_class = "plain"
on_read_failure = "ignores_file"
when = [{ block = "feature-gate", gate = "dedupe_mcp_servers", is = false }]

[items.mcp]
precedence = ["user-mcp"]
competition = { block = "exact-name" }
checks = ["disable", "precedence"]

[[feature_gates]]
name = "dedupe_mcp_servers"
assumed = true
affects = ["user-mcp"]
lookup = { block = "sqlite-json", anchor = { block = "home", root = "state" }, key = "workbench.experiments.statsigBootstrap", key_path = ["feature_gates", "{gate}", "value"], hash = "djb2", cached_at = ["time"] }
"#;

const BOOTSTRAP: &str = "workbench.experiments.statsigBootstrap";

/// The test Agent's server `docs`, with a Statsig bootstrap holding `bootstrap`.
fn test_agent_entry(bootstrap: Option<Value>) -> Value {
    let mut fs = FakeFileSystem::new().with_file(
        in_home(r".test\mcp.json"),
        json!({ "servers": { "docs": { "command": "docs" } } }).to_string(),
    );
    if let Some(bootstrap) = bootstrap {
        fs = fs.with_sqlite_item(
            in_home(r".test\state.vscdb"),
            BOOTSTRAP,
            &bootstrap.to_string(),
        );
    }
    let core = Core::with_extra_definitions(
        fake::ports(fs, FakeEnvironment::with_home(HOME)),
        &[("test-agent.toml", SQLITE_GATED)],
    )
    .unwrap();
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    let inventory = core.resolve(&snapshot, &Context::default());
    let item = inventory
        .items
        .iter()
        .find(|item| item.agent == "test-agent")
        .expect("the test Agent's server");
    entry_json(&item.entries[0])
}

/// A Statsig bootstrap that cached `dedupe_mcp_servers` as `value`. Its
/// djb2 hash is 1316230643.
fn bootstrap(value: bool) -> Value {
    json!({
        "feature_gates": {
            "1316230643": { "name": "1316230643", "value": value },
            "713966870": { "name": "713966870", "value": !value },
        },
        "time": "2026-10-01T15:11:00Z",
    })
}

#[test]
fn a_gate_is_read_by_its_djb2_hash_from_json_in_a_sqlite_store() {
    let entry = test_agent_entry(Some(bootstrap(false)));

    assert_eq!(entry["state"], "effective");
    assert_eq!(
        entry["caveats"],
        json!([{
            "code": "gate_cached",
            "path": r"C:\Users\fixture\.test\state.vscdb",
            "sentence": "Depends on Test Agent gate `dedupe_mcp_servers`: off as of 1 Oct 2026 15:11 UTC.",
        }])
    );
}

#[test]
fn a_condition_can_hold_while_a_gate_is_off() {
    let entry = test_agent_entry(Some(bootstrap(true)));

    assert_eq!(entry["state"], "unread");
    assert_eq!(
        entry["reason"]["sentence"],
        "Test Agent does not read it while its gate `dedupe_mcp_servers` is on."
    );
    assert_eq!(entry["disable"]["code"], "unread");
}

#[test]
fn a_gate_whose_sqlite_store_is_missing_resolves_with_its_assumed_default() {
    let entry = test_agent_entry(None);

    assert_eq!(entry["state"], "unread");
    assert_eq!(entry["caveats"][0]["code"], "gate_assumed");
    assert_eq!(
        entry["caveats"][0]["sentence"],
        "Depends on Test Agent gate `dedupe_mcp_servers`: assumed on; cannot be read."
    );
}

#[test]
fn a_sqlite_store_without_the_key_resolves_with_the_assumed_default() {
    let fs = FakeFileSystem::new()
        .with_file(
            in_home(r".test\mcp.json"),
            json!({ "servers": { "docs": { "command": "docs" } } }).to_string(),
        )
        .with_sqlite_item(in_home(r".test\state.vscdb"), "other.key", "{}");
    let core = Core::with_extra_definitions(
        fake::ports(fs, FakeEnvironment::with_home(HOME)),
        &[("test-agent.toml", SQLITE_GATED)],
    )
    .unwrap();
    let snapshot = core.scan(&ScanOptions::default()).unwrap();

    let inventory = core.resolve(&snapshot, &Context::default());

    let item = inventory
        .items
        .iter()
        .find(|item| item.agent == "test-agent")
        .unwrap();
    assert_eq!(
        entry_json(&item.entries[0])["caveats"][0]["code"],
        "gate_assumed"
    );
}

#[test]
fn a_variable_in_the_managed_settings_env_bypasses_the_cache() {
    let fs = claude_home(&cached(json!(false))).with_file(
        r"C:\Program Files\ClaudeCode\managed-settings.json",
        json!({ "env": { "DISABLE_TELEMETRY": "1" } }).to_string(),
    );
    let env = FakeEnvironment::with_home(HOME).with_var("ProgramFiles", r"C:\Program Files");

    let inventory = inventory_with(&fs, env);

    assert_eq!(
        pdf_caveat(&inventory, "gate_assumed"),
        bypassed_by("DISABLE_TELEMETRY")
    );
}

#[test]
fn a_value_in_a_settings_env_that_is_not_a_string_sets_nothing() {
    let fs = claude_home(&cached(json!(false))).with_file(
        in_home(r".claude\settings.json"),
        json!({ "env": { "DISABLE_TELEMETRY": null, "DISABLE_GROWTHBOOK": 1 } }).to_string(),
    );

    let inventory = inventory(&fs);

    let entry = entry_json(&skill(&inventory, "pdf").entries[0]);
    assert_eq!(entry["caveats"][0]["code"], "gate_cached", "{entry}");
}

#[test]
fn a_cache_time_may_be_a_fractional_number_of_milliseconds() {
    let state = json!({
        "cachedGrowthBookFeatures": { SYNC_GATE: true },
        "cachedGrowthBookFeaturesAt": 1.7909268e12,
    });

    let inventory = inventory(&claude_home(&state));

    assert_eq!(
        pdf_caveat(&inventory, "gate_cached"),
        "Depends on Claude Code gate `tengu_claudeai_skills_sync_enabled`: on as of 2 Oct 2026 07:40 UTC."
    );
}

#[test]
fn a_gate_kept_nowhere_on_disk_is_assumed_on_every_entry_it_affects() {
    let definition = SQLITE_GATED
        .replace(
            "lookup = { block = \"sqlite-json\", anchor = { block = \"home\", root = \"state\" }, key = \"workbench.experiments.statsigBootstrap\", key_path = [\"feature_gates\", \"{gate}\", \"value\"], hash = \"djb2\", cached_at = [\"time\"] }\n",
            "",
        )
        .replace("assumed = true", "assumed = false");
    let mcp_json = in_home(r".test\mcp.json");
    let fs = FakeFileSystem::new().with_file(
        &mcp_json,
        json!({ "servers": { "docs": { "command": "docs" } } }).to_string(),
    );
    let core = Core::with_extra_definitions(
        fake::ports(fs, FakeEnvironment::with_home(HOME)),
        &[("test-agent.toml", &definition)],
    )
    .unwrap();
    let snapshot = core.scan(&ScanOptions::default()).unwrap();

    let inventory = core.resolve(&snapshot, &Context::default());

    let item = inventory
        .items
        .iter()
        .find(|item| item.agent == "test-agent")
        .unwrap();
    let entry = entry_json(&item.entries[0]);
    assert_eq!(entry["state"], "effective");
    assert_eq!(entry["caveats"][0]["code"], "gate_assumed");
    assert_eq!(entry["caveats"][0]["path"], mcp_json.display().to_string());
    assert_eq!(
        entry["caveats"][0]["sentence"],
        "Depends on Test Agent gate `dedupe_mcp_servers`: assumed off; cannot be read."
    );
}

const PROGRAM_FILES: &str = r"C:\Program Files";
const POLICIES: &str = r"SOFTWARE\Policies\ClaudeCode";

fn managed_env() -> FakeEnvironment {
    FakeEnvironment::with_home(HOME).with_var("ProgramFiles", PROGRAM_FILES)
}

#[test]
fn a_variable_in_a_managed_drop_in_env_bypasses_the_cache() {
    let fs = claude_home(&cached(json!(false)))
        .with_file(
            format!(r"{PROGRAM_FILES}\ClaudeCode\managed-settings.json"),
            json!({ "model": "opus" }).to_string(),
        )
        .with_file(
            format!(r"{PROGRAM_FILES}\ClaudeCode\managed-settings.d\10-telemetry.json"),
            json!({ "env": { "DISABLE_TELEMETRY": "1" } }).to_string(),
        );

    let inventory = inventory_with(&fs, managed_env());

    assert_eq!(
        pdf_caveat(&inventory, "gate_assumed"),
        bypassed_by("DISABLE_TELEMETRY")
    );
}

#[test]
fn a_variable_in_every_admin_source_env_bypasses_the_cache_even_one_claude_code_skips() {
    // The registry policy holds settings, so Claude Code skips the
    // managed settings file, but still reads its `env`.
    let fs = claude_home(&cached(json!(false))).with_file(
        format!(r"{PROGRAM_FILES}\ClaudeCode\managed-settings.json"),
        json!({ "env": { "DISABLE_TELEMETRY": "1" } }).to_string(),
    );
    let env = managed_env().with_registry_value(
        config_digger_core::ports::Hive::LocalMachine,
        POLICIES,
        "Settings",
        &json!({ "model": "opus" }).to_string(),
    );

    let inventory = inventory_with(&fs, env);

    assert_eq!(
        pdf_caveat(&inventory, "gate_assumed"),
        bypassed_by("DISABLE_TELEMETRY")
    );
}

#[test]
fn the_user_registry_policy_env_is_not_read_beneath_an_admin_source() {
    let fs = claude_home(&cached(json!(false))).with_file(
        format!(r"{PROGRAM_FILES}\ClaudeCode\managed-settings.json"),
        json!({ "model": "opus" }).to_string(),
    );
    let env = managed_env().with_registry_value(
        config_digger_core::ports::Hive::CurrentUser,
        POLICIES,
        "Settings",
        &json!({ "env": { "DISABLE_TELEMETRY": "1" } }).to_string(),
    );

    let inventory = inventory_with(&fs, env);

    let entry = entry_json(&skill(&inventory, "pdf").entries[0]);
    assert_eq!(entry["caveats"][0]["code"], "gate_cached", "{entry}");
}

const PROJECT: &str = r"E:\projects\petme-app";

fn inventory_in(fs: &FakeFileSystem, project: Option<&str>) -> Inventory {
    let core = Core::new(fake::ports(fs.clone(), FakeEnvironment::with_home(HOME))).unwrap();
    let snapshot = core
        .scan(&ScanOptions {
            projects: vec![PathBuf::from(PROJECT)],
        })
        .unwrap();
    core.resolve(
        &snapshot,
        &Context {
            project: project.map(PathBuf::from),
        },
    )
}

#[test]
fn a_variable_in_a_projects_settings_env_bypasses_the_cache_in_that_project_only() {
    for file in [r".claude\settings.json", r".claude\settings.local.json"] {
        let fs = claude_home(&cached(json!(false))).with_file(
            PathBuf::from(PROJECT).join(file),
            json!({ "env": { "DISABLE_GROWTHBOOK": "1" } }).to_string(),
        );

        let in_project = inventory_in(&fs, Some(PROJECT));
        let entry = entry_json(&skill(&in_project, "pdf").entries[0]);
        assert_eq!(entry["state"], "effective", "{file}");
        assert_eq!(
            pdf_caveat(&in_project, "gate_assumed"),
            bypassed_by("DISABLE_GROWTHBOOK"),
            "{file}"
        );

        let outside = inventory_in(&fs, None);
        let entry = entry_json(&skill(&outside, "pdf").entries[0]);
        assert_eq!(entry["state"], "unread", "{file}");
        assert_eq!(entry["caveats"][0]["code"], "gate_cached", "{file}");
    }
}

#[test]
fn a_projects_settings_env_yields_to_the_managed_env() {
    let fs = claude_home(&cached(json!(false)))
        .with_file(
            PathBuf::from(PROJECT).join(r".claude\settings.json"),
            json!({ "env": { "DISABLE_GROWTHBOOK": "1" } }).to_string(),
        )
        .with_file(
            format!(r"{PROGRAM_FILES}\ClaudeCode\managed-settings.json"),
            json!({ "env": { "DISABLE_GROWTHBOOK": "0" } }).to_string(),
        );
    let core = Core::new(fake::ports(fs, managed_env())).unwrap();
    let snapshot = core
        .scan(&ScanOptions {
            projects: vec![PathBuf::from(PROJECT)],
        })
        .unwrap();

    let inventory = core.resolve(
        &snapshot,
        &Context {
            project: Some(PathBuf::from(PROJECT)),
        },
    );

    let entry = entry_json(&skill(&inventory, "pdf").entries[0]);
    assert_eq!(entry["caveats"][0]["code"], "gate_cached", "{entry}");
}

#[test]
fn a_variable_in_the_remote_settings_env_bypasses_the_cache() {
    let fs = claude_home(&cached(json!(false))).with_file(
        in_home(r".claude\remote-settings.json"),
        json!({ "env": { "DISABLE_TELEMETRY": "1" } }).to_string(),
    );

    let inventory = inventory(&fs);

    assert_eq!(
        pdf_caveat(&inventory, "gate_assumed"),
        bypassed_by("DISABLE_TELEMETRY")
    );
}
