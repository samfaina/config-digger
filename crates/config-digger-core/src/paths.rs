//! Windows path normalisation. A folder that exists is compared by its real
//! path, one that does not by lexical normalisation, always case-insensitively.

use crate::ports::FileSystem;
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

/// The key two normalised paths are compared by.
pub(crate) fn key(path: &Path) -> String {
    path.to_string_lossy().to_lowercase()
}

/// The key of a spelling's lexical normalisation, for comparing without the disk.
pub(crate) fn lexical_key(spelling: &Path) -> String {
    key(&lexical(spelling))
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
