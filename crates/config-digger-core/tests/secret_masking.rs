//! No Secret ever leaves the core: Entry configuration, preview diffs, a
//! refused Undo's diff and an incomplete Rollback's diff are masked, through
//! the core `api` with fake ports, and a field one Agent's definition marks
//! is masked in every Agent's view of the file. The only Secret in these
//! fixtures is the fake `not-a-real-secret`.

use config_digger_core::api::{
    Action, Context, Core, DiffUnavailable, Entry, Error, ErrorCode, Inventory, Plan, ScanOptions,
    Targets,
};
use config_digger_core::ports::fake::{self, FakeEnvironment, FakeFileSystem};
use serde_json::{Value, json};
use std::path::PathBuf;

const HOME: &str = r"C:\Users\fixture";
const LOCAL_APP_DATA: &str = r"C:\Users\fixture\AppData\Local";
const SECRET: &str = "not-a-real-secret";

fn claude_json() -> PathBuf {
    PathBuf::from(HOME).join(".claude.json")
}

fn core(fs: &FakeFileSystem) -> Core {
    let env = FakeEnvironment::with_home(HOME).with_var("LOCALAPPDATA", LOCAL_APP_DATA);
    Core::new(fake::ports(fs.clone(), env)).unwrap()
}

/// The one Entry of the user server `name` in `~/.claude.json`.
fn entry(servers: Value, name: &str) -> Entry {
    let text = json!({ "mcpServers": servers }).to_string();
    let fs = FakeFileSystem::new().with_file(claude_json(), text);
    let core = core(&fs);
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    let item = core.show(&snapshot, &Context::default(), name).unwrap();
    item.entries.into_iter().next().unwrap()
}

fn key_paths(paths: &[&[&str]]) -> Vec<Vec<String>> {
    paths
        .iter()
        .map(|path| path.iter().map(|key| key.to_string()).collect())
        .collect()
}

// Entry configuration.

#[test]
fn env_values_are_masked_and_listed() {
    let entry = entry(
        json!({ "github": {
            "command": "npx",
            "args": ["-y", "@modelcontextprotocol/server-github"],
            "env": { "GITHUB_TOKEN": SECRET, "LOG_LEVEL": "debug" }
        }}),
        "github",
    );

    assert_eq!(
        entry.config,
        json!({
            "command": "npx",
            "args": ["-y", "@modelcontextprotocol/server-github"],
            "env": { "GITHUB_TOKEN": "••••", "LOG_LEVEL": "••••" }
        })
    );
    assert_eq!(
        entry.masked,
        key_paths(&[&["env", "GITHUB_TOKEN"], &["env", "LOG_LEVEL"]])
    );
}

#[test]
fn a_masked_value_keeps_only_a_recognised_scheme_or_token_prefix() {
    let entry = entry(
        json!({ "api": {
            "type": "http",
            "url": "https://mcp.example.com/mcp",
            "headers": {
                "Authorization": format!("Bearer {SECRET}"),
                "X-Basic": format!("Basic {SECRET}"),
                "X-GitHub": format!("ghp_{SECRET}"),
                "X-Fine-Grained": format!("Bearer github_pat_{SECRET}"),
                "X-OpenAI": format!("sk-{SECRET}"),
                "X-Slack": format!("xoxb-{SECRET}"),
                "X-Plain": SECRET
            }
        }}),
        "api",
    );

    assert_eq!(
        entry.config["headers"],
        json!({
            "Authorization": "Bearer ••••",
            "X-Basic": "Basic ••••",
            "X-GitHub": "ghp_••••",
            "X-Fine-Grained": "Bearer github_pat_••••",
            "X-OpenAI": "sk-••••",
            "X-Slack": "xoxb-••••",
            "X-Plain": "••••"
        })
    );
    assert_eq!(entry.masked.len(), 7, "{:?}", entry.masked);
    assert!(!entry.config.to_string().contains(SECRET));
}

#[test]
fn a_url_keeps_its_scheme_host_and_path_and_masks_userinfo_and_query_values() {
    let entry = entry(
        json!({ "api": {
            "url": format!("https://user:{SECRET}@mcp.example.com/v1/mcp?key={SECRET}&region=eu#top")
        }}),
        "api",
    );

    assert_eq!(
        entry.config["url"],
        json!("https://••••@mcp.example.com/v1/mcp?key=••••&region=••••#top")
    );
    assert_eq!(entry.masked, key_paths(&[&["url"]]));
}

#[test]
fn a_url_without_userinfo_or_query_is_not_masked() {
    let entry = entry(
        json!({ "api": { "url": "https://mcp.linear.app/mcp" } }),
        "api",
    );

    assert_eq!(entry.config["url"], json!("https://mcp.linear.app/mcp"));
    assert!(entry.masked.is_empty(), "{:?}", entry.masked);
}

#[test]
fn args_after_token_like_flags_are_masked_and_other_args_are_not() {
    let entry = entry(
        json!({ "remote": {
            "command": "npx",
            "args": [
                "-y", "mcp-remote",
                format!("https://mcp.example.com/sse?token={SECRET}"),
                "--header", format!("Authorization: Bearer {SECRET}"),
                "--api-key", SECRET,
                format!("--access-token={SECRET}"),
                "--port", "8080",
                "--transport", "stdio"
            ]
        }}),
        "remote",
    );

    assert_eq!(
        entry.config["args"],
        json!([
            "-y",
            "mcp-remote",
            "https://mcp.example.com/sse?token=••••",
            "--header",
            "Authorization: Bearer ••••",
            "--api-key",
            "••••",
            "--access-token=••••",
            "--port",
            "8080",
            "--transport",
            "stdio"
        ])
    );
    assert_eq!(
        entry.masked,
        key_paths(&[
            &["args", "2"],
            &["args", "4"],
            &["args", "6"],
            &["args", "7"]
        ])
    );
}

#[test]
fn references_in_claude_codes_interpolation_syntax_are_shown_verbatim() {
    let entry = entry(
        json!({ "api": {
            "url": "https://${HOST}/mcp?key=${API_KEY}",
            "headers": {
                "Authorization": "Bearer ${API_KEY}",
                "X-Default": format!("${{API_KEY:-{SECRET}}}"),
                "X-Mixed": format!("{SECRET}${{SUFFIX}}")
            },
            "env": { "TOKEN": "${GITHUB_TOKEN}" }
        }}),
        "api",
    );

    assert_eq!(
        entry.config,
        json!({
            "url": "https://${HOST}/mcp?key=${API_KEY}",
            "headers": {
                "Authorization": "Bearer ${API_KEY}",
                "X-Default": "${API_KEY:-••••}",
                "X-Mixed": "••••${SUFFIX}"
            },
            "env": { "TOKEN": "${GITHUB_TOKEN}" }
        })
    );
    assert_eq!(
        entry.masked,
        key_paths(&[&["headers", "X-Default"], &["headers", "X-Mixed"]])
    );
}

// Preview diffs.

const TWO_SERVERS: &str = r#"{
  "mcpServers": {
    "github": {
      "command": "npx",
      "args": ["-y", "server-github", "--token", "not-a-real-secret"],
      "env": {
        "GITHUB_TOKEN": "ghp_not-a-real-secret"
      }
    },
    "linear": {
      "url": "https://mcp.linear.app/mcp?key=not-a-real-secret"
    }
  }
}
"#;

fn plan_with(core: &Core, action: Action, selector: &str) -> Plan {
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    let targets = Targets {
        selectors: vec![selector.to_owned()],
        all: false,
    };
    core.plan(&snapshot, &Context::default(), action, &targets)
        .unwrap()
}

fn plan_remove(fs: &FakeFileSystem, selector: &str) -> Plan {
    plan_with(&core(fs), Action::Remove, selector)
}

#[test]
fn a_preview_diff_masks_the_removed_entry_and_the_context_around_it() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_SERVERS);

    let plan = plan_remove(&fs, "linear");

    let file = &plan.preview().files[0];
    let diff = file.diff.as_deref().unwrap();
    assert!(!diff.contains(SECRET), "{diff}");
    assert_eq!(
        diff,
        format!(
            r#"--- a/{path}
+++ b/{path}
@@ -6,9 +6,6 @@
       "env": {{
         "GITHUB_TOKEN": "ghp_••••"
       }}
-    }},
-    "linear": {{
-      "url": "https://mcp.linear.app/mcp?key=••••"
     }}
   }}
 }}
"#,
            path = claude_json().display()
        )
    );
    assert_eq!(file.diff_unavailable, None);
}

// An incomplete Rollback's diff.

const PROJECT: &str = r"E:\projects\petme-app";

fn mcp_json() -> PathBuf {
    PathBuf::from(PROJECT).join(".mcp.json")
}

const PROJECT_SERVERS: &str = r#"{
  "mcpServers": {
    "github": {
      "env": {
        "GITHUB_TOKEN": "ghp_not-a-real-secret"
      }
    },
    "linear": {
      "url": "https://p"
    }
  }
}
"#;

/// Removes both `linear` Entries in [`PROJECT`]: `.mcp.json` is written
/// first, then `~/.claude.json`.
fn remove_both_linear(fs: &FakeFileSystem) -> Error {
    let core = core(fs);
    let snapshot = core
        .scan(&ScanOptions {
            projects: vec![PathBuf::from(PROJECT)],
        })
        .unwrap();
    let context = Context {
        project: Some(PathBuf::from(PROJECT)),
    };
    let targets = Targets {
        selectors: vec!["linear".to_owned()],
        all: true,
    };
    let plan = core
        .plan(&snapshot, &context, Action::Remove, &targets)
        .unwrap();
    core.execute(plan, &[]).unwrap_err()
}

#[test]
fn a_rollback_diff_shows_a_rotated_secret_as_a_secret_value_change() {
    // `.mcp.json` is written, then rewritten with the edit and a rotated
    // token; `~/.claude.json` fails, and `.mcp.json` cannot be restored.
    let rewritten = PROJECT_SERVERS
        .replace("ghp_not-a-real-secret", "ghp_not-a-real-secret-rotated")
        .replace(
            ",\n    \"linear\": {\n      \"url\": \"https://p\"\n    }",
            "",
        );
    let fs = FakeFileSystem::new()
        .with_file(claude_json(), TWO_SERVERS)
        .with_file(mcp_json(), PROJECT_SERVERS)
        .agent_rewrites_after_next_write(mcp_json(), rewritten)
        .failing_writes_to(claude_json());

    let error = remove_both_linear(&fs);

    assert_eq!(error.code, ErrorCode::RollbackIncomplete);
    let [diff] = error.diffs.as_slice() else {
        panic!("{:?}", error.diffs);
    };
    assert_eq!(diff.path, mcp_json());
    assert_eq!(
        diff.diff.as_deref().unwrap(),
        format!(
            r#"--- a/{path}
+++ b/{path}
@@ -2,11 +2,8 @@
   "mcpServers": {{
     "github": {{
       "env": {{
-        "GITHUB_TOKEN": "ghp_••••"
+        "GITHUB_TOKEN": "ghp_••••"
\ secret value changed
       }}
-    }},
-    "linear": {{
-      "url": "https://p"
     }}
   }}
 }}
"#,
            path = mcp_json().display()
        )
    );
}

#[test]
fn a_side_that_does_not_parse_gets_no_diff_but_says_why_and_how_many_lines_differ() {
    // The Agent rewrites `.mcp.json` with the edit and a trailing comma
    // strict JSON rejects, right after a quoted Secret.
    let rewritten = PROJECT_SERVERS.replace(
        ",\n    \"linear\": {\n      \"url\": \"https://p\"\n    }",
        ",",
    );
    let fs = FakeFileSystem::new()
        .with_file(claude_json(), TWO_SERVERS)
        .with_file(mcp_json(), PROJECT_SERVERS)
        .agent_rewrites_after_next_write(mcp_json(), rewritten)
        .failing_writes_to(claude_json());

    let error = remove_both_linear(&fs);

    assert_eq!(error.code, ErrorCode::RollbackIncomplete);
    let [diff] = error.diffs.as_slice() else {
        panic!("{:?}", error.diffs);
    };
    assert_eq!(diff.diff, None);
    assert_eq!(diff.diff_unavailable, Some(DiffUnavailable::Parse));
    assert_eq!(diff.changed_lines, 3);
    assert_eq!(
        diff.diff_note.as_deref(),
        Some(
            format!(
                "Can't show a masked diff: `{}` as it is now doesn't parse as strict JSON (parse error at 8:3); 3 lines differ.",
                mcp_json().display()
            )
            .as_str()
        )
    );
    let json = serde_json::to_value(&error).unwrap();
    assert_eq!(json["diffs"][0]["diff"], Value::Null);
    assert_eq!(json["diffs"][0]["diff_unavailable"], json!("parse"));
    assert!(!json.to_string().contains(SECRET), "{json}");
}

// A refused Undo's diff.

#[test]
fn a_refused_undo_diff_masks_the_rotated_secret() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_SERVERS);
    let core = core(&fs);
    let removed = core.execute(plan_remove(&fs, "github"), &[]).unwrap();
    let put_back = core
        .execute(core.plan_undo(&removed.operation).unwrap(), &[])
        .unwrap();
    // The user rotates the token the Undo put back.
    let now = String::from_utf8(fs.contents(claude_json()).unwrap())
        .unwrap()
        .replace("ghp_not-a-real-secret", "ghp_not-a-real-secret-rotated");
    fs.put_file(claude_json(), now);

    let error = core.plan_undo(&put_back.operation).err().unwrap();

    assert_eq!(error.code, ErrorCode::UndoRefused);
    let [diff] = error.diffs.as_slice() else {
        panic!("{:?}", error.diffs);
    };
    let text = diff.diff.as_deref().unwrap();
    assert!(
        text.contains(r#"+        "GITHUB_TOKEN": "ghp_••••""#),
        "{text}"
    );
    assert!(
        text.contains(r#"+      "args": ["-y", "server-github", "--token", "••••"],"#),
        "{text}"
    );
    assert!(!serde_json::to_string(&error).unwrap().contains(SECRET));
}

#[test]
fn env_args_keep_the_variable_name_and_mask_its_value() {
    let entry = entry(
        json!({ "docker": {
            "command": "docker",
            "args": [
                "run", "-i", "--rm",
                "-e", format!("GITHUB_TOKEN={SECRET}"),
                "-e", "GITHUB_HOST",
                format!("--env=API_KEY={SECRET}"),
                "ghcr.io/github/github-mcp-server"
            ]
        }}),
        "docker",
    );

    assert_eq!(
        entry.config["args"],
        json!([
            "run",
            "-i",
            "--rm",
            "-e",
            "GITHUB_TOKEN=••••",
            "-e",
            "GITHUB_HOST",
            "--env=API_KEY=••••",
            "ghcr.io/github/github-mcp-server"
        ])
    );
    assert_eq!(entry.masked, key_paths(&[&["args", "4"], &["args", "7"]]));
}

#[test]
fn a_url_fragment_with_parameters_is_masked_like_a_query() {
    let entry = entry(
        json!({ "api": { "url": format!("https://mcp.example.com/cb#access_token={SECRET}&type=bearer") } }),
        "api",
    );

    assert_eq!(
        entry.config["url"],
        json!("https://mcp.example.com/cb#access_token=••••&type=••••")
    );
}

#[test]
fn a_preview_diff_masks_secrets_the_definition_marks_outside_any_entry() {
    let text = r#"{
  "mcpServers": {
    "linear": {
      "url": "https://mcp.linear.app/mcp"
    }
  },
  "primaryApiKey": "sk-ant-not-a-real-secret",
  "customApiKeyResponses": {
    "approved": ["not-a-real-secret"]
  }
}
"#;
    let fs = FakeFileSystem::new().with_file(claude_json(), text);

    let plan = plan_remove(&fs, "linear");

    let diff = plan.preview().files[0].diff.clone().unwrap();
    assert!(!diff.contains(SECRET), "{diff}");
    assert!(
        diff.contains(r#"   "primaryApiKey": "sk-ant-••••","#),
        "{diff}"
    );
    assert!(diff.contains(r#"     "approved": ["••••"]"#), "{diff}");
}

// History's diff of what an Operation changed.

#[test]
fn history_diffs_mask_the_removed_entry_and_the_context_around_it() {
    let fs = FakeFileSystem::new().with_file(claude_json(), TWO_SERVERS);
    let core = core(&fs);
    let removed = core.execute(plan_remove(&fs, "github"), &[]).unwrap();

    let operation = core.history_show(&removed.operation).unwrap();

    let diff = operation.locations[0].diff.as_ref().unwrap();
    let text = diff.diff.as_deref().unwrap();
    assert!(
        text.contains(r#"-        "GITHUB_TOKEN": "ghp_••••""#),
        "{text}"
    );
    assert!(
        text.contains(r#"     "linear": {"#) && text.contains("?key=••••"),
        "{text}"
    );
    assert!(!serde_json::to_string(&operation).unwrap().contains(SECRET));
}

// A Secret one Agent's definition marks in a file that several Agents read.

const APP_DATA: &str = r"C:\Users\fixture\AppData\Roaming";

/// The folder of the Claude Code plugin `context7@market`, which Cursor
/// imports.
fn context7_folder() -> PathBuf {
    PathBuf::from(HOME).join(r".claude\plugins\cache\market\context7\1.0.0")
}

/// A Cursor that has run once, and Claude Code with the plugin
/// `context7@market` installed and enabled, its `.mcp.json` holding
/// `servers`.
fn context7_in_claude_code_and_cursor(servers: Value) -> FakeFileSystem {
    let install = json!([{
        "scope": "user",
        "installPath": context7_folder().display().to_string(),
        "version": "1.0.0",
    }]);
    FakeFileSystem::new()
        .with_file(
            PathBuf::from(APP_DATA).join(r"Cursor\User\settings.json"),
            "{}\n",
        )
        .with_file(
            PathBuf::from(HOME).join(r".claude\plugins\installed_plugins.json"),
            json!({ "version": 2, "plugins": { "context7@market": install } }).to_string(),
        )
        .with_file(
            PathBuf::from(HOME).join(r".claude\settings.json"),
            json!({ "enabledPlugins": { "context7@market": true } }).to_string(),
        )
        .with_file(
            context7_folder().join(".mcp.json"),
            json!({ "mcpServers": servers }).to_string(),
        )
}

/// An environment where Cursor and Claude Desktop find their folders too.
fn app_data_env() -> FakeEnvironment {
    FakeEnvironment::with_home(HOME)
        .with_var("APPDATA", APP_DATA)
        .with_var("LOCALAPPDATA", LOCAL_APP_DATA)
        .with_var("USERPROFILE", HOME)
}

fn inventory_with_cursor(fs: &FakeFileSystem) -> Inventory {
    let core = Core::new(fake::ports(fs.clone(), app_data_env())).unwrap();
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    core.resolve(&snapshot, &Context::default())
}

/// The one Entry of the Item `name` of the Agent `agent`.
fn entry_of<'i>(inventory: &'i Inventory, agent: &str, name: &str) -> &'i Entry {
    let item = inventory
        .items
        .iter()
        .find(|item| item.agent == agent && item.name == name)
        .unwrap_or_else(|| panic!("no {agent} Item `{name}`"));
    &item.entries[0]
}

#[test]
fn a_field_cursor_marks_in_a_claude_code_plugin_s_mcp_json_is_masked_in_both_agents() {
    let fs = context7_in_claude_code_and_cursor(json!({ "context7": {
        "url": "https://mcp.context7.com/mcp",
        "auth": { "CLIENT_ID": "context7-client", "CLIENT_SECRET": SECRET },
    }}));

    let inventory = inventory_with_cursor(&fs);

    for (agent, name) in [
        ("claude-code", "plugin:context7:context7"),
        ("cursor", "plugin-context7-context7"),
    ] {
        let entry = entry_of(&inventory, agent, name);
        assert_eq!(
            entry.config["auth"],
            json!({ "CLIENT_ID": "context7-client", "CLIENT_SECRET": "••••" }),
            "{agent}"
        );
        assert_eq!(
            entry.masked,
            key_paths(&[&["auth", "CLIENT_SECRET"]]),
            "{agent}"
        );
    }
    assert!(!serde_json::to_string(&inventory).unwrap().contains(SECRET));
}

const DEFINITIONS: &str = r"E:\dev\definitions";

/// An Agent loaded through `--definitions` that reads the servers of
/// `~/.claude.json` and marks their `auth.CLIENT_SECRET`.
const MARKS_CLAUDE_JSON: &str = r#"
format_version = 1

[agent]
id = "marker"
name = "Marker"

[[homes]]
id = "default"
folder = { block = "path", path = "~" }
roots.config = ".claude.json"

[[scopes]]
id = "user"

[[locations]]
id = "user-mcp"
anchor = { block = "home", root = "config" }
format = { block = "json", dialect = "strict" }
root = ["mcpServers"]
kind = "mcp"
scope = "user"
provenance = "file"
store_class = "plain"
secrets.fields = [["auth", "CLIENT_SECRET"]]
on_read_failure = "unknown"

[items.mcp]
precedence = ["user-mcp"]
competition = { block = "exact-name" }
checks = ["disable", "precedence"]
"#;

const AUTH_SERVERS: &str = r#"{
  "mcpServers": {
    "context7": {
      "url": "https://mcp.context7.com/mcp",
      "auth": {
        "CLIENT_SECRET": "not-a-real-secret"
      }
    },
    "linear": {
      "url": "https://mcp.linear.app/mcp"
    }
  }
}
"#;

/// A core that loads the definition `marker` through `--definitions`, over
/// the file at `path` holding `text`.
fn core_with_marker(path: PathBuf, text: &str, marker: &str) -> Core {
    let fs = FakeFileSystem::new()
        .with_file(path, text)
        .with_file(PathBuf::from(DEFINITIONS).join("marker.toml"), marker);
    let ports = fake::ports(fs, app_data_env());
    let loaded = Core::with_definitions_folder(ports, &PathBuf::from(DEFINITIONS)).unwrap();
    assert!(loaded.rejected.is_empty(), "{:?}", loaded.rejected);
    loaded.core
}

/// [`AUTH_SERVERS`] in `~/.claude.json`, with [`MARKS_CLAUDE_JSON`] loaded.
fn claude_json_with_marker() -> Core {
    core_with_marker(claude_json(), AUTH_SERVERS, MARKS_CLAUDE_JSON)
}

#[test]
fn a_field_a_loaded_definition_marks_is_masked_in_claude_code_s_view() {
    let core = claude_json_with_marker();
    let snapshot = core.scan(&ScanOptions::default()).unwrap();

    let inventory = core.resolve(&snapshot, &Context::default());

    for agent in ["claude-code", "marker"] {
        let entry = entry_of(&inventory, agent, "context7");
        assert_eq!(entry.config["auth"]["CLIENT_SECRET"], "••••", "{agent}");
        assert_eq!(
            entry.masked,
            key_paths(&[&["auth", "CLIENT_SECRET"]]),
            "{agent}"
        );
    }
    assert!(!serde_json::to_string(&inventory).unwrap().contains(SECRET));
}

#[test]
fn preview_and_history_diffs_mask_a_field_a_loaded_definition_marks() {
    let core = claude_json_with_marker();

    let plan = plan_with(&core, Action::Remove, "context7");

    let preview = serde_json::to_string(plan.preview()).unwrap();
    assert!(!preview.contains(SECRET), "{preview}");
    let diff = plan.preview().files[0].diff.clone().unwrap();
    assert!(
        diff.contains(r#"-        "CLIENT_SECRET": "••••""#),
        "{diff}"
    );
    let removed = core.execute(plan, &[]).unwrap();
    let operation = core.history_show(&removed.operation).unwrap();
    let history = serde_json::to_string(&operation).unwrap();
    assert!(!history.contains(SECRET), "{history}");
    assert!(history.contains("CLIENT_SECRET"), "{history}");
}

/// Claude Desktop's classic user file, whose servers config-digger parks
/// to Disable them.
fn desktop_config() -> PathBuf {
    PathBuf::from(APP_DATA).join(r"Claude\claude_desktop_config.json")
}

/// An Agent loaded through `--definitions` that reads the servers of
/// Claude Desktop's classic user file and marks their `auth.CLIENT_SECRET`.
fn marks_desktop_config() -> String {
    MARKS_CLAUDE_JSON
        .replace(r#"path = "~""#, r#"path = "%APPDATA%/Claude""#)
        .replace(".claude.json", "claude_desktop_config.json")
}

/// The server `notes` of Claude Desktop, parked out of its user file, while
/// [`marks_desktop_config`] is loaded.
fn parked_notes() -> (Core, String) {
    let servers = json!({ "mcpServers": { "notes": {
        "command": "notes-mcp",
        "auth": { "CLIENT_SECRET": SECRET },
    }}});
    let core = core_with_marker(
        desktop_config(),
        &servers.to_string(),
        &marks_desktop_config(),
    );
    let locator = format!("{}#mcpServers.notes", desktop_config().display());
    let plan = plan_with(&core, Action::Disable, &locator);
    let preview = serde_json::to_string(plan.preview()).unwrap();
    assert!(!preview.contains(SECRET), "{preview}");
    let parked = core.execute(plan, &[]).unwrap().operation;
    (core, parked)
}

#[test]
fn a_parked_entry_masks_a_field_a_reader_of_its_file_marks() {
    let (core, parked) = parked_notes();

    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    let inventory = core.resolve(&snapshot, &Context::default());
    let entry = entry_of(&inventory, "claude-desktop", "notes");
    assert_eq!(entry.config["auth"]["CLIENT_SECRET"], "••••");
    assert!(!serde_json::to_string(&inventory).unwrap().contains(SECRET));
    let history = serde_json::to_string(&core.history_show(&parked).unwrap()).unwrap();
    assert!(!history.contains(SECRET), "{history}");
}
