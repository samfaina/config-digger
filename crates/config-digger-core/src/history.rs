//! History: where Operations are recorded. Each Operation has a folder under
//! `backups` in the data folder, holding a copy of every file it touched and a
//! JSON manifest with the edits, their inverses and the keys they depend on.

use crate::api::{Action, Change, Difference, Verify};
use crate::edit::{Dependency, Edit};
use crate::ports::Environment;
use serde::Serialize;
use sha2::{Digest, Sha256};
use std::hash::{BuildHasher, RandomState};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// The version of the manifest format.
const MANIFEST_VERSION: u32 = 1;

/// config-digger's machine-local data folder: `%LOCALAPPDATA%\config-digger`,
/// or `~/.local/share/config-digger` where there is no `LOCALAPPDATA`.
pub(crate) fn data_folder(env: &dyn Environment) -> Option<PathBuf> {
    let base = match env.var("LOCALAPPDATA").filter(|value| !value.is_empty()) {
        Some(local) => PathBuf::from(local),
        None => env.home_dir()?.join(".local").join("share"),
    };
    Some(base.join("config-digger"))
}

/// The file the global write lock is taken on.
pub(crate) fn lock_file(data: &Path) -> PathBuf {
    data.join("lock")
}

/// The folder holding every Operation's backup and manifest.
pub(crate) fn backups_folder(data: &Path) -> PathBuf {
    data.join("backups")
}

/// A short id that sorts by time, such as `20261001-143012-a3f`: the UTC
/// date and time to the second, then three random hex digits.
pub(crate) fn operation_id(now: SystemTime) -> String {
    let stamp = humantime::format_rfc3339_seconds(now).to_string();
    let digits: String = stamp
        .chars()
        .filter(char::is_ascii_digit)
        .take(14)
        .collect();
    let random = RandomState::new().hash_one(now) & 0xfff;
    format!("{}-{}-{random:03x}", &digits[..8], &digits[8..])
}

/// The SHA-256 of some bytes, in lowercase hex.
pub(crate) fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// One Operation as recorded in History.
#[derive(Debug, Serialize)]
pub(crate) struct Manifest {
    pub manifest_version: u32,
    pub id: String,
    /// When the Operation started, in RFC 3339.
    pub created_at: String,
    pub action: Action,
    pub status: Status,
    /// Why the Operation did not complete.
    pub cause: Option<String>,
    pub project: Option<PathBuf>,
    /// The States the preview predicted.
    pub changes: Vec<Change>,
    pub locations: Vec<LocationRecord>,
    pub verify: Option<VerifyRecord>,
}

impl Manifest {
    pub fn new(id: String, now: SystemTime, action: Action, project: Option<PathBuf>) -> Self {
        Self {
            manifest_version: MANIFEST_VERSION,
            id,
            created_at: humantime::format_rfc3339_seconds(now).to_string(),
            action,
            status: Status::InProgress,
            cause: None,
            project,
            changes: Vec::new(),
            locations: Vec::new(),
            verify: None,
        }
    }

    pub fn to_json(&self) -> Vec<u8> {
        serde_json::to_vec_pretty(self).expect("a manifest serialises")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Status {
    /// Backed up and being written.
    InProgress,
    Applied,
    /// Failed before any write.
    Failed,
    /// Failed after some writes; the backup is needed to recover.
    NeedsAttention,
}

/// One touched file.
#[derive(Debug, Serialize)]
pub(crate) struct LocationRecord {
    pub path: PathBuf,
    /// The name of its copy in the Operation's folder.
    pub backup: String,
    /// Of the bytes read and backed up.
    pub sha256_before: String,
    /// Of the bytes written.
    pub sha256_after: String,
    pub edits: Vec<EditRecord>,
}

/// One edit, with what Undo needs (ADR 0003).
#[derive(Debug, Serialize)]
pub(crate) struct EditRecord {
    pub edit: Edit,
    pub inverse: Edit,
    pub depends_on: Vec<Dependency>,
}

#[derive(Debug, Serialize)]
pub(crate) struct VerifyRecord {
    pub verify: Verify,
    pub differences: Vec<Difference>,
}
