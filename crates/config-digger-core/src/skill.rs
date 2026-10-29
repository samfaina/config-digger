//! Skill content: what config-digger shows of a skill folder. Its files can
//! hold Secrets the masker does not understand, such as a `.env` file or a
//! script, so their content is never shown: only each file's relative path
//! and size, and the `name` and `description` of the `SKILL.md`
//! frontmatter. Two versions of a folder are compared file by file, and a
//! file that differs is only said to have changed.

use crate::api::{Error, SkillContent, SkillFile, SkillFileChange, SkillFileChangeKind};
use crate::definitions::blocks::{SkillDefect, SkillFallback, Unlisted};
use crate::ports::FileSystem;
use regex::Regex;
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::Path;
use std::sync::LazyLock;

/// The file whose frontmatter names and describes a skill.
const SKILL_MD: &str = "SKILL.md";

/// What the skill folder at `folder` holds, its frontmatter read as an
/// Agent whose `malformed_frontmatter` is `reading` reads it.
pub(crate) fn content(
    fs: &dyn FileSystem,
    folder: &Path,
    reading: SkillFallback,
) -> Result<SkillContent, Error> {
    let files = files(fs, folder)?;
    let keys = files
        .get(SKILL_MD)
        .and_then(|bytes| frontmatter(&String::from_utf8_lossy(bytes), reading).ok())
        .unwrap_or_default()
        .keys;
    Ok(SkillContent {
        name: keys.get("name").cloned(),
        description: keys.get("description").cloned(),
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
pub(crate) fn files(
    fs: &dyn FileSystem,
    folder: &Path,
) -> Result<BTreeMap<String, Vec<u8>>, Error> {
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

/// The frontmatter of a `SKILL.md` as an Agent reads it.
#[derive(Default)]
pub(crate) struct Frontmatter {
    /// Its top-level scalar keys, as far as a skill's `name` and
    /// `description` need.
    pub keys: BTreeMap<String, String>,
    /// Whether it is YAML the Agent reads, as far as config-digger can
    /// tell: not read line by line, and with no plain value opening a `[`
    /// or `{` collection it never closes, which YAML refuses though
    /// config-digger reads the value as a string.
    pub read_as_yaml: bool,
    /// Whether it sets `disable-model-invocation` to the boolean `true`.
    pub disables_model_invocation: bool,
}

/// The frontmatter of a `SKILL.md`, as an Agent whose
/// `malformed_frontmatter` is `reading` reads it. Or why it has none: the
/// text does not open with a `---` frontmatter, the frontmatter is not YAML
/// the Agent reads, or, read line by line, it lacks the `name` or the
/// `description`.
pub(crate) fn frontmatter(text: &str, reading: SkillFallback) -> Result<Frontmatter, SkillDefect> {
    let lines = match lines(text) {
        Ok(lines) => lines,
        Err(Unclosed) if reading.reads_again() => return Err(SkillDefect::NoFrontmatter),
        Err(Unclosed) => return Err(SkillDefect::MalformedFrontmatter),
    }
    .ok_or(SkillDefect::NoFrontmatter)?;
    let read = scalars(&lines);
    let yaml = read.as_ref().filter(|read| match reading {
        SkillFallback::Skip | SkillFallback::FolderName => !read.any(|scalar| scalar.colon),
        SkillFallback::QuoteValues => {
            !read.indented_first
                && !read.any(Scalar::refused_quoted)
                && !["name", "description"]
                    .iter()
                    .any(|key| read.twice.contains(*key))
        }
        SkillFallback::LineByLine => {
            !read.indented_first
                && !read.any(Scalar::refused)
                && read.twice.is_empty()
                && ["name", "description"]
                    .iter()
                    .all(|key| read.keys.get(*key).is_some_and(|scalar| scalar.yaml_string))
        }
    });
    match (yaml, reading) {
        (Some(read), _) => Ok(Frontmatter {
            keys: read
                .keys
                .iter()
                .map(|(key, scalar)| (key.clone(), scalar.value.clone()))
                .collect(),
            read_as_yaml: !read.any(|scalar| scalar.unclosed_collection),
            disables_model_invocation: read
                .keys
                .get("disable-model-invocation")
                .is_some_and(Scalar::is_true),
        }),
        (None, SkillFallback::LineByLine) => Ok(Frontmatter {
            keys: line_by_line(&lines)?,
            ..Frontmatter::default()
        }),
        (None, _) => Err(SkillDefect::MalformedFrontmatter),
    }
}

/// Of `reasons`, in their order as declared, those for which an Agent
/// leaves out of its model's list the skill whose `SKILL.md` holds `text`
/// and whose frontmatter it reads as `read`. A frontmatter that is not
/// YAML config-digger reads, or that never closes, gives no other reason
/// than the malformed frontmatter, as the Agent may keep values from it
/// that config-digger does not read; one the Agent reads as no
/// frontmatter gives none.
pub(crate) fn unlisted(
    text: &str,
    read: &Result<Frontmatter, SkillDefect>,
    reasons: &[Unlisted],
) -> Vec<Unlisted> {
    let holds = |reason: &Unlisted| match (read, reason) {
        // A frontmatter that never closes may be read as none too.
        (Err(SkillDefect::NoFrontmatter), Unlisted::NoDescription) => {
            matches!(lines(text), Ok(None))
        }
        (Err(SkillDefect::MalformedFrontmatter), Unlisted::MalformedFrontmatter) => true,
        (Ok(read), Unlisted::MalformedFrontmatter) => !read.read_as_yaml,
        (Ok(read), _) if !read.read_as_yaml => false,
        (Ok(read), Unlisted::NoDescription) => !read.keys.contains_key("description"),
        (Ok(read), Unlisted::DisableModelInvocation) => read.disables_model_invocation,
        (Err(_), _) => false,
    };
    let mut unlisted: Vec<Unlisted> = reasons.iter().copied().filter(holds).collect();
    unlisted.sort();
    unlisted.dedup();
    unlisted
}

/// A frontmatter that opens with `---` and never closes.
struct Unclosed;

/// The lines of the frontmatter of a `SKILL.md`, or none when it does not
/// open with `---`.
fn lines(text: &str) -> Result<Option<Vec<&str>>, Unclosed> {
    let mut lines = text.trim_start_matches('\u{feff}').lines();
    if lines.next().map(str::trim_end) != Some("---") {
        return Ok(None);
    }
    let mut closed = false;
    let lines = lines
        .take_while(|line| {
            closed = line.trim_end() == "---";
            !closed
        })
        .collect();
    if closed {
        Ok(Some(lines))
    } else {
        Err(Unclosed)
    }
}

/// The top-level keys of a frontmatter, each read as YAML would read it as
/// far as it can be told.
struct Scalars {
    keys: BTreeMap<String, Scalar>,
    /// The keys given more than once, which YAML refuses. The later wins in
    /// `keys`.
    twice: BTreeSet<String>,
    /// Whether an indented line comes before the first key, which YAML
    /// refuses.
    indented_first: bool,
}

impl Scalars {
    fn any(&self, test: impl Fn(&Scalar) -> bool) -> bool {
        self.keys.values().any(test)
    }
}

/// The value of a top-level key.
struct Scalar {
    value: String,
    /// Whether it is a plain value holding `: `, which would start a mapping
    /// in YAML.
    colon: bool,
    /// Whether it is a plain value with a line ending in `:`, which would
    /// start a mapping in YAML too.
    colon_at_end: bool,
    /// Whether it is a plain value going on over indented lines.
    continued: bool,
    /// Whether it is a quoted value whose quote does not close on its line.
    unclosed: bool,
    /// Whether it is a plain value opening a `[` or `{` collection it never
    /// closes.
    unclosed_collection: bool,
    /// Whether YAML reads it, when it reads it, as a string rather than
    /// null, a boolean, a number, a date or a collection.
    yaml_string: bool,
}

impl Scalar {
    /// Whether YAML refuses the value.
    fn refused(&self) -> bool {
        self.colon || self.colon_at_end || self.unclosed
    }

    /// Whether it is the plain value `true`, which YAML reads as the
    /// boolean. How an Agent reads `True` or `TRUE` is not known.
    fn is_true(&self) -> bool {
        !self.yaml_string && self.value == "true"
    }

    /// Whether YAML still refuses the value once Codex quotes it, which it
    /// does to a plain value holding `: `: quoted, such a value is YAML
    /// while it stays on its line.
    fn refused_quoted(&self) -> bool {
        let quoted = self.colon && !self.continued;
        self.unclosed || (!quoted && (self.colon || self.colon_at_end))
    }
}

/// The top-level keys of the frontmatter `lines`: plain, quoted, and `|` or
/// `>` block scalars, and plain scalars continued on indented lines. None
/// when a top-level line is not a key.
fn scalars(lines: &[&str]) -> Option<Scalars> {
    let mut read = Scalars {
        keys: BTreeMap::new(),
        twice: BTreeSet::new(),
        indented_first: false,
    };
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        index += 1;
        if line.trim().is_empty() || line.starts_with([' ', '\t', '#']) {
            read.indented_first |= read.keys.is_empty()
                && line.starts_with([' ', '\t'])
                && !line.trim().is_empty()
                && !line.trim_start().starts_with('#');
            continue;
        }
        let (key, value) = line.split_once(':')?;
        let value = value.trim();
        let continued: Vec<&str> = lines[index..]
            .iter()
            .take_while(|line| line.trim().is_empty() || line.starts_with([' ', '\t']))
            .copied()
            .collect();
        index += continued.len();
        let continued: Vec<&str> = continued.iter().map(|line| line.trim()).collect();
        let mut scalar = Scalar {
            value: String::new(),
            colon: false,
            colon_at_end: false,
            continued: false,
            unclosed: false,
            unclosed_collection: false,
            yaml_string: true,
        };
        match value.chars().next() {
            Some('|') => scalar.value = continued.join("\n").trim_end().to_owned(),
            Some('>') => scalar.value = fold(&continued),
            Some(quote @ ('"' | '\'')) => {
                let inner = value
                    .strip_prefix(quote)
                    .and_then(|value| value.strip_suffix(quote));
                scalar.unclosed = inner.is_none();
                scalar.value = inner.unwrap_or(value).to_owned();
            }
            // A key with no value on its line may open a mapping or a list,
            // which is no scalar.
            _ if value.is_empty()
                && continued
                    .iter()
                    .any(|line| line.contains(": ") || line.starts_with("- ")) =>
            {
                scalar.yaml_string = false;
            }
            _ => {
                let lines = [&[value][..], &continued].concat();
                // `: ` would start a mapping inside a plain scalar.
                scalar.colon = lines.iter().any(|line| line.contains(": "));
                scalar.colon_at_end = lines.iter().any(|line| line.ends_with(':'));
                scalar.continued = continued.iter().any(|line| !line.is_empty());
                scalar.value = fold(&lines);
                scalar.unclosed_collection =
                    [('[', ']'), ('{', '}')].iter().any(|(open, close)| {
                        scalar.value.starts_with(*open) && !scalar.value.ends_with(*close)
                    });
                scalar.yaml_string = !no_yaml_string(&scalar.value);
            }
        }
        let key = key.trim().to_owned();
        if read.keys.insert(key.clone(), scalar).is_some() {
            read.twice.insert(key);
        }
    }
    Some(read)
}

/// Whether YAML reads the plain value `value` as no string: null, a
/// boolean, a number or a date, as its core schema and timestamps do, or a
/// collection, an alias, a tag or a character no plain value starts with.
fn no_yaml_string(value: &str) -> bool {
    value.is_empty()
        || value.starts_with(['[', '{', '*', '!', '@', '`', '#'])
        || NO_STRING.is_match(value)
}

/// The plain values YAML reads as null, a boolean, a number or a date.
static NO_STRING: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
        r"^(?:~|null|Null|NULL|true|True|TRUE|false|False|FALSE",
        r"|[-+]?(?:0b[01_]+|0o[0-7_]+|0x[0-9a-fA-F_]+)",
        r"|[-+]?(?:[0-9][0-9_]*(?:\.[0-9_]*)?|\.[0-9_]+)(?:[eE][-+]?[0-9]+)?",
        r"|[-+]?\.(?:inf|Inf|INF)|\.(?:nan|NaN|NAN)",
        r"|[0-9]{4}-[0-9]{1,2}-[0-9]{1,2}(?:[Tt ].*)?)$",
    ))
    .expect("the pattern compiles")
});

/// The `name` and `description` of the frontmatter `lines`, read line by
/// line as Gemini CLI reads a frontmatter that is not YAML.
fn line_by_line(lines: &[&str]) -> Result<BTreeMap<String, String>, SkillDefect> {
    let (mut name, mut description) = (None, None);
    let mut index = 0;
    while index < lines.len() {
        let line = lines[index];
        index += 1;
        if let Some(value) = after_key(line, "name") {
            name = Some(value.trim().to_owned());
        } else if let Some(value) = after_key(line, "description") {
            let mut parts = vec![value.trim()];
            while let Some(next) = lines
                .get(index)
                .filter(|line| line.starts_with([' ', '\t']) && !line.trim().is_empty())
            {
                parts.push(next.trim());
                index += 1;
            }
            parts.retain(|part| !part.is_empty());
            description = Some(parts.join(" "));
        }
    }
    let name = name.ok_or(SkillDefect::NoName)?;
    let description = description.ok_or(SkillDefect::NoDescription)?;
    Ok(BTreeMap::from([
        ("name".to_owned(), name),
        ("description".to_owned(), description),
    ]))
}

/// What follows `<key>:` on `line`, after any blanks before the key.
fn after_key<'l>(line: &'l str, key: &str) -> Option<&'l str> {
    line.trim_start().strip_prefix(key)?.strip_prefix(':')
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

#[cfg(test)]
mod tests {
    use super::*;

    const BOTH: [Unlisted; 2] = [Unlisted::NoDescription, Unlisted::DisableModelInvocation];

    const EVERY: [Unlisted; 3] = [
        Unlisted::NoDescription,
        Unlisted::DisableModelInvocation,
        Unlisted::MalformedFrontmatter,
    ];

    /// Why an Agent whose `malformed_frontmatter` is `reading`, and which
    /// leaves skills out of its model's list for every reason, leaves out
    /// the skill whose `SKILL.md` holds `text`.
    fn unlisted_as(reading: SkillFallback, text: &str) -> Vec<Unlisted> {
        unlisted(text, &frontmatter(text, reading), &EVERY)
    }

    #[test]
    fn a_frontmatter_that_never_closes_gives_no_reason_though_an_agent_reads_it_as_none() {
        for reading in [SkillFallback::QuoteValues, SkillFallback::LineByLine] {
            let unclosed = "---\nname: x\ndisable-model-invocation: true\n";
            assert_eq!(
                frontmatter(unclosed, reading).err(),
                Some(SkillDefect::NoFrontmatter)
            );
            assert!(unlisted_as(reading, unclosed).is_empty());
            assert_eq!(
                unlisted_as(reading, "A skill with no frontmatter.\n"),
                [Unlisted::NoDescription]
            );
        }
    }

    #[test]
    fn the_reasons_come_in_their_declared_order_whatever_the_definition_s() {
        let text = "---\ndisable-model-invocation: true\n---\n";
        let read = frontmatter(text, SkillFallback::FolderName);
        let reversed = [Unlisted::DisableModelInvocation, Unlisted::NoDescription];
        assert_eq!(unlisted(text, &read, &reversed), BOTH);
        assert_eq!(
            unlisted(text, &read, &[Unlisted::DisableModelInvocation]),
            [Unlisted::DisableModelInvocation]
        );
    }

    #[test]
    fn a_frontmatter_config_digger_reads_as_malformed_gives_that_reason_alone() {
        for text in [
            "---\nname: [unclosed\ndisable-model-invocation: true\n---\n",
            "---\nname: x\nnot a key\n---\n",
            "---\nname: x\ndisable-model-invocation: true\n",
        ] {
            assert_eq!(
                unlisted_as(SkillFallback::FolderName, text),
                [Unlisted::MalformedFrontmatter],
                "{text}"
            );
        }
    }
}
