//! Ports backed by this machine.

use super::{Clock, Environment, FileSystem, GitIndex, HeldLock, Hive, Processes, RunningProcess};
use std::fs::{File, OpenOptions, TryLockError};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::SystemTime;

pub struct RealFileSystem;

impl FileSystem for RealFileSystem {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        std::fs::read(path)
    }

    fn read_at(&self, path: &Path, offset: u64, length: usize) -> io::Result<Vec<u8>> {
        use std::io::{Read, Seek, SeekFrom};

        let mut file = File::open(path)?;
        file.seek(SeekFrom::Start(offset))?;
        let mut bytes = Vec::new();
        file.take(u64::try_from(length).unwrap_or(u64::MAX))
            .read_to_end(&mut bytes)?;
        Ok(bytes)
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    /// `canonicalize` asks Windows through `GetFinalPathNameByHandle`, which
    /// answers with a verbatim path whose `\\?\` prefix is dropped here.
    fn real_path(&self, path: &Path) -> Option<PathBuf> {
        let real = std::fs::canonicalize(path).ok()?;
        let Some(text) = real.to_str() else {
            return Some(real);
        };
        Some(match text.strip_prefix(r"\\?\UNC\") {
            Some(share) => PathBuf::from(format!(r"\\{share}")),
            None => PathBuf::from(text.strip_prefix(r"\\?\").unwrap_or(text)),
        })
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        std::fs::read_dir(path)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect()
    }

    fn remove_dir_all(&self, path: &Path) -> io::Result<()> {
        std::fs::remove_dir_all(path)
    }

    fn remove_file(&self, path: &Path) -> io::Result<()> {
        std::fs::remove_file(path)
    }

    fn create_dir_all(&self, path: &Path) -> io::Result<()> {
        std::fs::create_dir_all(path)
    }

    /// The temporary file sits next to the target, so that the rename stays
    /// on one volume, and is removed when anything fails.
    fn write_atomic(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        let target = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let (Some(folder), Some(name)) = (target.parent(), target.file_name()) else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not a file path",
            ));
        };
        let mut temp_name = name.to_os_string();
        temp_name.push(format!(".config-digger-{}.tmp", std::process::id()));
        let temp = folder.join(temp_name);
        let written = File::create(&temp)
            .and_then(|mut file| {
                file.write_all(bytes)?;
                file.sync_all()
            })
            .and_then(|()| std::fs::rename(&temp, &target));
        if written.is_err() {
            let _ = std::fs::remove_file(&temp);
        }
        written
    }

    /// Closing the file releases the lock, so the held lock is the open file.
    fn try_lock(&self, path: &Path) -> io::Result<Option<HeldLock>> {
        let file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(path)?;
        match file.try_lock() {
            Ok(()) => Ok(Some(Box::new(file))),
            Err(TryLockError::WouldBlock) => Ok(None),
            Err(TryLockError::Error(error)) => Err(error),
        }
    }
}

/// Lists processes with `sysinfo`, refreshing processes only. Detection is
/// best-effort: a process whose executable path or command line the OS hides
/// still appears, without them.
pub struct RealProcesses;

impl Processes for RealProcesses {
    fn running(&self) -> Vec<RunningProcess> {
        use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

        let mut system = System::new();
        system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing()
                .with_exe(UpdateKind::OnlyIfNotSet)
                .with_cmd(UpdateKind::OnlyIfNotSet),
        );
        system
            .processes()
            .iter()
            .map(|(pid, process)| RunningProcess {
                pid: pid.as_u32(),
                image: process.name().to_string_lossy().into_owned(),
                path: process.exe().map(Path::to_path_buf),
                cmdline: process
                    .cmd()
                    .iter()
                    .map(|arg| arg.to_string_lossy())
                    .collect::<Vec<_>>()
                    .join(" "),
            })
            .collect()
    }
}

/// Asks `git` whether the path is in the index of its containing repo. Any
/// failure to ask, including git not being installed, counts as not versioned.
pub struct RealGitIndex;

impl GitIndex for RealGitIndex {
    fn is_versioned(&self, path: &Path) -> bool {
        let Some(parent) = path.parent() else {
            return false;
        };
        Command::new("git")
            .arg("-C")
            .arg(parent)
            .args(["ls-files", "--error-unmatch", "--"])
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|status| status.success())
    }
}

pub struct RealEnvironment;

impl Environment for RealEnvironment {
    fn var(&self, name: &str) -> Option<String> {
        std::env::var(name).ok()
    }

    fn home_dir(&self) -> Option<PathBuf> {
        let name = if cfg!(windows) { "USERPROFILE" } else { "HOME" };
        std::env::var_os(name)
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
    }

    #[cfg(windows)]
    fn registry_value(&self, hive: Hive, key: &str, name: &str) -> Option<String> {
        use winreg::RegKey;
        use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

        let root = RegKey::predef(match hive {
            Hive::CurrentUser => HKEY_CURRENT_USER,
            Hive::LocalMachine => HKEY_LOCAL_MACHINE,
        });
        root.open_subkey(key).ok()?.get_value(name).ok()
    }

    #[cfg(not(windows))]
    fn registry_value(&self, _hive: Hive, _key: &str, _name: &str) -> Option<String> {
        None
    }
}

pub struct RealClock;

impl Clock for RealClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}
