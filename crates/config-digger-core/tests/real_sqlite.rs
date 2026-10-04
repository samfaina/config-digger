//! The real SQLite read against a real store laid out as VS Code and Cursor
//! keep `state.vscdb`: a value under a key of its `ItemTable`.

use config_digger_core::ports::FileSystem;
use config_digger_core::ports::real::RealFileSystem;
use rusqlite::Connection;
use std::io;

#[test]
fn an_item_table_value_is_read_by_its_key() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("state.vscdb");
    let connection = Connection::open(&path).unwrap();
    connection
        .execute_batch(
            "CREATE TABLE ItemTable (key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB);
             INSERT INTO ItemTable VALUES ('as.text', '{\"on\":true}');
             INSERT INTO ItemTable VALUES ('as.blob', CAST('[1,2]' AS BLOB));",
        )
        .unwrap();

    let fs = RealFileSystem;

    assert_eq!(
        fs.read_sqlite_item(&path, "as.text").unwrap().as_deref(),
        Some(r#"{"on":true}"#)
    );
    assert_eq!(
        fs.read_sqlite_item(&path, "as.blob").unwrap().as_deref(),
        Some("[1,2]")
    );
    assert_eq!(fs.read_sqlite_item(&path, "missing").unwrap(), None);
    // The Agent holding the store open does not keep it from being read.
    drop(connection);
}

#[test]
fn a_missing_store_or_one_that_is_not_sqlite_is_an_error() {
    let folder = tempfile::tempdir().unwrap();
    let fs = RealFileSystem;

    let missing = fs
        .read_sqlite_item(&folder.path().join("state.vscdb"), "key")
        .unwrap_err();
    assert_eq!(missing.kind(), io::ErrorKind::NotFound);

    let text = folder.path().join("text.vscdb");
    std::fs::write(
        &text,
        "not a database, but long enough to have a header page",
    )
    .unwrap();
    assert!(fs.read_sqlite_item(&text, "key").is_err());
}

/// A store with one `ItemTable` row, closed again.
fn store(folder: &std::path::Path) -> std::path::PathBuf {
    let path = folder.join("state.vscdb");
    Connection::open(&path)
        .unwrap()
        .execute_batch(
            "CREATE TABLE ItemTable (key TEXT UNIQUE ON CONFLICT REPLACE, value BLOB);
             INSERT INTO ItemTable VALUES ('key', 'value');",
        )
        .unwrap();
    path
}

#[test]
fn the_store_is_closed_straight_after_the_read() {
    let folder = tempfile::tempdir().unwrap();
    let path = store(folder.path());

    assert_eq!(
        RealFileSystem
            .read_sqlite_item(&path, "key")
            .unwrap()
            .as_deref(),
        Some("value")
    );
    // Windows refuses to delete a file that is still open.
    std::fs::remove_file(&path).unwrap();
    // A read-only open never leaves a journal behind.
    assert!(std::fs::read_dir(folder.path()).unwrap().next().is_none());
}

#[test]
fn a_store_an_agent_holds_locked_fails_after_a_short_wait() {
    let folder = tempfile::tempdir().unwrap();
    let path = store(folder.path());
    let writer = Connection::open(&path).unwrap();
    writer.execute_batch("BEGIN EXCLUSIVE;").unwrap();

    let started = std::time::Instant::now();
    let error = RealFileSystem.read_sqlite_item(&path, "key").unwrap_err();
    assert_ne!(error.kind(), io::ErrorKind::NotFound);
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
    drop(writer);
}

#[test]
fn an_item_table_value_is_written_under_its_key_and_removed_with_none() {
    let folder = tempfile::tempdir().unwrap();
    let path = store(folder.path());
    let fs = RealFileSystem;

    fs.write_sqlite_item(&path, "key", Some(r#"[["a",false]]"#))
        .unwrap();
    fs.write_sqlite_item(&path, "new", Some("[]")).unwrap();
    assert_eq!(
        fs.read_sqlite_item(&path, "key").unwrap().as_deref(),
        Some(r#"[["a",false]]"#)
    );
    assert_eq!(
        fs.read_sqlite_item(&path, "new").unwrap().as_deref(),
        Some("[]")
    );

    fs.write_sqlite_item(&path, "new", None).unwrap();
    assert_eq!(fs.read_sqlite_item(&path, "new").unwrap(), None);
    // The store is closed again: Windows refuses to delete an open file.
    std::fs::remove_file(&path).unwrap();
}

#[test]
fn a_write_never_creates_a_store() {
    let folder = tempfile::tempdir().unwrap();
    let path = folder.path().join("state.vscdb");

    let error = RealFileSystem
        .write_sqlite_item(&path, "key", Some("[]"))
        .unwrap_err();

    assert_eq!(error.kind(), io::ErrorKind::NotFound);
    assert!(!path.exists());
}

#[test]
fn a_write_to_a_store_an_agent_holds_locked_fails_after_a_short_wait() {
    let folder = tempfile::tempdir().unwrap();
    let path = store(folder.path());
    let writer = Connection::open(&path).unwrap();
    writer.execute_batch("BEGIN EXCLUSIVE;").unwrap();

    let started = std::time::Instant::now();
    let error = RealFileSystem
        .write_sqlite_item(&path, "key", Some("other"))
        .unwrap_err();
    assert_ne!(error.kind(), io::ErrorKind::NotFound);
    assert!(started.elapsed() < std::time::Duration::from_secs(5));
    drop(writer);
    assert_eq!(
        RealFileSystem
            .read_sqlite_item(&path, "key")
            .unwrap()
            .as_deref(),
        Some("value")
    );
}
