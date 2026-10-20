//! The real check of whether only an administrator can change a file, as
//! Gemini CLI makes it before it reads its system settings.

use config_digger_core::ports::FileSystem;
use config_digger_core::ports::real::RealFileSystem;
use std::collections::BTreeSet;
use std::path::Path;

#[test]
fn a_file_the_user_wrote_is_not_admin_owned() {
    let root = tempfile::tempdir().unwrap();
    let file = root.path().join("settings.json");
    std::fs::write(&file, "{}").unwrap();

    assert!(RealFileSystem.admin_owned(&[&file]).is_empty());
}

#[cfg(windows)]
#[test]
fn a_file_of_windows_itself_is_admin_owned() {
    let windows = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_owned());
    let file = Path::new(&windows).join(r"System32\kernel32.dll");

    assert_eq!(
        RealFileSystem.admin_owned(&[&file]),
        BTreeSet::from([file.clone()])
    );
}

#[cfg(windows)]
#[test]
fn one_check_answers_for_each_file() {
    let windows = std::env::var("SystemRoot").unwrap_or_else(|_| r"C:\Windows".to_owned());
    let system = Path::new(&windows).join(r"System32\kernel32.dll");
    let root = tempfile::tempdir().unwrap();
    let user = root.path().join("settings.json");
    std::fs::write(&user, "{}").unwrap();
    let missing = root.path().join("missing.json");

    let owned = RealFileSystem.admin_owned(&[&user, &system, &missing]);

    assert_eq!(owned, BTreeSet::from([system.clone()]));
}
