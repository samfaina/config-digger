//! The real git-versioned check against a real repository: a file is
//! versioned when it is in the index, and ignored or untracked files are not.

use config_digger_core::ports::GitIndex;
use config_digger_core::ports::real::RealGitIndex;
use std::path::Path;
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

    assert!(git.is_versioned(&repo.join(".mcp.json")), "committed");
    assert!(
        git.is_versioned(&repo.join(".claude/settings.json")),
        "staged"
    );
    assert!(
        !git.is_versioned(&repo.join(".claude/settings.local.json")),
        "ignored"
    );
    assert!(!git.is_versioned(&repo.join("notes.json")), "untracked");
    assert!(!git.is_versioned(&repo.join("missing.json")), "missing");
    assert!(
        !git.is_versioned(&root.path().join("outside.json")),
        "outside any repo"
    );
}
