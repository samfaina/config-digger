//! Byte-level behaviour of the edit module: round-trips, encodings, line
//! endings and final newlines, whatever the edit.

use config_digger_core::edit::{Edit, EditableFile, ReadOnly, Syntax};
use serde_json::json;

const MCP_JSONC: &[u8] = include_bytes!("fixtures/edit/mcp.jsonc");
const CONFIG_TOML: &[u8] = include_bytes!("fixtures/edit/config.toml");
const VSCODE_SETTINGS: &[u8] = include_bytes!("fixtures/edit/vscode-settings.json");
const CODEX_DEFAULTS: &[u8] = include_bytes!("fixtures/edit/codex-defaults.toml");
const CODEX_FEISKYER: &[u8] = include_bytes!("fixtures/edit/codex-feiskyer.toml");
const CODEX_PULUMI: &[u8] = include_bytes!("fixtures/edit/codex-pulumi-awsx.toml");

fn crlf(bytes: &[u8]) -> Vec<u8> {
    String::from_utf8(bytes.to_vec())
        .unwrap()
        .replace('\n', "\r\n")
        .into_bytes()
}

fn with_bom(bytes: &[u8]) -> Vec<u8> {
    [&[0xEF, 0xBB, 0xBF][..], bytes].concat()
}

fn without_final_newline(bytes: &[u8]) -> Vec<u8> {
    bytes.strip_suffix(b"\n").unwrap().to_vec()
}

fn assert_round_trips(bytes: &[u8], syntax: Syntax) {
    let file = EditableFile::decode(bytes, syntax).unwrap();
    let printed = file.reprint().unwrap();
    assert!(
        printed == bytes,
        "not byte-identical:\n{}",
        String::from_utf8_lossy(&printed)
    );
}

#[test]
fn parse_then_print_is_byte_identical_for_jsonc() {
    for bytes in [MCP_JSONC, VSCODE_SETTINGS] {
        assert_round_trips(bytes, Syntax::Jsonc);
        assert_round_trips(&crlf(bytes), Syntax::Jsonc);
        assert_round_trips(&with_bom(bytes), Syntax::Jsonc);
        assert_round_trips(&with_bom(&crlf(bytes)), Syntax::Jsonc);
        assert_round_trips(&without_final_newline(bytes), Syntax::Jsonc);
    }
}

#[test]
fn parse_then_print_is_byte_identical_for_toml() {
    for bytes in [CONFIG_TOML, CODEX_DEFAULTS, CODEX_FEISKYER, CODEX_PULUMI] {
        assert_round_trips(bytes, Syntax::Toml);
        assert_round_trips(&crlf(bytes), Syntax::Toml);
        assert_round_trips(&with_bom(bytes), Syntax::Toml);
        assert_round_trips(&with_bom(&crlf(bytes)), Syntax::Toml);
        assert_round_trips(&without_final_newline(bytes), Syntax::Toml);
    }
}

#[test]
fn files_that_cannot_be_decoded_safely_are_read_only_with_an_explanation() {
    let ansi = b"{ \"name\": \"Caf\xe9\" }\n";
    let utf16le = [0xFF, 0xFE, b'{', 0, b'}', 0];
    let utf16be_without_bom = [0, b'{', 0, b'}'];

    let cases: [(&[u8], Syntax, ReadOnly); 5] = [
        (ansi, Syntax::Jsonc, ReadOnly::Ansi),
        (ansi, Syntax::Toml, ReadOnly::TomlNotUtf8),
        (&utf16le, Syntax::Jsonc, ReadOnly::Utf16),
        (&utf16be_without_bom, Syntax::Toml, ReadOnly::Utf16),
        (&with_bom(ansi), Syntax::Jsonc, ReadOnly::InvalidUtf8),
    ];
    for (bytes, syntax, expected) in cases {
        let error = EditableFile::decode(bytes, syntax).unwrap_err();
        assert_eq!(error, expected);
        assert!(!error.to_string().is_empty());
    }
}

#[test]
fn ascii_ansi_files_are_utf8_and_editable() {
    assert!(EditableFile::decode(b"{ \"a\": 1 }", Syntax::Jsonc).is_ok());
}

fn set(bytes: &[u8], syntax: Syntax, key_path: &[&str], value: serde_json::Value) -> String {
    let edit = Edit::Set {
        key_path: key_path.iter().map(|key| key.to_string()).collect(),
        value,
    };
    let applied = EditableFile::decode(bytes, syntax)
        .unwrap()
        .apply(&edit)
        .unwrap();
    String::from_utf8(applied.bytes).unwrap()
}

#[test]
fn inserted_lines_use_the_dominant_line_ending_and_other_lines_keep_theirs() {
    let mostly_crlf = b"a = 1\r\nb = 2\n[t]\r\nx = 1\r\n";
    assert_eq!(
        set(mostly_crlf, Syntax::Toml, &["t", "y"], json!(2)),
        "a = 1\r\nb = 2\n[t]\r\nx = 1\r\ny = 2\r\n"
    );

    let mostly_lf = b"{\r\n  \"a\": {\n    \"x\": 1\n  }\n}\n";
    assert_eq!(
        set(mostly_lf, Syntax::Jsonc, &["a", "y"], json!(2)),
        "{\r\n  \"a\": {\n    \"x\": 1,\n    \"y\": 2\n  }\n}\n"
    );
}

#[test]
fn a_missing_final_newline_stays_missing() {
    assert_eq!(
        set(b"[t]\nx = 1", Syntax::Toml, &["t", "y"], json!(2)),
        "[t]\nx = 1\ny = 2"
    );
    assert_eq!(
        set(b"{\n  \"x\": 1\n}", Syntax::Jsonc, &["y"], json!(2)),
        "{\n  \"x\": 1,\n  \"y\": 2\n}"
    );
}

#[test]
fn edits_of_a_bom_file_keep_the_bom() {
    let edited = set(&with_bom(b"x = 1\n"), Syntax::Toml, &["x"], json!(2));
    assert_eq!(edited, "\u{FEFF}x = 2\n");
    let edited = set(
        &with_bom(b"{ \"x\": 1 }\n"),
        Syntax::Jsonc,
        &["x"],
        json!(2),
    );
    assert_eq!(edited, "\u{FEFF}{ \"x\": 2 }\n");
}
