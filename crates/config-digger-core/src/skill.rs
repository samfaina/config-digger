//! Skill content: what config-digger shows of a skill folder. Its files can
//! hold Secrets the masker does not understand, such as a `.env` file or a
//! script, so their content is never shown: only each file's relative path
//! and size, and the `name` and `description` of the `SKILL.md`
//! frontmatter. Two versions of a folder are compared file by file, and a
//! file that differs is only said to have changed.

use crate::api::{Error, SkillContent, SkillFile, SkillFileChange, SkillFileChangeKind};
use crate::ports::FileSystem;
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::Path;

/// The file whose frontmatter names and describes a skill.
const SKILL_MD: &str = "SKILL.md";

/// What the skill folder at `folder` holds.
pub(crate) fn content(fs: &dyn FileSystem, folder: &Path) -> Result<SkillContent, Error> {
    let files = files(fs, folder)?;
    let frontmatter = files
        .get(SKILL_MD)
        .map(|bytes| frontmatter(&String::from_utf8_lossy(bytes)))
        .unwrap_or_default();
    Ok(SkillContent {
        name: frontmatter.get("name").cloned(),
        description: frontmatter.get("description").cloned(),
        files: files
            .iter()
            .map(|(path, bytes)| SkillFile {
                path: path.clone(),
                size: bytes.len() as u64,
            })
            .collect(),
    })
}

/// How the skill folder at `after` differs from the one at `before`, file
/// by file, in path order. A missing folder holds no files.
pub(crate) fn compare(
    fs: &dyn FileSystem,
    before: &Path,
    after: &Path,
) -> Result<Vec<SkillFileChange>, Error> {
    let before = files_or_none(fs, before)?;
    let after = files_or_none(fs, after)?;
    let mut paths: Vec<&String> = before.keys().chain(after.keys()).collect();
    paths.sort();
    paths.dedup();
    Ok(paths
        .into_iter()
        .filter_map(|path| {
            let (old, new) = (before.get(path), after.get(path));
            let change = match (old, new) {
                (None, Some(_)) => SkillFileChangeKind::Added,
                (Some(_), None) => SkillFileChangeKind::Removed,
                (Some(old), Some(new)) if old != new => SkillFileChangeKind::Changed,
                _ => return None,
            };
            Some(SkillFileChange {
                path: path.clone(),
                change,
                size_before: old.map(|bytes| bytes.len() as u64),
                size_after: new.map(|bytes| bytes.len() as u64),
            })
        })
        .collect())
}

fn files_or_none(fs: &dyn FileSystem, folder: &Path) -> Result<BTreeMap<String, Vec<u8>>, Error> {
    if fs.exists(folder) {
        files(fs, folder)
    } else {
        Ok(BTreeMap::new())
    }
}

/// Every file below `folder`, by its `/`-separated path relative to it.
fn files(fs: &dyn FileSystem, folder: &Path) -> Result<BTreeMap<String, Vec<u8>>, Error> {
    let mut files = BTreeMap::new();
    let mut folders = vec![(folder.to_path_buf(), String::new())];
    // A link back to a folder already read would never end.
    let mut seen = BTreeSet::new();
    while let Some((path, relative)) = folders.pop() {
        if !seen.insert(fs.real_path(&path).unwrap_or_else(|| path.clone())) {
            continue;
        }
        let children = fs
            .read_dir(&path)
            .map_err(|error| cannot_read(&path, &error))?;
        for child in children {
            let Some(name) = child.file_name().map(|name| name.to_string_lossy()) else {
                continue;
            };
            let child_relative = format!("{relative}{name}");
            match fs.read(&child) {
                Ok(bytes) => {
                    files.insert(child_relative, bytes);
                }
                // Not a file: a folder, read in turn.
                Err(_) if fs.read_dir(&child).is_ok() => {
                    folders.push((child, format!("{child_relative}/")));
                }
                Err(error) => return Err(cannot_read(&child, &error)),
            }
        }
    }
    Ok(files)
}

fn cannot_read(path: &Path, error: &io::Error) -> Error {
    Error::blocked(format!("cannot read {}: {error}", path.display()))
}

/// The top-level scalar keys of a YAML frontmatter, as far as a skill's
/// `name` and `description` need: plain, quoted, and `|` or `>` block
/// scalars, and plain scalars continued on indented lines.
fn frontmatter(text: &str) -> BTreeMap<String, String> {
    let mut keys = BTreeMap::new();
    let mut lines = text.lines();
    if lines.next().map(str::trim_end) != Some("---") {
        return keys;
    }
    let lines: Vec<&str> = lines.take_while(|line| line.trim_end() != "---").collect();
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        index += 1;
        if line.starts_with([' ', '\t', '#']) {
            continue;
        }
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        let continued: Vec<&str> = lines[index..]
            .iter()
            .take_while(|line| line.trim().is_empty() || line.starts_with([' ', '\t']))
            .copied()
            .collect();
        index += continued.len();
        let continued: Vec<&str> = continued.iter().map(|line| line.trim()).collect();
        let value = match value.chars().next() {
            Some('|') => continued.join("\n").trim_end().to_owned(),
            Some('>') => fold(&continued),
            Some(quote @ ('"' | '\'')) => value
                .strip_prefix(quote)
                .and_then(|value| value.strip_suffix(quote))
                .unwrap_or(value)
                .to_owned(),
            _ => fold(&[&[value][..], &continued].concat()),
        };
        keys.insert(key.trim().to_owned(), value);
    }
    keys
}

/// Lines folded into one, as YAML folds a plain or `>` scalar.
fn fold(lines: &[&str]) -> String {
    lines
        .iter()
        .filter(|line| !line.is_empty())
        .copied()
        .collect::<Vec<_>>()
        .join(" ")
}
