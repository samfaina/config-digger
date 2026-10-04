//! Disable mechanisms at work: which switches Disable an Entry in a
//! context, which one Disable would write for it, or why it is not offered,
//! and what Enable lifts. Disable never writes a git-versioned file, so a
//! switch in one is skipped for the next mechanism.

use crate::api::{DisableNotOffered, DisableOption, Reason, ReasonCode};
use crate::definitions::blocks::{
    DisableReason, Disables, MechanismContext, MechanismTarget, Switched,
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
    pub mechanism: Switched<'a>,
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
            Switched::NameList(_) => explain::in_list(
                self.key,
                self.key_path.last().expect("validation requires a key"),
                &self.switch.path,
                per_project,
            ),
            Switched::Field(block) => explain::field_holds(
                &self.key_path,
                self.value,
                block.default_value.as_ref(),
                &self.switch.path,
                per_project,
            ),
            Switched::ItemList(_) => explain::item_in_list(
                self.key,
                self.value.unwrap_or(&Value::Null),
                &self.key_path[self.key_path.len().saturating_sub(2)],
                &self.switch.path,
                per_project,
            ),
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
    set_for(definition, snapshot, entry, project, false)
}

/// The switches Enable lifts for `entry` in the context of the Project
/// with the key `project`: those that Disable it, and when Enable takes a
/// layered switch away, those of the later layers too, so that none
/// Disables it again, as VS Code takes a server
/// out of both its workspace's and its profile's `mcp.enablement`.
pub(crate) fn lifted_by_enable<'a>(
    definition: &'a AgentDefinition,
    snapshot: &'a Snapshot,
    entry: &'a ScannedEntry,
    project: Option<&str>,
) -> Vec<Hit<'a>> {
    set_for(definition, snapshot, entry, project, true).0
}

/// The switches set for `entry`, as [`hits`] finds them, or with
/// `every_layer`, as [`lifted_by_enable`] finds them.
fn set_for<'a>(
    definition: &'a AgentDefinition,
    snapshot: &'a Snapshot,
    entry: &'a ScannedEntry,
    project: Option<&str>,
    every_layer: bool,
) -> (Vec<Hit<'a>>, Vec<Hit<'a>>) {
    let location = definition.location(&entry.location);
    let mut disabling = Vec::new();
    let mut partial = Vec::new();
    // Whether a layered switch already holds a value for the Entry, which
    // the later layers then leave as it is.
    let mut layer_decided = false;
    let switched = definition
        .rules(location.kind)
        .disable
        .iter()
        .enumerate()
        .filter_map(|(index, mechanism)| Some((index, mechanism.switched()?)));
    for (index, mechanism) in switched {
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
            // Enable taking a switch away leaves the next layer to decide.
            let unset = every_layer && mechanism.enable_unsets() && mechanism.disables(value, key);
            layer_decided |= mechanism.layered() && value.is_some() && !unset;
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
/// it is part of, for a switch keyed by plugin, the id its location's
/// `switch_id` makes, for a switch per Entry of a location that has one, and
/// its name otherwise. `None` for an Entry of no plugin under a switch keyed by
/// plugin, or one whose id cannot be made.
fn switch_key<'e>(mechanism: Switched, entry: &'e ScannedEntry) -> Option<&'e str> {
    if mechanism.switch().plugins() {
        entry.plugin.as_ref().map(|plugin| plugin.id.as_str())
    } else if mechanism.target() == MechanismTarget::Entry
        && (entry.switch_id.is_some() || mechanism.by_id())
    {
        entry.switch_id.as_deref()
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

/// What Disable does to an Entry: write a switch, or park the Entry.
pub(crate) enum Choice {
    /// It makes `edits` to the switch in the file at `path`, a SQLite store
    /// when `sqlite`.
    Switch {
        path: PathBuf,
        edits: Vec<Edit>,
        sqlite: bool,
        /// What Disable writes, as a sentence.
        sentence: String,
    },
    /// It moves the Entry into the parked-Entry store at `store`.
    Park {
        store: PathBuf,
        /// What Disable does, as a sentence.
        sentence: String,
    },
}

/// What Disable would do to `entry` in the context of `project`.
pub(crate) fn option(
    definition: &AgentDefinition,
    snapshot: &Snapshot,
    entry: &ScannedEntry,
    project: Option<&ContextProject>,
) -> DisableOption {
    match choose(definition, snapshot, entry, project) {
        Ok(Choice::Switch { path, sentence, .. }) => DisableOption::Offered { path, sentence },
        Ok(Choice::Park { store, sentence }) => DisableOption::Offered {
            path: store,
            sentence,
        },
        Err((code, sentence)) => DisableOption::NotOffered { code, sentence },
    }
}

/// What Disable does to `entry` in the context of `project`: it writes the
/// switch of the most specific mechanism (target, then context) that covers
/// the Entry and applies in the context, the first in the definition's order
/// among equally specific ones, skipping any whose switch file is
/// git-versioned or cannot be read. With none left, and none skipped for a
/// file that cannot be read or a Project the context lacks, it parks the
/// Entry when a park mechanism covers it and its file is not git-versioned.
/// Or why Disable is not offered.
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
    // An Entry of a plugin is Disabled through the plugin's own switch only.
    let in_plugin = entry.plugin.is_some();
    let mechanisms = &definition.rules(location.kind).disable;
    let mut switched: Vec<(usize, Switched)> = mechanisms
        .iter()
        .enumerate()
        .filter_map(|(index, mechanism)| Some((index, mechanism.switched()?)))
        .filter(|(_, mechanism)| {
            mechanism.covers(&location.id)
                && mechanism.enable_offered()
                && (mechanism.target() == MechanismTarget::Plugin) == in_plugin
        })
        .collect();
    switched.sort_by_key(|(index, mechanism)| (mechanism.target(), mechanism.context(), *index));

    let mut versioned: Vec<PathBuf> = Vec::new();
    let mut unreadable: Vec<PathBuf> = Vec::new();
    let mut needs_project = false;
    let mut unrecorded = false;
    for (index, mechanism) in switched {
        let switch = mechanism.switch();
        let project_key = match (mechanism.context(), project) {
            (MechanismContext::Project, None) => {
                needs_project = true;
                continue;
            }
            (MechanismContext::Project, Some(project)) => Some(project.key),
            (MechanismContext::All, _) => None,
        };
        let context = project.map(|project| (project.key, project.path));
        let Some(path) = snapshot.file_for(entry, &switch.anchor, context) else {
            // The Agent keeps no folder for the Project yet.
            unrecorded |= switch.anchor.project_storage().is_some();
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
        let edits = mechanism.disable_edits(key_path, value, key);
        let per_project = mechanism.context() == MechanismContext::Project;
        let mut sentence = explain::disable_switch(&edits, key, &path, per_project);
        if mechanism.target() == MechanismTarget::Plugin {
            sentence.push(' ');
            sentence.push_str(&explain::takes_whole_plugin(key));
        }
        return Ok(Choice::Switch {
            path,
            edits,
            sqlite: switch.sqlite(),
            sentence,
        });
    }
    let agent = &definition.agent.name;
    // A switch the Agent has here, but that cannot be written now, is not a
    // reason to park the Entry instead.
    let parks = !in_plugin
        && unreadable.is_empty()
        && !needs_project
        && !unrecorded
        && mechanisms
            .iter()
            .any(|mechanism| mechanism.parks() && mechanism.covers(&location.id));
    if parks {
        // Moving an Entry out of a git-versioned file reaches every clone.
        if snapshot.versioned.contains(&entry.path) {
            versioned.push(entry.path.clone());
        } else if let Some(store) = &snapshot.parked_store {
            return Ok(Choice::Park {
                sentence: explain::disable_parks(&entry.name, &entry.path, store, agent),
                store: store.clone(),
            });
        }
    }
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
    let mut files: Vec<PathBuf> = Vec::new();
    for mechanism in &definition.rules(location.kind).disable {
        let Some(mechanism) = mechanism
            .switched()
            .filter(|mechanism| mechanism.covers(&location.id))
        else {
            continue;
        };
        let switch = mechanism.switch();
        if mechanism.context() == MechanismContext::Project && project.is_none() {
            continue;
        }
        let context = project.map(|project| (project.key, project.path));
        let Some(path) = snapshot.file_for(entry, &switch.anchor, context) else {
            continue;
        };
        if snapshot.unreadable_switches.contains(&path) && !files.contains(&path) {
            files.push(path);
        }
    }
    files
}
