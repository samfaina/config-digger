//! A trusted-folders gate whose source is a table of a SQLite store, one
//! row per trusted folder holding its path, through the core `api` with
//! fake ports: an Entry of a Project no row trusts is Blocked, and one of a
//! Project a row trusts, or a folder above it, is not.

use config_digger_core::api::{Context, Core, Inventory, ScanOptions, State};
use config_digger_core::ports::fake::{self, FakeEnvironment, FakeFileSystem};
use std::path::{Path, PathBuf};

const HOME: &str = r"C:\Users\fixture";
const STORE: &str = r"C:\Users\fixture\.table-agent\db.sqlite";

/// An Agent whose Project servers start only in a folder its store's
/// `trusted` table holds a row for.
const TABLE_AGENT: &str = r#"
format_version = 1

[agent]
id = "table-agent"
name = "Table Agent"

[[homes]]
id = "default"
folder = { block = "path", path = "~/.table-agent" }
roots.db = "db.sqlite"

[[scopes]]
id = "project"

[[locations]]
id = "project-mcp"
anchor = { block = "project", path = ".table-agent.json" }
format = { block = "json", dialect = "strict" }
root = ["servers"]
kind = "mcp"
scope = "project"
provenance = "file"
store_class = "plain"
on_read_failure = "unknown"

[items.mcp]
precedence = ["project-mcp"]
competition = { block = "exact-name" }
checks = ["trust", "disable", "precedence"]

[[items.mcp.gates]]
block = "trusted-folders"
locations = ["project-mcp"]
sources = [{ anchor = { block = "home", root = "db" }, key_path = ["path"], sqlite = true, table = "trusted" }]
"#;

fn project(name: &str) -> PathBuf {
    Path::new(r"E:\work").join(name)
}

fn inventory(fs: FakeFileSystem, project: &Path) -> Inventory {
    let ports = fake::ports(fs, FakeEnvironment::with_home(HOME));
    let core = Core::with_extra_definitions(ports, &[("table-agent.toml", TABLE_AGENT)]).unwrap();
    let options = ScanOptions {
        projects: vec![project.to_owned()],
    };
    let snapshot = core.scan(&options).unwrap();
    core.resolve(
        &snapshot,
        &Context {
            project: Some(project.to_owned()),
        },
    )
}

fn state_of(inventory: &Inventory) -> (State, Option<String>) {
    let item = inventory
        .items
        .iter()
        .find(|item| item.agent == "table-agent" && item.name == "notes")
        .expect("the Project's server is read");
    let entry = &item.entries[0];
    (
        entry.state,
        entry.reason.as_ref().map(|reason| {
            serde_json::to_value(reason.code)
                .unwrap()
                .as_str()
                .unwrap()
                .to_owned()
        }),
    )
}

fn with_server(fs: FakeFileSystem, project: &Path) -> FakeFileSystem {
    fs.with_file(
        project.join(".table-agent.json"),
        r#"{"servers": {"notes": {"command": "n"}}}"#,
    )
}

#[test]
fn a_project_no_row_trusts_is_blocked() {
    let fs = FakeFileSystem::new().with_sqlite_rows(STORE, "trusted", "path", &[r"E:\other"]);
    let fs = with_server(fs, &project("app"));

    let (state, reason) = state_of(&inventory(fs, &project("app")));

    assert_eq!(state, State::Blocked);
    assert_eq!(reason.as_deref(), Some("untrusted_folder"));
}

#[test]
fn a_row_for_the_project_or_a_folder_above_it_trusts_it() {
    for row in [r"E:\work\app", r"E:\work"] {
        let fs = FakeFileSystem::new().with_sqlite_rows(STORE, "trusted", "path", &[row]);
        let fs = with_server(fs, &project("app"));

        let (state, _) = state_of(&inventory(fs, &project("app")));

        assert_eq!(state, State::Effective, "{row}");
    }
}

#[test]
fn a_table_is_read_only_from_a_sqlite_store_and_by_one_column() {
    for (source, expected) in [
        (
            r#"{ anchor = { block = "home", root = "db" }, key_path = ["path"], table = "trusted" }"#,
            "table",
        ),
        (
            r#"{ anchor = { block = "home", root = "db" }, key_path = ["path", "x"], sqlite = true, table = "trusted" }"#,
            "one column",
        ),
        (
            r#"{ anchor = { block = "home", root = "db" }, key_path = ["path"], sqlite = true, table = "trusted; drop" }"#,
            "plain names",
        ),
    ] {
        let definition = TABLE_AGENT.replace(
            r#"{ anchor = { block = "home", root = "db" }, key_path = ["path"], sqlite = true, table = "trusted" }"#,
            source,
        );
        let ports = fake::ports(FakeFileSystem::new(), FakeEnvironment::with_home(HOME));

        let error = Core::with_extra_definitions(ports, &[("table-agent.toml", &definition)])
            .err()
            .expect("the definition is invalid");

        assert!(error.message.contains(expected), "{}", error.message);
    }
}
