//! The UAT plan, `uat/plan.json`, checked against its JSON Schema,
//! `uat/plan.schema.json`, and against the rules a schema cannot say:
//!
//! - every step id is unique;
//! - no step asks for a window smaller than the app's smallest, the
//!   `minWidth` and `minHeight` in `crates/config-digger-app/tauri.conf.json`;
//! - a step opens the Agent homes tab before it presses Add Agent home, which
//!   is only there;
//! - each check of a step's `expect` is one sentence, and its exact UI text
//!   is between a « and a » that pair up;
//! - the `test` of each tool in a step's `automation` that exists names a
//!   check it has: a step module of the WebDriver walk, a `Step` block of the
//!   Sandbox walk, or a shot of the screenshots script. The other tools'
//!   tickets add their check when they land.

use serde_json::Value;
use std::collections::HashSet;
use std::path::PathBuf;

/// A file of the repo, at `path` from its root, or None when there is none.
fn read_text(path: &str) -> Option<String> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(path);
    std::fs::read_to_string(path).ok()
}

/// A JSON file of the repo, at `path` from its root.
fn read_json(path: &str) -> Value {
    serde_json::from_str(&read_text(path).unwrap()).unwrap()
}

/// The text of the file a `<file> > <step id>` test names, and the step id,
/// when the file is in `folder`.
fn named_file(test: &str, folder: &str) -> Option<(String, String)> {
    let (file, id) = test.split_once(" > ")?;
    if !file.starts_with(folder) || file.contains("..") {
        return None;
    }
    Some((read_text(file)?, id.to_string()))
}

/// Whether a `webdriver` test names a step module of the WebDriver walk.
fn names_a_step_module(test: &str) -> bool {
    named_file(test, "uat/auto/webdriver/steps/")
        .is_some_and(|(text, id)| text.contains(&format!("id: \"{id}\"")))
}

/// Whether a `sandbox` test names a `Step` block of the Sandbox walk, or the
/// verdict `run.ps1` writes itself, as for 9.1.
fn names_a_step_block(test: &str) -> bool {
    named_file(test, "uat/auto/sandbox/").is_some_and(|(text, id)| {
        text.contains(&format!("Step \"{id}\" {{")) || text.contains(&format!("id = \"{id}\""))
    })
}

/// The shot names of `SHOTS` in the screenshots script.
fn shot_names() -> Vec<String> {
    let script = read_text("crates/config-digger-app/ui/scripts/screenshots.ts").unwrap();
    let start = script.find("const SHOTS: Shot[] = [").unwrap();
    let shots = &script[start..start + script[start..].find("\n];").unwrap()];
    shots
        .split("name: \"")
        .skip(1)
        .map(|rest| rest[..rest.find('"').unwrap()].to_string())
        .collect()
}

fn plan() -> Value {
    read_json("uat/plan.json")
}

/// The place of section 1 in `plan`, which the walk need not start with.
fn section_1(plan: &Value) -> usize {
    plan["sections"]
        .as_array()
        .unwrap()
        .iter()
        .position(|section| section["id"] == "1")
        .unwrap()
}

/// The smallest the app's main window can be, as `(width, height)`.
fn smallest_window() -> (u64, u64) {
    let config = read_json("crates/config-digger-app/tauri.conf.json");
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

/// Whether `check` holds more than one sentence. Exact UI text, between «
/// and », is one thing to see whatever its stops.
fn more_than_one_sentence(check: &str) -> bool {
    let mut quoted = false;
    let mut chars = check.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '«' => quoted = true,
            '»' => quoted = false,
            '.' | ';' | '?' | '!'
                if !quoted && chars.peek().is_some_and(|next| next.is_whitespace()) =>
            {
                return true;
            }
            _ => {}
        }
    }
    false
}

/// Whether each « in `check` is closed by a », with no » before its « and
/// no « inside another: the runner sets apart the text between them.
fn ui_text_pairs_up(check: &str) -> bool {
    let marks: Vec<char> = check.chars().filter(|c| matches!(c, '«' | '»')).collect();
    marks.chunks(2).all(|pair| pair == ['«', '»'])
}

/// What is wrong with `plan`: the schema's errors, each with its place in
/// the plan, then every step id used twice, then every window too small,
/// every Add Agent home pressed outside the Agent homes tab, every check
/// whose « and » do not pair up or that holds more than one sentence, and
/// every tool's test that names no check of that tool.
fn problems(plan: &Value) -> Vec<String> {
    let schema = read_json("uat/plan.schema.json");
    let validator = jsonschema::validator_for(&schema).unwrap();
    let mut problems: Vec<String> = validator
        .iter_errors(plan)
        .map(|error| format!("{}: {error}", error.instance_path()))
        .collect();
    let (min_width, min_height) = smallest_window();
    let shots = shot_names();
    let mut ids = HashSet::new();
    for section in plan["sections"].as_array().into_iter().flatten() {
        for step in section["steps"].as_array().into_iter().flatten() {
            let id = step["id"].as_str().unwrap_or("?");
            if !ids.insert(id) {
                problems.push(format!("step {id} is in the plan twice"));
            }
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
                let line = line.to_lowercase();
                agent_homes_tab |= line.contains("pestaña agent homes");
                if line.contains("pulsa add agent home") && !agent_homes_tab {
                    problems.push(format!(
                        "step {id} presses Add Agent home without opening the Agent homes tab first"
                    ));
                }
            }
            // One thing to see per check, so the tester ticks it at a glance.
            for check in step["expect"].as_array().into_iter().flatten() {
                let check = check.as_str().unwrap_or("");
                if !ui_text_pairs_up(check) {
                    problems.push(format!(
                        "step {id} has a check whose « and » do not pair up: {check:?}"
                    ));
                }
                if more_than_one_sentence(check) {
                    problems.push(format!(
                        "step {id} has a check of more than one sentence: {check:?}"
                    ));
                }
            }
            // A tool's test names a check the tool has, for the tools that
            // exist.
            for automation in step["automation"].as_array().into_iter().flatten() {
                let test = automation["test"].as_str().unwrap_or("");
                let tool = automation["tool"].as_str().unwrap_or("");
                let missing = match tool {
                    "webdriver" if !names_a_step_module(test) => "step module",
                    "sandbox" if !names_a_step_block(test) => "Step block",
                    "visual" if !shots.iter().any(|shot| shot == test) => "shot",
                    _ => continue,
                };
                problems.push(format!(
                    "step {id} has a {tool} test that names no {missing}: {test:?}"
                ));
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
    let at = section_1(&plan);
    plan["sections"][at]["steps"][0]
        .as_object_mut()
        .unwrap()
        .remove("expect");
    assert_eq!(
        problems(&plan),
        [format!(
            r#"/sections/{at}/steps/0: "expect" is a required property"#
        )]
    );
}

#[test]
fn what_to_expect_written_as_a_paragraph_fails() {
    let mut plan = plan();
    let at = section_1(&plan);
    plan["sections"][at]["steps"][0]["expect"] =
        "El script termina sin errores. Lista los homes.".into();
    assert_eq!(
        problems(&plan),
        [format!(
            r#"/sections/{at}/steps/0/expect: "El script termina sin errores. Lista los homes." is not of type "array""#
        )]
    );
}

#[test]
fn a_check_of_two_sentences_fails() {
    let mut plan = plan();
    let at = section_1(&plan);
    plan["sections"][at]["steps"][0]["expect"]
        .as_array_mut()
        .unwrap()
        .push("Sale el aviso «Hid it. Picking it still evaluates it.». El Project se va.".into());
    assert_eq!(
        problems(&plan),
        [
            r#"step 1.1 has a check of more than one sentence: "Sale el aviso «Hid it. Picking it still evaluates it.». El Project se va.""#
        ]
    );
}

#[test]
fn exact_ui_text_left_open_fails() {
    let mut plan = plan();
    let at = section_1(&plan);
    plan["sections"][at]["steps"][0]["expect"]
        .as_array_mut()
        .unwrap()
        .push("Sale el aviso «Copied the path.".into());
    assert_eq!(
        problems(&plan),
        [r#"step 1.1 has a check whose « and » do not pair up: "Sale el aviso «Copied the path.""#]
    );
}

#[test]
fn a_step_id_used_twice_fails() {
    let mut plan = plan();
    let at = section_1(&plan);
    let second = plan["sections"][at]["steps"][1].clone();
    plan["sections"][at]["steps"]
        .as_array_mut()
        .unwrap()
        .push(second);
    assert_eq!(problems(&plan), ["step 1.2 is in the plan twice"]);
}

#[test]
fn a_window_smaller_than_the_app_allows_fails() {
    let mut plan = plan();
    let at = section_1(&plan);
    plan["sections"][at]["steps"][0]["instructions"]
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
    let at = section_1(&plan);
    plan["sections"][at]["steps"][0]["instructions"]
        .as_array_mut()
        .unwrap()
        .push("Ve a Projects & homes y pulsa Add Agent home.".into());
    assert_eq!(
        problems(&plan),
        ["step 1.1 presses Add Agent home without opening the Agent homes tab first"]
    );
}

/// `plan` with `automation` and `manual` set on the first step of section 1.
fn automate(plan: &mut Value, automation: Value, manual: Option<&str>) {
    let at = section_1(plan);
    let step = plan["sections"][at]["steps"][0].as_object_mut().unwrap();
    step.insert("automation".into(), automation);
    if let Some(manual) = manual {
        step.insert("manual".into(), manual.into());
    }
}

#[test]
fn a_step_takes_what_walks_it_and_what_a_person_still_checks() {
    let mut plan = plan();
    automate(
        &mut plan,
        serde_json::json!([
            { "tool": "webdriver", "where": "local", "test": "uat/auto/webdriver/steps/data.mjs > 1.1" },
            { "tool": "sandbox", "where": "local", "test": "uat/auto/sandbox/in-sandbox.ps1 > 9.2" },
            { "tool": "visual", "where": "local", "test": "inventory" },
            { "tool": "fake-ui", "where": "ci", "test": "anything until #265 checks it" },
            { "tool": "local", "where": "local", "test": "anything" },
            { "tool": "cli", "where": "ci", "test": "anything" },
            { "tool": "agent-judge", "where": "local", "test": "anything" }
        ]),
        Some("Que la ventana se vea nítida: ninguna herramienta lo mira."),
    );
    assert_eq!(problems(&plan), Vec::<String>::new());
}

#[test]
fn an_unknown_tool_or_place_fails() {
    let mut plan = plan();
    let at = section_1(&plan);
    automate(
        &mut plan,
        serde_json::json!([
            { "tool": "selenium", "where": "local", "test": "x" },
            { "tool": "webdriver", "where": "cloud", "test": "uat/auto/webdriver/steps/data.mjs > 1.1" }
        ]),
        None,
    );
    let found = problems(&plan);
    assert_eq!(found.len(), 2, "{found:?}");
    assert!(
        found[0].starts_with(&format!("/sections/{at}/steps/0/automation/0/tool: ")),
        "{found:?}"
    );
    assert!(
        found[1].starts_with(&format!("/sections/{at}/steps/0/automation/1/where: ")),
        "{found:?}"
    );
}

#[test]
fn a_test_that_names_no_walked_step_fails() {
    let mut plan = plan();
    automate(
        &mut plan,
        serde_json::json!([
            { "tool": "webdriver", "where": "local", "test": "uat/auto/webdriver/steps/data.mjs > 9.2" },
            { "tool": "webdriver", "where": "ci", "test": "uat/auto/webdriver/steps/nothing.mjs > 1.1" },
            { "tool": "webdriver", "where": "ci", "test": "data.mjs 1.1" },
            { "tool": "sandbox", "where": "local", "test": "uat/auto/sandbox/in-sandbox.ps1 > 1.1" },
            { "tool": "sandbox", "where": "local", "test": "uat/auto/webdriver/steps/data.mjs > 1.1" },
            { "tool": "visual", "where": "local", "test": "no-such-shot" }
        ]),
        None,
    );
    assert_eq!(
        problems(&plan),
        [
            r#"step 1.1 has a webdriver test that names no step module: "uat/auto/webdriver/steps/data.mjs > 9.2""#,
            r#"step 1.1 has a webdriver test that names no step module: "uat/auto/webdriver/steps/nothing.mjs > 1.1""#,
            r#"step 1.1 has a webdriver test that names no step module: "data.mjs 1.1""#,
            r#"step 1.1 has a sandbox test that names no Step block: "uat/auto/sandbox/in-sandbox.ps1 > 1.1""#,
            r#"step 1.1 has a sandbox test that names no Step block: "uat/auto/webdriver/steps/data.mjs > 1.1""#,
            r#"step 1.1 has a visual test that names no shot: "no-such-shot""#,
        ]
    );
}
