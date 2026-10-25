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
    setup_with(root, reset, &[])
}

/// The script run as [`setup`] runs it, with the switches `more` too.
fn setup_with(root: &Path, reset: bool, more: &[&str]) -> Output {
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
    command.args(more);
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

#[test]
fn symlink_adds_a_skill_behind_a_relative_folder_symlink_and_reset_keeps_working() {
    let temp = tempfile::tempdir().unwrap();
    let probe = temp.path().join("probe");
    if let Err(error) = std::os::windows::fs::symlink_dir(".", &probe) {
        eprintln!("skipped: this machine cannot create symlinks: {error}");
        return;
    }
    let root = temp.path().join("cd-throwaway");

    let output = setup_with(&root, false, &["-Symlink"]);

    assert!(output.status.success(), "{}", stderr(&output));
    let link = root.join(r"claude-code-home\.claude\skills\cd-throwaway-linked-skill");
    assert_eq!(
        std::fs::read_link(&link).unwrap(),
        Path::new(r"..\..\..\linked\cd-throwaway-linked-skill")
    );
    assert!(link.join("SKILL.md").is_file());

    let again = setup_with(&root, true, &["-Symlink"]);

    assert!(again.status.success(), "{}", stderr(&again));
    assert!(link.join("SKILL.md").is_file());
}

#[test]
fn cursor_adds_its_own_appdata_and_a_git_project_with_a_nested_skill() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("cd-throwaway");

    let output = setup_with(&root, false, &["-Cursor"]);

    assert!(output.status.success(), "{}", stderr(&output));
    let user = root.join(r"cursor-appdata\Cursor\User");
    assert!(user.join("settings.json").is_file());
    let workspace =
        std::fs::read_to_string(user.join(r"workspaceStorage\cd-throwaway\workspace.json"))
            .unwrap();
    assert!(
        workspace.contains("/cd-throwaway/cursor-project\""),
        "{workspace}"
    );
    let project = root.join("cursor-project");
    assert!(project.join(".git").is_dir());
    assert!(
        project
            .join(r"packages\app\.cursor\skills\cd-throwaway-nested-skill\SKILL.md")
            .is_file()
    );

    let again = setup_with(&root, true, &["-Cursor"]);

    assert!(again.status.success(), "{}", stderr(&again));
}
