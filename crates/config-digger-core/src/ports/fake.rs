//! In-memory ports for tests.

use super::{
    Clock, Environment, FileSystem, GitIndex, HeldLock, Hive, Ports, Processes, RegistryData,
    RunningProcess,
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

/// What every SQLite file starts with.
const SQLITE_HEADER: &[u8] = b"SQLite format 3\0";

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
    /// The lowercased paths of the files whose reads are denied.
    denied: BTreeSet<String>,
    /// The `ItemTable` of each SQLite store, by the store's path as
    /// spelled on disk.
    sqlite: BTreeMap<PathBuf, BTreeMap<String, String>>,
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

    /// A SQLite store at `path` whose `ItemTable` holds `value` under `key`,
    /// next to what it held already.
    pub fn with_sqlite_item(self, path: impl Into<PathBuf>, key: &str, value: &str) -> Self {
        let path = path.into();
        let mut files = self.files();
        files.files.insert(path.clone(), SQLITE_HEADER.to_vec());
        files
            .sqlite
            .entry(path)
            .or_default()
            .insert(key.to_owned(), value.to_owned());
        drop(files);
        self
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

    /// Whether a link or junction is at `path` itself.
    pub fn is_link(&self, path: impl AsRef<Path>) -> bool {
        self.files().link_at(path.as_ref()).is_some()
    }

    /// Below `folder`, every write stores bytes other than those it was
    /// given, and a folder moved there has one file changed.
    pub fn corrupting_writes_below(self, folder: impl Into<PathBuf>) -> Self {
        self.files().corrupting.push(folder.into());
        self
    }

    /// Every read of the file at `path` is denied, as for a file the user
    /// has no permission to open.
    pub fn denying_reads_of(self, path: impl AsRef<Path>) -> Self {
        let path = path.as_ref().to_string_lossy().to_lowercase();
        self.files().denied.insert(path);
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
    /// `path` with each link at its start replaced by the link's target,
    /// until none is left, as a link to a folder holding a link is followed
    /// through both.
    fn follow(&self, path: &Path) -> PathBuf {
        let mut path = path.to_path_buf();
        // A loop of links would never end.
        for _ in 0..32 {
            let Some(followed) = self.links.iter().find_map(|(link, target)| {
                let rest = strip_prefix(&path, link)?;
                Some(
                    rest.iter()
                        .fold(target.clone(), |path, part| path.join(part)),
                )
            }) else {
                break;
            };
            path = followed;
        }
        path
    }

    /// The index in `links` of the link at `path` itself, its parent
    /// folders followed.
    fn link_at(&self, path: &Path) -> Option<usize> {
        let (parent, name) = (path.parent()?, path.file_name()?);
        let at = self.follow(parent).join(name);
        self.links
            .iter()
            .position(|(link, _)| strip_prefix(&at, link).is_some_and(|rest| rest.is_empty()))
    }

    /// `path` with its parent folders followed and not itself, in a folder
    /// that exists.
    fn unfollowed(&self, path: &Path) -> io::Result<PathBuf> {
        let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
            return Err(io::ErrorKind::InvalidInput.into());
        };
        let parent = self.follow(parent);
        if self.real_path(&parent).is_none() {
            return Err(io::ErrorKind::NotFound.into());
        }
        Ok(parent.join(name))
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
        if files
            .denied
            .contains(&path.to_string_lossy().to_lowercase())
        {
            return Err(io::ErrorKind::PermissionDenied.into());
        }
        match files.find_file(&path) {
            Some((_, contents)) => Ok(contents.clone()),
            None if files.real_path(&path).is_some() => Err(io::Error::other("is a folder")),
            None => Err(io::ErrorKind::NotFound.into()),
        }
    }

    fn read_sqlite_item(&self, path: &Path, key: &str) -> io::Result<Option<String>> {
        self.read(path)?;
        let files = self.files();
        let path = files.follow(path);
        let (_, table) = files
            .sqlite
            .iter()
            .find(|(store, _)| strip_prefix(store, &path).is_some_and(|rest| rest.is_empty()))
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidData, "not a SQLite store"))?;
        Ok(table.get(key).cloned())
    }

    fn exists(&self, path: &Path) -> bool {
        self.real_path(path).is_some()
    }

    fn real_path(&self, path: &Path) -> Option<PathBuf> {
        self.count_access();
        let files = self.files();
        files.real_path(&files.follow(path))
    }

    fn link_target(&self, path: &Path) -> Option<PathBuf> {
        self.count_access();
        let files = self.files();
        files
            .link_at(path)
            .map(|index| files.links[index].1.clone())
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
        let links = files.links.iter().map(|(link, _)| link);
        for known in files.files.keys().chain(&files.folders).chain(links) {
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

    fn remove_link(&self, path: &Path) -> io::Result<()> {
        self.count_access();
        let mut files = self.files();
        match files.link_at(path) {
            Some(index) => {
                files.links.remove(index);
                Ok(())
            }
            None if files.real_path(&files.follow(path)).is_some() => {
                Err(io::ErrorKind::InvalidInput.into())
            }
            None => Err(io::ErrorKind::NotFound.into()),
        }
    }

    fn create_link(&self, link: &Path, target: &Path) -> io::Result<()> {
        self.count_access();
        let mut files = self.files();
        let at = files.unfollowed(link)?;
        if files.link_at(link).is_some() || files.real_path(&at).is_some() {
            return Err(io::ErrorKind::AlreadyExists.into());
        }
        files.links.push((at, target.to_path_buf()));
        Ok(())
    }

    fn move_folder(&self, from: &Path, to: &Path) -> io::Result<()> {
        self.count_access();
        let mut files = self.files();
        if files.link_at(from).is_some() {
            return Err(io::Error::other("this fake moves no link"));
        }
        let from = files.unfollowed(from)?;
        let to = files.unfollowed(to)?;
        if files.find_file(&from).is_some() {
            return Err(io::Error::other("is a file"));
        }
        if files.real_path(&from).is_none() {
            return Err(io::ErrorKind::NotFound.into());
        }
        if files.real_path(&to).is_some()
            || files
                .links
                .iter()
                .any(|(link, _)| strip_prefix(link, &to).is_some_and(|rest| rest.is_empty()))
        {
            return Err(io::ErrorKind::AlreadyExists.into());
        }
        let moved = |path: &Path| {
            strip_prefix(path, &from)
                .map(|rest| rest.iter().fold(to.clone(), |to, part| to.join(part)))
        };
        let mut corrupt = FakeFiles::is_below(&files.corrupting, &to);
        files.files = std::mem::take(&mut files.files)
            .into_iter()
            .map(|(path, mut bytes)| match moved(&path) {
                Some(path) => {
                    if std::mem::take(&mut corrupt) {
                        bytes.push(b'!');
                    }
                    (path, bytes)
                }
                None => (path, bytes),
            })
            .collect();
        files.folders = std::mem::take(&mut files.folders)
            .into_iter()
            .map(|path| moved(&path).unwrap_or(path))
            .collect();
        for (link, _) in &mut files.links {
            if let Some(path) = moved(link) {
                *link = path;
            }
        }
        // The folder it leaves stays, as on a disk.
        if let Some(parent) = from.parent() {
            files.folders.insert(parent.to_path_buf());
        }
        files.folders.insert(to);
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

/// Processes held in memory. Clones share them, so a test can start a
/// process after the core was built.
#[derive(Debug, Clone, Default)]
pub struct FakeProcesses {
    running: Arc<Mutex<Vec<RunningProcess>>>,
}

impl FakeProcesses {
    pub fn with_process(self, process: RunningProcess) -> Self {
        self.start(process);
        self
    }

    /// The process runs from now on.
    pub fn start(&self, process: RunningProcess) {
        self.processes().push(process);
    }

    fn processes(&self) -> MutexGuard<'_, Vec<RunningProcess>> {
        self.running
            .lock()
            .expect("no test panics while holding the fake processes")
    }
}

impl Processes for FakeProcesses {
    fn running(&self) -> Vec<RunningProcess> {
        self.processes().clone()
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
    registry: BTreeMap<(Hive, String, String), RegistryData>,
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
        self.registry.insert(
            (hive, key.to_owned(), name.to_owned()),
            RegistryData::String(value.to_owned()),
        );
        self
    }

    /// A `REG_DWORD` value, such as a policy switch.
    pub fn with_registry_dword(self, hive: Hive, key: &str, name: &str, value: u32) -> Self {
        self.with_registry_data(hive, key, name, RegistryData::Dword(value))
    }

    /// A value of any type.
    pub fn with_registry_data(
        mut self,
        hive: Hive,
        key: &str,
        name: &str,
        data: RegistryData,
    ) -> Self {
        self.registry
            .insert((hive, key.to_owned(), name.to_owned()), data);
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
        match self
            .registry
            .get(&(hive, key.to_owned(), name.to_owned()))?
        {
            RegistryData::String(value) => Some(value.clone()),
            RegistryData::Dword(_) | RegistryData::Other => None,
        }
    }

    fn registry_values(&self, hive: Hive, key: &str) -> Option<Vec<(String, RegistryData)>> {
        let values: Vec<(String, RegistryData)> = self
            .registry
            .iter()
            .filter(|((known, known_key, _), _)| *known == hive && known_key == key)
            .map(|((_, _, name), data)| (name.clone(), data.clone()))
            .collect();
        (!values.is_empty()).then_some(values)
    }
}

/// The bytes of a minimal 64-bit Windows executable whose version resource
/// holds `version` as its product version, or no version at all.
pub fn executable(version: Option<[u16; 4]>) -> Vec<u8> {
    const RSRC_RVA: u32 = 0x1000;
    const RSRC_OFFSET: usize = 0x200;
    let dword = |out: &mut Vec<u8>, value: u32| out.extend_from_slice(&value.to_le_bytes());
    let word = |out: &mut Vec<u8>, value: u16| out.extend_from_slice(&value.to_le_bytes());
    // VS_VERSIONINFO: a header, its key, then VS_FIXEDFILEINFO.
    let mut info = Vec::new();
    word(&mut info, 0);
    word(&mut info, if version.is_some() { 52 } else { 0 });
    word(&mut info, 0);
    for unit in "VS_VERSION_INFO\0".encode_utf16() {
        word(&mut info, unit);
    }
    word(&mut info, 0);
    if let Some([a, b, c, d]) = version {
        let high = (u32::from(a) << 16) | u32::from(b);
        let low = (u32::from(c) << 16) | u32::from(d);
        for value in [0xFEEF_04BD, 0x0001_0000, high, low, high, low] {
            dword(&mut info, value);
        }
        info.resize(info.len() + 7 * 4, 0);
    }
    let length = u16::try_from(info.len()).expect("a small version resource");
    info[..2].copy_from_slice(&length.to_le_bytes());

    // The resource tree: type 16 (RT_VERSION) → id 1 → language 0x409.
    let mut rsrc = Vec::new();
    let directory = |out: &mut Vec<u8>, id: u32, target: u32| {
        out.resize(out.len() + 14, 0);
        word(out, 1);
        dword(out, id);
        dword(out, target);
    };
    directory(&mut rsrc, 16, 0x8000_0000 | 24);
    directory(&mut rsrc, 1, 0x8000_0000 | 48);
    directory(&mut rsrc, 0x409, 72);
    dword(&mut rsrc, RSRC_RVA + 88);
    dword(
        &mut rsrc,
        u32::try_from(info.len()).expect("a small version resource"),
    );
    rsrc.resize(88, 0);
    rsrc.extend_from_slice(&info);
    let rsrc_size = u32::try_from(rsrc.len()).expect("a small resource section");

    let mut exe = vec![0u8; 0x40];
    exe[..2].copy_from_slice(b"MZ");
    exe[0x3C..0x40].copy_from_slice(&0x40u32.to_le_bytes());
    exe.extend_from_slice(b"PE\0\0");
    // COFF header: AMD64, one section, a PE32+ optional header.
    word(&mut exe, 0x8664);
    word(&mut exe, 1);
    exe.resize(exe.len() + 12, 0);
    word(&mut exe, 240);
    word(&mut exe, 0x22);
    let optional = exe.len();
    exe.resize(optional + 240, 0);
    exe[optional..optional + 2].copy_from_slice(&0x20Bu16.to_le_bytes());
    exe[optional + 108..optional + 112].copy_from_slice(&16u32.to_le_bytes());
    exe[optional + 128..optional + 132].copy_from_slice(&RSRC_RVA.to_le_bytes());
    exe[optional + 132..optional + 136].copy_from_slice(&rsrc_size.to_le_bytes());
    exe.extend_from_slice(b".rsrc\0\0\0");
    for value in [rsrc_size, RSRC_RVA, rsrc_size, RSRC_OFFSET as u32] {
        dword(&mut exe, value);
    }
    exe.resize(exe.len() + 16, 0);
    exe.resize(RSRC_OFFSET, 0);
    exe.extend_from_slice(&rsrc);
    exe
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
