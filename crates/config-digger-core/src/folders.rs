//! The skill folders and links an Operation moves, removes or puts back,
//! rather than edits: what a plan holds of each, how a folder is measured
//! for Verify, and the file system as it will be once they are done, which
//! a plan scans to predict the States after.

use crate::api::{Error, FolderChange, FolderChangeKind};
use crate::ports::{FileSystem, HeldLock};
use crate::{explain, paths, skill};
use serde::{Deserialize, Serialize};
use std::ffi::OsString;
use std::io;
use std::path::{Path, PathBuf};

/// How many files a folder holds, at any depth, and their total size in
/// bytes. A folder moved whole keeps both, which is what Verify checks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct FolderSize {
    pub files: u64,
    pub bytes: u64,
}

/// The size of the folder at `folder`.
pub(crate) fn measure(fs: &dyn FileSystem, folder: &Path) -> Result<FolderSize, Error> {
    let files = skill::files(fs, folder)?;
    Ok(FolderSize {
        files: files.len() as u64,
        bytes: files.values().map(|bytes| bytes.len() as u64).sum(),
    })
}

/// The size of a link's target, `real` once resolved: nothing when the
/// target is gone.
pub(crate) fn measure_target(
    fs: &dyn FileSystem,
    real: Option<&Path>,
) -> Result<FolderSize, Error> {
    match real {
        Some(real) => measure(fs, real),
        None => Ok(FolderSize { files: 0, bytes: 0 }),
    }
}

/// What an Operation does at one skill folder's location.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum FolderOp {
    /// Moves the folder into the Operation's backup.
    MoveToBackup,
    /// Removes the link there, which points to `target`, and never `target`.
    RemoveLink { target: PathBuf },
    /// Moves the folder back from `from`, its copy in an earlier
    /// Operation's backup.
    MoveBack { from: PathBuf },
    /// Creates the link to `target` there again.
    CreateLink { target: PathBuf },
    /// Moves the folder into config-digger's parked-Entry store, at `to`.
    Park { to: PathBuf },
    /// Moves the folder back from `from`, where the parked-Entry store
    /// keeps it.
    Unpark { from: PathBuf },
}

/// One skill folder's location an Operation touches.
pub(crate) struct PlannedFolder {
    pub path: PathBuf,
    pub op: FolderOp,
    /// The size of the folder it moves, or of the link's target: what
    /// Verify expects to find afterwards.
    pub size: FolderSize,
    /// The folder is in the index of its git repo.
    pub git_versioned: bool,
}

impl PlannedFolder {
    /// The folder's row in a preview.
    pub fn change(&self) -> FolderChange {
        let (change, target) = match &self.op {
            FolderOp::MoveToBackup => (FolderChangeKind::MovedToBackup, None),
            FolderOp::RemoveLink { target } => (FolderChangeKind::LinkRemoved, Some(target)),
            FolderOp::MoveBack { .. } => (FolderChangeKind::MovedBack, None),
            FolderOp::CreateLink { target } => (FolderChangeKind::LinkCreated, Some(target)),
            FolderOp::Park { .. } => (FolderChangeKind::Parked, None),
            FolderOp::Unpark { .. } => (FolderChangeKind::PutBack, None),
        };
        FolderChange {
            sentence: explain::folder_change(change, &self.path, target, self.size),
            path: self.path.clone(),
            change,
            target: target.cloned(),
            files: self.size.files,
            size: self.size.bytes,
            git_versioned: self.git_versioned,
        }
    }
}

/// The file system as it will be once `folders` are done, for reading
/// only: a folder moved away or a link removed is gone, along with what
/// is reached through it; a folder moved back holds its copy, which is
/// gone from the backup or the parked-Entry store; a link created points to
/// its target.
pub(crate) struct AsIf<'f> {
    fs: &'f dyn FileSystem,
    /// Paths with nothing at them or below.
    gone: Vec<PathBuf>,
    /// Paths that show another path: a folder moved back, or a link created.
    shown: Vec<Shown>,
}

/// A path of an [`AsIf`] view that shows another path.
struct Shown {
    at: PathBuf,
    /// The copy moved back, or the link's target.
    from: PathBuf,
    link: bool,
}

/// Where a path of an [`AsIf`] view is read.
enum Seen {
    Gone,
    At(PathBuf),
}

impl<'f> AsIf<'f> {
    pub fn new(fs: &'f dyn FileSystem, folders: &[PlannedFolder]) -> Self {
        let mut gone = Vec::new();
        let mut shown = Vec::new();
        for folder in folders {
            match &folder.op {
                FolderOp::MoveToBackup | FolderOp::RemoveLink { .. } | FolderOp::Park { .. } => {
                    gone.push(folder.path.clone());
                }
                FolderOp::MoveBack { from } | FolderOp::Unpark { from } => {
                    gone.push(from.clone());
                    shown.push(Shown {
                        at: folder.path.clone(),
                        from: from.clone(),
                        link: false,
                    });
                }
                FolderOp::CreateLink { target } => {
                    shown.push(Shown {
                        at: folder.path.clone(),
                        from: target.clone(),
                        link: true,
                    });
                }
            }
        }
        Self { fs, gone, shown }
    }

    fn seen(&self, path: &Path) -> Seen {
        for shown in &self.shown {
            if let Some(rest) = below(path, &shown.at) {
                let from = shown.from.clone();
                return Seen::At(rest.iter().fold(from, |path, part| path.join(part)));
            }
        }
        let gone = |path: &Path| self.gone.iter().any(|gone| below(path, gone).is_some());
        // A path reached through a link into a folder that is gone is gone.
        if gone(path) || self.fs.real_path(path).is_some_and(|real| gone(&real)) {
            return Seen::Gone;
        }
        Seen::At(path.to_path_buf())
    }
}

/// The names of `path` below `base`, compared case-insensitively as on
/// Windows; `None` when it is not at or below it.
pub(crate) fn below(path: &Path, base: &Path) -> Option<Vec<OsString>> {
    let mut parts = path.components();
    for expected in base.components() {
        let part = parts.next()?;
        if !part.as_os_str().eq_ignore_ascii_case(expected.as_os_str()) {
            return None;
        }
    }
    Some(parts.map(|part| part.as_os_str().to_owned()).collect())
}

/// Whether `path` is `other`, compared as [`below`] compares.
pub(crate) fn is_at(path: &Path, other: &Path) -> bool {
    below(path, other).is_some_and(|rest| rest.is_empty())
}

/// Whether anything, a link included, is at `path`.
pub(crate) fn taken(fs: &dyn FileSystem, path: &Path) -> bool {
    fs.exists(path) || fs.link_target(path).is_some()
}

/// Whether the link at `path` points to `target`.
pub(crate) fn links_to(fs: &dyn FileSystem, path: &Path, target: &Path) -> bool {
    fs.link_target(path)
        .is_some_and(|now| is_at(&paths::lexical(&now), target))
}

/// Whether the folder at `path`, which is no link, has `size`.
pub(crate) fn holds(fs: &dyn FileSystem, path: &Path, size: FolderSize) -> bool {
    fs.link_target(path).is_none() && measure(fs, path).is_ok_and(|now| now == size)
}

fn read_only() -> io::Error {
    io::Error::other("a view of the file system as it will be is read-only")
}

impl FileSystem for AsIf<'_> {
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        match self.seen(path) {
            Seen::Gone => Err(io::ErrorKind::NotFound.into()),
            Seen::At(at) => self.fs.read(&at),
        }
    }

    fn read_sqlite_item(&self, path: &Path, key: &str) -> io::Result<Option<String>> {
        match self.seen(path) {
            Seen::Gone => Err(io::ErrorKind::NotFound.into()),
            Seen::At(at) => self.fs.read_sqlite_item(&at, key),
        }
    }

    fn read_sqlite_column(
        &self,
        path: &Path,
        table: &str,
        column: &str,
    ) -> io::Result<Vec<String>> {
        match self.seen(path) {
            Seen::Gone => Err(io::ErrorKind::NotFound.into()),
            Seen::At(at) => self.fs.read_sqlite_column(&at, table, column),
        }
    }

    fn exists(&self, path: &Path) -> bool {
        match self.seen(path) {
            Seen::Gone => false,
            Seen::At(at) => self.fs.exists(&at),
        }
    }

    fn admin_owned(&self, path: &Path) -> bool {
        match self.seen(path) {
            Seen::Gone => false,
            Seen::At(at) => self.fs.admin_owned(&at),
        }
    }

    fn real_path(&self, path: &Path) -> Option<PathBuf> {
        let moved_back = self
            .shown
            .iter()
            .any(|shown| !shown.link && below(path, &shown.at).is_some());
        match self.seen(path) {
            Seen::Gone => None,
            // A folder moved back is real where it lands.
            Seen::At(at) if moved_back => self.fs.exists(&at).then(|| paths::lexical(path)),
            Seen::At(at) => self.fs.real_path(&at),
        }
    }

    fn link_target(&self, path: &Path) -> Option<PathBuf> {
        let created = self
            .shown
            .iter()
            .find(|shown| shown.link && is_at(path, &shown.at));
        if let Some(shown) = created {
            return Some(shown.from.clone());
        }
        if self.gone.iter().any(|gone| is_at(path, gone)) {
            return None;
        }
        self.fs.link_target(path)
    }

    fn read_dir(&self, path: &Path) -> io::Result<Vec<PathBuf>> {
        let Seen::At(at) = self.seen(path) else {
            return Err(io::ErrorKind::NotFound.into());
        };
        let in_path = |shown: &Shown| shown.at.parent().is_some_and(|parent| is_at(parent, path));
        let found = match self.fs.read_dir(&at) {
            // A folder moved back creates the folders it goes in.
            Err(error)
                if error.kind() == io::ErrorKind::NotFound && self.shown.iter().any(in_path) =>
            {
                Vec::new()
            }
            found => found?,
        };
        let mut children: Vec<PathBuf> = found
            .into_iter()
            .filter_map(|child| Some(path.join(child.file_name()?)))
            .filter(|child| !self.gone.iter().any(|gone| is_at(child, gone)))
            .collect();
        for shown in &self.shown {
            if in_path(shown) && !children.iter().any(|child| is_at(child, &shown.at)) {
                children.push(shown.at.clone());
            }
        }
        Ok(children)
    }

    fn remove_dir_all(&self, _: &Path) -> io::Result<()> {
        Err(read_only())
    }

    fn remove_file(&self, _: &Path) -> io::Result<()> {
        Err(read_only())
    }

    fn create_dir_all(&self, _: &Path) -> io::Result<()> {
        Err(read_only())
    }

    fn remove_link(&self, _: &Path) -> io::Result<()> {
        Err(read_only())
    }

    fn create_link(&self, _: &Path, _: &Path) -> io::Result<()> {
        Err(read_only())
    }

    fn move_folder(&self, _: &Path, _: &Path) -> io::Result<()> {
        Err(read_only())
    }

    fn write_atomic(&self, _: &Path, _: &[u8]) -> io::Result<()> {
        Err(read_only())
    }

    fn write_sqlite_item(&self, _: &Path, _: &str, _: Option<&str>) -> io::Result<()> {
        Err(read_only())
    }

    fn try_lock(&self, _: &Path) -> io::Result<Option<HeldLock>> {
        Err(read_only())
    }
}
