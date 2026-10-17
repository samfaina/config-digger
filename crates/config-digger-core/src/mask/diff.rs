//! Masked diffs: a diff is computed on the real text of two versions of a
//! file, then each side is shown with every Secret masked, found through the
//! parser's spans across the whole file for every location that reads it.
//! Context lines and other Entries are masked too.

use crate::api::{DiffUnavailable, FileDiff};
use crate::definitions::blocks::{JsonFormat, Parse};
use crate::definitions::{AgentDefinition, LocationDefinition};
use crate::edit::{self, Syntax};
use crate::park;
use crate::scan::{self, LocationReader};
use serde_json::Value;
use similar::{DiffOp, DiffTag, TextDiff};
use std::ops::Range;
use std::path::Path;

/// How many lines of context surround each change.
const CONTEXT: usize = 3;

/// The line that follows a change whose only difference is a Secret.
const SECRET_CHANGED: &str = "\\ secret value changed\n";

/// One version of a file.
pub(crate) struct Side<'b> {
    /// Its bytes, or `None` when the file does not exist.
    pub bytes: Option<&'b [u8]>,
    /// How a sentence names this version, such as "as it is now".
    pub name: &'static str,
}

/// A diff from `before` to `after` of the file at `path`, read by `readers`,
/// with every Secret masked on both sides. It is not shown when a side does
/// not parse for one of the readers, or when no reader is left.
pub(crate) fn diff(
    definitions: &[AgentDefinition],
    readers: &[LocationReader],
    path: &Path,
    before: Side,
    after: Side,
) -> FileDiff {
    let locations: Vec<&LocationDefinition> = readers
        .iter()
        .map(|reader| definitions[reader.agent_index].location(&reader.location))
        .collect();
    masked_diff(path, before, after, &Secrets::Locations(locations))
}

/// A diff from `before` to `after` of the parked-Entry store at `path`, with
/// every Secret of each parked Entry masked on both sides, as the location it
/// was parked from masks it. Every text value of a parked Entry whose Agent
/// or location is not loaded is masked.
pub(crate) fn diff_parked(
    definitions: &[AgentDefinition],
    path: &Path,
    before: Side,
    after: Side,
) -> FileDiff {
    masked_diff(path, before, after, &Secrets::Parked(definitions))
}

/// Where the Secrets of a file are found.
enum Secrets<'d> {
    /// In the Entries of the locations that read it.
    Locations(Vec<&'d LocationDefinition>),
    /// In the Entries parked in it: it is the parked-Entry store.
    Parked(&'d [AgentDefinition]),
}

/// A diff from `before` to `after` of the file at `path`, with each
/// Secret that `secrets` finds masked on both sides.
fn masked_diff(path: &Path, before: Side, after: Side, secrets: &Secrets) -> FileDiff {
    let old = String::from_utf8_lossy(before.bytes.unwrap_or_default());
    let new = String::from_utf8_lossy(after.bytes.unwrap_or_default());
    let text_diff = TextDiff::from_lines(old.as_ref(), new.as_ref());
    let changed_lines = changed_lines(text_diff.ops());
    let unavailable = |reason, note: String| FileDiff {
        path: path.to_path_buf(),
        diff: None,
        diff_unavailable: Some(reason),
        diff_note: Some(note),
        changed_lines,
    };
    if matches!(secrets, Secrets::Locations(locations) if locations.is_empty()) {
        return unavailable(
            DiffUnavailable::NoReader,
            format!(
                "Can't show a masked diff: no Agent reads Entries from `{}`, so nothing says where its Secrets are; {}.",
                path.display(),
                lines_differ(changed_lines)
            ),
        );
    }
    let old_real: Vec<&str> = text_diff.iter_old_slices().collect();
    let new_real: Vec<&str> = text_diff.iter_new_slices().collect();
    let masked = |side: &Side, real: &[&str]| {
        masked_lines(secrets, side.bytes, real)
            .map_err(|failure| (failure.kind(), failure.note(path, side.name, changed_lines)))
    };
    let (old_lines, new_lines) = match (masked(&before, &old_real), masked(&after, &new_real)) {
        (Ok(old_lines), Ok(new_lines)) => (old_lines, new_lines),
        (Err((kind, note)), _) | (_, Err((kind, note))) => return unavailable(kind, note),
    };
    let label = path.display().to_string();
    let lines = Lines {
        old_real: &old_real,
        new_real: &new_real,
        old: &old_lines,
        new: &new_lines,
        newline_terminated: text_diff.newline_terminated(),
    };
    FileDiff {
        path: path.to_path_buf(),
        diff: Some(lines.unified(&text_diff.grouped_ops(CONTEXT), &label)),
        diff_unavailable: None,
        diff_note: None,
        changed_lines,
    }
}

/// How many lines differ: for a change, the longer of its two sides.
fn changed_lines(ops: &[DiffOp]) -> u32 {
    ops.iter()
        .map(|op| match op.tag() {
            DiffTag::Equal => 0,
            _ => op.old_range().len().max(op.new_range().len()),
        })
        .sum::<usize>()
        .try_into()
        .unwrap_or(u32::MAX)
}

/// Why a side's Secrets could not be found.
enum Unparsed {
    /// It is not UTF-8 text.
    Decode,
    /// It does not parse in a reader's dialect.
    Parse {
        dialect: &'static str,
        position: Option<(usize, usize)>,
    },
}

impl Unparsed {
    fn kind(&self) -> DiffUnavailable {
        match self {
            Unparsed::Decode => DiffUnavailable::Decode,
            Unparsed::Parse { .. } => DiffUnavailable::Parse,
        }
    }

    /// The core's own sentence for it, with the kind and `line:col` only:
    /// a parser's message can quote the text, Secrets included.
    fn note(&self, path: &Path, side: &str, changed_lines: u32) -> String {
        let why = match self {
            Unparsed::Decode => "isn't UTF-8 text".to_owned(),
            Unparsed::Parse { dialect, position } => {
                let at = match position {
                    Some((line, column)) => format!(" (parse error at {line}:{column})"),
                    None => String::new(),
                };
                format!("doesn't parse as {dialect}{at}")
            }
        };
        format!(
            "Can't show a masked diff: `{}` {side} {why}; {}.",
            path.display(),
            lines_differ(changed_lines)
        )
    }
}

/// How many lines differ, as a sentence says it.
fn lines_differ(changed_lines: u32) -> String {
    match changed_lines {
        1 => "1 line differs".to_owned(),
        n => format!("{n} lines differ"),
    }
}

/// The lines of one side, each with every Secret masked, or why its Secrets
/// cannot be found. A missing file has no lines.
fn masked_lines(
    secrets: &Secrets,
    bytes: Option<&[u8]>,
    lines: &[&str],
) -> Result<Vec<String>, Unparsed> {
    let Some(bytes) = bytes else {
        return Ok(Vec::new());
    };
    let text = std::str::from_utf8(bytes).map_err(|_| Unparsed::Decode)?;
    let mut replacements: Vec<(Range<usize>, String)> = Vec::new();
    match secrets {
        Secrets::Locations(locations) => {
            for location in locations {
                replacements.extend(secret_spans(location, locations, text)?);
            }
        }
        Secrets::Parked(definitions) => replacements.extend(parked_spans(definitions, text)?),
    }
    replacements.sort_by_key(|(range, _)| (range.start, range.end));
    replacements.dedup_by(|later, earlier| later.0.start < earlier.0.end);
    Ok(mask_lines(text, lines, &replacements))
}

/// Where each Secret of a location's Entries is in `text`, with what it is
/// masked as, those of the fields `readers`, the locations that read the
/// file, mark included.
fn secret_spans(
    location: &LocationDefinition,
    readers: &[&LocationDefinition],
    text: &str,
) -> Result<Vec<(Range<usize>, String)>, Unparsed> {
    // A folder of skill folders is no file, and holds no Secret a diff shows.
    let Some(format) = location.format.file() else {
        return Ok(Vec::new());
    };
    let unparsed = |position| Unparsed::Parse {
        dialect: format.dialect(),
        position,
    };
    let document = format
        .parse(text)
        .map_err(|error| unparsed(error.position))?;
    let syntax = format.syntax();
    let objects = scan::entries_of(location, &document).map_err(|_| unparsed(None))?;
    // Each Secret's key path from the root of the file, with its mask.
    let mut secrets = super::file_fields(location, &document);
    for object in objects {
        for found in object.entries {
            let masked = super::entry_read_by(location, readers, found.value);
            for relative in &masked.key_paths {
                let mut key_path = object.key_path.clone();
                key_path.push(found.key.clone());
                key_path.extend(relative.iter().cloned());
                let secret = lookup(&masked.value, relative)
                    .cloned()
                    .expect("a masked key path is in the value");
                secrets.push((key_path, secret));
            }
        }
    }
    let mut spans = Vec::new();
    for (key_path, secret) in secrets {
        let secret = secret.as_str().expect("a masked value is a string");
        // The format parsed the text, so its syntax finds every key.
        let span = edit::value_span(syntax, text, &key_path).ok_or_else(|| unparsed(None))?;
        spans.push((span, edit::string_literal(syntax, secret)));
    }
    Ok(spans)
}

/// Where each Secret of the Entries parked in the store whose text is `text`
/// is, with what it is masked as.
fn parked_spans(
    definitions: &[AgentDefinition],
    text: &str,
) -> Result<Vec<(Range<usize>, String)>, Unparsed> {
    let format = JsonFormat::STRICT;
    let unparsed = |position| Unparsed::Parse {
        dialect: format.dialect(),
        position,
    };
    let document = format
        .parse(text)
        .map_err(|error| unparsed(error.position))?;
    let mut secrets: Vec<(Vec<String>, Value)> = Vec::new();
    for (id, value, location) in park::raw_entries(&document, definitions) {
        let mut at = park::key_path(id);
        at.push(park::ENTRY.to_owned());
        let masked: Vec<(Vec<String>, Value)> = match location {
            Some(location) => {
                let masked = super::entry(location, value);
                masked
                    .key_paths
                    .iter()
                    .map(|relative| {
                        let secret = lookup(&masked.value, relative)
                            .cloned()
                            .expect("a masked key path is in the value");
                        (relative.clone(), secret)
                    })
                    .collect()
            }
            // Nothing says where its Secrets are, so every text is one.
            None => {
                let mut texts = Vec::new();
                text_leaves(value, &mut Vec::new(), &mut texts);
                texts
                    .into_iter()
                    .map(|relative| (relative, Value::from(super::MASK)))
                    .collect()
            }
        };
        for (relative, secret) in masked {
            let mut key_path = at.clone();
            key_path.extend(relative);
            secrets.push((key_path, secret));
        }
    }
    let mut spans = Vec::new();
    for (key_path, secret) in secrets {
        let secret = secret.as_str().expect("a masked value is a string");
        let span =
            edit::value_span(Syntax::Jsonc, text, &key_path).ok_or_else(|| unparsed(None))?;
        spans.push((span, edit::string_literal(Syntax::Jsonc, secret)));
    }
    Ok(spans)
}

/// The key paths below `value`, which is at `at`, of every text in it.
fn text_leaves(value: &Value, at: &mut Vec<String>, found: &mut Vec<Vec<String>>) {
    match value {
        Value::String(_) => found.push(at.clone()),
        Value::Array(items) => {
            for (index, item) in items.iter().enumerate() {
                at.push(index.to_string());
                text_leaves(item, at, found);
                at.pop();
            }
        }
        Value::Object(object) => {
            for (key, item) in object {
                at.push(key.clone());
                text_leaves(item, at, found);
                at.pop();
            }
        }
        _ => {}
    }
}

fn lookup<'v>(value: &'v Value, key_path: &[String]) -> Option<&'v Value> {
    key_path.iter().try_fold(value, |value, key| match value {
        Value::Object(object) => object.get(key),
        Value::Array(array) => array.get(key.parse::<usize>().ok()?),
        _ => None,
    })
}

/// The `lines` of `text`, which together are exactly `text`, with each
/// sorted, disjoint range replaced. A replacement goes on the line its range
/// starts on; the rest of a range that spans lines is dropped from the lines
/// it covers, which keep their line endings, so that the lines still match
/// the real ones one to one.
fn mask_lines(text: &str, lines: &[&str], replacements: &[(Range<usize>, String)]) -> Vec<String> {
    let mut replacements = replacements.iter().peekable();
    let mut at = 0;
    let mut start = 0;
    let mut masked_lines = Vec::with_capacity(lines.len());
    for line in lines {
        let end = start + line.len();
        let content = line.trim_end_matches(['\r', '\n']);
        let content_end = start + content.len();
        let mut masked = String::new();
        at = at.max(start);
        while let Some((range, replacement)) =
            replacements.next_if(|(range, _)| range.start < content_end)
        {
            masked.push_str(&text[at..range.start.max(at)]);
            masked.push_str(replacement);
            at = range.end;
        }
        if at < content_end {
            masked.push_str(&text[at..content_end]);
            at = content_end;
        }
        masked.push_str(&text[content_end..end]);
        masked_lines.push(masked);
        start = end;
    }
    masked_lines
}

/// Both sides of a diff, real and masked, line by line.
struct Lines<'d> {
    old_real: &'d [&'d str],
    new_real: &'d [&'d str],
    old: &'d [String],
    new: &'d [String],
    newline_terminated: bool,
}

impl Lines<'_> {
    /// A unified diff of the hunks `groups`, printed from the masked lines.
    fn unified(&self, groups: &[Vec<DiffOp>], label: &str) -> String {
        let mut out = String::new();
        for group in groups.iter().filter(|group| !group.is_empty()) {
            if out.is_empty() {
                out.push_str(&format!("--- a/{label}\n+++ b/{label}\n"));
            }
            out.push_str(&format!(
                "{}\n",
                similar::udiff::UnifiedHunkHeader::new(group)
            ));
            let mut changed: Vec<&DiffOp> = Vec::new();
            for op in group {
                if op.tag() == DiffTag::Equal {
                    self.secret_note(&changed, &mut out);
                    changed.clear();
                    for line in &self.old[op.old_range()] {
                        self.push(' ', line, &mut out);
                    }
                    continue;
                }
                changed.push(op);
                for line in &self.old[op.old_range()] {
                    if op.tag() != DiffTag::Insert {
                        self.push('-', line, &mut out);
                    }
                }
                for line in &self.new[op.new_range()] {
                    if op.tag() != DiffTag::Delete {
                        self.push('+', line, &mut out);
                    }
                }
            }
            self.secret_note(&changed, &mut out);
        }
        out
    }

    /// After a run of changes, says when one of its lines shows the same on
    /// both sides once masked: only a Secret changed there.
    fn secret_note(&self, changed: &[&DiffOp], out: &mut String) {
        let old = changed
            .iter()
            .filter(|op| op.tag() != DiffTag::Insert)
            .flat_map(|op| op.old_range());
        let new = changed
            .iter()
            .filter(|op| op.tag() != DiffTag::Delete)
            .flat_map(|op| op.new_range());
        let secret_only = old.zip(new).any(|(old, new)| {
            self.old[old] == self.new[new] && self.old_real[old] != self.new_real[new]
        });
        if secret_only {
            out.push_str(SECRET_CHANGED);
        }
    }

    /// One line of a hunk, as `similar` prints it.
    fn push(&self, tag: char, line: &str, out: &mut String) {
        out.push(tag);
        out.push_str(line);
        if !self.newline_terminated {
            out.push('\n');
        } else if !line.ends_with('\n') {
            out.push_str("\n\\ No newline at end of file\n");
        }
    }
}
