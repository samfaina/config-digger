//! History: where Operations are recorded. Each Operation has a folder under
//! `backups` in the data folder, holding a copy of every file it touched and a
//! JSON manifest with the edits, their inverses and the keys they depend on.

use crate::api::{
    Action, Change, Difference, EditSummary, Error, OperationDetail, OperationLocation,
    OperationStatus, OperationSummary, Outcome, Preview, RollbackLocation, Verify,
};
use crate::edit::{Dependency, Edit, Syntax};
use crate::ports::{Environment, HeldLock, Ports};
use crate::{explain, plan};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::hash::{BuildHasher, RandomState};
use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

/// The version of the manifest format.
const MANIFEST_VERSION: u32 = 1;

/// The name of the manifest in an Operation's folder.
const MANIFEST: &str = "manifest.json";

/// config-digger's machine-local data folder: `%LOCALAPPDATA%\config-digger`,
/// or `~/.local/share/config-digger` where there is no `LOCALAPPDATA`.
pub(crate) fn data_folder(env: &dyn Environment) -> Result<PathBuf, Error> {
    let base = match env.var("LOCALAPPDATA").filter(|value| !value.is_empty()) {
        Some(local) => PathBuf::from(local),
        None => env
            .home_dir()
            .ok_or_else(|| Error::unexpected("cannot determine config-digger's data folder"))?
            .join(".local")
            .join("share"),
    };
    Ok(base.join("config-digger"))
}

/// The file the global write lock is taken on.
pub(crate) fn lock_file(data: &Path) -> PathBuf {
    data.join("lock")
}

/// Takes the global write lock without waiting, failing with `Locked` when
/// another Operation holds it. It is released when dropped.
pub(crate) fn lock(ports: &Ports, data: &Path) -> Result<HeldLock, Error> {
    let lock = lock_file(data);
    ports
        .fs
        .try_lock(&lock)
        .map_err(|error| Error::unexpected(format!("cannot lock {}: {error}", lock.display())))?
        .ok_or_else(|| Error::locked(&lock))
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

/// The folder holding the backup and manifest of the Operation `id`.
pub(crate) fn operation_folder(ports: &Ports, id: &str) -> Result<PathBuf, Error> {
    Ok(backups_folder(&data_folder(ports.env.as_ref())?).join(id))
}

/// The SHA-256 of some bytes, in lowercase hex.
pub(crate) fn sha256(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// One Operation as recorded in History.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct Manifest {
    pub manifest_version: u32,
    pub id: String,
    /// One more than that of the Operation recorded before it, so that
    /// Operations started within one second still sort.
    pub sequence: u64,
    /// When the Operation started, in RFC 3339.
    pub created_at: String,
    pub action: Action,
    /// What the Operation does, as the preview titled it.
    pub title: String,
    /// The Operation this one undoes, when it is an Undo.
    pub undo_of: Option<String>,
    pub status: OperationStatus,
    /// Why the Operation did not complete.
    pub cause: Option<String>,
    pub project: Option<PathBuf>,
    /// The States the preview predicted.
    pub changes: Vec<Change>,
    pub locations: Vec<LocationRecord>,
    pub verify: Option<VerifyRecord>,
    /// Where the Rollback of a failed Operation left each touched file.
    #[serde(default)]
    pub rollback: Vec<RollbackLocation>,
    /// When the user dismissed an Operation that needed attention, in RFC 3339.
    #[serde(default)]
    pub dismissed_at: Option<String>,
    /// When the user deleted its backup, in RFC 3339.
    #[serde(default)]
    pub backup_deleted_at: Option<String>,
}

impl Manifest {
    pub fn new(id: String, sequence: u64, now: SystemTime, preview: &Preview) -> Self {
        Self {
            manifest_version: MANIFEST_VERSION,
            id,
            sequence,
            created_at: humantime::format_rfc3339_seconds(now).to_string(),
            action: preview.action,
            title: preview.title.clone(),
            undo_of: preview.undo_of.clone(),
            status: OperationStatus::InProgress,
            cause: None,
            project: preview.project.clone(),
            changes: preview.changes.clone(),
            locations: Vec::new(),
            verify: None,
            rollback: Vec::new(),
            dismissed_at: None,
            backup_deleted_at: None,
        }
    }

    /// Its Rollback is incomplete and the user has not dismissed it.
    pub fn needs_attention(&self) -> bool {
        self.status == OperationStatus::NeedsAttention && self.dismissed_at.is_none()
    }

    pub fn to_json(&self) -> Vec<u8> {
        serde_json::to_vec_pretty(self).expect("a manifest serialises")
    }
}

/// One touched file.
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct LocationRecord {
    pub path: PathBuf,
    /// The syntax its edits are made in.
    pub syntax: Syntax,
    /// The name of its copy in the Operation's folder, or `None` when the
    /// file was missing and the Operation created it.
    pub backup: Option<String>,
    /// Of the bytes read and backed up, `None` for a file it created.
    pub sha256_before: Option<String>,
    /// Of the bytes written.
    pub sha256_after: String,
    /// Empty once the backup is deleted, since edit values may hold Secrets.
    pub edits: Vec<EditRecord>,
    /// What each edit did, without its values, once the backup is deleted.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub deleted_edits: Vec<EditSummary>,
}

impl LocationRecord {
    /// What each edit did, whether or not the backup was deleted.
    fn edit_summaries(&self) -> Vec<EditSummary> {
        if self.edits.is_empty() {
            return self.deleted_edits.clone();
        }
        self.edits
            .iter()
            .map(|record| EditSummary::from(&record.edit))
            .collect()
    }
}

/// One edit, with what Undo needs (ADR 0003).
#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct EditRecord {
    pub edit: Edit,
    pub inverse: Edit,
    pub depends_on: Vec<Dependency>,
}

#[derive(Debug, Serialize, Deserialize)]
pub(crate) struct VerifyRecord {
    pub verify: Verify,
    pub differences: Vec<Difference>,
}

/// Writes an Operation's manifest into its folder.
pub(crate) fn record(ports: &Ports, folder: &Path, manifest: &Manifest) -> Result<(), String> {
    let path = folder.join(MANIFEST);
    ports
        .fs
        .write_atomic(&path, &manifest.to_json())
        .map_err(|error| format!("cannot write {}: {error}", path.display()))
}

/// Every Operation recorded in History, newest first. A folder whose
/// manifest cannot be read is left out, and logged.
pub(crate) fn load_all(ports: &Ports) -> Result<Vec<Manifest>, Error> {
    let backups = backups_folder(&data_folder(ports.env.as_ref())?);
    let folders = match ports.fs.read_dir(&backups) {
        Ok(folders) => folders,
        Err(error) if error.kind() == io::ErrorKind::NotFound => Vec::new(),
        Err(error) => {
            return Err(Error::unexpected(format!(
                "cannot list {}: {error}",
                backups.display()
            )));
        }
    };
    let mut manifests: Vec<Manifest> = folders
        .iter()
        .filter_map(|folder| match load(ports, folder) {
            Ok(manifest) => Some(manifest),
            Err(error) => {
                tracing::warn!(folder = %folder.display(), %error, "cannot read an Operation");
                None
            }
        })
        .collect();
    manifests.sort_by(|a, b| {
        (&b.created_at, b.sequence, &b.id).cmp(&(&a.created_at, a.sequence, &a.id))
    });
    Ok(manifests)
}

fn load(ports: &Ports, folder: &Path) -> Result<Manifest, String> {
    let path = folder.join(MANIFEST);
    let bytes = ports
        .fs
        .read(&path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    serde_json::from_slice(&bytes)
        .map_err(|error| format!("cannot parse {}: {error}", path.display()))
}

/// The sequence number of the next Operation recorded.
pub(crate) fn next_sequence(ports: &Ports) -> Result<u64, Error> {
    Ok(load_all(ports)?
        .iter()
        .map(|manifest| manifest.sequence)
        .max()
        .map_or(1, |last| last + 1))
}

/// What History says of each Operation in `manifests`, in their order.
pub(crate) fn summaries(ports: &Ports, manifests: &[Manifest]) -> Vec<OperationSummary> {
    manifests
        .iter()
        .map(|manifest| summary(ports, manifest, manifests))
        .collect()
}

/// What History says of one Operation. Whether it can be undone is checked
/// as its Undo would be planned, against the files as they are now.
fn summary(ports: &Ports, manifest: &Manifest, manifests: &[Manifest]) -> OperationSummary {
    let undone_by = undone_by(&manifest.id, manifests).map(|undo| undo.id.clone());
    let not_undoable = not_undoable(manifest, manifests).or_else(|| {
        plan::undo_files(ports, manifest)
            .err()
            .map(|error| error.message)
    });
    let mut agents: Vec<String> = Vec::new();
    for change in &manifest.changes {
        if !agents.contains(&change.agent_name) {
            agents.push(change.agent_name.clone());
        }
    }
    OperationSummary {
        id: manifest.id.clone(),
        created_at: manifest.created_at.clone(),
        action: manifest.action,
        title: manifest.title.clone(),
        status: manifest.status,
        status_text: explain::operation_status(
            manifest.status,
            manifest.dismissed_at.is_some(),
            manifest.undo_of.as_deref(),
            undone_by.as_deref(),
            manifest.backup_deleted_at.is_some(),
        ),
        cause: manifest.cause.clone(),
        needs_attention: manifest.needs_attention(),
        undo_of: manifest.undo_of.clone(),
        undone_by,
        backup_deleted: manifest.backup_deleted_at.is_some(),
        agents,
        locations: manifest
            .locations
            .iter()
            .map(|location| location.path.clone())
            .collect(),
        undoable: not_undoable.is_none(),
        not_undoable,
    }
}

/// The Operation `id` among `manifests`.
pub(crate) fn find<'m>(manifests: &'m [Manifest], id: &str) -> Result<&'m Manifest, Error> {
    manifests
        .iter()
        .find(|manifest| manifest.id == id)
        .ok_or_else(|| Error::no_operation(id))
}

/// Why the Operation cannot be undone, from History alone, or `None` when
/// History does not stop it. Whether its files still allow it is up to
/// [`plan::undo_files`].
pub(crate) fn not_undoable(manifest: &Manifest, manifests: &[Manifest]) -> Option<String> {
    if let Some(reason) =
        explain::not_undoable(manifest.status, manifest.backup_deleted_at.is_some())
    {
        return Some(reason);
    }
    undone_by(&manifest.id, manifests).map(|undo| explain::already_undone(&undo.id))
}

/// The newest applied Undo of the Operation `id` that is still in effect:
/// one that was not itself undone since.
fn undone_by<'m>(id: &str, manifests: &'m [Manifest]) -> Option<&'m Manifest> {
    manifests.iter().find(|undo| {
        undo.status == OperationStatus::Applied
            && undo.undo_of.as_deref() == Some(id)
            && undone_by(&undo.id, manifests).is_none()
    })
}

/// The Operation `id`, with its edits and backed-up files.
pub(crate) fn detail(ports: &Ports, id: &str) -> Result<OperationDetail, Error> {
    let manifests = load_all(ports)?;
    let manifest = find(&manifests, id)?;
    let folder = operation_folder(ports, id)?;
    Ok(OperationDetail {
        summary: summary(ports, manifest, &manifests),
        project: manifest.project.clone(),
        changes: manifest.changes.clone(),
        locations: manifest
            .locations
            .iter()
            .map(|location| OperationLocation {
                path: location.path.clone(),
                backup: location
                    .backup
                    .as_ref()
                    .filter(|_| manifest.backup_deleted_at.is_none())
                    .map(|backup| folder.join(backup)),
                edits: location.edit_summaries(),
            })
            .collect(),
        cause: manifest.cause.clone(),
        rollback: manifest.rollback.clone(),
        verify: manifest.verify.as_ref().map(|record| record.verify),
        differences: manifest
            .verify
            .as_ref()
            .map(|record| record.differences.clone())
            .unwrap_or_default(),
        backup: folder,
    })
}

/// Dismisses the Operation `id`, under the global lock, when it needs
/// attention.
pub(crate) fn dismiss(ports: &Ports, id: &str) -> Result<OperationSummary, Error> {
    let data = data_folder(ports.env.as_ref())?;
    // With no History yet there is no data folder to take the lock in.
    if !ports.fs.exists(&backups_folder(&data)) {
        return Err(Error::no_operation(id));
    }
    let _lock = lock(ports, &data)?;
    let mut manifests = load_all(ports)?;
    let index = manifests
        .iter()
        .position(|manifest| manifest.id == id)
        .ok_or_else(|| Error::no_operation(id))?;
    if !manifests[index].needs_attention() {
        return Err(Error::blocked(format!(
            "Operation {id} does not need attention, so there is nothing to dismiss"
        )));
    }
    manifests[index].dismissed_at =
        Some(humantime::format_rfc3339_seconds(ports.clock.now()).to_string());
    record(ports, &backups_folder(&data).join(id), &manifests[index]).map_err(Error::unexpected)?;
    tracing::info!(operation = %id, "operation dismissed");
    Ok(summary(ports, &manifests[index], &manifests))
}

/// The copies in the Operation's backup that are still there. Only the copies
/// its manifest names belong to it: nothing else in the data folder, such
/// as a parked Entry, is part of a backup.
pub(crate) fn backup_copies(ports: &Ports, folder: &Path, manifest: &Manifest) -> Vec<PathBuf> {
    manifest
        .locations
        .iter()
        .filter_map(|location| location.backup.as_ref())
        .map(|backup| folder.join(backup))
        .filter(|copy| ports.fs.exists(copy))
        .collect()
}

/// The backup was deleted and none of its `copies` is left. A deletion that
/// could not delete every copy can be run again.
pub(crate) fn backup_already_deleted(manifest: &Manifest, copies: &[PathBuf]) -> bool {
    manifest.backup_deleted_at.is_some() && copies.is_empty()
}

/// Deletes the backup of the Operation `id` under the global lock. Its
/// manifest is rewritten first without the values of its edits, which makes
/// it not undoable, and dismissed when it needs attention; then each copy is
/// deleted and checked gone. `needed_attention` says whether it needed
/// attention at the preview, whose check was acknowledged only then.
pub(crate) fn delete_backup(
    ports: &Ports,
    id: &str,
    needed_attention: bool,
) -> Result<Outcome, Error> {
    let data = data_folder(ports.env.as_ref())?;
    let _lock = lock(ports, &data)?;
    let mut manifests = load_all(ports)?;
    let index = manifests
        .iter()
        .position(|manifest| manifest.id == id)
        .ok_or_else(|| Error::stale(format!("Operation {id} is no longer in History")))?;
    let mut manifest = manifests.swap_remove(index);
    let folder = backups_folder(&data).join(id);
    let copies = backup_copies(ports, &folder, &manifest);
    if backup_already_deleted(&manifest, &copies) {
        return Err(Error::stale(format!(
            "the backup of Operation {id} was deleted since the preview"
        )));
    }
    if manifest.needs_attention() != needed_attention {
        return Err(Error::stale(format!(
            "Operation {id} changed status since the preview"
        )));
    }

    let now = humantime::format_rfc3339_seconds(ports.clock.now()).to_string();
    if manifest.needs_attention() {
        manifest.dismissed_at = Some(now.clone());
    }
    manifest.backup_deleted_at.get_or_insert(now);
    for location in &mut manifest.locations {
        if !location.edits.is_empty() {
            location.deleted_edits = location.edit_summaries();
            location.edits.clear();
        }
    }
    record(ports, &folder, &manifest).map_err(Error::unexpected)?;

    let left: Vec<String> = copies
        .iter()
        .filter_map(|copy| match ports.fs.remove_file(copy) {
            Ok(()) if !ports.fs.exists(copy) => None,
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Ok(()) => Some(format!("{} is still there", copy.display())),
            Err(error) => Some(format!("cannot delete {}: {error}", copy.display())),
        })
        .collect();
    if !left.is_empty() {
        return Err(Error::unexpected(format!(
            "{}; Operation {id} can no longer be undone, and deleting its backup again deletes the rest",
            left.join("; ")
        )));
    }
    tracing::info!(operation = %id, copies = copies.len(), "backup deleted");
    Ok(Outcome {
        operation: id.to_owned(),
        verify: Verify::Verified,
        differences: Vec::new(),
        backup: folder,
        sentence: explain::outcome(Action::DeleteBackup, 0, None, id, Verify::Verified),
    })
}

/// History keeps at least this many Operations…
const KEEP_OPERATIONS: usize = 50;
/// …and every Operation younger than this.
const KEEP_FOR: Duration = Duration::from_secs(30 * 24 * 60 * 60);

/// Prunes History under the global lock: every Operation beyond the last
/// [`KEEP_OPERATIONS`] that is older than [`KEEP_FOR`] goes, with its
/// backup, unless it needs attention and was not dismissed. Returns the ids
/// pruned, oldest first.
pub(crate) fn prune(ports: &Ports) -> Result<Vec<String>, Error> {
    let data = data_folder(ports.env.as_ref())?;
    let backups = backups_folder(&data);
    if !ports.fs.exists(&backups) {
        return Ok(Vec::new());
    }
    let _lock = lock(ports, &data)?;
    let now = ports.clock.now();
    let mut pruned = Vec::new();
    for manifest in load_all(ports)?.iter().skip(KEEP_OPERATIONS).rev() {
        let old = humantime::parse_rfc3339(&manifest.created_at)
            .is_ok_and(|created| now.duration_since(created).is_ok_and(|age| age > KEEP_FOR));
        if !old || manifest.needs_attention() {
            continue;
        }
        let folder = backups.join(&manifest.id);
        match ports.fs.remove_dir_all(&folder) {
            Ok(()) => pruned.push(manifest.id.clone()),
            Err(error) => {
                tracing::warn!(folder = %folder.display(), %error, "cannot prune an Operation");
            }
        }
    }
    if !pruned.is_empty() {
        tracing::info!(operations = pruned.len(), "history pruned");
    }
    Ok(pruned)
}
