//! Execute: carries out a plan. It refuses unless every warning is
//! acknowledged, then, under the global lock: re-reads each touched file,
//! backs it up and checks the copy, writes atomically, verifies, and records
//! the Operation in History.

use crate::api::{Difference, Error, OperationStatus, Outcome, Verify, WarningClass};
use crate::definitions::AgentDefinition;
use crate::edit::EditableFile;
use crate::history::{self, EditRecord, LocationRecord, Manifest, VerifyRecord};
use crate::plan::{self, Planned, PlannedFile};
use crate::ports::Ports;
use crate::{explain, resolve, scan};
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
    let mut missing: Vec<WarningClass> = Vec::new();
    for warning in &planned.warnings {
        if !acknowledged.contains(&warning.class) && !missing.contains(&warning.class) {
            missing.push(warning.class);
        }
    }
    if !missing.is_empty() {
        return Err(Error::ack_required(&missing));
    }

    let data = history::data_folder(ports.env.as_ref())?;
    ports
        .fs
        .create_dir_all(&data)
        .map_err(|error| Error::unexpected(format!("cannot create {}: {error}", data.display())))?;
    let lock = history::lock_file(&data);
    // Held until this function returns, after Verify and the History write.
    let _lock = ports
        .fs
        .try_lock(&lock)
        .map_err(|error| Error::unexpected(format!("cannot lock {}: {error}", lock.display())))?
        .ok_or_else(|| Error::locked(&lock))?;

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
        });
    }
    history::record(ports, &folder, &manifest).map_err(Error::backup_failed)?;

    for (index, write) in writes.iter().enumerate() {
        if let Err(error) = ports.fs.write_atomic(&write.file.path, &write.bytes) {
            let cause = format!("cannot write {}: {error}", write.file.path.display());
            return Err(fail(ports, &folder, &mut manifest, index > 0, cause));
        }
    }
    if let Err(cause) = verify_files(ports, &writes) {
        return Err(fail(ports, &folder, &mut manifest, true, cause));
    }

    let differences = verify_states(definitions, ports, &planned)?;
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
    })
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
    let edited = plan::apply_edits(&file.path, &current, file.syntax, &edits)
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

/// Records a failed Operation and the error to report. There is no Rollback
/// yet, so an Operation that wrote anything needs manual recovery.
fn fail(
    ports: &Ports,
    folder: &Path,
    manifest: &mut Manifest,
    written: bool,
    cause: String,
) -> Error {
    manifest.status = if written {
        OperationStatus::NeedsAttention
    } else {
        OperationStatus::Failed
    };
    manifest.cause = Some(cause.clone());
    if let Err(error) = history::record(ports, folder, manifest) {
        tracing::error!(operation = %manifest.id, %error, "cannot record the failed Operation");
    }
    tracing::error!(operation = %manifest.id, %cause, "operation failed");
    if written {
        Error::needs_recovery(cause, folder.to_path_buf())
    } else {
        Error::write_failed(cause)
    }
}

/// Verify, per file: it reads back and every key an edit changed holds the
/// value the edit gave it.
fn verify_files(ports: &Ports, writes: &[PendingWrite]) -> Result<(), String> {
    for write in writes {
        let path = &write.file.path;
        let bytes = ports
            .fs
            .read(path)
            .map_err(|error| format!("cannot read {} back: {error}", path.display()))?;
        let file = EditableFile::decode(&bytes, write.file.syntax)
            .map_err(|reason| format!("{} cannot be read back: {reason}", path.display()))?;
        // A key an edit changes again is checked against its last value only.
        let mut checked: Vec<&[String]> = Vec::new();
        let dependencies = write.file.edited.edits.iter().rev();
        for dependency in dependencies.flat_map(|applied| applied.depends_on.iter().rev()) {
            if checked.contains(&dependency.key_path.as_slice()) {
                continue;
            }
            checked.push(&dependency.key_path);
            let value = file
                .value_at(&dependency.key_path)
                .map_err(|error| format!("{} no longer parses: {error}", path.display()))?;
            if value != dependency.after {
                return Err(format!(
                    "`{}` in {} does not hold what was written",
                    dependency.key_path.join("."),
                    path.display()
                ));
            }
        }
    }
    Ok(())
}

/// Verify, overall: scans and resolves the context again and compares each
/// predicted State with the actual one.
fn verify_states(
    definitions: &[AgentDefinition],
    ports: &Ports,
    planned: &Planned,
) -> Result<Vec<Difference>, Error> {
    let snapshot = scan::scan(definitions, ports, &planned.scan_options.projects)
        .map_err(Error::unexpected)?;
    let actual = resolve::resolve(definitions, &snapshot, &planned.context);
    Ok(planned
        .preview
        .changes
        .iter()
        .filter_map(|change| {
            let state = plan::state_in(&actual, &change.agent, &change.home, &change.locator);
            (state != change.after).then(|| Difference {
                agent: change.agent.clone(),
                home: change.home.clone(),
                locator: change.locator.clone(),
                predicted: change.after,
                actual: state,
            })
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use crate::api::{
        Action, Context, Core, ErrorCode, Plan, ScanOptions, Targets, Warning, WarningClass,
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
}
