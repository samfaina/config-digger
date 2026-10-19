//! The UAT plan, `uat/plan.json`, checked against its JSON Schema,
//! `uat/plan.schema.json`, and against the rules a schema cannot say: every
//! step id is unique, no step asks for a window smaller than the app's
//! smallest, the `minWidth` and `minHeight` of its main window in
//! `crates/config-digger-app/tauri.conf.json`, and no step presses Add Agent
//! home before it opens the Agent homes tab, the only place the button is.

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

/// The smallest the app's main window can be, as `(width, height)`.
fn smallest_window() -> (u64, u64) {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../crates/config-digger-app/tauri.conf.json");
    let config: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let window = &config["app"]["windows"][0];
    (
        window["minWidth"].as_u64().unwrap(),
        window["minHeight"].as_u64().unwrap(),
    )
}

/// Every window size `line` asks for, written as `600 × 400`.
fn window_sizes(line: &str) -> Vec<(u64, u64)> {
    let number = |word: &str| {
        word.trim_matches(|c: char| !c.is_ascii_digit())
            .parse()
            .ok()
    };
    let words: Vec<&str> = line.split_whitespace().collect();
    words
        .windows(3)
        .filter(|three| three[1] == "×")
        .filter_map(|three| Some((number(three[0])?, number(three[2])?)))
        .collect()
}

/// What is wrong with `plan`: the schema's errors, each with its place in
/// the plan, then every step id used twice, then every window too small and
/// every Add Agent home pressed outside the Agent homes tab.
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
    let (min_width, min_height) = smallest_window();
    for section in plan["sections"].as_array().into_iter().flatten() {
        for step in section["steps"].as_array().into_iter().flatten() {
            let id = step["id"].as_str().unwrap_or("?");
            // Add Agent home is a button of the Agent homes tab only.
            let mut agent_homes_tab = false;
            for line in step["instructions"].as_array().into_iter().flatten() {
                let line = line.as_str().unwrap_or("");
                for (width, height) in window_sizes(line) {
                    if width < min_width || height < min_height {
                        problems.push(format!(
                            "step {id} asks for a window of {width} × {height}, smaller than the app's smallest, {min_width} × {min_height}"
                        ));
                    }
                }
                agent_homes_tab |= line.contains("pestaña Agent homes");
                if line.to_lowercase().contains("pulsa add agent home") && !agent_homes_tab {
                    problems.push(format!(
                        "step {id} presses Add Agent home without opening the Agent homes tab first"
                    ));
                }
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

#[test]
fn a_window_smaller_than_the_app_allows_fails() {
    let mut plan = plan();
    plan["sections"][0]["steps"][0]["instructions"]
        .as_array_mut()
        .unwrap()
        .push("Haz la ventana de config-digger de unos 600 × 400.".into());
    assert_eq!(
        problems(&plan),
        ["step 1.1 asks for a window of 600 × 400, smaller than the app's smallest, 1100 × 640"]
    );
}

#[test]
fn add_agent_home_before_the_agent_homes_tab_fails() {
    let mut plan = plan();
    plan["sections"][0]["steps"][0]["instructions"]
        .as_array_mut()
        .unwrap()
        .push("Ve a Projects & homes y pulsa Add Agent home.".into());
    assert_eq!(
        problems(&plan),
        ["step 1.1 presses Add Agent home without opening the Agent homes tab first"]
    );
}
