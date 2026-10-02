//! Black-box tests: the real `config-digger` binary with the real ports,
//! against a temporary home selected through environment overrides.

use serde_json::{Value, json};
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

const CLAUDE_JSON: &str =
    include_str!("../../config-digger-core/tests/fixtures/claude-code-home/.claude.json");

/// A temporary user home folder the binary is pointed at.
struct Harness {
    home: TempDir,
}

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

impl Harness {
    fn new() -> Self {
        Self {
            home: tempfile::tempdir().unwrap(),
        }
    }

    fn with_file(self, relative: &str, contents: &str) -> Self {
        let path = self.path(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
        self
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.home.path().join(relative)
    }

    fn run(&self, args: &[&str]) -> Run {
        let mut command = self.command(args);
        command
            .env("USERPROFILE", self.home.path())
            .env("HOME", self.home.path());
        execute(command)
    }

    /// Runs with no way to tell where the user's home folder is.
    fn run_without_home(&self, args: &[&str]) -> Run {
        let mut command = self.command(args);
        command.env_remove("USERPROFILE").env_remove("HOME");
        execute(command)
    }

    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_config-digger"));
        command
            .args(args)
            .current_dir(self.home.path())
            .env_remove("CLAUDE_CONFIG_DIR")
            .env_remove("RUST_LOG");
        command
    }
}

fn execute(mut command: Command) -> Run {
    let output = command.output().unwrap();
    Run {
        code: output.status.code().unwrap(),
        stdout: String::from_utf8(output.stdout).unwrap(),
        stderr: String::from_utf8(output.stderr).unwrap(),
    }
}

impl Run {
    /// Stdout parsed as exactly one JSON document.
    fn document(&self) -> Value {
        serde_json::from_str(&self.stdout)
            .unwrap_or_else(|e| panic!("stdout is not one JSON document ({e}):\n{}", self.stdout))
    }
}

fn locator(path: &Path, key_path: &str) -> String {
    format!("{}#{key_path}", path.display())
}

#[test]
fn list_prints_each_user_server_with_name_scope_location_and_state() {
    let harness = Harness::new().with_file(".claude.json", CLAUDE_JSON);

    let run = harness.run(&["list"]);

    assert_eq!(run.code, 0, "{}", run.stderr);
    let claude_json = harness.path(".claude.json");
    for name in ["cd-throwaway-github", "linear"] {
        let location = locator(&claude_json, &format!("mcpServers.{name}"));
        let row = run
            .stdout
            .lines()
            .find(|line| line.contains(&location))
            .unwrap_or_else(|| panic!("no row for {name}:\n{}", run.stdout));
        let columns: Vec<&str> = row.split_whitespace().collect();
        assert_eq!(columns, [name, "user", "Effective", location.as_str()]);
    }
    assert!(run.stdout.contains("Claude Code"), "{}", run.stdout);
    assert!(!run.stdout.contains("dart"), "{}", run.stdout);
}

#[test]
fn list_json_prints_one_document_with_schema_version() {
    let harness = Harness::new().with_file(".claude.json", CLAUDE_JSON);

    let run = harness.run(&["list", "--json"]);

    assert_eq!(run.code, 0, "{}", run.stderr);
    let document = run.document();
    assert_eq!(document["schema_version"], 1);
    assert_eq!(
        document["homes"],
        json!([{ "agent": "claude-code", "agent_name": "Claude Code", "home": "default" }])
    );
    let items: Vec<(&str, &str, &str)> = document["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| {
            (
                item["name"].as_str().unwrap(),
                item["entries"][0]["scope"].as_str().unwrap(),
                item["state"].as_str().unwrap(),
            )
        })
        .collect();
    assert_eq!(
        items,
        [
            ("cd-throwaway-github", "user", "effective"),
            ("linear", "user", "effective")
        ]
    );
    assert_eq!(
        document["items"][1]["entries"][0]["locator"],
        locator(&harness.path(".claude.json"), "mcpServers.linear")
    );
}

#[test]
fn list_without_an_agent_home_says_so() {
    let run = Harness::new().run(&["list"]);

    assert_eq!(run.code, 0, "{}", run.stderr);
    assert!(run.stdout.contains("No Agent home found"), "{}", run.stdout);
}

#[test]
fn read_failures_go_to_stderr_and_list_still_exits_0() {
    let harness = Harness::new().with_file(".claude.json", "{ not json");

    let run = harness.run(&["list", "--json"]);

    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(run.document()["read_failures"][0]["kind"], "parse");
    let claude_json = harness.path(".claude.json");
    assert!(
        run.stderr.contains(&claude_json.display().to_string()),
        "{}",
        run.stderr
    );
}

#[test]
fn an_unknown_command_is_a_usage_error() {
    let run = Harness::new().run(&["frobnicate"]);

    assert_eq!(run.code, 2);
    assert!(run.stdout.is_empty(), "{}", run.stdout);
    assert!(!run.stderr.is_empty());
}

#[test]
fn a_usage_error_with_json_is_one_error_document() {
    let run = Harness::new().run(&["list", "--json", "--frobnicate"]);

    assert_eq!(run.code, 2);
    let document = run.document();
    assert_eq!(document["schema_version"], 1);
    assert_eq!(document["error"]["code"], "usage");
    assert!(
        document["error"]["message"]
            .as_str()
            .unwrap()
            .contains("--frobnicate")
    );
    assert_eq!(document.as_object().unwrap().len(), 2, "{document}");
}

#[test]
fn an_unexpected_error_exits_1_with_an_error_document() {
    let run = Harness::new().run_without_home(&["list", "--json"]);

    assert_eq!(run.code, 1, "{}", run.stderr);
    let document = run.document();
    assert_eq!(document["schema_version"], 1);
    assert_eq!(document["error"]["code"], "unexpected");
    assert!(document["error"]["message"].is_string());
}

#[test]
fn an_unexpected_error_in_text_goes_to_stderr() {
    let run = Harness::new().run_without_home(&["list"]);

    assert_eq!(run.code, 1);
    assert!(run.stdout.is_empty(), "{}", run.stdout);
    assert!(run.stderr.contains("home folder"), "{}", run.stderr);
}

#[test]
fn help_exits_0() {
    let run = Harness::new().run(&["--help"]);

    assert_eq!(run.code, 0);
    assert!(run.stdout.contains("list"), "{}", run.stdout);
}

#[test]
fn verbose_logs_go_to_stderr_without_configuration_values() {
    let harness = Harness::new().with_file(".claude.json", CLAUDE_JSON);

    let run = harness.run(&["list", "--json", "-vv"]);

    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(run.document()["schema_version"], 1);
    assert!(run.stderr.contains("read location"), "{}", run.stderr);
    for value in ["not-a-real-secret", "mcp.linear.app", "server-github"] {
        assert!(
            !run.stderr.contains(value),
            "{value} logged:\n{}",
            run.stderr
        );
    }
}

#[test]
fn without_verbose_nothing_is_logged() {
    let harness = Harness::new().with_file(".claude.json", CLAUDE_JSON);

    let run = harness.run(&["list", "--json"]);

    assert_eq!(run.code, 0);
    assert!(run.stderr.is_empty(), "{}", run.stderr);
}
