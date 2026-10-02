//! In-memory ports for tests.

use super::{Clock, Environment, FileSystem, GitIndex, Hive, Ports, Processes, RunningProcess};
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};
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

/// Files held in memory. A folder exists when a file exists below it.
#[derive(Debug, Clone, Default)]
pub struct FakeFileSystem {
    files: BTreeMap<PathBuf, Vec<u8>>,
}

impl FakeFileSystem {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_file(mut self, path: impl Into<PathBuf>, contents: impl Into<Vec<u8>>) -> Self {
        self.files.insert(path.into(), contents.into());
        self
    }
}

impl FileSystem for FakeFileSystem {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        match self.files.get(path) {
            Some(contents) => Ok(contents.clone()),
            None if self.exists(path) => Err(io::Error::other("is a folder")),
            None => Err(io::ErrorKind::NotFound.into()),
        }
    }

    fn exists(&self, path: &Path) -> bool {
        self.files.keys().any(|file| file.starts_with(path))
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
