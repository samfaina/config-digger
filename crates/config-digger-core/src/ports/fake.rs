//! In-memory ports for tests.

use super::{Clock, Environment, FileSystem, GitIndex, Hive, Ports, Processes, RunningProcess};
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, SystemTime};

/// Fake ports around `fs` and `env`, with no running processes, nothing
/// versioned and the clock stopped at [`FakeClock::default`].
pub fn ports(fs: FakeFileSystem, env: FakeEnvironment) -> Ports {
    Ports {
        fs: Box::new(fs),
        processes: Box::new(FakeProcesses::default()),
        git: Box::new(FakeGitIndex::default()),
        env: Box::new(env),
        clock: Box::new(FakeClock::default()),
    }
}

/// Files held in memory, looked up case-insensitively as on Windows. A folder
/// exists when it was added or a file exists below it. Clones share one count
/// of [`accesses`](Self::accesses).
#[derive(Debug, Clone, Default)]
pub struct FakeFileSystem {
    /// Keyed by the path as spelled on disk.
    files: BTreeMap<PathBuf, Vec<u8>>,
    folders: BTreeSet<PathBuf>,
    /// Link or junction → target.
    links: Vec<(PathBuf, PathBuf)>,
    accesses: Arc<AtomicUsize>,
}

impl FakeFileSystem {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_file(mut self, path: impl Into<PathBuf>, contents: impl Into<Vec<u8>>) -> Self {
        self.files.insert(path.into(), contents.into());
        self
    }

    /// An empty folder.
    pub fn with_folder(mut self, path: impl Into<PathBuf>) -> Self {
        self.folders.insert(path.into());
        self
    }

    /// A link or junction at `link` that points to the folder `target`.
    pub fn with_link(mut self, link: impl Into<PathBuf>, target: impl Into<PathBuf>) -> Self {
        self.links.push((link.into(), target.into()));
        self
    }

    /// How many times the core has touched this file system, across clones.
    pub fn accesses(&self) -> usize {
        self.accesses.load(Ordering::SeqCst)
    }

    /// `path` with a link at its start replaced by the link's target.
    fn follow(&self, path: &Path) -> PathBuf {
        for (link, target) in &self.links {
            if let Some(rest) = strip_prefix(path, link) {
                return rest
                    .iter()
                    .fold(target.clone(), |path, part| path.join(part));
            }
        }
        path.to_path_buf()
    }

    /// Every file and folder, as spelled on disk.
    fn paths(&self) -> impl Iterator<Item = &PathBuf> {
        self.files.keys().chain(&self.folders)
    }

    fn count_access(&self) {
        self.accesses.fetch_add(1, Ordering::SeqCst);
    }
}

/// The components of `path` after `prefix`, compared case-insensitively.
fn strip_prefix<'a>(path: &'a Path, prefix: &Path) -> Option<Vec<std::path::Component<'a>>> {
    let mut components = path.components();
    for expected in prefix.components() {
        let actual = components.next()?;
        if !actual
            .as_os_str()
            .eq_ignore_ascii_case(expected.as_os_str())
        {
            return None;
        }
    }
    Some(components.collect())
}

impl FileSystem for FakeFileSystem {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        self.count_access();
        let path = self.follow(path);
        let file = self
            .files
            .iter()
            .find(|(file, _)| strip_prefix(file, &path).is_some_and(|rest| rest.is_empty()));
        match file {
            Some((_, contents)) => Ok(contents.clone()),
            None if self.real_path(&path).is_some() => Err(io::Error::other("is a folder")),
            None => Err(io::ErrorKind::NotFound.into()),
        }
    }

    fn exists(&self, path: &Path) -> bool {
        self.real_path(path).is_some()
    }

    fn real_path(&self, path: &Path) -> Option<PathBuf> {
        self.count_access();
        let path = self.follow(path);
        let depth = path.components().count();
        self.paths()
            .find(|known| strip_prefix(known, &path).is_some())
            .map(|known| known.components().take(depth).collect())
    }
}

#[derive(Debug, Clone, Default)]
pub struct FakeProcesses {
    running: Vec<RunningProcess>,
}

impl FakeProcesses {
    pub fn with_process(mut self, process: RunningProcess) -> Self {
        self.running.push(process);
        self
    }
}

impl Processes for FakeProcesses {
    fn running(&self) -> Vec<RunningProcess> {
        self.running.clone()
    }
}

#[derive(Debug, Clone, Default)]
pub struct FakeGitIndex {
    versioned: BTreeSet<PathBuf>,
}

impl FakeGitIndex {
    pub fn with_versioned(mut self, path: impl Into<PathBuf>) -> Self {
        self.versioned.insert(path.into());
        self
    }
}

impl GitIndex for FakeGitIndex {
    fn is_versioned(&self, path: &Path) -> bool {
        self.versioned.contains(path)
    }
}

#[derive(Debug, Clone, Default)]
pub struct FakeEnvironment {
    home: Option<PathBuf>,
    vars: BTreeMap<String, String>,
    registry: BTreeMap<(Hive, String, String), String>,
}

impl FakeEnvironment {
    /// An environment whose `~` is `home`.
    pub fn with_home(home: impl Into<PathBuf>) -> Self {
        Self {
            home: Some(home.into()),
            ..Self::default()
        }
    }

    pub fn with_var(mut self, name: &str, value: &str) -> Self {
        self.vars.insert(name.to_owned(), value.to_owned());
        self
    }

    pub fn with_registry_value(mut self, hive: Hive, key: &str, name: &str, value: &str) -> Self {
        self.registry
            .insert((hive, key.to_owned(), name.to_owned()), value.to_owned());
        self
    }
}

impl Environment for FakeEnvironment {
    fn var(&self, name: &str) -> Option<String> {
        self.vars.get(name).cloned()
    }

    fn home_dir(&self) -> Option<PathBuf> {
        self.home.clone()
    }

    fn registry_value(&self, hive: Hive, key: &str, name: &str) -> Option<String> {
        self.registry
            .get(&(hive, key.to_owned(), name.to_owned()))
            .cloned()
    }
}

/// A clock stopped at one instant.
#[derive(Debug, Clone, Copy)]
pub struct FakeClock(pub SystemTime);

impl Default for FakeClock {
    /// 2026-10-01T12:00:00Z.
    fn default() -> Self {
        Self(SystemTime::UNIX_EPOCH + Duration::from_secs(1_790_856_000))
    }
}

impl Clock for FakeClock {
    fn now(&self) -> SystemTime {
        self.0
    }
}
