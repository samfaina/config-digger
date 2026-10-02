//! Agent definitions: TOML data over the closed set of building blocks in
//! [`blocks`]. Deserialisation plus `validate()` is the only validation path.

pub(crate) mod blocks;

use blocks::{Anchor, Anchored, Competition, Format, Locator, Validate};
use schemars::JsonSchema;
use serde::Deserialize;
use std::collections::{BTreeMap, BTreeSet};

/// The definitions that ship inside config-digger, embedded at compile time.
const BUILT_IN: &[(&str, &str)] = &[(
    "claude-code.toml",
    include_str!("../../definitions/claude-code.toml"),
)];

/// The only `format_version` this release reads.
const FORMAT_VERSION: u32 = 1;

/// The JSON Schema of an Agent definition file, generated from the block enums.
pub fn json_schema() -> String {
    let schema = schemars::schema_for!(AgentDefinition);
    let mut json = serde_json::to_string_pretty(&schema).expect("a schema serialises");
    json.push('\n');
    json
}

/// A definition file that failed to load. It is rejected whole.
#[derive(Debug, thiserror::Error)]
#[error("Agent definition `{file}` is invalid: {message}")]
pub(crate) struct DefinitionError {
    pub file: String,
    pub message: String,
}

/// The built-in Agent definitions, in load order.
pub(crate) fn built_in() -> Result<Vec<AgentDefinition>, DefinitionError> {
    BUILT_IN
        .iter()
        .map(|(file, text)| parse(file, text))
        .collect()
}

fn parse(file: &str, text: &str) -> Result<AgentDefinition, DefinitionError> {
    let error = |message: String| DefinitionError {
        file: file.to_owned(),
        message,
    };
    let definition: AgentDefinition = toml::from_str(text).map_err(|e| error(e.to_string()))?;
    definition.validate().map_err(error)?;
    Ok(definition)
}

/// Where an Agent keeps its configuration, in which format, and with what precedence.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "config-digger Agent definition")]
pub(crate) struct AgentDefinition {
    /// The version of this file format. Must be 1.
    pub format_version: u32,
    pub agent: AgentInfo,
    /// The Agent homes this Agent can have.
    pub homes: Vec<HomeDefinition>,
    /// The Scopes this Agent applies Entries at.
    pub scopes: Vec<ScopeDefinition>,
    /// The configuration locations this Agent reads.
    pub locations: Vec<LocationDefinition>,
    /// Rules per Item kind.
    pub items: BTreeMap<ItemKind, ItemRules>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct AgentInfo {
    /// A stable id, such as `claude-code`.
    pub id: String,
    /// The display name, such as `Claude Code`.
    pub name: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct HomeDefinition {
    pub id: String,
    /// Named roots of the home. The home is found when any root exists.
    pub roots: BTreeMap<String, Locator>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ScopeDefinition {
    pub id: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct LocationDefinition {
    pub id: String,
    pub anchor: Anchor,
    pub format: Format,
    /// The key path of the object whose keys are Entries.
    pub root: Vec<String>,
    pub kind: ItemKind,
    /// The id of a declared Scope.
    pub scope: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ItemKind {
    Mcp,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ItemRules {
    /// Location ids of this kind, highest precedence first.
    pub precedence: Vec<String>,
    pub competition: Competition,
}

impl AgentDefinition {
    /// The rules for an Item kind. Validation guarantees every location's kind has some.
    pub fn rules(&self, kind: ItemKind) -> &ItemRules {
        &self.items[&kind]
    }

    /// The precedence rank of a location among its kind, 0 being highest.
    pub fn rank(&self, location: &LocationDefinition) -> usize {
        let precedence = &self.rules(location.kind).precedence;
        precedence
            .iter()
            .position(|id| *id == location.id)
            .expect("validation puts every location in its kind's precedence")
    }

    pub fn location(&self, id: &str) -> &LocationDefinition {
        self.locations
            .iter()
            .find(|location| location.id == id)
            .expect("snapshot location ids come from this definition")
    }

    fn validate(&self) -> Result<(), String> {
        if self.format_version != FORMAT_VERSION {
            return Err(format!(
                "format_version {} is not supported; expected {FORMAT_VERSION}",
                self.format_version
            ));
        }
        non_empty("agent.id", &self.agent.id)?;
        non_empty("agent.name", &self.agent.name)?;
        if self.homes.is_empty() {
            return Err("at least one home is required".into());
        }
        unique("home", self.homes.iter().map(|home| home.id.as_str()))?;
        unique("scope", self.scopes.iter().map(|scope| scope.id.as_str()))?;
        unique(
            "location",
            self.locations.iter().map(|location| location.id.as_str()),
        )?;

        for home in &self.homes {
            non_empty("home id", &home.id)?;
            if home.roots.is_empty() {
                return Err(format!("home `{}` declares no roots", home.id));
            }
            for (name, locator) in &home.roots {
                locator
                    .validate()
                    .map_err(|e| format!("home `{}` root `{name}`: {e}", home.id))?;
            }
        }

        for location in &self.locations {
            let context = |e: String| format!("location `{}`: {e}", location.id);
            location.anchor.validate().map_err(context)?;
            location.format.validate().map_err(context)?;
            if let Some(root) = location.anchor.home_root()
                && let Some(home) = self.homes.iter().find(|h| !h.roots.contains_key(root))
            {
                return Err(context(format!(
                    "home `{}` declares no root `{root}`",
                    home.id
                )));
            }
            if !self.scopes.iter().any(|scope| scope.id == location.scope) {
                return Err(context(format!("undeclared scope `{}`", location.scope)));
            }
            if !self.items.contains_key(&location.kind) {
                return Err(context(format!(
                    "no [items.{}] rules for its kind",
                    location.kind.id()
                )));
            }
        }

        for (kind, rules) in &self.items {
            let context = |e: String| format!("items.{}: {e}", kind.id());
            rules.competition.validate().map_err(context)?;
            unique(
                "precedence entry",
                rules.precedence.iter().map(String::as_str),
            )
            .map_err(context)?;
            let declared: BTreeSet<&str> = self
                .locations
                .iter()
                .filter(|location| location.kind == *kind)
                .map(|location| location.id.as_str())
                .collect();
            let ranked: BTreeSet<&str> = rules.precedence.iter().map(String::as_str).collect();
            if let Some(id) = ranked.difference(&declared).next() {
                return Err(context(format!(
                    "precedence names `{id}`, which is not a location of this kind"
                )));
            }
            if let Some(id) = declared.difference(&ranked).next() {
                return Err(context(format!("precedence leaves out location `{id}`")));
            }
        }
        Ok(())
    }
}

impl ItemKind {
    pub fn id(self) -> &'static str {
        match self {
            ItemKind::Mcp => "mcp",
        }
    }
}

fn non_empty(what: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        Err(format!("{what} must not be empty"))
    } else {
        Ok(())
    }
}

fn unique<'a>(what: &str, ids: impl Iterator<Item = &'a str>) -> Result<(), String> {
    let mut seen = BTreeSet::new();
    for id in ids {
        non_empty(&format!("{what} id"), id)?;
        if !seen.insert(id) {
            return Err(format!("{what} `{id}` is declared twice"));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const VALID: &str = r#"
format_version = 1

[agent]
id = "test-agent"
name = "Test Agent"

[[homes]]
id = "default"
roots.state = { block = "path", path = "~/.test.json" }

[[scopes]]
id = "user"

[[locations]]
id = "user-mcp"
anchor = { block = "home", root = "state" }
format = { block = "json" }
root = ["servers"]
kind = "mcp"
scope = "user"

[items.mcp]
precedence = ["user-mcp"]
competition = { block = "exact-name" }
"#;

    fn error(text: &str) -> String {
        parse("test.toml", text).unwrap_err().message
    }

    #[test]
    fn built_in_definitions_load() {
        let ids: Vec<String> = built_in()
            .unwrap()
            .into_iter()
            .map(|definition| definition.agent.id)
            .collect();
        assert_eq!(ids, ["claude-code"]);
    }

    #[test]
    fn a_valid_definition_loads() {
        assert!(parse("test.toml", VALID).is_ok());
    }

    #[test]
    fn unknown_fields_are_rejected() {
        let text = VALID.replace("scope = \"user\"\n", "scope = \"user\"\ncolour = \"red\"\n");
        assert!(error(&text).contains("colour"), "{}", error(&text));
    }

    #[test]
    fn unknown_fields_inside_a_block_are_rejected() {
        let text = VALID.replace(
            r#"{ block = "json" }"#,
            r#"{ block = "json", comments = true }"#,
        );
        assert!(error(&text).contains("comments"), "{}", error(&text));
    }

    #[test]
    fn unknown_blocks_are_rejected() {
        let text = VALID.replace(r#"{ block = "json" }"#, r#"{ block = "yaml" }"#);
        assert!(error(&text).contains("yaml"), "{}", error(&text));
    }

    #[test]
    fn an_unsupported_format_version_is_rejected() {
        let text = VALID.replace("format_version = 1", "format_version = 2");
        assert!(
            error(&text).contains("format_version 2"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn a_home_path_outside_the_user_folder_is_rejected() {
        let text = VALID.replace("~/.test.json", "C:/test.json");
        assert!(
            error(&text).contains("must start with `~/`"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn a_relative_path_that_climbs_out_is_rejected() {
        let text = VALID.replace("~/.test.json", "~/../other/.test.json");
        assert!(
            error(&text).contains("without `.` or `..`"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn an_anchor_to_an_undeclared_root_is_rejected() {
        let text = VALID.replace(r#"root = "state""#, r#"root = "config""#);
        assert!(
            error(&text).contains("no root `config`"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn a_location_in_an_undeclared_scope_is_rejected() {
        let text = VALID.replace("scope = \"user\"\n", "scope = \"local\"\n");
        assert!(
            error(&text).contains("undeclared scope `local`"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn precedence_must_rank_every_location_of_the_kind() {
        let text = VALID.replace(r#"precedence = ["user-mcp"]"#, "precedence = []");
        assert!(
            error(&text).contains("leaves out location `user-mcp`"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn precedence_must_not_name_unknown_locations() {
        let text = VALID.replace(r#"["user-mcp"]"#, r#"["user-mcp", "ghost"]"#);
        assert!(error(&text).contains("`ghost`"), "{}", error(&text));
    }

    #[test]
    fn duplicate_ids_are_rejected() {
        let text = format!("{VALID}\n[[scopes]]\nid = \"user\"\n");
        assert!(
            error(&text).contains("scope `user` is declared twice"),
            "{}",
            error(&text)
        );
    }
}
