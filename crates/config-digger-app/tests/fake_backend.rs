//! The fixtures of the UI's fake backend, recorded from the core over the
//! UAT's throwaway data, which `uat/data/cd-throwaway-setup.ps1` creates in
//! a temporary folder.

#![cfg(windows)]

use serde_json::Value;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// The throwaway data, created by the UAT's script in a temporary folder.
// A test runs in a console, so PowerShell opens no window of its own.
#[allow(clippy::disallowed_methods)]
fn throwaway() -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("cd-throwaway");
    let script =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../uat/data/cd-throwaway-setup.ps1");
    let output = Command::new("powershell")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(script)
        .arg("-Root")
        .arg(&root)
        .output()
        .expect("Windows PowerShell runs");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    (temp, root)
}

/// The fixtures, recorded once for every test, and the temporary folder
/// they were recorded in, deleted since, both as given, such as with a short
/// name like `RUNNER~1`, and canonical, with long names.
fn recorded() -> &'static ([PathBuf; 2], Value) {
    static RECORDED: OnceLock<([PathBuf; 2], Value)> = OnceLock::new();
    RECORDED.get_or_init(|| {
        let (temp, root) = throwaway();
        let fixtures = config_digger_app::fake_backend::record(&root).unwrap();
        let temps = [
            temp.path().to_path_buf(),
            std::fs::canonicalize(temp.path()).unwrap(),
        ];
        (temps, fixtures)
    })
}

/// `path` lowercased, with each run of slashes of either kind as one `/`,
/// so that it is found however a string spells or escapes it.
fn normalized(path: &str) -> String {
    let mut out = String::new();
    for c in path.to_lowercase().chars() {
        let c = if c == '\\' { '/' } else { c };
        if !(c == '/' && out.ends_with('/')) {
            out.push(c);
        }
    }
    out.trim_start_matches("//?/")
        .trim_start_matches("/?/")
        .to_owned()
}

/// Each Windows path in `text`, as [`normalized`] spells it, from its drive
/// letter on, such as `c:/program files/app`; a URL's `https://` is none.
fn absolute_paths(text: &str) -> Vec<&str> {
    let bytes = text.as_bytes();
    (0..bytes.len().saturating_sub(2))
        .filter(|&at| {
            bytes[at].is_ascii_alphabetic()
                && (at == 0 || !bytes[at - 1].is_ascii_alphanumeric())
                && bytes[at + 1] == b':'
                && bytes[at + 2] == b'/'
        })
        .map(|at| {
            let path = &text[at..];
            let end = path.find(['"', '\n', '#']).unwrap_or(path.len());
            &path[..end]
        })
        .collect()
}

/// Every string in `value`, object keys included.
fn strings(value: &Value) -> Vec<&str> {
    match value {
        Value::String(text) => vec![text],
        Value::Array(values) => values.iter().flat_map(strings).collect(),
        Value::Object(map) => map
            .iter()
            .flat_map(|(key, value)| {
                let mut found = vec![key.as_str()];
                found.extend(strings(value));
                found
            })
            .collect(),
        _ => Vec::new(),
    }
}

#[test]
fn every_item_is_a_throwaway_one() {
    let (_, fixtures) = recorded();

    let inventories = fixtures["inventories"].as_array().unwrap();
    assert!(inventories.len() >= 2, "{inventories:?}");
    let names: Vec<&str> = inventories
        .iter()
        .flat_map(|recorded| recorded["inventory"]["items"].as_array().unwrap())
        .map(|item| item["name"].as_str().unwrap())
        .collect();
    assert!(names.contains(&"cd-throwaway-codex"), "{names:?}");
    assert!(names.contains(&"cd-throwaway-project"), "{names:?}");
    // Cursor names a plugin's server `plugin-<plugin>-<server>`, and Claude
    // Code a plugin's server `plugin:<plugin>:<server>` and a skill synced
    // from claude.ai `anthropic-skills:<skill>`.
    assert!(
        names.iter().all(|name| name
            .strip_prefix("plugin-")
            .or_else(|| name.strip_prefix("plugin:"))
            .or_else(|| name.strip_prefix("anthropic-skills:"))
            .unwrap_or(name)
            .starts_with("cd-throwaway-")),
        "{names:?}"
    );
}

#[test]
fn paths_are_under_the_uat_root_and_name_no_folder_of_this_machine() {
    let (temps, fixtures) = recorded();

    let mut machine: Vec<String> = temps
        .iter()
        .map(|temp| normalized(&temp.display().to_string()))
        .collect();
    machine.push(normalized(&std::env::var("USERPROFILE").unwrap()));
    for text in strings(fixtures) {
        let text = normalized(text);
        for path in &machine {
            assert!(!text.contains(path.as_str()), "{text} names {path}");
        }
    }
    for text in strings(fixtures) {
        let text = normalized(text);
        for path in absolute_paths(&text) {
            assert!(
                path.starts_with("e:/cd-throwaway/") || path == "e:/cd-throwaway",
                "{path} in {text}"
            );
        }
    }
    let codex = r"E:\cd-throwaway\codex-home\config.toml";
    assert!(
        strings(fixtures).iter().any(|text| text.contains(codex)),
        "no path under E:\\cd-throwaway"
    );
}

#[test]
fn every_screen_has_what_it_shows() {
    let (_, fixtures) = recorded();

    let agents = fixtures["places"]["agents"].as_array().unwrap();
    let homes: Vec<&str> = agents
        .iter()
        .flat_map(|agent| agent["homes"].as_array().unwrap())
        .map(|home| home["home"].as_str().unwrap())
        .collect();
    assert!(
        homes
            .iter()
            .any(|home| home.ends_with("claude-desktop-home")),
        "{homes:?}"
    );
    // Claude Code's homes whose last parts fill a tile in the smallest
    // window, the one signed out of its claude.ai account, and the one with
    // plugins of a `directory` marketplace.
    for last in [
        "claude-code-shape-home",
        "claude-code-synced-home",
        "claude-code-many-home",
        "claude-code-logout-home",
        "claude-code-market-home",
    ] {
        assert!(
            homes.iter().any(|home| home.ends_with(last)),
            "{last} in {homes:?}"
        );
    }
    let history = fixtures["history"].as_array().unwrap();
    assert!(!history.is_empty());
    assert_eq!(fixtures["details"].as_array().unwrap().len(), history.len());
    assert!(!fixtures["previews"].as_array().unwrap().is_empty());
    assert_eq!(fixtures["executed"]["kind"], "applied");
    assert!(
        !fixtures["settings"]["parked"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(fixtures["about"]["version"].is_string());
    let stranded = fixtures["places"]["stranded"].as_array().unwrap();
    assert_eq!(stranded[0]["name"], "cd-throwaway-stranded", "{stranded:?}");
    let previews = fixtures["previews"].as_array().unwrap();
    assert!(
        previews
            .iter()
            .any(|preview| preview["request"]["kind"] == "stranded"),
        "{previews:?}"
    );
}

#[test]
fn a_directory_marketplace_s_listed_plugin_is_read_and_its_unlisted_one_fails() {
    let (_, fixtures) = recorded();

    let inventory = &fixtures["inventories"][0]["inventory"];
    let item = |name: &str| {
        inventory["items"]
            .as_array()
            .unwrap()
            .iter()
            .find(|item| {
                item["name"] == name
                    && item["home"]
                        .as_str()
                        .unwrap()
                        .ends_with("claude-code-market-home")
            })
            .unwrap_or_else(|| panic!("no {name}"))
    };
    let listed = item("cd-throwaway-listed:cd-throwaway-listed-skill");
    assert_eq!(listed["state"], "effective", "{listed:#}");
    assert!(
        normalized(listed["entries"][0]["path"].as_str().unwrap())
            .ends_with("/market/plugins/cd-throwaway-listed/skills/cd-throwaway-listed-skill"),
        "{listed:#}"
    );
    let unlisted = item("cd-throwaway-notlisted:cd-throwaway-notlisted-skill");
    assert_eq!(unlisted["state"], "invalid", "{unlisted:#}");
    let reason = &unlisted["entries"][0]["reason"];
    assert_eq!(reason["code"], "plugin_failed_to_load", "{unlisted:#}");
    assert!(
        reason["sentence"]
            .as_str()
            .unwrap()
            .ends_with(r"\market\.claude-plugin\marketplace.json, does not list it."),
        "{unlisted:#}"
    );
}

#[test]
fn a_synced_skill_on_under_its_anthropic_skills_name_beside_its_folder_off_is_effective() {
    let (_, fixtures) = recorded();

    let inventory = &fixtures["inventories"][0]["inventory"];
    let item = inventory["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| {
            item["name"] == "anthropic-skills:cd-throwaway-overridden-skill"
                && item["home"]
                    .as_str()
                    .unwrap()
                    .ends_with("claude-code-synced-home")
        })
        .expect("the overridden synced skill");
    assert_eq!(item["state"], "effective", "{item:#}");
    assert_eq!(
        item["entries"][0]["notes"],
        serde_json::json!([]),
        "{item:#}"
    );
}

/// The skill `name` of `claude-code-synced-home` in the Inventory of the
/// Project `project`, the second context recorded.
fn synced_home_skill_in_project<'f>(fixtures: &'f Value, name: &str) -> &'f Value {
    let inventory = &fixtures["inventories"][1]["inventory"];
    inventory["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| {
            item["name"] == name
                && item["home"]
                    .as_str()
                    .unwrap()
                    .ends_with("claude-code-synced-home")
        })
        .unwrap_or_else(|| panic!("no skill `{name}`"))
}

#[test]
fn a_synced_skill_on_in_the_user_settings_beside_its_folder_off_in_the_project_s_local_settings_is_effective()
 {
    let (_, fixtures) = recorded();

    let item = synced_home_skill_in_project(fixtures, "anthropic-skills:cd-throwaway-spread-skill");
    assert_eq!(item["state"], "effective", "{item:#}");
    assert_eq!(
        item["entries"][0]["notes"],
        serde_json::json!([]),
        "{item:#}"
    );
}

#[test]
fn enable_of_a_skill_off_in_the_user_settings_writes_on_in_the_project_s_local_settings() {
    let (_, fixtures) = recorded();

    let item = synced_home_skill_in_project(fixtures, "cd-throwaway-user-off-skill");
    assert_eq!(item["state"], "disabled", "{item:#}");
    let enable = &item["entries"][0]["enable"];
    assert_eq!(enable["status"], "offered", "{item:#}");
    let sentence = enable["sentence"].as_str().unwrap();
    assert!(
        sentence
            .starts_with(r#"Enable sets `skillOverrides.cd-throwaway-user-off-skill` to "on" in "#)
            && sentence.ends_with(r"\project\.claude\settings.local.json for this Project."),
        "{sentence}"
    );
}
