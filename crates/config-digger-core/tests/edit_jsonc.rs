//! JSONC edits, seeded with the J cases of the format-edit spike
//! (`spikes/format-edits` on `research/format-preserving-edits`).

use config_digger_core::edit::{Applied, Dependency, Edit, EditError, EditableFile, Syntax};
use serde_json::json;

const MCP_JSONC: &[u8] = include_bytes!("fixtures/edit/mcp.jsonc");

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
    EditableFile::decode(bytes, Syntax::Jsonc)
        .unwrap()
        .apply(edit)
        .unwrap()
}

/// The unified diff, with trailing whitespace trimmed so that the blank
/// context lines of the expected diffs stay visible in the source.
fn diff(bytes: &[u8], edit: &Edit) -> String {
    apply(bytes, edit)
        .unified_diff("mcp.jsonc")
        .lines()
        .map(|line| format!("{}\n", line.trim_end()))
        .collect()
}

#[test]
fn remove_takes_the_comment_attached_above_the_entry() {
    let edit = Edit::Remove {
        key_path: key("mcpServers.github"),
    };

    assert_eq!(
        diff(MCP_JSONC, &edit),
        r#"--- a/mcp.jsonc
+++ b/mcp.jsonc
@@ -7,13 +7,6 @@
       "args": ["-y", "@modelcontextprotocol/server-filesystem", "C:\\work"], // only C:\work
     },

-    /* GitHub: token comes from the environment */
-    "github": {
-      "command": "docker",
-      "args": ["run", "-i", "--rm", "ghcr.io/github/github-mcp-server"],
-      "env": { "GITHUB_PERSONAL_ACCESS_TOKEN": "${env:GH_TOKEN}" },
-      "disabled": false,
-    },
     "playwright": { "command": "npx", "args": ["@playwright/mcp@latest"] }, // last one
   },
   "disabledMcpjsonServers": [
"#
    );
}

#[test]
fn remove_of_the_last_entry_takes_its_trailing_comment() {
    let edit = Edit::Remove {
        key_path: key("mcpServers.playwright"),
    };

    assert_eq!(
        diff(MCP_JSONC, &edit),
        r#"--- a/mcp.jsonc
+++ b/mcp.jsonc
@@ -14,7 +14,6 @@
       "env": { "GITHUB_PERSONAL_ACCESS_TOKEN": "${env:GH_TOKEN}" },
       "disabled": false,
     },
-    "playwright": { "command": "npx", "args": ["@playwright/mcp@latest"] }, // last one
   },
   "disabledMcpjsonServers": [
     "legacy",
"#
    );
}

#[test]
fn remove_of_the_first_entry_takes_its_line_comment_and_keeps_the_blank_line_after_it() {
    let edit = Edit::Remove {
        key_path: key("mcpServers.filesystem"),
    };

    assert_eq!(
        diff(MCP_JSONC, &edit),
        r#"--- a/mcp.jsonc
+++ b/mcp.jsonc
@@ -1,11 +1,6 @@
 // User MCP config, edited by hand
 {
   "mcpServers": {
-    // Filesystem access for the monorepo
-    "filesystem": {
-      "command": "npx",
-      "args": ["-y", "@modelcontextprotocol/server-filesystem", "C:\\work"], // only C:\work
-    },

     /* GitHub: token comes from the environment */
     "github": {
"#
    );
}

#[test]
fn remove_keeps_a_section_comment_separated_by_a_blank_line() {
    let text = b"{\n  // Servers\n\n  // About a\n  \"a\": 1,\n  \"b\": 2\n}\n";
    let edit = Edit::Remove { key_path: key("a") };

    assert_eq!(
        String::from_utf8(apply(text, &edit).bytes).unwrap(),
        "{\n  // Servers\n\n  \"b\": 2\n}\n"
    );
}

#[test]
fn remove_in_a_crlf_file_with_a_bom_keeps_both() {
    let text = with_bom(&crlf(MCP_JSONC));
    let edit = Edit::Remove {
        key_path: key("mcpServers.github"),
    };

    let expected = with_bom(&crlf(&apply(MCP_JSONC, &edit).bytes));
    assert_eq!(apply(&text, &edit).bytes, expected);
}

#[test]
fn the_undo_of_a_remove_is_an_insert_that_brings_back_the_value_and_its_comment() {
    let edit = Edit::Remove {
        key_path: key("mcpServers.github"),
    };
    let applied = apply(MCP_JSONC, &edit);
    let github = json!({
        "command": "docker",
        "args": ["run", "-i", "--rm", "ghcr.io/github/github-mcp-server"],
        "env": { "GITHUB_PERSONAL_ACCESS_TOKEN": "${env:GH_TOKEN}" },
        "disabled": false
    });

    assert_eq!(
        applied.inverse,
        Edit::Insert {
            key_path: key("mcpServers.github"),
            value: github.clone(),
            after: Some("filesystem".into()),
            inline: false,
            comments: vec!["/* GitHub: token comes from the environment */".into()],
        }
    );
    assert_eq!(
        applied.depends_on,
        vec![Dependency {
            key_path: key("mcpServers.github"),
            before: Some(github),
            after: None,
        }]
    );

    let undone = apply(&applied.bytes, &applied.inverse);
    let original = EditableFile::decode(MCP_JSONC, Syntax::Jsonc).unwrap();
    let undone_file = EditableFile::decode(&undone.bytes, Syntax::Jsonc).unwrap();
    assert_eq!(
        undone_file.value_at(&[]).unwrap(),
        original.value_at(&[]).unwrap()
    );
    assert_eq!(undone.inverse, edit);
    let undone_text = String::from_utf8(undone.bytes).unwrap();
    assert!(
        undone_text.contains(
            "    },\n    /* GitHub: token comes from the environment */\n    \"github\": {\n      \"command\": \"docker\",\n"
        ),
        "{undone_text}"
    );
}

#[test]
fn remove_of_a_missing_key_is_refused() {
    let file = EditableFile::decode(MCP_JSONC, Syntax::Jsonc).unwrap();
    assert_eq!(
        file.apply(&Edit::Remove {
            key_path: key("mcpServers.nope")
        })
        .unwrap_err(),
        EditError::NotFound {
            key_path: key("mcpServers.nope")
        }
    );
}

#[test]
fn set_of_an_existing_field_changes_one_token() {
    let edit = Edit::Set {
        key_path: key("mcpServers.github.disabled"),
        value: json!(true),
    };

    assert_eq!(
        diff(MCP_JSONC, &edit),
        r#"--- a/mcp.jsonc
+++ b/mcp.jsonc
@@ -12,7 +12,7 @@
       "command": "docker",
       "args": ["run", "-i", "--rm", "ghcr.io/github/github-mcp-server"],
       "env": { "GITHUB_PERSONAL_ACCESS_TOKEN": "${env:GH_TOKEN}" },
-      "disabled": false,
+      "disabled": true,
     },
     "playwright": { "command": "npx", "args": ["@playwright/mcp@latest"] }, // last one
   },
"#
    );
}

#[test]
fn set_of_a_missing_field_adds_one_line_with_the_files_trailing_comma() {
    let edit = Edit::Set {
        key_path: key("mcpServers.filesystem.disabled"),
        value: json!(true),
    };

    assert_eq!(
        diff(MCP_JSONC, &edit),
        r#"--- a/mcp.jsonc
+++ b/mcp.jsonc
@@ -5,6 +5,7 @@
     "filesystem": {
       "command": "npx",
       "args": ["-y", "@modelcontextprotocol/server-filesystem", "C:\\work"], // only C:\work
+      "disabled": true,
     },

     /* GitHub: token comes from the environment */
"#
    );
}

#[test]
fn set_of_a_missing_field_in_a_file_without_trailing_commas_adds_none() {
    let text = b"{\n  \"a\": {\n    \"command\": \"npx\"\n  }\n}\n";
    let edit = Edit::Set {
        key_path: key("a.disabled"),
        value: json!(true),
    };

    assert_eq!(
        String::from_utf8(apply(text, &edit).bytes).unwrap(),
        "{\n  \"a\": {\n    \"command\": \"npx\",\n    \"disabled\": true\n  }\n}\n"
    );
}

#[test]
fn a_single_line_object_stays_single_line_when_a_field_is_added() {
    let edit = Edit::Set {
        key_path: key("mcpServers.playwright.disabled"),
        value: json!(true),
    };

    assert_eq!(
        diff(MCP_JSONC, &edit),
        r#"--- a/mcp.jsonc
+++ b/mcp.jsonc
@@ -14,7 +14,7 @@
       "env": { "GITHUB_PERSONAL_ACCESS_TOKEN": "${env:GH_TOKEN}" },
       "disabled": false,
     },
-    "playwright": { "command": "npx", "args": ["@playwright/mcp@latest"] }, // last one
+    "playwright": { "command": "npx", "args": ["@playwright/mcp@latest"], "disabled": true }, // last one
   },
   "disabledMcpjsonServers": [
     "legacy",
"#
    );
}

#[test]
fn a_single_line_object_keeps_its_trailing_comma_when_a_field_is_added() {
    let text = b"{ \"a\": { \"command\": \"npx\", } }\n";
    let edit = Edit::Set {
        key_path: key("a.env"),
        value: json!({ "TOKEN": "${env:TOKEN}" }),
    };

    assert_eq!(
        String::from_utf8(apply(text, &edit).bytes).unwrap(),
        "{ \"a\": { \"command\": \"npx\", \"env\": { \"TOKEN\": \"${env:TOKEN}\" }, } }\n"
    );
}

#[test]
fn append_adds_one_line_to_a_list() {
    let edit = Edit::Append {
        key_path: key("disabledMcpjsonServers"),
        value: json!("github"),
    };

    assert_eq!(
        diff(MCP_JSONC, &edit),
        r#"--- a/mcp.jsonc
+++ b/mcp.jsonc
@@ -18,5 +18,6 @@
   },
   "disabledMcpjsonServers": [
     "legacy",
+    "github",
   ],
 }
"#
    );
}

#[test]
fn append_to_a_single_line_list_stays_on_its_line() {
    let text = b"{ \"disabled\": [\"a\"] }\n";
    let edit = Edit::Append {
        key_path: key("disabled"),
        value: json!("b"),
    };

    assert_eq!(
        String::from_utf8(apply(text, &edit).bytes).unwrap(),
        "{ \"disabled\": [\"a\", \"b\"] }\n"
    );
}

#[test]
fn set_in_a_crlf_file_inserts_a_crlf_line() {
    let edit = Edit::Set {
        key_path: key("mcpServers.filesystem.disabled"),
        value: json!(true),
    };

    assert_eq!(
        apply(&crlf(MCP_JSONC), &edit).bytes,
        crlf(&apply(MCP_JSONC, &edit).bytes)
    );
}

#[test]
fn undoing_each_edit_gives_back_the_original_bytes() {
    let edits = [
        Edit::Set {
            key_path: key("mcpServers.github.disabled"),
            value: json!(true),
        },
        Edit::Set {
            key_path: key("mcpServers.filesystem.disabled"),
            value: json!(true),
        },
        Edit::Set {
            key_path: key("mcpServers.playwright.disabled"),
            value: json!(true),
        },
        Edit::Append {
            key_path: key("disabledMcpjsonServers"),
            value: json!("github"),
        },
    ];
    for edit in edits {
        let applied = apply(MCP_JSONC, &edit);
        let undone = apply(&applied.bytes, &applied.inverse);
        assert_eq!(
            String::from_utf8(undone.bytes).unwrap(),
            String::from_utf8(MCP_JSONC.to_vec()).unwrap(),
            "{edit:?}"
        );
    }
}

#[test]
fn inverses_and_dependencies_of_set_and_list_edits() {
    let cases = [
        (
            Edit::Set {
                key_path: key("mcpServers.github.disabled"),
                value: json!(true),
            },
            Edit::Set {
                key_path: key("mcpServers.github.disabled"),
                value: json!(false),
            },
            Some(json!(false)),
            Some(json!(true)),
        ),
        (
            Edit::Set {
                key_path: key("mcpServers.filesystem.disabled"),
                value: json!(true),
            },
            Edit::Remove {
                key_path: key("mcpServers.filesystem.disabled"),
            },
            None,
            Some(json!(true)),
        ),
        (
            Edit::Append {
                key_path: key("disabledMcpjsonServers"),
                value: json!("github"),
            },
            Edit::RemoveItem {
                key_path: key("disabledMcpjsonServers"),
                value: json!("github"),
            },
            Some(json!(["legacy"])),
            Some(json!(["legacy", "github"])),
        ),
    ];
    for (edit, inverse, before, after) in cases {
        let applied = apply(MCP_JSONC, &edit);
        assert_eq!(applied.inverse, inverse);
        assert_eq!(
            applied.depends_on,
            vec![Dependency {
                key_path: edit.key_path().to_vec(),
                before,
                after,
            }]
        );
    }
}

#[test]
fn an_edit_that_would_change_lines_outside_its_span_is_refused() {
    // jsonc-parser drops the blank line under `{` of an object it appends to,
    // which is not part of the gap the new server goes in.
    // The same without a final newline, which the line check makes up for.
    let text = "{\n  \"mcpServers\": {\n\n    \"a\": { \"command\": \"x\" },\n    \"b\": { \"command\": \"y\" }\n  }\n}";
    for text in [format!("{text}\n"), text.to_owned()] {
        let file = EditableFile::decode(text.as_bytes(), Syntax::Jsonc).unwrap();

        let error = file
            .apply(&Edit::Set {
                key_path: key("mcpServers.c"),
                value: json!({ "command": "z" }),
            })
            .unwrap_err();

        assert_eq!(error, EditError::OutsideEntry { line: 3 });
    }
}

#[test]
fn remove_item_takes_its_line_and_its_inverse_puts_it_back() {
    let text = b"{\n  \"disabled\": [\n    \"a\",\n    \"b\",\n  ],\n}\n";
    let edit = Edit::RemoveItem {
        key_path: key("disabled"),
        value: json!("b"),
    };

    let applied = apply(text, &edit);
    assert_eq!(
        String::from_utf8(applied.bytes.clone()).unwrap(),
        "{\n  \"disabled\": [\n    \"a\",\n  ],\n}\n"
    );
    assert_eq!(
        applied.inverse,
        Edit::Append {
            key_path: key("disabled"),
            value: json!("b"),
        }
    );
    assert_eq!(apply(&applied.bytes, &applied.inverse).bytes, text);
}

#[test]
fn the_undo_of_a_remove_writes_a_single_line_object_on_one_line_again() {
    let applied = apply(
        MCP_JSONC,
        &Edit::Remove {
            key_path: key("mcpServers.playwright"),
        },
    );

    let undone = String::from_utf8(apply(&applied.bytes, &applied.inverse).bytes).unwrap();

    assert!(
        undone.contains(
            "    },\n    \"playwright\": { \"command\": \"npx\", \"args\": [\"@playwright/mcp@latest\"] },\n  },\n"
        ),
        "{undone}"
    );
}

#[test]
fn insert_into_a_single_line_object_goes_after_its_sibling() {
    let text = b"{ \"servers\": { \"a\": 1, \"c\": 3 } }\n";
    let edit = Edit::Insert {
        key_path: key("servers.b"),
        value: json!(2),
        after: Some("a".into()),
        inline: true,
        comments: Vec::new(),
    };

    assert_eq!(
        String::from_utf8(apply(text, &edit).bytes).unwrap(),
        "{ \"servers\": { \"a\": 1, \"b\": 2, \"c\": 3 } }\n"
    );
}

#[test]
fn remove_item_from_a_single_line_list_keeps_its_spacing() {
    let text = b"{ \"args\": [\"-y\", \"pkg\", \"--flag\"] }\n";
    let remove = |value: &str| Edit::RemoveItem {
        key_path: key("args"),
        value: json!(value),
    };

    assert_eq!(
        String::from_utf8(apply(text, &remove("-y")).bytes).unwrap(),
        "{ \"args\": [\"pkg\", \"--flag\"] }\n"
    );
    assert_eq!(
        String::from_utf8(apply(text, &remove("--flag")).bytes).unwrap(),
        "{ \"args\": [\"-y\", \"pkg\"] }\n"
    );
}

#[test]
fn remove_of_the_first_key_of_a_single_line_object_keeps_its_spacing() {
    let text = b"{\"a\": 1, \"b\": 2}\n";

    assert_eq!(
        String::from_utf8(apply(text, &Edit::Remove { key_path: key("a") }).bytes).unwrap(),
        "{\"b\": 2}\n"
    );
}
