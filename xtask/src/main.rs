//! Repository tasks, run as `cargo xtask <task>`.
//!
//! - `schema`: writes the Agent definition JSON Schema generated from the block enums.
//! - `schema --check`: fails when the committed schema is stale.

use std::path::PathBuf;
use std::process::ExitCode;

const SCHEMA_PATH: &str = "schema/agent-definition.schema.json";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    match args.as_slice() {
        ["schema"] => write_schema(),
        ["schema", "--check"] => check_schema(),
        _ => {
            eprintln!("usage: cargo xtask schema [--check]");
            ExitCode::from(2)
        }
    }
}

fn schema_file() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(SCHEMA_PATH)
}

fn write_schema() -> ExitCode {
    match std::fs::write(
        schema_file(),
        config_digger_core::definitions::json_schema(),
    ) {
        Ok(()) => {
            println!("wrote {SCHEMA_PATH}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("cannot write {SCHEMA_PATH}: {error}");
            ExitCode::FAILURE
        }
    }
}

fn check_schema() -> ExitCode {
    let committed = std::fs::read_to_string(schema_file()).unwrap_or_default();
    // A checkout with CRLF line endings is not stale.
    if committed.replace("\r\n", "\n") == config_digger_core::definitions::json_schema() {
        println!("{SCHEMA_PATH} is current");
        ExitCode::SUCCESS
    } else {
        eprintln!("{SCHEMA_PATH} is stale: run `cargo xtask schema` and commit the result");
        ExitCode::FAILURE
    }
}
