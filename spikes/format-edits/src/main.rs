//! Throwaway spike for issue #15. Run with `cargo run` from this directory.
//!
//! Every operation starts from the pristine fixture, so each printed diff shows
//! exactly what one edit changes.

use jsonc_parser::ParseOptions;
use jsonc_parser::cst::CstRootNode;
use jsonc_parser::json;
use similar::TextDiff;
use toml_edit::{DocumentMut, value};

const MCP_JSONC: &str = include_str!("../fixtures/mcp.jsonc");
const CONFIG_TOML: &str = include_str!("../fixtures/config.toml");

const INLINE_TOML: &str = r#"[mcp_servers]
# Linear (hosted)
linear = { url = "https://mcp.linear.app/mcp" } # oauth
sentry = { command = "npx", args = ["@sentry/mcp-server"] }
"#;

/// Git on Windows may check fixtures out with CRLF; the LF baseline is explicit.
fn lf(s: &str) -> String {
    s.replace("\r\n", "\n")
}

fn crlf(s: &str) -> String {
    lf(s).replace('\n', "\r\n")
}

fn line_endings(s: &str) -> String {
    let crlf = s.matches("\r\n").count();
    let lf_only = s.matches('\n').count() - crlf;
    format!("CRLF={crlf} LF={lf_only} BOM={}", s.starts_with('\u{FEFF}'))
}

fn show(title: &str, before: &str, after: &str) {
    println!("\n### {title}");
    println!("before: {} | after: {}", line_endings(before), line_endings(after));
    if before == after {
        println!("(byte-identical)");
        return;
    }
    let diff = TextDiff::from_lines(before, after);
    // Show CR explicitly so CRLF changes are visible in the report.
    let text = diff
        .unified_diff()
        .context_radius(2)
        .header("a/before", "b/after")
        .to_string()
        .replace('\r', "\\r");
    print!("```diff\n{text}```\n");
}

// ---------- JSONC (jsonc-parser CST) ----------

fn jsonc_parse(text: &str) -> CstRootNode {
    CstRootNode::parse(text, &ParseOptions::default()).expect("parse jsonc")
}

fn jsonc_remove_server(text: &str, name: &str) -> String {
    let root = jsonc_parse(text);
    let servers = root.object_value().unwrap().object_value("mcpServers").unwrap();
    servers.get(name).expect("server exists").remove();
    root.to_string()
}

/// Variant of `jsonc_remove_server` that also removes comment lines attached
/// directly above the property (no blank line in between). jsonc-parser's own
/// `remove()` leaves those comments behind.
fn jsonc_remove_server_with_comments(text: &str, name: &str) -> String {
    use jsonc_parser::cst::{CstLeafNode, CstNode};
    let root = jsonc_parse(text);
    let servers = root.object_value().unwrap().object_value("mcpServers").unwrap();
    let prop = servers.get(name).expect("server exists");

    let prev: Vec<CstNode> = prop.previous_siblings().collect(); // nearest first
    let mut i = 0;
    while i < prev.len() && prev[i].is_whitespace() {
        i += 1; // indentation of the property's own line
    }
    let mut groups: Vec<Vec<CstNode>> = Vec::new();
    if i < prev.len() && prev[i].is_newline() {
        i += 1;
        loop {
            // Expect, walking backwards: [ws] comment [ws] newline
            let start = i;
            let mut j = i;
            while j < prev.len() && prev[j].is_whitespace() {
                j += 1;
            }
            if !(j < prev.len() && prev[j].is_comment()) {
                break;
            }
            let mut k = j + 1;
            while k < prev.len() && prev[k].is_whitespace() {
                k += 1;
            }
            if !(k < prev.len() && prev[k].is_newline()) {
                break; // comment shares its line with code: not attached to us
            }
            groups.push(prev[start..=k].to_vec());
            i = k + 1;
        }
    }
    for group in groups {
        // Remove the comment first: for `//` comments jsonc-parser also removes
        // the indentation and newline before it, which are in the group anyway.
        for node in group.iter().filter(|n| n.is_comment()) {
            if let CstNode::Leaf(CstLeafNode::Comment(c)) = node.clone() {
                c.remove();
            }
        }
        for node in group.into_iter().filter(|n| !n.is_comment()) {
            node.remove(); // no-op when already detached
        }
    }
    prop.remove();
    root.to_string()
}

fn jsonc_set_disabled(text: &str, name: &str, disabled: bool) -> String {
    let root = jsonc_parse(text);
    let server = root
        .object_value()
        .unwrap()
        .object_value("mcpServers")
        .unwrap()
        .object_value(name)
        .expect("server is an object");
    match server.get("disabled") {
        Some(prop) => prop.set_value(json!(disabled)),
        None => {
            server.append("disabled", json!(disabled));
        }
    }
    root.to_string()
}

fn jsonc_append_to_list(text: &str, list: &str, item: &str) -> String {
    let root = jsonc_parse(text);
    root.object_value_or_set()
        .array_value_or_set(list)
        .append(json!(item));
    root.to_string()
}

// ---------- TOML (toml_edit) ----------

fn toml_parse(text: &str) -> DocumentMut {
    text.parse::<DocumentMut>().expect("parse toml")
}

fn toml_remove_server(text: &str, name: &str) -> String {
    let mut doc = toml_parse(text);
    doc["mcp_servers"]
        .as_table_like_mut()
        .unwrap()
        .remove(name)
        .expect("server exists");
    doc.to_string()
}

fn toml_set_enabled(text: &str, name: &str, enabled: bool) -> String {
    let mut doc = toml_parse(text);
    doc["mcp_servers"][name]["enabled"] = value(enabled);
    doc.to_string()
}

/// Variant of `toml_remove_server` that keeps the part of the removed table's
/// leading comment block that is separated from its header by a blank line
/// (a section comment), moving it onto the next table in document order.
fn toml_remove_server_keep_section_comment(text: &str, name: &str) -> String {
    let mut doc = toml_parse(text);
    let removed = doc["mcp_servers"]
        .as_table_like_mut()
        .unwrap()
        .remove(name)
        .expect("server exists");
    let Some(table) = removed.as_table() else {
        return doc.to_string();
    };
    let prefix = table.decor().prefix().and_then(|p| p.as_str()).unwrap_or("");
    // Everything up to and including the last blank line is not "about" this table.
    let keep = match prefix.rfind("\n\n") {
        Some(idx) => prefix[..idx + 2].to_string(),
        None => String::new(),
    };
    let removed_pos = table.position().unwrap_or(isize::MAX);
    if keep.trim().is_empty() {
        return doc.to_string();
    }

    // Pass 1: find the position of the next explicit table in document order.
    fn next_pos(t: &toml_edit::Table, after: isize, best: &mut Option<isize>) {
        for (_, item) in t.iter() {
            if let toml_edit::Item::Table(sub) = item {
                if let Some(p) = sub.position().filter(|p| *p > after && !sub.is_implicit()) {
                    *best = Some(best.map_or(p, |b| b.min(p)));
                }
                next_pos(sub, after, best);
            }
        }
    }
    // Pass 2: prepend the kept comment block to that table's header decor.
    fn prepend(t: &mut toml_edit::Table, pos: isize, keep: &str) -> bool {
        for (_, item) in t.iter_mut() {
            if let toml_edit::Item::Table(sub) = item {
                if sub.position() == Some(pos) && !sub.is_implicit() {
                    let existing = sub.decor().prefix().and_then(|p| p.as_str()).unwrap_or("").to_string();
                    sub.decor_mut().set_prefix(format!("{keep}{}", existing.trim_start_matches('\n')));
                    return true;
                }
                if prepend(sub, pos, keep) {
                    return true;
                }
            }
        }
        false
    }
    let mut best = None;
    next_pos(doc.as_table(), removed_pos, &mut best);
    if let Some(pos) = best {
        prepend(doc.as_table_mut(), pos, &keep);
    }
    doc.to_string()
}

/// Mitigation for toml_edit's CRLF handling: edit an LF copy, then restore CRLF
/// when the original used it consistently.
fn with_line_endings(original: &str, edit: impl Fn(&str) -> String) -> String {
    let had_crlf = original.contains("\r\n");
    let out = edit(&lf(original));
    if had_crlf { out.replace('\n', "\r\n") } else { out }
}

/// `cargo run -- roundtrip <files...>`: parse + print each file without editing
/// and report only whether the bytes survived (never prints file contents).
fn roundtrip(paths: &[String]) {
    for path in paths {
        let Ok(bytes) = std::fs::read(path) else {
            println!("{path}: missing");
            continue;
        };
        let Ok(text) = String::from_utf8(bytes.clone()) else {
            println!("{path}: not UTF-8");
            continue;
        };
        let body = text.strip_prefix('\u{FEFF}').unwrap_or(&text);
        let result = if path.ends_with(".toml") {
            body.parse::<DocumentMut>().map(|d| d.to_string()).map_err(|e| e.to_string())
        } else {
            CstRootNode::parse(body, &ParseOptions::default()).map(|r| r.to_string()).map_err(|e| e.to_string())
        };
        match result {
            Ok(out) => println!(
                "{path}: {} bytes, {}, identical after round-trip = {}",
                bytes.len(),
                line_endings(&text),
                out == body
            ),
            Err(e) => println!("{path}: parse error: {e}"),
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.first().map(String::as_str) == Some("roundtrip") {
        roundtrip(&args[1..]);
        return;
    }
    let jsonc = lf(MCP_JSONC);
    let toml = lf(CONFIG_TOML);

    println!("## JSONC (jsonc-parser {} CST)", "0.34.0");

    show("J0 parse + print, no edit", &jsonc, &jsonc_parse(&jsonc).to_string());
    show("J1 Remove middle server `github` (block comment above it)", &jsonc, &jsonc_remove_server(&jsonc, "github"));
    show("J2 Remove last server `playwright` (trailing comma + trailing comment)", &jsonc, &jsonc_remove_server(&jsonc, "playwright"));
    show("J3 Remove first server `filesystem` (line comment above it)", &jsonc, &jsonc_remove_server(&jsonc, "filesystem"));
    show("J4 Toggle existing flag `github.disabled` false -> true", &jsonc, &jsonc_set_disabled(&jsonc, "github", true));
    show("J5 Add missing flag `filesystem.disabled = true`", &jsonc, &jsonc_set_disabled(&jsonc, "filesystem", true));
    show("J6 Add missing flag on single-line object `playwright`", &jsonc, &jsonc_set_disabled(&jsonc, "playwright", true));
    show("J7 Append to list `disabledMcpjsonServers`", &jsonc, &jsonc_append_to_list(&jsonc, "disabledMcpjsonServers", "github"));

    let jsonc_crlf = crlf(&jsonc);
    show("J8 CRLF file: Remove `github`", &jsonc_crlf, &jsonc_remove_server(&jsonc_crlf, "github"));
    show("J9 CRLF file: Add missing flag `filesystem.disabled`", &jsonc_crlf, &jsonc_set_disabled(&jsonc_crlf, "filesystem", true));

    let jsonc_bom = format!("\u{FEFF}{jsonc}");
    println!("\n### J10 UTF-8 BOM");
    match CstRootNode::parse(&jsonc_bom, &ParseOptions::default()) {
        Ok(root) => println!("parse with BOM: ok, round-trip identical = {}", root.to_string() == jsonc_bom),
        Err(e) => println!("parse with BOM: ERROR {e}"),
    }
    let stripped = jsonc_bom.strip_prefix('\u{FEFF}').unwrap();
    let restored = format!("\u{FEFF}{}", jsonc_remove_server(stripped, "github"));
    println!("strip BOM, Remove `github`, re-add BOM: starts with BOM = {}", restored.starts_with('\u{FEFF}'));

    show(
        "J11 Remove `github` and its attached comment line (helper on top of CST)",
        &jsonc,
        &jsonc_remove_server_with_comments(&jsonc, "github"),
    );
    show(
        "J12 Remove `filesystem` and its attached comment line (helper on top of CST)",
        &jsonc,
        &jsonc_remove_server_with_comments(&jsonc, "filesystem"),
    );
    show(
        "J13 Remove `playwright` with helper (no comment above: same as J2)",
        &jsonc,
        &jsonc_remove_server_with_comments(&jsonc, "playwright"),
    );
    let jsonc_crlf = crlf(&jsonc);
    show(
        "J14 CRLF file: Remove `github` and its attached comment line",
        &jsonc_crlf,
        &jsonc_remove_server_with_comments(&jsonc_crlf, "github"),
    );

    println!("\n## TOML (toml_edit 0.25.15)");

    show("T0 parse + print, no edit", &toml, &toml_parse(&toml).to_string());
    show("T1 Remove `[mcp_servers.context7]` (and its `.env` sub-table)", &toml, &toml_remove_server(&toml, "context7"));
    show("T2 Remove `[mcp_servers.github]` (last server, followed by another table)", &toml, &toml_remove_server(&toml, "github"));
    show("T3 Disable `github` by adding `enabled = false`", &toml, &toml_set_enabled(&toml, "github", false));
    let disabled = toml_set_enabled(&toml, "github", false);
    show("T4 Re-enable: flip existing `enabled` false -> true", &disabled, &toml_set_enabled(&disabled, "github", true));
    let both_removed = toml_remove_server(&toml_remove_server(&toml, "context7"), "github");
    show("T5 Remove both servers (implicit `mcp_servers` parent left empty)", &toml, &both_removed);

    let inline = lf(INLINE_TOML);
    show("T6 Inline-table form: Remove `linear`", &inline, &toml_remove_server(&inline, "linear"));
    show("T7 Inline-table form: Remove `sentry` (last)", &inline, &toml_remove_server(&inline, "sentry"));
    show("T8 Inline-table form: Disable `sentry`", &inline, &toml_set_enabled(&inline, "sentry", false));

    let toml_crlf = crlf(&toml);
    show("T9 CRLF file: parse + print, no edit", &toml_crlf, &toml_parse(&toml_crlf).to_string());
    show("T10 CRLF file: Remove `context7`, no mitigation", &toml_crlf, &toml_remove_server(&toml_crlf, "context7"));
    show(
        "T11 CRLF file: Remove `context7`, normalise to LF then restore CRLF",
        &toml_crlf,
        &with_line_endings(&toml_crlf, |t| toml_remove_server(t, "context7")),
    );

    let toml_bom = format!("\u{FEFF}{toml}");
    println!("\n### T12 UTF-8 BOM");
    match toml_bom.parse::<DocumentMut>() {
        Ok(doc) => {
            let out = doc.to_string();
            println!(
                "parse with BOM: ok, round-trip identical = {}, output starts with BOM = {}",
                out == toml_bom,
                out.starts_with('\u{FEFF}')
            );
        }
        Err(e) => println!("parse with BOM: ERROR {e}"),
    }

    show(
        "T14 Remove `context7` keeping the section comment (helper on top of toml_edit)",
        &toml,
        &toml_remove_server_keep_section_comment(&toml, "context7"),
    );
    show(
        "T15 Remove `github` with the same helper (no section comment: same as T2)",
        &toml,
        &toml_remove_server_keep_section_comment(&toml, "github"),
    );

    let no_final_newline = toml.trim_end_matches('\n').to_string();
    show("T13 No final newline: parse + print, no edit", &no_final_newline, &toml_parse(&no_final_newline).to_string());
}
