//! Disable mechanism selection: which of an Agent's own switches Disable
//! would write for an Entry in a context, or why it is not offered. Disable
//! never writes a git-versioned file, so a target that is one is skipped for
//! the next mechanism in the definition's order of preference.

use crate::api::{DisableOption, DisableTarget, DisableUnavailable, DisableUnavailableCode};
use crate::definitions::blocks::{Anchored, Mechanism};
use crate::definitions::{AgentDefinition, LocationDefinition};
use crate::explain;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// The context's Project as Disable needs it.
pub(crate) struct ContextProject<'p> {
    pub path: &'p Path,
    /// Its folder exists, so a file can be written in it.
    pub exists: bool,
}

/// What Disable would do to the Entry `name` of `location` in an Agent home
/// whose roots are `roots`. `versioned` holds the git-versioned files.
pub(crate) fn option(
    definition: &AgentDefinition,
    roots: &BTreeMap<String, PathBuf>,
    location: &LocationDefinition,
    name: &str,
    project: Option<&ContextProject>,
    versioned: &BTreeSet<PathBuf>,
) -> DisableOption {
    let agent = &definition.agent.name;
    let mut skipped: Vec<PathBuf> = Vec::new();
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
        if versioned.contains(&path) {
            skipped.push(path);
            continue;
        }
        let list = target.key_path.last().expect("validation requires a key");
        return DisableOption {
            target: Some(DisableTarget {
                sentence: explain::disable_target(name, list, &path, target.per_project()),
                path,
            }),
            unavailable: None,
        };
    }
    let (code, sentence) = if skipped.is_empty() {
        (
            DisableUnavailableCode::NoMechanism,
            explain::no_disable_mechanism(agent, needs_project),
        )
    } else {
        (
            DisableUnavailableCode::GitVersioned,
            explain::disable_only_in_versioned(agent, &skipped),
        )
    };
    DisableOption {
        target: None,
        unavailable: Some(DisableUnavailable { code, sentence }),
    }
}
