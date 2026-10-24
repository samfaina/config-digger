//! The rest of Codex's inventory through the core `api`, with fake ports
//! over fixture Codex homes: the skills of every root it reads, the plugins
//! it installed with their servers and skills, the enterprise cloud layer it
//! caches, and the allowlists of `requirements.toml`.

use config_digger_core::api::{Context, Core, Entry, Inventory, Item, ScanOptions};
use config_digger_core::ports::fake::{self, FakeEnvironment, FakeFileSystem};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

const HOME: &str = r"C:\Users\fixture";
const APP_DATA: &str = r"C:\Users\fixture\AppData\Roaming";
const PROGRAM_DATA: &str = r"C:\ProgramData";

/// The default Codex home, `~\.codex`.
const CODEX: &str = r"C:\Users\fixture\.codex";

/// Where the fixture Projects go.
const PROJECTS: &str = r"E:\projects";

fn user_config() -> PathBuf {
    Path::new(CODEX).join("config.toml")
}

fn project(name: &str) -> PathBuf {
    Path::new(PROJECTS).join(name)
}

fn env() -> FakeEnvironment {
    FakeEnvironment::with_home(HOME)
        .with_var("APPDATA", APP_DATA)
        .with_var("USERPROFILE", HOME)
        .with_var("ProgramData", PROGRAM_DATA)
}

fn core_with(fs: &FakeFileSystem) -> Core {
    Core::new(fake::ports(fs.clone(), env())).unwrap()
}

fn inventory(fs: &FakeFileSystem) -> Inventory {
    let core = core_with(fs);
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    core.resolve(&snapshot, &Context::default())
}

/// The inventory in the context of the Project at `path`, which the scan is
/// asked for whether or not a registry records it.
fn inventory_at(fs: &FakeFileSystem, path: &Path) -> Inventory {
    let core = core_with(fs);
    let options = ScanOptions {
        projects: vec![path.to_path_buf()],
    };
    let snapshot = core.scan(&options).unwrap();
    let context = Context {
        project: Some(path.to_path_buf()),
    };
    core.resolve(&snapshot, &context)
}

fn codex_items(inventory: &Inventory) -> impl Iterator<Item = &Item> {
    inventory
        .items
        .iter()
        .filter(|item| item.agent == "codex" && item.home == "default")
}

/// The Codex Item named `name` in the default home.
fn item<'i>(inventory: &'i Inventory, name: &str) -> &'i Item {
    codex_items(inventory)
        .find(|item| item.name == name)
        .unwrap_or_else(|| {
            let names: Vec<&str> = codex_items(inventory)
                .map(|item| item.name.as_str())
                .collect();
            panic!("no Codex Item `{name}` among {names:?}")
        })
}

fn json_of(value: impl serde::Serialize) -> Value {
    serde_json::to_value(value).unwrap()
}

fn entry_json(entry: &Entry) -> Value {
    json_of(entry)
}

fn code(value: impl serde::Serialize) -> String {
    json_of(value).as_str().unwrap().to_owned()
}

/// The States of an Item's Entries, highest first.
fn states(item: &Item) -> Vec<String> {
    item.entries.iter().map(|entry| code(entry.state)).collect()
}

/// A `SKILL.md` naming its skill `name`.
fn skill_md(name: &str) -> String {
    format!("---\nname: {name}\ndescription: Does {name}.\n---\nBody.\n")
}

/// The skill `name` in the folder of skills `skills`.
fn skill(skills: &Path, name: &str) -> PathBuf {
    skills.join(name).join("SKILL.md")
}

/// A git repository at `E:\projects\mono`, trusted in the user's
/// `config.toml` when `trusted`, holding the Project `apps\web`.
fn monorepo(trusted: bool) -> FakeFileSystem {
    let trust = if trusted {
        "[projects.'e:\\projects\\mono']\ntrust_level = \"trusted\"\n"
    } else {
        ""
    };
    FakeFileSystem::new()
        .with_folder(project(r"mono\.git"))
        .with_file(user_config(), trust)
}

#[test]
fn skills_come_from_every_root_codex_reads_each_its_own_item() {
    let web = project(r"mono\apps\web");
    // `npx skills` installs into `.agents\skills`, so its definition claims
    // what is there.
    let roots = [
        (project(r"mono\.codex\skills"), "project", "file"),
        (web.join(r".codex\skills"), "project", "file"),
        (project(r"mono\.agents\skills"), "project", "skill_manager"),
        (web.join(r".agents\skills"), "project", "skill_manager"),
        (Path::new(CODEX).join("skills"), "user", "file"),
        (
            Path::new(HOME).join(r".agents\skills"),
            "user",
            "skill_manager",
        ),
        (
            Path::new(CODEX).join(r"skills\.system"),
            "system",
            "built_in",
        ),
        (
            Path::new(PROGRAM_DATA).join(r"OpenAI\Codex\skills"),
            "managed",
            "managed",
        ),
    ];
    let mut fs = monorepo(true);
    for (index, (root, _, _)) in roots.iter().enumerate() {
        fs = fs.with_file(skill(root, &format!("s{index}")), skill_md("review"));
    }

    let inventory = inventory_at(&fs, &web);

    let reviews: Vec<&Item> = codex_items(&inventory)
        .filter(|item| item.name == "review")
        .collect();
    assert_eq!(reviews.len(), roots.len());
    for (index, (root, scope, provenance)) in roots.iter().enumerate() {
        let path = root.join(format!("s{index}"));
        let review = reviews
            .iter()
            .find(|item| item.entries[0].path == path)
            .unwrap_or_else(|| panic!("no skill at {}", path.display()));
        assert_eq!(states(review), ["effective"], "{}", path.display());
        let entry = entry_json(&review.entries[0]);
        assert_eq!(entry["scope"], *scope, "{}", path.display());
        assert_eq!(entry["provenance"], *provenance, "{}", path.display());
    }
}

#[test]
fn an_untrusted_project_s_skills_still_load() {
    let web = project(r"mono\apps\web");
    let fs = monorepo(false)
        .with_file(skill(&web.join(r".codex\skills"), "lint"), skill_md("lint"))
        .with_file(
            skill(&web.join(r".agents\skills"), "test"),
            skill_md("test"),
        );

    let inventory = inventory_at(&fs, &web);

    assert_eq!(code(item(&inventory, "lint").state), "effective");
    assert_eq!(code(item(&inventory, "test").state), "effective");
}

/// The folder of the version `version` of the plugin `name` Codex installed
/// from the marketplace `marketplace`.
/// The user's `config.toml` tables that turn on the plugins `ids`, as Codex
/// writes them when it installs a plugin: one missing from it is off.
fn on(ids: &[&str]) -> String {
    ids.iter()
        .map(|id| format!("\n[plugins.\"{id}\"]\nenabled = true\n"))
        .collect()
}

fn plugin(marketplace: &str, name: &str, version: &str) -> PathBuf {
    Path::new(CODEX)
        .join(r"plugins\cache")
        .join(marketplace)
        .join(name)
        .join(version)
}

/// A plugin's `.mcp.json` declaring the server `name` at `url`.
fn mcp_json(name: &str, url: &str) -> String {
    json!({ "mcpServers": { name: { "url": url } } }).to_string()
}

#[test]
fn an_installed_plugin_s_highest_version_brings_its_servers_and_skills() {
    let old = plugin("openai-curated", "linear", "1.2.0");
    let new = plugin("openai-curated", "linear", "1.10.0");
    let fs = FakeFileSystem::new()
        .with_file(user_config(), on(&["linear@openai-curated"]))
        .with_file(
            old.join(".mcp.json"),
            mcp_json("linear", "https://old.example/mcp"),
        )
        .with_file(skill(&old.join("skills"), "triage"), skill_md("triage"))
        .with_file(
            new.join(".mcp.json"),
            mcp_json("linear", "https://linear.example/mcp"),
        )
        .with_file(skill(&new.join("skills"), "triage"), skill_md("triage"));

    let inventory = inventory(&fs);

    let linear = item(&inventory, "linear");
    assert_eq!(states(linear), ["effective"]);
    let entry = entry_json(&linear.entries[0]);
    assert_eq!(entry["path"], new.join(".mcp.json").display().to_string());
    assert_eq!(entry["key_path"], json!(["mcpServers", "linear"]));
    assert_eq!(entry["scope"], "plugin");
    assert_eq!(entry["provenance"], "plugin");
    assert_eq!(entry["plugin"]["id"], "linear@openai-curated");
    assert_eq!(entry["plugin"]["version"], "1.10.0");
    assert_eq!(entry["remove"]["code"], "plugin");
    assert_eq!(
        entry["remove"]["how_to_remove"]["command"],
        "codex plugin remove linear@openai-curated"
    );

    let triage = item(&inventory, "linear:triage");
    assert_eq!(states(triage), ["effective"]);
    let entry = entry_json(&triage.entries[0]);
    assert_eq!(
        entry["path"],
        new.join(r"skills\triage").display().to_string()
    );
    assert_eq!(entry["provenance"], "plugin");
}

#[test]
fn a_plugin_missing_from_config_toml_is_off_with_its_servers_and_skills() {
    let linear = plugin("openai-curated", "linear", "1.0.0");
    let fs = FakeFileSystem::new()
        .with_file(user_config(), "")
        .with_file(
            linear.join(".mcp.json"),
            mcp_json("linear", "https://linear.example/mcp"),
        )
        .with_file(skill(&linear.join("skills"), "triage"), skill_md("triage"));

    let inventory = inventory(&fs);

    for name in ["linear", "linear:triage"] {
        let item = item(&inventory, name);
        assert_eq!(states(item), ["disabled"], "{name}");
        assert_eq!(
            code(item.entries[0].reason.as_ref().unwrap().code),
            "plugin_disabled",
            "{name}"
        );
    }
}

#[test]
fn an_mcp_servers_entry_shadows_a_same_named_plugin_server_whole() {
    let linear = plugin("openai-curated", "linear", "1.0.0");
    let fs = FakeFileSystem::new()
        .with_folder(Path::new(PROGRAM_DATA).join(r"OpenAI\Codex"))
        .with_file(
            Path::new(PROGRAM_DATA).join(r"OpenAI\Codex\config.toml"),
            "[mcp_servers.linear]\ncommand = \"linear-mcp\"\n",
        )
        .with_file(
            user_config(),
            format!(
                "[mcp_servers.linear]\nargs = [\"--user\"]\n{}",
                on(&["linear@openai-curated"])
            ),
        )
        .with_file(
            linear.join(".mcp.json"),
            mcp_json("linear", "https://linear.example/mcp"),
        );

    let inventory = inventory(&fs);

    let item = item(&inventory, "linear");
    assert_eq!(states(item), ["merged", "merged", "shadowed"]);
    let reason = item.entries[2].reason.as_ref().unwrap();
    assert_eq!(code(reason.code), "shadowed_by_precedence");
    // The plugin's server takes no part in the server Codex builds.
    assert_eq!(
        item.built.as_ref().unwrap().config,
        json!({ "command": "linear-mcp", "args": ["--user"] })
    );
}

#[test]
fn of_two_plugins_with_a_same_named_server_the_first_by_id_wins() {
    let fs = FakeFileSystem::new()
        .with_file(user_config(), on(&["docs@zeta", "docs@alpha"]))
        .with_file(
            plugin("zeta", "docs", "1.0.0").join(".mcp.json"),
            mcp_json("docs", "https://zeta.example/mcp"),
        )
        .with_file(
            plugin("alpha", "docs", "1.0.0").join(".mcp.json"),
            mcp_json("docs", "https://alpha.example/mcp"),
        );

    let inventory = inventory(&fs);

    let docs = item(&inventory, "docs");
    assert_eq!(states(docs), ["effective", "shadowed"]);
    assert_eq!(entry_json(&docs.entries[0])["plugin"]["id"], "docs@alpha");
}

/// The manifest of the plugin whose folder is `plugin`.
fn manifest_of(plugin: &Path) -> PathBuf {
    plugin.join(r".codex-plugin\plugin.json")
}

#[test]
fn a_manifest_value_codex_cannot_read_leaves_the_defaults_in_place() {
    let linear = plugin("openai-curated", "linear", "1.0.0");
    let fs = FakeFileSystem::new()
        .with_file(user_config(), on(&["linear@openai-curated"]))
        .with_file(
            manifest_of(&linear),
            json!({ "name": "linear", "mcpServers": ["./config/mcp.json"], "skills": ["./more", 3] })
                .to_string(),
        )
        .with_file(
            linear.join(r"config\mcp.json"),
            mcp_json("ignored", "https://ignored.example/mcp"),
        )
        .with_file(skill(&linear.join("more"), "plan"), skill_md("plan"))
        .with_file(
            linear.join(".mcp.json"),
            mcp_json("linear", "https://linear.example/mcp"),
        )
        .with_file(skill(&linear.join("skills"), "triage"), skill_md("triage"));

    let inventory = inventory(&fs);

    assert_eq!(states(item(&inventory, "linear")), ["effective"]);
    assert_eq!(states(item(&inventory, "linear:triage")), ["effective"]);
    assert!(
        codex_items(&inventory).all(|item| item.name != "ignored" && item.name != "linear:plan")
    );
    // Codex ignores such a value, so it is no Read failure either.
    assert_eq!(json_of(&inventory.read_failures), json!([]));
}

#[test]
fn a_plugin_s_mcp_file_may_hold_its_servers_at_its_top_level() {
    let alpha = plugin("alpha", "docs", "1.0.0");
    let zeta = plugin("zeta", "wiki", "1.0.0");
    let fs = FakeFileSystem::new()
        .with_file(user_config(), on(&["docs@alpha", "wiki@zeta"]))
        .with_file(
            manifest_of(&alpha),
            json!({ "name": "docs", "mcpServers": "./servers.json" }).to_string(),
        )
        .with_file(
            alpha.join("servers.json"),
            json!({ "docs": { "url": "https://docs.example/mcp" } }).to_string(),
        )
        .with_file(
            zeta.join(".mcp.json"),
            json!({ "wiki": { "url": "https://wiki.example/mcp" } }).to_string(),
        );

    let inventory = inventory(&fs);

    let docs = entry_json(&item(&inventory, "docs").entries[0]);
    assert_eq!(docs["key_path"], json!(["docs"]));
    assert_eq!(docs["state"], "effective");
    let wiki = entry_json(&item(&inventory, "wiki").entries[0]);
    assert_eq!(wiki["path"], zeta.join(".mcp.json").display().to_string());
    assert_eq!(wiki["state"], "effective");
}

#[test]
fn a_skills_path_that_names_one_skill_folder_reads_that_skill() {
    let linear = plugin("openai-curated", "linear", "1.0.0");
    let fs = FakeFileSystem::new()
        .with_file(user_config(), on(&["linear@openai-curated"]))
        .with_file(
            manifest_of(&linear),
            json!({ "name": "linear", "skills": "./triage" }).to_string(),
        )
        .with_file(linear.join(r"triage\SKILL.md"), skill_md("triage"));

    let inventory = inventory(&fs);

    let triage = item(&inventory, "linear:triage");
    assert_eq!(states(triage), ["effective"]);
    assert_eq!(
        entry_json(&triage.entries[0])["path"],
        linear.join("triage").display().to_string()
    );
}

#[test]
fn the_servers_a_manifest_declares_inline_replace_those_of_mcp_json() {
    let linear = plugin("openai-curated", "linear", "1.0.0");
    let fs = FakeFileSystem::new()
        .with_file(user_config(), on(&["linear@openai-curated"]))
        .with_file(
            manifest_of(&linear),
            json!({ "name": "linear", "mcpServers": { "linear": { "url": "https://linear.example/mcp" } } })
                .to_string(),
        )
        .with_file(
            linear.join(".mcp.json"),
            mcp_json("stale", "https://stale.example/mcp"),
        );

    let inventory = inventory(&fs);

    let item = item(&inventory, "linear");
    assert_eq!(states(item), ["effective"]);
    let entry = entry_json(&item.entries[0]);
    assert_eq!(entry["path"], manifest_of(&linear).display().to_string());
    assert_eq!(entry["key_path"], json!(["mcpServers", "linear"]));
    assert_eq!(entry["scope"], "plugin");
    assert_eq!(entry["provenance"], "plugin");
    assert_eq!(entry["plugin"]["id"], "linear@openai-curated");
    assert!(codex_items(&inventory).all(|item| item.name != "stale"));
}

#[test]
fn the_skills_folders_a_manifest_names_replace_the_skills_folder() {
    let linear = plugin("openai-curated", "linear", "1.0.0");
    let fs = FakeFileSystem::new()
        .with_file(user_config(), on(&["linear@openai-curated"]))
        .with_file(
            manifest_of(&linear),
            json!({ "name": "linear", "skills": ["./agent/skills", "./more"] }).to_string(),
        )
        .with_file(
            skill(&linear.join(r"agent\skills"), "triage"),
            skill_md("triage"),
        )
        .with_file(skill(&linear.join("more"), "plan"), skill_md("plan"))
        .with_file(skill(&linear.join("skills"), "stale"), skill_md("stale"));

    let inventory = inventory(&fs);

    for (name, folder) in [
        ("linear:triage", r"agent\skills\triage"),
        ("linear:plan", r"more\plan"),
    ] {
        let item = item(&inventory, name);
        assert_eq!(states(item), ["effective"], "{name}");
        let entry = entry_json(&item.entries[0]);
        assert_eq!(entry["path"], linear.join(folder).display().to_string());
        assert_eq!(entry["scope"], "plugin");
        assert_eq!(entry["provenance"], "plugin");
        assert_eq!(entry["plugin"]["id"], "linear@openai-curated");
    }
    assert!(codex_items(&inventory).all(|item| item.name != "linear:stale"));
}

#[test]
fn a_plugin_s_switch_turns_off_what_its_manifest_declares() {
    let linear = plugin("openai-curated", "linear", "1.0.0");
    let fs = FakeFileSystem::new()
        .with_file(
            user_config(),
            "[plugins.\"linear@openai-curated\"]\nenabled = false\n",
        )
        .with_file(
            manifest_of(&linear),
            json!({
                "name": "linear",
                "mcpServers": { "linear": { "url": "https://linear.example/mcp" } },
                "skills": "./agent-skills",
            })
            .to_string(),
        )
        .with_file(
            skill(&linear.join("agent-skills"), "triage"),
            skill_md("triage"),
        );

    let inventory = inventory(&fs);

    for name in ["linear", "linear:triage"] {
        let item = item(&inventory, name);
        assert_eq!(states(item), ["disabled"], "{name}");
        let reason = item.entries[0].reason.as_ref().unwrap();
        assert_eq!(code(reason.code), "plugin_disabled", "{name}");
        let switch = reason.switch.as_deref().unwrap();
        assert!(
            switch.starts_with(&user_config().display().to_string())
                && switch.contains("linear@openai-curated"),
            "{switch}"
        );
    }
}

#[test]
fn a_manifest_path_that_does_not_start_with_dot_slash_is_ignored() {
    let linear = plugin("openai-curated", "linear", "1.0.0");
    let fs = FakeFileSystem::new()
        .with_file(user_config(), on(&["linear@openai-curated"]))
        .with_file(
            manifest_of(&linear),
            json!({ "name": "linear", "mcpServers": "config/mcp.json", "skills": ["more", "./a/../more"] })
                .to_string(),
        )
        .with_file(
            linear.join(r"config\mcp.json"),
            mcp_json("ignored", "https://ignored.example/mcp"),
        )
        .with_file(skill(&linear.join("more"), "plan"), skill_md("plan"))
        .with_file(
            linear.join(".mcp.json"),
            mcp_json("linear", "https://linear.example/mcp"),
        )
        .with_file(skill(&linear.join("skills"), "triage"), skill_md("triage"));

    let inventory = inventory(&fs);

    assert_eq!(states(item(&inventory, "linear")), ["effective"]);
    assert_eq!(states(item(&inventory, "linear:triage")), ["effective"]);
    assert!(
        codex_items(&inventory).all(|item| item.name != "ignored" && item.name != "linear:plan")
    );
}

#[test]
fn servers_a_manifest_declares_by_path_compete_like_those_of_mcp_json() {
    let alpha = plugin("alpha", "docs", "1.0.0");
    let zeta = plugin("zeta", "docs", "1.0.0");
    let fs = FakeFileSystem::new()
        .with_file(user_config(), on(&["docs@zeta", "docs@alpha"]))
        .with_file(
            manifest_of(&alpha),
            json!({ "name": "docs", "mcpServers": "./config/mcp.json" }).to_string(),
        )
        .with_file(
            alpha.join(r"config\mcp.json"),
            mcp_json("docs", "https://alpha.example/mcp"),
        )
        .with_file(
            alpha.join(".mcp.json"),
            mcp_json("stale", "https://stale.example/mcp"),
        )
        .with_file(
            zeta.join(".mcp.json"),
            mcp_json("docs", "https://zeta.example/mcp"),
        );

    let inventory = inventory(&fs);

    let docs = item(&inventory, "docs");
    assert_eq!(states(docs), ["effective", "shadowed"]);
    let entry = entry_json(&docs.entries[0]);
    assert_eq!(entry["plugin"]["id"], "docs@alpha");
    assert_eq!(
        entry["path"],
        alpha.join(r"config\mcp.json").display().to_string()
    );
    assert!(codex_items(&inventory).all(|item| item.name != "stale"));
}

#[test]
fn a_remote_plugin_is_account_managed_and_read_only() {
    let pets = plugin("openai-curated-remote", "work-pets", "0.1.6");
    let fs = FakeFileSystem::new()
        .with_file(user_config(), "")
        .with_file(
            pets.parent()
                .unwrap()
                .join(".codex-remote-plugin-install.json"),
            r#"{"schema_version":1,"remote_plugin_id":"plugins~work-pets"}"#,
        )
        .with_file(
            pets.join(".mcp.json"),
            mcp_json("pets", "https://pets.example/mcp"),
        )
        .with_file(
            skill(&pets.join("skills"), "create-pet"),
            skill_md("create-pet"),
        );

    let inventory = inventory(&fs);

    for name in ["pets", "work-pets:create-pet"] {
        let item = item(&inventory, name);
        assert_eq!(states(item), ["effective"], "{name}");
        let entry = entry_json(&item.entries[0]);
        assert_eq!(entry["provenance"], "account_managed", "{name}");
        assert_eq!(entry["plugin"]["id"], "work-pets@openai-curated-remote");
        assert_eq!(entry["disable"]["code"], "read_only", "{name}");
        assert_eq!(entry["remove"]["code"], "read_only", "{name}");
    }
}

#[test]
fn what_a_remote_plugin_s_manifest_declares_is_account_managed_and_read_only() {
    let pets = plugin("openai-curated-remote", "work-pets", "0.1.6");
    let fs = FakeFileSystem::new()
        .with_file(user_config(), "")
        .with_file(
            pets.parent()
                .unwrap()
                .join(".codex-remote-plugin-install.json"),
            r#"{"schema_version":1,"remote_plugin_id":"plugins~work-pets"}"#,
        )
        .with_file(
            manifest_of(&pets),
            json!({
                "name": "work-pets",
                "mcpServers": { "pets": { "url": "https://pets.example/mcp" } },
                "skills": "./pet-skills",
            })
            .to_string(),
        )
        .with_file(
            skill(&pets.join("pet-skills"), "create-pet"),
            skill_md("create-pet"),
        );

    let inventory = inventory(&fs);

    for name in ["pets", "work-pets:create-pet"] {
        let item = item(&inventory, name);
        assert_eq!(states(item), ["effective"], "{name}");
        let entry = entry_json(&item.entries[0]);
        assert_eq!(entry["provenance"], "account_managed", "{name}");
        assert_eq!(entry["disable"]["code"], "read_only", "{name}");
        assert_eq!(entry["remove"]["code"], "read_only", "{name}");
    }
}

fn system_config() -> PathBuf {
    Path::new(PROGRAM_DATA).join(r"OpenAI\Codex\config.toml")
}

fn cloud_cache() -> PathBuf {
    Path::new(CODEX).join("cloud-config-bundle-cache.json")
}

/// The cloud bundle Codex caches, with its `config.toml` and
/// `requirements.toml` fragments, highest first, cached at 08:00 and
/// expired at 09:00 on 1 Oct 2026.
fn bundle(config: &[&str], requirements: &[&str]) -> String {
    let fragments = |texts: &[&str]| -> Vec<Value> {
        texts
            .iter()
            .enumerate()
            .map(|(index, text)| json!({ "id": format!("f{index}"), "name": format!("Fragment {index}"), "contents": text }))
            .collect()
    };
    json!({
        "signed_payload": {
            "version": 1,
            "cached_at": "2026-10-01T08:00:00Z",
            "expires_at": "2026-10-01T09:00:00Z",
            "chatgpt_user_id": "user-fixture",
            "account_id": "account-fixture",
            "bundle": {
                "config_toml": { "enterprise_managed": fragments(config) },
                "requirements_toml": { "enterprise_managed": fragments(requirements) },
            },
        },
        "signature": "fixture",
    })
    .to_string()
}

#[test]
fn the_cached_cloud_bundle_is_a_read_only_managed_layer_as_of_when_codex_cached_it() {
    let fs = FakeFileSystem::new()
        .with_file(
            system_config(),
            "[mcp_servers.docs]\ncommand = \"docs-mcp\"\nargs = [\"--system\"]\n",
        )
        .with_file(
            cloud_cache(),
            bundle(
                &[
                    "[mcp_servers.docs]\nargs = [\"--cloud-high\"]\n",
                    "[mcp_servers.docs]\nargs = [\"--cloud-low\"]\nstartup_timeout_sec = 30\n",
                ],
                &[],
            ),
        )
        .with_file(user_config(), "[mcp_servers.docs]\ncwd = \"C:/work\"\n");

    let inventory = inventory(&fs);

    let docs = item(&inventory, "docs");
    assert_eq!(states(docs), ["merged", "merged", "merged"]);
    let cloud = entry_json(&docs.entries[1]);
    assert_eq!(cloud["path"], cloud_cache().display().to_string());
    assert_eq!(cloud["scope"], "managed");
    assert_eq!(cloud["provenance"], "managed");
    assert_eq!(cloud["disable"]["code"], "read_only");
    assert_eq!(cloud["remove"]["code"], "read_only");
    // An expired bundle is still read, as of when Codex cached it.
    let caveat = &docs.entries[1].caveats[0];
    assert_eq!(code(caveat.code), "cached_copy");
    assert!(
        caveat.sentence.contains("as of 1 Oct 2026 08:00 UTC"),
        "{}",
        caveat.sentence
    );
    // The bundle's fragments fold the highest last, between the machine's
    // layer and the user's.
    assert_eq!(
        docs.built.as_ref().unwrap().config,
        json!({
            "command": "docs-mcp",
            "args": ["--cloud-high"],
            "startup_timeout_sec": 30,
            "cwd": "C:/work",
        })
    );
    assert!(docs.entries[0].caveats.is_empty());
    assert!(docs.entries[2].caveats.is_empty());
}

#[test]
fn without_a_cached_cloud_bundle_there_is_no_caveat() {
    let fs = FakeFileSystem::new().with_file(
        user_config(),
        "[mcp_servers.docs]\ncommand = \"docs-mcp\"\n",
    );

    let inventory = inventory(&fs);

    let docs = item(&inventory, "docs");
    assert_eq!(states(docs), ["effective"]);
    assert!(docs.entries[0].caveats.is_empty());
}

fn requirements() -> PathBuf {
    Path::new(PROGRAM_DATA).join(r"OpenAI\Codex\requirements.toml")
}

#[test]
fn requirements_toml_allowlists_block_every_other_server_with_policy() {
    let fs = FakeFileSystem::new()
        .with_file(
            requirements(),
            "[mcp_servers.docs.identity]\ncommand = \"docs-mcp\"\n\n[mcp_servers.wiki.identity]\nurl = \"https://wiki.example/mcp\"\n",
        )
        .with_file(
            user_config(),
            "[mcp_servers.docs]\ncommand = \"docs-mcp\"\nargs = [\"--any\"]\n\n[mcp_servers.wiki]\nurl = \"https://other.example/mcp\"\n\n[mcp_servers.fetch]\ncommand = \"uvx\"\n",
        );

    let inventory = inventory(&fs);

    assert_eq!(states(item(&inventory, "docs")), ["effective"]);
    for name in ["wiki", "fetch"] {
        let item = item(&inventory, name);
        assert_eq!(states(item), ["blocked"], "{name}");
        let reason = item.entries[0].reason.as_ref().unwrap();
        assert_eq!(code(reason.code), "policy", "{name}");
        assert!(
            reason
                .sentence
                .contains(&requirements().display().to_string()),
            "{}",
            reason.sentence
        );
    }
}

#[test]
fn requirements_that_match_args_or_urls_block_the_servers_that_fail_them() {
    let args = r#"{ executable = "docs-mcp", args = [{ match = "exact", value = "--stdio" }, { match = "regex", expression = "n[0-9]+" }] }"#;
    let requirements_toml = [
        ("docs", format!("command = {args}")),
        ("docs-partial", format!("command = {args}")),
        ("docs-short", format!("command = {args}")),
        ("docs-prefix", format!("command = {args}")),
        (
            "wiki",
            r#"url = { match = "prefix", value = "https://wiki.example/" }"#.to_owned(),
        ),
        (
            "wiki-other",
            r#"url = { match = "prefix", value = "https://wiki.example/" }"#.to_owned(),
        ),
        (
            "wiki-key",
            r#"url = { match = "prefix", value = "https://wiki.example/" }"#.to_owned(),
        ),
        (
            "wiki-other-key",
            r#"url = { match = "prefix", value = "https://wiki.example/" }"#.to_owned(),
        ),
        (
            "search",
            r#"url = { match = "regex", expression = 'https://search\.example/v[0-9]+' }"#
                .to_owned(),
        ),
        (
            "search-longer",
            r#"url = { match = "regex", expression = 'https://search\.example/v[0-9]+' }"#
                .to_owned(),
        ),
        (
            "exact",
            r#"url = { match = "exact", value = "https://exact.example/mcp" }"#.to_owned(),
        ),
        (
            "exact-other",
            r#"url = { match = "exact", value = "https://exact.example/mcp" }"#.to_owned(),
        ),
    ]
    .iter()
    .map(|(name, identity)| format!("[mcp_servers.{name}]\nidentity = {{ {identity} }}\n\n"))
    .collect::<String>();
    let servers = [
        (
            "docs",
            r#"command = "docs-mcp"
args = ["--stdio", "n42"]"#,
        ),
        (
            "docs-partial",
            r#"command = "docs-mcp"
args = ["--stdio", "xn42"]"#,
        ),
        (
            "docs-short",
            r#"command = "docs-mcp"
args = ["--stdio"]"#,
        ),
        (
            "docs-prefix",
            r#"command = "docs-mcp-other"
args = ["--stdio", "n42"]"#,
        ),
        ("wiki", r#"url = "https://wiki.example/mcp""#),
        (
            "wiki-other",
            r#"url = "https://other.example/wiki.example/""#,
        ),
        // Their query values are Secrets, shown masked.
        (
            "wiki-key",
            r#"url = "https://wiki.example/mcp?key=not-a-real-secret""#,
        ),
        (
            "wiki-other-key",
            r#"url = "https://other.example/mcp?key=not-a-real-secret""#,
        ),
        ("search", r#"url = "https://search.example/v2""#),
        (
            "search-longer",
            r#"url = "https://search.example/v2/extra""#,
        ),
        ("exact", r#"url = "https://exact.example/mcp""#),
        ("exact-other", r#"url = "https://exact.example/mcp/""#),
    ]
    .iter()
    .map(|(name, server)| format!("[mcp_servers.{name}]\n{server}\n\n"))
    .collect::<String>();
    let fs = FakeFileSystem::new()
        .with_file(requirements(), requirements_toml)
        .with_file(user_config(), servers);

    let inventory = inventory(&fs);

    for name in ["docs", "wiki", "wiki-key", "search", "exact"] {
        assert_eq!(states(item(&inventory, name)), ["effective"], "{name}");
    }
    for name in [
        "docs-partial",
        "docs-short",
        "docs-prefix",
        "wiki-other",
        "wiki-other-key",
        "search-longer",
        "exact-other",
    ] {
        let item = item(&inventory, name);
        assert_eq!(states(item), ["blocked"], "{name}");
        assert_eq!(
            code(item.entries[0].reason.as_ref().unwrap().code),
            "policy",
            "{name}"
        );
    }
}

#[test]
fn a_masked_arg_is_taken_to_meet_a_regex_it_cannot_be_shown_against() {
    let fs = FakeFileSystem::new()
        .with_file(
            requirements(),
            "[mcp_servers.docs.identity.command]\nexecutable = \"docs-mcp\"\nargs = [{ match = \"exact\", value = \"--token\" }, { match = \"regex\", expression = \"sk-[a-z]+\" }]\n",
        )
        .with_file(
            user_config(),
            "[mcp_servers.docs]\ncommand = \"docs-mcp\"\nargs = [\"--token\", \"sk-secret\"]\n",
        );

    let inventory = inventory(&fs);

    let docs = item(&inventory, "docs");
    assert_eq!(docs.entries[0].config["args"][1], "sk-••••");
    assert_eq!(states(docs), ["effective"]);
}

#[test]
fn the_server_codex_builds_from_its_layers_meets_the_requirements_whole() {
    let fs = FakeFileSystem::new()
        .with_file(
            requirements(),
            "[mcp_servers.docs.identity]\ncommand = \"docs-mcp\"\n",
        )
        .with_file(
            system_config(),
            "[mcp_servers.docs]\ncommand = \"docs-mcp\"\n",
        )
        .with_file(user_config(), "[mcp_servers.docs]\nargs = [\"--user\"]\n");

    let inventory = inventory(&fs);

    assert_eq!(states(item(&inventory, "docs")), ["merged", "merged"]);
}

#[test]
fn the_cloud_bundle_s_requirements_join_the_machine_s_as_of_when_codex_cached_them() {
    let fs = FakeFileSystem::new()
        .with_file(requirements(), "[mcp_servers.docs.identity]\ncommand = \"docs-mcp\"\n")
        .with_file(
            cloud_cache(),
            bundle(&[], &["[mcp_servers.fetch.identity]\ncommand = \"uvx\"\n"]),
        )
        .with_file(
            user_config(),
            "[mcp_servers.docs]\ncommand = \"docs-mcp\"\n\n[mcp_servers.fetch]\ncommand = \"uvx\"\n\n[mcp_servers.other]\ncommand = \"other\"\n",
        );

    let inventory = inventory(&fs);

    assert_eq!(states(item(&inventory, "docs")), ["effective"]);
    assert_eq!(states(item(&inventory, "fetch")), ["effective"]);
    let other = item(&inventory, "other");
    assert_eq!(states(other), ["blocked"]);
    let reason = other.entries[0].reason.as_ref().unwrap();
    assert_eq!(code(reason.code), "policy");
    assert!(
        reason.sentence.contains("as of 1 Oct 2026 08:00 UTC"),
        "{}",
        reason.sentence
    );
}

#[test]
fn plugin_servers_meet_only_the_requirements_of_their_plugin() {
    let fs = FakeFileSystem::new()
        .with_file(
            requirements(),
            "[mcp_servers.docs.identity]\ncommand = \"docs-mcp\"\n\n[plugins.\"linear@openai-curated\".mcp_servers.linear.identity]\nurl = \"https://linear.example/mcp\"\n",
        )
        .with_file(user_config(), on(&["linear@openai-curated", "wiki@alpha"]))
        .with_file(
            plugin("openai-curated", "linear", "1.0.0").join(".mcp.json"),
            mcp_json("linear", "https://linear.example/mcp"),
        )
        .with_file(
            plugin("alpha", "wiki", "1.0.0").join(".mcp.json"),
            mcp_json("wiki", "https://wiki.example/mcp"),
        );

    let inventory = inventory(&fs);

    assert_eq!(states(item(&inventory, "linear")), ["effective"]);
    let wiki = item(&inventory, "wiki");
    assert_eq!(states(wiki), ["blocked"]);
    assert_eq!(
        code(wiki.entries[0].reason.as_ref().unwrap().code),
        "policy"
    );
}

#[test]
fn plugin_servers_are_left_alone_while_no_plugin_has_requirements() {
    let fs = FakeFileSystem::new()
        .with_file(
            requirements(),
            "[mcp_servers.docs.identity]\ncommand = \"docs-mcp\"\n",
        )
        .with_file(user_config(), on(&["wiki@alpha"]))
        .with_file(
            plugin("alpha", "wiki", "1.0.0").join(".mcp.json"),
            mcp_json("wiki", "https://wiki.example/mcp"),
        );

    let inventory = inventory(&fs);

    assert_eq!(states(item(&inventory, "wiki")), ["effective"]);
}

#[test]
fn a_server_a_requirement_turns_off_still_shadows_a_same_named_plugin_server() {
    let fs = FakeFileSystem::new()
        .with_file(
            requirements(),
            "[mcp_servers.docs.identity]\ncommand = \"docs-mcp\"\n",
        )
        .with_file(
            user_config(),
            format!(
                "[mcp_servers.linear]\ncommand = \"linear-local\"\n{}",
                on(&["linear@openai-curated"])
            ),
        )
        .with_file(
            plugin("openai-curated", "linear", "1.0.0").join(".mcp.json"),
            mcp_json("linear", "https://linear.example/mcp"),
        );

    let inventory = inventory(&fs);

    let linear = item(&inventory, "linear");
    assert_eq!(states(linear), ["blocked", "shadowed"]);
    assert_eq!(code(linear.state), "blocked");
}

#[test]
fn a_cached_bundle_that_does_not_say_when_still_carries_its_caveat() {
    let mut cache: Value = serde_json::from_str(&bundle(
        &["[mcp_servers.docs]\ncommand = \"docs-mcp\"\n"],
        &[],
    ))
    .unwrap();
    cache["signed_payload"]
        .as_object_mut()
        .unwrap()
        .remove("cached_at");
    let fs = FakeFileSystem::new()
        .with_file(user_config(), "")
        .with_file(cloud_cache(), cache.to_string());

    let inventory = inventory(&fs);

    let caveat = &item(&inventory, "docs").entries[0].caveats[0];
    assert_eq!(code(caveat.code), "cached_copy");
    assert!(
        caveat.sentence.contains("does not record"),
        "{}",
        caveat.sentence
    );
}
