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

    /// A file in the folder the binary runs in, which is its default Project.
    fn with_cwd_file(self, relative: &str, contents: &str) -> Self {
        let path = self.root.path().join(relative);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
        self
    }

    /// The real path of the folder the binary runs in.
    fn cwd(&self) -> PathBuf {
        real_path(self.root.path())
    }

    /// Where the binary's `%LOCALAPPDATA%` points.
    fn local_app_data(&self) -> PathBuf {
        self.root.path().join("AppData").join("Local")
    }

    /// Where the binary's `%APPDATA%` points.
    fn roaming_app_data(&self) -> PathBuf {
        self.root.path().join("AppData").join("Roaming")
    }

    /// The folder Operations are backed up and recorded in.
    fn backups(&self) -> PathBuf {
        self.local_app_data().join("config-digger").join("backups")
    }

    /// A `.claude.json` with user servers `linear` and `notes`, and a local
    /// `linear` keyed to the current folder in another spelling.
    fn with_local_linear(self) -> Self {
        let key = self
            .cwd()
            .display()
            .to_string()
            .replace('\\', "/")
            .to_lowercase();
        let claude_json = json!({
            "mcpServers": { "linear": { "url": "https://user" }, "notes": { "command": "n" } },
            "projects": { key: { "mcpServers": { "linear": { "url": "https://local" } } } }
        });
        self.with_file(".claude.json", &claude_json.to_string())
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
            // config-digger's data folders (backups, History, lock, and the
            // homes and Projects the user added or hid) stay in the temporary folder.
            .env("LOCALAPPDATA", self.local_app_data())
            .env("APPDATA", self.roaming_app_data())
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

/// A path with links resolved, as config-digger prints it: without `\\?\`.
fn real_path(path: &Path) -> PathBuf {
    let real = std::fs::canonicalize(path).unwrap();
    let text = real.display().to_string();
    PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(&text))
}

/// `(scope, state)` of each Entry of the Item named `name`.
fn entry_states(document: &Value, name: &str) -> Vec<(String, String)> {
    let item = document["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == name)
        .unwrap_or_else(|| panic!("no Item {name}: {document:#}"));
    item["entries"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| {
            (
                entry["scope"].as_str().unwrap().to_owned(),
                entry["state"].as_str().unwrap().to_owned(),
            )
        })
        .collect()
}

fn pairs(pairs: &[(&str, &str)]) -> Vec<(String, String)> {
    pairs
        .iter()
        .map(|(a, b)| ((*a).to_owned(), (*b).to_owned()))
        .collect()
}

#[test]
fn the_current_directory_is_the_default_project() {
    let harness = Harness::new().with_local_linear().with_cwd_file(
        ".mcp.json",
        r#"{"mcpServers": {"supabase": {"url": "https://p"}}}"#,
    );

    let run = harness.run(&["list", "--json"]);

    assert_eq!(run.code, 0, "{}", run.stderr);
    let document = run.document();
    assert_eq!(document["project"], harness.cwd().display().to_string());
    assert_eq!(
        entry_states(&document, "linear"),
        pairs(&[("local", "effective"), ("user", "shadowed")])
    );
    assert_eq!(
        entry_states(&document, "supabase"),
        pairs(&[("project", "effective")])
    );
}

#[test]
fn the_current_directory_is_evaluated_even_when_no_registry_knows_it() {
    let harness = Harness::new()
        .with_file(".claude.json", CLAUDE_JSON)
        .with_cwd_file(
            ".mcp.json",
            r#"{"mcpServers": {"linear": {"url": "https://p"}}}"#,
        );

    let run = harness.run(&["list", "--json"]);

    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(
        entry_states(&run.document(), "linear"),
        pairs(&[("project", "effective"), ("user", "shadowed")])
    );
}

#[test]
fn no_project_leaves_out_local_and_project_entries() {
    let harness = Harness::new().with_local_linear().with_cwd_file(
        ".mcp.json",
        r#"{"mcpServers": {"supabase": {"url": "https://p"}}}"#,
    );

    let run = harness.run(&["list", "--json", "--no-project"]);

    assert_eq!(run.code, 0, "{}", run.stderr);
    let document = run.document();
    assert_eq!(document["project"], Value::Null);
    assert_eq!(
        entry_states(&document, "linear"),
        pairs(&[("user", "effective")])
    );
    let names: Vec<&str> = document["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| item["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, ["linear", "notes"]);
}

#[test]
fn project_picks_another_folder() {
    let harness = Harness::new()
        .with_file(".claude.json", CLAUDE_JSON)
        .with_file(
            r"work\app\.mcp.json",
            r#"{"mcpServers": {"linear": {"url": "https://p"}}}"#,
        );
    let app = harness.path(r"work\app");

    let run = harness.run(&["list", "--json", "--project", app.to_str().unwrap()]);

    assert_eq!(run.code, 0, "{}", run.stderr);
    let document = run.document();
    assert_eq!(document["project"], real_path(&app).display().to_string());
    assert_eq!(
        entry_states(&document, "linear"),
        pairs(&[("project", "effective"), ("user", "shadowed")])
    );
}

#[test]
fn project_and_no_project_together_are_a_usage_error() {
    let run = Harness::new().run(&["list", "--project", ".", "--no-project"]);

    assert_eq!(run.code, 2);
}

#[test]
fn list_names_the_project_it_resolved_for() {
    let harness = Harness::new().with_file(".claude.json", CLAUDE_JSON);

    let in_project = harness.run(&["list"]);
    let without = harness.run(&["list", "--no-project"]);

    let cwd = harness.cwd().display().to_string();
    assert!(
        in_project.stdout.lines().next().unwrap().contains(&cwd),
        "{}",
        in_project.stdout
    );
    assert_eq!(without.stdout.lines().next(), Some("No Project"));
}

#[test]
fn show_lists_every_entry_in_precedence_order_with_its_reason() {
    let harness = Harness::new().with_local_linear();
    let claude_json = harness.path(".claude.json");

    let run = harness.run(&["show", "linear"]);

    assert_eq!(run.code, 0, "{}", run.stderr);
    let local = run
        .stdout
        .lines()
        .position(|line| line.contains("#projects.") && line.contains("Effective"))
        .unwrap_or_else(|| panic!("no local Entry:\n{}", run.stdout));
    let user_locator = locator(&claude_json, "mcpServers.linear");
    let user = run
        .stdout
        .lines()
        .position(|line| line.contains(&user_locator))
        .unwrap_or_else(|| panic!("no user Entry:\n{}", run.stdout));
    assert!(local < user, "{}", run.stdout);
    let user_line: Vec<&str> = run
        .stdout
        .lines()
        .nth(user)
        .unwrap()
        .split_whitespace()
        .collect();
    assert_eq!(
        user_line,
        ["2.", "user", "Shadowed", "file", user_locator.as_str()]
    );
    let reason = run.stdout.lines().nth(user + 1).unwrap();
    assert!(
        reason.trim().starts_with("Shadowed by the local Entry"),
        "{}",
        run.stdout
    );
}

#[test]
fn show_accepts_a_locator() {
    let harness = Harness::new().with_local_linear();
    let user_locator = locator(&harness.path(".claude.json"), "mcpServers.linear");

    let run = harness.run(&["show", &user_locator, "--json"]);

    assert_eq!(run.code, 0, "{}", run.stderr);
    let document = run.document();
    assert_eq!(document["schema_version"], 1);
    assert_eq!(document["item"]["name"], "linear");
    assert_eq!(document["item"]["entries"][1]["locator"], user_locator);
    assert_eq!(
        document["item"]["entries"][1]["reason"]["code"],
        "shadowed_by_precedence"
    );
    assert_eq!(
        document["item"]["entries"][1]["reason"]["effective"],
        document["item"]["entries"][0]["locator"]
    );
    assert_eq!(document["project"], harness.cwd().display().to_string());
}

#[test]
fn show_of_nothing_exits_3() {
    let harness = Harness::new().with_file(".claude.json", CLAUDE_JSON);

    let text = harness.run(&["show", "nothing"]);
    let json = harness.run(&["show", "nothing", "--json"]);

    assert_eq!(text.code, 3);
    assert!(text.stderr.contains("nothing"), "{}", text.stderr);
    assert_eq!(json.code, 3);
    assert_eq!(json.document()["error"]["code"], "no_match");
}

#[test]
fn list_accepts_a_selector() {
    let harness = Harness::new().with_file(".claude.json", CLAUDE_JSON);
    let linear = locator(&harness.path(".claude.json"), "mcpServers.linear");

    let by_locator = harness.run(&["list", &linear, "--json"]);
    let by_name = harness.run(&["list", "linear"]);
    let nothing = harness.run(&["list", "nothing"]);

    assert_eq!(by_locator.code, 0, "{}", by_locator.stderr);
    let items = by_locator.document()["items"].clone();
    assert_eq!(items.as_array().unwrap().len(), 1, "{items}");
    assert_eq!(items[0]["name"], "linear");
    assert!(by_name.stdout.contains(&linear), "{}", by_name.stdout);
    assert!(
        !by_name.stdout.contains("cd-throwaway-github"),
        "{}",
        by_name.stdout
    );
    assert_eq!(nothing.code, 3);
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
        json!([{
            "agent": "claude-code",
            "agent_name": "Claude Code",
            "home": "default",
            // As `%USERPROFILE%` spells it, like the locators of its roots.
            "folder": harness.home.display().to_string(),
            "source": {"kind": "default"},
            "hidden": false,
            "no_configuration": null,
            "scopes": ["local", "project", "user"]
        }])
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

/// User servers `linear` and `notes`.
const TWO_USER_SERVERS: &str = r#"{
  "mcpServers": {
    "linear": { "url": "https://u" },
    "notes": { "command": "notes-mcp" }
  }
}
"#;

fn read(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap()
}

#[test]
fn remove_without_a_tty_or_yes_prints_the_preview_and_exits_4() {
    let harness = Harness::new().with_file(".claude.json", TWO_USER_SERVERS);

    let run = harness.run(&["remove", "linear", "--no-project"]);

    assert_eq!(run.code, 4, "{}", run.stderr);
    assert!(run.stdout.contains("Effective"), "{}", run.stdout);
    assert!(run.stdout.contains("removed"), "{}", run.stdout);
    assert!(run.stderr.contains("--yes"), "{}", run.stderr);
    assert_eq!(read(&harness.path(".claude.json")), TWO_USER_SERVERS);
    assert!(!harness.backups().exists());
}

#[test]
fn remove_with_yes_prints_the_preview_then_writes_and_verifies() {
    let harness = Harness::new().with_file(".claude.json", TWO_USER_SERVERS);
    let claude_json = harness.path(".claude.json");

    let run = harness.run(&["remove", "linear", "--no-project", "--yes"]);

    assert_eq!(run.code, 0, "{}", run.stderr);
    let stdout = &run.stdout;
    assert!(
        stdout.contains(&locator(&claude_json, "mcpServers.linear")),
        "{stdout}"
    );
    assert!(
        stdout.contains(r#"-    "linear": { "url": "https://u" },"#),
        "{stdout}"
    );
    assert!(
        stdout.contains(&harness.backups().display().to_string()),
        "{stdout}"
    );
    assert!(stdout.contains("verified"), "{stdout}");
    assert_eq!(
        read(&claude_json),
        "{\n  \"mcpServers\": {\n    \"notes\": { \"command\": \"notes-mcp\" }\n  }\n}\n"
    );
}

#[test]
fn remove_json_returns_plan_warnings_and_result() {
    let harness = Harness::new().with_file(".claude.json", TWO_USER_SERVERS);

    let run = harness.run(&["remove", "linear", "--no-project", "--yes", "--json"]);

    assert_eq!(run.code, 0, "{}", run.stderr);
    let document = run.document();
    assert_eq!(document["schema_version"], 1);
    assert_eq!(document["plan"]["changes"][0]["after"], "removed");
    assert_eq!(document["warnings"], json!([]));
    assert_eq!(document["result"]["verify"], "verified");
    let operation = document["result"]["operation"].as_str().unwrap();
    let backup = harness.backups().join(operation);
    assert_eq!(document["result"]["backup"], backup.display().to_string());
    assert_eq!(read(&backup.join("1-.claude.json")), TWO_USER_SERVERS);
    let manifest: Value = serde_json::from_str(&read(&backup.join("manifest.json"))).unwrap();
    assert_eq!(manifest["status"], "applied");
}

#[test]
fn remove_dry_run_prints_the_plan_and_exits_0() {
    let harness = Harness::new().with_file(".claude.json", TWO_USER_SERVERS);

    let run = harness.run(&["remove", "linear", "--no-project", "--dry-run", "--json"]);

    assert_eq!(run.code, 0, "{}", run.stderr);
    let document = run.document();
    assert_eq!(document["plan"]["changes"][0]["before"], "effective");
    assert_eq!(document["result"], Value::Null);
    assert_eq!(read(&harness.path(".claude.json")), TWO_USER_SERVERS);
    assert!(!harness.backups().exists());
}

#[test]
fn remove_dry_run_in_text_says_nothing_was_written() {
    let harness = Harness::new().with_file(".claude.json", TWO_USER_SERVERS);

    let run = harness.run(&["remove", "linear", "--no-project", "--dry-run"]);

    assert_eq!(run.code, 0, "{}", run.stderr);
    assert!(run.stdout.contains("removed"), "{}", run.stdout);
    assert!(run.stdout.contains("Dry run"), "{}", run.stdout);
    assert_eq!(read(&harness.path(".claude.json")), TWO_USER_SERVERS);
}

#[test]
fn remove_json_without_yes_is_one_error_document_exiting_4() {
    let harness = Harness::new().with_file(".claude.json", TWO_USER_SERVERS);

    let run = harness.run(&["remove", "linear", "--no-project", "--json"]);

    assert_eq!(run.code, 4, "{}", run.stderr);
    let document = run.document();
    assert_eq!(document["error"]["code"], "confirmation_required");
    assert_eq!(read(&harness.path(".claude.json")), TWO_USER_SERVERS);
}

#[test]
fn remove_of_a_name_with_several_entries_exits_3_and_lists_their_locators() {
    let harness = Harness::new().with_local_linear();
    let claude_json = harness.path(".claude.json");

    let run = harness.run(&["remove", "linear", "--yes", "--json"]);

    assert_eq!(run.code, 3, "{}", run.stderr);
    let document = run.document();
    assert_eq!(document["error"]["code"], "ambiguous");
    let candidates = document["error"]["candidates"].as_array().unwrap();
    assert_eq!(candidates.len(), 2, "{document}");
    assert_eq!(candidates[1], locator(&claude_json, "mcpServers.linear"));
}

#[test]
fn remove_all_removes_every_entry_of_the_item_in_one_operation() {
    let harness = Harness::new().with_local_linear();

    let run = harness.run(&["remove", "linear", "--all", "--yes", "--json"]);

    assert_eq!(run.code, 0, "{}", run.stderr);
    let document = run.document();
    let afters: Vec<&str> = document["plan"]["changes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|change| change["after"].as_str().unwrap())
        .collect();
    assert_eq!(afters, ["removed", "removed"]);
    let left: Value = serde_json::from_str(&read(&harness.path(".claude.json"))).unwrap();
    assert_eq!(left["mcpServers"], json!({ "notes": { "command": "n" } }));
}

#[test]
fn remove_of_nothing_exits_3() {
    let harness = Harness::new().with_file(".claude.json", TWO_USER_SERVERS);

    let run = harness.run(&["remove", "nope", "--yes"]);

    assert_eq!(run.code, 3, "{}", run.stderr);
}

#[test]
fn remove_while_another_operation_holds_the_lock_exits_6() {
    let harness = Harness::new().with_file(".claude.json", TWO_USER_SERVERS);
    let data = harness.local_app_data().join("config-digger");
    std::fs::create_dir_all(&data).unwrap();
    let lock = std::fs::File::create(data.join("lock")).unwrap();
    lock.try_lock().unwrap();

    let run = harness.run(&["remove", "linear", "--no-project", "--yes", "--json"]);

    assert_eq!(run.code, 6, "{}", run.stderr);
    assert_eq!(run.document()["error"]["code"], "locked");
    assert_eq!(read(&harness.path(".claude.json")), TWO_USER_SERVERS);
}

impl Harness {
    /// Removes `selector` with `--yes` and returns the Operation's id.
    fn removed(&self, selector: &str) -> String {
        let run = self.run(&["remove", selector, "--no-project", "--yes", "--json"]);
        assert_eq!(run.code, 0, "{}", run.stderr);
        run.document()["result"]["operation"]
            .as_str()
            .unwrap()
            .to_owned()
    }
}

#[test]
fn history_lists_operations_newest_first() {
    let harness = Harness::new().with_file(".claude.json", TWO_USER_SERVERS);
    let first = harness.removed("linear");
    let second = harness.removed("notes");

    let json = harness.run(&["history", "--json"]);
    let text = harness.run(&["history"]);

    assert_eq!(json.code, 0, "{}", json.stderr);
    let document = json.document();
    assert_eq!(document["schema_version"], 1);
    let operations = document["operations"].as_array().unwrap();
    assert_eq!(operations.len(), 2);
    assert_eq!(operations[0]["id"], second.as_str());
    assert_eq!(operations[1]["id"], first.as_str());
    assert_eq!(operations[0]["title"], "Remove 1 Entry");
    assert_eq!(operations[0]["status_text"], "Applied");
    assert_eq!(operations[0]["undoable"], true);
    assert!(operations[0]["created_at"].as_str().unwrap().ends_with('Z'));
    assert_eq!(text.code, 0, "{}", text.stderr);
    let row = text
        .stdout
        .lines()
        .find(|line| line.contains(&second))
        .unwrap_or_else(|| panic!("no row for {second}:\n{}", text.stdout));
    let columns: Vec<&str> = row.split_whitespace().collect();
    assert_eq!(columns[0], second);
    assert_eq!(&columns[2..], ["Applied", "yes", "Remove", "1", "Entry"]);
}

#[test]
fn history_without_operations_says_so() {
    let run = Harness::new().run(&["history"]);

    assert_eq!(run.code, 0, "{}", run.stderr);
    assert!(run.stdout.contains("No Operations"), "{}", run.stdout);
}

#[test]
fn history_show_prints_the_edits_and_the_backed_up_files() {
    let harness = Harness::new().with_file(".claude.json", TWO_USER_SERVERS);
    let operation = harness.removed("linear");
    let backup = harness.backups().join(&operation).join("1-.claude.json");

    let text = harness.run(&["history", "show", &operation]);
    let json = harness.run(&["history", "show", &operation, "--json"]);

    assert_eq!(text.code, 0, "{}", text.stderr);
    assert!(
        text.stdout.contains("remove mcpServers.linear"),
        "{}",
        text.stdout
    );
    assert!(
        text.stdout.contains(&backup.display().to_string()),
        "{}",
        text.stdout
    );
    let document = json.document();
    assert_eq!(document["operation"]["summary"]["id"], operation.as_str());
    assert_eq!(
        document["operation"]["locations"][0]["backup"],
        backup.display().to_string()
    );
    assert_eq!(
        document["operation"]["locations"][0]["edits"][0]["key_path"],
        json!(["mcpServers", "linear"])
    );
}

#[test]
fn history_show_of_an_unknown_operation_exits_3() {
    let run = Harness::new().run(&["history", "show", "20261001-120000-abc", "--json"]);

    assert_eq!(run.code, 3, "{}", run.stderr);
    assert_eq!(run.document()["error"]["code"], "no_match");
}

#[test]
fn undo_with_yes_puts_the_entry_back_and_history_shows_it() {
    let harness = Harness::new().with_file(".claude.json", TWO_USER_SERVERS);
    let removed = harness.removed("linear");

    let run = harness.run(&["undo", &removed, "--yes"]);

    assert_eq!(run.code, 0, "{}", run.stderr);
    assert!(run.stdout.contains("removed"), "{}", run.stdout);
    assert!(run.stdout.contains("Effective"), "{}", run.stdout);
    assert!(
        run.stdout
            .contains(r#"+    "linear": { "url": "https://u" },"#),
        "{}",
        run.stdout
    );
    assert!(
        run.stdout.contains(&format!("Undid {removed}")),
        "{}",
        run.stdout
    );
    assert_eq!(read(&harness.path(".claude.json")), TWO_USER_SERVERS);
    let history = harness.run(&["history", "--json"]).document();
    let undo = history["operations"][0]["id"].as_str().unwrap();
    assert_eq!(
        history["operations"][0]["status_text"],
        format!("Undo of {removed}")
    );
    assert_eq!(
        history["operations"][1]["status_text"],
        format!("Undone by {undo}")
    );
    assert_eq!(history["operations"][1]["undoable"], false);
}

#[test]
fn undo_without_a_tty_or_yes_prints_the_preview_and_exits_4() {
    let harness = Harness::new().with_file(".claude.json", TWO_USER_SERVERS);
    let removed = harness.removed("linear");
    let after_remove = read(&harness.path(".claude.json"));

    let run = harness.run(&["undo", &removed]);

    assert_eq!(run.code, 4, "{}", run.stderr);
    assert!(run.stdout.contains("Undo"), "{}", run.stdout);
    assert_eq!(read(&harness.path(".claude.json")), after_remove);
}

#[test]
fn undo_dry_run_prints_the_plan_and_writes_nothing() {
    let harness = Harness::new().with_file(".claude.json", TWO_USER_SERVERS);
    let removed = harness.removed("linear");
    let after_remove = read(&harness.path(".claude.json"));

    let run = harness.run(&["undo", &removed, "--dry-run", "--json"]);

    assert_eq!(run.code, 0, "{}", run.stderr);
    let document = run.document();
    assert_eq!(document["plan"]["action"], "undo");
    assert_eq!(document["plan"]["undo_of"], removed.as_str());
    assert_eq!(document["plan"]["changes"][0]["before"], "removed");
    assert_eq!(document["result"], Value::Null);
    assert_eq!(read(&harness.path(".claude.json")), after_remove);
}

#[test]
fn undo_refused_because_the_entry_changed_exits_8_with_the_backup_and_a_diff() {
    let harness = Harness::new().with_file(".claude.json", TWO_USER_SERVERS);
    let removed = harness.removed("linear");
    let undo = harness.run(&["undo", &removed, "--yes", "--json"]);
    let undo = undo.document()["result"]["operation"]
        .as_str()
        .unwrap()
        .to_owned();
    let rotated = TWO_USER_SERVERS.replace("https://u", "https://rotated");
    std::fs::write(harness.path(".claude.json"), &rotated).unwrap();

    let text = harness.run(&["undo", &undo, "--yes"]);
    let json = harness.run(&["undo", &undo, "--yes", "--json"]);

    let backup = harness.backups().join(&undo).display().to_string();
    assert_eq!(text.code, 8, "{}", text.stderr);
    assert!(text.stderr.contains(&backup), "{}", text.stderr);
    assert!(
        text.stderr
            .contains(r#"+    "linear": { "url": "https://rotated" },"#),
        "{}",
        text.stderr
    );
    assert_eq!(json.code, 8);
    let error = &json.document()["error"];
    assert_eq!(error["code"], "undo_refused");
    assert_eq!(error["backup"], backup);
    assert!(
        error["diffs"][0]["diff"]
            .as_str()
            .unwrap()
            .contains("rotated"),
        "{error}"
    );
    assert_eq!(read(&harness.path(".claude.json")), rotated);
}

#[test]
fn undo_of_a_remove_exits_5_when_a_same_named_entry_appeared() {
    let harness = Harness::new().with_file(".claude.json", TWO_USER_SERVERS);
    let removed = harness.removed("linear");
    let readded = TWO_USER_SERVERS.replace("https://u", "https://readded");
    std::fs::write(harness.path(".claude.json"), &readded).unwrap();

    let run = harness.run(&["undo", &removed, "--yes", "--json"]);

    assert_eq!(run.code, 5, "{}", run.stderr);
    assert_eq!(run.document()["error"]["code"], "blocked");
    assert_eq!(read(&harness.path(".claude.json")), readded);
}

#[test]
fn undo_of_an_unknown_operation_exits_3() {
    let run = Harness::new().run(&["undo", "20261001-120000-abc", "--yes"]);

    assert_eq!(run.code, 3, "{}", run.stderr);
}

/// Makes a file read-only, or writable again.
fn set_read_only(path: &Path, read_only: bool) {
    let mut permissions = std::fs::metadata(path).unwrap().permissions();
    permissions.set_readonly(read_only);
    std::fs::set_permissions(path, permissions).unwrap();
}

#[test]
fn a_write_that_fails_exits_9_and_history_shows_it_failed_and_rolled_back() {
    let harness = Harness::new().with_file(".claude.json", TWO_USER_SERVERS);
    let claude_json = harness.path(".claude.json");
    // A read-only file cannot be replaced, so the write fails.
    set_read_only(&claude_json, true);

    let json = harness.run(&["remove", "linear", "--no-project", "--yes", "--json"]);
    let text = harness.run(&["remove", "linear", "--no-project", "--yes"]);
    let history = harness.run(&["history"]);
    set_read_only(&claude_json, false);

    assert_eq!(json.code, 9, "{}", json.stderr);
    let error = &json.document()["error"];
    assert_eq!(error["code"], "write_failed");
    let operation = error["operation"].as_str().unwrap();
    assert_eq!(
        error["backup"],
        harness.backups().join(operation).display().to_string()
    );
    assert_eq!(error["rollback"][0]["outcome"], "not_written");
    assert_eq!(
        error["rollback"][0]["path"],
        claude_json.display().to_string()
    );
    assert_eq!(text.code, 9, "{}", text.stderr);
    assert!(text.stderr.contains("rolled back"), "{}", text.stderr);
    assert!(text.stderr.contains("not written"), "{}", text.stderr);
    assert_eq!(read(&claude_json), TWO_USER_SERVERS);
    assert!(
        history.stdout.contains("Failed, rolled back"),
        "{}",
        history.stdout
    );
    assert!(
        history.stdout.contains("Cause: cannot write"),
        "{}",
        history.stdout
    );
}

#[test]
fn dismissing_an_operation_that_does_not_need_attention_exits_5() {
    let harness = Harness::new().with_file(".claude.json", TWO_USER_SERVERS);
    let removed = harness.removed("linear");

    let run = harness.run(&["history", "dismiss", &removed, "--json"]);

    assert_eq!(run.code, 5, "{}", run.stderr);
    assert_eq!(run.document()["error"]["code"], "blocked");
}

#[test]
fn dismissing_an_unknown_operation_exits_3() {
    let run = Harness::new().run(&["history", "dismiss", "20261001-120000-abc"]);

    assert_eq!(run.code, 3, "{}", run.stderr);
}

impl Harness {
    /// The copy of `.claude.json` in the backup of the Operation `id`.
    fn backup_copy(&self, id: &str) -> PathBuf {
        self.backups().join(id).join("1-.claude.json")
    }

    /// Marks the Operation `id` as needing attention, as an incomplete
    /// Rollback would, which the real file system cannot be made to cause.
    fn needing_attention(&self, id: &str) {
        let manifest = self.backups().join(id).join("manifest.json");
        let mut recorded: Value = serde_json::from_str(&read(&manifest)).unwrap();
        recorded["status"] = json!("needs_attention");
        std::fs::write(&manifest, recorded.to_string()).unwrap();
    }
}

#[test]
fn delete_backup_without_a_tty_or_yes_prints_the_preview_and_exits_4() {
    let harness = Harness::new().with_file(".claude.json", TWO_USER_SERVERS);
    let removed = harness.removed("linear");
    let copy = harness.backup_copy(&removed);

    let run = harness.run(&["history", "delete-backup", &removed]);

    assert_eq!(run.code, 4, "{}", run.stderr);
    assert!(
        run.stdout
            .contains(&format!("Delete the backup of {removed}")),
        "{}",
        run.stdout
    );
    assert!(
        run.stdout.contains(&copy.display().to_string()),
        "{}",
        run.stdout
    );
    assert!(copy.exists());
}

#[test]
fn delete_backup_with_yes_deletes_the_copies_and_history_marks_it_not_undoable() {
    let harness = Harness::new().with_file(".claude.json", TWO_USER_SERVERS);
    let removed = harness.removed("linear");

    let run = harness.run(&["history", "delete-backup", &removed, "--yes"]);
    let json = harness.run(&["history", "--json"]).document();
    let text = harness.run(&["history"]);
    let undo = harness.run(&["undo", &removed, "--yes", "--json"]);

    assert_eq!(run.code, 0, "{}", run.stderr);
    assert!(
        run.stdout
            .contains(&format!("Deleted the backup of {removed}")),
        "{}",
        run.stdout
    );
    assert!(!harness.backup_copy(&removed).exists());
    assert_eq!(json["operations"][0]["backup_deleted"], true);
    assert!(
        text.stdout
            .contains("Applied, backup deleted, not undoable"),
        "{}",
        text.stdout
    );
    assert_eq!(undo.code, 5, "{}", undo.stderr);
    assert_eq!(undo.document()["error"]["code"], "blocked");
}

#[test]
fn delete_backup_json_returns_the_plan_and_the_result() {
    let harness = Harness::new().with_file(".claude.json", TWO_USER_SERVERS);
    let removed = harness.removed("linear");
    let copy = harness.backup_copy(&removed).display().to_string();

    let dry_run = harness.run(&["history", "delete-backup", &removed, "--dry-run", "--json"]);
    let copy_after_dry_run = harness.backup_copy(&removed).exists();
    let run = harness.run(&["history", "delete-backup", &removed, "--yes", "--json"]);

    assert_eq!(dry_run.code, 0, "{}", dry_run.stderr);
    let plan = dry_run.document();
    assert_eq!(plan["plan"]["action"], "delete_backup");
    assert_eq!(plan["plan"]["backup_of"], removed.as_str());
    assert_eq!(plan["plan"]["deleted"], json!([copy]));
    assert_eq!(plan["warnings"], json!([]));
    assert_eq!(plan["result"], Value::Null);
    assert!(copy_after_dry_run);
    assert_eq!(run.code, 0, "{}", run.stderr);
    let document = run.document();
    assert_eq!(document["schema_version"], 1);
    assert_eq!(document["result"]["operation"], removed.as_str());
    assert_eq!(document["result"]["verify"], "verified");
}

#[test]
fn delete_backup_of_an_unknown_operation_exits_3() {
    let run = Harness::new().run(&["history", "delete-backup", "20261001-120000-abc", "--yes"]);

    assert_eq!(run.code, 3, "{}", run.stderr);
}

#[test]
fn delete_backup_of_an_operation_that_needs_attention_needs_its_own_ack() {
    let harness = Harness::new().with_file(".claude.json", TWO_USER_SERVERS);
    let removed = harness.removed("linear");
    harness.needing_attention(&removed);

    let yes = harness.run(&["history", "delete-backup", &removed, "--yes", "--json"]);
    let copy_after_yes = harness.backup_copy(&removed).exists();
    let text = harness.run(&["history", "delete-backup", &removed, "--yes"]);
    let acked = harness.run(&[
        "history",
        "delete-backup",
        &removed,
        "--yes",
        "--ack",
        "needs-attention",
    ]);
    let history = harness.run(&["history", "--json"]).document();

    assert_eq!(yes.code, 4, "{}", yes.stderr);
    let error = &yes.document()["error"];
    assert_eq!(error["code"], "ack_required");
    assert!(
        error["message"]
            .as_str()
            .unwrap()
            .contains("--ack needs-attention"),
        "{error}"
    );
    assert!(copy_after_yes);
    assert!(
        text.stdout.contains("only copy for manual recovery"),
        "{}",
        text.stdout
    );
    assert_eq!(acked.code, 0, "{}", acked.stderr);
    assert!(!harness.backup_copy(&removed).exists());
    assert_eq!(history["operations"][0]["needs_attention"], false);
}

// Secrets.

/// The fake Secret, the only one fixtures hold.
const SECRET: &str = "not-a-real-secret";

/// A user server `github` whose token is a Secret, next to `linear`.
const SERVERS_WITH_A_SECRET: &str = r#"{
  "mcpServers": {
    "github": {
      "command": "npx",
      "env": {
        "GITHUB_TOKEN": "ghp_not-a-real-secret"
      }
    },
    "linear": {
      "url": "https://mcp.linear.app/mcp"
    }
  }
}
"#;

#[test]
fn show_prints_the_entry_configuration_with_its_secrets_masked() {
    let harness = Harness::new().with_file(".claude.json", SERVERS_WITH_A_SECRET);

    let text = harness.run(&["--no-project", "show", "github"]);
    let json = harness.run(&["--no-project", "show", "github", "--json"]);

    assert_eq!(text.code, 0, "{}", text.stderr);
    assert!(
        text.stdout
            .contains(r#"{"command":"npx","env":{"GITHUB_TOKEN":"ghp_••••"}}"#),
        "{}",
        text.stdout
    );
    assert!(!text.stdout.contains(SECRET), "{}", text.stdout);
    let entry = &json.document()["item"]["entries"][0];
    assert_eq!(entry["config"]["env"]["GITHUB_TOKEN"], "ghp_••••");
    assert_eq!(entry["masked"], json!([["env", "GITHUB_TOKEN"]]));
    assert!(!json.stdout.contains(SECRET), "{}", json.stdout);
}

#[test]
fn a_remove_preview_masks_secrets_in_its_diff() {
    let harness = Harness::new().with_file(".claude.json", SERVERS_WITH_A_SECRET);

    let text = harness.run(&["--no-project", "remove", "github", "--dry-run"]);
    let json = harness.run(&["--no-project", "remove", "github", "--dry-run", "--json"]);

    assert_eq!(text.code, 0, "{}", text.stderr);
    assert!(
        text.stdout
            .contains(r#"-        "GITHUB_TOKEN": "ghp_••••""#),
        "{}",
        text.stdout
    );
    assert!(!text.stdout.contains(SECRET), "{}", text.stdout);
    let file = &json.document()["plan"]["files"][0];
    assert!(
        file["diff"].as_str().unwrap().contains("ghp_••••"),
        "{file}"
    );
    assert_eq!(file["diff_unavailable"], Value::Null);
    assert!(!json.stdout.contains(SECRET), "{}", json.stdout);
}

#[test]
fn a_refused_undo_masks_secrets_in_its_diffs() {
    let harness = Harness::new().with_file(".claude.json", SERVERS_WITH_A_SECRET);
    let removed = harness.removed("github");
    let undo = harness.run(&["--no-project", "undo", &removed, "--yes", "--json"]);
    let undo = undo.document()["result"]["operation"]
        .as_str()
        .unwrap()
        .to_owned();
    let rotated = read(&harness.path(".claude.json")).replace(SECRET, "not-a-real-secret-2");
    std::fs::write(harness.path(".claude.json"), &rotated).unwrap();

    let text = harness.run(&["undo", &undo, "--yes"]);
    let json = harness.run(&["undo", &undo, "--yes", "--json"]);

    assert_eq!(text.code, 8, "{}", text.stderr);
    assert!(
        text.stderr
            .contains(r#"+        "GITHUB_TOKEN": "ghp_••••""#),
        "{}",
        text.stderr
    );
    assert!(!text.stderr.contains(SECRET), "{}", text.stderr);
    let diff = &json.document()["error"]["diffs"][0];
    assert!(
        diff["diff"].as_str().unwrap().contains("ghp_••••"),
        "{diff}"
    );
    assert!(!json.stdout.contains(SECRET), "{}", json.stdout);
}

/// Runs git in the folder the binary runs in.
fn git(harness: &Harness, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(harness.root.path())
        .args([
            "-c",
            "user.name=fixture",
            "-c",
            "user.email=fixture@example.com",
        ])
        .args(["-c", "commit.gpgsign=false"])
        .args(args)
        .output()
        .expect("git runs");
    assert!(output.status.success(), "git {args:?} failed");
    String::from_utf8(output.stdout).unwrap()
}

/// The folder the binary runs in as a git repo, with `committed` (paths
/// relative to it) in its one commit.
fn in_git_repo(harness: &Harness, committed: &[&str]) {
    git(harness, &["init", "--quiet"]);
    git(harness, &[&["add", "--"][..], committed].concat());
    git(
        harness,
        &["commit", "--quiet", "--allow-empty", "-m", "fixture"],
    );
}

/// User `notes`, and `linear` in a `.mcp.json` of the current folder.
fn versioned_fixture() -> Harness {
    Harness::new()
        .with_file(
            ".claude.json",
            r#"{"mcpServers": {"notes": {"command": "notes-mcp"}}}"#,
        )
        .with_cwd_file(
            ".mcp.json",
            "{\n  \"mcpServers\": {\n    \"linear\": { \"url\": \"https://mcp.linear.app/mcp\" }\n  }\n}\n",
        )
}

#[test]
fn list_and_show_tag_an_entry_in_a_versioned_file() {
    let harness = versioned_fixture();
    in_git_repo(&harness, &[".mcp.json"]);

    let list = harness.run(&["list", "--wide"]);
    let show = harness.run(&["show", "linear", "--wide"]);
    let json = harness.run(&["show", "linear", "--json"]);

    assert_eq!(list.code, 0, "{}", list.stderr);
    let line = |out: &str, name: &str| {
        out.lines()
            .find(|line| line.contains(name))
            .unwrap_or_else(|| panic!("no line for {name}:\n{out}"))
            .to_owned()
    };
    assert!(
        line(&list.stdout, "linear").ends_with("[git-versioned]"),
        "{}",
        list.stdout
    );
    assert!(
        !line(&list.stdout, "notes").contains("git-versioned"),
        "{}",
        list.stdout
    );
    assert!(
        line(&show.stdout, ".mcp.json").ends_with("[git-versioned]"),
        "{}",
        show.stdout
    );
    assert_eq!(json.document()["item"]["entries"][0]["git_versioned"], true);
}

#[test]
fn an_untracked_file_is_not_tagged() {
    let harness = versioned_fixture();
    in_git_repo(&harness, &[]);

    let json = harness.run(&["show", "linear", "--json"]);

    assert_eq!(
        json.document()["item"]["entries"][0]["git_versioned"],
        false
    );
}

#[test]
fn remove_in_a_versioned_file_without_a_tty_needs_ack_git_versioned() {
    let harness = versioned_fixture();
    in_git_repo(&harness, &[".mcp.json"]);
    let before = read(&harness.cwd().join(".mcp.json"));

    let run = harness.run(&["remove", "linear", "--yes"]);

    assert_eq!(run.code, 4, "{}", run.stderr);
    assert!(run.stdout.contains("[git-versioned]"), "{}", run.stdout);
    assert!(run.stdout.contains("git diff"), "{}", run.stdout);
    assert!(run.stderr.contains("--ack git-versioned"), "{}", run.stderr);
    assert_eq!(read(&harness.cwd().join(".mcp.json")), before);
}

#[test]
fn remove_in_a_versioned_file_never_stages_or_commits() {
    let harness = versioned_fixture();
    in_git_repo(&harness, &[".mcp.json"]);

    let run = harness.run(&[
        "remove",
        "linear",
        "--yes",
        "--ack",
        "git-versioned",
        "--json",
    ]);

    assert_eq!(run.code, 0, "{}", run.stderr);
    let document = run.document();
    assert_eq!(document["warnings"][0]["class"], "git-versioned");
    assert_eq!(document["warnings"][0]["acknowledged"], true);
    assert_eq!(document["plan"]["files"][0]["git_versioned"], true);
    assert!(!read(&harness.cwd().join(".mcp.json")).contains("linear"));
    // The removal shows in the git diff, unstaged, and nothing was committed.
    assert_eq!(git(&harness, &["diff", "--name-only"]), ".mcp.json\n");
    assert_eq!(git(&harness, &["diff", "--cached", "--name-only"]), "");
    assert_eq!(git(&harness, &["rev-list", "--count", "HEAD"]), "1\n");
}

#[test]
fn disable_where_every_switch_is_versioned_exits_5_with_the_reason() {
    // A home folder kept in the repo, as a dotfiles repo would.
    let harness = versioned_fixture();
    in_git_repo(&harness, &[".mcp.json", "home/.claude.json"]);

    let show = harness.run(&["show", "notes", "--wide"]);
    let json = harness.run(&["show", "notes", "--json"]);
    let disable = harness.run(&["disable", "notes", "--yes", "--json"]);

    assert!(
        show.stdout.contains("Disable is not offered"),
        "{}",
        show.stdout
    );
    let unavailable = &json.document()["item"]["entries"][0]["disable"];
    assert_eq!(unavailable["status"], "not_offered");
    assert_eq!(unavailable["code"], "git_versioned");
    assert_eq!(disable.code, 5, "{}", disable.stdout);
    assert_eq!(disable.document()["error"]["code"], "blocked");
    assert!(
        disable.document()["error"]["message"]
            .as_str()
            .unwrap()
            .contains(unavailable["sentence"].as_str().unwrap()),
        "{}",
        disable.stdout
    );
}

#[test]
fn show_names_the_machine_local_switch_disable_would_write() {
    let harness = versioned_fixture();
    // Claude Code keys its switch by the Project as it recorded it.
    let claude_json_text = json!({
        "mcpServers": { "notes": { "command": "notes-mcp" } },
        "projects": { project_key(&harness): {} }
    });
    std::fs::write(harness.path(".claude.json"), claude_json_text.to_string()).unwrap();
    in_git_repo(&harness, &[".mcp.json"]);

    let show = harness.run(&["show", "linear", "--wide"]);

    let claude_json = harness.path(".claude.json").display().to_string();
    assert!(
        show.stdout.contains(&format!(
            "Disable adds `linear` to `disabledMcpServers` in {claude_json} for this Project."
        )),
        "{}",
        show.stdout
    );
}

#[test]
fn show_falls_back_on_the_local_settings_while_claude_code_has_not_recorded_the_project() {
    let harness = versioned_fixture();
    in_git_repo(&harness, &[".mcp.json"]);

    let show = harness.run(&["show", "linear", "--wide"]);

    let settings = harness.cwd().join(".claude").join("settings.local.json");
    assert!(
        show.stdout.contains(&format!(
            "Disable adds `linear` to `disabledMcpjsonServers` in {} for this Project.",
            settings.display()
        )),
        "{}",
        show.stdout
    );
}

/// A `.claude.json` with a user `notes`, while Claude Code runs: a session
/// file names this test's own PID, which is alive while it runs.
fn claude_code_running() -> Harness {
    let session = format!(".claude/sessions/{}.json", std::process::id());
    Harness::new()
        .with_file(
            ".claude.json",
            &json!({ "mcpServers": { "notes": { "command": "n" } } }).to_string(),
        )
        .with_file(&session, "{}")
}

#[test]
fn remove_while_the_agent_runs_without_a_tty_needs_ack_running_agent() {
    let harness = claude_code_running();
    let before = read(&harness.path(".claude.json"));

    let run = harness.run(&["remove", "notes", "--yes"]);

    assert_eq!(run.code, 4, "{}", run.stderr);
    assert!(run.stdout.contains("[running-agent]"), "{}", run.stdout);
    assert!(
        run.stdout
            .contains("may overwrite what this Operation writes"),
        "{}",
        run.stdout
    );
    assert!(run.stderr.contains("--ack running-agent"), "{}", run.stderr);
    assert_eq!(read(&harness.path(".claude.json")), before);
}

#[test]
fn remove_while_the_agent_runs_with_ack_prints_a_restart_hint() {
    let harness = claude_code_running();

    let run = harness.run(&["remove", "notes", "--yes", "--ack", "running-agent"]);

    assert_eq!(run.code, 0, "{}", run.stderr);
    assert!(
        run.stdout
            .contains("Restart Claude Code so that it reads what the Operation wrote."),
        "{}",
        run.stdout
    );
    assert!(
        run.stdout
            .contains("may be overwritten while Claude Code runs."),
        "{}",
        run.stdout
    );
    assert!(!read(&harness.path(".claude.json")).contains("notes"));
}

#[test]
fn remove_while_the_agent_runs_json_has_the_warning_and_the_hints() {
    let harness = claude_code_running();

    let run = harness.run(&[
        "remove",
        "notes",
        "--yes",
        "--ack",
        "running-agent",
        "--json",
    ]);

    assert_eq!(run.code, 0, "{}", run.stderr);
    let document = run.document();
    assert_eq!(document["blocks"], json!([]));
    assert_eq!(document["warnings"][0]["class"], "running-agent");
    assert_eq!(document["warnings"][0]["acknowledged"], true);
    let hints = document["result"]["hints"].as_array().unwrap();
    assert_eq!(hints.len(), 2, "{hints:?}");
    assert_eq!(
        hints[0],
        "Restart Claude Code so that it reads what the Operation wrote."
    );
}

/// The key `with_local_linear` records the current folder under, as Claude
/// Code spelled it.
fn project_key(harness: &Harness) -> String {
    harness
        .cwd()
        .display()
        .to_string()
        .replace('\\', "/")
        .to_lowercase()
}

fn claude_json_value(harness: &Harness) -> Value {
    serde_json::from_str(&read(&harness.path(".claude.json"))).unwrap()
}

#[test]
fn disable_then_enable_round_trips_through_the_switch_claude_code_writes() {
    let harness = Harness::new().with_local_linear();
    let key = project_key(&harness);

    let disable = harness.run(&["disable", "notes", "--yes"]);
    assert_eq!(disable.code, 0, "{}\n{}", disable.stdout, disable.stderr);
    assert!(
        disable.stdout.contains("Disabled 1 Entry"),
        "{}",
        disable.stdout
    );
    assert_eq!(
        claude_json_value(&harness)["projects"][&key]["disabledMcpServers"],
        json!(["notes"])
    );
    let list = harness.run(&["list", "--json"]).document();
    assert_eq!(entry_states(&list, "notes"), pairs(&[("user", "disabled")]));
    let show = harness.run(&["show", "notes", "--wide"]);
    assert!(show.stdout.contains("Disabled by name"), "{}", show.stdout);
    assert!(
        show.stdout
            .contains("Disabled: would be Effective once Enabled."),
        "{}",
        show.stdout
    );
    assert!(
        show.stdout
            .contains("Enable removes `notes` from `disabledMcpServers`"),
        "{}",
        show.stdout
    );

    let enable = harness.run(&["enable", "notes", "--yes"]);
    assert_eq!(enable.code, 0, "{}\n{}", enable.stdout, enable.stderr);
    assert!(
        enable.stdout.contains("Enabled 1 Entry"),
        "{}",
        enable.stdout
    );
    assert_eq!(
        claude_json_value(&harness)["projects"][&key]["disabledMcpServers"],
        json!([])
    );
    let list = harness.run(&["list", "--json"]).document();
    assert_eq!(
        entry_states(&list, "notes"),
        pairs(&[("user", "effective")])
    );
    let history = harness.run(&["history", "--json"]).document();
    let titles: Vec<&str> = history["operations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|operation| operation["title"].as_str().unwrap())
        .collect();
    assert_eq!(titles, ["Enable 1 Entry", "Disable 1 Entry"]);
}

#[test]
fn disable_by_name_of_one_entry_without_a_tty_needs_ack_wide_reach() {
    let harness = Harness::new().with_local_linear();
    let local = locator(
        &harness.path(".claude.json"),
        &format!("projects.\"{}\".mcpServers.linear", project_key(&harness)),
    );
    let before = read(&harness.path(".claude.json"));

    let refused = harness.run(&["disable", &local, "--yes"]);

    assert_eq!(refused.code, 4, "{}", refused.stderr);
    assert!(
        refused.stdout.contains("[wide-reach]"),
        "{}",
        refused.stdout
    );
    let user = locator(&harness.path(".claude.json"), "mcpServers.linear");
    assert!(refused.stdout.contains(&user), "{}", refused.stdout);
    assert!(
        refused.stderr.contains("--ack wide-reach"),
        "{}",
        refused.stderr
    );
    assert_eq!(read(&harness.path(".claude.json")), before);

    let run = harness.run(&["disable", &local, "--yes", "--ack", "wide-reach"]);
    assert_eq!(run.code, 0, "{}", run.stderr);
    let list = harness.run(&["list", "--json"]).document();
    assert_eq!(
        entry_states(&list, "linear"),
        pairs(&[("local", "disabled"), ("user", "disabled")])
    );
}

#[test]
fn enable_of_an_entry_that_is_not_disabled_exits_5() {
    let harness = Harness::new().with_local_linear();

    let run = harness.run(&["enable", "notes", "--yes", "--json"]);

    assert_eq!(run.code, 5, "{}", run.stdout);
    assert_eq!(run.document()["error"]["code"], "blocked");
}

#[test]
fn list_and_show_print_read_failures_with_their_position_to_stderr_and_exit_0() {
    let harness = Harness::new()
        .with_file(
            ".claude.json",
            r#"{"mcpServers": {"linear": {"url": "https://user"}}}"#,
        )
        .with_cwd_file(".mcp.json", "{\n  \"mcpServers\": {,\n}\n");
    let mcp_json = harness.cwd().join(".mcp.json");

    let list = harness.run(&["list"]);
    let show = harness.run(&["show", "linear"]);
    let json = harness.run(&["list", "--json"]);

    for run in [&list, &show, &json] {
        assert_eq!(run.code, 0, "{}", run.stderr);
        assert!(
            run.stderr.contains(&format!(
                "Claude Code cannot parse {} as strict JSON (line 2, column 18)",
                mcp_json.display()
            )),
            "{}",
            run.stderr
        );
    }
    let failure = &json.document()["read_failures"][0];
    assert_eq!(failure["path"], json!(mcp_json));
    assert_eq!(failure["agent"], "claude-code");
    assert_eq!(failure["project"], json!(harness.cwd()));
    assert_eq!(failure["kind"], "parse");
    assert_eq!(failure["position"], json!({ "line": 2, "column": 18 }));
    // Every Item in the context could be beaten by a server in the file.
    assert!(show.stdout.contains("couldn't be read"), "{}", show.stdout);
}

#[test]
fn a_write_whose_outcome_a_read_failure_could_change_previews_an_info_row() {
    let harness = Harness::new()
        .with_file(
            ".claude.json",
            r#"{"mcpServers": {"linear": {"url": "https://user"}}}"#,
        )
        .with_cwd_file(".mcp.json", "{ not json");

    let text = harness.run(&["remove", "linear", "--dry-run"]);
    let json = harness.run(&["remove", "linear", "--dry-run", "--json"]);

    assert_eq!(text.code, 0, "{}", text.stderr);
    assert!(
        text.stdout.contains("States after this change may differ"),
        "{}",
        text.stdout
    );
    assert_eq!(json.code, 0, "{}", json.stderr);
    assert_eq!(json.document()["plan"]["info"][0]["code"], "read_failure");
    assert_eq!(json.document()["warnings"], json!([]));
}

#[test]
fn a_disable_that_must_write_into_read_failures_exits_5() {
    let harness = Harness::new()
        .with_file(".claude.json", "{ not json")
        .with_cwd_file(
            ".mcp.json",
            r#"{"mcpServers": {"github": {"command": "gh"}}}"#,
        )
        .with_cwd_file(".claude/settings.local.json", "{ not json");

    let run = harness.run(&["disable", "github", "--yes", "--json"]);

    assert_eq!(run.code, 5, "{}", run.stderr);
    assert_eq!(run.document()["error"]["code"], "blocked");
}

impl Harness {
    /// A folder next to the home, created empty, by its real path.
    fn folder(&self, name: &str) -> PathBuf {
        let folder = self.root.path().join(name);
        std::fs::create_dir_all(&folder).unwrap();
        real_path(&folder)
    }

    /// The list of homes and Projects the user added or hid.
    fn user_list(&self) -> Value {
        let file = self
            .roaming_app_data()
            .join("config-digger")
            .join("homes-and-projects.json");
        serde_json::from_slice(&std::fs::read(file).unwrap()).unwrap()
    }
}

/// `(home, source kind, hidden)` of each home `homes list --json` lists.
fn listed_homes(document: &Value) -> Vec<(String, String, bool)> {
    document["homes"]
        .as_array()
        .unwrap()
        .iter()
        .map(|home| {
            (
                home["home"].as_str().unwrap().to_owned(),
                home["source"]["kind"].as_str().unwrap().to_owned(),
                home["hidden"].as_bool().unwrap(),
            )
        })
        .collect()
}

/// The paths `projects list --json` lists.
fn listed_projects(document: &Value) -> Vec<String> {
    document["projects"]
        .as_array()
        .unwrap()
        .iter()
        .map(|project| project["path"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn homes_add_reads_an_existing_folder_as_an_agent_home() {
    let harness = Harness::new().with_file(".claude.json", CLAUDE_JSON);
    let alt = harness.folder("alt");
    std::fs::write(
        alt.join(".claude.json"),
        r#"{"mcpServers": {"notes": {"command": "notes"}}}"#,
    )
    .unwrap();
    let alt = alt.display().to_string();
    let empty = harness.folder("empty").display().to_string();

    let added = harness.run(&["homes", "add", &alt, "--agent", "claude-code", "--json"]);
    let without_files = harness.run(&["homes", "add", &empty, "--agent", "claude-code"]);
    let list = harness.run(&["homes", "list", "--json"]);
    let text = harness.run(&["homes", "list", "--wide"]);
    let items = harness.run(&["list", "--no-project", "--json"]);

    assert_eq!(added.code, 0, "{}", added.stderr);
    assert_eq!(added.document()["home"]["source"]["kind"], "added");
    assert_eq!(without_files.code, 0, "{}", without_files.stderr);
    assert!(
        without_files
            .stdout
            .contains("No configuration found for Claude Code."),
        "{}",
        without_files.stdout
    );
    assert_eq!(
        listed_homes(&list.document()),
        vec![
            ("default".to_owned(), "default".to_owned(), false),
            (alt.clone(), "added".to_owned(), false),
            (empty.clone(), "added".to_owned(), false),
        ]
    );
    assert!(
        text.stdout.contains("definition default"),
        "{}",
        text.stdout
    );
    assert!(text.stdout.contains("added by you"), "{}", text.stdout);
    let notes_home = items.document()["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["name"] == "notes")
        .map(|item| item["home"].clone());
    assert_eq!(notes_home, Some(json!(alt)));
}

#[test]
fn homes_add_needs_an_existing_folder_and_a_known_agent() {
    let harness = Harness::new().with_file(".claude.json", CLAUDE_JSON);
    let missing = harness.root.path().join("missing").display().to_string();
    let alt = harness.folder("alt").display().to_string();

    let no_folder = harness.run(&["homes", "add", &missing, "--agent", "claude-code", "--json"]);
    let no_agent = harness.run(&["homes", "add", &alt, "--agent", "zed", "--json"]);
    let no_agent_flag = harness.run(&["homes", "add", &alt]);

    assert_eq!(no_folder.code, 2, "{}", no_folder.stderr);
    assert_eq!(no_folder.document()["error"]["code"], "invalid_path");
    assert_eq!(no_agent.code, 3, "{}", no_agent.stderr);
    assert_eq!(no_agent.document()["error"]["code"], "no_match");
    assert_eq!(no_agent_flag.code, 2);
    assert!(
        !harness.roaming_app_data().exists(),
        "a refused add writes nothing"
    );
}

#[test]
fn a_hidden_home_and_its_items_are_listed_only_with_include_hidden() {
    let harness = Harness::new().with_file(".claude.json", CLAUDE_JSON);

    let hide = harness.run(&["homes", "hide", "default", "--agent", "claude-code"]);
    let homes = harness.run(&["homes", "list", "--json"]);
    let agents = harness.run(&["agents", "--json"]);
    let items = harness.run(&["list", "--json"]);
    let all_items = harness.run(&["list", "--json", "--include-hidden"]);
    let name = all_items.document()["items"][0]["name"]
        .as_str()
        .unwrap()
        .to_owned();
    let show = harness.run(&["show", &name]);
    let unhide = harness.run(&["homes", "unhide", "default", "--json"]);
    let shown = harness.run(&["homes", "list", "--json"]);

    assert_eq!(hide.code, 0, "{}", hide.stderr);
    assert_eq!(listed_homes(&homes.document()), vec![]);
    assert_eq!(agents.document()["agents"][0]["homes"], json!([]));
    assert_eq!(items.document()["items"], json!([]));
    assert!(!all_items.document()["items"].as_array().unwrap().is_empty());
    // Hiding only filters what is listed: a hidden home's Item still shows.
    assert_eq!(show.code, 0, "{}", show.stderr);
    assert_eq!(unhide.code, 0, "{}", unhide.stderr);
    assert_eq!(unhide.document()["home"]["hidden"], false);
    assert_eq!(
        listed_homes(&shown.document()),
        vec![("default".to_owned(), "default".to_owned(), false)]
    );
}

#[test]
fn projects_add_hide_unhide_and_forget() {
    let harness = Harness::new().with_local_linear();
    let scratch = harness.folder("scratch").display().to_string();
    let workspace = harness.root.path().join("team.code-workspace");
    std::fs::write(&workspace, r#"{"folders": []}"#).unwrap();
    let workspace = real_path(&workspace).display().to_string();
    let registered = harness.cwd().display().to_string();

    let add = harness.run(&["projects", "add", &scratch, "--json"]);
    let add_workspace = harness.run(&["projects", "add", &workspace]);
    let listed = harness.run(&["projects", "list", "--json"]);
    let hide = harness.run(&["projects", "hide", &scratch]);
    let hidden = harness.run(&["projects", "list", "--json"]);
    let with_hidden = harness.run(&["projects", "list", "--json", "--include-hidden"]);
    let unhide = harness.run(&["projects", "unhide", &scratch]);
    let forget = harness.run(&["projects", "forget", &scratch, "--json"]);
    let forgotten = harness.run(&["projects", "list", "--json"]);

    assert_eq!(add.code, 0, "{}", add.stderr);
    assert_eq!(add.document()["project"]["added"], true);
    assert_eq!(add_workspace.code, 0, "{}", add_workspace.stderr);
    let mut expected = vec![registered.clone(), scratch.clone(), workspace.clone()];
    expected.sort_by_key(|path| path.to_lowercase());
    assert_eq!(listed_projects(&listed.document()), expected);
    assert_eq!(hide.code, 0, "{}", hide.stderr);
    assert!(!listed_projects(&hidden.document()).contains(&scratch));
    assert!(listed_projects(&with_hidden.document()).contains(&scratch));
    assert_eq!(unhide.code, 0, "{}", unhide.stderr);
    assert_eq!(forget.code, 0, "{}", forget.stderr);
    assert_eq!(
        listed_projects(&forgotten.document()),
        vec![registered, workspace.clone()]
    );
    assert_eq!(harness.user_list()["projects"], json!([workspace]));
}

#[test]
fn forget_refuses_what_is_discovered_and_points_to_hide() {
    let harness = Harness::new().with_local_linear();
    let registered = harness.cwd().display().to_string();

    let home = harness.run(&["homes", "forget", "default", "--json"]);
    let project = harness.run(&["projects", "forget", &registered]);

    assert_eq!(home.code, 5, "{}", home.stderr);
    assert_eq!(home.document()["error"]["code"], "blocked");
    assert_eq!(project.code, 5, "{}", project.stderr);
    assert!(
        project
            .stderr
            .contains("hide it with `config-digger projects hide`"),
        "{}",
        project.stderr
    );
}

#[test]
fn a_hidden_project_picked_with_project_is_still_evaluated() {
    let harness = Harness::new().with_local_linear();
    let registered = harness.cwd().display().to_string();
    let claude_json = std::fs::read(harness.path(".claude.json")).unwrap();

    let hide = harness.run(&["projects", "hide", &registered]);
    let listed = harness.run(&["projects", "list", "--json"]);
    let picked = harness.run(&["list", "--project", &registered, "--json"]);

    assert_eq!(hide.code, 0, "{}", hide.stderr);
    assert_eq!(listed_projects(&listed.document()), Vec::<String>::new());
    assert_eq!(picked.code, 0, "{}", picked.stderr);
    assert_eq!(picked.document()["project"], registered);
    assert_eq!(
        entry_states(&picked.document(), "linear"),
        pairs(&[("local", "effective"), ("user", "shadowed")])
    );
    // Hiding writes config-digger's own list, never Agent configuration.
    assert_eq!(
        std::fs::read(harness.path(".claude.json")).unwrap(),
        claude_json
    );
}

#[test]
fn agents_lists_the_loaded_definitions_and_the_homes_found() {
    let harness = Harness::new().with_file(".claude.json", CLAUDE_JSON);

    let json = harness.run(&["agents", "--json"]);
    let text = harness.run(&["agents"]);

    assert_eq!(json.code, 0, "{}", json.stderr);
    let agent = &json.document()["agents"][0];
    assert_eq!(agent["id"], "claude-code");
    assert_eq!(agent["built_in"], true);
    assert_eq!(agent["homes"][0]["home"], "default");
    assert_eq!(agent["homes"][0]["source"]["kind"], "default");
    assert!(
        text.stdout
            .starts_with("Claude Code (claude-code), built-in definition\n"),
        "{}",
        text.stdout
    );
}
