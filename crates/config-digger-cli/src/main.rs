//! The `config-digger` console binary: parses input, calls the core `api` and
//! renders its output as text or as one JSON document.

use clap::{ArgAction, Parser, Subcommand};
use config_digger_core::api::{self, Context, Core, Inventory, SCHEMA_VERSION};
use config_digger_core::ports::Ports;
use serde::Serialize;
use std::io::IsTerminal;
use std::process::ExitCode;

#[derive(Parser)]
#[command(
    name = "config-digger",
    bin_name = "config-digger",
    version,
    about = "Inspect the effective configuration of AI coding Agents."
)]
struct Cli {
    /// Print exactly one JSON document to stdout.
    #[arg(long, global = true)]
    json: bool,

    /// Log to stderr: -v for info, -vv for debug, -vvv for trace.
    #[arg(short, long, action = ArgAction::Count, global = true)]
    verbose: u8,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List the Items of every Agent home and the State of each.
    List,
}

/// The public exit codes. Each failure also has a stable `error.code`.
#[derive(Clone, Copy)]
enum Exit {
    Unexpected = 1,
    Usage = 2,
}

/// A failed invocation, reported as `{schema_version, error: {code, message}}`.
#[derive(Serialize)]
struct Failure {
    code: &'static str,
    message: String,
    #[serde(skip)]
    exit: Exit,
}

impl From<api::Error> for Failure {
    fn from(error: api::Error) -> Self {
        match error.code {
            api::ErrorCode::Unexpected => Failure {
                code: "unexpected",
                message: error.message,
                exit: Exit::Unexpected,
            },
        }
    }
}

/// A JSON document on stdout.
#[derive(Serialize)]
struct Document<T> {
    schema_version: u32,
    #[serde(flatten)]
    body: T,
}

#[derive(Serialize)]
struct ErrorBody<'a> {
    error: &'a Failure,
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => return usage_error(&error),
    };
    init_logging(cli.verbose);

    match run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => {
            if cli.json {
                print_json(&ErrorBody { error: &failure });
            } else {
                eprintln!("error: {}", failure.message);
            }
            ExitCode::from(failure.exit as u8)
        }
    }
}

fn run(cli: &Cli) -> Result<(), Failure> {
    match cli.command {
        Command::List => {
            let core = Core::new(Ports::real())?;
            let snapshot = core.scan()?;
            let inventory = core.resolve(&snapshot, &Context::default());
            for failure in &inventory.read_failures {
                eprintln!(
                    "warning: {} cannot read {} ({})",
                    agent_name(&inventory, &failure.agent),
                    failure.path.display(),
                    failure.kind.code()
                );
            }
            if cli.json {
                print_json(&inventory);
            } else {
                print!("{}", render_list(&inventory));
            }
            Ok(())
        }
    }
}

/// Help and version go to stdout and succeed; every other parse error is a usage error.
fn usage_error(error: &clap::Error) -> ExitCode {
    use clap::error::ErrorKind;

    if matches!(
        error.kind(),
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
    ) {
        let _ = error.print();
        return ExitCode::SUCCESS;
    }
    // Parsing failed, so whether `--json` was asked for is read from the raw arguments.
    if std::env::args_os().skip(1).any(|arg| arg == "--json") {
        let failure = Failure {
            code: "usage",
            message: clap_message(error),
            exit: Exit::Usage,
        };
        print_json(&ErrorBody { error: &failure });
    } else {
        let _ = error.print();
    }
    ExitCode::from(Exit::Usage as u8)
}

fn init_logging(verbose: u8) {
    use tracing_subscriber::filter::LevelFilter;

    let level = match verbose {
        0 => return,
        1 => LevelFilter::INFO,
        2 => LevelFilter::DEBUG,
        _ => LevelFilter::TRACE,
    };
    tracing_subscriber::fmt()
        .with_max_level(level)
        .with_writer(std::io::stderr)
        .with_ansi(std::io::stderr().is_terminal())
        .init();
}

fn print_json(body: &impl Serialize) {
    let document = Document {
        schema_version: SCHEMA_VERSION,
        body,
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&document).expect("a document serialises")
    );
}

fn agent_name<'a>(inventory: &'a Inventory, agent: &'a str) -> &'a str {
    inventory
        .homes
        .iter()
        .find(|home| home.agent == agent)
        .map_or(agent, |home| home.agent_name.as_str())
}

/// One table per Agent home: each Item with the Scope, State and location of its winning Entry.
fn render_list(inventory: &Inventory) -> String {
    if inventory.homes.is_empty() {
        return "No Agent home found.\n".to_owned();
    }
    let mut out = String::new();
    for (index, home) in inventory.homes.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        out.push_str(&format!("{} (home: {})\n", home.agent_name, home.home));
        let rows: Vec<[&str; 4]> = inventory
            .items
            .iter()
            .filter(|item| item.agent == home.agent && item.home == home.home)
            .map(|item| {
                let winner = &item.entries[0];
                [
                    item.name.as_str(),
                    winner.scope.as_str(),
                    state_label(item.state),
                    winner.locator.as_str(),
                ]
            })
            .collect();
        if rows.is_empty() {
            out.push_str("  No Items.\n");
            continue;
        }
        let header = ["NAME", "SCOPE", "STATE", "LOCATION"];
        let widths: Vec<usize> = (0..3)
            .map(|column| {
                rows.iter()
                    .map(|row| row[column].chars().count())
                    .chain([header[column].len()])
                    .max()
                    .unwrap_or(0)
            })
            .collect();
        for row in std::iter::once(&header).chain(&rows) {
            out.push_str(&format!(
                "  {:w0$}  {:w1$}  {:w2$}  {}\n",
                row[0],
                row[1],
                row[2],
                row[3],
                w0 = widths[0],
                w1 = widths[1],
                w2 = widths[2],
            ));
        }
    }
    out
}

fn state_label(state: api::State) -> &'static str {
    match state {
        api::State::Effective => "Effective",
        api::State::Shadowed => "Shadowed",
    }
}

/// A parse error's text without clap's own `error: ` prefix.
fn clap_message(error: &clap::Error) -> String {
    let text = error.render().to_string();
    let text = text.trim();
    text.strip_prefix("error: ").unwrap_or(text).to_owned()
}
