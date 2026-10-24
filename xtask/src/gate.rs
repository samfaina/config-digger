//! The release gate over the `unverified` marks of the built-in
//! definitions: no release ships while a mark is outside the exception,
//! and the release notes list each mark under it with what its test needs.

use config_digger_core::definitions::marks::Mark;
use std::fmt::Write;

/// The marks the release gate refuses: those outside the exception, whose
/// mark does not say what its test `needs`.
pub fn refused(marks: &[Mark]) -> Vec<&Mark> {
    marks.iter().filter(|mark| mark.needs.is_none()).collect()
}

/// The section of the release notes on the marks under the exception, in
/// Markdown, or nothing when no mark is under it.
pub fn notes(marks: &[Mark]) -> String {
    let mut notes = String::new();
    for mark in marks {
        let Some(needs) = &mark.needs else { continue };
        if notes.is_empty() {
            notes.push_str(
                "## Unverified marks\n\nThese facts of the built-in definitions are untested: each test needs an account or a deployment the project does not have.\n\n",
            );
        }
        let table = if mark.location.is_empty() {
            String::new()
        } else {
            format!(" `{}`", mark.location)
        };
        let _ = writeln!(
            notes,
            "- `{}`{table}: {} Its test needs {needs}.",
            mark.file, mark.sentence
        );
    }
    notes
}

#[cfg(test)]
mod tests {
    use super::*;

    /// An Agent whose servers are in `~/.alpha.json`, with a mark outside
    /// the exception and one under it.
    const ALPHA: &str = r#"
format_version = 1

[agent]
id = "alpha"
name = "Alpha"

[[homes]]
id = "default"
folder = { block = "path", path = "~" }
roots.config = ".alpha.json"

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
on_read_failure = "unknown"
unverified = "What Alpha does with a `.alpha.json` it cannot parse is not known."

[items.mcp]
precedence = ["user-mcp"]
competition = { block = "exact-name" }
checks = ["disable", "precedence"]
unverified = { sentence = "Alpha's enterprise servers are taken to win.", needs = "an Alpha enterprise account" }
"#;

    fn marks() -> Vec<Mark> {
        config_digger_core::definitions::agent_marks("alpha.toml", ALPHA).unwrap()
    }

    #[test]
    fn the_gate_refuses_a_mark_outside_the_exception() {
        let marks = marks();

        let refused: Vec<String> = refused(&marks).iter().map(ToString::to_string).collect();

        assert_eq!(
            refused,
            [
                "alpha.toml:26: locations[user-mcp]: What Alpha does with a `.alpha.json` it cannot parse is not known."
            ]
        );
    }

    #[test]
    fn the_gate_lets_marks_under_the_exception_ship() {
        let marks: Vec<Mark> = marks().into_iter().filter(|m| m.needs.is_some()).collect();

        assert!(refused(&marks).is_empty());
    }

    #[test]
    fn the_release_notes_list_each_mark_under_the_exception_with_what_its_test_needs() {
        let notes = notes(&marks());

        assert_eq!(
            notes,
            "## Unverified marks\n\
             \n\
             These facts of the built-in definitions are untested: each test needs an account or a deployment the project does not have.\n\
             \n\
             - `alpha.toml` `items.mcp`: Alpha's enterprise servers are taken to win. Its test needs an Alpha enterprise account.\n"
        );
    }

    #[test]
    fn the_release_notes_say_nothing_without_a_mark_under_the_exception() {
        let marks: Vec<Mark> = marks().into_iter().filter(|m| m.needs.is_none()).collect();

        assert_eq!(notes(&marks), "");
    }
}
