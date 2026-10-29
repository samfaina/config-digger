//! Where the roots of an Agent home are, as a scan found them, and for an
//! Overlay home, which of its own paths and of the home under it the Agent
//! reads.

use crate::definitions::blocks;
use crate::ports::FileSystem;
use std::collections::{BTreeMap, BTreeSet};
use std::ops::Deref;
use std::path::{Path, PathBuf};

/// The path of each root of an Agent home by name, and the machine-wide
/// paths of its definition by the text that names them. A path below a
/// root is the one [`Roots::join`] gives, which for an Overlay home can be
/// in the folder of the home under it.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct Roots {
    paths: BTreeMap<String, PathBuf>,
    /// The folder roots an Overlay home lists from both folders, by name.
    under: BTreeMap<String, Under>,
}

/// A folder root of an Overlay home that both its folder and the folder of
/// the home under it hold.
#[derive(Clone, Debug, PartialEq)]
struct Under {
    /// The root's folder in the home under it.
    folder: PathBuf,
    /// The names of the entries the overlay's own folder holds, which win,
    /// as [`entry_key`] has them.
    own: BTreeSet<String>,
}

impl Roots {
    /// The roots of an Overlay home whose own roots are `own`, over the
    /// home whose folder is `under_folder` and whose roots are `under`,
    /// laid out alike, path by path: a root is the overlay's if it exists,
    /// else the other home's while that home's folder exists, else the
    /// overlay's. A folder root both hold lists both folders by entry name.
    pub fn overlay(
        own: BTreeMap<String, PathBuf>,
        under: &BTreeMap<String, PathBuf>,
        under_folder: &Path,
        fs: &dyn FileSystem,
    ) -> Self {
        let under_exists = fs.exists(under_folder);
        let mut roots = Roots::default();
        for (name, path) in own {
            let Some(below) = under.get(&name) else {
                roots.paths.insert(name, path);
                continue;
            };
            if fs.exists(&path) {
                if fs.exists(below)
                    && let Ok(children) = fs.read_dir(&path)
                {
                    let own = children
                        .iter()
                        .filter_map(|child| child.file_name())
                        .map(|child| entry_key(&child.to_string_lossy()))
                        .collect();
                    let folder = below.clone();
                    roots.under.insert(name.clone(), Under { folder, own });
                }
                roots.paths.insert(name, path);
            } else if under_exists {
                roots.paths.insert(name, below.clone());
            } else {
                roots.paths.insert(name, path);
            }
        }
        roots
    }

    /// The path `relative`, `/`-separated, names below the root `name`: for
    /// a folder root an Overlay home lists from both folders, in the other
    /// home's folder when the overlay's holds no entry by its first name.
    pub fn join(&self, name: &str, relative: &str) -> Option<PathBuf> {
        let root = self.paths.get(name)?;
        let first = relative.split('/').next().unwrap_or_default();
        if let Some(under) = self.under.get(name)
            && !under.own.contains(&entry_key(first))
        {
            return Some(blocks::join_relative(&under.folder, relative));
        }
        Some(blocks::join_relative(root, relative))
    }

    /// For `folder`, a folder root an Overlay home lists from both folders,
    /// the root's folder in the home under it, and whether an entry of it
    /// by its name is one the overlay's own folder holds, which wins.
    pub fn under(&self, folder: &Path) -> Option<(&Path, impl Fn(&str) -> bool + '_)> {
        let (name, _) = self.paths.iter().find(|(_, path)| *path == folder)?;
        let under = self.under.get(name)?;
        let overlaid = |entry: &str| under.own.contains(&entry_key(entry));
        Some((under.folder.as_path(), overlaid))
    }

    /// Adds `paths`, machine-wide paths by the text that names them.
    pub fn extend(&mut self, paths: BTreeMap<String, PathBuf>) {
        self.paths.extend(paths);
    }
}

/// How an entry of a folder is told from another by its name: in any case,
/// as Windows does.
fn entry_key(name: &str) -> String {
    name.to_lowercase()
}

impl From<BTreeMap<String, PathBuf>> for Roots {
    fn from(paths: BTreeMap<String, PathBuf>) -> Self {
        Roots {
            paths,
            under: BTreeMap::new(),
        }
    }
}

impl Deref for Roots {
    type Target = BTreeMap<String, PathBuf>;

    fn deref(&self) -> &Self::Target {
        &self.paths
    }
}
