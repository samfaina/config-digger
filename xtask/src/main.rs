//! Repository tasks, run as `cargo xtask <task>`.
//!
//! - `schema`: writes the Agent definition and skill manager definition JSON
//!   Schemas generated from the block enums.
//! - `bindings`: writes the app's TypeScript types and command wrappers generated from the api DTOs.
//! - `definitions`: checks the built-in Agent and skill manager definition
//!   files: each loads, as the schemas' types and their validation read it,
//!   and the folder holds exactly the files that ship. It writes nothing.
//! - `release <tag>`: checks that the tag is `v` and the workspace version,
//!   and prints `version=` and `prerelease=` lines for `$GITHUB_OUTPUT`.
//! - `checksums <dir>`: writes `<dir>/SHA256SUMS` for every other file in it.
//! - `winget <tag> <installer> <dir>`: writes the winget manifest of the
//!   tag's release, with the installer's SHA-256, into `<dir>`.
//!
//! With `--check`, a task writes nothing and fails when a committed file is stale.

mod release;

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

/// A committed file generated from Rust code.
struct Generated {
    task: &'static str,
    /// Relative to the repository root.
    path: &'static str,
    generate: fn() -> Result<String, String>,
}

const SCHEMAS: &[Generated] = &[
    Generated {
        task: "schema",
        path: "schema/agent-definition.schema.json",
        generate: || Ok(config_digger_core::definitions::json_schema()),
    },
    Generated {
        task: "schema",
        path: "schema/skill-manager-definition.schema.json",
        generate: || Ok(config_digger_core::definitions::skill_managers::json_schema()),
    },
];

const BINDINGS: &[Generated] = &[Generated {
    task: "bindings",
    path: "crates/config-digger-app/ui/src/bindings.ts",
    generate: generate_bindings,
}];

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    let (task, check) = match args.as_slice() {
        ["schema"] => (SCHEMAS, false),
        ["schema", "--check"] => (SCHEMAS, true),
        ["bindings"] => (BINDINGS, false),
        ["bindings", "--check"] => (BINDINGS, true),
        ["definitions"] => return check_definitions(),
        ["release", tag] => return print_release(tag),
        ["checksums", dir] => return write_checksums(Path::new(dir)),
        ["winget", tag, installer, dir] => {
            return write_winget(tag, Path::new(installer), Path::new(dir));
        }
        _ => {
            eprintln!(
                "usage: cargo xtask <schema|bindings> [--check] | cargo xtask definitions
                        cargo xtask release <tag> | checksums <dir> | winget <tag> <installer> <dir>"
            );
            return ExitCode::from(2);
        }
    };
    let mut code = ExitCode::SUCCESS;
    for generated in task {
        let contents = match (generated.generate)() {
            Ok(contents) => contents,
            Err(error) => {
                eprintln!("cannot generate {}: {error}", generated.path);
                return ExitCode::FAILURE;
            }
        };
        let done = if check {
            check_current(generated, &contents)
        } else {
            write(generated, &contents)
        };
        if done != ExitCode::SUCCESS {
            code = done;
        }
    }
    code
}

/// The built-in definitions, as CI checks them.
fn check_definitions() -> ExitCode {
    let folder = repo_file("crates/config-digger-core/definitions");
    let problems = config_digger_core::definitions::check_built_in_folder(&folder);
    if problems.is_empty() {
        println!("the built-in definitions are valid");
        return ExitCode::SUCCESS;
    }
    for problem in problems {
        eprintln!("{problem}");
    }
    ExitCode::FAILURE
}

/// The release `tag` names, checked against the workspace version.
fn checked_release(tag: &str) -> Result<release::Release, ExitCode> {
    release::release(tag, env!("CARGO_PKG_VERSION")).map_err(|error| {
        eprintln!("{error}");
        ExitCode::FAILURE
    })
}

fn print_release(tag: &str) -> ExitCode {
    match checked_release(tag) {
        Ok(release) => {
            println!("version={}", release.version);
            println!("prerelease={}", release.prerelease);
            ExitCode::SUCCESS
        }
        Err(code) => code,
    }
}

fn write_checksums(dir: &Path) -> ExitCode {
    let sums = dir.join("SHA256SUMS");
    let files = std::fs::read_dir(dir).and_then(|entries| {
        let mut files = Vec::new();
        for entry in entries {
            let path = entry?.path();
            if path.is_file() && path != sums {
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned();
                files.push((name, std::fs::read(&path)?));
            }
        }
        Ok(files)
    });
    let written = files.and_then(|files| std::fs::write(&sums, release::sha256sums(&files)));
    match written {
        Ok(()) => {
            println!("wrote {}", sums.display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("cannot write {}: {error}", sums.display());
            ExitCode::FAILURE
        }
    }
}

fn write_winget(tag: &str, installer: &Path, dir: &Path) -> ExitCode {
    let release = match checked_release(tag) {
        Ok(release) => release,
        Err(code) => return code,
    };
    let written = std::fs::read(installer).and_then(|bytes| {
        std::fs::create_dir_all(dir)?;
        let sha256 = release::sha256(&bytes);
        for (name, contents) in release::winget_manifest(&release.version, &sha256) {
            std::fs::write(dir.join(&name), contents)?;
            println!("wrote {}", dir.join(&name).display());
        }
        Ok(())
    });
    match written {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("cannot write the winget manifest: {error}");
            ExitCode::FAILURE
        }
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
