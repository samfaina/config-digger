//! `agents`, `homes` and `projects`: the loaded Agent definitions, and the
//! Agent homes and Projects the user adds, hides and forgets.

use super::{
    BOLD, Cli, DIM, Failure, INDENT, WARNING, column_widths, last_column_width, plain_line,
    print_json, table_line,
};
use crate::terminal::{self, Line};
use anstyle::Style;
use clap::Subcommand;
use config_digger_core::api::{self, Context, Core, Inventory, ScanOptions};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Subcommand)]
pub(crate) enum HomesCommand {
    /// List the Agent homes found and added, with where each came from.
    List,
    /// Add an existing folder as an Agent home of an Agent.
    ///
    /// The folder is read as the Agent's default home is laid out, with no
    /// other check: it stands where `homes list` shows the default home's
    /// folder, such as the folder holding `.claude.json` for Claude Code. A
    /// folder with none of the files the Agent reads is listed with "No
    /// configuration found".
    Add {
        /// The folder.
        path: PathBuf,
        /// The Agent whose home it is, by id, as `agents` lists it.
        #[arg(long, value_name = "ID")]
        agent: String,
    },
    /// Hide an Agent home, discovered or added: it and its Items are listed
    /// only with --include-hidden.
    Hide(HomeSelector),
    /// List a hidden Agent home again.
    Unhide(HomeSelector),
    /// Forget an Agent home you added. A home found without you can only be hidden.
    Forget(HomeSelector),
}

/// The Agent home a command acts on.
#[derive(clap::Args)]
pub(crate) struct HomeSelector {
    /// The home's id or folder, as `homes list` shows them.
    home: String,
    /// The home's Agent, when homes of several Agents match.
    #[arg(long, value_name = "ID")]
    agent: Option<String>,
}

impl HomeSelector {
    /// The selector as the core takes it: a folder relative to the current
    /// directory made absolute, anything else as typed.
    fn home(&self) -> String {
        let path = Path::new(&self.home);
        match std::path::absolute(path) {
            Ok(absolute) if path.is_relative() && path.is_dir() => absolute.display().to_string(),
            _ => self.home.clone(),
        }
    }
}

#[derive(Subcommand)]
pub(crate) enum ProjectsCommand {
    /// List the Projects the Agents record and the ones added, with where each came from.
    ///
    /// A missing Project with no leftovers, such as a deleted worktree
    /// Claude Code still records, is left out of the text unless
    /// --all-missing asks for it, or it is hidden and --include-hidden asks
    /// for the hidden ones; --json lists it either way.
    List {
        /// Also list the missing Projects with no leftovers, which only
        /// `projects hide` acts on.
        #[arg(long)]
        all_missing: bool,
    },
    /// Add an existing folder or `.code-workspace` file as a Project.
    Add {
        /// The folder or `.code-workspace` file.
        path: PathBuf,
    },
    /// Hide a Project, discovered or added: it is listed only with
    /// --include-hidden, and still evaluated when picked with --project.
    Hide {
        /// The Project, in any spelling.
        path: PathBuf,
    },
    /// List a hidden Project again.
    Unhide {
        /// The Project, in any spelling.
        path: PathBuf,
    },
    /// Forget a Project you added. A Project an Agent records can only be hidden.
    Forget {
        /// The Project, in any spelling.
        path: PathBuf,
    },
}

/// `agents --json`.
#[derive(Serialize)]
struct AgentsBody<'a> {
    agents: &'a [api::Agent],
}

/// `homes list --json`.
#[derive(Serialize)]
struct HomesBody<'a> {
    homes: &'a [api::Home],
}

/// `homes add|hide|unhide|forget --json`.
#[derive(Serialize)]
struct HomeBody<'a> {
    home: &'a api::Home,
}

/// `projects list --json`.
#[derive(Serialize)]
struct ProjectsBody<'a> {
    projects: &'a [api::Project],
}

/// `projects add|hide|unhide|forget --json`.
#[derive(Serialize)]
struct ProjectBody<'a> {
    project: &'a api::Project,
}

/// Every Agent home and Project, with no Project as context, without the
/// hidden ones unless asked for.
fn listing(cli: &Cli, core: &Core) -> Result<Inventory, Failure> {
    let snapshot = core.scan(&ScanOptions::default())?;
    let inventory = core.resolve(&snapshot, &Context::default());
    Ok(if cli.include_hidden {
        inventory
    } else {
        inventory.without_hidden()
    })
}

pub(crate) fn agents(cli: &Cli, core: &Core) -> Result<(), Failure> {
    let agents = core.agents(&listing(cli, core)?);
    if cli.json {
        print_json(&AgentsBody { agents: &agents });
    } else {
        anstream::print!(
            "{}",
            render_agents(&agents, terminal::output_width(cli.wide))
        );
    }
    Ok(())
}

pub(crate) fn homes(cli: &Cli, core: &Core, command: &HomesCommand) -> Result<(), Failure> {
    let width = terminal::output_width(cli.wide);
    let (home, done) = match command {
        HomesCommand::List => {
            let inventory = listing(cli, core)?;
            if cli.json {
                print_json(&HomesBody {
                    homes: &inventory.homes,
                });
            } else {
                anstream::print!("{}", render_homes(&inventory.homes, width));
            }
            return Ok(());
        }
        HomesCommand::Add { path, agent } => {
            let path = std::path::absolute(path).map_err(api::Error::unexpected)?;
            (core.add_home(&path, agent)?, "Added")
        }
        HomesCommand::Hide(selector) => (
            core.hide_home(&selector.home(), selector.agent.as_deref())?,
            "Hid",
        ),
        HomesCommand::Unhide(selector) => (
            core.unhide_home(&selector.home(), selector.agent.as_deref())?,
            "Listed again",
        ),
        HomesCommand::Forget(selector) => (
            core.forget_home(&selector.home(), selector.agent.as_deref())?,
            "Forgot",
        ),
    };
    if cli.json {
        print_json(&HomeBody { home: &home });
    } else {
        let sentence = format!("{done} Agent home {}.", home_label(&home));
        anstream::print!("{}", plain_line(&sentence, width));
        let added = matches!(command, HomesCommand::Add { .. });
        if let (Some(note), true) = (&home.no_configuration, added) {
            anstream::print!("{}", plain_line(&format!("{note}."), width));
        }
    }
    Ok(())
}

pub(crate) fn projects(cli: &Cli, core: &Core, command: &ProjectsCommand) -> Result<(), Failure> {
    let width = terminal::output_width(cli.wide);
    let absolute = |path: &Path| std::path::absolute(path).map_err(api::Error::unexpected);
    let (project, done) = match command {
        ProjectsCommand::List { all_missing } => {
            let snapshot = core.scan(&ScanOptions::default())?;
            let mut inventory = core.resolve(&snapshot, &Context::default());
            if cli.json {
                if !cli.include_hidden {
                    inventory = inventory.without_hidden();
                }
                print_json(&ProjectsBody {
                    projects: &inventory.projects,
                });
                return Ok(());
            }
            let mut leftovers = core.leftovers(&snapshot);
            if !cli.include_hidden {
                leftovers = inventory.leftovers_without_hidden(leftovers);
                inventory = inventory.without_hidden();
            }
            let listed = inventory.projects.len();
            if !all_missing {
                inventory = inventory.without_missing_lacking_leftovers(&leftovers);
            }
            let agents = core.agents(&inventory);
            anstream::print!("{}", render_projects(&inventory.projects, &agents, width));
            let left_out = listed - inventory.projects.len();
            if left_out > 0 {
                anstream::print!("{}", plain_line(&left_out_sentence(left_out), width));
            }
            return Ok(());
        }
        ProjectsCommand::Add { path } => (core.add_project(&absolute(path)?)?, "Added"),
        ProjectsCommand::Hide { path } => (core.hide_project(&absolute(path)?)?, "Hid"),
        ProjectsCommand::Unhide { path } => {
            (core.unhide_project(&absolute(path)?)?, "Listed again")
        }
        ProjectsCommand::Forget { path } => (core.forget_project(&absolute(path)?)?, "Forgot"),
    };
    if cli.json {
        print_json(&ProjectBody { project: &project });
    } else {
        let sentence = format!("{done} Project {}.", project.path.display());
        anstream::print!("{}", plain_line(&sentence, width));
    }
    Ok(())
}

/// An Agent home as a sentence names it: its id, unless that is its
/// folder, its Agent and its folder.
fn home_label(home: &api::Home) -> String {
    let folder = home.folder.display().to_string();
    if home.home == folder {
        format!("{folder} of {}", home.agent_name)
    } else {
        format!("{} of {} ({folder})", home.home, home.agent_name)
    }
}

/// An Agent home's id as `homes list` and `agents` show it: blank when it
/// is the folder, which they show anyway.
fn home_id(home: &api::Home) -> &str {
    if home.home == home.folder.display().to_string() {
        ""
    } else {
        &home.home
    }
}

/// Where an Agent home came from, as `homes list` and `agents` show it.
fn home_source(home: &api::Home) -> String {
    let source = match &home.source {
        api::HomeSource::Default => "definition default".to_owned(),
        api::HomeSource::Environment { variable } => format!("environment variable %{variable}%"),
        api::HomeSource::KnownLocation => "known location".to_owned(),
        api::HomeSource::Added => "added by you".to_owned(),
    };
    if home.hidden {
        format!("{source}, hidden")
    } else {
        source
    }
}

/// One table of Agent homes: each home's Agent, id and source.
fn render_homes(homes: &[api::Home], width: Option<usize>) -> String {
    if homes.is_empty() {
        return plain_line("No Agent home found.", width);
    }
    let sources: Vec<String> = homes.iter().map(home_source).collect();
    let rows: Vec<[(&str, Style); 3]> = homes
        .iter()
        .zip(&sources)
        .map(|(home, source)| {
            [
                (home.agent_name.as_str(), Style::new()),
                (home_id(home), Style::new()),
                (source.as_str(), Style::new()),
            ]
        })
        .collect();
    let header = [("AGENT", DIM), ("HOME", DIM), ("SOURCE", DIM)];
    home_table(homes, &header, &rows, width, |_| Vec::new())
}

/// A table with one row of `cells` per home, then its folder, and under it
/// any note, then the lines `more` adds for it.
fn home_table<const N: usize>(
    homes: &[api::Home],
    header: &[(&str, Style); N],
    rows: &[[(&str, Style); N]],
    width: Option<usize>,
    more: impl Fn(&api::Home) -> Vec<String>,
) -> String {
    let widths = column_widths(header, rows);
    let folder_width = last_column_width(&widths, width);
    let mut out = table_line(header, &widths, "FOLDER".to_owned(), DIM)
        .fit(width)
        .render();
    for (home, cells) in homes.iter().zip(rows) {
        let folder = folder_cell(&home.folder, folder_width);
        out.push_str(
            &table_line(cells, &widths, folder, Style::new())
                .fit(width)
                .render(),
        );
        out.push_str(&home_notes(home, width));
        for line in more(home) {
            out.push_str(&plain_line(&format!("{INDENT}{INDENT}{line}"), width));
        }
    }
    out
}

/// A folder, its middle elided to fit `width`.
fn folder_cell(folder: &Path, width: Option<usize>) -> String {
    let folder = folder.display().to_string();
    match width {
        Some(width) if terminal::columns(&folder) > width => terminal::elide_middle(&folder, width),
        _ => folder,
    }
}

/// What to know about an Agent home, each on its own indented line: that
/// another tool copies it, that the Agent never reads it, and that none of
/// its files exist.
fn home_notes(home: &api::Home, width: Option<usize>) -> String {
    let mirror = home
        .mirror
        .iter()
        .map(|mirror| format!("{}: {}", mirror.label, mirror.sentence));
    let not_read = home
        .not_read
        .iter()
        .map(|not_read| not_read.sentence.clone());
    let no_configuration = home.no_configuration.iter().map(|note| format!("{note}."));
    mirror
        .chain(not_read)
        .chain(no_configuration)
        .map(|note| plain_line(&format!("{INDENT}{INDENT}{note}"), width))
        .collect()
}

/// Each loaded Agent definition, then the Agent homes listed for it.
fn render_agents(agents: &[api::Agent], width: Option<usize>) -> String {
    let mut out = String::new();
    for (index, agent) in agents.iter().enumerate() {
        if index > 0 {
            out.push('\n');
        }
        let definition = if agent.built_in {
            "built-in definition"
        } else {
            "definition from --definitions"
        };
        let mut title = Line::default();
        title.push(format!("{} ({}), {definition}", agent.name, agent.id), BOLD);
        out.push_str(&title.fit(width).render());
        if agent.homes.is_empty() {
            out.push_str(&plain_line(&format!("{INDENT}No Agent home found."), width));
            continue;
        }
        let sources: Vec<String> = agent.homes.iter().map(home_source).collect();
        let installed: Vec<String> = agent.homes.iter().map(installed).collect();
        let tested = match &agent.tested {
            Some(range) if range.min == range.max => range.min.clone(),
            Some(range) => format!("{}-{}", range.min, range.max),
            None => "untested".to_owned(),
        };
        let rows: Vec<[(&str, Style); 4]> = agent
            .homes
            .iter()
            .zip(&sources)
            .zip(&installed)
            .map(|((home, source), installed)| {
                [
                    (home_id(home), Style::new()),
                    (source.as_str(), Style::new()),
                    (
                        installed.as_str(),
                        if home.out_of_range {
                            WARNING
                        } else {
                            Style::new()
                        },
                    ),
                    (tested.as_str(), Style::new()),
                ]
            })
            .collect();
        let header = [
            ("HOME", DIM),
            ("SOURCE", DIM),
            ("INSTALLED", DIM),
            ("TESTED", DIM),
        ];
        out.push_str(&home_table(&agent.homes, &header, &rows, width, |home| {
            let last_ran = home
                .last_ran
                .iter()
                .map(|version| format!("Last ran with {version}."));
            last_ran
                .chain(home.notes.iter().map(|note| note.sentence.clone()))
                .collect()
        }));
    }
    out
}

/// The versions of the Installations that read an Agent home, as `agents`
/// shows them: each marked `!` when outside the tested range, `unknown`
/// when it cannot be read, and `none` without any Installation.
fn installed(home: &api::Home) -> String {
    if home.installations.is_empty() {
        return "none".to_owned();
    }
    let versions: Vec<String> = home
        .installations
        .iter()
        .map(|installation| {
            let version = installation.version.as_deref().unwrap_or("unknown");
            let mark = if installation.out_of_range { "!" } else { "" };
            format!("{version}{mark}")
        })
        .collect();
    versions.join(", ")
}

/// What `projects list` says of the missing Projects with no leftovers it
/// leaves out.
fn left_out_sentence(count: usize) -> String {
    if count == 1 {
        "1 missing Project with no leftovers is not listed; --all-missing lists it".to_owned()
    } else {
        format!(
            "{count} missing Projects with no leftovers are not listed; --all-missing lists them"
        )
    }
}

/// One table of Projects: where each came from, then its path.
fn render_projects(
    projects: &[api::Project],
    agents: &[api::Agent],
    width: Option<usize>,
) -> String {
    if projects.is_empty() {
        return plain_line("No Project found.", width);
    }
    let agent_name = |id: &str| {
        agents
            .iter()
            .find(|agent| agent.id == id)
            .map_or(id.to_owned(), |agent| agent.name.clone())
    };
    let sources: Vec<String> = projects
        .iter()
        .map(|project| {
            let mut source: Vec<String> = Vec::new();
            if !project.registered_by.is_empty() {
                let names: Vec<String> = project
                    .registered_by
                    .iter()
                    .map(|id| agent_name(id))
                    .collect();
                source.push(format!("registry of {}", names.join(", ")));
            }
            if project.added {
                source.push("added by you".to_owned());
            }
            if project.workspace_file {
                source.push("workspace file".to_owned());
            }
            if project.missing {
                source.push("missing".to_owned());
            }
            if project.hidden {
                source.push("hidden".to_owned());
            }
            source.join(", ")
        })
        .collect();
    let rows: Vec<[(&str, Style); 1]> = sources
        .iter()
        .map(|source| [(source.as_str(), Style::new())])
        .collect();
    let header = [("SOURCE", DIM)];
    let widths = column_widths(&header, &rows);
    let path_width = last_column_width(&widths, width);
    let mut out = table_line(&header, &widths, "PATH".to_owned(), DIM)
        .fit(width)
        .render();
    for (project, cells) in projects.iter().zip(&rows) {
        let path = folder_cell(&project.path, path_width);
        out.push_str(
            &table_line(cells, &widths, path, Style::new())
                .fit(width)
                .render(),
        );
    }
    out
}
