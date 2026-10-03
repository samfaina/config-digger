//! Disable mechanisms at work: which switches Disable an Entry in a
//! context, which one Disable would write for it, or why it is not offered,
//! and what Enable lifts. Disable never writes a git-versioned file, so a
//! switch in one is skipped for the next mechanism.

use crate::api::{DisableNotOffered, DisableOption, Reason, ReasonCode};
use crate::definitions::AgentDefinition;
use crate::definitions::blocks::{
    Anchored, DisableMechanism, DisableReason, Disables, MechanismContext,
};
use crate::edit::Edit;
use crate::explain;
use crate::locator;
use crate::scan::{ScannedEntry, ScannedSwitch, Snapshot};
use serde_json::Value;
use std::path::{Path, PathBuf};

/// The context's Project as Disable needs it.
pub(crate) struct ContextProject<'p> {
    pub path: &'p Path,
    /// What Projects are compared by.
    pub key: &'p str,
}

/// A switch set for an Entry: one that Disables it, or turns part of it off.
pub(crate) struct Hit<'a> {
    pub mechanism: &'a DisableMechanism,
    pub switch: &'a ScannedSwitch,
    /// The switch's key path for the Entry.
    pub key_path: Vec<String>,
    /// What the switch holds for the Entry.
    pub value: Option<&'a Value>,
    /// The Entry's name.
    pub name: &'a str,
}

impl Hit<'_> {
    /// Whether lifting `other` lifts this one too: the same key of the same
    /// file, for the same name.
    pub fn same_switch(&self, other: &Hit) -> bool {
        self.switch.path == other.switch.path
            && self.key_path == other.key_path
            && self.name == other.name
    }

    /// The switch's `<path>#<key path>` locator.
    pub fn locator(&self) -> String {
        locator::render(&self.switch.path, &self.key_path)
    }

    /// Why the Entry is Disabled, as its State's reason.
    pub fn reason(&self, agent_name: &str) -> Reason {
        let holds = self.holds();
        let (code, sentence) = match self.mechanism.reason() {
            DisableReason::DisabledByName => (
                ReasonCode::DisabledByName,
                explain::disabled_by_name(&holds, agent_name),
            ),
            DisableReason::DisabledNatively => (
                ReasonCode::DisabledNatively,
                explain::disabled_natively(&holds, agent_name),
            ),
        };
        Reason {
            code,
            effective: None,
            switch: Some(self.locator()),
            sentence,
        }
    }

    /// What the switch holds, as a sentence says it.
    pub fn holds(&self) -> String {
        let per_project = self.mechanism.context() == MechanismContext::Project;
        match self.mechanism {
            DisableMechanism::JsonNameList(_) => explain::in_list(
                self.name,
                self.key_path.last().expect("validation requires a key"),
                &self.switch.path,
                per_project,
            ),
            DisableMechanism::JsonField(_) => {
                explain::field_holds(&self.key_path, self.value, &self.switch.path, per_project)
            }
        }
    }

    /// The edits that lift the switch.
    pub fn enable_edits(&self) -> Vec<Edit> {
        self.mechanism
            .enable_edits(self.key_path.clone(), self.value, self.name)
    }

    /// What Enable writes to lift the switch, as a sentence.
    pub fn enable_sentence(&self) -> String {
        let per_project = self.mechanism.context() == MechanismContext::Project;
        explain::enable_switch(
            &self.enable_edits(),
            self.name,
            &self.switch.path,
            per_project,
        )
    }
}

/// The switches of the mechanisms covering `entry` that are set for it in
/// the context of the Project with the key `project`: those that Disable
/// it, and those that turn part of it off. Each in the definition's order.
pub(crate) fn hits<'a>(
    definition: &'a AgentDefinition,
    snapshot: &'a Snapshot,
    entry: &'a ScannedEntry,
    project: Option<&str>,
) -> (Vec<Hit<'a>>, Vec<Hit<'a>>) {
    let location = definition.location(&entry.location);
    let mut disabling = Vec::new();
    let mut partial = Vec::new();
    for (index, mechanism) in definition.rules(location.kind).disable.iter().enumerate() {
        if !mechanism.covers(&location.id) {
            continue;
        }
        let switches = snapshot.switches.iter().filter(|switch| {
            switch.agent_index == entry.agent_index
                && switch.home == entry.home
                && switch.kind == location.kind
                && switch.mechanism == index
                && (switch.project.is_none() || switch.project.as_deref() == project)
        });
        for switch in switches {
            let value = switch.value_for(&entry.name);
            let hit = Hit {
                mechanism,
                switch,
                key_path: switch.key_path_for(&entry.name),
                value,
                name: &entry.name,
            };
            if mechanism.disables(value, &entry.name) {
                disabling.push(hit);
            } else if mechanism.partial(value) {
                partial.push(hit);
            }
        }
    }
    (disabling, partial)
}

/// The switch Disable writes for an Entry, and the edit it makes there.
pub(crate) struct Choice {
    pub path: PathBuf,
    pub edit: Edit,
    /// What Disable writes, as a sentence.
    pub sentence: String,
}

/// What Disable would do to `entry` in the context of `project`.
pub(crate) fn option(
    definition: &AgentDefinition,
    snapshot: &Snapshot,
    entry: &ScannedEntry,
    project: Option<&ContextProject>,
) -> DisableOption {
    match choose(definition, snapshot, entry, project) {
        Ok(choice) => DisableOption::Offered {
            path: choice.path,
            sentence: choice.sentence,
        },
        Err((code, sentence)) => DisableOption::NotOffered { code, sentence },
    }
}

/// The switch Disable writes for `entry` in the context of `project`: that
/// of the most specific mechanism (target, then context) that covers the
/// Entry and applies in the context, the first in the definition's order
/// among equally specific ones, skipping any whose switch file is
/// git-versioned or cannot be read. Or why Disable is not offered.
pub(crate) fn choose(
    definition: &AgentDefinition,
    snapshot: &Snapshot,
    entry: &ScannedEntry,
    project: Option<&ContextProject>,
) -> Result<Choice, (DisableNotOffered, String)> {
    let location = definition.location(&entry.location);
    let roots = snapshot.roots(entry.agent_index, &entry.home);
    let mut mechanisms: Vec<(usize, &DisableMechanism)> = definition
        .rules(location.kind)
        .disable
        .iter()
        .enumerate()
        .filter(|(_, mechanism)| mechanism.covers(&location.id))
        .collect();
    mechanisms.sort_by_key(|(index, mechanism)| (mechanism.target(), mechanism.context(), *index));

    let mut versioned: Vec<PathBuf> = Vec::new();
    let mut unreadable: Vec<PathBuf> = Vec::new();
    let mut needs_project = false;
    let mut unrecorded = false;
    for (index, mechanism) in mechanisms {
        let switch = mechanism.switch();
        let project_key = match (mechanism.context(), project) {
            (MechanismContext::Project, None) => {
                needs_project = true;
                continue;
            }
            (MechanismContext::Project, Some(project)) => Some(project.key),
            (MechanismContext::All, _) => None,
        };
        let folder = project
            .map(|project| project.path)
            .filter(|_| switch.anchor.in_project());
        let Some(path) = switch.anchor.path(roots, folder) else {
            continue;
        };
        if snapshot.versioned.contains(&path) {
            versioned.push(path);
            continue;
        }
        if snapshot.unreadable_switches.contains(&path) {
            unreadable.push(path);
            continue;
        }
        let mut places = snapshot.switches.iter().filter(|place| {
            place.agent_index == entry.agent_index
                && place.home == entry.home
                && place.kind == location.kind
                && place.mechanism == index
                && place.path == path
                && place.project.as_deref() == project_key
        });
        // Under several spellings of one Project key, the one whose switch
        // exists already, or else the first the file holds.
        let first = places.clone().next();
        let Some(place) = places.find(|place| place.value.is_some()).or(first) else {
            // The Project folder is gone, or the Agent has not recorded the
            // Project in the file that keys the switch by it.
            unrecorded = true;
            continue;
        };
        let key_path = place.key_path_for(&entry.name);
        let value = place.value_for(&entry.name);
        let edit = mechanism.disable_edit(key_path, value, &entry.name);
        let sentence = explain::disable_switch(
            &edit,
            &entry.name,
            &path,
            mechanism.context() == MechanismContext::Project,
        );
        return Ok(Choice {
            path,
            edit,
            sentence,
        });
    }
    let agent = &definition.agent.name;
    // A file that cannot be read comes first: fixing it makes Disable available.
    Err(if !unreadable.is_empty() {
        (
            DisableNotOffered::UnreadableSwitch,
            explain::disable_only_in_unreadable(agent, &unreadable),
        )
    } else if !versioned.is_empty() {
        (
            DisableNotOffered::GitVersioned,
            explain::disable_only_in_versioned(agent, &versioned, needs_project),
        )
    } else {
        (
            DisableNotOffered::NoMechanism,
            explain::no_disable_mechanism(agent, needs_project, unrecorded),
        )
    })
}
