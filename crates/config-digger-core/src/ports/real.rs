//! Ports backed by this machine.

use super::{
    Clock, Environment, FileSystem, GitIndex, HeldLock, Hive, Processes, RegistryData,
    RunningProcess, WrittenLink,
};
use crate::api::LinkKind;
use std::collections::BTreeSet;
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

    /// Opened read-only, waiting briefly for an Agent that holds a write
    /// lock, and closed when the query is done.
    fn read_sqlite_item(&self, path: &Path, key: &str) -> io::Result<Option<String>> {
        use rusqlite::types::ValueRef;
        use rusqlite::{Connection, OpenFlags, OptionalExtension};

        if !path.is_file() {
            return Err(io::ErrorKind::NotFound.into());
        }
        let sqlite = |error: rusqlite::Error| io::Error::other(error);
        let connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(sqlite)?;
        connection
            .busy_timeout(std::time::Duration::from_millis(250))
            .map_err(sqlite)?;
        connection
            .query_row("SELECT value FROM ItemTable WHERE key = ?1", [key], |row| {
                Ok(match row.get_ref(0)? {
                    ValueRef::Text(bytes) | ValueRef::Blob(bytes) => {
                        Some(String::from_utf8_lossy(bytes).into_owned())
                    }
                    _ => None,
                })
            })
            .optional()
            .map(Option::flatten)
            .map_err(sqlite)
    }

    /// Opened read-only, waiting briefly for an Agent that holds a write
    /// lock, and closed when the query is done. The table and column are
    /// quoted as identifiers.
    fn read_sqlite_column(
        &self,
        path: &Path,
        table: &str,
        column: &str,
    ) -> io::Result<Vec<String>> {
        use rusqlite::types::ValueRef;
        use rusqlite::{Connection, OpenFlags};

        if !path.is_file() {
            return Err(io::ErrorKind::NotFound.into());
        }
        let sqlite = |error: rusqlite::Error| io::Error::other(error);
        let connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(sqlite)?;
        connection
            .busy_timeout(std::time::Duration::from_millis(250))
            .map_err(sqlite)?;
        let quoted = |name: &str| format!("\"{}\"", name.replace('"', "\"\""));
        let mut statement = connection
            .prepare(&format!("SELECT {} FROM {}", quoted(column), quoted(table)))
            .map_err(sqlite)?;
        let rows = statement
            .query_map([], |row| {
                Ok(match row.get_ref(0)? {
                    ValueRef::Text(bytes) => Some(String::from_utf8_lossy(bytes).into_owned()),
                    _ => None,
                })
            })
            .map_err(sqlite)?;
        let mut values = Vec::new();
        for value in rows {
            values.extend(value.map_err(sqlite)?);
        }
        Ok(values)
    }

    /// Opened read-write without creating the file, waiting briefly for an
    /// Agent that holds a lock, and closed when the write is done.
    fn write_sqlite_item(&self, path: &Path, key: &str, value: Option<&str>) -> io::Result<()> {
        use rusqlite::{Connection, OpenFlags};

        if !path.is_file() {
            return Err(io::ErrorKind::NotFound.into());
        }
        let sqlite = |error: rusqlite::Error| io::Error::other(error);
        let connection = Connection::open_with_flags(
            path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(sqlite)?;
        connection
            .busy_timeout(std::time::Duration::from_millis(250))
            .map_err(sqlite)?;
        match value {
            Some(value) => connection.execute(
                "INSERT OR REPLACE INTO ItemTable (key, value) VALUES (?1, ?2)",
                [key, value],
            ),
            None => connection.execute("DELETE FROM ItemTable WHERE key = ?1", [key]),
        }
        .map_err(sqlite)?;
        connection.close().map_err(|(_, error)| sqlite(error))
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    /// Asks `Get-Acl` about each file and each folder above it, as Gemini
    /// CLI does, the paths of the links resolved too: the crate forbids
    /// the unsafe code that would read the permissions itself. One
    /// PowerShell answers for every file. The paths reach it through the
    /// environment, never in the script.
    #[cfg(windows)]
    fn admin_owned(&self, paths: &[&Path]) -> BTreeSet<PathBuf> {
        const SCRIPT: &str = r#"
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$trusted = { param($sid) $sid -eq 'S-1-5-32-544' -or $sid -eq 'S-1-5-18' -or $sid -like 'S-1-5-80-*' -or $sid -eq 'S-1-3-0' -or $sid -eq 'S-1-3-1' }
foreach ($path in ($env:CONFIG_DIGGER_ACL_PATHS -split "`n")) {
  try {
    $acl = Get-Acl -LiteralPath $path
    if (-not (& $trusted $acl.GetOwner([System.Security.Principal.SecurityIdentifier]).Value)) { 'UNSAFE'; continue }
    $writers = $acl.Access | Where-Object {
      $_.AccessControlType -eq 'Allow' -and
      $_.FileSystemRights -match 'Write|Modify|FullControl|CreateFiles|AppendData|CreateDirectories|Delete|TakeOwnership|ChangePermissions' -and
      -not (& $trusted $_.IdentityReference.Translate([System.Security.Principal.SecurityIdentifier]).Value)
    }
    if ($writers) { 'UNSAFE' } else { 'SAFE' }
  } catch { 'UNSAFE' }
}
"#;
        // Each file with the indexes in `acl_paths` of the paths whose
        // permissions it depends on, each path asked about once.
        let mut acl_paths: Vec<String> = Vec::new();
        let mut chains: Vec<(&Path, Vec<usize>)> = Vec::new();
        for &path in paths {
            let real = self.real_path(path);
            let mut chain = Vec::new();
            for found in acl_chain(path)
                .into_iter()
                .chain(real.iter().flat_map(|real| acl_chain(real)))
            {
                let found = found.display().to_string();
                let known = acl_paths
                    .iter()
                    .position(|known| known.eq_ignore_ascii_case(&found));
                chain.push(known.unwrap_or_else(|| {
                    acl_paths.push(found);
                    acl_paths.len() - 1
                }));
            }
            chains.push((path, chain));
        }
        if acl_paths.is_empty() {
            return BTreeSet::new();
        }
        let output = powershell(SCRIPT)
            .env("CONFIG_DIGGER_ACL_PATHS", acl_paths.join("\n"))
            .stderr(Stdio::null())
            .output();
        let Ok(output) = output else {
            return BTreeSet::new();
        };
        // PowerShell can write progress records there too, as `#< CLIXML`.
        let safe: Vec<bool> = String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::trim)
            .filter(|line| *line == "SAFE" || *line == "UNSAFE")
            .map(|line| line == "SAFE")
            .collect();
        if safe.len() != acl_paths.len() {
            return BTreeSet::new();
        }
        chains
            .into_iter()
            .filter(|(_, chain)| chain.iter().all(|&index| safe[index]))
            .map(|(path, _)| path.to_path_buf())
            .collect()
    }

    /// Gemini CLI wants the file and each folder above it to belong to root
    /// and be writable by no one else.
    #[cfg(unix)]
    fn admin_owned(&self, paths: &[&Path]) -> BTreeSet<PathBuf> {
        use std::os::unix::fs::MetadataExt;

        paths
            .iter()
            .filter(|path| {
                path.ancestors()
                    .filter(|path| !path.as_os_str().is_empty())
                    .all(|path| {
                        std::fs::metadata(path)
                            .is_ok_and(|found| found.uid() == 0 && found.mode() & 0o022 == 0)
                    })
            })
            .map(|path| path.to_path_buf())
            .collect()
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

    /// The standard library cannot tell a junction from a folder symlink
    /// with an absolute target, so the `junction` crate reads a junction,
    /// and the standard library what is left.
    #[cfg(windows)]
    fn written_link(&self, path: &Path) -> Option<WrittenLink> {
        use std::os::windows::fs::FileTypeExt;

        let file_type = std::fs::symlink_metadata(path).ok()?.file_type();
        if !file_type.is_symlink() {
            return None;
        }
        if let Ok(target) = junction::get_target(path) {
            return Some(WrittenLink {
                kind: LinkKind::Junction,
                target: without_verbatim_prefix(target),
            });
        }
        let kind = if file_type.is_symlink_dir() {
            LinkKind::FolderSymlink
        } else {
            LinkKind::FileSymlink
        };
        Some(WrittenLink {
            kind,
            target: without_verbatim_prefix(std::fs::read_link(path).ok()?),
        })
    }

    /// A symlink here is a folder symlink when it leads to a folder.
    #[cfg(not(windows))]
    fn written_link(&self, path: &Path) -> Option<WrittenLink> {
        if !std::fs::symlink_metadata(path)
            .ok()?
            .file_type()
            .is_symlink()
        {
            return None;
        }
        let kind = if std::fs::metadata(path).is_ok_and(|metadata| metadata.is_dir()) {
            LinkKind::FolderSymlink
        } else {
            LinkKind::FileSymlink
        };
        Some(WrittenLink {
            kind,
            target: std::fs::read_link(path).ok()?,
        })
    }

    /// The standard library creates no junction, and this crate forbids
    /// the unsafe code that would, so the `junction` crate creates it.
    #[cfg(windows)]
    fn create_link(&self, link: &Path, kind: LinkKind, target: &Path) -> io::Result<()> {
        if std::fs::symlink_metadata(link).is_ok() {
            return Err(io::ErrorKind::AlreadyExists.into());
        }
        match kind {
            LinkKind::Junction => {
                // A junction's target is absolute: the crate would resolve
                // a relative one against the current folder.
                let target = match link.parent() {
                    Some(parent) if target.is_relative() => parent.join(target),
                    _ => target.to_path_buf(),
                };
                junction::create(target, link)
            }
            LinkKind::FolderSymlink => std::os::windows::fs::symlink_dir(target, link),
            LinkKind::FileSymlink => std::os::windows::fs::symlink_file(target, link),
        }
    }

    #[cfg(not(windows))]
    fn create_link(&self, link: &Path, _: LinkKind, target: &Path) -> io::Result<()> {
        std::os::unix::fs::symlink(target, link)
    }

    /// Asked once per session: what lets this process create a symlink
    /// does not change while it runs. When the data folder does not exist
    /// yet, the probe goes in the system's temporary folder rather than
    /// creating it.
    fn can_create_symlinks(&self, folder: &Path) -> bool {
        static CAN: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *CAN.get_or_init(|| {
            let folder = if folder.is_dir() {
                folder.to_path_buf()
            } else {
                std::env::temp_dir()
            };
            let probe = folder.join(format!(".symlink-probe-{}", std::process::id()));
            let created = self
                .create_link(&probe, LinkKind::FolderSymlink, Path::new("."))
                .is_ok();
            if created && let Err(error) = self.remove_link(&probe) {
                tracing::warn!(probe = %probe.display(), %error, "cannot delete the symlink probe");
            }
            tracing::info!(created, "probed whether symlinks can be created");
            created
        })
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

/// The file at `path` and each folder above it but the drive's root, whose
/// permissions Gemini CLI leaves to whoever owns the drive.
#[cfg(windows)]
fn acl_chain(path: &Path) -> Vec<PathBuf> {
    path.ancestors()
        .filter(|path| path.parent().is_some() && !path.as_os_str().is_empty())
        .map(Path::to_path_buf)
        .collect()
}

/// `script` as PowerShell's `-EncodedCommand` takes it: its UTF-16LE bytes
/// in base64.
#[cfg(windows)]
fn encoded_command(script: &str) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes: Vec<u8> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    let mut encoded = String::new();
    for chunk in bytes.chunks(3) {
        let group = chunk.iter().enumerate().fold(0u32, |group, (at, &byte)| {
            group | u32::from(byte) << (16 - 8 * at)
        });
        for at in 0..4 {
            if at <= chunk.len() {
                let index = (group >> (18 - 6 * at)) & 0x3f;
                encoded.push(char::from(ALPHABET[index as usize]));
            } else {
                encoded.push('=');
            }
        }
    }
    encoded
}

/// A command for `program`, which runs in the background. On Windows it
/// gets a console with no window: the app has no console to share with it,
/// so a console program would otherwise open a window until it exits.
#[allow(clippy::disallowed_methods)]
fn background_command(program: &str) -> Command {
    let mut command = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    command
}

/// Windows PowerShell, in the background, to run `script` with nothing on
/// its stdin.
#[cfg(windows)]
fn powershell(script: &str) -> Command {
    let mut command = background_command("powershell.exe");
    command
        .args(["-NoProfile", "-NonInteractive", "-EncodedCommand"])
        .arg(encoded_command(script))
        // The one PowerShell 7 sets keeps Windows PowerShell from loading `Get-Acl`.
        .env_remove("PSModulePath")
        .stdin(Stdio::null());
    command
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

/// Asks `git` which paths are in the index of their containing repo. Any
/// failure to ask, including git not being installed, counts as not versioned.
pub struct RealGitIndex;

impl GitIndex for RealGitIndex {
    /// The folder at or above `folder` that holds `.git`, a folder or a
    /// linked worktree's file, as git looks for it from there.
    fn repository(&self, folder: &Path) -> Option<PathBuf> {
        folder
            .ancestors()
            .filter(|parent| !parent.as_os_str().is_empty())
            .find(|parent| parent.join(".git").exists())
            .map(Path::to_path_buf)
    }

    /// Lists the whole index once with `git ls-files`, whose paths are
    /// relative to the folder it runs in and spelled as git keeps them.
    fn versioned_in(&self, repository: &Path, paths: &[&Path]) -> BTreeSet<PathBuf> {
        let output = background_command("git")
            .arg("-C")
            .arg(repository)
            .args(["ls-files", "-z"])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output();
        let Ok(output) = output else {
            return BTreeSet::new();
        };
        if !output.status.success() {
            return BTreeSet::new();
        }
        let index: BTreeSet<&[u8]> = output
            .stdout
            .split(|&byte| byte == 0)
            .filter(|file| !file.is_empty())
            .collect();
        paths
            .iter()
            .filter(|path| {
                let Some(relative) = path.strip_prefix(repository).ok().and_then(index_spelling)
                else {
                    return false;
                };
                // A folder is versioned when a file below it is.
                let folder = format!("{relative}/");
                index.contains(relative.as_bytes())
                    || index
                        .range(folder.as_bytes()..)
                        .next()
                        .is_some_and(|file| file.starts_with(folder.as_bytes()))
            })
            .map(|path| path.to_path_buf())
            .collect()
    }
}

/// `relative` as the index spells a path: its names joined by `/`. `None`
/// for an empty path or one that is not Unicode.
fn index_spelling(relative: &Path) -> Option<String> {
    let names: Option<Vec<&str>> = relative
        .components()
        .map(|component| component.as_os_str().to_str())
        .collect();
    let names = names?;
    (!names.is_empty()).then(|| names.join("/"))
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

    #[cfg(windows)]
    fn registry_values(&self, hive: Hive, key: &str) -> Option<Vec<(String, RegistryData)>> {
        use winreg::RegKey;
        use winreg::enums::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE, RegType};
        use winreg::types::FromRegValue;

        let root = RegKey::predef(match hive {
            Hive::CurrentUser => HKEY_CURRENT_USER,
            Hive::LocalMachine => HKEY_LOCAL_MACHINE,
        });
        let key = root.open_subkey(key).ok()?;
        Some(
            key.enum_values()
                .filter_map(Result::ok)
                .map(|(name, value)| {
                    let data = match value.vtype {
                        RegType::REG_SZ | RegType::REG_EXPAND_SZ => String::from_reg_value(&value)
                            .map_or(RegistryData::Other, RegistryData::String),
                        RegType::REG_DWORD => u32::from_reg_value(&value)
                            .map_or(RegistryData::Other, RegistryData::Dword),
                        _ => RegistryData::Other,
                    };
                    (name, data)
                })
                .collect(),
        )
    }

    #[cfg(not(windows))]
    fn registry_values(&self, _hive: Hive, _key: &str) -> Option<Vec<(String, RegistryData)>> {
        None
    }
}

pub struct RealClock;

impl Clock for RealClock {
    fn now(&self) -> SystemTime {
        SystemTime::now()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[cfg(windows)]
    #[test]
    fn a_background_process_has_no_console_window() {
        const SCRIPT: &str = r#"
Add-Type -Namespace ConfigDigger -Name Console -MemberDefinition @'
[DllImport("kernel32.dll")] public static extern System.IntPtr GetConsoleWindow();
[DllImport("kernel32.dll")] public static extern uint GetConsoleProcessList(uint[] list, uint count);
'@
$attached = [ConfigDigger.Console]::GetConsoleProcessList((New-Object UInt32[] 64), 64)
$window = [ConfigDigger.Console]::GetConsoleWindow() -ne [System.IntPtr]::Zero
"attached=$attached window=$window"
"#;
        let output = powershell(SCRIPT).output().expect("PowerShell runs");

        // Alone on its console, so it shares none, a terminal's included.
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            "attached=1 window=False"
        );
    }
}
