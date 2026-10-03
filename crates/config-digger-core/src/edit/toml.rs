//! TOML edits on `toml_edit`, which keeps comments and whitespace as decor
//! around each item. The lines an edit may change come from the spans of a
//! separate parse, so that a mistake in either shows up as a refused edit.
//!
//! `toml_edit` keeps everything between two items, comments and blank lines
//! included, in the decor before the second one. A comment block separated
//! from an item by a blank line is not about that item, so a Remove moves it
//! onto whatever comes next instead of taking it away.

use super::{Change, Edit, EditError, line_of, line_range, line_start, lines, split_key_path};
use serde_json::Value;
use std::ops::Range;
use toml_edit::{Decor, DocumentMut, InlineTable, Item, Key, Table, TableLike};

type Document = toml_edit::Document<String>;

fn parse(text: &str) -> Result<DocumentMut, EditError> {
    text.parse().map_err(|_| EditError::Parse)
}

pub(super) fn reprint(text: &str) -> Result<String, EditError> {
    Ok(parse(text)?.to_string())
}

pub(super) fn value(text: &str) -> Result<Value, EditError> {
    Ok(table_to_json(parse(text)?.as_table()))
}

pub(super) fn apply(text: &str, edit: &Edit) -> Result<Change, EditError> {
    let file = File {
        text,
        doc: parse(text)?,
        spans: Document::parse(text.to_owned()).map_err(|_| EditError::Parse)?,
    };
    match edit {
        Edit::Remove { key_path } => file.remove(key_path),
        Edit::Set { key_path, value } => file.set(key_path, value),
        Edit::Insert {
            key_path,
            value,
            after,
            inline,
            comments,
        } => file.insert(key_path, value, after.as_deref(), *inline, comments),
        Edit::Append { key_path, value } => file.append(key_path, value),
        Edit::RemoveItem { key_path, value } => file.remove_item(key_path, value),
    }
}

struct File<'t> {
    text: &'t str,
    /// The document to edit.
    doc: DocumentMut,
    /// The same document, parsed with the spans that editing drops.
    spans: Document,
}

/// A table that holds keys, as an edit sees it.
enum Parent<'d> {
    Table(&'d mut Table),
    Inline(&'d mut InlineTable),
}

impl<'d> Parent<'d> {
    fn table_like(&mut self) -> &mut dyn TableLike {
        match self {
            Parent::Table(table) => *table,
            Parent::Inline(table) => *table,
        }
    }

    /// The item at `key`, for as long as the table is borrowed.
    fn into_item(self, key: &str) -> Option<&'d mut Item> {
        match self {
            Parent::Table(table) => table.get_mut(key),
            Parent::Inline(table) => TableLike::get_mut(table, key),
        }
    }
}

impl File<'_> {
    fn remove(mut self, key_path: &[String]) -> Result<Change, EditError> {
        let (parent_path, name) = split_key_path(key_path)?;
        let allowed = self.lines_with_attached_comments(key_path)?;
        let mut parent = parent_mut(&mut self.doc, parent_path)?;
        let siblings: Vec<String> = parent
            .table_like()
            .iter()
            .map(|(key, _)| key.to_owned())
            .collect();
        let index = siblings
            .iter()
            .position(|sibling| sibling == name)
            .ok_or_else(|| EditError::NotFound {
                key_path: key_path.to_vec(),
            })?;
        let (parent_position, lead) = match &parent {
            Parent::Table(table) => (
                table.position(),
                match table.get(name) {
                    Some(Item::Value(_)) => table.key(name).map(|key| prefix(key.leaf_decor())),
                    Some(Item::Table(child)) if !child.is_implicit() => Some(prefix(child.decor())),
                    _ => None,
                }
                .unwrap_or_default()
                .to_owned(),
            ),
            Parent::Inline(_) => (None, String::new()),
        };
        let (section, attached) = split_prefix(&lead);
        let item = parent
            .table_like()
            .remove(name)
            .ok_or_else(|| EditError::NotFound {
                key_path: key_path.to_vec(),
            })?;
        let inverse = Edit::Insert {
            key_path: key_path.to_vec(),
            value: item_to_json(&item),
            after: index
                .checked_sub(1)
                .map(|previous| siblings[previous].clone()),
            inline: item.is_value(),
            comments: attached
                .lines()
                .map(str::trim)
                .filter(|line| line.starts_with('#'))
                .map(str::to_owned)
                .collect(),
        };

        // The section's comments go on the next key of the same table, or
        // else on the next table header, or else at the end of the file.
        let mut after_position = None;
        if !section.trim().is_empty() {
            match (&mut parent, &item) {
                (Parent::Table(table), Item::Value(_)) => {
                    let next_key = siblings[index + 1..]
                        .iter()
                        .find(|sibling| matches!(table.get(sibling), Some(Item::Value(_))));
                    match next_key.and_then(|next_key| table.key_mut(next_key)) {
                        Some(mut key) => prepend_section(key.leaf_decor_mut(), section),
                        None => after_position = Some(parent_position.unwrap_or(isize::MIN)),
                    }
                }
                (_, Item::Table(removed)) => {
                    after_position = Some(removed.position().unwrap_or(isize::MAX));
                }
                _ => {}
            }
        }
        if let Some(after_position) = after_position {
            self.prepend_section_to_next_table(after_position, section);
        }
        Ok(Change {
            text: self.doc.to_string(),
            inverse,
            allowed,
        })
    }

    fn set(mut self, key_path: &[String], value: &Value) -> Result<Change, EditError> {
        let (parent_path, name) = split_key_path(key_path)?;
        if let (Some(parent), Value::Object(_)) =
            (table_at(self.spans.as_table(), parent_path), value)
            && parent.is_implicit()
            && !parent.contains_key(name)
        {
            // A table with no header of its own holds only tables, so the
            // new object becomes one too, after the last of them.
            let last = parent.iter().last().map(|(key, _)| key.to_owned());
            return self.insert(key_path, value, last.as_deref(), false, &[]);
        }
        let mut new = to_toml(value, key_path)?;
        let own_lines = self.lines_of(key_path);
        let body_end = self.body_end(parent_path);
        let mut parent = parent_mut(&mut self.doc, parent_path)?;
        let (inverse, allowed) = match parent.table_like().get_mut(name) {
            Some(Item::Value(old)) => {
                *new.decor_mut() = old.decor().clone();
                let inverse = Edit::Set {
                    key_path: key_path.to_vec(),
                    value: value_to_json(old),
                };
                *old = new;
                (inverse, own_lines?)
            }
            Some(_) => {
                return Err(EditError::WrongType {
                    key_path: key_path.to_vec(),
                    expected: "a key with a value",
                });
            }
            None => {
                let last = parent
                    .table_like()
                    .iter()
                    .last()
                    .map(|(key, _)| key.to_owned());
                insert_value(&mut parent, name, new, last.as_deref());
                let inverse = Edit::Remove {
                    key_path: key_path.to_vec(),
                };
                (inverse, body_end?)
            }
        };
        Ok(Change {
            text: self.doc.to_string(),
            inverse,
            allowed: vec![allowed],
        })
    }

    fn insert(
        mut self,
        key_path: &[String],
        value: &Value,
        after: Option<&str>,
        inline: bool,
        comments: &[String],
    ) -> Result<Change, EditError> {
        let (parent_path, name) = split_key_path(key_path)?;
        let mut parent = parent_mut(&mut self.doc, parent_path)?;
        if parent.table_like().contains_key(name) {
            return Err(EditError::AlreadyExists {
                key_path: key_path.to_vec(),
            });
        }
        let into_table = matches!(parent, Parent::Table(_));
        let comment_block: String = comments
            .iter()
            .map(|comment| format!("{comment}\n"))
            .collect();
        let inverse = Edit::Remove {
            key_path: key_path.to_vec(),
        };

        if let (true, false, Value::Object(object)) = (into_table, inline, value) {
            let mut table = to_toml_table(object, key_path)?;
            table.decor_mut().set_prefix(format!("\n{comment_block}"));
            let position = self.table_position(parent_path, after)?;
            let line = self.line_before_table_after(position);
            let insertion = line..line;
            let mut count = 1;
            for_each_table(&mut table, &mut |_| count += 1);
            for_each_table(self.doc.as_table_mut(), &mut |table| {
                if let Some(later) = table.position().filter(|later| *later > position) {
                    table.set_position(Some(later + count));
                }
            });
            table.set_position(Some(position + 1));
            let mut next = position + 2;
            for_each_table(&mut table, &mut |table| {
                table.set_position(Some(next));
                next += 1;
            });
            if let Parent::Table(parent) = parent_mut(&mut self.doc, parent_path)? {
                parent.insert(name, Item::Table(table));
            }
            return Ok(Change {
                text: self.doc.to_string(),
                inverse,
                allowed: vec![insertion],
            });
        }

        let allowed = self.body(parent_path)?;
        let mut parent = parent_mut(&mut self.doc, parent_path)?;
        insert_value(&mut parent, name, to_toml(value, key_path)?, after);
        if let (Parent::Table(table), false) = (parent, comments.is_empty())
            && let Some(mut key) = table.key_mut(name)
        {
            key.leaf_decor_mut().set_prefix(comment_block);
        }
        Ok(Change {
            text: self.doc.to_string(),
            inverse,
            allowed: vec![allowed],
        })
    }

    fn append(mut self, key_path: &[String], value: &Value) -> Result<Change, EditError> {
        let mut new = to_toml(value, key_path)?;
        let allowed = self.lines_of(key_path)?;
        let array = array_mut(&mut self.doc, key_path)?;
        if let Some(last) = array.iter_mut().last() {
            // The new item takes the last one's place in the layout.
            if let Some(prefix) = last.decor().prefix() {
                new.decor_mut().set_prefix(prefix.clone());
            }
            if let Some(suffix) = last.decor().suffix().cloned() {
                new.decor_mut().set_suffix(suffix);
                last.decor_mut().set_suffix("");
            }
        }
        array.push_formatted(new);
        Ok(Change {
            text: self.doc.to_string(),
            inverse: Edit::RemoveItem {
                key_path: key_path.to_vec(),
                value: value.clone(),
            },
            allowed: vec![allowed],
        })
    }

    fn remove_item(mut self, key_path: &[String], value: &Value) -> Result<Change, EditError> {
        let allowed = self.lines_of(key_path)?;
        let array = array_mut(&mut self.doc, key_path)?;
        let index = (0..array.len())
            .rev()
            .find(|index| array.get(*index).map(value_to_json).as_ref() == Some(value))
            .ok_or_else(|| EditError::ItemNotFound {
                key_path: key_path.to_vec(),
                value: value.clone(),
            })?;
        let removed = array.remove(index);
        // A neighbour takes its place in the layout.
        if index == 0 {
            if let (Some(first), Some(prefix)) = (array.get_mut(0), removed.decor().prefix()) {
                first.decor_mut().set_prefix(prefix.clone());
            }
        } else if index == array.len()
            && let (Some(last), Some(suffix)) = (array.get_mut(index - 1), removed.decor().suffix())
        {
            last.decor_mut().set_suffix(suffix.clone());
        }
        Ok(Change {
            text: self.doc.to_string(),
            inverse: Edit::Append {
                key_path: key_path.to_vec(),
                value: value.clone(),
            },
            allowed: vec![allowed],
        })
    }

    /// Prepends `section` to the first table header after document position
    /// `after`, or to the end of the file when there is none.
    fn prepend_section_to_next_table(&mut self, after: isize, section: &str) {
        match next_table_position(self.doc.as_table(), after) {
            Some(next) => for_each_table(self.doc.as_table_mut(), &mut |table| {
                if !table.is_implicit() && table.position() == Some(next) {
                    prepend_section(table.decor_mut(), section);
                }
            }),
            None => {
                let trailing = self.doc.trailing().as_str().unwrap_or_default().to_owned();
                self.doc.set_trailing(format!("{section}{trailing}"));
            }
        }
    }

    /// The document position a new table under `parent_path` goes after:
    /// the last one of the sibling `after`, the one before the first sibling
    /// when `after` is `None`, and otherwise the parent's last one.
    fn table_position(
        &self,
        parent_path: &[String],
        after: Option<&str>,
    ) -> Result<isize, EditError> {
        let parent = table_at(self.spans.as_table(), parent_path).ok_or(EditError::Parse)?;
        if let Some(sibling) = after.and_then(|after| parent.get(after)?.as_table())
            && let Some((_, last)) = positions(sibling)
        {
            return Ok(last);
        }
        // A table cannot go among plain keys, so after one it goes first.
        if after.is_none_or(|after| parent.get(after).is_some_and(Item::is_value)) {
            let first = parent
                .iter()
                .filter_map(|(_, item)| positions(item.as_table()?))
                .map(|(first, _)| first)
                .min();
            if let Some(first) = first {
                return Ok(first - 1);
            }
        }
        Ok(positions(parent).map_or(0, |(_, last)| last))
    }

    /// The line where a table goes when it follows document position
    /// `position`: where the decor of the next table header starts, or after
    /// the last line of the file's content.
    fn line_before_table_after(&self, position: isize) -> usize {
        let root = self.spans.as_table();
        let mut next_start = None;
        if let Some(next) = next_table_position(root, position) {
            visit_tables(root, &mut |table| {
                if !table.is_implicit() && table.position() == Some(next) {
                    next_start = table.span().map(|span| {
                        line_of(self.text, span.start)
                            - spanned_prefix(self.text, table.decor())
                                .matches('\n')
                                .count()
                    });
                }
            });
        }
        next_start.unwrap_or_else(|| {
            table_extent(root).map_or(0, |extent| line_of(self.text, extent.end) + 1)
        })
    }

    /// The lines of the key at `key_path`, from its key to the end of its value.
    fn lines_of(&self, key_path: &[String]) -> Result<Range<usize>, EditError> {
        let extent = self.extent(key_path)?;
        Ok(line_range(self.text, extent.start, extent.end))
    }

    /// The lines of the key at `key_path` with the comments attached above it.
    fn lines_with_attached_comments(
        &self,
        key_path: &[String],
    ) -> Result<Vec<Range<usize>>, EditError> {
        let (parent_path, name) = split_key_path(key_path)?;
        let not_found = || EditError::NotFound {
            key_path: key_path.to_vec(),
        };
        let parent = table_like_at(self.spans.as_table(), parent_path).ok_or_else(not_found)?;
        let item = parent.get(name).ok_or_else(not_found)?;
        let mut ranges = Vec::new();
        match item {
            Item::Table(table) => self.header_ranges(table, &mut ranges),
            Item::ArrayOfTables(tables) => {
                for table in tables.iter() {
                    self.header_ranges(table, &mut ranges);
                }
            }
            _ => {
                let extent = item_extent(parent.key(name), item).ok_or(EditError::Parse)?;
                ranges.push(self.with_attached_comments(extent));
            }
        }
        Ok(ranges)
    }

    /// The lines of each table header in `table`, itself included: from the
    /// comments attached above the header to the last key written under it.
    /// Sub-tables need not follow their parent, so each header gets its own
    /// range, and what lies between them stays out.
    fn header_ranges(&self, table: &Table, ranges: &mut Vec<Range<usize>>) {
        let mut add = |table: &Table| {
            let Some(mut extent) = table.span().filter(|_| !table.is_implicit()) else {
                return;
            };
            for (key, item) in table.iter() {
                if let Some(span) = item_extent(table.key(key), item).filter(|_| item.is_value()) {
                    extent.end = extent.end.max(span.end);
                }
            }
            ranges.push(self.with_attached_comments(extent));
        };
        add(table);
        visit_tables(table, &mut add);
        if ranges.is_empty()
            && let Some(extent) = table_extent(table)
        {
            ranges.push(self.with_attached_comments(extent));
        }
    }

    /// The lines of `extent`, from the comments attached above it.
    fn with_attached_comments(&self, extent: Range<usize>) -> Range<usize> {
        let lines = lines(self.text);
        let mut top = line_of(self.text, extent.start);
        if self.text[line_start(self.text, extent.start)..extent.start]
            .trim()
            .is_empty()
        {
            while top > 0 && lines[top - 1].trim_start().starts_with('#') {
                top -= 1;
            }
        }
        top..line_range(self.text, extent.start, extent.end).end
    }

    /// The span of the key at `key_path` and everything below it.
    fn extent(&self, key_path: &[String]) -> Result<Range<usize>, EditError> {
        let (parent_path, name) = split_key_path(key_path)?;
        let not_found = || EditError::NotFound {
            key_path: key_path.to_vec(),
        };
        let parent = table_like_at(self.spans.as_table(), parent_path).ok_or_else(not_found)?;
        let item = parent.get(name).ok_or_else(not_found)?;
        item_extent(parent.key(name), item).ok_or(EditError::Parse)
    }

    /// The lines of the keys written directly under the table at
    /// `key_path`, from its header, where a new key may go.
    fn body(&self, key_path: &[String]) -> Result<Range<usize>, EditError> {
        let end = self.body_end(key_path)?;
        let start = table_at(self.spans.as_table(), key_path)
            .and_then(Table::span)
            .map_or(end.start, |span| {
                line_of(self.text, span.start).min(end.start)
            });
        Ok(start..end.end)
    }

    /// The last line of the keys written directly under the table at
    /// `key_path`, or of its header, after which a new key goes. For an
    /// inline table, its lines.
    fn body_end(&self, key_path: &[String]) -> Result<Range<usize>, EditError> {
        if let Some(inline) = inline_table_span(self.spans.as_table(), key_path) {
            return Ok(line_range(self.text, inline.start, inline.end));
        }
        let table = table_like_at(self.spans.as_table(), key_path).ok_or(EditError::Parse)?;
        let mut end: Option<usize> = None;
        for (key, item) in table.iter() {
            if item.is_value()
                && let Some(extent) = item_extent(table.key(key), item)
            {
                end = end.max(Some(line_of(self.text, extent.end)));
            }
        }
        let header = table_at(self.spans.as_table(), key_path)
            .and_then(Table::span)
            .map(|span| line_of(self.text, span.start));
        Ok(match end.or(header) {
            Some(line) => line..line + 1,
            None => 0..0,
        })
    }
}

/// The table holding the keys at `key_path`.
fn parent_mut<'d>(doc: &'d mut DocumentMut, key_path: &[String]) -> Result<Parent<'d>, EditError> {
    let mut parent = Parent::Table(doc.as_table_mut());
    for (depth, key) in key_path.iter().enumerate() {
        let item = parent.into_item(key).ok_or_else(|| EditError::NotFound {
            key_path: key_path[..=depth].to_vec(),
        })?;
        parent = match item {
            Item::Table(table) => Parent::Table(table),
            Item::Value(toml_edit::Value::InlineTable(table)) => Parent::Inline(table),
            _ => {
                return Err(EditError::WrongType {
                    key_path: key_path[..=depth].to_vec(),
                    expected: "a table",
                });
            }
        };
    }
    Ok(parent)
}

fn array_mut<'d>(
    doc: &'d mut DocumentMut,
    key_path: &[String],
) -> Result<&'d mut toml_edit::Array, EditError> {
    let (parent_path, name) = split_key_path(key_path)?;
    let item = parent_mut(doc, parent_path)?
        .into_item(name)
        .ok_or_else(|| EditError::NotFound {
            key_path: key_path.to_vec(),
        })?;
    item.as_array_mut().ok_or_else(|| EditError::WrongType {
        key_path: key_path.to_vec(),
        expected: "a list",
    })
}

/// Adds a key with a value to a table, right after the key `after`: first
/// when `after` is `None`, last when it is `Some` of a missing key. An inline
/// table keeps its spacing: whatever value ends up last keeps the space
/// before `}`.
fn insert_value(
    parent: &mut Parent<'_>,
    name: &str,
    mut value: toml_edit::Value,
    after: Option<&str>,
) {
    let mut order: Vec<String> = parent
        .table_like()
        .iter()
        .map(|(key, _)| key.to_owned())
        .collect();
    let index = match after {
        None => 0,
        Some(after) => order
            .iter()
            .position(|key| key == after)
            .map_or(order.len(), |index| index + 1),
    };
    order.insert(index, name.to_owned());
    let rank = |key: &Key| order.iter().position(|name| name == key.get());
    match parent {
        Parent::Table(table) => {
            table.insert(name, Item::Value(value));
            table.sort_values_by(|a, _, b, _| rank(a).cmp(&rank(b)));
        }
        Parent::Inline(table) => {
            let closing = table.iter_mut().last().and_then(|(_, last)| {
                let suffix = last.decor().suffix().cloned();
                last.decor_mut().set_suffix("");
                suffix
            });
            if !table.is_empty() {
                value.decor_mut().set_prefix(" ");
            }
            value.decor_mut().set_suffix("");
            table.insert(name, value);
            table.sort_values_by(|a, _, b, _| rank(a).cmp(&rank(b)));
            if let (Some(closing), Some((_, last))) = (closing, table.iter_mut().last()) {
                last.decor_mut().set_suffix(closing);
            }
        }
    }
}

/// The table at `key_path` of the spans document, if it is a `Table`.
fn table_at<'d>(root: &'d Table, key_path: &[String]) -> Option<&'d Table> {
    key_path
        .iter()
        .try_fold(root, |table, key| table.get(key)?.as_table())
}

/// The table or inline table at `key_path` of the spans document.
fn table_like_at<'d>(root: &'d Table, key_path: &[String]) -> Option<&'d dyn TableLike> {
    let mut table: &dyn TableLike = root;
    for key in key_path {
        table = table.get(key)?.as_table_like()?;
    }
    Some(table)
}

/// The span of the inline table at `key_path`, if it is one.
fn inline_table_span(root: &Table, key_path: &[String]) -> Option<Range<usize>> {
    let (parent, name) = key_path.split_last().map(|(name, parent)| (parent, name))?;
    table_like_at(root, parent)?
        .get(name)?
        .as_value()?
        .as_inline_table()?
        .span()
}

/// The span from a key to the end of its item, sub-tables included.
fn item_extent(key: Option<&Key>, item: &Item) -> Option<Range<usize>> {
    let mut extent = key.and_then(Key::span);
    let mut merge = |span: Option<Range<usize>>| {
        if let Some(span) = span {
            extent = Some(match extent.take() {
                Some(extent) => extent.start.min(span.start)..extent.end.max(span.end),
                None => span,
            });
        }
    };
    match item {
        Item::None => {}
        Item::Value(value) => merge(value.span()),
        Item::Table(table) => merge(table_extent(table)),
        Item::ArrayOfTables(tables) => tables.iter().for_each(|table| merge(table_extent(table))),
    }
    extent
}

fn table_extent(table: &Table) -> Option<Range<usize>> {
    let mut extent = table.span();
    for (key, item) in table.iter() {
        if let Some(span) = item_extent(table.key(key), item) {
            extent = Some(match extent {
                Some(extent) => extent.start.min(span.start)..extent.end.max(span.end),
                None => span,
            });
        }
    }
    extent
}

/// Calls `visit` on every table below `table`, in no particular order.
fn visit_tables(table: &Table, visit: &mut impl FnMut(&Table)) {
    for (_, item) in table.iter() {
        match item {
            Item::Table(child) => {
                visit(child);
                visit_tables(child, visit);
            }
            Item::ArrayOfTables(tables) => {
                for child in tables.iter() {
                    visit(child);
                    visit_tables(child, visit);
                }
            }
            _ => {}
        }
    }
}

/// Calls `visit` on `table`'s own tables below it, then recursively, parents
/// before children.
fn for_each_table(table: &mut Table, visit: &mut impl FnMut(&mut Table)) {
    for (_, item) in table.iter_mut() {
        match item {
            Item::Table(child) => {
                visit(child);
                for_each_table(child, visit);
            }
            Item::ArrayOfTables(tables) => {
                for child in tables.iter_mut() {
                    visit(child);
                    for_each_table(child, visit);
                }
            }
            _ => {}
        }
    }
}

/// The first and last document positions of the table headers in
/// `table`, itself included.
fn positions(table: &Table) -> Option<(isize, isize)> {
    let mut range: Option<(isize, isize)> = None;
    let mut add = |table: &Table| {
        if let Some(position) = table.position().filter(|_| !table.is_implicit()) {
            range = Some(match range {
                Some((first, last)) => (first.min(position), last.max(position)),
                None => (position, position),
            });
        }
    };
    add(table);
    visit_tables(table, &mut add);
    range
}

/// The document position of the first table header after `after`.
fn next_table_position(root: &Table, after: isize) -> Option<isize> {
    let mut next: Option<isize> = None;
    visit_tables(root, &mut |table| {
        if let Some(position) = table.position().filter(|position| *position > after)
            && !table.is_implicit()
            && next.is_none_or(|next| position < next)
        {
            next = Some(position);
        }
    });
    next
}

fn prefix(decor: &Decor) -> &str {
    decor
        .prefix()
        .and_then(|prefix| prefix.as_str())
        .unwrap_or_default()
}

/// Splits the decor before an item at its last blank line: the comments
/// above that line are not about the item, those below it are.
fn split_prefix(prefix: &str) -> (&str, &str) {
    let mut cut = 0;
    let mut offset = 0;
    for line in prefix.split_inclusive('\n') {
        offset += line.len();
        if line.ends_with('\n') && line.trim().is_empty() {
            cut = offset;
        }
    }
    prefix.split_at(cut)
}

/// Puts a section's comments before an item, which keeps its own comments,
/// in place of the blank lines that separated it from what came before.
fn prepend_section(decor: &mut Decor, section: &str) {
    let mut own = prefix(decor);
    while let Some((line, rest)) = own.split_once('\n')
        && line.trim().is_empty()
    {
        own = rest;
    }
    decor.set_prefix(format!("{section}{own}"));
}

fn to_toml(value: &Value, key_path: &[String]) -> Result<toml_edit::Value, EditError> {
    let unrepresentable = || EditError::Unrepresentable {
        key_path: key_path.to_vec(),
    };
    if let Some((tag, text)) = untagged(value) {
        return match tag {
            DATETIME => text
                .parse::<toml_edit::Datetime>()
                .map(Into::into)
                .map_err(|_| unrepresentable()),
            _ => {
                let magnitude = if text.trim_start_matches('-') == "nan" {
                    f64::NAN
                } else {
                    f64::INFINITY
                };
                Ok(if text.starts_with('-') {
                    -magnitude
                } else {
                    magnitude
                }
                .into())
            }
        };
    }
    Ok(match value {
        Value::Null => return Err(unrepresentable()),
        Value::Bool(boolean) => (*boolean).into(),
        Value::Number(number) => match (number.as_i64(), number.is_f64()) {
            (Some(integer), _) => integer.into(),
            (None, true) => number.as_f64().ok_or_else(unrepresentable)?.into(),
            // An integer above TOML's 64-bit signed range.
            (None, false) => return Err(unrepresentable()),
        },
        Value::String(string) => string.as_str().into(),
        Value::Array(items) => {
            let mut array = toml_edit::Array::new();
            for item in items {
                array.push(to_toml(item, key_path)?);
            }
            array.into()
        }
        Value::Object(object) => {
            let mut table = InlineTable::new();
            for (name, value) in object {
                table.insert(name, to_toml(value, key_path)?);
            }
            table.into()
        }
    })
}

/// A `[table]`, with its objects as sub-tables.
fn to_toml_table(
    object: &serde_json::Map<String, Value>,
    key_path: &[String],
) -> Result<Table, EditError> {
    let mut table = Table::new();
    for (name, value) in object {
        let item = match value {
            Value::Object(object) => {
                let mut child = to_toml_table(object, key_path)?;
                child.decor_mut().set_prefix("\n");
                Item::Table(child)
            }
            value => Item::Value(to_toml(value, key_path)?),
        };
        table.insert(name, item);
    }
    Ok(table)
}

fn table_to_json(table: &dyn TableLike) -> Value {
    Value::Object(
        table
            .iter()
            .map(|(key, item)| (key.to_owned(), item_to_json(item)))
            .collect(),
    )
}

fn item_to_json(item: &Item) -> Value {
    match item {
        Item::None => Value::Null,
        Item::Value(value) => value_to_json(value),
        Item::Table(table) => table_to_json(table),
        Item::ArrayOfTables(tables) => tables.iter().map(|table| table_to_json(table)).collect(),
    }
}

fn value_to_json(value: &toml_edit::Value) -> Value {
    match value {
        toml_edit::Value::String(string) => Value::String(string.value().clone()),
        toml_edit::Value::Integer(integer) => Value::from(*integer.value()),
        toml_edit::Value::Float(float) => {
            let float = *float.value();
            serde_json::Number::from_f64(float).map_or_else(
                || {
                    let sign = if float.is_sign_negative() { "-" } else { "" };
                    let name = if float.is_nan() { "nan" } else { "inf" };
                    tagged(SPECIAL_FLOAT, format!("{sign}{name}"))
                },
                Value::Number,
            )
        }
        toml_edit::Value::Boolean(boolean) => Value::Bool(*boolean.value()),
        toml_edit::Value::Datetime(datetime) => tagged(DATETIME, datetime.value().to_string()),
        toml_edit::Value::Array(array) => array.iter().map(value_to_json).collect(),
        toml_edit::Value::InlineTable(table) => table_to_json(table),
    }
}

/// The decor before an item of the spans document, which holds spans of the
/// text rather than strings.
fn spanned_prefix<'t>(text: &'t str, decor: &'t Decor) -> &'t str {
    decor
        .prefix()
        .and_then(|prefix| prefix.as_str().or_else(|| text.get(prefix.span()?)))
        .unwrap_or_default()
}

/// The key of the object that stands for a TOML datetime in a JSON value,
/// as the `toml` crate spells it, so that it is not mistaken for a string.
const DATETIME: &str = "$__toml_private_datetime";
/// The key of the object that stands for a TOML `nan` or `inf`, which JSON
/// numbers cannot hold.
const SPECIAL_FLOAT: &str = "$__toml_private_special_float";

fn tagged(tag: &str, text: String) -> Value {
    Value::Object(serde_json::Map::from_iter([(
        tag.to_owned(),
        Value::String(text),
    )]))
}

/// The tag and text of a value made by [`tagged`].
fn untagged(value: &Value) -> Option<(&str, &str)> {
    let object = value.as_object().filter(|object| object.len() == 1)?;
    let (tag, text) = object.iter().next()?;
    let tag = [DATETIME, SPECIAL_FLOAT]
        .into_iter()
        .find(|known| known == tag)?;
    Some((tag, text.as_str()?))
}

/// The span of the value at `key_path`, where an array item's key is its index.
pub(super) fn value_span(text: &str, key_path: &[String]) -> Option<Range<usize>> {
    let document = Document::parse(text.to_owned()).ok()?;
    let (first, rest) = key_path.split_first()?;
    item_span(document.as_table().get(first)?, rest)
}

fn item_span(item: &Item, key_path: &[String]) -> Option<Range<usize>> {
    let Some((key, rest)) = key_path.split_first() else {
        return item.as_value()?.span();
    };
    match item {
        Item::Table(table) => item_span(table.get(key)?, rest),
        Item::ArrayOfTables(tables) => {
            let table = tables.get(key.parse().ok()?)?;
            let (key, rest) = rest.split_first()?;
            item_span(table.get(key)?, rest)
        }
        Item::Value(value) => value_span_in(value, key_path),
        Item::None => None,
    }
}

fn value_span_in(value: &toml_edit::Value, key_path: &[String]) -> Option<Range<usize>> {
    let Some((key, rest)) = key_path.split_first() else {
        return value.span();
    };
    match value {
        toml_edit::Value::InlineTable(table) => value_span_in(table.get(key)?, rest),
        toml_edit::Value::Array(array) => value_span_in(array.get(key.parse().ok()?)?, rest),
        _ => None,
    }
}
