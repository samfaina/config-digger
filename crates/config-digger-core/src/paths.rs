//! Windows path normalisation. A folder that exists is compared by its real
//! path, one that does not by lexical normalisation, always case-insensitively.

use crate::ports::{Environment, FileSystem};
use std::path::{Path, PathBuf};

/// The lexical normalisation of a path spelling: `/` becomes `\`, a leading
/// `\` before a drive (`/E:/`) is dropped, the drive letter is upper-cased,
/// `.` and `..` are resolved, and a trailing `\` is trimmed.
pub(crate) fn lexical(spelling: &Path) -> PathBuf {
    let text = spelling.to_string_lossy().replace('/', "\\");
    let mut text = text.as_str();
    if is_drive(text.strip_prefix('\\').unwrap_or("")) {
        text = &text[1..];
    }
    // A drive-relative `E:x` is read as `E:\x`: config-digger has no current
    // folder per drive to resolve it against.
    let (prefix, rest) = if is_drive(text) {
        let drive = text[..1].to_ascii_uppercase();
        (format!("{drive}:\\"), &text[2..])
    } else if let Some(unc) = text.strip_prefix("\\\\") {
        // `\\server\share` is the root of a UNC path, which `..` cannot climb out of.
        let mut parts = unc.splitn(3, '\\');
        let server = parts.next().unwrap_or("");
        let share = parts.next().unwrap_or("");
        (format!("\\\\{server}\\{share}"), parts.next().unwrap_or(""))
    } else if let Some(rest) = text.strip_prefix('\\') {
        ("\\".to_owned(), rest)
    } else {
        (String::new(), text)
    };

    let mut parts: Vec<&str> = Vec::new();
    for part in rest.split('\\') {
        match part {
            "" | "." => {}
            ".." => {
                // `..` above a root stays at the root; in a relative path it is kept.
                if parts.last().is_some_and(|last| *last != "..") {
                    parts.pop();
                } else if prefix.is_empty() {
                    parts.push(part);
                }
            }
            part => parts.push(part),
        }
    }
    let parts = parts.join("\\");
    if prefix.is_empty() || prefix.ends_with('\\') || parts.is_empty() {
        PathBuf::from(format!("{prefix}{parts}"))
    } else {
        PathBuf::from(format!("{prefix}\\{parts}"))
    }
}

/// Whether a lexically normalised path is absolute: it starts with a drive or
/// a UNC share. A path relative to some current folder cannot name a Project.
pub(crate) fn is_absolute(lexical: &Path) -> bool {
    let text = lexical.to_string_lossy();
    is_drive(&text) || text.starts_with("\\\\")
}

/// Whether `text` starts with a drive, such as `E:`.
fn is_drive(text: &str) -> bool {
    let bytes = text.as_bytes();
    bytes.len() >= 2 && bytes[0].is_ascii_alphabetic() && bytes[1] == b':'
}

/// The path a spelling names: its real path when it exists, its lexical
/// normalisation otherwise. The flag says whether it exists.
pub(crate) fn canonical(fs: &dyn FileSystem, spelling: &Path) -> (PathBuf, bool) {
    let lexical = lexical(spelling);
    match fs.real_path(&lexical) {
        Some(real) => (self::lexical(&real), true),
        None => (lexical, false),
    }
}

/// The key of the path a spelling names, as [`canonical`] finds it.
pub(crate) fn canonical_key(fs: &dyn FileSystem, spelling: &Path) -> String {
    key(&canonical(fs, spelling).0)
}

/// The key two normalised paths are compared by.
pub(crate) fn key(path: &Path) -> String {
    path.to_string_lossy().to_lowercase()
}

/// The key of a spelling's lexical normalisation, for comparing without the disk.
pub(crate) fn lexical_key(spelling: &Path) -> String {
    key(&lexical(spelling))
}

/// Checks a path glob of an Agent definition: it starts with `~/`, `%NAME%`
/// or a drive, and each `{` closes before the next one opens.
pub(crate) fn validate_glob(glob: &str) -> Result<(), String> {
    if !(glob.starts_with("~/") || is_drive(glob) || variable_prefix(glob).is_some()) {
        return Err(format!(
            "path glob `{glob}` must start with `~/`, `%NAME%` or a drive"
        ));
    }
    let mut open = false;
    for c in glob.chars() {
        match c {
            '{' if !open => open = true,
            '}' if open => open = false,
            '{' | '}' => return Err(format!("path glob `{glob}` has unbalanced braces")),
            _ => {}
        }
    }
    if open {
        return Err(format!("path glob `{glob}` has unbalanced braces"));
    }
    Ok(())
}

/// Whether `path` matches a glob that [`validate_glob`] accepts, its `~/`
/// or `%NAME%` start expanded from `env`, compared case-insensitively. `*`
/// and `?` stay within one folder name, and `{a,b}` is either.
pub(crate) fn glob_matches(glob: &str, path: &Path, env: &dyn Environment) -> bool {
    let expanded = if let Some(rest) = glob.strip_prefix("~/") {
        let Some(home) = env.home_dir() else {
            return false;
        };
        format!("{}\\{rest}", home.display())
    } else if let Some((name, rest)) = variable_prefix(glob) {
        let Some(value) = env.var(name) else {
            return false;
        };
        format!("{value}{rest}")
    } else {
        glob.to_owned()
    };
    let pattern = key(&lexical(Path::new(&expanded)));
    let path: Vec<char> = key(&lexical(path)).chars().collect();
    alternatives(&pattern).iter().any(|pattern| {
        let pattern: Vec<char> = pattern.chars().collect();
        wildcard(&pattern, &path)
    })
}

/// The variable name and the rest of a glob that starts with `%NAME%`.
fn variable_prefix(glob: &str) -> Option<(&str, &str)> {
    let (name, rest) = glob.strip_prefix('%')?.split_once('%')?;
    let is_name = !name.is_empty()
        && name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '(' | ')'));
    is_name.then_some((name, rest))
}

/// Every pattern a glob's `{a,b}` groups stand for.
fn alternatives(pattern: &str) -> Vec<String> {
    let Some(open) = pattern.find('{') else {
        return vec![pattern.to_owned()];
    };
    let Some(close) = pattern[open..].find('}').map(|close| open + close) else {
        return vec![pattern.to_owned()];
    };
    let (head, tail) = (&pattern[..open], &pattern[close + 1..]);
    pattern[open + 1..close]
        .split(',')
        .flat_map(|choice| alternatives(&format!("{head}{choice}{tail}")))
        .collect()
}

/// Whether `text` matches `pattern`, where `*` is any run and `?` any one
/// character, neither of them a `\`.
fn wildcard(pattern: &[char], text: &[char]) -> bool {
    match pattern.split_first() {
        None => text.is_empty(),
        Some(('*', rest)) => (0..=text.len())
            .take_while(|&skipped| skipped == 0 || text[skipped - 1] != '\\')
            .any(|skipped| wildcard(rest, &text[skipped..])),
        Some(('?', rest)) => text.first().is_some_and(|&c| c != '\\') && wildcard(rest, &text[1..]),
        Some((&c, rest)) => text.first() == Some(&c) && wildcard(rest, &text[1..]),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lexical(spelling: &str) -> String {
        super::lexical(Path::new(spelling)).display().to_string()
    }

    #[test]
    fn separators_drive_and_trailing_separator() {
        assert_eq!(lexical("e:/x/y/"), r"E:\x\y");
        assert_eq!(lexical("/E:/x"), r"E:\x");
        assert_eq!(lexical(r"E:\"), r"E:\");
        assert_eq!(lexical("e:"), r"E:\");
    }

    #[test]
    fn dots_are_resolved_without_climbing_above_the_root() {
        assert_eq!(lexical(r"E:\a\.\b\..\c"), r"E:\a\c");
        assert_eq!(lexical(r"E:\..\a"), r"E:\a");
        assert_eq!(lexical(r"..\a\..\..\b"), r"..\..\b");
    }

    #[test]
    fn unc_paths_keep_their_leading_separators() {
        assert_eq!(lexical("//server/share/x/"), r"\\server\share\x");
        assert_eq!(lexical(r"\\server\share\..\..\x"), r"\\server\share\x");
    }

    #[test]
    fn only_drive_and_unc_paths_are_absolute() {
        for (spelling, absolute) in [
            (r"E:\x", true),
            ("/E:/x", true),
            (r"\\server\share", true),
            (r"\x", false),
            ("rel/x", false),
            ("", false),
        ] {
            assert_eq!(
                is_absolute(&super::lexical(Path::new(spelling))),
                absolute,
                "{spelling}"
            );
        }
    }
}
