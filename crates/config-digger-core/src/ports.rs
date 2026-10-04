//! The core's I/O ports. The core ships the real implementations in [`real`];
//! tests swap in the fakes from [`fake`].

use std::io;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

pub mod fake;
pub mod real;

/// A lock held on a file, released when dropped.
pub type HeldLock = Box<dyn Send>;

/// Access to files and folders.
pub trait FileSystem: Send + Sync {
    /// The bytes of a file. A missing file is an error of kind `NotFound`.
    fn read(&self, path: &Path) -> io::Result<Vec<u8>>;

    /// Up to `length` bytes of a file from `offset` on, fewer at its end,
    /// for a large file read in parts, such as an executable's headers.
    fn read_at(&self, path: &Path, offset: u64, length: usize) -> io::Result<Vec<u8>> {
        let bytes = self.read(path)?;
        let start = usize::try_from(offset)
            .unwrap_or(usize::MAX)
            .min(bytes.len());
        let end = start.saturating_add(length).min(bytes.len());
        Ok(bytes[start..end].to_vec())
    }

    /// The value under `key` in the `ItemTable` of the SQLite store at
    /// `path`, as VS Code and Cursor keep their state in `state.vscdb`,
    /// opened read-only for this one query: `None` when the key is not
    /// there. A missing file is an error of kind `NotFound`.
    fn read_sqlite_item(&self, path: &Path, key: &str) -> io::Result<Option<String>>;

    /// Sets the value under `key` in the `ItemTable` of the SQLite store at
    /// `path` to the text `value`, or deletes the row with `None`, in one
    /// transaction, waiting briefly for an Agent that holds a lock. It never
    /// creates a store: a missing file is an error of kind `NotFound`.
    fn write_sqlite_item(&self, path: &Path, key: &str, value: Option<&str>) -> io::Result<()>;

    /// Whether a file or folder exists at `path`.
    fn exists(&self, path: &Path) -> bool;

    /// The real path of an existing file or folder, with every link and
    /// junction resolved and each name spelled as on disk; `None` when nothing
    /// exists at `path`.
    fn real_path(&self, path: &Path) -> Option<PathBuf>;

    /// The target of the link or junction at `path` itself, made absolute,
    /// whether or not it exists; `None` when `path` is no link. Links in
    /// its parent folders are followed and do not count.
    fn link_target(&self, path: &Path) -> Option<PathBuf>;

    /// The paths of the files and folders directly in a folder, links
    /// included, in no particular order. A missing folder is an error of
    /// kind `NotFound`.
    fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>>;

    /// Deletes a folder and everything below it.
    fn remove_dir_all(&self, path: &Path) -> io::Result<()>;

    /// Deletes a file. A missing file is an error of kind `NotFound`.
    fn remove_file(&self, path: &Path) -> io::Result<()>;

    /// Creates a folder and any missing parent folders.
    fn create_dir_all(&self, path: &Path) -> io::Result<()>;

    /// Deletes the link or junction at `path` itself, never what it points
    /// to. Anything else at `path` is an error of kind `InvalidInput`.
    fn remove_link(&self, path: &Path) -> io::Result<()>;

    /// Creates a link to the folder `target` at `link`, where nothing is,
    /// in an existing folder: a junction on Windows, which needs no
    /// privilege, and a symlink elsewhere.
    fn create_link(&self, link: &Path, target: &Path) -> io::Result<()>;

    /// Moves the folder at `from`, and everything below it, to `to`, where
    /// nothing is, in an existing folder.
    fn move_folder(&self, from: &Path, to: &Path) -> io::Result<()>;

    /// Replaces the file at `path` with `bytes` through a temporary file and a
    /// rename, so that readers see either the old file or the new one. A link
    /// is written through, to the file it points to. The folder must exist.
    fn write_atomic(&self, path: &Path, bytes: &[u8]) -> io::Result<()>;

    /// Takes an exclusive OS lock on the file at `path`, creating the file if
    /// needed, without waiting: `None` when someone else holds it.
    fn try_lock(&self, path: &Path) -> io::Result<Option<HeldLock>>;
}

/// A process running on this machine, as far as process detection needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningProcess {
    pub pid: u32,
    /// The image name, such as `claude.exe`.
    pub image: String,
    /// The executable's full path, when the OS lets us read it.
    pub path: Option<PathBuf>,
    /// The command line, its arguments joined by spaces; empty when the OS
    /// hides it.
    pub cmdline: String,
}

/// The processes running on this machine.
pub trait Processes: Send + Sync {
    fn running(&self) -> Vec<RunningProcess>;
}

/// Whether a file or folder is git-versioned: it is in the index of its containing repo.
pub trait GitIndex: Send + Sync {
    fn is_versioned(&self, path: &Path) -> bool;
}

/// A Windows registry hive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Hive {
    CurrentUser,
    LocalMachine,
}

/// What a registry value holds, as far as the core reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegistryData {
    /// A string, `REG_SZ` or `REG_EXPAND_SZ` unexpanded.
    String(String),
    /// A 32-bit number, `REG_DWORD`.
    Dword(u32),
    /// Any other type, such as `REG_MULTI_SZ`, which the core does not read.
    Other,
}

/// The process environment and the Windows registry.
pub trait Environment: Send + Sync {
    /// An environment variable, if it is set and valid Unicode.
    fn var(&self, name: &str) -> Option<String>;

    /// The user's home folder, which `~` expands to.
    fn home_dir(&self) -> Option<PathBuf>;

    /// A string value under `hive\key`, if it exists.
    fn registry_value(&self, hive: Hive, key: &str, name: &str) -> Option<String>;

    /// Every value directly under `hive\key`, by name, in no particular
    /// order; `None` when the key does not exist.
    fn registry_values(&self, hive: Hive, key: &str) -> Option<Vec<(String, RegistryData)>>;
}

/// The current time.
pub trait Clock: Send + Sync {
    fn now(&self) -> SystemTime;
}

/// Every port the core uses.
pub struct Ports {
    pub fs: Box<dyn FileSystem>,
    pub processes: Box<dyn Processes>,
    pub git: Box<dyn GitIndex>,
    pub env: Box<dyn Environment>,
    pub clock: Box<dyn Clock>,
}

impl Ports {
    /// The ports backed by this machine.
    pub fn real() -> Self {
        Self {
            fs: Box::new(real::RealFileSystem),
            processes: Box::new(real::RealProcesses),
            git: Box::new(real::RealGitIndex),
            env: Box::new(real::RealEnvironment),
            clock: Box::new(real::RealClock),
        }
    }
}
