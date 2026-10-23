//! Repository tasks, run as `cargo xtask <task>`.
//!
//! - `schema`: writes the Agent definition and skill manager definition JSON
//!   Schemas generated from the block enums.
//! - `bindings`: writes the app's TypeScript types and command wrappers generated from the api DTOs.
//! - `definitions`: checks the built-in Agent and skill manager definition
//!   files: each loads, as the schemas' types and their validation read it,
//!   and the folder holds exactly the files that ship. It writes nothing.
//! - `unverified`: lists every `unverified` mark of the built-in definitions,
//!   one line each with its file, line, table and sentence: what the
//!   release gate still has to test. It writes nothing.
//! - `installer`: checks that the installer's install folder in `hooks.nsh`
//!   is a folder of its own: no data folder, nor a folder inside or above
//!   one. It writes nothing.
//! - `fixtures`: checks that git ignores no file under a
//!   `crates/*/tests/fixtures` folder, the contributor's global excludes
//!   included. It writes nothing.
//! - `fake-backend`: records the fixtures of the UI's fake backend, which
//!   `npm run dev:fake` serves the UI on, into
//!   `crates/config-digger-app/ui/src/fake/fixtures.ts`: the UAT's throwaway
//!   data is created in a temporary folder and read through the app's
//!   commands, with an environment that names nothing of this machine.
//! - `release <tag>`: checks that the tag is `v` and the workspace version,
//!   and prints `version=` and `prerelease=` lines for `$GITHUB_OUTPUT`.
//! - `checksums <dir>`: writes `<dir>/SHA256SUMS` for every other file in it.
//! - `winget <tag> <installer> <dir>`: writes the winget manifest of the
//!   tag's release, with the installer's SHA-256, into `<dir>`.
//! - `updater <tag> <dir>`: writes the updater manifest `<dir>/latest.json`
//!   from the signature of the tag's installer, `<dir>/<installer>.sig`, and
//!   fails when the signature is missing or empty.
//! - `assets <tag> <dir>`: fails unless `<dir>` holds every asset the tag's
//!   release publishes, the updater's signature and manifest included.
//! - `vcruntime <exe>`: fails when the executable imports a DLL of the VC++
//!   runtime, which a Windows without it lacks: the CLI must run there, for
//!   the installer's hooks run it.
//!
//! With `--check`, a task writes nothing and fails when a committed file is stale.

mod fixtures;
mod installer;
mod release;
mod vcruntime;

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

/// The built-in definitions' folder, relative to the repository root.
const DEFINITIONS: &str = "crates/config-digger-core/definitions";

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
        ["unverified"] => return list_unverified(),
        ["installer"] => return check_installer(),
        ["fixtures"] => return check_fixtures(),
        ["fake-backend"] => return record_fake_backend(),
        ["release", tag] => return print_release(tag),
        ["checksums", dir] => return write_checksums(Path::new(dir)),
        ["winget", tag, installer, dir] => {
            return write_winget(tag, Path::new(installer), Path::new(dir));
        }
        ["updater", tag, dir] => return write_updater_manifest(tag, Path::new(dir)),
        ["assets", tag, dir] => return check_assets(tag, Path::new(dir)),
        ["vcruntime", exe] => return check_vcruntime(Path::new(exe)),
        _ => {
            eprintln!(
                "usage: cargo xtask <schema|bindings> [--check] | cargo xtask <definitions|unverified|installer|fixtures|fake-backend>\n       \
                 cargo xtask release <tag> | checksums <dir> | winget <tag> <installer> <dir>
                        cargo xtask updater <tag> <dir> | assets <tag> <dir> | vcruntime <exe>"
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
    let folder = repo_file(DEFINITIONS);
    let problems = config_digger_core::definitions::check_built_in_folder(&folder);
    report(&problems, "the built-in definitions are valid")
}

/// The `unverified` marks of the built-in definitions, each as
/// `<file>:<line>: <table>: <sentence>`, the file relative to the
/// repository root.
fn list_unverified() -> ExitCode {
    let marks = match config_digger_core::definitions::unverified_marks() {
        Ok(marks) => marks,
        Err(error) => {
            eprintln!("{error}");
            return ExitCode::FAILURE;
        }
    };
    for mark in &marks {
        println!("{DEFINITIONS}/{mark}");
    }
    println!("{} unverified marks", marks.len());
    ExitCode::SUCCESS
}

/// The installer's install folder, as CI checks it.
fn check_installer() -> ExitCode {
    let read = |path: &str| {
        std::fs::read_to_string(repo_file(path)).map_err(|error| {
            eprintln!("cannot read {path}: {error}");
            ExitCode::FAILURE
        })
    };
    let files = read("crates/config-digger-app/installer/hooks.nsh")
        .and_then(|hooks| Ok((hooks, read("crates/config-digger-app/tauri.conf.json")?)));
    match files {
        Ok((hooks, tauri_conf)) => report(
            &installer::problems(&hooks, &tauri_conf),
            "the installer installs in a folder of its own",
        ),
        Err(code) => code,
    }
}

/// The fixture files, as CI checks them.
fn check_fixtures() -> ExitCode {
    match fixtures::ignored(&repo_file("")) {
        Ok(ignored) => report(
            &ignored
                .iter()
                .map(|file| format!("git ignores the fixture file {file}"))
                .collect::<Vec<_>>(),
            "git ignores no fixture file",
        ),
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

/// The fake backend's fixtures, generated from the app's commands.
const FAKE_BACKEND: &str = "crates/config-digger-app/ui/src/fake/fixtures.ts";

/// Records the fake backend's fixtures over the UAT's throwaway data, made
/// by its script in a temporary `cd-throwaway` folder, deleted afterwards.
fn record_fake_backend() -> ExitCode {
    let temp =
        std::env::temp_dir().join(format!("config-digger-fake-backend-{}", std::process::id()));
    let root = temp.join("cd-throwaway");
    let recorded = std::fs::create_dir_all(&temp)
        .map_err(|error| format!("cannot create {}: {error}", temp.display()))
        .and_then(|()| {
            run(
                Command::new("powershell")
                    .args([
                        "-NoProfile",
                        "-NonInteractive",
                        "-ExecutionPolicy",
                        "Bypass",
                        "-File",
                    ])
                    .arg(repo_file("uat/data/cd-throwaway-setup.ps1"))
                    .arg("-Root")
                    .arg(&root),
                "the throwaway data script",
            )
        })
        .and_then(|()| {
            let cargo = std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
            run(
                Command::new(cargo)
                    .args([
                        "run",
                        "--quiet",
                        "--package",
                        "config-digger-app",
                        "--example",
                        "record_fake_backend",
                        "--",
                    ])
                    .arg(&root)
                    .arg(repo_file(FAKE_BACKEND)),
                "the record_fake_backend example",
            )
        });
    let _ = std::fs::remove_dir_all(&temp);
    match recorded {
        Ok(()) => {
            println!("wrote {FAKE_BACKEND}");
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

/// Runs `command`, named `what` in its error, and fails when it does.
fn run(command: &mut Command, what: &str) -> Result<(), String> {
    let status = command
        .status()
        .map_err(|error| format!("cannot run {what}: {error}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{what} failed ({status})"))
    }
}

/// Prints `problems` and fails, or prints `valid` when there are none.
fn report(problems: &[impl std::fmt::Display], valid: &str) -> ExitCode {
    if problems.is_empty() {
        println!("{valid}");
        return ExitCode::SUCCESS;
    }
    for problem in problems {
        eprintln!("{problem}");
    }
    ExitCode::FAILURE
}

/// The release `tag` names, checked against the workspace version.
fn checked_release(tag: &str) -> Result<release::Release, ExitCode> {
    release::from_tag(tag, env!("CARGO_PKG_VERSION")).map_err(|error| {
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

fn write_updater_manifest(tag: &str, dir: &Path) -> ExitCode {
    let release = match checked_release(tag) {
        Ok(release) => release,
        Err(code) => return code,
    };
    let signature = dir.join(release::signature_name(&release.version));
    let manifest = std::fs::read_to_string(&signature)
        .map_err(|error| format!("cannot read {}: {error}", signature.display()))
        .and_then(|signature| release::updater_manifest(&release.version, &signature));
    let file = dir.join(release::UPDATER_MANIFEST);
    let written = manifest.and_then(|manifest| {
        std::fs::write(&file, manifest)
            .map_err(|error| format!("cannot write {}: {error}", file.display()))
    });
    match written {
        Ok(()) => {
            println!("wrote {}", file.display());
            ExitCode::SUCCESS
        }
        Err(error) => {
            eprintln!("{error}");
            ExitCode::FAILURE
        }
    }
}

fn check_assets(tag: &str, dir: &Path) -> ExitCode {
    let release = match checked_release(tag) {
        Ok(release) => release,
        Err(code) => return code,
    };
    let names: Vec<String> = match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .flatten()
            .filter(|entry| entry.path().is_file())
            .map(|entry| entry.file_name().to_string_lossy().into_owned())
            .collect(),
        Err(error) => {
            eprintln!("cannot read {}: {error}", dir.display());
            return ExitCode::FAILURE;
        }
    };
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let missing = release::missing_assets(&release.version, &names);
    if missing.is_empty() {
        println!("{} holds every asset of {tag}", dir.display());
        return ExitCode::SUCCESS;
    }
    for asset in missing {
        eprintln!("the release of {tag} lacks {asset} in {}", dir.display());
    }
    ExitCode::FAILURE
}

/// The executable at `exe`, as CI and the release check the CLI.
fn check_vcruntime(exe: &Path) -> ExitCode {
    match std::fs::read(exe) {
        Ok(bytes) => report(
            &vcruntime::problems(&bytes)
                .iter()
                .map(|problem| format!("{}: {problem}", exe.display()))
                .collect::<Vec<_>>(),
            &format!("{} imports no VC++ runtime DLL", exe.display()),
        ),
        Err(error) => {
            eprintln!("cannot read {}: {error}", exe.display());
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
