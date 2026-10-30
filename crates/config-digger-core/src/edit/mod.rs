//! Format-preserving edits of JSONC and TOML configuration files.
//!
//! An edit changes only the lines of the Entry it targets: comments, key
//! order, whitespace, encoding, BOM, line endings and the final newline of the
//! rest of the file stay as they were. Both syntaxes are edited on an LF copy
//! of the file; the file's own line endings are put back afterwards, with the
//! dominant one for inserted lines.
//!
//! Every edit is checked twice before it is returned: each changed line must
//! fall inside the lines the edit may touch, and the edited file must parse to
//! exactly the values the edit asked for. For Undo (ADR 0003), every edit
//! comes with its inverse and with the keys it depends on.

mod jsonc;
mod toml;

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use similar::{Algorithm, DiffTag, TextDiff};
use std::borrow::Cow;
use std::ops::Range;

/// The syntax a file is edited in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Syntax {
    /// JSON with comments and trailing commas, which also covers strict JSON.
    Jsonc,
    Toml,
}

/// One change to one key of a file. Key paths are in the file's own
/// spelling, as in an Entry's key path.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Edit {
    /// Removes the key, with the comment lines directly above it.
    Remove { key_path: Vec<String> },
    /// Sets the key's value, adding the key at the end of its object or table
    /// when it is missing. In TOML, an object added under a table that has no
    /// header of its own becomes a `[table]`.
    Set { key_path: Vec<String>, value: Value },
    /// Adds a missing key right after its sibling `after`: first when `after`
    /// is `None`, last when that sibling is gone. `comments`, in the file's
    /// own syntax, go on their own lines above it, unless it goes in an
    /// object or table written on one line, where they are left out.
    /// `inline` writes an object on one line: a JSONC object on a single
    /// line, or a TOML inline table instead of a `[table]`.
    ///
    /// `emptied_object` is set on the inverse of a JSONC Remove that took
    /// the last key of its object: the object's text from its `{` to its `}`
    /// as it was read, with the key's comments. While the object is still
    /// `{}`, that text replaces it, so the object comes back as it was and
    /// the rule above on comments does not apply. Once the object holds
    /// another key, the key is added as above.
    Insert {
        key_path: Vec<String>,
        value: Value,
        after: Option<String>,
        inline: bool,
        comments: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        emptied_object: Option<String>,
    },
    /// Appends a value to the list at the key. With [`Edit::RemoveItem`], its
    /// inverse, it treats the list as a set of names, such as a list of
    /// disabled servers, whose order does not matter.
    Append { key_path: Vec<String>, value: Value },
    /// Removes the last item equal to `value` from the list at the key. Its
    /// inverse appends the item back at the end.
    RemoveItem { key_path: Vec<String>, value: Value },
}

impl Edit {
    /// The key the edit changes.
    pub fn key_path(&self) -> &[String] {
        match self {
            Edit::Remove { key_path }
            | Edit::Set { key_path, .. }
            | Edit::Insert { key_path, .. }
            | Edit::Append { key_path, .. }
            | Edit::RemoveItem { key_path, .. } => key_path,
        }
    }

    /// The edit, writing no comment: an Insert puts back an emptied
    /// object's text only when that text holds no comment.
    pub fn without_comments(self) -> Edit {
        match self {
            Edit::Insert {
                key_path,
                value,
                after,
                inline,
                emptied_object,
                ..
            } => Edit::Insert {
                key_path,
                value,
                after,
                inline,
                comments: Vec::new(),
                emptied_object: emptied_object.filter(|text| !jsonc::holds_comments(text)),
            },
            edit => edit,
        }
    }
}

/// An edit computed on a file, ready to be written.
#[derive(Debug, Clone)]
pub struct Applied {
    /// The whole file after the edit.
    pub bytes: Vec<u8>,
    /// The edit that undoes this one, applied to the file as it is then.
    pub inverse: Edit,
    /// The keys the edit depends on. The edit applies as previewed while they
    /// hold their `before` values, and its inverse while they hold their
    /// `after` values.
    pub depends_on: Vec<Dependency>,
    before: String,
    after: String,
}

impl Applied {
    /// A unified diff of the file before and after the edit, as text, with
    /// `label` as the file name in its header.
    pub fn unified_diff(&self, label: &str) -> String {
        unified_diff(&self.before, &self.after, label)
    }
}

/// The span of the value at `key_path` in a file's text, where an array
/// item's key is its index: `None` when the text does not parse or holds
/// no value there.
pub(crate) fn value_span(syntax: Syntax, text: &str, key_path: &[String]) -> Option<Range<usize>> {
    match syntax {
        Syntax::Jsonc => jsonc::value_span(text, key_path),
        Syntax::Toml => toml::value_span(text, key_path),
    }
}

/// A string value as a syntax writes it.
pub(crate) fn string_literal(syntax: Syntax, text: &str) -> String {
    match syntax {
        Syntax::Jsonc => Value::String(text.to_owned()).to_string(),
        Syntax::Toml => toml_edit::Value::from(text).to_string(),
    }
}

fn unified_diff(before: &str, after: &str, label: &str) -> String {
    TextDiff::from_lines(before, after)
        .unified_diff()
        .header(&format!("a/{label}"), &format!("b/{label}"))
        .to_string()
}

/// A key an edit depends on, with its value before and after the edit
/// (`None` when the key is missing).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Dependency {
    pub key_path: Vec<String>,
    pub before: Option<Value>,
    pub after: Option<Value>,
}

/// What a syntax backend made of an edit, on the LF copy of the file.
struct Change {
    text: String,
    inverse: Edit,
    /// The lines of the file before the edit that it may change, as 0-based
    /// line ranges. An insertion may land at either end of a range.
    allowed: Vec<Range<usize>>,
}

/// A configuration file decoded for editing.
#[derive(Debug, Clone)]
pub struct EditableFile {
    syntax: Syntax,
    bom: bool,
    /// The text after the BOM, with the file's own line endings.
    text: String,
}

const BOM: &str = "\u{FEFF}";

impl EditableFile {
    /// Decodes a file's bytes, or says why the file can only be read.
    pub fn decode(bytes: &[u8], syntax: Syntax) -> Result<Self, ReadOnly> {
        // A UTF-16 or UTF-32 BOM, or, without one, a NUL byte among the first
        // two, which the first character of a configuration file never has.
        if bytes.starts_with(&[0xFF, 0xFE])
            || bytes.starts_with(&[0xFE, 0xFF])
            || bytes.starts_with(&[0, 0, 0xFE, 0xFF])
            || bytes.iter().take(2).any(|byte| *byte == 0)
        {
            return Err(ReadOnly::Utf16);
        }
        let text = match std::str::from_utf8(bytes) {
            Ok(text) => text,
            Err(_) if bytes.starts_with(BOM.as_bytes()) => return Err(ReadOnly::InvalidUtf8),
            Err(_) => {
                return Err(match syntax {
                    Syntax::Jsonc => ReadOnly::Ansi,
                    Syntax::Toml => ReadOnly::TomlNotUtf8,
                });
            }
        };
        let (bom, text) = match text.strip_prefix(BOM) {
            Some(text) => (true, text),
            None => (false, text),
        };
        Ok(Self {
            syntax,
            bom,
            text: text.to_owned(),
        })
    }

    /// Parses the file and prints it again without editing it.
    pub fn reprint(&self) -> Result<Vec<u8>, EditError> {
        let lf = to_lf(&self.text);
        let printed = match self.syntax {
            Syntax::Jsonc => jsonc::reprint(&lf)?,
            Syntax::Toml => toml::reprint(&lf)?,
        };
        Ok(self.encode(&self.with_own_line_endings(&lf, printed)))
    }

    /// The value at a key, `None` when the key is missing.
    pub fn value_at(&self, key_path: &[String]) -> Result<Option<Value>, EditError> {
        Ok(lookup(&self.value(&to_lf(&self.text))?, key_path).cloned())
    }

    /// Computes an edit without writing anything. It is refused when it would
    /// change a line outside what it targets, or when the edited file would
    /// not hold exactly the values asked for.
    pub fn apply(&self, edit: &Edit) -> Result<Applied, EditError> {
        let lf = to_lf(&self.text);
        let before_value = self.value(&lf)?;
        let change = match self.syntax {
            Syntax::Jsonc => jsonc::apply(&lf, edit)?,
            Syntax::Toml => toml::apply(&lf, edit)?,
        };
        let after = self.with_own_line_endings(&lf, change.text);
        let lf_after = to_lf(&after);
        check_lines(&lf, &lf_after, &change.allowed)?;
        let after_value = self.value(&lf_after)?;
        let mut expected = expected(&before_value, edit).ok_or(EditError::Unverified)?;
        let mut actual = after_value.clone();
        if self.syntax == Syntax::Toml {
            // A table left without keys, or an array of tables without
            // tables, is not written, so it is gone once the file is read
            // again.
            remove_empty_objects(&mut expected);
            remove_empty_objects(&mut actual);
        }
        if expected != actual {
            return Err(EditError::Unverified);
        }
        let key_path = edit.key_path();
        Ok(Applied {
            bytes: self.encode(&after),
            inverse: change.inverse,
            depends_on: vec![Dependency {
                key_path: key_path.to_vec(),
                before: lookup(&before_value, key_path).cloned(),
                after: lookup(&after_value, key_path).cloned(),
            }],
            before: self.text.clone(),
            after,
        })
    }

    fn value(&self, lf: &str) -> Result<Value, EditError> {
        match self.syntax {
            Syntax::Jsonc => jsonc::value(lf),
            Syntax::Toml => toml::value(lf),
        }
    }

    /// Puts back what editing an LF copy loses: the final newline's absence
    /// and the line endings of every line the edit kept.
    fn with_own_line_endings(&self, lf_before: &str, mut lf_after: String) -> String {
        if lacks_final_newline(&self.text) && lf_after.ends_with('\n') {
            lf_after.pop();
        }
        let endings = line_endings(&self.text);
        let dominant = dominant_ending(&endings);
        let before = lines(lf_before);
        let after = lines(&lf_after);
        let mut out = String::with_capacity(lf_after.len() + endings.len());
        for op in similar::capture_diff_slices(Algorithm::Myers, &before, &after) {
            for (offset, line) in after[op.new_range()].iter().enumerate() {
                let Some(content) = line.strip_suffix('\n') else {
                    out.push_str(line);
                    continue;
                };
                out.push_str(content);
                out.push_str(match op.tag() {
                    DiffTag::Equal => endings[op.old_range().start + offset],
                    _ => dominant,
                });
            }
        }
        out
    }

    fn encode(&self, text: &str) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(text.len() + BOM.len());
        if self.bom {
            bytes.extend_from_slice(BOM.as_bytes());
        }
        bytes.extend_from_slice(text.as_bytes());
        bytes
    }
}

/// Why a file can be read but not edited.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum ReadOnly {
    #[error(
        "the file is UTF-16 or UTF-32; config-digger only writes UTF-8, so it leaves this file as it is"
    )]
    Utf16,
    #[error(
        "the file is not UTF-8 and has no BOM, so it is probably in a legacy ANSI code page; config-digger cannot tell which one, so it cannot write the file back safely"
    )]
    Ansi,
    #[error(
        "the file is not UTF-8, which TOML requires; config-digger does not guess its encoding"
    )]
    TomlNotUtf8,
    #[error("the file starts with a UTF-8 BOM but is not valid UTF-8")]
    InvalidUtf8,
}

/// Why an edit was refused.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum EditError {
    #[error("the file does not parse")]
    Parse,
    #[error("`{}` does not exist", .key_path.join("."))]
    NotFound { key_path: Vec<String> },
    #[error("`{}` already exists", .key_path.join("."))]
    AlreadyExists { key_path: Vec<String> },
    #[error("`{}` is not {expected}", .key_path.join("."))]
    WrongType {
        key_path: Vec<String>,
        expected: &'static str,
    },
    #[error("the list `{}` does not hold the item to remove", .key_path.join("."))]
    ItemNotFound { key_path: Vec<String>, value: Value },
    #[error("the file's syntax cannot hold the value of `{}`", .key_path.join("."))]
    Unrepresentable { key_path: Vec<String> },
    #[error("the edit would change line {line}, outside what it targets")]
    OutsideEntry { line: usize },
    #[error("the edited file would not hold the values the edit asked for")]
    Unverified,
}

/// The parent key path and the last key, or `NotFound` for an empty path.
fn split_key_path(key_path: &[String]) -> Result<(&[String], &str), EditError> {
    match key_path.split_last() {
        Some((name, parent)) => Ok((parent, name)),
        None => Err(EditError::NotFound {
            key_path: Vec::new(),
        }),
    }
}

/// The value at `key_path` below `value`.
pub(crate) fn lookup<'v>(value: &'v Value, key_path: &[String]) -> Option<&'v Value> {
    key_path
        .iter()
        .try_fold(value, |value, key| value.as_object()?.get(key))
}

/// Gives `key_path` below `value` the value `to`, `None` taking it out. The
/// keys above it become objects when they are not.
pub(crate) fn set_at(value: &mut Option<Value>, key_path: &[String], to: &Option<Value>) {
    let Some((name, below)) = key_path.split_first() else {
        value.clone_from(to);
        return;
    };
    let mut object = match value.take() {
        Some(Value::Object(object)) => object,
        // Nothing to take out.
        other if to.is_none() => {
            *value = other;
            return;
        }
        _ => Map::new(),
    };
    let mut child = object.remove(name);
    set_at(&mut child, below, to);
    if let Some(child) = child {
        object.insert(name.clone(), child);
    }
    *value = Some(Value::Object(object));
}

/// The whole file's value once `edit` is applied to `value`.
fn expected(value: &Value, edit: &Edit) -> Option<Value> {
    let mut value = value.clone();
    let (parent, name) = split_key_path(edit.key_path()).ok()?;
    let object = parent
        .iter()
        .try_fold(&mut value, |value, key| value.as_object_mut()?.get_mut(key))?
        .as_object_mut()?;
    match edit {
        Edit::Remove { .. } => {
            object.remove(name)?;
        }
        Edit::Set { value, .. } => {
            object.insert(name.to_owned(), value.clone());
        }
        Edit::Insert { value, .. } => {
            if object.insert(name.to_owned(), value.clone()).is_some() {
                return None;
            }
        }
        Edit::Append { value, .. } => object.get_mut(name)?.as_array_mut()?.push(value.clone()),
        Edit::RemoveItem { value, .. } => {
            let list = object.get_mut(name)?.as_array_mut()?;
            let index = list.iter().rposition(|item| item == value)?;
            list.remove(index);
        }
    }
    Some(value)
}

/// Removes the empty objects and the empty lists, and so the objects
/// that held nothing else, at any depth.
fn remove_empty_objects(value: &mut Value) {
    if let Value::Object(object) = value {
        for child in object.values_mut() {
            remove_empty_objects(child);
        }
        object.retain(|_, child| match child {
            Value::Object(child) => !child.is_empty(),
            Value::Array(items) => !items.is_empty(),
            _ => true,
        });
    }
}

/// Refuses the edit when a changed line falls outside every allowed range.
/// Blank lines next to a range count as part of it.
fn check_lines(before: &str, after: &str, allowed: &[Range<usize>]) -> Result<(), EditError> {
    // In a file without a final newline, a line that becomes the last one, or
    // stops being it, gains or loses its newline without being changed, so
    // both texts are compared with one. A file the edit empties stays empty.
    let (before, after) = if lacks_final_newline(before) {
        (with_final_newline(before), with_final_newline(after))
    } else {
        (Cow::Borrowed(before), Cow::Borrowed(after))
    };
    let before = lines(&before);
    let after = lines(&after);
    let is_blank = |line: &&str| line.trim().is_empty();
    let mut widened: Vec<Range<usize>> = allowed
        .iter()
        .map(|range| {
            // A key added at the end of a file may be said to go after its
            // last line.
            let start = range.start.min(before.len());
            let mut range = start..range.end.clamp(start, before.len());
            while range.start > 0 && is_blank(&before[range.start - 1]) {
                range.start -= 1;
            }
            while range.end < before.len() && is_blank(&before[range.end]) {
                range.end += 1;
            }
            range
        })
        .collect();
    widened.sort_by_key(|range| range.start);
    let mut allowed: Vec<Range<usize>> = Vec::new();
    for range in widened {
        match allowed.last_mut() {
            Some(last) if range.start <= last.end => last.end = last.end.max(range.end),
            _ => allowed.push(range),
        }
    }
    for op in similar::capture_diff_slices(Algorithm::Myers, &before, &after) {
        if op.tag() == DiffTag::Equal {
            continue;
        }
        let old = op.old_range();
        if !allowed
            .iter()
            .any(|range| range.start <= old.start && old.end <= range.end)
        {
            return Err(EditError::OutsideEntry {
                line: old.start + 1,
            });
        }
    }
    Ok(())
}

fn lines(text: &str) -> Vec<&str> {
    text.split_inclusive('\n').collect()
}

fn lacks_final_newline(text: &str) -> bool {
    !text.is_empty() && !text.ends_with('\n')
}

fn with_final_newline(text: &str) -> Cow<'_, str> {
    if lacks_final_newline(text) {
        Cow::Owned(format!("{text}\n"))
    } else {
        Cow::Borrowed(text)
    }
}

/// The 0-based lines from the one that holds byte `start` of `text` to the
/// one that holds byte `end`.
fn line_range(text: &str, start: usize, end: usize) -> Range<usize> {
    line_of(text, start)..line_of(text, end) + 1
}

/// Where the line that holds byte `offset` of `text` starts.
fn line_start(text: &str, offset: usize) -> usize {
    text[..offset].rfind('\n').map_or(0, |newline| newline + 1)
}

/// The 0-based line that holds byte `offset` of `text`.
fn line_of(text: &str, offset: usize) -> usize {
    text[..offset].matches('\n').count()
}

fn to_lf(text: &str) -> String {
    text.replace("\r\n", "\n")
}

/// The ending of each line: `"\r\n"`, `"\n"`, or `""` for a last line without one.
fn line_endings(text: &str) -> Vec<&'static str> {
    text.split_inclusive('\n')
        .map(|line| {
            if line.ends_with("\r\n") {
                "\r\n"
            } else if line.ends_with('\n') {
                "\n"
            } else {
                ""
            }
        })
        .collect()
}

/// The more frequent of CRLF and LF, LF on a tie.
fn dominant_ending(endings: &[&'static str]) -> &'static str {
    let crlf = endings.iter().filter(|ending| **ending == "\r\n").count();
    let lf = endings.iter().filter(|ending| **ending == "\n").count();
    if crlf > lf { "\r\n" } else { "\n" }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spanned<'t>(syntax: Syntax, text: &'t str, key_path: &[&str]) -> Option<&'t str> {
        let key_path: Vec<String> = key_path.iter().map(|key| key.to_string()).collect();
        value_span(syntax, text, &key_path).map(|span| &text[span])
    }

    #[test]
    fn a_value_span_reaches_into_tables_inline_tables_and_arrays() {
        let text = "[mcp_servers.github]\nargs = [\"-y\", \"--token\", \"t\"]\nenv = { TOKEN = \"s\" }\n\n[[hooks]]\ncommand = \"c\"\n";

        assert_eq!(
            spanned(Syntax::Toml, text, &["mcp_servers", "github", "args", "2"]),
            Some("\"t\"")
        );
        assert_eq!(
            spanned(
                Syntax::Toml,
                text,
                &["mcp_servers", "github", "env", "TOKEN"]
            ),
            Some("\"s\"")
        );
        assert_eq!(
            spanned(Syntax::Toml, text, &["hooks", "0", "command"]),
            Some("\"c\"")
        );
        assert_eq!(spanned(Syntax::Toml, text, &["mcp_servers", "x"]), None);
    }

    #[test]
    fn a_value_span_reaches_into_jsonc_objects_and_arrays() {
        let text = "{\n  // servers\n  \"s\": { \"args\": [\"a\", \"t\"], },\n}\n";

        assert_eq!(
            spanned(Syntax::Jsonc, text, &["s", "args", "1"]),
            Some("\"t\"")
        );
        assert_eq!(spanned(Syntax::Jsonc, text, &["s", "env"]), None);
    }

    #[test]
    fn the_line_check_lets_a_line_become_the_last_one_but_not_change() {
        // Removing the last line of a file without a final newline.
        let before = "[t]\nx = 1\ny = 2";
        let last_line = [line_range(before, 10, 10)];

        assert_eq!(check_lines(before, "[t]\nx = 1", &last_line), Ok(()));
        assert_eq!(
            check_lines(before, "[t]\nx = 9", &last_line),
            Err(EditError::OutsideEntry { line: 2 })
        );
    }

    #[test]
    fn a_string_literal_is_written_in_the_syntax() {
        assert_eq!(string_literal(Syntax::Jsonc, "a\"b"), r#""a\"b""#);
        assert_eq!(string_literal(Syntax::Toml, "••••"), "\"••••\"");
    }
}
