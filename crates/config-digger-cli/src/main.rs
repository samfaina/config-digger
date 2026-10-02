//! The `config-digger` console binary: parses input, calls the core `api` and
//! renders its output as text or as one JSON document.

mod terminal;

use anstyle::{AnsiColor, Style};
use clap::{ArgAction, CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};
use config_digger_core::api::{self, Context, Core, Inventory, SCHEMA_VERSION};
use config_digger_core::ports::Ports;
use serde::Serialize;
use std::ffi::OsString;
use std::process::ExitCode;
use terminal::{ColorWhen, Line};
use unicode_width::UnicodeWidthStr;

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

    /// When to colour output. `auto` colours a terminal unless NO_COLOR is set.
    #[arg(long, value_enum, value_name = "WHEN", default_value_t, global = true)]
    color: ColorWhen,

    /// Print whole lines instead of fitting them to the terminal width.
    #[arg(long, global = true)]
    wide: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List the Items of every Agent home and the State of each.
    List,
    /// Print a completion script for a shell.
    Completions {
        #[arg(value_enum)]
        shell: Shell,
    },
}

#[derive(Clone, Copy, ValueEnum)]
enum Shell {
    Powershell,
    Bash,
    Zsh,
    Fish,
}

impl From<Shell> for clap_complete::Shell {
    fn from(shell: Shell) -> Self {
        match shell {
            Shell::Powershell => clap_complete::Shell::PowerShell,
            Shell::Bash => clap_complete::Shell::Bash,
            Shell::Zsh => clap_complete::Shell::Zsh,
            Shell::Fish => clap_complete::Shell::Fish,
        }
    }
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
    let args: Vec<OsString> = std::env::args_os().collect();
    let color = ColorWhen::requested(&args);
    color.apply();
    let parsed = Cli::command()
        .color(color.for_clap())
        .try_get_matches_from(&args)
        .and_then(|matches| Cli::from_arg_matches(&matches));
    let cli = match parsed {
        Ok(cli) => cli,
        Err(error) => return usage_error(&error, &args),
    };
    init_logging(cli.verbose);
    report_panics_as_unexpected(cli.json);

    match run(&cli) {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => {
            if cli.json {
                print_json(&ErrorBody { error: &failure });
            } else {
                print_error(&failure.message);
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
                anstream::eprintln!(
                    "{WARNING}warning:{WARNING:#} {} cannot read {} ({})",
                    failure.agent_name,
                    failure.path.display(),
                    failure.kind.code()
                );
            }
            if cli.json {
                print_json(&inventory);
            } else {
                anstream::print!(
                    "{}",
                    render_list(&inventory, terminal::output_width(cli.wide))
                );
            }
            Ok(())
        }
        Command::Completions { shell } => {
            if cli.json {
                return Err(Failure {
                    code: "usage",
                    message: "completions prints a shell script, not a JSON document".to_owned(),
                    exit: Exit::Usage,
                });
            }
            clap_complete::generate(
                clap_complete::Shell::from(shell),
                &mut Cli::command(),
                "config-digger",
                &mut std::io::stdout(),
            );
            Ok(())
        }
    }
}

const ERROR: Style = AnsiColor::Red.on_default().bold();
const WARNING: Style = AnsiColor::Yellow.on_default().bold();

fn print_error(message: &str) {
    anstream::eprintln!("{ERROR}error:{ERROR:#} {message}");
}

/// Help and version go to stdout and succeed; every other parse error is a usage error.
fn usage_error(error: &clap::Error, args: &[OsString]) -> ExitCode {
    use clap::error::ErrorKind;

    if matches!(
        error.kind(),
        ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
    ) {
        let _ = error.print();
        return ExitCode::SUCCESS;
    }
    // Parsing failed, so whether `--json` was asked for is read from the raw arguments.
    if args.iter().skip(1).any(|arg| arg == "--json") {
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

/// A panic is an unexpected error: it exits 1 and, with `--json`, still prints one error document.
fn report_panics_as_unexpected(json: bool) {
    std::panic::set_hook(Box::new(move |info| {
        let failure = Failure {
            code: "unexpected",
            message: format!("internal error: {info}"),
            exit: Exit::Unexpected,
        };
        if json {
            print_json(&ErrorBody { error: &failure });
        } else {
            print_error(&failure.message);
        }
        std::process::exit(failure.exit as i32);
    }));
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
        .with_ansi(terminal::stderr_is_colored())
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

const BOLD: Style = Style::new().bold();
const DIM: Style = Style::new().dimmed();

/// Below this many columns a location is cut with the rest of the line instead of elided.
const MIN_LOCATION_WIDTH: usize = 8;

/// One table per Agent home: each Item with the Scope, State and location of its
/// Effective Entry, fitted to `width` columns when there is a limit.
fn render_list(inventory: &Inventory, width: Option<usize>) -> String {
    if inventory.homes.is_empty() {
        return plain_line("No Agent home found.", width);
    }
    let mut out = String::new();
    for (index, home) in inventory.homes.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        let mut title = Line::default();
        title.push(format!("{} (home: {})", home.agent_name, home.home), BOLD);
        out.push_str(&title.fit(width).render());
        let rows: Vec<Row> = inventory
            .items
            .iter()
            .filter(|item| item.agent == home.agent && item.home == home.home)
            .map(|item| {
                // Entries come in precedence order, so the first one is Effective.
                let effective = &item.entries[0];
                Row {
                    cells: [
                        item.name.as_str(),
                        effective.scope.as_str(),
                        state_label(item.state),
                    ],
                    state: item.state,
                    location: effective,
                }
            })
            .collect();
        if rows.is_empty() {
            out.push_str(&plain_line("  No Items.", width));
            continue;
        }
        let header = ["NAME", "SCOPE", "STATE"];
        let widths: Vec<usize> = (0..3)
            .map(|column| {
                rows.iter()
                    .map(|row| row.cells[column].width())
                    .chain([header[column].len()])
                    .max()
                    .unwrap_or(0)
            })
            .collect();
        let location_width = width.map(|width| {
            let before: usize = widths.iter().map(|w| w + 2).sum::<usize>() + 2;
            width.saturating_sub(before)
        });

        let mut line = Line::default();
        line.push("  ", Style::new());
        for (cell, &w) in header.iter().zip(&widths) {
            line.push_padded(cell, w, DIM).push("  ", Style::new());
        }
        line.push("LOCATION", DIM);
        out.push_str(&line.fit(width).render());

        for row in &rows {
            let mut line = Line::default();
            line.push("  ", Style::new());
            for (column, (cell, &w)) in row.cells.iter().zip(&widths).enumerate() {
                let style = if column == 2 {
                    state_style(row.state)
                } else {
                    Style::new()
                };
                line.push_padded(cell, w, style).push("  ", Style::new());
            }
            line.push(location(row.location, location_width), Style::new());
            out.push_str(&line.fit(width).render());
        }
    }
    out
}

struct Row<'a> {
    cells: [&'a str; 3],
    state: api::State,
    location: &'a api::Entry,
}

fn plain_line(text: &str, width: Option<usize>) -> String {
    let mut line = Line::default();
    line.push(text, Style::new());
    line.fit(width).render()
}

/// An Entry's locator, shortened to `width` by eliding the middle of its path:
/// the key path is what tells Entries of one file apart, so it stays whole.
fn location(entry: &api::Entry, width: Option<usize>) -> String {
    let locator = entry.locator.as_str();
    let Some(width) = width.filter(|&width| locator.width() > width) else {
        return locator.to_owned();
    };
    let path = entry.path.display().to_string();
    let (path, key_path) = match locator.strip_prefix(path.as_str()) {
        Some(key_path) => (path.as_str(), key_path),
        None => (locator, ""),
    };
    match width.checked_sub(key_path.width()) {
        Some(path_width) if path_width >= MIN_LOCATION_WIDTH => {
            format!("{}{key_path}", terminal::elide_middle(path, path_width))
        }
        _ if width >= MIN_LOCATION_WIDTH => terminal::elide_middle(locator, width),
        // Too narrow to elide usefully: the line is cut at the terminal edge instead.
        _ => locator.to_owned(),
    }
}

fn state_label(state: api::State) -> &'static str {
    match state {
        api::State::Effective => "Effective",
        api::State::Shadowed => "Shadowed",
    }
}

/// The app's State colours: green Effective, grey Shadowed.
fn state_style(state: api::State) -> Style {
    match state {
        api::State::Effective => AnsiColor::Green.on_default(),
        api::State::Shadowed => AnsiColor::BrightBlack.on_default(),
    }
}

/// A parse error's text without clap's own `error: ` prefix.
fn clap_message(error: &clap::Error) -> String {
    let text = error.render().to_string();
    let text = text.trim();
    text.strip_prefix("error: ").unwrap_or(text).to_owned()
}
