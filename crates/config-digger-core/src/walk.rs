//! The walk down from a Project folder: the folders below it, by the rules
//! a location's [`DownWalk`] gives, such as the ones Cursor searches for
//! nested skills folders.

use crate::definitions::blocks::DownWalk;
use crate::paths;
use crate::ports::FileSystem;
use ignore::Match;
use ignore::gitignore::{Gitignore, GitignoreBuilder};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;

/// The ignore files of one folder, one for each name the walk reads, in
/// its order.
struct IgnoreFiles {
    files: Vec<Gitignore>,
    /// The folder above's, if it has any.
    above: Option<Rc<IgnoreFiles>>,
}

impl IgnoreFiles {
    /// Whether these files and those of the folders above leave out the
    /// folder at `path`: the deepest file of a name that matches it
    /// decides, a name over those after it, and a `!` pattern keeps it.
    fn leave_out(files: Option<&Rc<IgnoreFiles>>, path: &Path) -> bool {
        let mut matched = vec![Match::None; files.map_or(0, |files| files.files.len())];
        let mut level = files;
        while let Some(files) = level {
            for (matched, file) in matched.iter_mut().zip(&files.files) {
                if matched.is_none() {
                    *matched = file.matched(path, true).map(|_| ());
                }
            }
            level = files.above.as_ref();
        }
        matched
            .into_iter()
            .find(|matched| !matched.is_none())
            .is_some_and(|matched| matched.is_ignore())
    }
}

/// The ignore files named `names` of `folder`, under those of the folder
/// above; `above` itself when it has none.
fn ignore_files(
    fs: &dyn FileSystem,
    folder: &Path,
    names: &[String],
    above: Option<Rc<IgnoreFiles>>,
) -> Option<Rc<IgnoreFiles>> {
    let read = |name: &str| {
        let text = fs
            .read(&folder.join(name))
            .ok()
            .map(|bytes| String::from_utf8_lossy(&bytes).into_owned())?;
        let mut builder = GitignoreBuilder::new(folder);
        for line in text.lines() {
            // A line that is no pattern is skipped, as git skips it.
            let _ = builder.add_line(Some(folder.join(name)), line);
        }
        builder.build().ok()
    };
    let files: Vec<Option<Gitignore>> = names.iter().map(|name| read(name)).collect();
    if files.iter().all(Option::is_none) {
        return above;
    }
    Some(Rc::new(IgnoreFiles {
        files: files
            .into_iter()
            .map(|file| file.unwrap_or_else(Gitignore::empty))
            .collect(),
        above,
    }))
}

/// Every folder below `project` the walk `walk` enters, at any depth, in
/// the order of their paths, each folder before those below it; none when
/// the walk needs a git work tree and `project` is in none.
fn folders_below(fs: &dyn FileSystem, project: &Path, walk: &DownWalk) -> Vec<PathBuf> {
    if walk.needs_git
        && !project
            .ancestors()
            .any(|folder| fs.exists(&folder.join(".git")))
    {
        return Vec::new();
    }
    // The ignore files of the folders above the Project folder apply too.
    let names = &walk.ignore_files;
    let above = {
        let mut parents: Vec<&Path> = project.ancestors().skip(1).collect();
        parents.reverse();
        parents
            .into_iter()
            .fold(None, |above, parent| ignore_files(fs, parent, names, above))
    };
    let mut found = Vec::new();
    // A link back to a folder already entered would never end.
    let mut seen = BTreeSet::new();
    let mut folders: Vec<(PathBuf, Option<Rc<IgnoreFiles>>)> = vec![(project.to_path_buf(), above)];
    while let Some((folder, above)) = folders.pop() {
        if !seen.insert(paths::canonical_key(fs, &folder)) {
            continue;
        }
        let Ok(mut children) = fs.read_dir(&folder) else {
            continue;
        };
        if folder != project {
            found.push(folder.clone());
        }
        let files = ignore_files(fs, &folder, names, above);
        children.sort();
        // Last pushed, first entered.
        for child in children.into_iter().rev() {
            let Some(name) = child.file_name().and_then(|name| name.to_str()) else {
                continue;
            };
            if walk.skips(name)
                || (!walk.follow_links && fs.link_target(&child).is_some())
                || IgnoreFiles::leave_out(files.as_ref(), &child)
            {
                continue;
            }
            folders.push((child, files.clone()));
        }
    }
    found
}

/// One walk down: the Project folder it starts from, its rules, and the
/// folders it entered.
struct Walked {
    project: PathBuf,
    rules: DownWalk,
    entered: Vec<PathBuf>,
}

/// The walks down a scan has made, so that the locations that walk alike
/// from one Project folder walk once.
#[derive(Default)]
pub(crate) struct WalksDown(Vec<Walked>);

impl WalksDown {
    /// The folders below `project` that may hold the `/`-separated path
    /// `path`, by the rules `walk`, in the order the walk entered them: the
    /// folder holding each entered folder named as the path's first folder,
    /// case aside, so that the rules apply down to that folder and not
    /// below it. A path of one name may be in any folder entered, and so
    /// may any path when the rules leave the path's first folder to it.
    pub fn holders(
        &mut self,
        fs: &dyn FileSystem,
        project: &Path,
        walk: &DownWalk,
        path: &str,
    ) -> Vec<PathBuf> {
        let at = match self
            .0
            .iter()
            .position(|walked| walked.project == project && walked.rules == *walk)
        {
            Some(at) => at,
            None => {
                self.0.push(Walked {
                    project: project.to_path_buf(),
                    rules: walk.clone(),
                    entered: folders_below(fs, project, walk),
                });
                self.0.len() - 1
            }
        };
        let entered = &self.0[at].entered;
        let Some((first, _)) = path.split_once('/').filter(|_| walk.ignores_path) else {
            return entered.clone();
        };
        entered
            .iter()
            .filter(|folder| {
                folder
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case(first))
            })
            .filter_map(|folder| folder.parent())
            .filter(|holder| *holder != project)
            .map(Path::to_path_buf)
            .collect()
    }
}
