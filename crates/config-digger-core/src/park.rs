//! The parked-Entry store (ADR 0002): where Disable moves an Entry that has
//! no switch of its own, out of the Agent's reach, until Enable puts it back.
//! It is one JSON manifest in config-digger's data folder, holding each
//! parked Entry under `entries.<id>` with where it came from. The store is
//! part of the inventory, not a backup: History never prunes it. Parking and
//! putting back are edits of the Entry's file and of the manifest, so they
//! are backed up, verified and undone as any other Operation.

use crate::api::Error;
use crate::definitions::{AgentDefinition, LocationDefinition};
use crate::edit::Edit;
use crate::history;
use crate::ports::Environment;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use std::path::PathBuf;

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
    /// Its value, verbatim, Secrets included.
    pub entry: Value,
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
            entry: value.clone(),
        })
    }

    /// The edit that puts the Entry back into its file.
    pub fn put_back(&self) -> Edit {
        Edit::Insert {
            key_path: self.origin.key_path.clone(),
            value: self.entry.clone(),
            after: self.after.clone(),
            inline: self.inline,
            comments: self.comments.clone(),
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

/// Every parked Entry the manifest `document` holds, by id. One that does
/// not read as a parked Entry is left out, and logged without its value.
pub(crate) fn parked_entries(document: &Value) -> Vec<(String, ParkedEntry)> {
    entries(document)
        .into_iter()
        .flatten()
        .filter_map(
            |(id, parked)| match serde_json::from_value(parked.clone()) {
                Ok(parked) => Some((id.clone(), parked)),
                Err(_) => {
                    tracing::warn!(%id, "cannot read a parked Entry");
                    None
                }
            },
        )
        .collect()
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
/// location it was parked from when its Agent and location are loaded.
/// Parked Entries are read as they are, so that the value of one that does
/// not read as a parked Entry is found too.
pub(crate) fn raw_entries<'d>(
    document: &'d Value,
    definitions: &'d [AgentDefinition],
) -> Vec<(&'d str, &'d Value, Option<&'d LocationDefinition>)> {
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
            Some((id.as_str(), value, location))
        })
        .collect()
}
