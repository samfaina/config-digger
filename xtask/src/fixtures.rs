//! The fixture files under `crates/*/tests/fixtures`, checked against git's
//! ignore rules, a contributor's global excludes included. A fixture git
//! ignores is dropped from a commit without any warning.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

/// The files under a `crates/*/tests/fixtures` folder of `repo` that git
/// ignores, relative to `repo`, with `/` separators. Tracked files count:
/// a force-added fixture is one a new copy of would be dropped.
pub fn ignored(repo: &Path) -> Result<Vec<String>, String> {
    let mut files = Vec::new();
    let crates = repo.join("crates");
    let entries = std::fs::read_dir(&crates)
        .map_err(|error| format!("cannot read {}: {error}", crates.display()))?;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let fixtures = format!("crates/{name}/tests/fixtures");
        if repo.join(&fixtures).is_dir() {
            files_in(repo, &fixtures, &mut files)?;
        }
    }
    if files.is_empty() {
        return Ok(files);
    }
    files.sort();
    check_ignore(repo, &files)
}

/// Adds every file in the folder `folder` of `repo`, and in its folders, to
/// `files`.
fn files_in(repo: &Path, folder: &str, files: &mut Vec<String>) -> Result<(), String> {
    let entries = std::fs::read_dir(repo.join(folder))
        .map_err(|error| format!("cannot read {folder}: {error}"))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("cannot read {folder}: {error}"))?;
        let path = format!("{folder}/{}", entry.file_name().to_string_lossy());
        if entry.path().is_dir() {
            files_in(repo, &path, files)?;
        } else {
            files.push(path);
        }
    }
    Ok(())
}

/// Which of `files` git ignores in `repo`, as `git check-ignore` reports
/// them. `--no-index` checks tracked files as if they were new.
fn check_ignore(repo: &Path, files: &[String]) -> Result<Vec<String>, String> {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["check-ignore", "--no-index", "--stdin", "-z"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| format!("cannot run git: {error}"))?;
    let mut stdin = child.stdin.take().expect("stdin is piped");
    let input: Vec<u8> = files
        .iter()
        .flat_map(|file| [file.as_bytes(), b"\0"].concat())
        .collect();
    let written = stdin.write_all(&input);
    drop(stdin);
    let output = child
        .wait_with_output()
        .map_err(|error| format!("cannot run git: {error}"))?;
    written.map_err(|error| format!("cannot write to git check-ignore: {error}"))?;
    // 0: some files are ignored; 1: none is.
    match output.status.code() {
        Some(0 | 1) => Ok(String::from_utf8_lossy(&output.stdout)
            .split('\0')
            .filter(|file| !file.is_empty())
            .map(str::to_owned)
            .collect()),
        _ => Err(format!(
            "git check-ignore failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn no_committed_fixture_is_ignored() {
        let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("..");

        assert_eq!(ignored(&repo), Ok(Vec::<String>::new()));
    }

    /// The global exclude that dropped the VS Code fixture of #63.
    const GLOBAL_EXCLUDE: &str = "**/.claude/settings.local.json\n";

    const SETTINGS_LOCAL: &str = "crates/demo/tests/fixtures/app/.claude/settings.local.json";
    const VSCODE_MCP: &str = "crates/demo/tests/fixtures/app/.vscode/mcp.json";

    /// A new repository with `gitignore` as its `.gitignore`, `GLOBAL_EXCLUDE`
    /// as its `core.excludesFile`, and an empty file at each of `files`.
    fn repo(gitignore: &str, files: &[&str]) -> tempfile::TempDir {
        let repo = tempfile::tempdir().expect("a temporary folder");
        git(repo.path(), &["init", "--quiet"]);
        let excludes = repo.path().join(".git").join("global-excludes");
        std::fs::write(&excludes, GLOBAL_EXCLUDE).expect("the global excludes are written");
        git(
            repo.path(),
            &["config", "core.excludesFile", &excludes.to_string_lossy()],
        );
        std::fs::write(repo.path().join(".gitignore"), gitignore).expect("the .gitignore");
        for file in files {
            let path = repo.path().join(file);
            std::fs::create_dir_all(path.parent().expect("a folder")).expect("the folder");
            std::fs::write(path, "").expect("the file is written");
        }
        repo
    }

    /// The output of `git args` in `repo`, which must succeed.
    fn git(repo: &Path, args: &[&str]) -> String {
        let output = std::process::Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()
            .expect("git runs");
        assert!(output.status.success(), "git {args:?} failed: {output:?}");
        String::from_utf8(output.stdout).expect("UTF-8 output")
    }

    #[test]
    fn a_fixture_a_global_exclude_drops_is_ignored() {
        let repo = repo(
            "",
            &[SETTINGS_LOCAL, "crates/demo/tests/fixtures/app/.mcp.json"],
        );

        assert_eq!(ignored(repo.path()), Ok(vec![SETTINGS_LOCAL.to_owned()]));
    }

    const GITIGNORE: &str = include_str!("../../.gitignore");

    /// The files `git status` shows in `repo`, untracked ones included.
    fn status(repo: &Path) -> Vec<String> {
        git(repo, &["status", "--porcelain", "--untracked-files=all"])
            .lines()
            .map(|line| line.trim_start_matches("?? ").to_owned())
            .collect()
    }

    #[test]
    fn the_gitignore_re_includes_a_fixture_a_global_exclude_drops() {
        let repo = repo(GITIGNORE, &[SETTINGS_LOCAL, VSCODE_MCP]);

        assert_eq!(ignored(repo.path()), Ok(Vec::new()));
        assert_eq!(
            status(repo.path()),
            [".gitignore", SETTINGS_LOCAL, VSCODE_MCP]
        );
    }

    #[test]
    fn build_output_stays_ignored_inside_and_outside_fixtures() {
        let fixture_target = "crates/demo/tests/fixtures/app/target/debug/out";
        let fixture_modules = "crates/demo/tests/fixtures/app/node_modules/x/index.js";
        let repo = repo(
            GITIGNORE,
            &[
                fixture_target,
                fixture_modules,
                "target/debug/out",
                "crates/demo/ui/node_modules/x/index.js",
                "crates/config-digger-app/gen/schemas/x.json",
                "crates/config-digger-app/ui/dist/index.html",
                "crates/config-digger-app/permissions/autogenerated/x.toml",
            ],
        );

        assert_eq!(
            ignored(repo.path()),
            Ok(vec![fixture_modules.to_owned(), fixture_target.to_owned()])
        );
        assert_eq!(status(repo.path()), [".gitignore"]);
    }
}
