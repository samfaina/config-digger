//! JSONC edits on `jsonc-parser`'s concrete syntax tree, which prints an
//! unedited tree byte for byte. The lines an edit may change come from the
//! parser's AST ranges instead, so that a mistake in either shows up as a
//! refused edit.

use super::{Change, Edit, EditError, line_of, line_range, line_start, lines, split_key_path};
use jsonc_parser::common::Ranged;
use jsonc_parser::cst::{
    CstArray, CstComment, CstInputValue, CstLeafNode, CstNode, CstObject, CstObjectProp,
    CstRootNode,
};
use jsonc_parser::{CollectOptions, ParseOptions, ast};
use serde_json::Value;
use std::ops::Range;

fn parse(text: &str) -> Result<CstRootNode, EditError> {
    CstRootNode::parse(text, &ParseOptions::default()).map_err(|_| EditError::Parse)
}

pub(super) fn reprint(text: &str) -> Result<String, EditError> {
    Ok(parse(text)?.to_string())
}

pub(super) fn value(text: &str) -> Result<Value, EditError> {
    jsonc_parser::parse_to_serde_value::<Option<Value>>(text, &ParseOptions::default())
        .ok()
        .flatten()
        .ok_or(EditError::Parse)
}

pub(super) fn apply(text: &str, edit: &Edit) -> Result<Change, EditError> {
    let root = parse(text)?;
    let ast =
        jsonc_parser::parse_to_ast(text, &CollectOptions::default(), &ParseOptions::default())
            .map_err(|_| EditError::Parse)?;
    let ast = ast.value.ok_or(EditError::Parse)?;
    let file = File { text, root, ast };
    match edit {
        Edit::Remove { key_path } => file.remove(key_path),
        Edit::Insert {
            key_path,
            value,
            after,
            inline,
            comments,
            emptied,
        } => file.insert(
            key_path,
            value,
            after.as_deref(),
            *inline,
            comments,
            emptied.as_deref(),
        ),
        Edit::Set { key_path, value } => file.set(key_path, value),
        Edit::Append { key_path, value } => file.append(key_path, value),
        Edit::RemoveItem { key_path, value } => file.remove_item(key_path, value),
    }
}

/// The ranges of an object's properties or an array's elements.
struct Members {
    container: jsonc_parser::common::Range,
    members: Vec<jsonc_parser::common::Range>,
}

impl Members {
    fn of_object(object: &ast::Object<'_>) -> Self {
        Self {
            container: object.range,
            members: object.properties.iter().map(|prop| prop.range).collect(),
        }
    }

    fn of_array(array: &ast::Array<'_>) -> Self {
        Self {
            container: array.range,
            members: array
                .elements
                .iter()
                .map(|element| element.range())
                .collect(),
        }
    }
}

/// A string no configuration file holds, inserted where a value goes and
/// then replaced by text `jsonc-parser` would not write.
const PLACEHOLDER: &str = "config-digger:placeholder:6f1c2a";

struct File<'t> {
    text: &'t str,
    root: CstRootNode,
    ast: ast::Value<'t>,
}

impl File<'_> {
    fn remove(self, key_path: &[String]) -> Result<Change, EditError> {
        let (parent, name) = split_key_path(key_path)?;
        let object = self.cst_object(parent)?;
        let prop = object.get(name).ok_or_else(|| EditError::NotFound {
            key_path: key_path.to_vec(),
        })?;
        let span = self.ast_object(parent)?;
        let index = span
            .properties
            .iter()
            .position(|prop| prop.name.as_str() == name)
            .ok_or(EditError::Parse)?;
        let ast_prop = &span.properties[index];

        let comments = remove_attached_comments(&prop);
        let inverse = Edit::Insert {
            key_path: key_path.to_vec(),
            value: prop
                .value()
                .and_then(|value| value.to_serde_value())
                .ok_or(EditError::Parse)?,
            after: prop
                .previous_property()
                .and_then(|previous| previous.name())
                .and_then(|name| name.decoded_value().ok()),
            inline: !self.text[ast_prop.value.range().start..ast_prop.value.range().end]
                .contains('\n'),
            comments,
            emptied: (span.properties.len() == 1)
                .then(|| self.text[span.range.start..span.range.end].to_owned()),
        };
        prop.remove();
        let members = Members::of_object(span);
        if index == 0 && members.members.len() > 1 {
            self.keep_opening_gap(&members, object.children());
        }

        Ok(Change {
            text: self.root.to_string(),
            inverse,
            allowed: self.removal(&members, index),
        })
    }

    fn set(self, key_path: &[String], value: &Value) -> Result<Change, EditError> {
        let (parent, name) = split_key_path(key_path)?;
        let object = self.cst_object(parent)?;
        let span = self.ast_object(parent)?;
        let Some(prop) = object.get(name) else {
            let members = Members::of_object(span);
            let index = members.members.len();
            let text = if self.is_single_line(members.container) {
                splice_into_single_line(self.text, span, index, name, value)
            } else {
                object.append(name, input(value));
                self.root.to_string()
            };
            return Ok(Change {
                text,
                inverse: Edit::Remove {
                    key_path: key_path.to_vec(),
                },
                allowed: vec![self.gap(&members, index)],
            });
        };
        let old = prop
            .value()
            .and_then(|value| value.to_serde_value())
            .ok_or(EditError::Parse)?;
        let ast_prop = span.get(name).ok_or(EditError::Parse)?;
        let text = if self.is_single_line(ast_prop.value.range()) && forces_multiline(value) {
            self.with_placeholder(|placeholder| prop.set_value(placeholder), value)
        } else {
            prop.set_value(input(value));
            self.root.to_string()
        };
        Ok(Change {
            text,
            inverse: Edit::Set {
                key_path: key_path.to_vec(),
                value: old,
            },
            allowed: vec![self.lines(ast_prop.range)],
        })
    }

    fn append(self, key_path: &[String], value: &Value) -> Result<Change, EditError> {
        let (array, span) = self.list(key_path)?;
        let members = Members::of_array(span);
        let text = if self.is_single_line(members.container) && forces_multiline(value) {
            self.with_placeholder(
                |placeholder| {
                    array.append(placeholder);
                },
                value,
            )
        } else {
            array.append(input(value));
            self.root.to_string()
        };
        Ok(Change {
            text,
            inverse: Edit::RemoveItem {
                key_path: key_path.to_vec(),
                value: value.clone(),
            },
            allowed: vec![self.gap(&members, members.members.len())],
        })
    }

    fn remove_item(self, key_path: &[String], value: &Value) -> Result<Change, EditError> {
        let (array, span) = self.list(key_path)?;
        let elements = array.elements();
        let index = elements
            .iter()
            .rposition(|element| element.to_serde_value().as_ref() == Some(value))
            .ok_or_else(|| EditError::ItemNotFound {
                key_path: key_path.to_vec(),
                value: value.clone(),
            })?;
        elements[index].clone().remove();
        let members = Members::of_array(span);
        if index == 0 && members.members.len() > 1 {
            self.keep_opening_gap(&members, array.children());
        }
        Ok(Change {
            text: self.root.to_string(),
            inverse: Edit::Append {
                key_path: key_path.to_vec(),
                value: value.clone(),
            },
            allowed: self.removal(&members, index),
        })
    }

    /// Once the first member of a container written on one line is removed,
    /// `jsonc-parser` leaves the space that followed its comma after the
    /// opening bracket. This takes it away again when the container had none
    /// there.
    fn keep_opening_gap(&self, members: &Members, children: Vec<CstNode>) {
        let gap = &self.text[members.container.start + 1..members.members[0].start];
        if !gap.is_empty() || !self.is_single_line(members.container) {
            return;
        }
        for space in children
            .into_iter()
            .skip(1)
            .take_while(CstNode::is_whitespace)
        {
            space.remove();
        }
    }

    /// The list at `key_path`, in the tree and in the AST.
    fn list(&self, key_path: &[String]) -> Result<(CstArray, &ast::Array<'_>), EditError> {
        let (parent, name) = split_key_path(key_path)?;
        let prop = self
            .cst_object(parent)?
            .get(name)
            .ok_or_else(|| EditError::NotFound {
                key_path: key_path.to_vec(),
            })?;
        let array = prop.array_value().ok_or_else(|| EditError::WrongType {
            key_path: key_path.to_vec(),
            expected: "a list",
        })?;
        let span = self
            .ast_object(parent)?
            .get_array(name)
            .ok_or(EditError::Parse)?;
        Ok((array, span))
    }

    /// Writes `value` on one line where `write` puts a placeholder, since
    /// `jsonc-parser` writes objects over several lines.
    fn with_placeholder(&self, write: impl FnOnce(CstInputValue), value: &Value) -> String {
        if self.text.contains(PLACEHOLDER) {
            write(input(value));
            return self.root.to_string();
        }
        write(PLACEHOLDER.into());
        self.root.to_string().replacen(
            &Value::from(PLACEHOLDER).to_string(),
            &inline_json(value),
            1,
        )
    }

    fn insert(
        self,
        key_path: &[String],
        value: &Value,
        after: Option<&str>,
        inline: bool,
        comments: &[String],
        emptied: Option<&str>,
    ) -> Result<Change, EditError> {
        let (parent, name) = split_key_path(key_path)?;
        let object = self.cst_object(parent)?;
        if object.get(name).is_some() {
            return Err(EditError::AlreadyExists {
                key_path: key_path.to_vec(),
            });
        }
        let span = self.ast_object(parent)?;
        let index = match after {
            None => 0,
            Some(after) => span
                .properties
                .iter()
                .position(|prop| prop.name.as_str() == after)
                .map_or(span.properties.len(), |index| index + 1),
        };
        let allowed = vec![self.gap(&Members::of_object(span), index)];

        if let Some(emptied) = emptied
            && &self.text[span.range.start..span.range.end] == "{}"
        {
            let mut text = self.text.to_owned();
            text.replace_range(span.range.start..span.range.end, emptied);
            return Ok(Change {
                text,
                inverse: Edit::Remove {
                    key_path: key_path.to_vec(),
                },
                allowed,
            });
        }
        if self.is_single_line(span.range) {
            return Ok(Change {
                text: splice_into_single_line(self.text, span, index, name, value),
                inverse: Edit::Remove {
                    key_path: key_path.to_vec(),
                },
                allowed,
            });
        }
        let mut text = if inline && forces_multiline(value) {
            self.with_placeholder(
                |placeholder| {
                    object.insert(index, name, placeholder);
                },
                value,
            )
        } else {
            object.insert(index, name, input(value));
            self.root.to_string()
        };
        if !comments.is_empty() {
            text = with_comments_above(&text, key_path, comments)?;
        }
        Ok(Change {
            text,
            inverse: Edit::Remove {
                key_path: key_path.to_vec(),
            },
            allowed,
        })
    }

    /// The lines a new member at `index` goes between: from the end of the
    /// member before it (or the opening bracket) to the start of the member
    /// it goes before (or the closing bracket).
    fn gap(&self, members: &Members, index: usize) -> Range<usize> {
        let before = match index.checked_sub(1) {
            Some(previous) => members.members[previous].end,
            None => members.container.start,
        };
        let after = match members.members.get(index) {
            Some(next) => next.start,
            None => members.container.end - 1,
        };
        line_range(self.text, before, after)
    }

    /// The lines removing member `index` may change: its own with the
    /// comments attached above it, and the separator lines around it.
    fn removal(&self, members: &Members, index: usize) -> Vec<Range<usize>> {
        let member = members.members[index];
        let mut allowed = vec![self.lines_with_attached_comments(member.start, member.end)];
        allowed.push(self.line(match index.checked_sub(1) {
            Some(previous) => members.members[previous].end,
            None => members.container.start,
        }));
        if index + 1 == members.members.len() {
            allowed.push(self.line(members.container.end - 1));
        }
        allowed
    }

    fn is_single_line(&self, range: jsonc_parser::common::Range) -> bool {
        !self.text[range.start..range.end].contains('\n')
    }

    /// The lines a range spans.
    fn lines(&self, range: jsonc_parser::common::Range) -> Range<usize> {
        line_range(self.text, range.start, range.end)
    }

    /// The object at `key_path` in the tree.
    fn cst_object(&self, key_path: &[String]) -> Result<CstObject, EditError> {
        let mut object = self
            .root
            .object_value()
            .ok_or_else(|| EditError::WrongType {
                key_path: Vec::new(),
                expected: "an object",
            })?;
        for (depth, key) in key_path.iter().enumerate() {
            let prop = object.get(key).ok_or_else(|| EditError::NotFound {
                key_path: key_path[..=depth].to_vec(),
            })?;
            object = prop.object_value().ok_or_else(|| EditError::WrongType {
                key_path: key_path[..=depth].to_vec(),
                expected: "an object",
            })?;
        }
        Ok(object)
    }

    /// The object at `key_path` in the AST, which has the ranges.
    fn ast_object(&self, key_path: &[String]) -> Result<&ast::Object<'_>, EditError> {
        let mut object = self.ast.as_object().ok_or(EditError::Parse)?;
        for key in key_path {
            object = object.get_object(key).ok_or(EditError::Parse)?;
        }
        Ok(object)
    }

    /// The line that holds byte `offset`, as a range.
    fn line(&self, offset: usize) -> Range<usize> {
        let line = line_of(self.text, offset);
        line..line + 1
    }

    /// The lines from the comments attached above byte `start` to the line
    /// that holds byte `end`.
    fn lines_with_attached_comments(&self, start: usize, end: usize) -> Range<usize> {
        let lines = lines(self.text);
        let mut top = line_of(self.text, start);
        if self.text[line_start(self.text, start)..start]
            .trim()
            .is_empty()
        {
            while top > 0 {
                let above = lines[top - 1].trim();
                if above.starts_with("//") || (above.starts_with("/*") && above.ends_with("*/")) {
                    top -= 1;
                } else if above.ends_with("*/") {
                    match (0..top - 1)
                        .rev()
                        .find(|line| lines[*line].trim_start().starts_with("/*"))
                    {
                        Some(open) => top = open,
                        None => break,
                    }
                } else {
                    break;
                }
            }
        }
        top..line_range(self.text, start, end).end
    }
}

/// Removes the comments on their own lines directly above `prop`, with no
/// blank line in between, and returns them top to bottom. `jsonc-parser`'s
/// own `remove()` leaves them behind.
fn remove_attached_comments(prop: &CstObjectProp) -> Vec<String> {
    let previous: Vec<CstNode> = prop.previous_siblings().collect(); // nearest first
    let mut index = 0;
    while index < previous.len() && previous[index].is_whitespace() {
        index += 1; // the indentation of the property's own line
    }
    let mut groups: Vec<Vec<CstNode>> = Vec::new();
    if index < previous.len() && previous[index].is_newline() {
        index += 1;
        loop {
            // Walking backwards: [whitespace] comment [whitespace] newline.
            let start = index;
            let mut comment = index;
            while comment < previous.len() && previous[comment].is_whitespace() {
                comment += 1;
            }
            if !(comment < previous.len() && previous[comment].is_comment()) {
                break;
            }
            let mut newline = comment + 1;
            while newline < previous.len() && previous[newline].is_whitespace() {
                newline += 1;
            }
            if !(newline < previous.len() && previous[newline].is_newline()) {
                break; // the comment shares its line with code, so it is not attached
            }
            groups.push(previous[start..=newline].to_vec());
            index = newline + 1;
        }
    }
    let mut comments = Vec::new();
    for group in groups {
        // Remove the comment first: for `//` comments jsonc-parser also removes
        // the indentation and newline before it, which are in the group anyway.
        for node in group.iter() {
            if let CstNode::Leaf(CstLeafNode::Comment(comment)) = node {
                comments.push(comment.raw_value());
                CstComment::remove(comment.clone());
            }
        }
        for node in group.into_iter().filter(|node| !node.is_comment()) {
            node.remove(); // a no-op when already detached
        }
    }
    comments.reverse();
    comments
}

/// Adds `"name": value` as member `index` of an object written on one line,
/// keeping it on one line and keeping its trailing comma, if it has one.
fn splice_into_single_line(
    text: &str,
    object: &ast::Object<'_>,
    index: usize,
    name: &str,
    value: &Value,
) -> String {
    let member = format!("{}: {}", Value::from(name), inline_json(value));
    let (at, insertion) = match (object.properties.get(index), object.properties.last()) {
        (Some(next), _) => (next.range.start, format!("{member}, ")),
        (None, Some(last)) => {
            let rest = &text[last.range.end..object.range.end - 1];
            let space = rest.len() - rest.trim_start().len();
            if rest.trim_start().starts_with(',') {
                (last.range.end + space + 1, format!(" {member},"))
            } else {
                (last.range.end, format!(", {member}"))
            }
        }
        (None, None) => {
            let inner = object.range.start + 1..object.range.end - 1;
            if text[inner.clone()].trim().is_empty() {
                let mut text = text.to_owned();
                text.replace_range(inner, &format!(" {member} "));
                return text;
            }
            (inner.start, format!(" {member}"))
        }
    };
    let mut text = text.to_owned();
    text.insert_str(at, &insertion);
    text
}

/// Puts `comments` on their own lines above the property at `key_path`, at
/// its indentation.
fn with_comments_above(
    text: &str,
    key_path: &[String],
    comments: &[String],
) -> Result<String, EditError> {
    let ast =
        jsonc_parser::parse_to_ast(text, &CollectOptions::default(), &ParseOptions::default())
            .map_err(|_| EditError::Parse)?;
    let (parent, name) = split_key_path(key_path)?;
    let mut object = ast
        .value
        .as_ref()
        .and_then(|value| value.as_object())
        .ok_or(EditError::Parse)?;
    for key in parent {
        object = object.get_object(key).ok_or(EditError::Parse)?;
    }
    let start = object.get(name).ok_or(EditError::Parse)?.range.start;
    let line_start = line_start(text, start);
    let indent = &text[line_start..start];
    if !indent.trim().is_empty() {
        return Ok(text.to_owned()); // the property shares its line with code
    }
    let block: String = comments
        .iter()
        .map(|comment| format!("{indent}{comment}\n"))
        .collect();
    let mut text = text.to_owned();
    text.insert_str(line_start, &block);
    Ok(text)
}

/// Whether `jsonc-parser` writes `value` over several lines.
fn forces_multiline(value: &Value) -> bool {
    match value {
        Value::Object(object) => !object.is_empty(),
        Value::Array(items) => items.iter().any(|item| match item {
            Value::Object(object) => !object.is_empty(),
            Value::Array(items) => !items.is_empty(),
            _ => false,
        }),
        _ => false,
    }
}

/// `value` written on one line, with a space after each comma and colon.
fn inline_json(value: &Value) -> String {
    match value {
        Value::Array(items) => {
            let items: Vec<String> = items.iter().map(inline_json).collect();
            format!("[{}]", items.join(", "))
        }
        Value::Object(object) if object.is_empty() => "{}".to_owned(),
        Value::Object(object) => {
            let members: Vec<String> = object
                .iter()
                .map(|(name, value)| {
                    format!("{}: {}", Value::from(name.as_str()), inline_json(value))
                })
                .collect();
            format!("{{ {} }}", members.join(", "))
        }
        scalar => scalar.to_string(),
    }
}

fn input(value: &Value) -> CstInputValue {
    match value {
        Value::Null => CstInputValue::Null,
        Value::Bool(boolean) => CstInputValue::Bool(*boolean),
        Value::Number(number) => CstInputValue::Number(number.to_string()),
        Value::String(string) => CstInputValue::String(string.clone()),
        Value::Array(items) => CstInputValue::Array(items.iter().map(input).collect()),
        Value::Object(object) => CstInputValue::Object(
            object
                .iter()
                .map(|(name, value)| (name.clone(), input(value)))
                .collect(),
        ),
    }
}

/// The span of the value at `key_path`, where an array item's key is its index.
pub(super) fn value_span(text: &str, key_path: &[String]) -> Option<Range<usize>> {
    let ast =
        jsonc_parser::parse_to_ast(text, &CollectOptions::default(), &ParseOptions::default())
            .ok()?;
    let mut value = ast.value?;
    for key in key_path {
        value = match value {
            ast::Value::Object(object) => {
                object
                    .properties
                    .into_iter()
                    .find(|prop| prop.name.as_str() == key)?
                    .value
            }
            ast::Value::Array(array) => array.elements.into_iter().nth(key.parse().ok()?)?,
            _ => return None,
        };
    }
    let range = value.range();
    Some(range.start..range.end)
}
