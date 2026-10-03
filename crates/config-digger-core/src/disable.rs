//! Disable mechanism selection: which of an Agent's own switches Disable
//! would write for an Entry in a context, or why it is not offered. Disable
//! never writes a git-versioned file, so a target that is one is skipped for
//! the next mechanism in the definition's order of preference.

use crate::api::{DisableNotOffered, DisableOption};
use crate::definitions::AgentDefinition;
use crate::definitions::blocks::{Anchored, Disables};
use crate::explain;
use crate::scan::{ScannedEntry, Snapshot};
use std::path::{Path, PathBuf};

/// The context's Project as Disable needs it.
pub(crate) struct ContextProject<'p> {
    pub path: &'p Path,
    /// Its folder exists, so a file can be written in it.
    pub exists: bool,
}

/// What Disable would do to `entry` in the context of `project`.
pub(crate) fn option(
    definition: &AgentDefinition,
    snapshot: &Snapshot,
    entry: &ScannedEntry,
    project: Option<&ContextProject>,
) -> DisableOption {
    let location = definition.location(&entry.location);
    let roots = snapshot.roots(entry.agent_index, &entry.home);
    let mut versioned: Vec<PathBuf> = Vec::new();
    let mut needs_project = false;
    for mechanism in &definition.rules(location.kind).disable {
        if !mechanism.covers(&location.id) {
            continue;
        }
        let target = mechanism.target();
        if target.per_project() && project.is_none() {
            needs_project = true;
            continue;
        }
        // A file in a Project folder can only be written while the folder exists.
        let folder = match project {
            Some(project) if target.anchor.in_project() && project.exists => Some(project.path),
            _ if target.anchor.in_project() => continue,
            _ => None,
        };
        let Some(path) = target.anchor.path(roots, folder) else {
            continue;
        };
        if snapshot.versioned.contains(&path) {
            versioned.push(path);
            continue;
        }
        let list = target.key_path.last().expect("validation requires a key");
        return DisableOption::Offered {
            sentence: explain::disable_target(&entry.name, list, &path, target.per_project()),
            path,
        };
    }
    let agent = &definition.agent.name;
    if versioned.is_empty() {
        DisableOption::NotOffered {
            code: DisableNotOffered::NoMechanism,
            sentence: explain::no_disable_mechanism(agent, needs_project),
        }
    } else {
        DisableOption::NotOffered {
            code: DisableNotOffered::GitVersioned,
            sentence: explain::disable_only_in_versioned(agent, &versioned, needs_project),
        }
    }
}
