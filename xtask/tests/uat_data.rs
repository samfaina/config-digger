//! The UAT's test data script, `uat/data/cd-throwaway-setup.ps1`, run with
//! Windows PowerShell against a temporary folder. Its guard on the folder
//! name is what keeps `-Reset` from deleting anything else.

#![cfg(windows)]

use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn script() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../uat/data/cd-throwaway-setup.ps1")
}

/// The script run with `-Root root`, and `-Reset` when `reset` is set.
fn setup(root: &Path, reset: bool) -> Output {
    let mut command = Command::new("powershell");
    command
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-File",
        ])
        .arg(script())
        .arg("-Root")
        .arg(root);
    if reset {
        command.arg("-Reset");
    }
    command.output().expect("Windows PowerShell runs")
}

fn stderr(output: &Output) -> String {
    String::from_utf8_lossy(&output.stderr).into_owned()
}

#[test]
fn a_folder_not_named_cd_throwaway_is_refused() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("projects");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("keep.txt"), "mine").unwrap();

    let output = setup(&root, true);

    assert!(!output.status.success());
    assert!(
        stderr(&output).contains("The folder must be called cd-throwaway"),
        "{}",
        stderr(&output)
    );
    assert_eq!(
        std::fs::read_to_string(root.join("keep.txt")).unwrap(),
        "mine"
    );
}

#[test]
fn an_existing_folder_is_kept_without_reset() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("cd-throwaway");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("keep.txt"), "mine").unwrap();

    let output = setup(&root, false);

    assert!(!output.status.success());
    assert!(stderr(&output).contains("-Reset"), "{}", stderr(&output));
    assert!(root.join("keep.txt").exists());
}

#[test]
fn reset_creates_the_test_data_again() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("cd-throwaway");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("stale.txt"), "old").unwrap();

    let output = setup(&root, true);

    assert!(output.status.success(), "{}", stderr(&output));
    assert!(!root.join("stale.txt").exists());
    let claude = std::fs::read_to_string(root.join("claude-code-home/.claude.json")).unwrap();
    assert!(claude.contains("cd-throwaway-user"));
    assert!(root.join("codex-home/config.toml").is_file());
    assert!(
        root.join("claude-desktop-home/claude_desktop_config.json")
            .is_file()
    );
}
