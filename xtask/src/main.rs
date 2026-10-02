//! Repository tasks, run as `cargo xtask <task>`.
//!
//! - `schema`: writes the Agent definition JSON Schema generated from the block enums.
//! - `bindings`: writes the app's TypeScript types and command wrappers generated from the api DTOs.
//!
//! With `--check`, a task writes nothing and fails when the committed file is stale.

use std::path::PathBuf;
use std::process::{Command, ExitCode};

/// A committed file generated from Rust code.
struct Generated {
    task: &'static str,
    /// Relative to the repository root.
    path: &'static str,
    generate: fn() -> Result<String, String>,
}

const SCHEMA: Generated = Generated {
    task: "schema",
    path: "schema/agent-definition.schema.json",
    generate: || Ok(config_digger_core::definitions::json_schema()),
};

const BINDINGS: Generated = Generated {
    task: "bindings",
    path: "crates/config-digger-app/ui/src/bindings.ts",
    generate: generate_bindings,
};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let (generated, check) = match args.as_slice() {
        ["schema"] => (SCHEMA, false),
        ["schema", "--check"] => (SCHEMA, true),
        ["bindings"] => (BINDINGS, false),
        ["bindings", "--check"] => (BINDINGS, true),
        _ => {
            eprintln!("usage: cargo xtask <schema|bindings> [--check]");
            return ExitCode::from(2);
        }
    };
    let contents = match (generated.generate)() {
        Ok(contents) => contents,
        Err(error) => {
            eprintln!("cannot generate {}: {error}", generated.path);
            return ExitCode::FAILURE;
        }
    };
    if check {
        check_current(&generated, &contents)
    } else {
        write(&generated, &contents)
    }
}

fn repo_file(path: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join(path)
}

fn write(generated: &Generated, contents: &str) -> ExitCode {
    match std::fs::write(repo_file(generated.path), contents) {
        Ok(()) => {
            println!("wrote {}", generated.path);
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("cannot write {}: {error}", generated.path);
            ExitCode::FAILURE
        }
    }
}

fn check_current(generated: &Generated, contents: &str) -> ExitCode {
    let committed = std::fs::read_to_string(repo_file(generated.path)).unwrap_or_default();
    // A checkout with CRLF line endings is not stale.
    if committed.replace("\r\n", "\n") == contents.replace("\r\n", "\n") {
        println!("{} is current", generated.path);
        ExitCode::SUCCESS
    } else {
        eprintln!(
            "{} is stale: run `cargo xtask {}` and commit the result",
            generated.path, generated.task
        );
        ExitCode::FAILURE
    }
}

/// Runs the app's `export_bindings` example. Linking the app into `xtask`
/// would fail: Tauri's static Windows runtime only applies to the app's own
/// targets. tauri-specta only exports to a file, so the bindings go through a
/// temporary one.
fn generate_bindings() -> Result<String, String> {
    let file =
        std::env::temp_dir().join(format!("config-digger-bindings-{}.ts", std::process::id()));
    let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = Command::new(cargo)
        .args([
            "run",
            "--quiet",
            "--package",
            "config-digger-app",
            "--example",
            "export_bindings",
            "--",
        ])
        .arg(&file)
        .status()
        .map_err(|error| format!("cannot run cargo: {error}"))?;
    if !status.success() {
        return Err(format!("the export_bindings example failed ({status})"));
    }
    let contents = std::fs::read_to_string(&file).map_err(|error| error.to_string());
    let _ = std::fs::remove_file(&file);
    contents
}
