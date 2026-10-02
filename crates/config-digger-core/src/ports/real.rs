//! Ports backed by this machine.

use super::{Clock, Environment, FileSystem, GitIndex, Hive, Processes, RunningProcess};
use std::io;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::SystemTime;

pub struct RealFileSystem;

impl FileSystem for RealFileSystem {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        std::fs::read(path)
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }
}

/// Lists processes with `sysinfo`. Detection is best-effort: a process whose
/// executable path the OS hides still appears, without a path.
pub struct RealProcesses;

impl Processes for RealProcesses {
    fn running(&self) -> Vec<RunningProcess> {
        use sysinfo::{ProcessRefreshKind, ProcessesToUpdate, System, UpdateKind};

        let mut system = System::new();
        system.refresh_processes_specifics(
            ProcessesToUpdate::All,
            true,
            ProcessRefreshKind::nothing().with_exe(UpdateKind::OnlyIfNotSet),
        );
        system
            .processes()
            .iter()
            .map(|(pid, process)| RunningProcess {
                pid: pid.as_u32(),
                image: process.name().to_string_lossy().into_owned(),
                path: process.exe().map(Path::to_path_buf),
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
