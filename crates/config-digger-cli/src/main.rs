//! The `config-digger` console binary: parses input, calls the core `api` and
//! renders its output as text or as one JSON document.

mod terminal;

use anstyle::{AnsiColor, Style};
use clap::{ArgAction, CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};
use config_digger_core::api::{self, Context, Core, Inventory, SCHEMA_VERSION, ScanOptions};
use config_digger_core::ports::Ports;
use serde::Serialize;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::ExitCode;
use terminal::{ColorWhen, Line, columns};

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

    /// Print whole lines instead of fitting them to the terminal width (or COLUMNS).
    #[arg(long, global = true)]
    wide: bool,

    /// Resolve for this Project, a folder in any spelling. Defaults to the current directory.
    #[arg(
        long,
        value_name = "PATH",
        global = true,
        conflicts_with = "no_project"
    )]
    project: Option<PathBuf>,

    /// Resolve with no Project: only Entries that apply everywhere take part.
    #[arg(long, global = true)]
    no_project: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List the Items of every Agent home and the State of each.
    List {
        /// Only the Items this selects: an Item name, or the
        /// `<path>#<key path>` locator of one of their Entries.
        selector: Option<String>,
    },
    /// Show one Item with every Entry in precedence order, and why each has its State.
    Show {
        /// An Item name, or the `<path>#<key path>` locator of one of its Entries.
        selector: String,
    },
    /// Print a completion script for a shell.
    Completions {
        /// The shell to complete in.
        #[arg(value_enum)]
        shell: Shell,
    },
}

/// The shells `completions` writes a script for.
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
    Selector = 3,
}

/// A failed invocation, reported as `{schema_version, error: {code, message, …}}`.
#[derive(Serialize)]
struct Failure {
    code: &'static str,
    message: String,
    /// What an ambiguous selector could mean.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    candidates: Vec<String>,
    #[serde(skip)]
    exit: Exit,
}

impl Failure {
    fn usage(message: impl Into<String>) -> Self {
        Failure {
            code: "usage",
            message: message.into(),
            candidates: Vec::new(),
            exit: Exit::Usage,
        }
    }
}

impl From<api::Error> for Failure {
    fn from(error: api::Error) -> Self {
        let (code, exit) = match error.code {
            api::ErrorCode::Unexpected => ("unexpected", Exit::Unexpected),
            api::ErrorCode::NoMatch => ("no_match", Exit::Selector),
            api::ErrorCode::Ambiguous => ("ambiguous", Exit::Selector),
        };
        Failure {
            code,
            message: error.message,
            candidates: error.candidates,
            exit,
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
                for candidate in &failure.candidates {
                    anstream::eprintln!("  {candidate}");
                }
            }
            ExitCode::from(failure.exit as u8)
        }
    }
}

/// `show --json`: the Item and the context it was resolved for.
#[derive(Serialize)]
struct ShowBody<'a> {
    project: &'a Option<PathBuf>,
    item: &'a api::Item,
    read_failures: &'a [api::ReadFailure],
}

fn run(cli: &Cli) -> Result<(), Failure> {
    match &cli.command {
        Command::List { selector } => {
            let mut inventory = inventory(cli)?;
            if let Some(selector) = selector {
                let selected: Vec<api::Item> =
                    inventory.select(selector).into_iter().cloned().collect();
                if selected.is_empty() {
                    return Err(api::Error::no_match(selector).into());
                }
                inventory.items = selected;
            }
            if cli.json {
                print_json(&inventory);
            } else {
                anstream::print!(
                    "{}",
                    render_list(
                        &inventory,
                        selector.is_some(),
                        terminal::output_width(cli.wide)
                    )
                );
            }
            Ok(())
        }
        Command::Show { selector } => {
            let inventory = inventory(cli)?;
            let item = inventory.select_one(selector)?;
            if cli.json {
                print_json(&ShowBody {
                    project: &inventory.project,
                    item,
                    read_failures: &inventory.read_failures,
                });
            } else {
                anstream::print!(
                    "{}",
                    render_show(&inventory, item, terminal::output_width(cli.wide))
                );
            }
            Ok(())
        }
        Command::Completions { shell } => {
            if cli.json {
                return Err(Failure::usage(
                    "completions prints a shell script, not a JSON document",
                ));
            }
            clap_complete::generate(
                clap_complete::Shell::from(*shell),
                &mut Cli::command(),
                "config-digger",
                &mut std::io::stdout(),
            );
            Ok(())
        }
    }
}

/// Scans and resolves for the Project the flags ask for, printing Read
/// failures to stderr as warnings.
fn inventory(cli: &Cli) -> Result<Inventory, Failure> {
    let project = if cli.no_project {
        None
    } else {
        let project = match &cli.project {
            Some(project) => std::path::absolute(project),
            None => std::env::current_dir(),
        };
        Some(project.map_err(api::Error::unexpected)?)
    };
    let core = Core::new(Ports::real())?;
    let snapshot = core.scan(&ScanOptions {
        projects: project.iter().cloned().collect(),
    })?;
    let inventory = core.resolve(&snapshot, &Context { project });
    for failure in &inventory.read_failures {
        anstream::eprintln!(
            "{WARNING}warning:{WARNING:#} {} cannot read {} ({})",
            failure.agent_name,
            failure.path.display(),
            failure.kind.code()
        );
    }
    Ok(inventory)
}

/// The `error:` prefix on stderr.
const ERROR: Style = AnsiColor::Red.on_default().bold();
/// The `warning:` prefix on stderr.
const WARNING: Style = AnsiColor::Yellow.on_default().bold();

/// A human-readable error on stderr.
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
        let failure = Failure::usage(clap_message(error));
        print_json(&ErrorBody { error: &failure });
    } else {
        let _ = error.print();
    }
    ExitCode::from(Exit::Usage as u8)
}

/// A panic is an unexpected error: it exits 1 and, with `--json`, still prints one error document.
fn report_panics_as_unexpected(json: bool) {
    std::panic::set_hook(Box::new(move |info| {
        let failure = Failure::from(api::Error::unexpected(format!("internal error: {info}")));
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

/// Home titles.
const BOLD: Style = Style::new().bold();
/// Column headers.
const DIM: Style = Style::new().dimmed();

/// Below this many columns a location is cut with the rest of the line instead of elided.
const MIN_LOCATION_WIDTH: usize = 8;

/// Before the first column of a table.
const INDENT: &str = "  ";
/// Between table columns.
const GAP: &str = "  ";

/// One table per Agent home: each Item with the Scope, State and location of its
/// Effective Entry, fitted to `width` columns when there is a limit. When the
/// Items were `selected`, homes with none of them are left out.
fn render_list(inventory: &Inventory, selected: bool, width: Option<usize>) -> String {
    let mut out = project_line(inventory, width);
    out.push('\n');
    if inventory.homes.is_empty() {
        out.push_str(&plain_line("No Agent home found.", width));
        return out;
    }
    let mut first = true;
    for home in &inventory.homes {
        let rows: Vec<Row> = inventory
            .items
            .iter()
            .filter(|item| item.agent == home.agent && item.home == home.home)
            .map(|item| {
                // Entries come in precedence order, so the first one is Effective.
                let effective = &item.entries[0];
                Row {
                    cells: [
                        (item.name.as_str(), Style::new()),
                        (effective.scope.as_str(), Style::new()),
                        (state_label(item.state), state_style(item.state)),
                    ],
                    effective,
                }
            })
            .collect();
        if rows.is_empty() && selected {
            continue;
        }
        if !first {
            out.push('\n');
        }
        first = false;
        let mut title = Line::default();
        title.push(format!("{} (home: {})", home.agent_name, home.home), BOLD);
        out.push_str(&title.fit(width).render());
        if rows.is_empty() {
            out.push_str(&plain_line("  No Items.", width));
            continue;
        }
        let header = [("NAME", DIM), ("SCOPE", DIM), ("STATE", DIM)];
        let cells: Vec<[(&str, Style); 3]> = rows.iter().map(|row| row.cells).collect();
        let widths = column_widths(&header, &cells);
        let location_width = last_column_width(&widths, width);

        let header_line = table_line(&header, &widths, "LOCATION".to_owned(), DIM);
        out.push_str(&header_line.fit(width).render());
        for row in &rows {
            let location = location(row.effective, location_width);
            let line = table_line(&row.cells, &widths, location, Style::new());
            out.push_str(&line.fit(width).render());
        }
    }
    out
}

/// An Item in `list`: its name, Scope and State cells, and its Effective Entry.
struct Row<'a> {
    cells: [(&'a str, Style); 3],
    effective: &'a api::Entry,
}

/// The padded cells of one table line, followed by its unpadded last column.
fn table_line<const N: usize>(
    cells: &[(&str, Style); N],
    widths: &[usize; N],
    last: String,
    style: Style,
) -> Line {
    let mut line = Line::default();
    line.push(INDENT, Style::new());
    for (&(cell, cell_style), &width) in cells.iter().zip(widths) {
        line.push_padded(cell, width, cell_style)
            .push(GAP, Style::new());
    }
    line.push(last, style);
    line
}

/// The width of each padded column, wide enough for the header and every row.
fn column_widths<const N: usize>(
    header: &[(&str, Style); N],
    rows: &[[(&str, Style); N]],
) -> [usize; N] {
    std::array::from_fn(|column| {
        rows.iter()
            .map(|row| columns(row[column].0))
            .chain([columns(header[column].0)])
            .max()
            .unwrap_or(0)
    })
}

/// How many columns are left for the last column after the padded ones.
fn last_column_width<const N: usize>(widths: &[usize; N], width: Option<usize>) -> Option<usize> {
    width.map(|width| {
        let before: usize = widths.iter().map(|w| w + GAP.len()).sum::<usize>() + INDENT.len();
        width.saturating_sub(before)
    })
}

/// The first line of `list` and `show`: the Project the States are resolved for.
fn project_line(inventory: &Inventory, width: Option<usize>) -> String {
    let Some(project) = &inventory.project else {
        return plain_line("No Project", width);
    };
    let missing = inventory
        .projects
        .iter()
        .any(|known| known.path == *project && known.missing);
    let suffix = if missing { " (missing)" } else { "" };
    plain_line(&format!("Project: {}{suffix}", project.display()), width)
}

/// One Item with every Entry in precedence order: its rank, Scope, State,
/// Provenance and locator, then the reason for its State.
fn render_show(inventory: &Inventory, item: &api::Item, width: Option<usize>) -> String {
    let mut out = project_line(inventory, width);
    out.push('\n');
    let agent_name = inventory
        .homes
        .iter()
        .find(|home| home.agent == item.agent)
        .map_or(item.agent.as_str(), |home| home.agent_name.as_str());
    let mut title = Line::default();
    title.push(
        format!("{} ({agent_name}, home: {})", item.name, item.home),
        BOLD,
    );
    out.push_str(&title.fit(width).render());

    let ranks: Vec<String> = (1..=item.entries.len())
        .map(|rank| format!("{rank}."))
        .collect();
    let rows: Vec<[(&str, Style); 4]> = item
        .entries
        .iter()
        .zip(&ranks)
        .map(|(entry, rank)| {
            [
                (rank.as_str(), Style::new()),
                (entry.scope.as_str(), Style::new()),
                (state_label(entry.state), state_style(entry.state)),
                (provenance_label(entry.provenance), Style::new()),
            ]
        })
        .collect();
    let header = [
        ("#", DIM),
        ("SCOPE", DIM),
        ("STATE", DIM),
        ("PROVENANCE", DIM),
    ];
    let widths = column_widths(&header, &rows);
    let location_width = last_column_width(&widths, width);
    let header_line = table_line(&header, &widths, "LOCATION".to_owned(), DIM);
    out.push_str(&header_line.fit(width).render());

    // A reason sits under the Scope column, wrapped to the room left there.
    let reason_indent = " ".repeat(INDENT.len() + widths[0] + GAP.len());
    let reason_width = width.map(|width| width.saturating_sub(reason_indent.len()));
    for (entry, cells) in item.entries.iter().zip(&rows) {
        let location = location(entry, location_width);
        let line = table_line(cells, &widths, location, Style::new());
        out.push_str(&line.fit(width).render());
        if let Some(reason) = &entry.reason {
            for text in terminal::wrap(&reason.sentence, reason_width) {
                let mut line = Line::default();
                line.push(reason_indent.as_str(), Style::new())
                    .push(text, Style::new());
                out.push_str(&line.fit(width).render());
            }
        }
    }
    out
}

/// A line of unstyled text, fitted to `width`.
fn plain_line(text: &str, width: Option<usize>) -> String {
    let mut line = Line::default();
    line.push(text, Style::new());
    line.fit(width).render()
}

/// An Entry's locator, shortened to `width` by eliding the middle of its path:
/// the key path is what tells Entries of one file apart, so it stays whole.
fn location(entry: &api::Entry, width: Option<usize>) -> String {
    let locator = entry.locator.as_str();
    let Some(width) = width.filter(|&width| columns(locator) > width) else {
        return locator.to_owned();
    };
    let path = entry.path.display().to_string();
    let (path, key_path) = match locator.strip_prefix(path.as_str()) {
        Some(key_path) => (path.as_str(), key_path),
        None => (locator, ""),
    };
    match width.checked_sub(columns(key_path)) {
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

fn provenance_label(provenance: api::Provenance) -> &'static str {
    match provenance {
        api::Provenance::File => "file",
    }
}

/// A parse error's text without clap's own `error: ` prefix.
fn clap_message(error: &clap::Error) -> String {
    let text = error.render().to_string();
    let text = text.trim();
    text.strip_prefix("error: ").unwrap_or(text).to_owned()
}
