//! `unverified` marks: a sentence on any table of a definition saying what
//! it assumes, for a fact taken from inference rather than from docs, disk
//! or an experiment, or a default taken because the fact is not known (ADR
//! 0005). Marks are taken out of a definition before it is deserialised, so
//! they change nothing it does; they exist so that a person and CI can list
//! what the release gate still has to test.

use schemars::generate::SchemaSettings;
use schemars::transform::RecursiveTransform;
use schemars::{JsonSchema, Schema};
use serde::de::DeserializeOwned;
use toml::Spanned;
use toml::de::{DeTable, DeValue};

/// The key of a mark, which every table of a definition may hold, so no
/// table, not even a map of names, can hold a key of its own by that name.
const KEY: &str = "unverified";

/// The schema definition of a mark.
const SCHEMA_NAME: &str = "Unverified";

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
    /// For a mark under the release gate's exception, the account or the
    /// deployment its test needs, which the project does not have: the
    /// release gate lets such a mark ship, and the release notes list it.
    pub needs: Option<String>,
}

/// `<file>:<line>: <table>: <sentence>`, the table `(top)` for the top of
/// the file, followed by `Its test needs <needs>.` for a mark under the
/// exception.
impl std::fmt::Display for Mark {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let table = if self.location.is_empty() {
            "(top)"
        } else {
            &self.location
        };
        write!(f, "{}:{}: {table}: {}", self.file, self.line, self.sentence)?;
        if let Some(needs) = &self.needs {
            write!(f, " Its test needs {needs}.")?;
        }
        Ok(())
    }
}

/// Deserialises the definition `text` of `file` into `T`, without its
/// marks, and returns them in the order they appear.
pub(crate) fn deserialize<T: DeserializeOwned>(
    file: &str,
    text: &str,
) -> Result<(T, Vec<Mark>), String> {
    let mut root = DeTable::parse(text).map_err(|e| e.to_string())?;
    let mut marks = Marks {
        file,
        text,
        found: Vec::new(),
    };
    marks.take(root.get_mut(), "")?;
    let value = T::deserialize(toml::de::Deserializer::from(root)).map_err(|mut e| {
        e.set_input(Some(text));
        e.to_string()
    })?;
    let mut found = marks.found;
    found.sort_by_key(|mark| mark.line);
    Ok((value, found))
}

/// The marks taken out of the definition `text` of `file` so far.
struct Marks<'a> {
    file: &'a str,
    text: &'a str,
    found: Vec<Mark>,
}

impl Marks<'_> {
    /// Takes the marks out of `table`, at key path `location`, and out of
    /// every table inside it. A table holds one mark or several, a list of
    /// them. A mark is a sentence, or, under the exception, a table of its
    /// `sentence` and what its test `needs`.
    fn take(&mut self, table: &mut DeTable<'_>, location: &str) -> Result<(), String> {
        if let Some((_, value)) = table.remove_entry(KEY) {
            let at = child(location, KEY);
            let items: Vec<&Spanned<DeValue<'_>>> = match value.get_ref() {
                DeValue::Array(items) => items.iter().collect(),
                _ => vec![&value],
            };
            if items.is_empty() {
                return Err(format!("{at}: needs at least one sentence"));
            }
            for item in items {
                let (sentence, needs) = match item.get_ref() {
                    DeValue::Table(mark) => {
                        if let Some(key) = mark
                            .keys()
                            .find(|key| !["sentence", "needs"].contains(&key.get_ref().as_ref()))
                        {
                            return Err(format!("{at}: unknown key `{}`", key.get_ref()));
                        }
                        let needs = mark.get("needs").and_then(text).ok_or_else(|| {
                            format!("{at}: `needs` must name the account or the deployment the test needs")
                        })?;
                        (mark.get("sentence").and_then(text), Some(needs))
                    }
                    _ => (text(item), None),
                };
                let sentence = sentence
                    .ok_or_else(|| format!("{at}: must be a sentence saying what is assumed"))?;
                self.found.push(Mark {
                    file: self.file.to_owned(),
                    line: self.text[..item.span().start].matches('\n').count() + 1,
                    location: location.to_owned(),
                    sentence: sentence.to_owned(),
                    needs: needs.map(str::to_owned),
                });
            }
        }
        for (key, value) in table.iter_mut() {
            self.take_in(value, &child(location, key.get_ref()))?;
        }
        Ok(())
    }

    /// Takes the marks out of every table in `value`, at key path `location`.
    fn take_in(&mut self, value: &mut Spanned<DeValue<'_>>, location: &str) -> Result<(), String> {
        match value.get_mut() {
            DeValue::Table(table) => self.take(table, location),
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
                    self.take_in(item, &location)?;
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}

/// The text of `value`, unless it is not text or is only blanks.
fn text<'v>(value: &'v Spanned<DeValue<'_>>) -> Option<&'v str> {
    value
        .get_ref()
        .as_str()
        .filter(|text| !text.trim().is_empty())
}

/// The key path of `key` in the table at key path `location`.
fn child(location: &str, key: &str) -> String {
    if location.is_empty() {
        key.to_owned()
    } else {
        format!("{location}.{key}")
    }
}

/// The JSON Schema of a definition file whose top is `T`, in which every
/// table may hold a mark.
pub(crate) fn json_schema<T: JsonSchema>() -> String {
    let mut schema = SchemaSettings::default()
        .with_transform(RecursiveTransform(|schema: &mut Schema| {
            if schema.get("type") == Some(&serde_json::json!("object")) {
                let reference = serde_json::json!({ "$ref": format!("#/$defs/{SCHEMA_NAME}") });
                object_at(schema, "properties").insert(KEY.into(), reference);
            }
        }))
        .into_generator()
        .into_root_schema_for::<T>();
    let sentence = serde_json::json!({ "type": "string", "pattern": r"\S" });
    let under_exception = serde_json::json!({
        "type": "object",
        "properties": {
            "sentence": sentence,
            "needs": sentence,
        },
        "required": ["sentence", "needs"],
        "additionalProperties": false,
    });
    object_at(&mut schema, "$defs").insert(
        SCHEMA_NAME.into(),
        serde_json::json!({
            "description": "Facts of this table taken from inference, or defaults taken because a fact is not known, rather than from docs, disk or an experiment: a sentence saying what is assumed, or a list of them, which the release gate has to test. A fact whose test needs an account or a deployment the project does not have is under the release gate's exception: its mark is a table of the `sentence` and what the test `needs`, which the release gate lets ship and the release notes list. It changes nothing the definition does.",
            "anyOf": [
                sentence,
                under_exception,
                {
                    "type": "array",
                    "items": { "anyOf": [sentence, under_exception] },
                    "minItems": 1,
                },
            ],
        }),
    );
    let mut json = serde_json::to_string_pretty(&schema).expect("a schema serialises");
    json.push('\n');
    json
}

/// The object under `key` in `schema`, added empty when missing.
fn object_at<'s>(
    schema: &'s mut Schema,
    key: &str,
) -> &'s mut serde_json::Map<String, serde_json::Value> {
    schema
        .ensure_object()
        .entry(key)
        .or_insert_with(|| serde_json::json!({}))
        .as_object_mut()
        .expect("a schema keyword holding schemas is an object")
}
