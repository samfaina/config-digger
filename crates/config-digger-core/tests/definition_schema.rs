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

/// The built-in Claude Code definition, as JSON.
fn claude_code() -> Value {
    toml::from_str(include_str!("../definitions/claude-code.toml")).unwrap()
}

/// The anchor of the location `id` of `definition`.
fn anchor<'d>(definition: &'d mut Value, id: &str) -> &'d mut Value {
    definition["locations"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|location| location["id"] == id)
        .unwrap()
        .get_mut("anchor")
        .unwrap()
}

#[test]
fn the_schema_accepts_a_main_checkout_fallback_for_a_linked_worktree() {
    let mut definition = claude_code();

    assert_eq!(
        anchor(&mut definition, "project-skills")["linked_worktree"],
        "main-checkout-fallback"
    );
    assert_eq!(errors(&definition), Vec::<String>::new());
}

#[test]
fn the_schema_rejects_an_unknown_linked_worktree_rule() {
    let mut definition = claude_code();
    anchor(&mut definition, "project-skills")["linked_worktree"] = "main-checkout-adds".into();

    assert_ne!(errors(&definition), Vec::<String>::new());
}

/// The registry of the plugin list `id` of `definition`.
fn plugin_registry<'d>(definition: &'d mut Value, id: &str) -> &'d mut Value {
    definition["plugins"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|plugins| plugins["id"] == id)
        .unwrap()
        .get_mut("registry")
        .unwrap()
}

#[test]
fn the_schema_accepts_a_plugin_record_reaching_the_other_checkouts_of_its_repository() {
    let mut definition = claude_code();

    assert_eq!(
        plugin_registry(&mut definition, "installed")["project_reach"],
        "checkouts"
    );
    assert_eq!(errors(&definition), Vec::<String>::new());
}

#[test]
fn the_schema_rejects_an_unknown_project_reach() {
    let mut definition = claude_code();
    plugin_registry(&mut definition, "installed")["project_reach"] = "worktrees".into();

    assert_ne!(errors(&definition), Vec::<String>::new());
}

#[test]
fn the_schema_accepts_the_files_that_bring_a_plugin_record_into_a_project_naming_it() {
    let mut definition = claude_code();

    assert_eq!(
        plugin_registry(&mut definition, "installed")["named_in"][1]["key_path"],
        serde_json::json!(["enabledPlugins"])
    );
    assert_eq!(errors(&definition), Vec::<String>::new());
}

#[test]
fn the_schema_rejects_a_file_naming_plugins_without_its_key_path() {
    let mut definition = claude_code();
    let named_in = &mut plugin_registry(&mut definition, "installed")["named_in"][0];
    named_in.as_object_mut().unwrap().remove("key_path");

    assert_ne!(errors(&definition), Vec::<String>::new());
}

#[test]
fn the_schema_accepts_a_file_of_the_agent_home_that_names_plugins_for_every_project() {
    let mut definition = claude_code();

    assert_eq!(
        plugin_registry(&mut definition, "installed")["named_in"][2]["anchor"],
        serde_json::json!({ "block": "home", "root": "settings_file" })
    );
    assert_eq!(errors(&definition), Vec::<String>::new());
}

#[test]
fn the_schema_accepts_a_switch_enable_sets_over_the_later_layers() {
    let definition = claude_code();
    let set_over: Vec<&Value> = definition["items"]["mcp"]["disable"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|switch| switch["sets_over_later_layers"] == true)
        .collect();

    assert_eq!(set_over.len(), 1);
    assert_eq!(errors(&definition), Vec::<String>::new());
}
