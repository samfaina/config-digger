//! Running Agents: which Agents that read the files an Operation writes or
//! the skill folders it moves, or keep Disable switches in those files, are
//! running, and what each write then needs given the store class of the
//! location or switch: a block, a warning, and hints for the result.
//! Detection is best-effort and never closes an Agent.

use crate::api::{Advice, Block, BlockCode, Warning, WarningClass};
use crate::definitions::blocks::{Sessions, StoreClass};
use crate::definitions::{AgentDefinition, ProcessDefinition};
use crate::explain;
use crate::folders::PlannedFolder;
use crate::installations;
use crate::paths;
use crate::plan::PlannedFile;
use crate::ports::{Ports, RunningProcess};
use crate::scan::{LocationReader, Snapshot};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How long the check waits before it lists the processes again, for those
/// it found without a path: about how long a process takes to start or exit.
const PAUSE_BEFORE_LISTING_AGAIN: Duration = Duration::from_millis(250);

/// The Agent homes of a snapshot, by Agent index and id, that tell which
/// homes a running Agent may read.
#[derive(Default)]
pub(crate) struct RunningHomes {
    /// Those an Installation found reads: an Agent can run only for them.
    pub installed: BTreeSet<(usize, String)>,
    /// Those no launch of their Agent reads by lookup order, with their
    /// folders: homes the user added outside the definition's homes of an
    /// Agent that picks its home by lookup order, such as Claude Desktop.
    /// An Agent runs for one only while a matching process names its folder
    /// through the data folder switch of its process. A home added in the
    /// folder of a definition's home, however spelled, is that home and not
    /// one of them.
    pub added_outside_lookup_order: BTreeMap<(usize, String), PathBuf>,
}

impl RunningHomes {
    pub fn of(definitions: &[AgentDefinition], snapshot: &Snapshot) -> RunningHomes {
        RunningHomes {
            installed: installations::installed_homes(definitions, snapshot),
            added_outside_lookup_order: snapshot
                .homes
                .iter()
                .filter(|home| {
                    home.declared.is_none()
                        && !definitions[home.agent_index].lookup_orders.is_empty()
                })
                .map(|home| ((home.agent_index, home.home.clone()), home.folder.clone()))
                .collect(),
        }
    }
}

/// What the running Agents mean for an Operation's writes.
pub(crate) struct RunningCheck {
    /// The names of the running Agents that read a written file or a moved
    /// folder.
    pub agents: Vec<String>,
    /// One per running Agent that holds a written SQLite store open.
    pub blocks: Vec<Block>,
    /// One `running-agent` warning covering every other running reader.
    pub warning: Option<Warning>,
    /// What the result adds once the files are written.
    pub hints: Vec<String>,
}

/// A running Agent that reads written files or moved folders, or keeps
/// switches in those files, through any of its homes, with the store class
/// of each: the strongest when several of its locations or switches are
/// in it.
struct RunningReader<'d> {
    agent_name: &'d str,
    /// The written files and moved folders it reads.
    read: BTreeMap<PathBuf, StoreClass>,
    /// The files among them it rewrites from memory that an Entry is
    /// parked out of or put back into.
    parking: BTreeSet<PathBuf>,
    /// An Installation found reads one of those homes. Without one, its
    /// SQLite stores still block, but it gets no warning or hint.
    installed: bool,
}

impl RunningReader<'_> {
    /// The files and folders of `class`, but those that block as parking.
    fn paths(&self, class: StoreClass) -> Vec<&Path> {
        self.read
            .iter()
            .filter(|(path, read_class)| **read_class == class && !self.parking.contains(*path))
            .map(|(path, _)| path.as_path())
            .collect()
    }
}

/// A written file or a moved folder that an Agent home reads, or keeps a
/// Disable switch in.
struct Touched<'a> {
    /// The file, or the folder where the Agent home reads it.
    path: &'a PathBuf,
    /// An Entry is parked out of the file or put back into it.
    parking: bool,
    agent_index: usize,
    home: &'a str,
    /// How the file or folder behaves while the Agent runs.
    class: StoreClass,
}

/// Checks the readers of `files` and `folders`, and the Agents with
/// switches in `files`, against the processes running now. An Agent runs
/// for a home of `homes` added outside its lookup order only while one of
/// its processes names the home's folder. Only an Agent running
/// for an installed home of `homes` gets the warning and the hints; any
/// running Agent blocks a write to a SQLite store it holds.
pub(crate) fn check<'a>(
    definitions: &[AgentDefinition],
    ports: &Ports,
    homes: &RunningHomes,
    files: &'a [PlannedFile],
    folders: &'a [PlannedFolder],
) -> RunningCheck {
    let mut listings = Listings::new(ports);
    // Whether the Agent of each home runs, since a home may override its process.
    let mut detected: BTreeMap<(usize, &str), bool> = BTreeMap::new();
    let mut readers: BTreeMap<usize, RunningReader> = BTreeMap::new();
    let located = |reader: &'a LocationReader| {
        let class = reader.location(definitions).store_class;
        (reader.agent_index, reader.home.as_str(), class)
    };
    // Each Agent home that reads a written file, or keeps a Disable switch
    // in it, then each that reads a moved folder.
    let in_files =
        files.iter().flat_map(|file| {
            let switches = file
                .switches
                .iter()
                .map(|store| (store.agent_index, store.home.as_str(), store.store_class));
            file.readers.iter().map(located).chain(switches).map(
                move |(agent_index, home, class)| Touched {
                    path: &file.path,
                    parking: file.parking,
                    agent_index,
                    home,
                    class,
                },
            )
        });
    let in_folders = folders.iter().flat_map(|folder| {
        folder.readers.iter().map(|read| {
            let (agent_index, home, class) = located(&read.reader);
            Touched {
                path: &read.at,
                parking: false,
                agent_index,
                home,
                class,
            }
        })
    });
    for Touched {
        path,
        parking,
        agent_index,
        home,
        class,
    } in in_files.chain(in_folders)
    {
        let key = (agent_index, home.to_owned());
        let definition = &definitions[agent_index];
        let running = *detected.entry((agent_index, home)).or_insert_with(|| {
            definition.process(home).is_some_and(|process| {
                match (
                    homes.added_outside_lookup_order.get(&key),
                    process.data_folder_switch.as_deref(),
                ) {
                    (None, _) => is_running(process, &mut listings),
                    (Some(folder), Some(switch)) => {
                        names_folder(process, switch, &mut listings, folder)
                    }
                    (Some(_), None) => false,
                }
            })
        });
        if !running {
            continue;
        }
        let reader = readers.entry(agent_index).or_insert_with(|| RunningReader {
            agent_name: &definition.agent.name,
            read: BTreeMap::new(),
            parking: BTreeSet::new(),
            installed: false,
        });
        reader.installed |= homes.installed.contains(&key);
        let stored = reader.read.entry(path.clone()).or_insert(class);
        *stored = (*stored).max(class);
        if parking && class == StoreClass::RewrittenWhileRunning {
            reader.parking.insert(path.clone());
        }
    }

    let mut blocks = Vec::new();
    // Each cause of the `running-agent` warning, as its sentence and advice.
    let mut causes = Vec::new();
    let mut hints = Vec::new();
    for reader in readers.values() {
        let sqlite = reader.paths(StoreClass::Sqlite);
        if !sqlite.is_empty() {
            let (sentence, advice) = explain::running_agent_sqlite(reader.agent_name, &sqlite);
            blocks.push(Block {
                code: BlockCode::RunningAgentSqlite,
                sentence,
                advice: vec![advice],
            });
        }
        if !reader.parking.is_empty() {
            let parking: Vec<&Path> = reader.parking.iter().map(PathBuf::as_path).collect();
            let (sentence, advice) = explain::running_agent_parking(reader.agent_name, &parking);
            blocks.push(Block {
                code: BlockCode::RunningAgentParking,
                sentence,
                advice: vec![advice],
            });
        }
        if !reader.installed {
            continue;
        }
        let rewritten = reader.paths(StoreClass::RewrittenWhileRunning);
        if !rewritten.is_empty() {
            causes.push(explain::running_agent_rewrites(
                reader.agent_name,
                &rewritten,
            ));
        }
        let plain = reader.paths(StoreClass::Plain);
        if !plain.is_empty() {
            causes.push(explain::running_agent_reads(reader.agent_name, &plain));
        }
        hints.push(explain::restart_hint(reader.agent_name));
        for path in rewritten {
            hints.push(explain::may_be_overwritten(path, reader.agent_name));
        }
    }
    RunningCheck {
        agents: readers
            .values()
            .filter(|reader| reader.installed)
            .map(|reader| reader.agent_name.to_owned())
            .collect(),
        blocks,
        warning: (!causes.is_empty()).then(|| {
            let (sentences, advice): (Vec<String>, Vec<Advice>) = causes.into_iter().unzip();
            Warning {
                class: WarningClass::RunningAgent,
                sentence: sentences.join(" "),
                advice,
            }
        }),
        hints,
    }
}

/// Whether a process matches `process`, or a session file names a PID that
/// is alive.
fn is_running(process: &ProcessDefinition, listings: &mut Listings) -> bool {
    if listings.any_matching(process, |_| true) {
        return true;
    }
    let ports = listings.ports;
    process.session_files.as_ref().is_some_and(|session_files| {
        let running = listings.first();
        session_files
            .pids(ports.fs.as_ref(), ports.env.as_ref())
            .iter()
            .any(|pid| running.iter().any(|candidate| candidate.pid == *pid))
    })
}

/// The processes running now, listed the first time a check needs them, and
/// listed once more, after a pause, the first time a candidate matches only
/// because its path is hidden.
struct Listings<'p> {
    ports: &'p Ports,
    first: Option<Vec<RunningProcess>>,
    again: Option<Vec<RunningProcess>>,
}

impl<'p> Listings<'p> {
    fn new(ports: &'p Ports) -> Self {
        Listings {
            ports,
            first: None,
            again: None,
        }
    }

    fn first(&mut self) -> &[RunningProcess] {
        self.first
            .get_or_insert_with(|| self.ports.processes.running())
    }

    fn again(&mut self) -> &[RunningProcess] {
        self.again.get_or_insert_with(|| {
            self.ports.clock.pause(PAUSE_BEFORE_LISTING_AGAIN);
            self.ports.processes.running()
        })
    }

    /// Whether a process that matches `process` passes `test`. One that
    /// matches only because its path is hidden is judged by the second
    /// listing: gone, it doesn't count; with its path read, the globs decide;
    /// still without a path, it counts, since the OS may hide it for good.
    fn any_matching(
        &mut self,
        process: &ProcessDefinition,
        test: impl Fn(&RunningProcess) -> bool,
    ) -> bool {
        let ports = self.ports;
        let mut hidden = BTreeSet::new();
        for candidate in self.first() {
            match matches(process, candidate, ports) {
                Match::Yes if test(candidate) => return true,
                Match::PathHidden if test(candidate) => {
                    hidden.insert(candidate.pid);
                }
                _ => {}
            }
        }
        !hidden.is_empty()
            && self.again().iter().any(|candidate| {
                hidden.contains(&candidate.pid)
                    && matches(process, candidate, ports) != Match::No
                    && test(candidate)
            })
    }
}

/// Whether a process that matches `process` names `folder` through
/// `switch`, compared by the canonical key of the folders.
fn names_folder(
    process: &ProcessDefinition,
    switch: &str,
    listings: &mut Listings,
    folder: &Path,
) -> bool {
    let fs = listings.ports.fs.as_ref();
    let key = paths::canonical_key(fs, folder);
    listings.any_matching(process, |candidate| {
        switch_values(&candidate.cmdline, switch)
            .into_iter()
            .any(|named| paths::canonical_key(fs, Path::new(named)) == key)
    })
}

/// The values a command line gives `switch` in the arguments it starts, as
/// `<switch>=<value>`: the value quoted or not, or the whole argument quoted.
fn switch_values<'c>(cmdline: &'c str, switch: &str) -> Vec<&'c str> {
    let prefix = format!("{switch}=");
    let mut values = Vec::new();
    let mut rest = cmdline;
    while let Some(at) = rest.find(&prefix) {
        let before = rest[..at].chars().next_back();
        let after = &rest[at + prefix.len()..];
        rest = after;
        if !before.is_none_or(|c| c.is_whitespace() || c == '"') {
            continue;
        }
        let (value, end) = match after.strip_prefix('"') {
            Some(quoted) => quoted.split_once('"').unwrap_or((quoted, "")),
            None if before == Some('"') => after.split_once('"').unwrap_or((after, "")),
            None => after.split_once(char::is_whitespace).unwrap_or((after, "")),
        };
        rest = end;
        if !value.is_empty() {
            values.push(value);
        }
    }
    values
}

/// How one process matches a process definition.
#[derive(PartialEq, Eq)]
enum Match {
    No,
    /// It has one of the images, a path that matches one of the globs, if
    /// any, and every text in its command line.
    Yes,
    /// It has one of the images and every text in its command line, but the
    /// OS hides its path from the globs: as while it starts or exits, or for
    /// good for some elevated or protected processes.
    PathHidden,
}

/// How one process matches `process`: by its image, its path and every text
/// in its command line.
fn matches(process: &ProcessDefinition, candidate: &RunningProcess, ports: &Ports) -> Match {
    let image = process
        .images
        .iter()
        .any(|image| image.eq_ignore_ascii_case(&candidate.image));
    let cmdline = candidate.cmdline.to_lowercase();
    let cmdline = process
        .cmdline_contains
        .iter()
        .all(|text| cmdline.contains(&text.to_lowercase()));
    if !(image && cmdline) {
        return Match::No;
    }
    if process.paths.is_empty() {
        return Match::Yes;
    }
    match candidate.path.as_deref() {
        None => Match::PathHidden,
        Some(path)
            if process
                .paths
                .iter()
                .any(|glob| paths::glob_matches(glob, path, ports.env.as_ref())) =>
        {
            Match::Yes
        }
        Some(_) => Match::No,
    }
}
