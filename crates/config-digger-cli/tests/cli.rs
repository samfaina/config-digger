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
    root: TempDir,
    home: PathBuf,
}

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

impl Harness {
    fn new() -> Self {
        Self::in_folder("home")
    }

    /// A home in a folder with the given name, inside a fresh temporary folder.
    fn in_folder(name: &str) -> Self {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join(name);
        std::fs::create_dir(&home).unwrap();
        Self { root, home }
    }

    fn with_file(self, relative: &str, contents: &str) -> Self {
        let path = self.path(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
        self
    }

    fn path(&self, relative: &str) -> PathBuf {
        self.home.join(relative)
    }

    fn run(&self, args: &[&str]) -> Run {
        self.run_with_env(args, &[])
    }

    fn run_with_env(&self, args: &[&str], env: &[(&str, &str)]) -> Run {
        let mut command = self.command(args);
        command
            .env("USERPROFILE", &self.home)
            .env("HOME", &self.home)
            .envs(env.iter().copied());
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
            .current_dir(self.root.path())
            .env_remove("CLAUDE_CONFIG_DIR")
            .env_remove("RUST_LOG");
        // Terminal conventions the tests opt into one at a time.
        for variable in ["NO_COLOR", "CLICOLOR", "CLICOLOR_FORCE", "COLUMNS"] {
            command.env_remove(variable);
        }
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

const ESC: char = '\u{1b}';

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

#[test]
fn completions_print_a_script_for_each_shell() {
    for (shell, marker) in [
        ("powershell", "Register-ArgumentCompleter"),
        ("bash", "complete -F"),
        ("zsh", "#compdef config-digger"),
        ("fish", "complete -c config-digger"),
    ] {
        let run = Harness::new().run(&["completions", shell]);

        assert_eq!(run.code, 0, "{shell}: {}", run.stderr);
        assert!(run.stdout.contains(marker), "{shell}:\n{}", run.stdout);
        assert!(run.stdout.contains("list"), "{shell}:\n{}", run.stdout);
        assert!(run.stderr.is_empty(), "{shell}: {}", run.stderr);
    }
}

#[test]
fn completions_for_an_unknown_shell_is_a_usage_error() {
    let run = Harness::new().run(&["completions", "tcsh"]);

    assert_eq!(run.code, 2);
    assert!(run.stdout.is_empty(), "{}", run.stdout);
}

#[test]
fn completions_with_json_is_a_usage_error_document() {
    let run = Harness::new().run(&["completions", "bash", "--json"]);

    assert_eq!(run.code, 2);
    assert_eq!(run.document()["error"]["code"], "usage");
}

#[test]
fn color_always_colours_the_list() {
    let harness = Harness::new().with_file(".claude.json", CLAUDE_JSON);

    let run = harness.run(&["list", "--color", "always"]);

    assert_eq!(run.code, 0, "{}", run.stderr);
    assert!(run.stdout.contains(ESC), "{}", run.stdout);
}

#[test]
fn color_never_prints_no_escape_codes_even_when_forced() {
    let harness = Harness::new().with_file(".claude.json", CLAUDE_JSON);

    let run = harness.run_with_env(&["list", "--color", "never"], &[("CLICOLOR_FORCE", "1")]);

    assert_eq!(run.code, 0, "{}", run.stderr);
    assert!(run.stdout.contains("Effective"), "{}", run.stdout);
    assert!(!run.stdout.contains(ESC), "{}", run.stdout);
}

#[test]
fn color_auto_is_off_when_stdout_is_not_a_terminal() {
    let harness = Harness::new().with_file(".claude.json", CLAUDE_JSON);

    let run = harness.run(&["list"]);

    assert_eq!(run.code, 0, "{}", run.stderr);
    assert!(!run.stdout.contains(ESC), "{}", run.stdout);
}

#[test]
fn no_color_turns_colour_off_in_auto() {
    let harness = Harness::new().with_file(".claude.json", CLAUDE_JSON);

    let forced = harness.run_with_env(&["list"], &[("CLICOLOR_FORCE", "1")]);
    let no_color = harness.run_with_env(
        &["list", "--color", "auto"],
        &[("CLICOLOR_FORCE", "1"), ("NO_COLOR", "1")],
    );

    assert!(forced.stdout.contains(ESC), "{}", forced.stdout);
    assert_eq!(no_color.code, 0, "{}", no_color.stderr);
    assert!(no_color.stdout.contains("Effective"), "{}", no_color.stdout);
    assert!(!no_color.stdout.contains(ESC), "{}", no_color.stdout);
}

#[test]
fn an_empty_no_color_is_ignored() {
    let harness = Harness::new().with_file(".claude.json", CLAUDE_JSON);

    let run = harness.run_with_env(&["list"], &[("CLICOLOR_FORCE", "1"), ("NO_COLOR", "")]);

    assert!(run.stdout.contains(ESC), "{}", run.stdout);
}

#[test]
fn color_always_overrides_no_color() {
    let harness = Harness::new().with_file(".claude.json", CLAUDE_JSON);

    let run = harness.run_with_env(&["list", "--color", "always"], &[("NO_COLOR", "1")]);

    assert!(run.stdout.contains(ESC), "{}", run.stdout);
}

#[test]
fn color_applies_to_warnings_on_stderr() {
    let harness = Harness::new().with_file(".claude.json", "{ not json");

    let always = harness.run(&["list", "--color", "always"]);
    let never = harness.run_with_env(&["list", "--color", "never"], &[("CLICOLOR_FORCE", "1")]);

    assert!(always.stderr.contains(ESC), "{}", always.stderr);
    assert!(never.stderr.contains("warning"), "{}", never.stderr);
    assert!(!never.stderr.contains(ESC), "{}", never.stderr);
}

#[test]
fn color_never_applies_to_help() {
    let harness = Harness::new();

    let forced = harness.run_with_env(&["--help"], &[("CLICOLOR_FORCE", "1")]);
    let never = harness.run_with_env(&["--help", "--color", "never"], &[("CLICOLOR_FORCE", "1")]);

    assert!(forced.stdout.contains(ESC), "{}", forced.stdout);
    assert_eq!(never.code, 0);
    assert!(!never.stdout.contains(ESC), "{}", never.stdout);
}

#[test]
fn json_is_never_coloured() {
    let harness = Harness::new().with_file(".claude.json", CLAUDE_JSON);

    let run = harness.run(&["list", "--json", "--color", "always"]);

    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(run.document()["schema_version"], 1);
}

#[test]
fn an_unknown_color_is_a_usage_error() {
    let run = Harness::new().run(&["list", "--color", "sometimes"]);

    assert_eq!(run.code, 2);
}

#[test]
fn list_truncates_to_the_terminal_width() {
    let harness = Harness::new().with_file(".claude.json", CLAUDE_JSON);

    let run = harness.run_with_env(&["list"], &[("COLUMNS", "80")]);

    assert_eq!(run.code, 0, "{}", run.stderr);
    for line in run.stdout.lines() {
        assert!(line.chars().count() <= 80, "too wide:\n{}", run.stdout);
    }
    assert!(run.stdout.contains('…'), "{}", run.stdout);
    // The key path is what tells the rows apart, so it survives the cut.
    for name in ["cd-throwaway-github", "linear"] {
        let key_path = format!("#mcpServers.{name}");
        assert!(run.stdout.contains(&key_path), "{}", run.stdout);
    }
}

#[test]
fn list_wide_does_not_truncate() {
    let harness = Harness::new().with_file(".claude.json", CLAUDE_JSON);

    let run = harness.run_with_env(&["list", "--wide"], &[("COLUMNS", "80")]);

    assert_eq!(run.code, 0, "{}", run.stderr);
    assert!(!run.stdout.contains('…'), "{}", run.stdout);
    let location = locator(&harness.path(".claude.json"), "mcpServers.linear");
    assert!(run.stdout.contains(&location), "{}", run.stdout);
}

#[test]
fn list_is_not_truncated_when_stdout_is_not_a_terminal() {
    let harness = Harness::new().with_file(".claude.json", CLAUDE_JSON);

    let run = harness.run(&["list"]);

    assert!(!run.stdout.contains('…'), "{}", run.stdout);
}

#[test]
fn non_ascii_paths_print_as_utf8() {
    let harness = Harness::in_folder("café-ñandú-日本").with_file(".claude.json", CLAUDE_JSON);
    let location = locator(&harness.path(".claude.json"), "mcpServers.linear");

    let text = harness.run(&["list"]);
    let json = harness.run(&["list", "--json"]);

    assert_eq!(text.code, 0, "{}", text.stderr);
    assert!(text.stdout.contains(&location), "{}", text.stdout);
    assert_eq!(
        json.document()["items"][1]["entries"][0]["locator"],
        location
    );
}
