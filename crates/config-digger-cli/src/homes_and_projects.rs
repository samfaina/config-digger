//! `agents`, `homes` and `projects`: the loaded Agent definitions, and the
//! Agent homes and Projects the user adds, hides and forgets.

use super::{
    BOLD, Cli, DIM, Failure, INDENT, column_widths, last_column_width, plain_line, print_json,
    table_line,
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
    /// The folder is read as the Agent's own home is laid out, with no other
    /// check: a folder with none of the files the Agent reads is listed with
    /// "No configuration found".
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

#[derive(Subcommand)]
pub(crate) enum ProjectsCommand {
    /// List the Projects the Agents record and the ones added, with where each came from.
    List,
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
            core.hide_home(&selector.home, selector.agent.as_deref())?,
            "Hid",
        ),
        HomesCommand::Unhide(selector) => (
            core.unhide_home(&selector.home, selector.agent.as_deref())?,
            "Listed again",
        ),
        HomesCommand::Forget(selector) => (
            core.forget_home(&selector.home, selector.agent.as_deref())?,
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
        ProjectsCommand::List => {
            let inventory = listing(cli, core)?;
            if cli.json {
                print_json(&ProjectsBody {
                    projects: &inventory.projects,
                });
            } else {
                let agents = core.agents(&inventory);
                anstream::print!("{}", render_projects(&inventory.projects, &agents, width));
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
    let source = match home.source {
        api::HomeSource::Default => "definition default",
        api::HomeSource::Added => "added by you",
    };
    if home.hidden {
        format!("{source}, hidden")
    } else {
        source.to_owned()
    }
}

/// One table of Agent homes: each home's Agent, id and source, then its
/// folder, and under it any note.
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
    let widths = column_widths(&header, &rows);
    let folder_width = last_column_width(&widths, width);
    let mut out = table_line(&header, &widths, "FOLDER".to_owned(), DIM)
        .fit(width)
        .render();
    for (home, cells) in homes.iter().zip(&rows) {
        let folder = folder_cell(&home.folder, folder_width);
        out.push_str(
            &table_line(cells, &widths, folder, Style::new())
                .fit(width)
                .render(),
        );
        out.push_str(&home_notes(home, width));
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

/// What to know about an Agent home, each on its own indented line.
fn home_notes(home: &api::Home, width: Option<usize>) -> String {
    home.no_configuration
        .iter()
        .map(|note| plain_line(&format!("{INDENT}{INDENT}{note}."), width))
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
        let rows: Vec<[(&str, Style); 2]> = agent
            .homes
            .iter()
            .zip(&sources)
            .map(|(home, source)| {
                [
                    (home_id(home), Style::new()),
                    (source.as_str(), Style::new()),
                ]
            })
            .collect();
        let header = [("HOME", DIM), ("SOURCE", DIM)];
        let widths = column_widths(&header, &rows);
        let folder_width = last_column_width(&widths, width);
        out.push_str(
            &table_line(&header, &widths, "FOLDER".to_owned(), DIM)
                .fit(width)
                .render(),
        );
        for (home, cells) in agent.homes.iter().zip(&rows) {
            let folder = folder_cell(&home.folder, folder_width);
            out.push_str(
                &table_line(cells, &widths, folder, Style::new())
                    .fit(width)
                    .render(),
            );
            out.push_str(&home_notes(home, width));
        }
    }
    out
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
            if project.workspace {
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
