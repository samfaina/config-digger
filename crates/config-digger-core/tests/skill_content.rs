//! Skill content is never shown: a skill folder is summarised by its files'
//! relative paths and sizes and its `SKILL.md` frontmatter `name` and
//! `description`, and two versions of it are compared file by file, through
//! the core `api` with fake ports. The only Secret here is the fake
//! `not-a-real-secret`.

use config_digger_core::api::{
    Core, ErrorCode, SkillContent, SkillFile, SkillFileChange, SkillFileChangeKind,
};
use config_digger_core::ports::fake::{self, FakeEnvironment, FakeFileSystem};
use std::path::PathBuf;

const HOME: &str = r"C:\Users\fixture";
const SECRET: &str = "not-a-real-secret";

fn skill() -> PathBuf {
    PathBuf::from(HOME).join(r".claude\skills\deploy")
}

fn backup() -> PathBuf {
    PathBuf::from(HOME).join(r"AppData\Local\config-digger\backups\op\deploy")
}

const SKILL_MD: &str = "---\nname: deploy\ndescription: Deploys the app to staging.\n---\n\nRun `scripts/deploy.sh`.\n";

fn core(fs: &FakeFileSystem) -> Core {
    Core::new(fake::ports(fs.clone(), FakeEnvironment::with_home(HOME))).unwrap()
}

/// A skill folder at `folder` holding a `.env` file with a Secret.
fn with_skill(fs: FakeFileSystem, folder: PathBuf) -> FakeFileSystem {
    fs.with_file(folder.join("SKILL.md"), SKILL_MD)
        .with_file(folder.join(".env"), format!("TOKEN={SECRET}\n"))
        .with_file(
            folder.join(r"scripts\deploy.sh"),
            "#!/bin/sh\necho deploy\n",
        )
}

fn file(path: &str, size: u64) -> SkillFile {
    SkillFile {
        path: path.to_owned(),
        size,
    }
}

#[test]
fn a_skill_is_its_files_paths_and_sizes_and_its_frontmatter_name_and_description() {
    let fs = with_skill(FakeFileSystem::new(), skill());

    let content = core(&fs).skill_content(&skill(), "claude-code").unwrap();

    assert_eq!(
        content,
        SkillContent {
            name: Some("deploy".to_owned()),
            description: Some("Deploys the app to staging.".to_owned()),
            files: vec![
                file(".env", 24),
                file("SKILL.md", SKILL_MD.len() as u64),
                file("scripts/deploy.sh", 22),
            ],
        }
    );
    assert!(!serde_json::to_string(&content).unwrap().contains(SECRET));
}

#[test]
fn two_versions_of_a_skill_differ_by_files_added_removed_or_changed_and_never_by_content() {
    let fs = with_skill(FakeFileSystem::new(), backup());
    let fs = with_skill(fs, skill())
        .with_file(skill().join(".env"), "TOKEN=not-a-real-secreT\n")
        .with_file(skill().join("notes.txt"), "x");
    fs.delete_file(skill().join(r"scripts\deploy.sh"));

    let changes = core(&fs)
        .compare_skill_folders(&backup(), &skill())
        .unwrap();

    assert_eq!(
        changes,
        [
            SkillFileChange {
                path: ".env".to_owned(),
                change: SkillFileChangeKind::Changed,
                size_before: Some(24),
                size_after: Some(24),
            },
            SkillFileChange {
                path: "notes.txt".to_owned(),
                change: SkillFileChangeKind::Added,
                size_before: None,
                size_after: Some(1),
            },
            SkillFileChange {
                path: "scripts/deploy.sh".to_owned(),
                change: SkillFileChangeKind::Removed,
                size_before: Some(22),
                size_after: None,
            },
        ]
    );
    assert!(
        !serde_json::to_string(&changes)
            .unwrap()
            .contains("not-a-real")
    );
}

#[test]
fn a_skill_folder_that_is_gone_holds_no_files() {
    let fs = with_skill(FakeFileSystem::new(), backup());

    let changes = core(&fs)
        .compare_skill_folders(&backup(), &skill())
        .unwrap();

    assert_eq!(changes.len(), 3);
    assert!(
        changes
            .iter()
            .all(|change| change.change == SkillFileChangeKind::Removed)
    );
}

#[test]
fn the_summary_of_a_missing_skill_folder_fails() {
    let error = core(&FakeFileSystem::new())
        .skill_content(&skill(), "claude-code")
        .unwrap_err();

    assert_eq!(error.code, ErrorCode::Blocked);
}

#[test]
fn a_multi_line_or_quoted_description_is_read_from_the_frontmatter() {
    let fs = FakeFileSystem::new().with_file(
        skill().join("SKILL.md"),
        "---\nname: \"deploy\"\ndescription: >\n  Deploys the app\n  to staging.\nallowed-tools: Bash\n---\nBody\n",
    );

    let content = core(&fs).skill_content(&skill(), "claude-code").unwrap();

    assert_eq!(content.name.as_deref(), Some("deploy"));
    assert_eq!(
        content.description.as_deref(),
        Some("Deploys the app to staging.")
    );
}

#[test]
fn a_skill_md_without_frontmatter_has_no_name_or_description() {
    let fs = FakeFileSystem::new().with_file(skill().join("SKILL.md"), "# Deploy\n");

    let content = core(&fs).skill_content(&skill(), "claude-code").unwrap();

    assert_eq!((content.name, content.description), (None, None));
    assert_eq!(content.files, [file("SKILL.md", 9)]);
}
