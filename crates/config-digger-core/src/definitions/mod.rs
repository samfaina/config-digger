//! Agent definitions: TOML data over the closed set of building blocks in
//! [`blocks`]. Deserialisation plus `validate()` is the only validation path.

pub(crate) mod blocks;

use blocks::{
    Anchor, Anchored, Competition, DisableMechanism, Disables, Format, Interpolation, Locator,
    Provenance, Register, Registry, Validate,
};
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
    /// The Scopes this Agent applies Entries at, in display order. A Scope is
    /// only a label: precedence comes from the order of locations in `items`.
    pub scopes: Vec<ScopeDefinition>,
    /// Where this Agent records the Projects it has been started on.
    #[serde(default)]
    pub projects: ProjectsDefinition,
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

#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ProjectsDefinition {
    /// Each registry adds the Projects it records. Projects found by any Agent
    /// are evaluated for every Agent.
    #[serde(default)]
    pub registries: Vec<Registry>,
}

/// The `root` segment that stands for any Project path key.
pub(crate) const PROJECT_KEY: &str = "{project}";

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct LocationDefinition {
    pub id: String,
    pub anchor: Anchor,
    pub format: Format,
    /// The key path of the object whose keys are Entries. One segment may be
    /// `{project}`: each key there is a Project path, and the Entries below it
    /// apply only in that Project.
    pub root: Vec<String>,
    pub kind: ItemKind,
    /// The id of a declared Scope.
    pub scope: String,
    /// The base Provenance of the location's Entries.
    pub provenance: Provenance,
    /// How the Agent writes references in this location's values. Without
    /// it, every value is literal text.
    pub interpolation: Option<Interpolation>,
    /// Fields of this location's Entries that hold Secrets, or that never
    /// do, beyond what the core masks by default.
    #[serde(default)]
    pub secrets: SecretFields,
}

/// Key paths below an Entry, one key per segment, where `*` stands for any
/// key or array index.
#[derive(Debug, Default, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct SecretFields {
    /// Fields whose values are Secrets. Below an object or array, every
    /// value is.
    #[serde(default)]
    pub fields: Vec<Vec<String>>,
    /// Fields that hold the name of a variable, such as Codex's
    /// `bearer_token_env_var`: never a Secret, whatever else says so.
    #[serde(default)]
    pub variable_names: Vec<Vec<String>>,
    /// Key paths from the root of the file, outside any Entry, whose values
    /// are Secrets, such as an API key stored next to the Entries. They are
    /// masked wherever the file's text is shown, as in a diff's context lines.
    #[serde(default)]
    pub file_fields: Vec<Vec<String>>,
}

impl Validate for SecretFields {
    fn validate(&self) -> Result<(), String> {
        for field in self
            .fields
            .iter()
            .chain(&self.variable_names)
            .chain(&self.file_fields)
        {
            if field.is_empty() || field.iter().any(String::is_empty) {
                return Err(format!(
                    "secret field {field:?} needs keys, none of them empty"
                ));
            }
        }
        Ok(())
    }
}

impl LocationDefinition {
    /// Whether the location's Entries are keyed by Project inside the file.
    pub fn keyed_by_project(&self) -> bool {
        self.root.iter().any(|key| key == PROJECT_KEY)
    }
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
    /// How the Agent Disables an Entry, in its order of preference.
    /// Disable uses the first that covers the Entry and applies in the
    /// context, skipping any whose target is git-versioned.
    #[serde(default)]
    pub disable: Vec<DisableMechanism>,
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

        for (index, registry) in self.projects.registries.iter().enumerate() {
            let context = |e: String| format!("projects.registries[{index}]: {e}");
            registry.validate().map_err(context)?;
            self.validate_home_root(registry.anchor())
                .map_err(context)?;
        }

        for location in &self.locations {
            let context = |e: String| format!("location `{}`: {e}", location.id);
            location.anchor.validate().map_err(context)?;
            location.format.validate().map_err(context)?;
            if let Some(interpolation) = &location.interpolation {
                interpolation.validate().map_err(context)?;
            }
            location.secrets.validate().map_err(context)?;
            self.validate_home_root(&location.anchor).map_err(context)?;
            let project_keys = location.root.iter().filter(|key| *key == PROJECT_KEY);
            if project_keys.count() > 1 {
                return Err(context(format!("root holds `{PROJECT_KEY}` twice")));
            }
            if location.keyed_by_project() && location.anchor.in_project() {
                return Err(context(format!(
                    "a location in a Project cannot be keyed by `{PROJECT_KEY}`"
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
            for (index, mechanism) in rules.disable.iter().enumerate() {
                let context = |e: String| context(format!("disable[{index}]: {e}"));
                mechanism.validate().map_err(context)?;
                self.validate_home_root(&mechanism.target().anchor)
                    .map_err(context)?;
                if let Some(id) = mechanism
                    .locations()
                    .iter()
                    .find(|id| !declared.contains(id.as_str()))
                {
                    return Err(context(format!("`{id}` is not a location of this kind")));
                }
            }
        }
        Ok(())
    }
}

impl AgentDefinition {
    /// Every home must declare the root an anchor needs.
    fn validate_home_root(&self, anchor: &Anchor) -> Result<(), String> {
        if let Some(root) = anchor.home_root()
            && let Some(home) = self.homes.iter().find(|h| !h.roots.contains_key(root))
        {
            return Err(format!("home `{}` declares no root `{root}`", home.id));
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
provenance = "file"

[items.mcp]
precedence = ["user-mcp"]
competition = { block = "exact-name" }
"#;

    /// `VALID` plus a project registry and a location keyed by Project.
    fn with_projects(registry_anchor: &str, local_anchor: &str, local_root: &str) -> String {
        let local = format!(
            r#"
[[locations]]
id = "local-mcp"
anchor = {local_anchor}
format = {{ block = "json" }}
root = {local_root}
kind = "mcp"
scope = "user"
provenance = "file"
"#
        );
        VALID
            .replace(
                "[[locations]]",
                &format!(
                    "[projects]\nregistries = [{{ block = \"json-keys\", anchor = {registry_anchor}, root = [\"projects\"] }}]\n\n{local}\n[[locations]]"
                ),
            )
            .replace(r#"["user-mcp"]"#, r#"["local-mcp", "user-mcp"]"#)
    }

    const STATE: &str = r#"{ block = "home", root = "state" }"#;
    const IN_PROJECT: &str = r#"{ block = "project", path = ".test.json" }"#;

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
    fn a_definition_with_projects_loads() {
        let text = with_projects(STATE, STATE, r#"["projects", "{project}", "servers"]"#);
        assert!(parse("test.toml", &text).is_ok(), "{}", error(&text));
        let text = with_projects(STATE, IN_PROJECT, r#"["servers"]"#);
        assert!(parse("test.toml", &text).is_ok(), "{}", error(&text));
    }

    #[test]
    fn a_project_registry_in_a_project_is_rejected() {
        let text = with_projects(IN_PROJECT, STATE, r#"["servers"]"#);
        assert!(
            error(&text).contains("must not lie in a Project"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn a_project_registry_with_an_undeclared_root_is_rejected() {
        let text = with_projects(
            r#"{ block = "home", root = "config" }"#,
            STATE,
            r#"["servers"]"#,
        );
        assert!(
            error(&text).contains("no root `config`"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn a_location_in_a_project_cannot_be_keyed_by_project() {
        let text = with_projects(STATE, IN_PROJECT, r#"["{project}", "servers"]"#);
        assert!(
            error(&text).contains("cannot be keyed by `{project}`"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn a_root_holds_at_most_one_project_key() {
        let text = with_projects(STATE, STATE, r#"["{project}", "{project}"]"#);
        assert!(
            error(&text).contains("`{project}` twice"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn a_location_declares_its_interpolation_and_secret_fields() {
        let text = VALID.replace(
            "provenance = \"file\"\n",
            "provenance = \"file\"\ninterpolation = { block = \"braces\", namespaces = [\"env\"], default_separator = \":-\" }\nsecrets.fields = [[\"auth\", \"*\"]]\nsecrets.variable_names = [[\"token_env_var\"]]\n",
        );
        assert!(parse("test.toml", &text).is_ok(), "{}", error(&text));
    }

    #[test]
    fn an_empty_secret_field_key_is_rejected() {
        let text = VALID.replace(
            "provenance = \"file\"\n",
            "provenance = \"file\"\nsecrets.fields = [[\"auth\", \"\"]]\n",
        );
        assert!(error(&text).contains("secret field"), "{}", error(&text));
    }

    #[test]
    fn an_interpolation_namespace_must_be_a_name() {
        let text = VALID.replace(
            "provenance = \"file\"\n",
            "provenance = \"file\"\ninterpolation = { block = \"braces\", namespaces = [\"e:nv\"] }\n",
        );
        assert!(
            error(&text).contains("namespace `e:nv`"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn a_location_needs_a_provenance() {
        let text = VALID.replace("provenance = \"file\"\n", "");
        assert!(error(&text).contains("provenance"), "{}", error(&text));
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

    /// `VALID` with `mechanism` as the one Disable mechanism of its servers.
    fn with_disable(mechanism: &str) -> String {
        format!(
            "{VALID}disable = [{mechanism}]
"
        )
    }

    #[test]
    fn a_disable_mechanism_loads() {
        let text = with_disable(
            r#"{ block = "json-name-list", target = { anchor = { block = "home", root = "state" }, key_path = ["projects", "{project}", "off"] }, locations = ["user-mcp"] }"#,
        );
        assert!(parse("test.toml", &text).is_ok(), "{}", error(&text));
    }

    #[test]
    fn a_disable_mechanism_for_an_unknown_location_is_rejected() {
        let text = with_disable(
            r#"{ block = "json-name-list", target = { anchor = { block = "home", root = "state" }, key_path = ["off"] }, locations = ["ghost"] }"#,
        );
        assert!(
            error(&text).contains("`ghost` is not a location of this kind"),
            "{}",
            error(&text)
        );
    }

    #[test]
    fn a_disable_target_in_a_project_cannot_be_keyed_by_project() {
        let text = with_disable(
            r#"{ block = "json-name-list", target = { anchor = { block = "project", path = ".test.json" }, key_path = ["{project}", "off"] } }"#,
        );
        assert!(
            error(&text).contains("cannot be keyed by `{project}`"),
            "{}",
            error(&text)
        );
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
