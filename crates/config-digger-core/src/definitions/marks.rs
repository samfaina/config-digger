//! `unverified` marks: a sentence on any table of a definition saying what
//! it assumes, for a fact taken from inference rather than from docs, disk
//! or an experiment (ADR 0005). Marks are taken out of a definition before
//! it is deserialised, so they change nothing it does; they exist so that
//! a person and CI can list what the release gate still has to test.

use schemars::generate::SchemaSettings;
use schemars::transform::RecursiveTransform;
use schemars::{JsonSchema, Schema};
use serde::de::DeserializeOwned;
use toml::Spanned;
use toml::de::{DeTable, DeValue};

/// The key of a mark, which every table of a definition may hold.
pub(crate) const KEY: &str = "unverified";

/// One `unverified` mark of a definition file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mark {
    /// The definition file, as named when it was loaded.
    pub file: String,
    /// The 1-based line of the mark in the file.
    pub line: usize,
    /// The table that holds the mark, as a key path: `items.mcp.gates[0]`,
    /// or `locations[project-mcp]` for an entry of an array of tables that
    /// has an `id`, or else a `name`. Empty for the top of the file.
    pub location: String,
    /// What the fact assumes.
    pub sentence: String,
}

/// Deserialises the definition `text` of `file` into `T`, without its
/// marks, and returns them in the order they appear.
pub(crate) fn deserialize<T: DeserializeOwned>(
    file: &str,
    text: &str,
) -> Result<(T, Vec<Mark>), String> {
    let mut root = DeTable::parse(text).map_err(|e| e.to_string())?;
    let mut marks = Vec::new();
    take(root.get_mut(), "", file, text, &mut marks)?;
    let value = T::deserialize(toml::de::Deserializer::from(root)).map_err(|mut e| {
        e.set_input(Some(text));
        e.to_string()
    })?;
    marks.sort_by_key(|mark| mark.line);
    Ok((value, marks))
}

/// Takes the marks out of `table`, at key path `location`, and out of every
/// table inside it. A table holds one mark, a sentence, or several, a list
/// of them.
fn take(
    table: &mut DeTable<'_>,
    location: &str,
    file: &str,
    text: &str,
    marks: &mut Vec<Mark>,
) -> Result<(), String> {
    if let Some((_, value)) = table.remove_entry(KEY) {
        let sentences: Vec<&Spanned<DeValue<'_>>> = match value.get_ref() {
            DeValue::Array(sentences) => sentences.iter().collect(),
            _ => vec![&value],
        };
        let dot = if location.is_empty() { "" } else { "." };
        if sentences.is_empty() {
            return Err(format!("{location}{dot}{KEY}: needs at least one sentence"));
        }
        for item in sentences {
            let sentence = item
                .get_ref()
                .as_str()
                .filter(|sentence| !sentence.trim().is_empty())
                .ok_or_else(|| {
                    format!("{location}{dot}{KEY}: must be a sentence saying what is assumed")
                })?;
            marks.push(Mark {
                file: file.to_owned(),
                line: text[..item.span().start].matches('\n').count() + 1,
                location: location.to_owned(),
                sentence: sentence.to_owned(),
            });
        }
    }
    for (key, value) in table.iter_mut() {
        let location = if location.is_empty() {
            key.get_ref().to_string()
        } else {
            format!("{location}.{}", key.get_ref())
        };
        take_in(value, &location, file, text, marks)?;
    }
    Ok(())
}

/// Takes the marks out of every table in `value`, at key path `location`.
fn take_in(
    value: &mut Spanned<DeValue<'_>>,
    location: &str,
    file: &str,
    text: &str,
    marks: &mut Vec<Mark>,
) -> Result<(), String> {
    match value.get_mut() {
        DeValue::Table(table) => take(table, location, file, text, marks),
        DeValue::Array(array) => {
            for (index, item) in array.iter_mut().enumerate() {
                let id = match item.get_ref() {
                    DeValue::Table(table) => ["id", "name"]
                        .iter()
                        .find_map(|key| table.get(*key)?.get_ref().as_str())
                        .map(str::to_owned),
                    _ => None,
                };
                let location = match id {
                    Some(id) => format!("{location}[{id}]"),
                    None => format!("{location}[{index}]"),
                };
                take_in(item, &location, file, text, marks)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

/// The JSON Schema of a definition file whose top is `T`, in which every
/// table may hold a mark.
pub(crate) fn json_schema<T: JsonSchema>() -> String {
    let mut schema = SchemaSettings::default()
        .with_transform(RecursiveTransform(|schema: &mut Schema| {
            if schema.get("type") == Some(&serde_json::json!("object")) {
                let properties = schema
                    .ensure_object()
                    .entry("properties")
                    .or_insert_with(|| serde_json::json!({}));
                if let Some(properties) = properties.as_object_mut() {
                    properties.insert(
                        KEY.into(),
                        serde_json::json!({ "$ref": "#/$defs/Unverified" }),
                    );
                }
            }
        }))
        .into_generator()
        .into_root_schema_for::<T>();
    let definitions = schema
        .ensure_object()
        .entry("$defs")
        .or_insert_with(|| serde_json::json!({}));
    if let Some(definitions) = definitions.as_object_mut() {
        let sentence = serde_json::json!({ "type": "string", "pattern": r"\S" });
        definitions.insert(
            "Unverified".into(),
            serde_json::json!({
                "description": "Facts of this table taken from inference, or defaults taken because a fact is not known, rather than from docs, disk or an experiment: a sentence saying what is assumed, or a list of them, which the release gate has to test. It changes nothing the definition does.",
                "anyOf": [
                    sentence,
                    { "type": "array", "items": sentence, "minItems": 1 },
                ],
            }),
        );
    }
    let mut json = serde_json::to_string_pretty(&schema).expect("a schema serialises");
    json.push('\n');
    json
}
