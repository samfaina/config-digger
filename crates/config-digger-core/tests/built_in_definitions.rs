//! The check CI runs over the built-in definition files, `cargo xtask
//! definitions`: every Agent definition and skill manager definition in
//! the folder loads, and the folder holds exactly the files that ship.

use config_digger_core::definitions::check_built_in_folder;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn built_in_folder() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("definitions")
}

/// A copy of the built-in definitions folder.
fn copy() -> TempDir {
    let copy = tempfile::tempdir().unwrap();
    for folder in ["", "skill-managers"] {
        std::fs::create_dir_all(copy.path().join(folder)).unwrap();
        for file in std::fs::read_dir(built_in_folder().join(folder)).unwrap() {
            let file = file.unwrap().path();
            if file.is_file() {
                let to = copy.path().join(folder).join(file.file_name().unwrap());
                std::fs::copy(&file, to).unwrap();
            }
        }
    }
    copy
}

#[test]
fn the_built_in_definitions_pass() {
    assert_eq!(
        check_built_in_folder(&built_in_folder()),
        Vec::<String>::new()
    );
}

#[test]
fn a_broken_agent_or_skill_manager_definition_fails() {
    let folder = copy();
    let codex = folder.path().join("codex.toml");
    let text = std::fs::read_to_string(&codex).unwrap();
    std::fs::write(&codex, text.replacen("[agent]", "[agent]\nshiny = true", 1)).unwrap();
    let skillfish = folder.path().join(r"skill-managers\skillfish.toml");
    let text = std::fs::read_to_string(&skillfish).unwrap();
    std::fs::write(&skillfish, text.replace("marker-file", "marker-glob")).unwrap();

    let problems = check_built_in_folder(folder.path());

    assert_eq!(problems.len(), 2, "{problems:#?}");
    assert!(
        problems[0].contains("codex.toml") && problems[0].contains("shiny"),
        "{}",
        problems[0]
    );
    assert!(
        problems[1].contains("skillfish.toml") && problems[1].contains("marker-glob"),
        "{}",
        problems[1]
    );
}

#[test]
fn a_file_that_does_not_ship_or_a_missing_one_fails() {
    let folder = copy();
    std::fs::copy(
        folder.path().join("codex.toml"),
        folder.path().join("codex-copy.toml"),
    )
    .unwrap();
    std::fs::remove_file(folder.path().join(r"skill-managers\openskills.toml")).unwrap();

    let problems = check_built_in_folder(folder.path());

    assert_eq!(problems.len(), 2, "{problems:#?}");
    assert!(
        problems
            .iter()
            .any(|problem| problem.contains("codex-copy.toml") && problem.contains("not built in")),
        "{problems:#?}"
    );
    assert!(
        problems
            .iter()
            .any(|problem| problem.contains("openskills.toml") && problem.contains("missing")),
        "{problems:#?}"
    );
}
