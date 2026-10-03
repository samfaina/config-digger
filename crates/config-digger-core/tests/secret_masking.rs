//! No Secret ever leaves the core: Entry configuration, preview diffs, a
//! refused Undo's diff and an incomplete Rollback's diff are masked, through
//! the core `api` with fake ports. The only Secret in these fixtures is the
//! fake `not-a-real-secret`.

use config_digger_core::api::{
    Action, Context, Core, DiffUnavailable, Entry, Error, ErrorCode, Plan, ScanOptions, Targets,
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
fn a_url_without_credentials_or_query_is_not_masked() {
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

fn plan_remove(fs: &FakeFileSystem, selector: &str) -> Plan {
    let core = core(fs);
    let snapshot = core.scan(&ScanOptions::default()).unwrap();
    let targets = Targets {
        selectors: vec![selector.to_owned()],
        all: false,
    };
    core.plan(&snapshot, &Context::default(), Action::Remove, &targets)
        .unwrap()
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
fn a_rollback_diff_shows_a_rotated_token_as_a_secret_value_change() {
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
fn a_refused_undo_diff_masks_the_rotated_token() {
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
