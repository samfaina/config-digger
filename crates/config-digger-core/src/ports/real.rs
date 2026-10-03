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
        std::fs::canonicalize(path)
            .ok()
            .map(without_verbatim_prefix)
    }

    /// Windows counts a junction as a symlink here, both being
    /// name-surrogate reparse points, and `read_link` reads either.
    fn link_target(&self, path: &Path) -> Option<PathBuf> {
        if !std::fs::symlink_metadata(path)
            .ok()?
            .file_type()
            .is_symlink()
        {
            return None;
        }
        let target = without_verbatim_prefix(std::fs::read_link(path).ok()?);
        Some(match path.parent() {
            Some(parent) if target.is_relative() => parent.join(target),
            _ => target,
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

    /// Windows removes a junction or a folder symlink as a folder, which
    /// takes the link alone; elsewhere a symlink is removed as a file.
    fn remove_link(&self, path: &Path) -> io::Result<()> {
        if self.link_target(path).is_none() {
            return Err(io::Error::new(io::ErrorKind::InvalidInput, "not a link"));
        }
        if cfg!(windows) {
            // A file symlink is removed as a file; any other failure is the
            // folder's to report.
            std::fs::remove_dir(path).or_else(|error| std::fs::remove_file(path).map_err(|_| error))
        } else {
            std::fs::remove_file(path)
        }
    }

    /// The standard library creates no junction, and this crate forbids
    /// the unsafe code that would, so `mklink /J` creates it. Each path is
    /// quoted for `cmd`, which then takes `&` or `^` in it literally; one
    /// holding `"` or `%`, which quotes do not protect, is refused.
    #[cfg(windows)]
    fn create_link(&self, link: &Path, target: &Path) -> io::Result<()> {
        if std::fs::symlink_metadata(link).is_ok() {
            return Err(io::ErrorKind::AlreadyExists.into());
        }
        use std::os::windows::process::CommandExt;

        // `mklink` reads a `/` as the start of a switch.
        let quoted = |path: &Path| {
            let text = path.as_os_str().to_string_lossy().replace('/', "\\");
            if text.contains(['"', '%']) {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("cannot create a junction at or to {text}"),
                ));
            }
            Ok(format!("\"{text}\""))
        };
        let output = Command::new("cmd")
            .args(["/D", "/C"])
            .raw_arg(format!("mklink /J {} {}", quoted(link)?, quoted(target)?))
            .stdin(Stdio::null())
            .output()?;
        if output.status.success() && self.link_target(link).is_some() {
            return Ok(());
        }
        Err(io::Error::other(format!(
            "mklink /J failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )))
    }

    #[cfg(not(windows))]
    fn create_link(&self, link: &Path, target: &Path) -> io::Result<()> {
        std::os::unix::fs::symlink(target, link)
    }

    /// A rename. Across volumes, where no rename works, the folder is first
    /// renamed aside on its own volume, under a dot name no Agent reads,
    /// then copied: a copy that fails is removed and the folder renamed
    /// back, so its location is never left half empty. Verify checks the copy.
    /// What is left aside once the copy is complete is removed, and a
    /// failure to remove it is only logged.
    fn move_folder(&self, from: &Path, to: &Path) -> io::Result<()> {
        if std::fs::symlink_metadata(to).is_ok() {
            return Err(io::ErrorKind::AlreadyExists.into());
        }
        match std::fs::rename(from, to) {
            Err(error) if error.kind() == io::ErrorKind::CrossesDevices => {}
            moved => return moved,
        }
        let (Some(folder), Some(name)) = (from.parent(), from.file_name()) else {
            return Err(io::ErrorKind::InvalidInput.into());
        };
        let aside = folder.join(format!(
            ".{}.config-digger-{}.moving",
            name.to_string_lossy(),
            std::process::id()
        ));
        std::fs::rename(from, &aside)?;
        if let Err(error) = copy_folder(&aside, to) {
            let _ = std::fs::remove_dir_all(to);
            std::fs::rename(&aside, from)?;
            return Err(error);
        }
        if let Err(error) = std::fs::remove_dir_all(&aside) {
            tracing::warn!(folder = %aside.display(), %error, "cannot remove a folder moved across volumes");
        }
        Ok(())
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

/// Copies the folder `from` and everything below it to `to`. A link inside
/// it is refused: copying it would copy what it points to, or never end.
fn copy_folder(from: &Path, to: &Path) -> io::Result<()> {
    std::fs::create_dir(to)?;
    for entry in std::fs::read_dir(from)? {
        let entry = entry?;
        let target = to.join(entry.file_name());
        let kind = entry.file_type()?;
        if kind.is_symlink() {
            return Err(io::Error::other(format!(
                "{} is a link, which cannot be moved to another volume",
                entry.path().display()
            )));
        }
        if kind.is_dir() {
            copy_folder(&entry.path(), &target)?;
        } else {
            std::fs::copy(entry.path(), target)?;
        }
    }
    Ok(())
}

/// A path without the `\\?\` or `\??\` prefix Windows answers some calls
/// with, a `\\?\UNC\` share back to `\\`.
fn without_verbatim_prefix(path: PathBuf) -> PathBuf {
    let Some(text) = path.to_str() else {
        return path;
    };
    match text.strip_prefix(r"\\?\UNC\") {
        Some(share) => PathBuf::from(format!(r"\\{share}")),
        None => PathBuf::from(
            text.strip_prefix(r"\\?\")
                .or_else(|| text.strip_prefix(r"\??\"))
                .unwrap_or(text),
        ),
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
