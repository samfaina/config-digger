//! The Agent definition JSON Schema, `schema/agent-definition.schema.json`,
//! as an editor checks a definition file against it.

use config_digger_core::definitions::json_schema;
use serde_json::Value;

fn validator() -> jsonschema::Validator {
    let schema: Value = serde_json::from_str(&json_schema()).unwrap();
    jsonschema::validator_for(&schema).unwrap()
}

/// The built-in Claude Desktop definition, as JSON.
fn claude_desktop() -> Value {
    toml::from_str(include_str!("../definitions/claude-desktop.toml")).unwrap()
}

/// The schema's errors for `definition`, each with its place.
fn errors(definition: &Value) -> Vec<String> {
    validator()
        .iter_errors(definition)
        .map(|error| format!("{}: {error}", error.instance_path()))
        .collect()
}

#[test]
fn the_schema_accepts_claude_desktop_s_msix_package_installations() {
    let definition = claude_desktop();

    assert_eq!(
        definition["installations"][0]["folder"],
        serde_json::json!({ "block": "msix-package", "family": "Claude_pzs8sxrjxfjjc" })
    );
    assert_eq!(errors(&definition), Vec::<String>::new());
}

#[test]
fn the_schema_rejects_an_msix_package_without_a_family() {
    let mut definition = claude_desktop();
    definition["installations"][0]["folder"]
        .as_object_mut()
        .unwrap()
        .remove("family");

    assert_ne!(errors(&definition), Vec::<String>::new());
}

#[test]
fn the_schema_rejects_a_package_family_without_its_publisher_id() {
    let mut definition = claude_desktop();
    definition["installations"][0]["folder"]["family"] = "Claude_*".into();

    assert_ne!(errors(&definition), Vec::<String>::new());
}
