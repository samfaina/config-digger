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
    assert!(
        names.iter().all(|name| name.starts_with("cd-throwaway-")),
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
}
