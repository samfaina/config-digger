//! The parked-Entry store (ADR 0002): where Disable moves an Entry that has
//! no switch of its own, out of the Agent's reach, until Enable puts it back.
//! It is one JSON manifest in config-digger's data folder, holding each
//! parked Entry under `entries.<id>` with where it came from, and beside it
//! a `folders` folder holding each parked skill's folder under its id. The
//! store is part of the inventory, not a backup: History never prunes it.
//! Parking and putting back are edits of the Entry's file and of the
//! manifest, and moves of a skill's folder, so they are backed up, verified
//! and undone as any other Operation.

use crate::api::Error;
use crate::definitions::{AgentDefinition, LocationDefinition};
use crate::edit::Edit;
use crate::history;
use crate::ports::Environment;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

/// The key of the manifest that holds the parked Entries, by id.
const ENTRIES: &str = "entries";

/// The key of a parked Entry in the manifest that holds its value.
pub(crate) const ENTRY: &str = "entry";

/// The store's manifest: `%LOCALAPPDATA%\config-digger\parked\manifest.json`.
pub(crate) fn store_path(env: &dyn Environment) -> Result<PathBuf, Error> {
    Ok(history::data_folder(env)?
        .join("parked")
        .join("manifest.json"))
}

/// The folder that holds the folders of parked skills, beside the manifest
/// at `store`.
pub(crate) fn folders_path(store: &Path) -> PathBuf {
    store.with_file_name("folders")
}

/// The key path of the parked Entry `id` in the manifest.
pub(crate) fn key_path(id: &str) -> Vec<String> {
    vec![ENTRIES.to_owned(), id.to_owned()]
}

/// One parked Entry as the manifest holds it, with where it came from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct ParkedEntry {
    #[serde(flatten)]
    pub origin: Origin,
    /// When it was parked, in RFC 3339.
    pub parked_at: String,
    /// The sibling it followed, how it was written and the comments that
    /// went with it, so that Enable puts it back as it was.
    #[serde(default)]
    pub after: Option<String>,
    #[serde(default)]
    pub inline: bool,
    #[serde(default)]
    pub comments: Vec<String>,
    /// For a skill, the name its folder has in the store's `folders`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub folder: Option<String>,
    /// For a skill reached through a link, where the link pointed: parking
    /// it took the link away, and Enable creates it again.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub link: Option<PathBuf>,
    /// Its value, verbatim, Secrets included.
    pub entry: Value,
}

/// What the store keeps of a parked skill.
pub(crate) enum Kept {
    /// Its folder, under this name in the store's `folders`.
    Folder(String),
    /// Where the link it was reached through pointed.
    Link(PathBuf),
}

/// Where a parked Entry came from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Origin {
    /// The id of the Agent, its home and the location it was read from.
    pub agent: String,
    pub home: String,
    pub location: String,
    pub name: String,
    /// The file it was in, and its key path there.
    pub path: PathBuf,
    pub key_path: Vec<String>,
    /// The key of the Project it applied in, `None` for every context.
    #[serde(default)]
    pub project: Option<String>,
}

impl ParkedEntry {
    /// The Entry from `origin` that `removal`, the inverse of the edit that
    /// took it out of its file, puts back.
    pub fn new(origin: Origin, parked_at: String, removal: &Edit) -> Option<ParkedEntry> {
        let Edit::Insert {
            value,
            after,
            inline,
            comments,
            ..
        } = removal
        else {
            return None;
        };
        Some(ParkedEntry {
            origin,
            parked_at,
            after: after.clone(),
            inline: *inline,
            comments: comments.clone(),
            folder: None,
            link: None,
            entry: value.clone(),
        })
    }

    /// The skill from `origin`, with the value `entry`, whose folder goes
    /// into the store's `folders` under the name `folder`, or for a skill
    /// reached through a link, whose link to `link` goes away.
    pub fn skill(origin: Origin, parked_at: String, entry: Value, kept: Kept) -> ParkedEntry {
        let (folder, link) = match kept {
            Kept::Folder(name) => (Some(name), None),
            Kept::Link(target) => (None, Some(target)),
        };
        ParkedEntry {
            origin,
            parked_at,
            after: None,
            inline: false,
            comments: Vec::new(),
            folder,
            link,
            entry,
        }
    }

    /// The edit that puts the Entry back into its file.
    pub fn put_back(&self) -> Edit {
        Edit::Insert {
            key_path: self.origin.key_path.clone(),
            value: self.entry.clone(),
            after: self.after.clone(),
            inline: self.inline,
            comments: self.comments.clone(),
            emptied_object: None,
        }
    }

    /// The value the manifest holds for it.
    pub fn to_value(&self) -> Value {
        serde_json::to_value(self).expect("a parked Entry serialises")
    }
}

/// The parked Entries of the manifest `document`, by id, as they are.
fn entries(document: &Value) -> Option<&Map<String, Value>> {
    document.get(ENTRIES).and_then(Value::as_object)
}

/// What a manifest holds, as [`parked_entries`] reads it.
#[derive(Default)]
pub(crate) struct Held {
    /// Every parked Entry, by id.
    pub parked: Vec<(String, ParkedEntry)>,
    /// The ids of the Entries that do not read as parked Entries.
    pub unreadable: Vec<String>,
}

/// Every Entry the manifest `document` holds, one that does not read as a
/// parked Entry logged without its value. `None` when the manifest is not
/// an object, or holds something else than an object where its Entries are.
pub(crate) fn parked_entries(document: &Value) -> Option<Held> {
    let entries = match document.get(ENTRIES) {
        None if document.is_object() => return Some(Held::default()),
        entries => entries?.as_object()?,
    };
    let mut held = Held::default();
    for (id, parked) in entries {
        match serde_json::from_value(parked.clone()) {
            Ok(parked) => held.parked.push((id.clone(), parked)),
            Err(_) => {
                tracing::warn!(%id, "cannot read a parked Entry");
                held.unreadable.push(id.clone());
            }
        }
    }
    Some(held)
}

/// The ids of the parked Entries the manifest `document` holds, read or not.
pub(crate) fn ids(document: &Value) -> Vec<String> {
    entries(document)
        .into_iter()
        .flatten()
        .map(|(id, _)| id.clone())
        .collect()
}

/// An id for a parked Entry named `name` that none of `taken` is: its name,
/// or else its name with the first free number.
pub(crate) fn free_id(name: &str, taken: &[String]) -> String {
    let free = |id: &String| !taken.iter().any(|known| known == id);
    std::iter::once(name.to_owned())
        .chain((2..).map(|number| format!("{name}~{number}")))
        .find(free)
        .expect("some number is free")
}

/// Each parked Entry's value in the manifest `document`, by id, with the
/// location it was parked from when its Agent and location are loaded, and
/// the file it was parked from.
/// Parked Entries are read as they are, so that the value of one that does
/// not read as a parked Entry is found too.
pub(crate) fn raw_entries<'d>(
    document: &'d Value,
    definitions: &'d [AgentDefinition],
) -> Vec<(
    &'d str,
    &'d Value,
    Option<&'d LocationDefinition>,
    Option<&'d Path>,
)> {
    entries(document)
        .into_iter()
        .flatten()
        .filter_map(|(id, parked)| {
            let value = parked.get(ENTRY)?;
            let text = |key: &str| parked.get(key).and_then(Value::as_str);
            let location = definitions
                .iter()
                .find(|definition| Some(definition.agent.id.as_str()) == text("agent"))
                .and_then(|definition| {
                    definition
                        .locations
                        .iter()
                        .find(|location| Some(location.id.as_str()) == text("location"))
                });
            Some((id.as_str(), value, location, text("path").map(Path::new)))
        })
        .collect()
}
