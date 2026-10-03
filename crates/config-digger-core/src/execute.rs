//! Execute: carries out a plan. It refuses unless every warning is
//! acknowledged, then, under the global lock: re-reads each touched file and
//! writes nothing when a touched Entry changed since the preview, backs each
//! file up and checks the copy, writes atomically, verifies, rolls the whole
//! Operation back when a write or Verify fails, and records the Operation in
//! History.

use crate::api::{
    Block, Difference, Error, ErrorCode, Inventory, OperationStatus, Outcome, RollbackLocation,
    RollbackOutcome, Verify, WarningClass,
};
use crate::definitions::AgentDefinition;
use crate::edit::EditableFile;
use crate::history::{self, EditRecord, LocationRecord, Manifest, VerifyRecord};
use crate::mask::{self, Side};
use crate::plan::{self, Planned, PlannedFile};
use crate::ports::Ports;
use crate::{explain, resolve, running, scan};
use serde_json::Value;
use std::io;
use std::path::{Path, PathBuf};

/// One file about to be written.
struct PendingWrite<'p> {
    file: &'p PlannedFile,
    /// The bytes read under the lock, which the backup holds, or `None` for
    /// a file the plan creates.
    current: Option<Vec<u8>>,
    /// The bytes to write.
    bytes: Vec<u8>,
}

pub(crate) fn execute(
    definitions: &[AgentDefinition],
    ports: &Ports,
    planned: Planned,
    acknowledged: &[WarningClass],
) -> Result<Outcome, Error> {
    // No acknowledgement lifts a block.
    if !planned.blocks.is_empty() {
        return Err(blocked(&planned.blocks));
    }
    let mut missing: Vec<WarningClass> = Vec::new();
    for warning in &planned.warnings {
        if !acknowledged.contains(&warning.class) && !missing.contains(&warning.class) {
            missing.push(warning.class);
        }
    }
    if !missing.is_empty() {
        return Err(Error::ack_required(&missing));
    }
    if let Some(operation) = &planned.preview.backup_of {
        // It deletes copies in History and writes no configuration file.
        let needed_attention = planned
            .warnings
            .iter()
            .any(|warning| warning.class == WarningClass::NeedsAttention);
        return history::delete_backup(ports, operation, needed_attention);
    }

    let data = history::data_folder(ports.env.as_ref())?;
    ports
        .fs
        .create_dir_all(&data)
        .map_err(|error| Error::unexpected(format!("cannot create {}: {error}", data.display())))?;
    // Held until this function returns, after Verify and the History write.
    let _lock = history::lock(ports, &data)?;
    // An Agent may have started since the plan: check again before writing.
    let running = running::check(definitions, ports, &planned.files);
    if !running.blocks.is_empty() {
        return Err(blocked(&running.blocks));
    }
    if let Some(undone) = &planned.preview.undo_of {
        // Another Undo of the same Operation may have run since the plan.
        let manifests = history::load_all(ports)?;
        if let Some(reason) = history::not_undoable(history::find(&manifests, undone)?, &manifests)
        {
            return Err(Error::stale(format!(
                "Operation {undone} cannot be undone any more: {reason}"
            )));
        }
    }

    let writes = planned
        .files
        .iter()
        .map(|file| reread(ports, file))
        .collect::<Result<Vec<_>, _>>()?;

    let now = ports.clock.now();
    let sequence = history::next_sequence(ports)?;
    let (id, folder) = operation_folder(ports, &data, now)?;
    let mut manifest = Manifest::new(id.clone(), sequence, now, &planned.preview);
    for (index, write) in writes.iter().enumerate() {
        let backup = match &write.current {
            Some(current) => {
                let backup = backup_name(index, &write.file.path);
                back_up(ports, &folder.join(&backup), current)?;
                Some(backup)
            }
            None => None,
        };
        manifest.locations.push(LocationRecord {
            path: write.file.path.clone(),
            syntax: write.file.syntax,
            backup,
            sha256_before: write.current.as_deref().map(history::sha256),
            sha256_after: history::sha256(&write.bytes),
            edits: write
                .file
                .edited
                .edits
                .iter()
                .map(|applied| EditRecord {
                    edit: applied.edit.clone(),
                    inverse: applied.inverse.clone(),
                    depends_on: applied.depends_on.clone(),
                })
                .collect(),
            deleted_edits: Vec::new(),
        });
    }
    history::record(ports, &folder, &manifest).map_err(Error::backup_failed)?;

    let mut written = 0;
    let mut failure = None;
    for write in &writes {
        if let Err(error) = ports.fs.write_atomic(&write.file.path, &write.bytes) {
            failure = Some(Failure::new(
                ErrorCode::WriteFailed,
                format!("cannot write {}: {error}", write.file.path.display()),
            ));
            break;
        }
        written += 1;
    }
    let verified = match failure {
        Some(failure) => Err(failure),
        None => verify(definitions, ports, &planned, &writes),
    };
    let differences = match verified {
        Ok(differences) => differences,
        Err(failure) => {
            let rollback = roll_back(ports, &writes, written);
            return Err(fail(
                definitions,
                ports,
                &folder,
                &mut manifest,
                &writes,
                rollback,
                failure,
            ));
        }
    };
    let verify = if differences.is_empty() {
        Verify::Verified
    } else {
        Verify::VerifiedWithDifferences
    };
    manifest.status = OperationStatus::Applied;
    manifest.verify = Some(VerifyRecord {
        verify,
        differences: differences.clone(),
    });
    // The files are written and verified: a History write that fails now is
    // logged, not reported as a failed Operation.
    if let Err(error) = history::record(ports, &folder, &manifest) {
        tracing::error!(operation = %id, %error, "cannot record the applied Operation");
    }
    tracing::info!(operation = %id, files = writes.len(), ?verify, "operation applied");
    Ok(Outcome {
        verify,
        sentence: explain::outcome(
            planned.action,
            planned.entries,
            planned.preview.undo_of.as_deref(),
            &id,
            verify,
        ),
        operation: id,
        differences,
        backup: folder,
        hints: running.hints,
    })
}

/// The refusal of a plan with `blocks`, each sentence saying what to do.
fn blocked(blocks: &[Block]) -> Error {
    let sentences: Vec<&str> = blocks.iter().map(|block| block.sentence.as_str()).collect();
    Error::blocked(sentences.join(" "))
}

/// Reads a touched file again under the lock. When it changed since the plan,
/// the edits are applied again to what it holds now, as long as every key
/// they depend on still holds the value the plan saw.
fn reread<'p>(ports: &Ports, file: &'p PlannedFile) -> Result<PendingWrite<'p>, Error> {
    if file.created {
        if ports.fs.exists(&file.path) {
            return Err(Error::stale(format!(
                "{} appeared since the preview",
                file.path.display()
            )));
        }
        return Ok(PendingWrite {
            file,
            bytes: file.edited.bytes.clone(),
            current: None,
        });
    }
    let current = plan::read(ports, &file.path)?;
    if current == file.read {
        return Ok(PendingWrite {
            file,
            bytes: file.edited.bytes.clone(),
            current: Some(current),
        });
    }
    let edits: Vec<_> = file
        .edited
        .edits
        .iter()
        .map(|applied| applied.edit.clone())
        .collect();
    let edited = plan::apply_edits(&current, file.syntax, &edits)
        .map_err(|failure| failure.into_error(&file.path))?;
    let unchanged = edited
        .edits
        .iter()
        .zip(&file.edited.edits)
        .all(|(now, planned)| {
            now.depends_on
                .iter()
                .zip(&planned.depends_on)
                .all(|(now, planned)| now.before == planned.before)
        });
    if !unchanged {
        return Err(Error::stale(format!(
            "an Entry in {} changed since the preview",
            file.path.display()
        )));
    }
    Ok(PendingWrite {
        file,
        bytes: edited.bytes,
        current: Some(current),
    })
}

/// Creates the Operation's folder under a fresh id.
fn operation_folder(
    ports: &Ports,
    data: &Path,
    now: std::time::SystemTime,
) -> Result<(String, PathBuf), Error> {
    let backups = history::backups_folder(data);
    for _ in 0..16 {
        let id = history::operation_id(now);
        let folder = backups.join(&id);
        if ports.fs.exists(&folder) {
            continue;
        }
        ports.fs.create_dir_all(&folder).map_err(|error| {
            Error::backup_failed(format!("cannot create {}: {error}", folder.display()))
        })?;
        return Ok((id, folder));
    }
    Err(Error::unexpected("cannot find a free Operation id"))
}

/// The name of a touched file's copy: its position, then its own name, so
/// that two files with one name do not collide.
fn backup_name(index: usize, path: &Path) -> String {
    let name = path
        .file_name()
        .map_or_else(|| "file".into(), |name| name.to_string_lossy());
    format!("{}-{name}", index + 1)
}

/// Copies `bytes` to `copy` and checks the copy holds exactly them.
fn back_up(ports: &Ports, copy: &Path, bytes: &[u8]) -> Result<(), Error> {
    ports.fs.write_atomic(copy, bytes).map_err(|error| {
        Error::backup_failed(format!(
            "cannot write the backup {}: {error}",
            copy.display()
        ))
    })?;
    let copied = ports.fs.read(copy).map_err(|error| {
        Error::backup_failed(format!(
            "cannot read the backup {} back: {error}",
            copy.display()
        ))
    })?;
    if history::sha256(&copied) != history::sha256(bytes) {
        return Err(Error::backup_failed(format!(
            "the backup {} does not match the file it copies",
            copy.display()
        )));
    }
    Ok(())
}

/// Why an Operation failed after it was backed up.
struct Failure {
    code: ErrorCode,
    cause: String,
}

impl Failure {
    fn new(code: ErrorCode, cause: String) -> Self {
        Self { code, cause }
    }
}

/// Verify: each edit landed, and every written file still parses for each
/// reader that could read it before. Then it scans and resolves the context
/// again and compares each predicted State with the actual one.
fn verify(
    definitions: &[AgentDefinition],
    ports: &Ports,
    planned: &Planned,
    writes: &[PendingWrite],
) -> Result<Vec<Difference>, Failure> {
    for write in writes {
        verify_file(ports, planned, write)?;
    }
    let snapshot =
        scan::scan(definitions, ports, &planned.scan_options.projects).map_err(|error| {
            Failure::new(
                ErrorCode::VerifyFailed,
                format!("cannot scan again: {error}"),
            )
        })?;
    let actual = resolve::resolve(definitions, &snapshot, &planned.context);
    for write in writes {
        let path = &write.file.path;
        let introduced = actual.read_failures.iter().find(|failure| {
            failure.path == *path
                && !planned
                    .read_failures
                    .iter()
                    .any(|before| before.same_reader_and_location(failure))
        });
        let Some(introduced) = introduced else {
            continue;
        };
        let ours = ports.fs.read(path).is_ok_and(|bytes| bytes == write.bytes);
        return Err(if ours {
            Failure::new(
                ErrorCode::ReadFailureIntroduced,
                format!(
                    "{} no longer parses for {}",
                    path.display(),
                    introduced.agent_name
                ),
            )
        } else {
            overwritten(planned, write)
        });
    }
    Ok(differences(planned, &actual))
}

/// Verify of one file: it reads back, and every key an edit changed holds the
/// value the edit gave it. When the file holds other bytes than those
/// written, something rewrote it since, and only a rewrite that took an edit
/// away fails the Operation.
fn verify_file(ports: &Ports, planned: &Planned, write: &PendingWrite) -> Result<(), Failure> {
    let path = &write.file.path;
    let bytes = ports.fs.read(path).map_err(|error| {
        Failure::new(
            ErrorCode::VerifyFailed,
            format!("cannot read {} back: {error}", path.display()),
        )
    })?;
    match edits_hold(&bytes, write) {
        Ok(()) => Ok(()),
        Err(_) if bytes != write.bytes => Err(overwritten(planned, write)),
        Err(Landed::ReadFailure(cause)) => {
            Err(Failure::new(ErrorCode::ReadFailureIntroduced, cause))
        }
        Err(Landed::Mismatch(cause)) => Err(Failure::new(ErrorCode::VerifyFailed, cause)),
    }
}

/// Why the edits of a file do not hold in it.
enum Landed {
    /// The file no longer decodes or parses.
    ReadFailure(String),
    /// A key does not hold what its last edit gave it.
    Mismatch(String),
}

/// One key the edits of a file changed.
struct TouchedKey<'w> {
    key_path: &'w [String],
    /// Its value before the Operation.
    before: &'w Option<Value>,
    /// The value its last edit gave it.
    after: &'w Option<Value>,
}

/// Each key the edits of a file changed, once, in the order first changed.
fn touched_keys<'w>(write: &'w PendingWrite) -> Vec<TouchedKey<'w>> {
    let mut keys: Vec<TouchedKey> = Vec::new();
    let dependencies = write.file.edited.edits.iter();
    for dependency in dependencies.flat_map(|applied| &applied.depends_on) {
        match keys
            .iter_mut()
            .find(|key| key.key_path == dependency.key_path.as_slice())
        {
            Some(key) => key.after = &dependency.after,
            None => keys.push(TouchedKey {
                key_path: &dependency.key_path,
                before: &dependency.before,
                after: &dependency.after,
            }),
        }
    }
    keys
}

/// Whether every key the edits of a file changed holds, in `bytes`, the
/// value its last edit gave it.
fn edits_hold(bytes: &[u8], write: &PendingWrite) -> Result<(), Landed> {
    let path = &write.file.path;
    let file = EditableFile::decode(bytes, write.file.syntax).map_err(|reason| {
        Landed::ReadFailure(format!("{} cannot be read back: {reason}", path.display()))
    })?;
    for key in touched_keys(write) {
        let value = file.value_at(key.key_path).map_err(|error| {
            Landed::ReadFailure(format!("{} no longer parses: {error}", path.display()))
        })?;
        if value != *key.after {
            return Err(Landed::Mismatch(format!(
                "`{}` in {} does not hold what was written",
                key.key_path.join("."),
                path.display()
            )));
        }
    }
    Ok(())
}

/// Whether every key the edits of a file changed holds, in `bytes`, the
/// value it had before the Operation.
fn edits_undone(bytes: &[u8], write: &PendingWrite) -> bool {
    let Ok(file) = EditableFile::decode(bytes, write.file.syntax) else {
        return false;
    };
    touched_keys(write).iter().all(|key| {
        file.value_at(key.key_path)
            .is_ok_and(|value| value == *key.before)
    })
}

/// A file rewritten after the write, taking an edit away. Agents that hold
/// their file in memory do this when they save; config-digger does not know
/// which one did, so it names the one whose States the file changes.
fn overwritten(planned: &Planned, write: &PendingWrite) -> Failure {
    let path = &write.file.path;
    let agent = planned
        .preview
        .changes
        .iter()
        .find(|change| change.path == *path)
        .map_or("the Agent that reads it", |change| {
            change.agent_name.as_str()
        });
    Failure::new(
        ErrorCode::OverwrittenByAgent,
        format!(
            "{} was rewritten after the write; if {agent} is running, close it and retry",
            path.display()
        ),
    )
}

/// The States after that differ from the preview's prediction.
fn differences(planned: &Planned, actual: &Inventory) -> Vec<Difference> {
    planned
        .preview
        .changes
        .iter()
        .filter_map(|change| {
            let state = plan::state_in(actual, &change.agent, &change.home, &change.locator);
            (state != change.after).then(|| Difference {
                agent: change.agent.clone(),
                home: change.home.clone(),
                locator: change.locator.clone(),
                predicted: change.after,
                actual: state,
            })
        })
        .collect()
}

/// Rollback: puts back the bytes each of the first `written` files held
/// before the Operation, but only while the file holds exactly what
/// config-digger wrote. A file changed since is left as it is.
fn roll_back(ports: &Ports, writes: &[PendingWrite], written: usize) -> Vec<RollbackLocation> {
    writes
        .iter()
        .enumerate()
        .map(|(index, write)| {
            let (outcome, reason) = if index < written {
                restore(ports, write)
            } else {
                (RollbackOutcome::NotWritten, None)
            };
            RollbackLocation {
                path: write.file.path.clone(),
                outcome,
                reason,
            }
        })
        .collect()
}

/// Rolls one written file back, and says why when it is not restored.
fn restore(ports: &Ports, write: &PendingWrite) -> (RollbackOutcome, Option<String>) {
    let path = &write.file.path;
    let not_restored = |reason: &str| (RollbackOutcome::NotRestored, Some(reason.to_owned()));
    let current = match ports.fs.read(path) {
        Ok(current) => Some(current),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => return not_restored(&format!("Cannot read it: {error}.")),
    };
    if current.as_ref() == Some(&write.bytes) {
        let restored = match &write.current {
            Some(before) => put_back(ports, path, before),
            None => ports
                .fs
                .remove_file(path)
                .map_err(|error| format!("Cannot delete the file it created: {error}.")),
        };
        return match restored {
            Ok(()) => (RollbackOutcome::Restored, None),
            Err(reason) => not_restored(&reason),
        };
    }
    let left_alone = |reason: &str| (RollbackOutcome::LeftAlone, Some(reason.to_owned()));
    match current {
        None if write.current.is_none() => {
            left_alone("It was deleted after config-digger created it.")
        }
        None => not_restored("It was deleted after config-digger wrote it."),
        Some(current) if edits_undone(&current, write) => left_alone(
            "It changed after config-digger wrote it and holds none of the Operation's edits, so it is left as it is.",
        ),
        Some(_) => not_restored(
            "It changed after config-digger wrote it and still holds some of the Operation's edits; restoring it would overwrite those changes.",
        ),
    }
}

/// Writes a file's bytes from before the Operation back, and checks it holds them.
fn put_back(ports: &Ports, path: &Path, bytes: &[u8]) -> Result<(), String> {
    ports
        .fs
        .write_atomic(path, bytes)
        .map_err(|error| format!("Cannot write it back: {error}."))?;
    match ports.fs.read(path) {
        Ok(now) if now == bytes => Ok(()),
        Ok(_) => Err("It does not hold its backup after being written back.".to_owned()),
        Err(error) => Err(format!("Cannot read it back: {error}.")),
    }
}

/// Records a failed Operation with its cause and Rollback, and returns the
/// error to report: the failure itself when the Rollback is complete, or
/// `RollbackIncomplete` with a diff of each file it could not restore.
fn fail(
    definitions: &[AgentDefinition],
    ports: &Ports,
    folder: &Path,
    manifest: &mut Manifest,
    writes: &[PendingWrite],
    rollback: Vec<RollbackLocation>,
    failure: Failure,
) -> Error {
    let complete = rollback
        .iter()
        .all(|location| location.outcome != RollbackOutcome::NotRestored);
    manifest.status = if complete {
        OperationStatus::Failed
    } else {
        OperationStatus::NeedsAttention
    };
    manifest.cause = Some(failure.cause.clone());
    manifest.rollback = rollback.clone();
    if let Err(error) = history::record(ports, folder, manifest) {
        tracing::error!(operation = %manifest.id, %error, "cannot record the failed Operation");
    }
    tracing::error!(
        operation = %manifest.id,
        code = failure.code.code(),
        cause = %failure.cause,
        complete,
        "operation failed"
    );
    let diffs = writes
        .iter()
        .zip(&rollback)
        .filter(|(_, location)| location.outcome == RollbackOutcome::NotRestored)
        .map(|(write, _)| {
            let current = ports.fs.read(&write.file.path).ok();
            mask::diff(
                definitions,
                &write.file.readers,
                &write.file.path,
                Side {
                    bytes: write.current.as_deref(),
                    name: "before the Operation",
                },
                Side {
                    bytes: current.as_deref(),
                    name: "as it is now",
                },
            )
        })
        .collect();
    Error::rolled_back(
        failure.code,
        failure.cause,
        manifest.id.clone(),
        folder.to_path_buf(),
        rollback,
        diffs,
    )
}

#[cfg(test)]
mod tests {
    use crate::api::{
        Action, Context, Core, ErrorCode, Plan, RollbackOutcome, ScanOptions, Targets, Warning,
        WarningClass,
    };
    use crate::ports::fake::{self, FakeEnvironment, FakeFileSystem};

    const CLAUDE_JSON: &str = r"C:\Users\fixture\.claude.json";
    const SERVERS: &str = r#"{"mcpServers": {"linear": {"url": "https://u"}}}"#;

    /// A plan to remove `linear`, given the warnings no plan has yet.
    fn plan_with_warnings(fs: &FakeFileSystem, classes: &[WarningClass]) -> (Core, Plan) {
        let env = FakeEnvironment::with_home(r"C:\Users\fixture")
            .with_var("LOCALAPPDATA", r"C:\Users\fixture\AppData\Local");
        let core = Core::new(fake::ports(fs.clone(), env)).unwrap();
        let snapshot = core.scan(&ScanOptions::default()).unwrap();
        let targets = Targets {
            selectors: vec!["linear".to_owned()],
            all: false,
        };
        let mut plan = core
            .plan(&snapshot, &Context::default(), Action::Remove, &targets)
            .unwrap();
        for &class in classes {
            plan.0.warnings.push(Warning {
                class,
                sentence: String::new(),
            });
        }
        (core, plan)
    }

    #[test]
    fn execute_refuses_unless_every_warning_is_acknowledged() {
        let fs = FakeFileSystem::new().with_file(CLAUDE_JSON, SERVERS);
        let warnings = [WarningClass::GitVersioned, WarningClass::RunningAgent];
        let (core, plan) = plan_with_warnings(&fs, &warnings);

        let error = core
            .execute(plan, &[WarningClass::GitVersioned])
            .unwrap_err();

        assert_eq!(error.code, ErrorCode::AckRequired);
        assert!(error.message.contains("running-agent"), "{}", error.message);
        assert_eq!(fs.contents(CLAUDE_JSON).unwrap(), SERVERS.as_bytes());
    }

    #[test]
    fn execute_runs_once_every_warning_is_acknowledged() {
        let fs = FakeFileSystem::new().with_file(CLAUDE_JSON, SERVERS);
        let warnings = [WarningClass::GitVersioned, WarningClass::RunningAgent];
        let (core, plan) = plan_with_warnings(&fs, &warnings);

        core.execute(plan, &warnings).unwrap();

        assert_eq!(fs.contents(CLAUDE_JSON).unwrap(), br#"{"mcpServers": {}}"#);
    }

    /// Executes the plan to remove `linear`, writing `bytes` in place of
    /// what the edit computed, as a faulty edit would.
    fn execute_writing(fs: &FakeFileSystem, bytes: &str) -> crate::api::Error {
        let (core, mut plan) = plan_with_warnings(fs, &[]);
        plan.0.files[0].edited.bytes = bytes.as_bytes().to_vec();
        core.execute(plan, &[]).unwrap_err()
    }

    #[test]
    fn an_edit_that_did_not_land_fails_verify_and_is_rolled_back() {
        let fs = FakeFileSystem::new().with_file(CLAUDE_JSON, SERVERS);

        let error = execute_writing(&fs, r#"{"mcpServers": {"linear": {"url": "https://x"}}}"#);

        assert_eq!(error.code, ErrorCode::VerifyFailed);
        assert!(
            error.message.contains("mcpServers.linear"),
            "{}",
            error.message
        );
        assert_eq!(fs.contents(CLAUDE_JSON).unwrap(), SERVERS.as_bytes());
        assert_eq!(
            error.failed.as_ref().unwrap().rollback[0].outcome,
            RollbackOutcome::Restored
        );
    }

    #[test]
    fn a_write_its_reader_cannot_parse_any_more_is_rolled_back() {
        let fs = FakeFileSystem::new().with_file(CLAUDE_JSON, SERVERS);

        // The edit holds, but Claude Code reads the file as strict JSON.
        let error = execute_writing(&fs, r#"{"mcpServers": {} /* removed */}"#);

        assert_eq!(error.code, ErrorCode::ReadFailureIntroduced);
        assert!(error.message.contains("Claude Code"), "{}", error.message);
        assert_eq!(fs.contents(CLAUDE_JSON).unwrap(), SERVERS.as_bytes());
        assert_eq!(
            error.failed.as_ref().unwrap().rollback[0].outcome,
            RollbackOutcome::Restored
        );
    }
}
