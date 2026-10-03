//! Disable mechanisms at work: which switches Disable an Entry in a
//! context, which one Disable would write for it, or why it is not offered,
//! and what Enable lifts. Disable never writes a git-versioned file, so a
//! switch in one is skipped for the next mechanism.

use crate::api::{DisableNotOffered, DisableOption, Reason, ReasonCode};
use crate::definitions::blocks::{
    Anchored, DisableMechanism, DisableReason, Disables, MechanismContext, MechanismTarget,
};
use crate::definitions::{AgentDefinition, LocationDefinition};
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

/// A switch set for an Entry: one that Disables it, or a partial switch.
pub(crate) struct Hit<'a> {
    pub mechanism: &'a DisableMechanism,
    pub switch: &'a ScannedSwitch,
    /// The switch's key path for the Entry.
    pub key_path: Vec<String>,
    /// What the switch holds for the Entry.
    pub value: Option<&'a Value>,
    /// What the switch holds the Entry by: its name, or its plugin's id.
    pub key: &'a str,
}

impl Hit<'_> {
    /// Whether lifting `other` lifts this one too: the same key of the same
    /// file, for the same key.
    pub fn same_switch(&self, other: &Hit) -> bool {
        self.switch.path == other.switch.path
            && self.key_path == other.key_path
            && self.key == other.key
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
            DisableReason::PluginDisabled => (
                ReasonCode::PluginDisabled,
                explain::plugin_disabled(self.key, &holds, agent_name),
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
        let per_project = self.per_project();
        match self.mechanism {
            DisableMechanism::JsonNameList(_) => explain::in_list(
                self.key,
                self.key_path.last().expect("validation requires a key"),
                &self.switch.path,
                per_project,
            ),
            DisableMechanism::JsonField(_) => {
                explain::field_holds(&self.key_path, self.value, &self.switch.path, per_project)
            }
        }
    }

    /// Whether the switch applies in the context's Project only.
    fn per_project(&self) -> bool {
        self.mechanism.context() == MechanismContext::Project
    }

    /// The edits that lift the switch.
    pub fn enable_edits(&self) -> Vec<Edit> {
        self.mechanism
            .enable_edits(self.key_path.clone(), self.value, self.key)
    }

    /// What Enable writes to lift the switch, as a sentence.
    pub fn enable_sentence(&self) -> String {
        let per_project = self.per_project();
        explain::enable_switch(
            &self.enable_edits(),
            self.key,
            &self.switch.path,
            per_project,
        )
    }
}

/// The switches of the mechanisms covering `entry` that are set for it in
/// the context of the Project with the key `project`: those that Disable
/// it, and the partial switches. Each in the definition's order. Of the
/// layered switches, only the first that holds a value for the Entry counts.
pub(crate) fn hits<'a>(
    definition: &'a AgentDefinition,
    snapshot: &'a Snapshot,
    entry: &'a ScannedEntry,
    project: Option<&str>,
) -> (Vec<Hit<'a>>, Vec<Hit<'a>>) {
    let location = definition.location(&entry.location);
    let mut disabling = Vec::new();
    let mut partial = Vec::new();
    // Whether a layered switch already holds a value for the Entry, which
    // the later layers then leave as it is.
    let mut layer_decided = false;
    for (index, mechanism) in definition.rules(location.kind).disable.iter().enumerate() {
        if !mechanism.covers(&location.id) || (mechanism.layered() && layer_decided) {
            continue;
        }
        let switches = snapshot
            .switches_of(entry, location.kind, index)
            .filter(|switch| switch.project.is_none() || switch.project.as_deref() == project);
        let Some(key) = switch_key(mechanism, entry) else {
            continue;
        };
        for switch in switches {
            let value = switch.value_for(key);
            layer_decided |= mechanism.layered() && value.is_some();
            let hit = Hit {
                mechanism,
                switch,
                key_path: switch.key_path_for(key),
                value,
                key,
            };
            if mechanism.disables(value, key) {
                disabling.push(hit);
            } else if mechanism.partial(value) {
                partial.push(hit);
            }
        }
    }
    (disabling, partial)
}

/// What the switch of `mechanism` holds `entry` by: the id of the plugin
/// it is part of, for a switch keyed by plugin, and its name otherwise.
/// `None` for an Entry of no plugin under a switch keyed by plugin.
fn switch_key<'e>(mechanism: &DisableMechanism, entry: &'e ScannedEntry) -> Option<&'e str> {
    if mechanism.switch().plugins() {
        entry.plugin.as_ref().map(|plugin| plugin.id.as_str())
    } else {
        Some(&entry.name)
    }
}

/// What Enable writes to lift `disabling`, the switches that Disable an
/// Entry of `location`, as a sentence. Or why it is not offered:
/// config-digger only reads the Entry, or one of them is a switch it only
/// reads.
pub(crate) fn enable(location: &LocationDefinition, disabling: &[Hit]) -> Result<String, String> {
    if location.provenance.read_only() {
        return Err(explain::read_only_enable());
    }
    if let Some(hit) = disabling.iter().find(|hit| !hit.mechanism.enable_offered()) {
        return Err(explain::enable_not_offered(&hit.holds()));
    }
    let sentences: Vec<String> = disabling.iter().map(Hit::enable_sentence).collect();
    Ok(sentences.join(" "))
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
    if location.provenance.read_only() {
        return Err((DisableNotOffered::ReadOnly, explain::read_only_option()));
    }
    let roots = snapshot.roots(entry.agent_index, &entry.home);
    // An Entry of a plugin is Disabled through the plugin's own switch only.
    let in_plugin = entry.plugin.is_some();
    let mut mechanisms: Vec<(usize, &DisableMechanism)> = definition
        .rules(location.kind)
        .disable
        .iter()
        .enumerate()
        .filter(|(_, mechanism)| {
            mechanism.covers(&location.id)
                && mechanism.enable_offered()
                && (mechanism.target() == MechanismTarget::Plugin) == in_plugin
        })
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
        let mut places = snapshot
            .switches_of(entry, location.kind, index)
            .filter(|place| place.path == path && place.project.as_deref() == project_key);
        // Under several spellings of one Project key, the one whose switch
        // exists already, or else the first the file holds.
        let first = places.clone().next();
        let Some(place) = places.find(|place| place.value.is_some()).or(first) else {
            // The Project folder is gone, or the Agent has not recorded the
            // Project in the file that keys the switch by it.
            unrecorded = true;
            continue;
        };
        let Some(key) = switch_key(mechanism, entry) else {
            continue;
        };
        let key_path = place.key_path_for(key);
        let value = place.value_for(key);
        let edit = mechanism.disable_edit(key_path, value, key);
        let per_project = mechanism.context() == MechanismContext::Project;
        let mut sentence = explain::disable_switch(&edit, key, &path, per_project);
        if mechanism.target() == MechanismTarget::Plugin {
            sentence.push(' ');
            sentence.push_str(&explain::takes_whole_plugin(key));
        }
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

/// The files of the switches that could Disable `entry` in the context of
/// `project`, or of none, and cannot be read. The Entry is resolved as if
/// they set nothing, which the Agent's defaults say.
pub(crate) fn failed_switch_files(
    definition: &AgentDefinition,
    snapshot: &Snapshot,
    entry: &ScannedEntry,
    project: Option<&ContextProject>,
) -> Vec<PathBuf> {
    let location = definition.location(&entry.location);
    let roots = snapshot.roots(entry.agent_index, &entry.home);
    let mut files: Vec<PathBuf> = Vec::new();
    for mechanism in &definition.rules(location.kind).disable {
        if !mechanism.covers(&location.id) {
            continue;
        }
        let switch = mechanism.switch();
        let folder = match (mechanism.context(), project) {
            (MechanismContext::Project, None) => continue,
            (MechanismContext::Project, Some(project)) if switch.anchor.in_project() => {
                Some(project.path)
            }
            _ => None,
        };
        let Some(path) = switch.anchor.path(roots, folder) else {
            continue;
        };
        if snapshot.unreadable_switches.contains(&path) && !files.contains(&path) {
            files.push(path);
        }
    }
    files
}
