//! Running Agents: which Agents that read the files an Operation writes are
//! running, and what each write then needs given its location's store
//! class: a block, a warning, and hints for the result. Detection is
//! best-effort and never closes an Agent.

use crate::api::{Block, BlockCode, Warning, WarningClass};
use crate::definitions::blocks::{Sessions, StoreClass};
use crate::definitions::{AgentDefinition, ProcessDefinition};
use crate::explain;
use crate::paths;
use crate::plan::PlannedFile;
use crate::ports::{Ports, RunningProcess};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

/// What the running Agents mean for an Operation's writes.
pub(crate) struct RunningCheck {
    /// One per running Agent that holds a written SQLite store open.
    pub blocks: Vec<Block>,
    /// One `running-agent` warning covering every other running reader.
    pub warning: Option<Warning>,
    /// What the result adds once the files are written.
    pub hints: Vec<String>,
}

/// A running Agent home that reads written files, with the store class of
/// each file it reads, the strongest when several of its locations do.
struct RunningReader<'d> {
    agent_name: &'d str,
    files: BTreeMap<PathBuf, StoreClass>,
}

impl RunningReader<'_> {
    fn paths(&self, class: StoreClass) -> Vec<&Path> {
        self.files
            .iter()
            .filter(|(_, file_class)| **file_class == class)
            .map(|(path, _)| path.as_path())
            .collect()
    }
}

/// Checks the readers of `files` against the processes running now.
pub(crate) fn check(
    definitions: &[AgentDefinition],
    ports: &Ports,
    files: &[PlannedFile],
) -> RunningCheck {
    let mut processes: Option<Vec<RunningProcess>> = None;
    let mut detected: BTreeMap<(usize, String), bool> = BTreeMap::new();
    let mut readers: Vec<((usize, String), RunningReader)> = Vec::new();
    for file in files {
        for reader in &file.readers {
            let definition = &definitions[reader.agent_index];
            let key = (reader.agent_index, reader.home.clone());
            let running = *detected.entry(key.clone()).or_insert_with(|| {
                definition.process(&reader.home).is_some_and(|process| {
                    let processes = processes.get_or_insert_with(|| ports.processes.running());
                    is_running(process, processes, ports)
                })
            });
            if !running {
                continue;
            }
            let class = definition.location(&reader.location).store_class;
            let index = match readers.iter().position(|(known, _)| *known == key) {
                Some(index) => index,
                None => {
                    readers.push((
                        key,
                        RunningReader {
                            agent_name: &definition.agent.name,
                            files: BTreeMap::new(),
                        },
                    ));
                    readers.len() - 1
                }
            };
            let stored = readers[index]
                .1
                .files
                .entry(file.path.clone())
                .or_insert(class);
            *stored = (*stored).max(class);
        }
    }

    let mut blocks = Vec::new();
    let mut sentences = Vec::new();
    let mut hints = Vec::new();
    for (_, reader) in &readers {
        let sqlite = reader.paths(StoreClass::Sqlite);
        if !sqlite.is_empty() {
            blocks.push(Block {
                code: BlockCode::RunningAgentSqlite,
                sentence: explain::running_agent_sqlite(reader.agent_name, &sqlite),
            });
        }
        let rewritten = reader.paths(StoreClass::RewrittenWhileRunning);
        if !rewritten.is_empty() {
            sentences.push(explain::running_agent_rewrites(
                reader.agent_name,
                &rewritten,
            ));
        }
        let plain = reader.paths(StoreClass::Plain);
        if !plain.is_empty() {
            sentences.push(explain::running_agent_reads(reader.agent_name, &plain));
        }
        hints.push(explain::restart_hint(reader.agent_name));
        for path in rewritten {
            hints.push(explain::may_be_overwritten(path, reader.agent_name));
        }
    }
    RunningCheck {
        blocks,
        warning: (!sentences.is_empty()).then(|| Warning {
            class: WarningClass::RunningAgent,
            sentence: sentences.join(" "),
        }),
        hints,
    }
}

/// Whether a process matches `process`, or a session file names a PID that
/// is alive.
fn is_running(process: &ProcessDefinition, running: &[RunningProcess], ports: &Ports) -> bool {
    if running
        .iter()
        .any(|candidate| matches(process, candidate, ports))
    {
        return true;
    }
    process.session_files.as_ref().is_some_and(|session_files| {
        session_files
            .pids(ports.fs.as_ref(), ports.env.as_ref())
            .iter()
            .any(|pid| running.iter().any(|candidate| candidate.pid == *pid))
    })
}

/// Whether one process has one of the images, a path that matches one of
/// the globs, unless the OS hides it, and every text in its command line.
fn matches(process: &ProcessDefinition, candidate: &RunningProcess, ports: &Ports) -> bool {
    let image = process
        .images
        .iter()
        .any(|image| image.eq_ignore_ascii_case(&candidate.image));
    let path = process.paths.is_empty()
        || candidate.path.as_deref().is_none_or(|path| {
            process
                .paths
                .iter()
                .any(|glob| paths::glob_matches(glob, path, ports.env.as_ref()))
        });
    let cmdline = candidate.cmdline.to_lowercase();
    let cmdline = process
        .cmdline_contains
        .iter()
        .all(|text| cmdline.contains(&text.to_lowercase()));
    image && path && cmdline
}
