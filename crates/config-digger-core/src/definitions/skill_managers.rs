//! Skill manager definitions: TOML data that says how a skill manager, such
//! as `npx skills`, lays out the skills it installs, so that the Provenance
//! of a skill Entry can be recognised in any Agent. Each one is a list of
//! detectors from the closed set in [`Detector`], each strong or weak.

use super::DefinitionError;
use super::blocks::Validate;
use super::marks::{self, Mark};
use schemars::JsonSchema;
use serde::Deserialize;

/// The skill manager definitions that ship inside config-digger, embedded
/// at compile time, in the order a tie between them is settled.
pub(super) const BUILT_IN: &[(&str, &str)] = &[
    (
        "npx-skills.toml",
        include_str!("../../definitions/skill-managers/npx-skills.toml"),
    ),
    (
        "skillfish.toml",
        include_str!("../../definitions/skill-managers/skillfish.toml"),
    ),
    (
        "openskills.toml",
        include_str!("../../definitions/skill-managers/openskills.toml"),
    ),
];

/// The only `format_version` this release reads.
const FORMAT_VERSION: u32 = 1;

/// The JSON Schema of a skill manager definition file.
pub fn json_schema() -> String {
    marks::json_schema::<SkillManagerDefinition>()
}

/// The built-in skill manager definitions, in the order a tie between
/// them is settled.
pub(crate) fn built_in() -> Result<Vec<SkillManagerDefinition>, DefinitionError> {
    BUILT_IN
        .iter()
        .map(|(file, text)| parse(file, text))
        .collect()
}

pub(crate) fn parse(file: &str, text: &str) -> Result<SkillManagerDefinition, DefinitionError> {
    parse_marked(file, text).map(|(definition, _)| definition)
}

/// The skill manager definition `text` of `file`, and its marks.
pub(super) fn parse_marked(
    file: &str,
    text: &str,
) -> Result<(SkillManagerDefinition, Vec<Mark>), DefinitionError> {
    let error = |message: String| DefinitionError {
        kind: "skill manager",
        file: file.to_owned(),
        message,
    };
    let (definition, marks): (SkillManagerDefinition, _) =
        marks::deserialize(file, text).map_err(error)?;
    definition.validate().map_err(error)?;
    Ok((definition, marks))
}

/// How a skill manager lays out the skills it installs.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
#[schemars(title = "config-digger skill manager definition")]
pub(crate) struct SkillManagerDefinition {
    /// The version of this file format. Must be 1.
    pub format_version: u32,
    pub manager: ManagerInfo,
    /// What tells a skill folder it installed apart. A folder is the
    /// manager's when any detector matches; across managers, the strongest
    /// match wins, then the first manager loaded.
    pub detectors: Vec<Detector>,
}

#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct ManagerInfo {
    /// A stable id, such as `npx-skills`.
    pub id: String,
    /// The display name, such as `npx skills`.
    pub name: String,
}

/// How much a detector's match says about who installed a folder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum Strength {
    /// A folder made by hand can match it too, such as one named like a
    /// skill in the manager's lock file: the Entry gets a
    /// `provenance_by_name_only` note.
    Weak,
    /// Only the manager leaves it, such as its marker file.
    Strong,
}

/// One sign that a skill folder was installed by the manager.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(tag = "block", rename_all = "kebab-case")]
pub(crate) enum Detector {
    LinkTargetGlob(LinkTargetGlob),
    MarkerFile(MarkerFile),
    LocationGlob(LocationGlob),
    LockFileEntry(LockFileEntry),
}

impl Detector {
    pub fn strength(&self) -> Strength {
        match self {
            Detector::LinkTargetGlob(block) => block.strength,
            Detector::MarkerFile(block) => block.strength,
            Detector::LocationGlob(block) => block.strength,
            Detector::LockFileEntry(block) => block.strength,
        }
    }
}

impl Validate for Detector {
    fn validate(&self) -> Result<(), String> {
        match self {
            Detector::LinkTargetGlob(block) => validate_tail_glob(&block.glob),
            Detector::MarkerFile(block) => block.validate(),
            Detector::LocationGlob(block) => validate_tail_glob(&block.glob),
            Detector::LockFileEntry(block) => block.validate(),
        }
    }
}

/// The skill folder is a link or junction whose target matches a glob.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct LinkTargetGlob {
    /// A `/`-separated glob matched against the last folder names of the
    /// target, such as `.agents/skills/*`; `*` and `?` stay within one
    /// name. Compared case-insensitively.
    pub glob: String,
    pub strength: Strength,
}

/// The skill folder holds a file the manager writes into each copy.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct MarkerFile {
    /// The file's name, such as `.skillfish.json`.
    pub file: String,
    pub strength: Strength,
}

impl Validate for MarkerFile {
    fn validate(&self) -> Result<(), String> {
        if self.file.is_empty() || self.file.contains(['/', '\\']) || self.file == ".." {
            return Err(format!("marker file `{}` must be one file name", self.file));
        }
        Ok(())
    }
}

/// The skill folder's own location matches a glob.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct LocationGlob {
    /// A `/`-separated glob matched against the last folder names of the
    /// skill folder's path, as in `link-target-glob`.
    pub glob: String,
    pub strength: Strength,
}

/// A JSON lock file of the manager has a key named like the skill folder.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub(crate) struct LockFileEntry {
    /// The lock file: a path starting with `~/` or `%NAME%`, or one
    /// relative to the skill folder, such as `../../skills-lock.json`.
    pub file: String,
    /// The key path of the object whose keys are skills.
    pub key_path: Vec<String>,
    /// How the manager turns a key into a folder name, if it does.
    pub sanitize: Option<KeySanitize>,
    /// The id of the Scope whose skills the lock file covers, such as
    /// `user` or `project`, compared with the Scope its Agent definition
    /// gives the skill folder's location. Without it, the lock file covers
    /// skills in every Scope.
    pub scope: Option<String>,
    pub strength: Strength,
}

impl Validate for LockFileEntry {
    fn validate(&self) -> Result<(), String> {
        if self.file.is_empty() {
            return Err("lock file must not be empty".into());
        }
        if let Some(scope) = &self.scope {
            super::non_empty("lock file Scope", scope)?;
        }
        super::blocks::validate_key_path(&self.key_path)
    }
}

/// How a skill manager turns a lock file key into a folder name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum KeySanitize {
    /// Lower case, each run of characters other than `a`-`z`, `0`-`9`, `.`
    /// and `_` replaced by one `-`, with no `.` or `-` at either end, as
    /// `npx skills` names folders.
    LowercaseHyphensDots,
}

impl KeySanitize {
    pub fn apply(self, key: &str) -> String {
        match self {
            KeySanitize::LowercaseHyphensDots => {
                let mut sanitized = String::new();
                for c in key.chars().flat_map(char::to_lowercase) {
                    if c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, '.' | '_') {
                        sanitized.push(c);
                    } else if !sanitized.ends_with('-') {
                        sanitized.push('-');
                    }
                }
                sanitized.trim_matches(['.', '-']).to_owned()
            }
        }
    }
}

/// Checks a glob matched against the last folder names of a path.
fn validate_tail_glob(glob: &str) -> Result<(), String> {
    let anchored = glob.starts_with(['/', '\\', '~', '%']) || glob.contains(':');
    if glob.is_empty() || anchored || glob.split('/').any(|part| part.is_empty() || part == "..") {
        return Err(format!(
            "glob `{glob}` must be relative folder names separated by `/`"
        ));
    }
    Ok(())
}

impl SkillManagerDefinition {
    fn validate(&self) -> Result<(), String> {
        if self.format_version != FORMAT_VERSION {
            return Err(format!(
                "format_version {} is not supported; expected {FORMAT_VERSION}",
                self.format_version
            ));
        }
        super::non_empty("manager id", &self.manager.id)?;
        super::non_empty("manager name", &self.manager.name)?;
        if self.detectors.is_empty() {
            return Err("a skill manager needs detectors".into());
        }
        self.detectors.iter().try_for_each(Validate::validate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn built_in_skill_manager_definitions_load() {
        let managers = built_in().unwrap();
        let ids: Vec<&str> = managers
            .iter()
            .map(|definition| definition.manager.id.as_str())
            .collect();
        assert_eq!(ids, ["npx-skills", "skillfish", "openskills"]);
    }

    /// A definition with the one detector `detector`.
    fn with_detector(detector: &str) -> Result<SkillManagerDefinition, DefinitionError> {
        let text = format!(
            "format_version = 1\nmanager = {{ id = \"m\", name = \"M\" }}\n[[detectors]]\n{detector}\n"
        );
        parse("m.toml", &text)
    }

    fn error(detector: &str) -> String {
        with_detector(detector).unwrap_err().message
    }

    #[test]
    fn a_glob_is_relative_folder_names() {
        for glob in ["~/.agents/*", "C:/x", "/x", "a//b", "../x"] {
            let message = error(&format!(
                "block = \"location-glob\"\nglob = \"{glob}\"\nstrength = \"weak\""
            ));
            assert!(
                message.contains("relative folder names"),
                "{glob}: {message}"
            );
        }
    }

    #[test]
    fn a_marker_file_is_one_file_name() {
        let message = error("block = \"marker-file\"\nfile = \"a/.marker\"\nstrength = \"strong\"");
        assert!(message.contains("one file name"), "{message}");
    }

    fn lock_file_entry(scope: &str) -> String {
        format!(
            "block = \"lock-file-entry\"\nfile = \"~/lock.json\"\nkey_path = [\"skills\"]\n{scope}strength = \"weak\""
        )
    }

    #[test]
    fn a_lock_file_entry_scope_is_a_scope_id() {
        for scope in ["\"\"", "\" \""] {
            let message = error(&lock_file_entry(&format!("scope = {scope}\n")));
            assert!(message.contains("Scope"), "{scope}: {message}");
        }
        with_detector(&lock_file_entry("scope = \"user\"\n")).unwrap();
        with_detector(&lock_file_entry("")).unwrap();
    }

    #[test]
    fn unknown_detectors_are_rejected() {
        let message = error("block = \"guess\"\nstrength = \"strong\"");
        assert!(message.contains("unknown variant"), "{message}");
    }

    #[test]
    fn a_lock_key_is_sanitised_as_npx_skills_names_folders() {
        let sanitize = KeySanitize::LowercaseHyphensDots;
        assert_eq!(sanitize.apply("ce:review"), "ce-review");
        assert_eq!(sanitize.apply("My Skill_v1.2"), "my-skill_v1.2");
        assert_eq!(sanitize.apply("..odd--"), "odd");
    }
}
