//! In-memory ports for tests.

use super::{
    Clock, Environment, FileSystem, GitIndex, HeldLock, Hive, Ports, Processes, RunningProcess,
};
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
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
/// exists when it was added or a file exists below it. Clones share the same
/// files, so a test sees what the core wrote, and one count of
/// [`accesses`](Self::accesses).
#[derive(Debug, Clone, Default)]
pub struct FakeFileSystem {
    state: Arc<Mutex<FakeFiles>>,
    accesses: Arc<AtomicUsize>,
}

#[derive(Debug, Default)]
struct FakeFiles {
    /// Keyed by the path as spelled on disk.
    files: BTreeMap<PathBuf, Vec<u8>>,
    folders: BTreeSet<PathBuf>,
    /// Link or junction → target.
    links: Vec<(PathBuf, PathBuf)>,
    /// The lowercased paths of the files locked through this file system.
    locked: BTreeSet<String>,
    /// The lowercased paths of the files someone else holds a lock on.
    locked_elsewhere: BTreeSet<String>,
    /// Folders below which a write stores other bytes than it was given.
    corrupting: Vec<PathBuf>,
    /// Files whose writes fail, each with the number of writes to it that
    /// still succeed first.
    failing: Vec<(PathBuf, usize)>,
    /// Files an Agent rewrites with other bytes right after the core's next
    /// write to them.
    rewrites: Vec<(PathBuf, Vec<u8>)>,
}

impl FakeFileSystem {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_file(self, path: impl Into<PathBuf>, contents: impl Into<Vec<u8>>) -> Self {
        self.put_file(path, contents);
        self
    }

    /// Writes a file as someone other than the core would, such as an Agent.
    pub fn put_file(&self, path: impl Into<PathBuf>, contents: impl Into<Vec<u8>>) {
        self.files().files.insert(path.into(), contents.into());
    }

    /// Deletes a file as someone other than the core would.
    pub fn delete_file(&self, path: impl AsRef<Path>) {
        let mut files = self.files();
        let path = files.follow(path.as_ref());
        files
            .files
            .retain(|file, _| strip_prefix(file, &path).is_none_or(|rest| !rest.is_empty()));
    }

    /// Deletes a folder and everything below it as someone other than the core would.
    pub fn delete_folder(&self, path: impl AsRef<Path>) {
        let mut files = self.files();
        let path = files.follow(path.as_ref());
        files
            .files
            .retain(|file, _| strip_prefix(file, &path).is_none());
        files
            .folders
            .retain(|folder| strip_prefix(folder, &path).is_none());
    }

    /// An empty folder.
    pub fn with_folder(self, path: impl Into<PathBuf>) -> Self {
        self.files().folders.insert(path.into());
        self
    }

    /// A link or junction at `link` that points to the folder `target`.
    pub fn with_link(self, link: impl Into<PathBuf>, target: impl Into<PathBuf>) -> Self {
        self.files().links.push((link.into(), target.into()));
        self
    }

    /// Below `folder`, every write stores bytes other than those it was given.
    pub fn corrupting_writes_below(self, folder: impl Into<PathBuf>) -> Self {
        self.files().corrupting.push(folder.into());
        self
    }

    /// Every write to the file at `path` fails.
    pub fn failing_writes_to(self, path: impl Into<PathBuf>) -> Self {
        self.failing_writes_to_after(path, 0)
    }

    /// The next `successes` writes to the file at `path` succeed, and every
    /// one after them fails.
    pub fn failing_writes_to_after(self, path: impl Into<PathBuf>, successes: usize) -> Self {
        self.files().failing.push((path.into(), successes));
        self
    }

    /// Right after the core's next write to the file at `path`, an Agent
    /// rewrites it with `contents`, as one holding the file in memory would.
    pub fn agent_rewrites_after_next_write(
        self,
        path: impl Into<PathBuf>,
        contents: impl Into<Vec<u8>>,
    ) -> Self {
        self.files().rewrites.push((path.into(), contents.into()));
        self
    }

    /// Writes that failed through [`failing_writes_to`](Self::failing_writes_to) succeed from now on.
    pub fn stop_failing_writes(&self) {
        self.files().failing.clear();
    }

    /// Someone else holds the lock on the file at `path` from now on.
    pub fn lock_elsewhere(&self, path: impl AsRef<Path>) {
        self.files()
            .locked_elsewhere
            .insert(lock_key(path.as_ref()));
    }

    /// The bytes of the file at `path`, `None` when there is none.
    pub fn contents(&self, path: impl AsRef<Path>) -> Option<Vec<u8>> {
        let files = self.files();
        files
            .find_file(&files.follow(path.as_ref()))
            .map(|(_, bytes)| bytes.clone())
    }

    /// The files below `folder`, as spelled on disk.
    pub fn files_below(&self, folder: impl AsRef<Path>) -> Vec<PathBuf> {
        self.files()
            .files
            .keys()
            .filter(|file| strip_prefix(file, folder.as_ref()).is_some_and(|rest| !rest.is_empty()))
            .cloned()
            .collect()
    }

    /// How many times the core has touched this file system, across clones.
    pub fn accesses(&self) -> usize {
        self.accesses.load(Ordering::SeqCst)
    }

    fn files(&self) -> MutexGuard<'_, FakeFiles> {
        self.state
            .lock()
            .expect("no test panics while holding the fake files")
    }

    fn count_access(&self) {
        self.accesses.fetch_add(1, Ordering::SeqCst);
    }
}

impl FakeFiles {
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

    /// The file at a followed path, with its spelling on disk.
    fn find_file(&self, path: &Path) -> Option<(&PathBuf, &Vec<u8>)> {
        self.files
            .iter()
            .find(|(file, _)| strip_prefix(file, path).is_some_and(|rest| rest.is_empty()))
    }

    /// The real path of a followed path.
    fn real_path(&self, path: &Path) -> Option<PathBuf> {
        let depth = path.components().count();
        self.files
            .keys()
            .chain(&self.folders)
            .find(|known| strip_prefix(known, path).is_some())
            .map(|known| known.components().take(depth).collect())
    }

    fn is_below(folders: &[PathBuf], path: &Path) -> bool {
        folders
            .iter()
            .any(|folder| strip_prefix(path, folder).is_some())
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

fn lock_key(path: &Path) -> String {
    path.to_string_lossy().to_lowercase()
}

/// A lock taken through [`FakeFileSystem::try_lock`], released when dropped.
struct FakeLock {
    state: Arc<Mutex<FakeFiles>>,
    key: String,
}

impl Drop for FakeLock {
    fn drop(&mut self) {
        if let Ok(mut files) = self.state.lock() {
            files.locked.remove(&self.key);
        }
    }
}

impl FileSystem for FakeFileSystem {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        self.count_access();
        let files = self.files();
        let path = files.follow(path);
        match files.find_file(&path) {
            Some((_, contents)) => Ok(contents.clone()),
            None if files.real_path(&path).is_some() => Err(io::Error::other("is a folder")),
            None => Err(io::ErrorKind::NotFound.into()),
        }
    }

    fn exists(&self, path: &Path) -> bool {
        self.real_path(path).is_some()
    }

    fn real_path(&self, path: &Path) -> Option<PathBuf> {
        self.count_access();
        let files = self.files();
        files.real_path(&files.follow(path))
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        self.count_access();
        let files = self.files();
        let followed = files.follow(path);
        if files.find_file(&followed).is_some() {
            return Err(io::Error::other("is a file"));
        }
        if files.real_path(&followed).is_none() {
            return Err(io::ErrorKind::NotFound.into());
        }
        let mut children: Vec<PathBuf> = Vec::new();
        for known in files.files.keys().chain(&files.folders) {
            let Some(rest) = strip_prefix(known, &followed) else {
                continue;
            };
            if let Some(name) = rest.first() {
                let child = path.join(name);
                if !children.contains(&child) {
                    children.push(child);
                }
            }
        }
        Ok(children)
    }

    fn remove_dir_all(&self, path: &Path) -> io::Result<()> {
        self.count_access();
        let exists = {
            let files = self.files();
            files.real_path(&files.follow(path)).is_some()
        };
        if !exists {
            return Err(io::ErrorKind::NotFound.into());
        }
        self.delete_folder(path);
        Ok(())
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        self.count_access();
        let exists = {
            let files = self.files();
            files.find_file(&files.follow(path)).is_some()
        };
        if !exists {
            return Err(io::ErrorKind::NotFound.into());
        }
        self.delete_file(path);
        Ok(())
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        self.count_access();
        let mut files = self.files();
        let path = files.follow(path);
        if files.find_file(&path).is_some() {
            return Err(io::ErrorKind::AlreadyExists.into());
        }
        if files.real_path(&path).is_none() {
            files.folders.insert(path);
        }
        Ok(())
    }

    fn write_atomic(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        self.count_access();
        let mut files = self.files();
        let path = files.follow(path);
        let is = |known: &Path| strip_prefix(&path, known).is_some_and(|rest| rest.is_empty());
        if let Some((_, successes)) = files.failing.iter_mut().find(|(failing, _)| is(failing)) {
            if *successes == 0 {
                return Err(io::Error::other("this fake write fails"));
            }
            *successes -= 1;
        }
        let folder = path.parent().unwrap_or(Path::new(""));
        if files.real_path(folder).is_none() {
            return Err(io::ErrorKind::NotFound.into());
        }
        let mut bytes = bytes.to_vec();
        if FakeFiles::is_below(&files.corrupting, &path) {
            bytes.push(b'!');
        }
        if let Some(index) = files
            .rewrites
            .iter()
            .position(|(rewritten, _)| is(rewritten))
        {
            bytes = files.rewrites.remove(index).1;
        }
        let spelling = match files.find_file(&path) {
            Some((spelling, _)) => spelling.clone(),
            None => path,
        };
        files.files.insert(spelling, bytes);
        Ok(())
    }

    fn try_lock(&self, path: &Path) -> io::Result<Option<HeldLock>> {
        self.count_access();
        let mut files = self.files();
        let key = lock_key(&files.follow(path));
        if files.locked_elsewhere.contains(&key) || !files.locked.insert(key.clone()) {
            return Ok(None);
        }
        Ok(Some(Box::new(FakeLock {
            state: Arc::clone(&self.state),
            key,
        })))
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

/// A clock stopped at one instant until a test moves it. Clones share the
/// same instant, so a test can move the clock the core holds.
#[derive(Debug, Clone)]
pub struct FakeClock(Arc<Mutex<SystemTime>>);

impl FakeClock {
    pub fn at(now: SystemTime) -> Self {
        Self(Arc::new(Mutex::new(now)))
    }

    pub fn advance(&self, by: Duration) {
        *self.instant() += by;
    }

    fn instant(&self) -> MutexGuard<'_, SystemTime> {
        self.0
            .lock()
            .expect("no test panics while holding the fake clock")
    }
}

impl Default for FakeClock {
    /// 2026-10-01T12:00:00Z.
    fn default() -> Self {
        Self::at(SystemTime::UNIX_EPOCH + Duration::from_secs(1_790_856_000))
    }
}

impl Clock for FakeClock {
    fn now(&self) -> SystemTime {
        *self.instant()
    }
}
