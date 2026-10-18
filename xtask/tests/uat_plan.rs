//! The UAT plan, `uat/plan.json`, checked against its JSON Schema,
//! `uat/plan.schema.json`, and against the rule a schema cannot say: every
//! step id is unique.

use serde_json::Value;
use std::collections::HashSet;
use std::path::PathBuf;

fn uat_file(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../uat")
        .join(name)
}

fn read_json(name: &str) -> Value {
    let text = std::fs::read_to_string(uat_file(name)).unwrap();
    serde_json::from_str(&text).unwrap()
}

fn plan() -> Value {
    read_json("plan.json")
}

/// What is wrong with `plan`: the schema's errors, each with its place in
/// the plan, then every step id used twice.
fn problems(plan: &Value) -> Vec<String> {
    let schema = read_json("plan.schema.json");
    let validator = jsonschema::validator_for(&schema).unwrap();
    let mut problems: Vec<String> = validator
        .iter_errors(plan)
        .map(|error| format!("{}: {error}", error.instance_path()))
        .collect();
    let mut ids = HashSet::new();
    for section in plan["sections"].as_array().into_iter().flatten() {
        for step in section["steps"].as_array().into_iter().flatten() {
            if let Some(id) = step["id"].as_str()
                && !ids.insert(id)
            {
                problems.push(format!("step {id} is in the plan twice"));
            }
        }
    }
    problems
}

#[test]
fn the_plan_matches_its_schema() {
    assert_eq!(problems(&plan()), Vec::<String>::new());
}

#[test]
fn a_step_without_what_to_expect_fails() {
    let mut plan = plan();
    plan["sections"][0]["steps"][0]
        .as_object_mut()
        .unwrap()
        .remove("expect");
    assert_eq!(
        problems(&plan),
        [r#"/sections/0/steps/0: "expect" is a required property"#]
    );
}

#[test]
fn a_step_id_used_twice_fails() {
    let mut plan = plan();
    let second = plan["sections"][0]["steps"][1].clone();
    plan["sections"][0]["steps"]
        .as_array_mut()
        .unwrap()
        .push(second);
    assert_eq!(problems(&plan), ["step 1.2 is in the plan twice"]);
}
