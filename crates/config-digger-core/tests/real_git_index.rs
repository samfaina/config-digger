//! The real git-versioned check against a real repository: a file is
//! versioned when it is in the index, and ignored or untracked files are not.

use config_digger_core::ports::GitIndex;
use config_digger_core::ports::real::RealGitIndex;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::process::Command;

// The fixture's own git, which the test runs in its console.
#[allow(clippy::disallowed_methods)]
fn git(repo: &Path, args: &[&str]) {
    let status = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args([
            "-c",
            "user.name=fixture",
            "-c",
            "user.email=fixture@example.com",
        ])
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .status()
        .expect("git runs");
    assert!(status.success(), "git {args:?} failed");
}

fn write(path: &Path, contents: &str) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(path, contents).unwrap();
}

#[test]
fn only_a_file_in_the_index_is_versioned() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "--quiet"]);
    write(&repo.join(".gitignore"), ".claude/settings.local.json\n");
    write(&repo.join(".mcp.json"), "{}\n");
    write(&repo.join(".claude/settings.json"), "{}\n");
    git(&repo, &["add", ".gitignore", ".mcp.json"]);
    git(&repo, &["commit", "--quiet", "-m", "fixture"]);
    // Staged and never committed is in the index too.
    git(&repo, &["add", ".claude/settings.json"]);
    write(&repo.join(".claude/settings.local.json"), "{}\n");
    write(&repo.join("notes.json"), "{}\n");
    write(&root.path().join("outside.json"), "{}\n");

    let git = RealGitIndex;
    let versioned = |path: &Path| match git.repository(path.parent().unwrap()) {
        Some(repository) => git.versioned_in(&repository, &[path]).contains(path),
        None => false,
    };

    assert!(versioned(&repo.join(".mcp.json")), "committed");
    assert!(versioned(&repo.join(".claude/settings.json")), "staged");
    assert!(
        !versioned(&repo.join(".claude/settings.local.json")),
        "ignored"
    );
    assert!(!versioned(&repo.join("notes.json")), "untracked");
    assert!(!versioned(&repo.join("missing.json")), "missing");
    assert!(
        !versioned(&root.path().join("outside.json")),
        "outside any repo"
    );
}

#[test]
fn one_question_answers_for_every_path_in_the_repository() {
    let root = tempfile::tempdir().unwrap();
    let repo = root.path().join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "--quiet"]);
    write(&repo.join(".mcp.json"), "{}\n");
    write(&repo.join(".claude/skills/tracked/SKILL.md"), "# tracked\n");
    write(
        &repo.join(".claude/skills/untracked/SKILL.md"),
        "# untracked\n",
    );
    write(&repo.join(".claude/skills/tracked-too/SKILL.md"), "# no\n");
    git(&repo, &["add", ".mcp.json", ".claude/skills/tracked"]);
    let skills = repo.join(".claude/skills");
    let git = RealGitIndex;

    assert_eq!(git.repository(&skills), Some(repo.clone()));
    let asked = [
        repo.join(".mcp.json"),
        skills.join("tracked"),
        skills.join("untracked"),
        skills.join("tracked-too"),
        skills.join("track"),
    ];
    let asked: Vec<&Path> = asked.iter().map(PathBuf::as_path).collect();
    let versioned = git.versioned_in(&repo, &asked);

    assert_eq!(
        versioned,
        BTreeSet::from([repo.join(".mcp.json"), skills.join("tracked")]),
        "a folder is versioned when a file below it is"
    );
}
