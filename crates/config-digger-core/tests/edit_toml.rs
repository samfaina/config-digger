//! TOML edits, seeded with the T cases of the format-edit spike
//! (`spikes/format-edits` on `research/format-preserving-edits`).

use config_digger_core::edit::{Applied, Dependency, Edit, EditError, EditableFile, Syntax};
use serde_json::json;

const CONFIG_TOML: &[u8] = include_bytes!("fixtures/edit/config.toml");
const CODEX_FEISKYER: &[u8] = include_bytes!("fixtures/edit/codex-feiskyer.toml");

const INLINE_TOML: &[u8] = br#"[mcp_servers]
# Linear (hosted)
linear = { url = "https://mcp.linear.app/mcp" } # oauth
sentry = { command = "npx", args = ["@sentry/mcp-server"] }
"#;

fn crlf(bytes: &[u8]) -> Vec<u8> {
    String::from_utf8(bytes.to_vec())
        .unwrap()
        .replace('\n', "\r\n")
        .into_bytes()
}

fn with_bom(bytes: &[u8]) -> Vec<u8> {
    [&[0xEF, 0xBB, 0xBF][..], bytes].concat()
}

fn key(path: &str) -> Vec<String> {
    path.split('.').map(String::from).collect()
}

fn apply(bytes: &[u8], edit: &Edit) -> Applied {
    EditableFile::decode(bytes, Syntax::Toml)
        .unwrap()
        .apply(edit)
        .unwrap()
}

fn text(bytes: &[u8], edit: &Edit) -> String {
    String::from_utf8(apply(bytes, edit).bytes).unwrap()
}

/// The unified diff, with trailing whitespace trimmed so that the blank
/// context lines of the expected diffs stay visible in the source.
fn diff(bytes: &[u8], edit: &Edit) -> String {
    apply(bytes, edit)
        .unified_diff("config.toml")
        .lines()
        .map(|line| format!("{}\n", line.trim_end()))
        .collect()
}

#[test]
fn remove_of_a_table_keeps_the_section_comment_above_its_own() {
    let edit = Edit::Remove {
        key_path: key("mcp_servers.context7"),
    };

    assert_eq!(
        diff(CONFIG_TOML, &edit),
        r#"--- a/config.toml
+++ b/config.toml
@@ -3,15 +3,6 @@

 # --- MCP servers ---

-# Docs server, read-only
-[mcp_servers.context7]
-command = "npx"
-args = ["-y", "@upstash/context7-mcp"]  # pinned below
-startup_timeout_sec = 20
-
-[mcp_servers.context7.env]
-LOG_LEVEL = "warn"
-
 # GitHub, uses a PAT from the environment
 [mcp_servers.github]
 url = "https://api.githubcopilot.com/mcp/"
"#
    );
}

#[test]
fn remove_of_the_last_table_keeps_the_next_tables_comment() {
    let edit = Edit::Remove {
        key_path: key("mcp_servers.github"),
    };

    assert_eq!(
        text(CONFIG_TOML, &edit),
        r#"# Codex config
model = "gpt-5-codex"

# --- MCP servers ---

# Docs server, read-only
[mcp_servers.context7]
command = "npx"
args = ["-y", "@upstash/context7-mcp"]  # pinned below
startup_timeout_sec = 20

[mcp_servers.context7.env]
LOG_LEVEL = "warn"

# Profiles below
[profiles.fast]
model = "gpt-5-mini"
"#
    );
}

#[test]
fn remove_of_every_server_leaves_no_empty_parent_table() {
    let first = apply(
        CONFIG_TOML,
        &Edit::Remove {
            key_path: key("mcp_servers.context7"),
        },
    );
    let both = text(
        &first.bytes,
        &Edit::Remove {
            key_path: key("mcp_servers.github"),
        },
    );

    assert_eq!(
        both,
        r#"# Codex config
model = "gpt-5-codex"

# --- MCP servers ---

# Profiles below
[profiles.fast]
model = "gpt-5-mini"
"#
    );
}

#[test]
fn set_of_a_missing_field_adds_one_line_at_the_end_of_the_table() {
    let edit = Edit::Set {
        key_path: key("mcp_servers.github.enabled"),
        value: json!(false),
    };

    assert_eq!(
        diff(CONFIG_TOML, &edit),
        r#"--- a/config.toml
+++ b/config.toml
@@ -17,6 +17,7 @@
 url = "https://api.githubcopilot.com/mcp/"
 bearer_token_env_var = "GH_TOKEN"
 enabled_tools = ["list_issues", "get_issue",]
+enabled = false

 # Profiles below
 [profiles.fast]
"#
    );
}

#[test]
fn set_of_an_existing_field_changes_one_token_and_keeps_its_comment() {
    let edit = Edit::Set {
        key_path: key("mcp_servers.context7.args"),
        value: json!(["-y", "@upstash/context7-mcp@1.0.0"]),
    };

    assert_eq!(
        diff(CONFIG_TOML, &edit),
        r#"--- a/config.toml
+++ b/config.toml
@@ -6,7 +6,7 @@
 # Docs server, read-only
 [mcp_servers.context7]
 command = "npx"
-args = ["-y", "@upstash/context7-mcp"]  # pinned below
+args = ["-y", "@upstash/context7-mcp@1.0.0"]  # pinned below
 startup_timeout_sec = 20

 [mcp_servers.context7.env]
"#
    );
}

#[test]
fn undoing_a_set_gives_back_the_original_bytes() {
    let disable = Edit::Set {
        key_path: key("mcp_servers.github.enabled"),
        value: json!(false),
    };
    let disabled = apply(CONFIG_TOML, &disable);
    let enable = Edit::Set {
        key_path: key("mcp_servers.github.enabled"),
        value: json!(true),
    };
    let enabled = apply(&disabled.bytes, &enable);

    assert_eq!(
        enabled.inverse,
        Edit::Set {
            key_path: key("mcp_servers.github.enabled"),
            value: json!(false),
        }
    );
    assert_eq!(
        disabled.inverse,
        Edit::Remove {
            key_path: key("mcp_servers.github.enabled"),
        }
    );
    assert_eq!(
        disabled.depends_on,
        vec![Dependency {
            key_path: key("mcp_servers.github.enabled"),
            before: None,
            after: Some(json!(false)),
        }]
    );
    assert_eq!(apply(&disabled.bytes, &disabled.inverse).bytes, CONFIG_TOML);
}

#[test]
fn remove_of_an_inline_server_takes_its_comment_line() {
    let edit = Edit::Remove {
        key_path: key("mcp_servers.linear"),
    };

    assert_eq!(
        text(INLINE_TOML, &edit),
        "[mcp_servers]\nsentry = { command = \"npx\", args = [\"@sentry/mcp-server\"] }\n"
    );
}

#[test]
fn remove_of_the_last_inline_server() {
    let edit = Edit::Remove {
        key_path: key("mcp_servers.sentry"),
    };

    assert_eq!(
        text(INLINE_TOML, &edit),
        "[mcp_servers]\n# Linear (hosted)\nlinear = { url = \"https://mcp.linear.app/mcp\" } # oauth\n"
    );
}

#[test]
fn set_in_an_inline_table_keeps_it_on_one_line_with_normal_spacing() {
    let edit = Edit::Set {
        key_path: key("mcp_servers.sentry.enabled"),
        value: json!(false),
    };

    assert_eq!(
        text(INLINE_TOML, &edit),
        "[mcp_servers]\n# Linear (hosted)\nlinear = { url = \"https://mcp.linear.app/mcp\" } # oauth\nsentry = { command = \"npx\", args = [\"@sentry/mcp-server\"], enabled = false }\n"
    );
}

#[test]
fn append_and_remove_item_follow_the_lists_layout() {
    let append = Edit::Append {
        key_path: key("mcp_servers.github.enabled_tools"),
        value: json!("create_issue"),
    };
    let appended = apply(CONFIG_TOML, &append);
    assert!(
        String::from_utf8(appended.bytes.clone())
            .unwrap()
            .contains("enabled_tools = [\"list_issues\", \"get_issue\", \"create_issue\",]\n")
    );
    assert_eq!(
        appended.inverse,
        Edit::RemoveItem {
            key_path: key("mcp_servers.github.enabled_tools"),
            value: json!("create_issue"),
        }
    );
    assert_eq!(apply(&appended.bytes, &appended.inverse).bytes, CONFIG_TOML);

    let multiline = b"tools = [\n  \"a\",\n  \"b\",\n]\n";
    let appended = text(
        multiline,
        &Edit::Append {
            key_path: key("tools"),
            value: json!("c"),
        },
    );
    assert_eq!(appended, "tools = [\n  \"a\",\n  \"b\",\n  \"c\",\n]\n");
    let removed = text(
        multiline,
        &Edit::RemoveItem {
            key_path: key("tools"),
            value: json!("a"),
        },
    );
    assert_eq!(removed, "tools = [\n  \"b\",\n]\n");
}

#[test]
fn edits_in_a_crlf_file_keep_crlf() {
    for edit in [
        Edit::Remove {
            key_path: key("mcp_servers.context7"),
        },
        Edit::Set {
            key_path: key("mcp_servers.github.enabled"),
            value: json!(false),
        },
    ] {
        assert_eq!(
            apply(&crlf(CONFIG_TOML), &edit).bytes,
            crlf(&apply(CONFIG_TOML, &edit).bytes)
        );
    }
}

#[test]
fn edits_keep_the_bom() {
    let edit = Edit::Remove {
        key_path: key("mcp_servers.github"),
    };

    assert_eq!(
        apply(&with_bom(CONFIG_TOML), &edit).bytes,
        with_bom(&apply(CONFIG_TOML, &edit).bytes)
    );
}

#[test]
fn edits_keep_a_missing_final_newline() {
    let bytes = CONFIG_TOML.strip_suffix(b"\n").unwrap();
    let edit = Edit::Set {
        key_path: key("profiles.fast.model_reasoning_effort"),
        value: json!("low"),
    };

    assert!(
        text(bytes, &edit).ends_with("model = \"gpt-5-mini\"\nmodel_reasoning_effort = \"low\"")
    );
}

/// A Codex `config.toml` whose last line, in the server's table, has no
/// newline, as `uat/data/cd-throwaway-setup.ps1` writes it.
const NO_FINAL_NEWLINE: &[u8] =
    b"model = \"gpt-5\"\n\n[mcp_servers.x]\ncommand = \"node\"\nargs = [\"x\"]";

/// Codex's Disable of the server in [`NO_FINAL_NEWLINE`], which appends a key
/// on a new last line.
fn disable_x() -> Edit {
    Edit::Set {
        key_path: key("mcp_servers.x.enabled"),
        value: json!(false),
    }
}

#[test]
fn undoing_a_disable_in_a_file_without_a_final_newline_gives_back_the_original_bytes() {
    for original in [NO_FINAL_NEWLINE.to_vec(), crlf(NO_FINAL_NEWLINE)] {
        let disabled = apply(&original, &disable_x());

        assert_eq!(apply(&disabled.bytes, &disabled.inverse).bytes, original);
    }
}

#[test]
fn an_enable_that_deletes_the_key_after_a_disable_gives_back_the_original_bytes() {
    let disabled = apply(NO_FINAL_NEWLINE, &disable_x());
    let enable = Edit::Remove {
        key_path: key("mcp_servers.x.enabled"),
    };

    assert_eq!(apply(&disabled.bytes, &enable).bytes, NO_FINAL_NEWLINE);
}

#[test]
fn remove_of_a_key_on_the_last_line_keeps_the_final_newline_missing() {
    let edit = Edit::Remove {
        key_path: key("mcp_servers.x.args"),
    };

    assert_eq!(
        text(NO_FINAL_NEWLINE, &edit),
        "model = \"gpt-5\"\n\n[mcp_servers.x]\ncommand = \"node\""
    );
}

#[test]
fn the_undo_of_a_remove_is_an_insert_that_brings_back_the_table() {
    let edit = Edit::Remove {
        key_path: key("mcp_servers.context7"),
    };
    let applied = apply(CONFIG_TOML, &edit);
    let context7 = json!({
        "command": "npx",
        "args": ["-y", "@upstash/context7-mcp"],
        "startup_timeout_sec": 20,
        "env": { "LOG_LEVEL": "warn" }
    });

    assert_eq!(
        applied.inverse,
        Edit::Insert {
            key_path: key("mcp_servers.context7"),
            value: context7.clone(),
            after: None,
            inline: false,
            comments: vec!["# Docs server, read-only".into()],
            emptied: None,
        }
    );
    assert_eq!(
        applied.depends_on,
        vec![Dependency {
            key_path: key("mcp_servers.context7"),
            before: Some(context7),
            after: None,
        }]
    );

    let undone = apply(&applied.bytes, &applied.inverse);
    let original = EditableFile::decode(CONFIG_TOML, Syntax::Toml).unwrap();
    let undone_file = EditableFile::decode(&undone.bytes, Syntax::Toml).unwrap();
    assert_eq!(
        undone_file.value_at(&[]).unwrap(),
        original.value_at(&[]).unwrap()
    );
    let undone_text = String::from_utf8(undone.bytes).unwrap();
    assert!(
        undone_text.contains(
            "# Docs server, read-only\n[mcp_servers.context7]\ncommand = \"npx\"\nargs = [\"-y\", \"@upstash/context7-mcp\"]\nstartup_timeout_sec = 20\n\n[mcp_servers.context7.env]\nLOG_LEVEL = \"warn\"\n"
        ),
        "{undone_text}"
    );
}

#[test]
fn the_undo_of_a_remove_puts_a_table_back_after_its_sibling() {
    let applied = apply(
        CONFIG_TOML,
        &Edit::Remove {
            key_path: key("mcp_servers.github"),
        },
    );

    let undone = text(&applied.bytes, &applied.inverse);

    let env = undone.find("[mcp_servers.context7.env]").unwrap();
    let github = undone.find("# GitHub, uses a PAT from the environment\n[mcp_servers.github]\n");
    let profiles = undone.find("# Profiles below\n[profiles.fast]\n").unwrap();
    assert!(
        github.is_some_and(|github| env < github && github < profiles),
        "{undone}"
    );
}

#[test]
fn a_table_is_removed_from_a_real_codex_config() {
    let edit = Edit::Remove {
        key_path: key("mcp_servers.chrome"),
    };

    assert_eq!(
        diff(CODEX_FEISKYER, &edit),
        r#"--- a/config.toml
+++ b/config.toml
@@ -33,12 +33,6 @@
 [sandbox_workspace_write]
 network_access = true

-# MCP Servers
-[mcp_servers.chrome]
-command = "npx"
-args = ["chrome-devtools-mcp@latest", "--no-usage-statistics", "--autoConnect"]
-startup_timeout_sec = 60
-
 [tui]
 status_line = ["model-with-reasoning", "context-remaining", "current-dir", "model-name", "git-branch", "context-used", "context-window-size", "used-tokens", "total-output-tokens", "five-hour-limit", "weekly-limit"]

"#
    );
}

#[test]
fn the_undo_of_a_remove_puts_an_inline_server_back_in_place() {
    let applied = apply(
        INLINE_TOML,
        &Edit::Remove {
            key_path: key("mcp_servers.linear"),
        },
    );
    assert_eq!(
        applied.inverse,
        Edit::Insert {
            key_path: key("mcp_servers.linear"),
            value: json!({ "url": "https://mcp.linear.app/mcp" }),
            after: None,
            inline: true,
            comments: vec!["# Linear (hosted)".into()],
            emptied: None,
        }
    );

    assert_eq!(
        text(&applied.bytes, &applied.inverse),
        "[mcp_servers]\n# Linear (hosted)\nlinear = { url = \"https://mcp.linear.app/mcp\" }\nsentry = { command = \"npx\", args = [\"@sentry/mcp-server\"] }\n"
    );
}

#[test]
fn edits_of_missing_or_mistyped_keys_are_refused() {
    let file = EditableFile::decode(CONFIG_TOML, Syntax::Toml).unwrap();

    assert_eq!(
        file.apply(&Edit::Remove {
            key_path: key("mcp_servers.nope")
        })
        .unwrap_err(),
        EditError::NotFound {
            key_path: key("mcp_servers.nope")
        }
    );
    assert_eq!(
        file.apply(&Edit::Append {
            key_path: key("mcp_servers.github.url"),
            value: json!("x"),
        })
        .unwrap_err(),
        EditError::WrongType {
            key_path: key("mcp_servers.github.url"),
            expected: "a list"
        }
    );
    assert_eq!(
        file.apply(&Edit::Insert {
            key_path: key("mcp_servers.github"),
            value: json!({}),
            after: None,
            inline: false,
            comments: Vec::new(),
            emptied: None,
        })
        .unwrap_err(),
        EditError::AlreadyExists {
            key_path: key("mcp_servers.github")
        }
    );
}

#[test]
fn remove_of_a_table_whose_sub_table_is_further_down_keeps_what_lies_between() {
    let text = b"[a]\nx = 1\n\n[b]\ny = 2\n\n# --- later ---\n\n[a.sub]\nz = 3\n";
    let file = EditableFile::decode(text, Syntax::Toml).unwrap();

    match file.apply(&Edit::Remove { key_path: key("a") }) {
        Ok(applied) => {
            let edited = String::from_utf8(applied.bytes).unwrap();
            assert!(edited.contains("[b]\ny = 2\n"), "{edited}");
            assert!(edited.contains("# --- later ---"), "{edited}");
        }
        Err(error) => assert!(matches!(error, EditError::OutsideEntry { .. }), "{error:?}"),
    }
}

#[test]
fn the_inverse_of_a_set_keeps_datetimes_and_special_floats() {
    let text = b"created = 1979-05-27T07:32:00Z\nratio = nan\n";
    for key_path in ["created", "ratio"] {
        let edit = Edit::Set {
            key_path: key(key_path),
            value: json!("changed"),
        };
        let applied = apply(text, &edit);

        assert_eq!(
            apply(&applied.bytes, &applied.inverse).bytes,
            text,
            "{key_path}"
        );
    }
}

#[test]
fn a_datetime_is_not_mistaken_for_its_string_form() {
    let file = EditableFile::decode(b"created = 1979-05-27T07:32:00Z\n", Syntax::Toml).unwrap();
    let applied = file
        .apply(&Edit::Set {
            key_path: key("created"),
            value: json!("1979-05-27T07:32:00Z"),
        })
        .unwrap();

    assert_eq!(
        String::from_utf8(applied.bytes).unwrap(),
        "created = \"1979-05-27T07:32:00Z\"\n"
    );
}

#[test]
fn insert_into_an_inline_table_goes_after_its_sibling() {
    let toml = b"server = { a = 1, c = 3 }\n";
    let insert = |after: Option<&str>| Edit::Insert {
        key_path: key("server.b"),
        value: json!(2),
        after: after.map(String::from),
        inline: true,
        comments: Vec::new(),
        emptied: None,
    };

    assert_eq!(
        text(toml, &insert(Some("a"))),
        "server = { a = 1, b = 2, c = 3 }\n"
    );
    assert_eq!(
        text(toml, &insert(None)),
        "server = { b = 2, a = 1, c = 3 }\n"
    );
}

#[test]
fn a_table_inserted_after_a_plain_key_goes_before_the_first_sibling_table() {
    let toml = b"[servers]\ntimeout = 5\n\n[servers.a]\nx = 1\n\n[other]\ny = 2\n";
    let edit = Edit::Insert {
        key_path: key("servers.b"),
        value: json!({ "x": 2 }),
        after: Some("timeout".into()),
        inline: false,
        comments: Vec::new(),
        emptied: None,
    };

    assert_eq!(
        text(toml, &edit),
        "[servers]\ntimeout = 5\n\n[servers.b]\nx = 2\n\n[servers.a]\nx = 1\n\n[other]\ny = 2\n"
    );
}

#[test]
fn a_server_set_under_a_table_without_header_gets_its_own_header_and_its_undo_leaves_none() {
    let edit = Edit::Set {
        key_path: key("mcp_servers.linear"),
        value: json!({ "url": "https://mcp.linear.app/mcp" }),
    };
    let applied = apply(CONFIG_TOML, &edit);
    let edited = String::from_utf8(applied.bytes.clone()).unwrap();

    assert!(
        edited.contains(
            "enabled_tools = [\"list_issues\", \"get_issue\",]\n\n[mcp_servers.linear]\nurl = \"https://mcp.linear.app/mcp\"\n\n# Profiles below\n"
        ),
        "{edited}"
    );
    assert!(!edited.contains("[mcp_servers]\n"), "{edited}");
    assert_eq!(apply(&applied.bytes, &applied.inverse).bytes, CONFIG_TOML);
}
