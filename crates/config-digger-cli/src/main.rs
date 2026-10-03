//! The `config-digger` console binary: parses input, calls the core `api` and
//! renders its output as text or as one JSON document.

mod terminal;

use anstyle::{AnsiColor, Style};
use clap::{ArgAction, CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};
use config_digger_core::api::{
    self, Action, Context, Core, Inventory, Plan, SCHEMA_VERSION, ScanOptions, Snapshot, Targets,
    WarningClass,
};
use config_digger_core::ports::Ports;
use serde::Serialize;
use std::ffi::OsString;
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
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
    /// Remove Entries for good, from where they live, for every Agent that reads them.
    ///
    /// Prints the preview, asks for confirmation, then backs up each touched
    /// file, writes, verifies and records the Operation in History.
    Remove {
        /// Each one an Item name with a single Entry, or the
        /// `<path>#<key path>` locator of an Entry. Together they form one
        /// Operation, applied entirely or not at all.
        #[arg(required = true)]
        selectors: Vec<String>,
        /// Target every Entry of the one Item each selector names.
        #[arg(long)]
        all: bool,
        /// Confirm the Operation without asking.
        #[arg(long)]
        yes: bool,
        /// Acknowledge one class of warning without asking. Repeat for several.
        #[arg(long, value_enum, value_name = "CLASS")]
        ack: Vec<Ack>,
        /// Print the plan and write nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// List past Operations, newest first, and whether each can be undone.
    History {
        #[command(subcommand)]
        command: Option<HistoryCommand>,
    },
    /// Undo an Operation in History, any one of them, not only the last.
    ///
    /// Applies the inverse of its edits to the files as they are now, through
    /// the same preview, confirmation, backup, write and Verify as any
    /// Operation. It is refused when what the Operation touched changed since.
    Undo {
        /// The Operation's id, as `history` lists it.
        id: String,
        /// Confirm the Operation without asking.
        #[arg(long)]
        yes: bool,
        /// Acknowledge one class of warning without asking. Repeat for several.
        #[arg(long, value_enum, value_name = "CLASS")]
        ack: Vec<Ack>,
        /// Print the plan and write nothing.
        #[arg(long)]
        dry_run: bool,
    },
    /// Print a completion script for a shell.
    Completions {
        /// The shell to complete in.
        #[arg(value_enum)]
        shell: Shell,
    },
}

#[derive(Subcommand)]
enum HistoryCommand {
    /// Show one Operation: what it changed, its edits, and the files copied to its backup.
    Show {
        /// The Operation's id, as `history` lists it.
        id: String,
    },
    /// Dismiss an Operation that needs attention, once its files are
    /// recovered by hand. History then prunes it like any other.
    Dismiss {
        /// The Operation's id, as `history` lists it.
        id: String,
    },
    /// Delete an Operation's backup for good, such as one holding Secrets.
    ///
    /// The Operation stays in History but can no longer be undone. Prints the
    /// preview and asks for confirmation. When the Operation needs attention,
    /// its backup is the only copy for manual recovery: that needs its own
    /// acknowledgement, and deleting it dismisses the Operation.
    DeleteBackup {
        /// The Operation's id, as `history` lists it.
        id: String,
        /// Confirm the deletion without asking.
        #[arg(long)]
        yes: bool,
        /// Acknowledge one class of warning without asking. Repeat for several.
        #[arg(long, value_enum, value_name = "CLASS")]
        ack: Vec<Ack>,
        /// Print the plan and delete nothing.
        #[arg(long)]
        dry_run: bool,
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

/// The classes of warning `--ack` acknowledges.
#[derive(Clone, Copy, ValueEnum)]
enum Ack {
    GitVersioned,
    WideReach,
    RunningAgent,
    NeedsAttention,
}

impl From<Ack> for WarningClass {
    fn from(ack: Ack) -> Self {
        match ack {
            Ack::GitVersioned => WarningClass::GitVersioned,
            Ack::WideReach => WarningClass::WideReach,
            Ack::RunningAgent => WarningClass::RunningAgent,
            Ack::NeedsAttention => WarningClass::NeedsAttention,
        }
    }
}

/// The public exit codes. Each failure also has a stable `error.code`.
#[derive(Clone, Copy)]
enum Exit {
    Unexpected = 1,
    Usage = 2,
    Selector = 3,
    NotConfirmed = 4,
    Blocked = 5,
    Locked = 6,
    Stale = 7,
    UndoRefused = 8,
    Failed = 9,
    RollbackIncomplete = 10,
}

/// A failed invocation, reported as `{schema_version, error: {code, message, …}}`.
#[derive(Serialize)]
struct Failure {
    code: &'static str,
    message: String,
    /// What an ambiguous selector could mean.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    candidates: Vec<String>,
    /// The backup folder of a failed Operation, or of one whose Undo was
    /// refused. Boxed, like `diff`, to keep every `Result<_, Failure>` small.
    #[serde(skip_serializing_if = "Option::is_none")]
    backup: Option<Box<Path>>,
    /// For a refused Undo or an incomplete Rollback, a diff from each
    /// backed-up file to what it holds now, with every Secret masked. A
    /// boxed slice keeps every `Result<_, Failure>` small.
    #[serde(skip_serializing_if = "<[_]>::is_empty")]
    diffs: Box<[api::FileDiff]>,
    /// The Operation that failed, its id, the failure that started an
    /// incomplete Rollback, and where the Rollback left each file.
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    failed: Option<Box<api::FailedOperation>>,
    #[serde(skip)]
    exit: Exit,
}

impl Failure {
    fn new(code: &'static str, message: impl Into<String>, exit: Exit) -> Self {
        Failure {
            code,
            message: message.into(),
            candidates: Vec::new(),
            backup: None,
            diffs: Box::default(),
            failed: None,
            exit,
        }
    }

    fn usage(message: impl Into<String>) -> Self {
        Failure::new("usage", message, Exit::Usage)
    }
}

impl From<api::Error> for Failure {
    fn from(error: api::Error) -> Self {
        let exit = match error.code {
            api::ErrorCode::Unexpected => Exit::Unexpected,
            api::ErrorCode::NoMatch | api::ErrorCode::Ambiguous => Exit::Selector,
            api::ErrorCode::AckRequired => Exit::NotConfirmed,
            api::ErrorCode::Blocked => Exit::Blocked,
            api::ErrorCode::Locked => Exit::Locked,
            api::ErrorCode::Stale => Exit::Stale,
            api::ErrorCode::BackupFailed
            | api::ErrorCode::WriteFailed
            | api::ErrorCode::VerifyFailed
            | api::ErrorCode::OverwrittenByAgent
            | api::ErrorCode::ReadFailureIntroduced => Exit::Failed,
            api::ErrorCode::RollbackIncomplete => Exit::RollbackIncomplete,
            api::ErrorCode::UndoRefused => Exit::UndoRefused,
        };
        Failure {
            candidates: error.candidates,
            backup: error.backup.map(PathBuf::into_boxed_path),
            diffs: error.diffs.into_boxed_slice(),
            failed: error.failed,
            ..Failure::new(error.code.code(), error.message, exit)
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
                if let Some(backup) = &failure.backup {
                    anstream::eprintln!("  backup: {}", backup.display());
                }
                if let Some(failed) = &failure.failed {
                    anstream::eprint!("{}", render_rollback(&failed.rollback, INDENT));
                }
                for diff in &failure.diffs {
                    anstream::eprint!(
                        "{}",
                        render_file_diff(diff.diff.as_deref(), diff.diff_note.as_deref(), None)
                    );
                }
                if let (Exit::RollbackIncomplete, Some(failed)) = (failure.exit, &failure.failed) {
                    anstream::eprintln!(
                        "Once its files are recovered, dismiss it with `config-digger history dismiss {}`.",
                        failed.operation
                    );
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

/// `history --json`: every Operation, newest first.
#[derive(Serialize)]
struct HistoryBody<'a> {
    operations: &'a [api::OperationSummary],
}

/// `history show --json`: one Operation.
#[derive(Serialize)]
struct OperationBody<'a> {
    operation: &'a api::OperationDetail,
}

/// `history dismiss --json`: the Operation as History now lists it.
#[derive(Serialize)]
struct DismissBody<'a> {
    operation: &'a api::OperationSummary,
}

fn run(cli: &Cli) -> Result<(), Failure> {
    if let Command::Completions { shell } = &cli.command {
        return completions(cli, *shell);
    }
    let core = Core::new(Ports::real())?;
    // Startup pruning is best-effort: History is pruned again next time.
    if let Err(error) = core.prune_history() {
        tracing::warn!(%error, "history not pruned");
    }
    let width = terminal::output_width(cli.wide);
    match &cli.command {
        Command::List { selector } => {
            let mut inventory = scanned(cli, &core)?.inventory;
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
                anstream::print!("{}", render_list(&inventory, selector.is_some(), width));
            }
            Ok(())
        }
        Command::Show { selector } => {
            let inventory = scanned(cli, &core)?.inventory;
            let item = inventory.select_one(selector)?;
            if cli.json {
                print_json(&ShowBody {
                    project: &inventory.project,
                    item,
                    read_failures: &inventory.read_failures,
                });
            } else {
                anstream::print!("{}", render_show(&inventory, item, width));
            }
            Ok(())
        }
        Command::Remove {
            selectors,
            all,
            yes,
            ack,
            dry_run,
        } => {
            let scanned = scanned(cli, &core)?;
            let targets = Targets {
                selectors: selectors.clone(),
                all: *all,
            };
            let plan = core.plan(
                &scanned.snapshot,
                &scanned.context,
                Action::Remove,
                &targets,
            )?;
            confirm_and_execute(
                cli,
                &core,
                plan,
                Some(&project_line(&scanned.inventory, width)),
                &WriteFlags {
                    yes: *yes,
                    ack: ack.clone(),
                    dry_run: *dry_run,
                },
            )
        }
        Command::History { command: None } => {
            let operations = core.history()?;
            if cli.json {
                print_json(&HistoryBody {
                    operations: &operations,
                });
            } else {
                anstream::print!("{}", render_history(&operations, width));
            }
            Ok(())
        }
        Command::History {
            command: Some(HistoryCommand::Show { id }),
        } => {
            let operation = core.history_show(id)?;
            if cli.json {
                print_json(&OperationBody {
                    operation: &operation,
                });
            } else {
                anstream::print!("{}", render_operation(&operation, width));
            }
            Ok(())
        }
        Command::History {
            command: Some(HistoryCommand::Dismiss { id }),
        } => {
            let operation = core.dismiss(id)?;
            if cli.json {
                print_json(&DismissBody {
                    operation: &operation,
                });
            } else {
                anstream::print!(
                    "{}",
                    plain_line(
                        &format!(
                            "Dismissed Operation {}: {}.",
                            operation.id, operation.status_text
                        ),
                        width
                    )
                );
            }
            Ok(())
        }
        Command::History {
            command:
                Some(HistoryCommand::DeleteBackup {
                    id,
                    yes,
                    ack,
                    dry_run,
                }),
        } => {
            let plan = core.plan_delete_backup(id)?;
            // It resolves no State, so there is no Project to name.
            confirm_and_execute(
                cli,
                &core,
                plan,
                None,
                &WriteFlags {
                    yes: *yes,
                    ack: ack.clone(),
                    dry_run: *dry_run,
                },
            )
        }
        Command::Undo {
            id,
            yes,
            ack,
            dry_run,
        } => {
            let plan = core.plan_undo(id)?;
            // An Undo resolves in the context its Operation ran in.
            let project = context_line(plan.preview().project.as_deref(), width);
            confirm_and_execute(
                cli,
                &core,
                plan,
                Some(&project),
                &WriteFlags {
                    yes: *yes,
                    ack: ack.clone(),
                    dry_run: *dry_run,
                },
            )
        }
        Command::Completions { .. } => unreachable!("completions run without a core"),
    }
}

fn completions(cli: &Cli, shell: Shell) -> Result<(), Failure> {
    if cli.json {
        return Err(Failure::usage(
            "completions prints a shell script, not a JSON document",
        ));
    }
    clap_complete::generate(
        clap_complete::Shell::from(shell),
        &mut Cli::command(),
        "config-digger",
        &mut std::io::stdout(),
    );
    Ok(())
}

/// What [`scanned`] read and resolved.
struct Scanned {
    snapshot: Snapshot,
    context: Context,
    inventory: Inventory,
}

/// Scans and resolves for the Project the flags ask for, printing Read
/// failures to stderr as warnings.
fn scanned(cli: &Cli, core: &Core) -> Result<Scanned, Failure> {
    let project = if cli.no_project {
        None
    } else {
        let project = match &cli.project {
            Some(project) => std::path::absolute(project),
            None => std::env::current_dir(),
        };
        Some(project.map_err(api::Error::unexpected)?)
    };
    let snapshot = core.scan(&ScanOptions {
        projects: project.iter().cloned().collect(),
    })?;
    let context = Context { project };
    let inventory = core.resolve(&snapshot, &context);
    for failure in &inventory.read_failures {
        anstream::eprintln!(
            "{WARNING}warning:{WARNING:#} {} cannot read {} ({})",
            failure.agent_name,
            failure.path.display(),
            failure.kind.code()
        );
    }
    Ok(Scanned {
        snapshot,
        context,
        inventory,
    })
}

/// A write's JSON document: the plan, its warnings and, unless it was a dry
/// run, the result.
#[derive(Serialize)]
struct WriteBody<'a> {
    plan: &'a api::Preview,
    warnings: Vec<WarningBody<'a>>,
    result: Option<&'a api::Outcome>,
}

#[derive(Serialize)]
struct WarningBody<'a> {
    class: WarningClass,
    sentence: &'a str,
    acknowledged: bool,
}

fn warning_bodies<'a>(
    warnings: &'a [api::Warning],
    acknowledged: &[WarningClass],
) -> Vec<WarningBody<'a>> {
    warnings
        .iter()
        .map(|warning| WarningBody {
            class: warning.class,
            sentence: &warning.sentence,
            acknowledged: acknowledged.contains(&warning.class),
        })
        .collect()
}

/// Prints a plan's preview under the line naming its Project, if it has one,
/// gets the Operation confirmed and its warnings acknowledged, then executes it.
fn confirm_and_execute(
    cli: &Cli,
    core: &Core,
    plan: Plan,
    project_line: Option<&str>,
    flags: &WriteFlags,
) -> Result<(), Failure> {
    let preview = plan.preview().clone();
    let warnings = plan.warnings().to_vec();
    let mut acknowledged: Vec<WarningClass> = flags.ack.iter().map(|&ack| ack.into()).collect();
    let width = terminal::output_width(cli.wide);
    if !cli.json {
        anstream::print!(
            "{}",
            render_preview(project_line, &preview, &warnings, width)
        );
    }
    if flags.dry_run {
        if cli.json {
            print_json(&WriteBody {
                plan: &preview,
                warnings: warning_bodies(&warnings, &acknowledged),
                result: None,
            });
        } else if preview.action == Action::DeleteBackup {
            anstream::println!("\nDry run: nothing was deleted.");
        } else {
            anstream::println!("\nDry run: nothing was written.");
        }
        return Ok(());
    }
    confirm(cli, &preview.title, flags.yes, &warnings, &mut acknowledged)?;
    let outcome = core.execute(plan, &acknowledged)?;
    if cli.json {
        print_json(&WriteBody {
            plan: &preview,
            warnings: warning_bodies(&warnings, &acknowledged),
            result: Some(&outcome),
        });
    } else {
        anstream::print!("{}", render_outcome(preview.action, &outcome, width));
    }
    Ok(())
}

/// The flags every write takes.
struct WriteFlags {
    yes: bool,
    ack: Vec<Ack>,
    dry_run: bool,
}

/// Gets the Operation confirmed, then each warning not acknowledged by
/// `--ack`. On a terminal it asks; otherwise `--yes` and `--ack` must say so.
fn confirm(
    cli: &Cli,
    title: &str,
    yes: bool,
    warnings: &[api::Warning],
    acknowledged: &mut Vec<WarningClass>,
) -> Result<(), Failure> {
    let interactive =
        !cli.json && std::io::stdin().is_terminal() && std::io::stderr().is_terminal();
    if !yes {
        if !interactive {
            return Err(Failure::new(
                "confirmation_required",
                "nothing was written: confirm the Operation with --yes",
                Exit::NotConfirmed,
            ));
        }
        if !ask(&format!("\n{title}? [y/N] "))? {
            return Err(Failure::new(
                "cancelled",
                "cancelled; nothing was written",
                Exit::NotConfirmed,
            ));
        }
    }
    let mut missing: Vec<&str> = Vec::new();
    for warning in warnings {
        if acknowledged.contains(&warning.class) {
            continue;
        }
        if interactive && ask(&format!("{} Acknowledge? [y/N] ", warning.sentence))? {
            acknowledged.push(warning.class);
        } else if !missing.contains(&warning.class.code()) {
            missing.push(warning.class.code());
        }
    }
    if !missing.is_empty() {
        let flags: Vec<String> = missing
            .iter()
            .map(|class| format!("--ack {class}"))
            .collect();
        return Err(Failure::new(
            "ack_required",
            format!(
                "nothing was written: acknowledge each warning with {}",
                flags.join(" ")
            ),
            Exit::NotConfirmed,
        ));
    }
    Ok(())
}

/// Asks a yes-or-no question on stderr; only `y` or `yes` is yes.
fn ask(question: &str) -> Result<bool, Failure> {
    anstream::eprint!("{question}");
    let mut answer = String::new();
    std::io::stdin()
        .read_line(&mut answer)
        .map_err(api::Error::unexpected)?;
    Ok(matches!(
        answer.trim().to_ascii_lowercase().as_str(),
        "y" | "yes"
    ))
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
    let missing = inventory.project.as_ref().is_some_and(|project| {
        inventory
            .projects
            .iter()
            .any(|known| known.path == *project && known.missing)
    });
    let Some(project) = &inventory.project else {
        return context_line(None, width);
    };
    let suffix = if missing { " (missing)" } else { "" };
    plain_line(&format!("Project: {}{suffix}", project.display()), width)
}

/// The line naming a Project, or saying there is none.
fn context_line(project: Option<&Path>, width: Option<usize>) -> String {
    match project {
        Some(project) => plain_line(&format!("Project: {}", project.display()), width),
        None => plain_line("No Project", width),
    }
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
        // The configuration as the core masked it, wrapped rather than cut.
        for text in terminal::wrap(&entry.config.to_string(), reason_width) {
            let mut line = Line::default();
            line.push(reason_indent.as_str(), Style::new())
                .push(text, DIM);
            out.push_str(&line.fit(width).render());
        }
    }
    out
}

/// A line of unstyled text, fitted to `width`.
fn plain_line(text: &str, width: Option<usize>) -> String {
    styled_line(text, Style::new(), width)
}

/// A line of text in one style, fitted to `width`.
fn styled_line(text: &str, style: Style, width: Option<usize>) -> String {
    let mut line = Line::default();
    line.push(text, style);
    line.fit(width).render()
}

/// An Entry's locator, shortened to `width` by eliding the middle of its path:
/// the key path is what tells Entries of one file apart, so it stays whole.
fn location(entry: &api::Entry, width: Option<usize>) -> String {
    elide_locator(&entry.locator, &entry.path, width)
}

/// The locator of an Entry in the file at `path`, shortened like [`location`].
fn elide_locator(locator: &str, path: &Path, width: Option<usize>) -> String {
    let Some(width) = width.filter(|&width| columns(locator) > width) else {
        return locator.to_owned();
    };
    let path = path.display().to_string();
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

/// A write's preview: the State before and after of each (Agent, Entry,
/// Project) it changes, which Entry is Effective afterwards, the files diff,
/// the warnings and where the backup goes. A Delete backup lists the copies
/// it deletes instead.
fn render_preview(
    project_line: Option<&str>,
    preview: &api::Preview,
    warnings: &[api::Warning],
    width: Option<usize>,
) -> String {
    let mut out = String::new();
    if let Some(project_line) = project_line {
        out.push_str(project_line);
        out.push('\n');
    }
    out.push_str(&styled_line(&preview.title, BOLD, width));

    if preview.action == Action::DeleteBackup {
        out.push('\n');
        out.push_str(&styled_line("Deleted for good", BOLD, width));
        if preview.deleted.is_empty() {
            out.push_str(&plain_line(
                &format!("{INDENT}No file was copied to this backup."),
                width,
            ));
        }
        // A path is never cut: it names what goes for good.
        for copy in &preview.deleted {
            out.push_str(&format!("{INDENT}{}\n", copy.display()));
        }
    } else {
        out.push_str(&render_changes(&preview.changes, width));
        let sentence_width = width.map(|width| width.saturating_sub(INDENT.len()));
        for effective in &preview.effective {
            out.push('\n');
            for text in terminal::wrap(&effective.sentence, sentence_width) {
                out.push_str(&plain_line(&format!("{INDENT}{text}"), width));
            }
        }

        out.push('\n');
        out.push_str(&styled_line("Files", BOLD, width));
        for file in &preview.files {
            out.push_str(&render_file_diff(
                file.diff.as_deref(),
                file.diff_note.as_deref(),
                width,
            ));
        }
    }

    if !warnings.is_empty() {
        out.push('\n');
        out.push_str(&styled_line("Warnings", BOLD, width));
        for warning in warnings {
            let text = format!("{INDENT}[{}] {}", warning.class.code(), warning.sentence);
            for line in terminal::wrap(&text, width) {
                out.push_str(&plain_line(&line, width));
            }
        }
    }

    out.push('\n');
    for line in terminal::wrap(&preview.backup_note, width) {
        out.push_str(&plain_line(&line, width));
    }
    out
}

/// What an executed write did, any State that differs from the prediction,
/// and where its backup is, unless it deleted one.
fn render_outcome(action: Action, outcome: &api::Outcome, width: Option<usize>) -> String {
    let mut out = String::from("\n");
    for line in terminal::wrap(&outcome.sentence, width) {
        out.push_str(&plain_line(&line, width));
    }
    for difference in &outcome.differences {
        out.push_str(&plain_line(
            &format!(
                "{INDENT}{}: predicted {}, actually {}",
                difference.locator,
                state_after_label(difference.predicted),
                state_after_label(difference.actual)
            ),
            width,
        ));
    }
    if action != Action::DeleteBackup {
        out.push_str(&plain_line(
            &format!("Backup: {}", outcome.backup.display()),
            width,
        ));
    }
    out
}

/// A file's masked diff, or the core's note on why it cannot be shown.
fn render_file_diff(diff: Option<&str>, note: Option<&str>, width: Option<usize>) -> String {
    match (diff, note) {
        (Some(diff), _) => render_diff(diff),
        (None, Some(note)) => terminal::wrap(&format!("{INDENT}{note}"), width)
            .iter()
            .map(|line| plain_line(line, width))
            .collect(),
        (None, None) => String::new(),
    }
}

/// A unified diff, coloured line by line. A diff line is never cut: a hidden
/// change is worse than a long line.
fn render_diff(diff: &str) -> String {
    let mut out = String::new();
    for line in diff.lines() {
        let style = match line.as_bytes().first() {
            Some(b'+') if !line.starts_with("+++") => AnsiColor::Green.on_default(),
            Some(b'-') if !line.starts_with("---") => AnsiColor::Red.on_default(),
            Some(b'@') => AnsiColor::Cyan.on_default(),
            // A note, such as `\ secret value changed`.
            Some(b'\\') => DIM,
            _ => Style::new(),
        };
        out.push_str(&format!("{style}{line}{style:#}\n"));
    }
    out
}

/// Every Operation, newest first: its id, time, status, whether it can be
/// undone and its title, then why it cannot be undone when it cannot.
fn render_history(operations: &[api::OperationSummary], width: Option<usize>) -> String {
    if operations.is_empty() {
        return plain_line("No Operations in History.", width);
    }
    let rows: Vec<[(&str, Style); 4]> = operations
        .iter()
        .map(|operation| {
            [
                (operation.id.as_str(), Style::new()),
                (operation.created_at.as_str(), Style::new()),
                (operation.status_text.as_str(), Style::new()),
                (if operation.undoable { "yes" } else { "no" }, Style::new()),
            ]
        })
        .collect();
    let header = [("ID", DIM), ("TIME", DIM), ("STATUS", DIM), ("UNDO", DIM)];
    let widths = column_widths(&header, &rows);
    let mut out = table_line(&header, &widths, "SUMMARY".to_owned(), DIM)
        .fit(width)
        .render();
    // Why one cannot be undone sits under the time column.
    let reason_indent = " ".repeat(INDENT.len() + widths[0] + GAP.len());
    let reason_width = width.map(|width| width.saturating_sub(reason_indent.len()));
    for (operation, cells) in operations.iter().zip(&rows) {
        out.push_str(
            &table_line(cells, &widths, operation.title.clone(), Style::new())
                .fit(width)
                .render(),
        );
        let cause = operation
            .cause
            .as_ref()
            .map(|cause| format!("Cause: {cause}"));
        for reason in cause.iter().chain(&operation.not_undoable) {
            for text in terminal::wrap(reason, reason_width) {
                out.push_str(&plain_line(&format!("{reason_indent}{text}"), width));
            }
        }
    }
    out
}

/// One Operation: its status and context, the States it changed, and per
/// file its edits and where the file was copied before it.
fn render_operation(operation: &api::OperationDetail, width: Option<usize>) -> String {
    let summary = &operation.summary;
    let mut out = styled_line(
        &format!("Operation {}: {}", summary.id, summary.title),
        BOLD,
        width,
    );
    out.push_str(&plain_line(&format!("Time: {}", summary.created_at), width));
    out.push_str(&plain_line(
        &format!("Status: {}", summary.status_text),
        width,
    ));
    let undo = match &summary.not_undoable {
        None => "Undo: yes".to_owned(),
        Some(reason) => format!("Undo: no. {reason}"),
    };
    for line in terminal::wrap(&undo, width) {
        out.push_str(&plain_line(&line, width));
    }
    if let Some(cause) = &operation.cause {
        for line in terminal::wrap(&format!("Cause: {cause}"), width) {
            out.push_str(&plain_line(&line, width));
        }
    }
    out.push_str(&context_line(operation.project.as_deref(), width));
    out.push_str(&plain_line(
        &format!("Backup: {}", operation.backup.display()),
        width,
    ));

    if !operation.rollback.is_empty() {
        out.push('\n');
        out.push_str(&styled_line("Rollback", BOLD, width));
        out.push_str(&render_rollback(&operation.rollback, INDENT));
    }

    if !operation.changes.is_empty() {
        out.push('\n');
        out.push_str(&styled_line("Changes", BOLD, width));
        out.push_str(&render_changes(&operation.changes, width));
    }

    out.push('\n');
    out.push_str(&styled_line("Files", BOLD, width));
    for location in &operation.locations {
        out.push_str(&plain_line(
            &format!("{INDENT}{}", location.path.display()),
            width,
        ));
        for edit in &location.edits {
            out.push_str(&plain_line(
                &format!(
                    "{INDENT}{INDENT}{} {}",
                    edit_label(edit.op),
                    edit.key_path.join(".")
                ),
                width,
            ));
        }
        let backup = match &location.backup {
            Some(backup) => format!("backed up to {}", backup.display()),
            None if summary.backup_deleted => "its backup was deleted".to_owned(),
            None => "created by this Operation, so nothing was backed up".to_owned(),
        };
        out.push_str(&plain_line(&format!("{INDENT}{INDENT}{backup}"), width));
    }
    out
}

/// Where a failed Operation's Rollback left each file, one per line after
/// `indent`, then why when it was not restored. Never cut: it is what manual
/// recovery starts from.
fn render_rollback(rollback: &[api::RollbackLocation], indent: &str) -> String {
    let mut out = String::new();
    for location in rollback {
        let (label, style) = match location.outcome {
            api::RollbackOutcome::Restored => ("restored", AnsiColor::Green.on_default()),
            api::RollbackOutcome::NotWritten => ("not written", Style::new()),
            api::RollbackOutcome::LeftAlone => ("left alone", Style::new()),
            api::RollbackOutcome::NotRestored => ("not restored", ERROR),
        };
        out.push_str(&format!(
            "{indent}{style}{label}{style:#}: {}\n",
            location.path.display()
        ));
        if let Some(reason) = &location.reason {
            out.push_str(&format!("{indent}{INDENT}{reason}\n"));
        }
    }
    out
}

fn edit_label(op: api::EditOp) -> &'static str {
    match op {
        api::EditOp::Remove => "remove",
        api::EditOp::Set => "set",
        api::EditOp::Insert => "insert",
        api::EditOp::Append => "append",
        api::EditOp::RemoveItem => "remove item",
    }
}

fn state_after_label(state: api::StateAfter) -> &'static str {
    match state {
        api::StateAfter::Effective => "Effective",
        api::StateAfter::Shadowed => "Shadowed",
        api::StateAfter::Removed => "removed",
    }
}

fn state_after_style(state: api::StateAfter) -> Style {
    match state {
        api::StateAfter::Effective => state_style(api::State::Effective),
        api::StateAfter::Shadowed => state_style(api::State::Shadowed),
        api::StateAfter::Removed => AnsiColor::Red.on_default(),
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

/// The State before and after of each (Agent, Entry, Project) an Operation
/// changes, one row each.
fn render_changes(changes: &[api::Change], width: Option<usize>) -> String {
    let rows: Vec<[(&str, Style); 4]> = changes
        .iter()
        .map(|change| {
            [
                (change.agent_name.as_str(), Style::new()),
                (change.item.as_str(), Style::new()),
                (
                    state_after_label(change.before),
                    state_after_style(change.before),
                ),
                (
                    state_after_label(change.after),
                    state_after_style(change.after),
                ),
            ]
        })
        .collect();
    let header = [
        ("AGENT", DIM),
        ("ITEM", DIM),
        ("BEFORE", DIM),
        ("AFTER", DIM),
    ];
    let widths = column_widths(&header, &rows);
    let location_width = last_column_width(&widths, width);
    let mut out = table_line(&header, &widths, "ENTRY".to_owned(), DIM)
        .fit(width)
        .render();
    for (change, cells) in changes.iter().zip(&rows) {
        let entry = elide_locator(&change.locator, &change.path, location_width);
        out.push_str(
            &table_line(cells, &widths, entry, Style::new())
                .fit(width)
                .render(),
        );
    }
    out
}
