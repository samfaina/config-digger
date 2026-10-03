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
