//! What an Operation writes: a file, or values in the `ItemTable` of a
//! SQLite store, such as VS Code's `state.vscdb`. Edits, diffs, backups and
//! History handle a store's values as one JSON document holding each key
//! and its value's text, so that a value is edited and undone like a key of
//! a file, and a store is never copied whole.

use crate::edit::{self, Syntax};
use crate::ports::FileSystem;
use serde::{Deserialize, Serialize};
use std::io;
use std::path::Path;

/// What a location an Operation writes is.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub(crate) enum Medium {
    /// A file, written whole.
    #[default]
    File,
    /// These keys of the `ItemTable` of a SQLite store, each holding JSON
    /// text, in the order the document holds them.
    SqliteItems { keys: Vec<String> },
}

impl Medium {
    /// Whether it is a file, written whole.
    pub fn is_file(&self) -> bool {
        *self == Medium::File
    }

    /// The medium of a location in a SQLite store whose edits change the
    /// value of each of `keys`, each once.
    pub fn sqlite<'k>(keys: impl IntoIterator<Item = &'k str>) -> Self {
        let mut known: Vec<String> = Vec::new();
        for key in keys {
            if !known.iter().any(|known| known == key) {
                known.push(key.to_owned());
            }
        }
        Medium::SqliteItems { keys: known }
    }
}

/// The bytes of the location at `path`: the file's, or the document holding
/// what the store holds under each key. A missing file or store is an error
/// of kind `NotFound`; a key the store does not hold is left out.
pub(crate) fn read(fs: &dyn FileSystem, path: &Path, medium: &Medium) -> io::Result<Vec<u8>> {
    match medium {
        Medium::File => fs.read(path),
        Medium::SqliteItems { keys } => {
            let mut items = Vec::new();
            for key in keys {
                items.push((key.as_str(), fs.read_sqlite_item(path, key)?));
            }
            Ok(document(&items))
        }
    }
}

/// Writes `bytes` to the location at `path`: the whole file atomically, or
/// what the document holds under each key into the store, deleting a key it
/// no longer holds. A document that does not parse writes nothing.
pub(crate) fn write(
    fs: &dyn FileSystem,
    path: &Path,
    medium: &Medium,
    bytes: &[u8],
) -> io::Result<()> {
    match medium {
        Medium::File => fs.write_atomic(path, bytes),
        Medium::SqliteItems { keys } => {
            let items = values(bytes, keys).ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidData, "the values do not parse")
            })?;
            for (key, value) in items {
                fs.write_sqlite_item(path, key, value.as_deref())?;
            }
            Ok(())
        }
    }
}

/// The document of a store's values, `None` for a key it does not hold,
/// each value's text as the store holds it.
pub(crate) fn document(items: &[(&str, Option<String>)]) -> Vec<u8> {
    let held: Vec<String> = items
        .iter()
        .filter_map(|(key, text)| {
            let text = text.as_ref()?;
            Some(format!("  {}: {text}", serde_json::Value::from(*key)))
        })
        .collect();
    if held.is_empty() {
        b"{\n}\n".to_vec()
    } else {
        format!("{{\n{}\n}}\n", held.join(",\n")).into_bytes()
    }
}

/// What the document `bytes` holds under each of `keys`, as the text of
/// its value there; `None` when the document does not parse.
pub(crate) fn values<'k>(
    bytes: &[u8],
    keys: &'k [String],
) -> Option<Vec<(&'k str, Option<String>)>> {
    let text = std::str::from_utf8(bytes).ok()?;
    let file = edit::EditableFile::decode(bytes, Syntax::Jsonc).ok()?;
    file.value_at(&[]).ok()??;
    Some(
        keys.iter()
            .map(|key| {
                let span = edit::value_span(Syntax::Jsonc, text, std::slice::from_ref(key));
                (key.as_str(), span.map(|span| text[span].to_owned()))
            })
            .collect(),
    )
}

/// The document `bytes` with its values laid out as [`document`] lays them
/// out, so that it holds the same bytes once written and read back.
pub(crate) fn normalized(bytes: &[u8], keys: &[String]) -> Option<Vec<u8>> {
    let items = values(bytes, keys)?;
    let items: Vec<(&str, Option<String>)> = items.into_iter().collect();
    Some(document(&items))
}

/// What a location will hold once an Operation writes it, as a scan reads
/// it to predict the States after.
pub(crate) struct Written {
    pub path: std::path::PathBuf,
    pub medium: Medium,
    pub bytes: Vec<u8>,
}

impl Written {
    /// The text of the file at `path` once written, if it is written.
    pub fn file<'w>(written: &'w [Written], path: &Path) -> Option<&'w [u8]> {
        written
            .iter()
            .find(|known| known.medium == Medium::File && known.path == path)
            .map(|known| known.bytes.as_slice())
    }

    /// What the store at `path` will hold under `key`, if a write sets it:
    /// `Some(None)` once the write deletes it.
    pub fn item(written: &[Written], path: &Path, key: &str) -> Option<Option<String>> {
        written.iter().find_map(|known| {
            let Medium::SqliteItems { keys } = &known.medium else {
                return None;
            };
            if known.path != path || !keys.iter().any(|known| known == key) {
                return None;
            }
            let values = values(&known.bytes, keys)?;
            values
                .into_iter()
                .find(|(known, _)| *known == key)
                .map(|(_, text)| text)
        })
    }
}
