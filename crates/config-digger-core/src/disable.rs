//! Disable mechanisms at work: which switches Disable an Entry in a
//! context, which one Disable would write for it, or why it is not offered,
//! and what Enable lifts. Disable never writes a git-versioned file, so a
//! switch in one is skipped for the next mechanism.

use crate::api::{DisableNotOffered, DisableOption, Reason, ReasonCode, State};
use crate::definitions::blocks::{
    DisableMechanism, DisableReason, Disables, Holds, JsonPathRules, MechanismContext,
    MechanismTarget, Merge, MergedField, NAME_KEY, OWN_NAME_KEY, PartialLimit, Switched,
};
use crate::definitions::{AgentDefinition, LocationDefinition};
use crate::edit::{Edit, Syntax};
use crate::explain;
use crate::locator;
use crate::mask::MASK;
use crate::resolve;
use crate::scan::{ScannedEntry, ScannedSwitch, Snapshot};
use serde_json::Value;
use std::borrow::Cow;
use std::collections::BTreeSet;
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
    pub key: Cow<'a, str>,
    /// The folder of the context's Project, which path rules are read for.
    pub folder: Option<&'a Path>,
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

    /// The State the switch gives the Entry: Blocked for managed policy,
    /// Disabled otherwise.
    pub fn state(&self) -> State {
        if self.mechanism.policy() {
            State::Blocked
        } else {
            State::Disabled
        }
    }

    /// Why the Entry is Disabled, or Blocked by policy, as its State's reason.
    pub fn reason(&self, agent_name: &str) -> Reason {
        let holds = self.holds();
        if self.mechanism.policy() {
            return Reason {
                code: ReasonCode::Policy,
                effective: None,
                switch: Some(self.locator()),
                sentence: explain::switched_off_by_policy(self.plugin(), &holds, agent_name),
            };
        }
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
                explain::plugin_disabled(&self.key, &holds, agent_name),
            ),
        };
        Reason {
            code,
            effective: None,
            switch: Some(self.locator()),
            sentence,
        }
    }

    /// The id of the plugin the switch holds the Entry by, for a switch of
    /// a whole plugin.
    fn plugin(&self) -> Option<&str> {
        (self.mechanism.target() == MechanismTarget::Plugin).then_some(&*self.key)
    }

    /// The note on the Entry the switch limits part of, as a partial switch.
    pub fn partial_note(&self, agent_name: &str) -> String {
        match self.mechanism {
            Switched::ItemList(block) if block.partial == Some(PartialLimit::HiddenFromUser) => {
                explain::hidden_from_user(&self.holds(), agent_name)
            }
            _ => explain::partial_switch(&self.holds(), agent_name),
        }
    }

    /// What the switch holds, as a sentence says it.
    pub fn holds(&self) -> String {
        let per_project = self.per_project();
        match self.mechanism {
            // An item of a partial list says no more than that it is there.
            Switched::ItemList(block) if block.partial.is_some() => explain::in_list(
                &self.key,
                &self.key_path[self.key_path.len().saturating_sub(2)],
                &self.switch.path,
                per_project,
            ),
            Switched::NameList(_) => explain::in_list(
                &self.key,
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
                &self.key,
                self.value.unwrap_or(&Value::Null),
                &self.key_path[self.key_path.len().saturating_sub(2)],
                &self.switch.path,
                per_project,
            ),
            Switched::PathRules(_) => explain::path_rule_holds(
                self.folder
                    .and_then(|folder| JsonPathRules::deciding(self.value, folder))
                    .unwrap_or_default(),
                &self.key_path,
                &self.switch.path,
                self.folder,
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
            .enable_edits(self.key_path.clone(), self.value, &self.key, self.folder)
    }

    /// What Enable writes to lift the switch, as a sentence.
    pub fn enable_sentence(&self) -> String {
        if let Switched::PathRules(_) = self.mechanism {
            return explain::enable_path_rules(&self.enable_edits(), &self.switch.path);
        }
        let per_project = self.per_project();
        explain::enable_switch(
            &self.enable_edits(),
            &self.key,
            &self.switch.path,
            per_project,
        )
    }
}

/// The switches of the mechanisms covering `entry` that are set for it in
/// the context of the Project with the key `project`: those that Disable
/// it, and the partial switches. Each in the definition's order. Of the
/// layered switches, only the first that holds a value for the Entry counts,
/// or when none does, the last one's default.
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
    // The last layered switch that holds nothing for the Entry, whose
    // default the Agent takes when no layer holds a value.
    let mut by_default: Option<Hit> = None;
    // The folder of the context's Project, which path rules are read for.
    let folder = project
        .and_then(|key| snapshot.project(key))
        .map(|project| project.path.as_path());
    let switched = definition
        .rules(location.kind)
        .disable
        .iter()
        .enumerate()
        .filter_map(|(index, mechanism)| Some((index, mechanism.switched()?)));
    for (index, mechanism) in switched {
        if !mechanism.covers(&location.id)
            || !mechanism.covers_plugin(plugin_scope(entry))
            || !switch_read(definition, snapshot, entry, mechanism, project)
            || (mechanism.layered() && layer_decided)
        {
            continue;
        }
        let switches = snapshot
            .switches_of(entry, location.kind, index)
            .filter(|switch| switch.project.is_none() || switch.project.as_deref() == project);
        let keys = switch_keys(mechanism, entry);
        for (switch, key) in switches.flat_map(|switch| keys.iter().map(move |key| (switch, key))) {
            // Of the files of one layer, such as merged managed settings,
            // the first that holds a value decides too.
            if mechanism.layered() && layer_decided {
                break;
            }
            let value = switch.value_for(key);
            // Enable taking a switch away leaves the next layer to decide.
            let unset =
                every_layer && mechanism.enable_unsets() && mechanism.disables(value, key, folder);
            layer_decided |= mechanism.layered() && value.is_some() && !unset;
            let hit = Hit {
                mechanism,
                switch,
                key_path: switch.key_path_for(key),
                value,
                key: key.clone(),
                folder,
            };
            // A layer that holds nothing leaves the next one to decide.
            if mechanism.layered() && value.is_none() {
                by_default = Some(hit);
                continue;
            }
            if mechanism.disables(value, key, folder) {
                disabling.push(hit);
            } else if mechanism.partial(value) {
                partial.push(hit);
            }
        }
    }
    if let Some(hit) = by_default.filter(|_| !layer_decided) {
        if hit.mechanism.disables(None, &hit.key, folder) {
            disabling.push(hit);
        } else if hit.mechanism.partial(None) {
            partial.push(hit);
        }
    }
    (disabling, partial)
}

/// The first switch of managed policy that holds a value for `entry` in
/// the context of the Project with the key `project`, forcing it on or off.
fn policy_hold<'a>(
    definition: &'a AgentDefinition,
    snapshot: &'a Snapshot,
    entry: &'a ScannedEntry,
    project: Option<&str>,
) -> Option<Hit<'a>> {
    let location = definition.location(&entry.location);
    let folder = project
        .and_then(|key| snapshot.project(key))
        .map(|project| project.path.as_path());
    definition
        .rules(location.kind)
        .disable
        .iter()
        .enumerate()
        .filter_map(|(index, mechanism)| Some((index, mechanism.switched()?)))
        .filter(|(_, mechanism)| {
            mechanism.policy()
                && mechanism.covers(&location.id)
                && mechanism.covers_plugin(plugin_scope(entry))
                && switch_read(definition, snapshot, entry, *mechanism, project)
        })
        .find_map(|(index, mechanism)| {
            let keys = switch_keys(mechanism, entry);
            snapshot
                .switches_of(entry, location.kind, index)
                .filter(|switch| switch.project.is_none() || switch.project.as_deref() == project)
                .find_map(|switch| {
                    keys.iter().find_map(|key| {
                        let value = switch.value_for(key)?;
                        Some(Hit {
                            mechanism,
                            switch,
                            key_path: switch.key_path_for(key),
                            value: Some(value),
                            key: key.clone(),
                            folder,
                        })
                    })
                })
        })
}

/// What the switch of `mechanism` holds `entry` by: the id of the plugin
/// it is part of, for a switch keyed by plugin alone, the id its location's
/// `switch_id` makes, for a switch per Entry of a location that has one, and
/// its name otherwise. `None` for an Entry of no plugin under a switch keyed by
/// plugin, or one whose id cannot be made. A switch of names in lower case
/// holds the name trimmed and in lower case.
fn switch_key<'e>(mechanism: Switched, entry: &'e ScannedEntry) -> Option<Cow<'e, str>> {
    if mechanism.switch().per_plugin() {
        entry
            .plugin
            .as_ref()
            .map(|plugin| plugin.id.as_str().into())
    } else if mechanism.target() == MechanismTarget::Entry
        && (entry.switch_id.is_some() || mechanism.by_id())
    {
        entry.switch_id.as_deref().map(Cow::from)
    } else if mechanism.switch().lower_names {
        Some(entry.name.trim().to_lowercase().into())
    } else {
        Some(entry.name.as_str().into())
    }
}

/// Every key the switch of `mechanism` holds `entry` under: the one
/// [`switch_key`] finds, which Disable writes, then each of its switch's
/// `also_names`, a key that comes out as another left out. None where
/// `switch_key` finds none.
fn switch_keys<'e>(mechanism: Switched, entry: &'e ScannedEntry) -> Vec<Cow<'e, str>> {
    let Some(key) = switch_key(mechanism, entry) else {
        return Vec::new();
    };
    let mut keys = vec![key];
    for template in &mechanism.switch().also_names {
        let key = template
            .replace(OWN_NAME_KEY, entry.own_name())
            .replace(NAME_KEY, &entry.name);
        if !keys.iter().any(|known| *known == key) {
            keys.push(key.into());
        }
    }
    keys
}

/// What Enable writes to lift `disabling`, the switches that Disable an
/// Entry of `location`, as a sentence. Or why it is not offered:
/// config-digger only reads the Entry, or one of them is a switch it only
/// reads.
pub(crate) fn enable(location: &LocationDefinition, disabling: &[Hit]) -> Result<String, String> {
    if location.provenance.read_only() {
        return Err(explain::read_only_enable());
    }
    if let Some(hit) = disabling.iter().find(|hit| hit.mechanism.policy()) {
        return Err(explain::policy_enable_not_offered(&hit.holds()));
    }
    if let Some(hit) = disabling.iter().find(|hit| !hit.mechanism.enable_offered()) {
        return Err(explain::enable_not_offered(&hit.holds()));
    }
    let sentences: Vec<String> = disabling.iter().map(Hit::enable_sentence).collect();
    Ok(sentences.join(" "))
}

/// What Disable does to an Entry: write a switch, or park the Entry.
pub(crate) enum Choice {
    /// It makes `edits` to the switch in the file at `path`, edited in
    /// `syntax`, a SQLite store when `sqlite`.
    Switch {
        path: PathBuf,
        edits: Vec<Edit>,
        syntax: Syntax,
        sqlite: bool,
        /// The switch holds the Entry by its name in every context, so that
        /// it reaches every Entry of that name wherever it is.
        by_name_everywhere: bool,
        /// What Disable writes, as a sentence.
        sentence: String,
        /// What the write does outside the context, as sentences the
        /// preview says too, for a write in a fallback's file.
        elsewhere: Vec<String>,
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
        Ok(Choice::Switch {
            path,
            sentence,
            elsewhere,
            ..
        }) => DisableOption::Offered {
            path,
            sentence: std::iter::once(sentence)
                .chain(elsewhere)
                .collect::<Vec<_>>()
                .join(" "),
        },
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
    if let Some(mirror) = snapshot.mirror_of(entry) {
        let sentence =
            explain::mirrored_option("Disable", &mirror.owner, mirror.source_folder.as_deref());
        return Err((DisableNotOffered::Mirrored, sentence));
    }
    if location.provenance.read_only() {
        return Err((DisableNotOffered::ReadOnly, explain::read_only_option()));
    }
    let context = project.map(|project| project.key);
    if let Some(hit) = policy_hold(definition, snapshot, entry, context) {
        let on = !hit.mechanism.disables(hit.value, &hit.key, hit.folder);
        let sentence = explain::policy_option(&hit.holds(), on, hit.plugin().is_some());
        return Err((DisableNotOffered::Policy, sentence));
    }
    let merged_field = definition
        .rules(location.kind)
        .disable
        .iter()
        .filter_map(DisableMechanism::merged_field)
        .find(|block| block.locations.contains(&location.id));
    if let Some(block) = merged_field {
        return through_merged_field(definition, snapshot, entry, project, block);
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
                && mechanism.covers_plugin(plugin_scope(entry))
                && switch_read(
                    definition,
                    snapshot,
                    entry,
                    *mechanism,
                    project.map(|project| project.key),
                )
                && mechanism.enable_offered()
                && mechanism.switch().plugins() == in_plugin
        })
        .collect();
    switched.sort_by_key(|(index, mechanism)| (mechanism.target(), mechanism.context(), *index));

    let mut versioned: Vec<PathBuf> = Vec::new();
    let mut unreadable: Vec<PathBuf> = Vec::new();
    let mut needs_project = false;
    // A switch of path rules needs a Project's folder to write a rule for.
    let mut needs_folder = false;
    let mut unrecorded = false;
    for (index, mechanism) in switched {
        let switch = mechanism.switch();
        let project_key = match (mechanism.context(), project) {
            (MechanismContext::Project, None) => {
                needs_project = true;
                needs_folder |= matches!(mechanism, Switched::PathRules(_));
                continue;
            }
            (MechanismContext::Project, Some(project)) if switch.per_project() => Some(project.key),
            // Path rules in a file for every Project are read for this one.
            (MechanismContext::Project, Some(_)) | (MechanismContext::All, _) => None,
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
        let key_path = place.key_path_for(&key);
        let value = place.value_for(&key);
        let folder = project.map(|project| project.path);
        let edits = mechanism.disable_edits(key_path, value, &key, folder);
        let per_project = mechanism.context() == MechanismContext::Project;
        let mut sentence = match mechanism {
            Switched::PathRules(_) => explain::disable_path_rule(&edits, &path),
            _ => explain::disable_switch(&edits, &key, &path, per_project),
        };
        if mechanism.target() == MechanismTarget::Plugin {
            sentence.push(' ');
            sentence.push_str(&explain::takes_whole_plugin(&key));
        }
        return Ok(Choice::Switch {
            path,
            edits,
            syntax: switch.syntax(),
            sqlite: switch.sqlite(),
            by_name_everywhere: by_name_everywhere(mechanism),
            sentence,
            elsewhere: Vec::new(),
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
    } else if needs_folder {
        (
            DisableNotOffered::NoMechanism,
            explain::disable_needs_folder(agent),
        )
    } else {
        (
            DisableNotOffered::NoMechanism,
            explain::no_disable_mechanism(agent, needs_project, unrecorded),
        )
    })
}

/// A table Disable could write a merged field in.
struct FieldTable<'d> {
    path: PathBuf,
    key_path: Vec<String>,
    /// How the Agent ranks its layer, as a merge orders Entries: lower is
    /// higher, the deepest Project layer first.
    rank: (usize, usize),
    location: &'d LocationDefinition,
    /// It is in the fallback's file, not the Entry's own table.
    fallback: bool,
}

/// The table of the fallback's file where Disable writes a Stand-in.
struct StandInTable<'d> {
    path: PathBuf,
    key_path: Vec<String>,
    layer: &'d LocationDefinition,
    /// The fields that fix the server's transport, one of which it holds.
    transports: &'d [String],
    /// The field that turns the server off.
    switch: &'d str,
}

/// What Disable writes for `entry`, an Entry of a server its Agent builds
/// from same-named Entries, through the field `block` writes: `false` in
/// the Entry's own table, or when that file is git-versioned or read only,
/// in the fallback's file: in its table of the Entry's name, or else in a
/// Stand-in when the block declares one, and else in a table holding the
/// field alone. A write counts only where no higher layer sets the field,
/// when the block requires it, and none goes in a git-versioned file.
fn through_merged_field(
    definition: &AgentDefinition,
    snapshot: &Snapshot,
    entry: &ScannedEntry,
    project: Option<&ContextProject>,
    block: &MergedField,
) -> Result<Choice, (DisableNotOffered, String)> {
    let location = definition.location(&entry.location);
    let merge = definition
        .rules(location.kind)
        .merge
        .as_ref()
        .expect("validation gives a merged field a merge");
    let field = merge
        .enabled()
        .expect("validation gives a merged field's merge an `enabled` field");
    let rank = |entry: &ScannedEntry| layer_rank(definition, entry);
    let setters = setters(
        definition,
        snapshot,
        entry,
        project.map(|project| project.key),
        merge,
        field,
    );
    let highest_above = |at: (usize, usize)| {
        setters
            .iter()
            .copied()
            .filter(|setter| rank(setter) < at)
            .min_by_key(|setter| rank(setter))
    };
    let mut targets = vec![FieldTable {
        path: entry.path.clone(),
        key_path: entry.key_path.clone(),
        rank: rank(entry),
        location,
        fallback: false,
    }];
    if let Some(fallback) = block.fallback.as_deref().map(|id| definition.location(id))
        && let Some(path) = snapshot.file_for(entry, &fallback.anchor, None)
    {
        let name = &entry.key_path[location.root.len()..];
        targets.push(FieldTable {
            path,
            key_path: fallback.root.iter().chain(name).cloned().collect(),
            rank: (definition.rank(fallback), 0),
            location: fallback,
            fallback: true,
        });
    }
    let mut versioned: Vec<PathBuf> = Vec::new();
    let mut set_above: Option<&ScannedEntry> = None;
    for target in targets {
        let FieldTable {
            path,
            mut key_path,
            rank,
            location: layer,
            fallback,
        } = target;
        if snapshot.versioned.contains(&path) {
            if !versioned.contains(&path) {
                versioned.push(path);
            }
            continue;
        }
        if layer.provenance.read_only() {
            continue;
        }
        if let Some(above) = highest_above(rank).filter(|_| block.requires_unset_above) {
            set_above.get_or_insert(above);
            continue;
        }
        let agent = &definition.agent.name;
        let declared = fallback
            && snapshot.entries.iter().any(|other| {
                other.agent_index == entry.agent_index
                    && other.home == entry.home
                    && other.location == layer.id
                    && other.path == path
                    && other.key_path == key_path
            });
        if let Some(transports) = block.stand_in.as_deref().filter(|_| fallback && !declared) {
            let table = StandInTable {
                path,
                key_path,
                layer,
                transports,
                switch: field,
            };
            return stand_in(definition, snapshot, entry, project, merge, table);
        }
        key_path.push(field.to_owned());
        let edits = vec![Edit::Set {
            key_path,
            value: Value::Bool(false),
        }];
        let sentence = format!(
            "{} {}",
            explain::disable_switch(&edits, &entry.name, &path, false),
            explain::turns_off_built(agent, &entry.name)
        );
        // Validation keeps a fallback out of Projects, so every folder reads
        // its file.
        let elsewhere = if fallback {
            vec![explain::turns_off_everywhere(
                agent,
                &entry.name,
                field,
                &path,
            )]
        } else {
            Vec::new()
        };
        return Ok(Choice::Switch {
            path,
            edits,
            syntax: merged_syntax(layer),
            sqlite: false,
            by_name_everywhere: false,
            sentence,
            elsewhere,
        });
    }
    let agent = &definition.agent.name;
    Err(match set_above {
        Some(above) => {
            let at = locator::render(&above.path, &field_of(above, field));
            let code = if snapshot.versioned.contains(&above.path) {
                DisableNotOffered::GitVersioned
            } else {
                DisableNotOffered::NoMechanism
            };
            (
                code,
                explain::merged_field_set_above(
                    agent,
                    field,
                    &at,
                    code == DisableNotOffered::GitVersioned,
                ),
            )
        }
        None => (
            DisableNotOffered::GitVersioned,
            explain::disable_only_in_versioned(agent, &versioned, false),
        ),
    })
}

/// The Stand-in Disable writes for `entry` in `table`: the first of its
/// fields the server has, as the highest layer that gives it one has it,
/// and the field that turns it off. Or why Disable is not offered: no layer
/// gives the server one of the fields, or a known Project declares it with
/// another of them, where the Agent would stop starting.
fn stand_in(
    definition: &AgentDefinition,
    snapshot: &Snapshot,
    entry: &ScannedEntry,
    project: Option<&ContextProject>,
    merge: &Merge,
    table: StandInTable,
) -> Result<Choice, (DisableNotOffered, String)> {
    let agent = &definition.agent.name;
    let context = project.map(|project| project.key);
    let transport = server_entries(definition, snapshot, entry, context, merge)
        .into_iter()
        .find_map(|layer| {
            table.transports.iter().find_map(|field| {
                let value = layer.config.get(field)?.as_str()?;
                Some((layer, field, stand_in_value(value)?))
            })
        });
    let Some((source, transport, value)) = transport else {
        return Err((
            DisableNotOffered::NoMechanism,
            explain::stand_in_without_transport(agent, &entry.name, table.transports),
        ));
    };
    // Every known Project, trusted or not: trusting it later would stop the
    // Agent there.
    let others: Vec<&ScannedEntry> = declaring_projects(definition, snapshot, entry, merge)
        .into_iter()
        .filter(|other| other.project.as_deref() != context)
        .collect();
    let stopped = folders_of(
        snapshot,
        others.iter().copied().filter(|other| {
            table
                .transports
                .iter()
                .any(|field| field != transport && other.config.get(field).is_some())
        }),
    );
    if !stopped.is_empty() {
        return Err((
            DisableNotOffered::StopsAgent,
            explain::stand_in_stops_agent(agent, &entry.name, transport, &stopped),
        ));
    }
    // A Project whose own layer sets the switch decides it there.
    let deciding = folders_of(
        snapshot,
        others
            .iter()
            .copied()
            .filter(|other| other.config.get(table.switch).is_some()),
    );
    let mut turned_off = folders_of(snapshot, others.iter().copied());
    turned_off.retain(|folder| !deciding.contains(folder));
    let mut stand_in = serde_json::Map::new();
    stand_in.insert(transport.clone(), Value::String(value));
    stand_in.insert(table.switch.to_owned(), Value::Bool(false));
    let edits = vec![Edit::Set {
        key_path: table.key_path,
        value: Value::Object(stand_in),
    }];
    let sentence = format!(
        "{} {}",
        explain::disable_stand_in(&entry.name, transport, table.switch, &table.path),
        explain::turns_off_built(agent, &entry.name)
    );
    let mut elsewhere = vec![explain::stand_in_reach(agent, &entry.name, &turned_off)];
    // The transport it copies is the Project layer's only when it comes
    // from one.
    if let Some(project) = project.filter(|_| source.project.is_some()) {
        elsewhere.push(explain::stand_in_drift(
            agent,
            &entry.name,
            &source.path,
            project.path,
        ));
    }
    Ok(Choice::Switch {
        path: table.path,
        edits,
        syntax: merged_syntax(table.layer),
        sqlite: false,
        by_name_everywhere: false,
        sentence,
        elsewhere,
    })
}

/// `value` as a Stand-in holds it: a URL without its userinfo, query and
/// fragment, where Secrets go. `None` when it holds a masked Secret still.
fn stand_in_value(value: &str) -> Option<String> {
    let mut value = value.to_owned();
    if let Some(start) = value.find("://").map(|at| at + "://".len()) {
        if let Some(end) = value[start..].find(['?', '#']) {
            value.truncate(start + end);
        }
        let authority_end = value[start..]
            .find('/')
            .map_or(value.len(), |slash| start + slash);
        if let Some(at) = value[start..authority_end].rfind('@') {
            value.replace_range(start..=start + at, "");
        }
    }
    (!value.contains(MASK)).then_some(value)
}

/// The Entries of the Item of `entry` that `merge` merges, in the layers of
/// any known Project: those of every Project config-digger scans for its
/// Agent home.
fn declaring_projects<'s>(
    definition: &AgentDefinition,
    snapshot: &'s Snapshot,
    entry: &ScannedEntry,
    merge: &Merge,
) -> Vec<&'s ScannedEntry> {
    snapshot
        .entries
        .iter()
        .filter(|other| other.project.is_some() && same_server(definition, entry, other, merge))
        .collect()
}

/// Whether `other` is an Entry of the server of `entry` that `merge`
/// merges, in any context.
fn same_server(
    definition: &AgentDefinition,
    entry: &ScannedEntry,
    other: &ScannedEntry,
    merge: &Merge,
) -> bool {
    other.agent_index == entry.agent_index
        && other.home == entry.home
        && other.name == entry.name
        && definition.location(&other.location).kind == definition.location(&entry.location).kind
        && merge.covers(&other.location)
}

/// The folders of the Projects of `entries`, sorted, each once.
fn folders_of<'s>(
    snapshot: &Snapshot,
    entries: impl Iterator<Item = &'s ScannedEntry>,
) -> Vec<PathBuf> {
    let folders: BTreeSet<PathBuf> = entries
        .filter_map(|other| snapshot.project(other.project.as_deref()?))
        .map(|project| project.path.clone())
        .collect();
    folders.into_iter().collect()
}

/// Whether, of `server`, the Entries of a server in one context, a Project
/// layer gives it a transport: a Stand-in for it stands in for that one.
fn stands_in_here(definition: &AgentDefinition, server: &[&ScannedEntry]) -> bool {
    server
        .iter()
        .any(|other| other.project.is_some() && gives_transport(definition, other))
}

/// The merged-field block of the Item kind of `entry`, when its Agent has
/// one.
fn merged_field<'d>(
    definition: &'d AgentDefinition,
    entry: &ScannedEntry,
) -> Option<&'d MergedField> {
    let kind = definition.location(&entry.location).kind;
    definition
        .rules(kind)
        .disable
        .iter()
        .find_map(DisableMechanism::merged_field)
}

/// Whether `entry` is a Stand-in, told by its shape whoever wrote it: a
/// table of the fallback of its merged-field block, when the block declares
/// a Stand-in, that holds nothing but `false` in the field that turns the
/// server off and one of the block's transport fields.
pub(crate) fn is_stand_in(definition: &AgentDefinition, entry: &ScannedEntry) -> bool {
    let Some(block) = merged_field(definition, entry) else {
        return false;
    };
    let (Some(fields), Some(fallback)) = (&block.stand_in, &block.fallback) else {
        return false;
    };
    let kind = definition.location(&entry.location).kind;
    let Some(field) = definition
        .rules(kind)
        .merge
        .as_ref()
        .and_then(Merge::enabled)
    else {
        return false;
    };
    let Some(table) = entry.config.as_object() else {
        return false;
    };
    entry.location == *fallback
        && table.len() == 2
        && table.get(field) == Some(&Value::Bool(false))
        && fields.iter().any(|transport| table.contains_key(transport))
}

/// For a Stand-in seen in the context of the Project with the key
/// `project`, where no layer of the server it names gives a transport, the
/// known Projects it stands in for: there it is a server of its own. `None`
/// for any other Entry, and for a Stand-in in a Project it stands in for or
/// with no known Project to stand in for, which is the user's own server.
pub(crate) fn stand_in_outside(
    definition: &AgentDefinition,
    snapshot: &Snapshot,
    entry: &ScannedEntry,
    project: Option<&str>,
) -> Option<Vec<PathBuf>> {
    if !is_stand_in(definition, entry) {
        return None;
    }
    let kind = definition.location(&entry.location).kind;
    let merge = definition.rules(kind).merge.as_ref()?;
    let server = server_entries(definition, snapshot, entry, project, merge);
    let in_project = stands_in_here(definition, &server);
    let projects = stood_in_for(definition, snapshot, entry);
    (!in_project && !projects.is_empty()).then_some(projects)
}

/// Whether `entry` holds one of the transport fields of its merged-field
/// block's Stand-in.
fn gives_transport(definition: &AgentDefinition, entry: &ScannedEntry) -> bool {
    merged_field(definition, entry)
        .and_then(|block| block.stand_in.as_ref())
        .is_some_and(|fields| fields.iter().any(|field| entry.config.get(field).is_some()))
}

/// The folders of the known Projects a Stand-in such as `entry` stands in
/// for: those whose layers give the server it names a transport, sorted.
pub(crate) fn stood_in_for(
    definition: &AgentDefinition,
    snapshot: &Snapshot,
    entry: &ScannedEntry,
) -> Vec<PathBuf> {
    let kind = definition.location(&entry.location).kind;
    let Some(merge) = definition.rules(kind).merge.as_ref() else {
        return Vec::new();
    };
    let declaring = declaring_projects(definition, snapshot, entry, merge);
    folders_of(
        snapshot,
        declaring
            .into_iter()
            .filter(|other| gives_transport(definition, other)),
    )
}

/// The folders of the known Projects a Remove of the Stand-in `entry` turns
/// its server back on in: those it stands in for, but for any whose own
/// layer sets the field that turns the server off, which decides there.
pub(crate) fn freed_by_remove(
    definition: &AgentDefinition,
    snapshot: &Snapshot,
    entry: &ScannedEntry,
) -> Vec<PathBuf> {
    let kind = definition.location(&entry.location).kind;
    let Some((merge, field)) = definition
        .rules(kind)
        .merge
        .as_ref()
        .and_then(|merge| Some((merge, merge.enabled()?)))
    else {
        return Vec::new();
    };
    let deciding = folders_of(
        snapshot,
        declaring_projects(definition, snapshot, entry, merge)
            .into_iter()
            .filter(|other| other.config.get(field).is_some()),
    );
    let mut folders = stood_in_for(definition, snapshot, entry);
    folders.retain(|folder| !deciding.contains(folder));
    folders
}

/// How the Agent ranks the layer of `entry`, as a merge orders Entries:
/// lower is higher, the deepest Project layer first.
fn layer_rank(definition: &AgentDefinition, entry: &ScannedEntry) -> (usize, usize) {
    let location = definition.location(&entry.location);
    (definition.rank(location), entry.depth)
}

/// The Entries of the Item of `entry` that `merge` merges in the context of
/// the Project with the key `project` and that set `field`, highest first.
fn setters<'s>(
    definition: &AgentDefinition,
    snapshot: &'s Snapshot,
    entry: &ScannedEntry,
    project: Option<&str>,
    merge: &Merge,
    field: &str,
) -> Vec<&'s ScannedEntry> {
    let mut setters = server_entries(definition, snapshot, entry, project, merge);
    setters.retain(|setter| setter.config.get(field).is_some());
    setters
}

/// The Entries of the Item of `entry` that `merge` merges in the context of
/// the Project with the key `project`, highest first.
fn server_entries<'s>(
    definition: &AgentDefinition,
    snapshot: &'s Snapshot,
    entry: &ScannedEntry,
    project: Option<&str>,
    merge: &Merge,
) -> Vec<&'s ScannedEntry> {
    let mut entries: Vec<&ScannedEntry> = snapshot
        .entries
        .iter()
        .filter(|other| {
            same_server(definition, entry, other, merge)
                && (other.project.is_none() || other.project.as_deref() == project)
                && resolve::gates(definition, snapshot, other, project).is_empty()
        })
        .collect();
    entries.sort_by_key(|other| layer_rank(definition, other));
    entries
}

/// The key path of `field` in the table of `entry`.
fn field_of(entry: &ScannedEntry, field: &str) -> Vec<String> {
    let mut key_path = entry.key_path.clone();
    key_path.push(field.to_owned());
    key_path
}

/// The syntax a file of `layer` is edited in, which merges its Entries.
fn merged_syntax(layer: &LocationDefinition) -> Syntax {
    layer
        .format
        .file()
        .expect("validation merges Entries of files only")
        .syntax()
}

/// A `false` of the field that turns off the server an Agent builds from
/// same-named Entries, which Enable deletes.
pub(crate) struct FalseField {
    pub path: PathBuf,
    /// The field's key path in its file.
    pub key_path: Vec<String>,
    pub syntax: Syntax,
}

impl FalseField {
    pub fn edit(&self) -> Edit {
        Edit::Remove {
            key_path: self.key_path.clone(),
        }
    }
}

/// The `false` values of the field a merged-field Disable writes that
/// Enable deletes for `entry` in the context of the Project with the key
/// `project`, so that none turns the server its Agent builds off again:
/// that of the highest layer that sets the field, and each lower one down to
/// the first that sets it otherwise. Empty when the server is not turned
/// off. Or why Enable is not offered: one of them is in a git-versioned
/// file, or in a layer config-digger only reads, or it only reads `entry`.
pub(crate) fn deleted_by_enable(
    definition: &AgentDefinition,
    snapshot: &Snapshot,
    entry: &ScannedEntry,
    project: Option<&str>,
) -> Result<Vec<FalseField>, String> {
    let location = definition.location(&entry.location);
    let rules = definition.rules(location.kind);
    let Some((merge, field)) = rules
        .merge
        .as_ref()
        .filter(|merge| merge.covers(&entry.location))
        .filter(|_| merged_field(definition, entry).is_some())
        .and_then(|merge| Some((merge, merge.enabled()?)))
    else {
        return Ok(Vec::new());
    };
    // A Stand-in stands in for a server whose Project layer gives it a
    // transport, which deleting the Stand-in leaves it.
    let server = server_entries(definition, snapshot, entry, project, merge);
    let in_project = stands_in_here(definition, &server);
    let mut deleted = Vec::new();
    for setter in setters(definition, snapshot, entry, project, merge, field) {
        if setter.config.get(field) != Some(&Value::Bool(false)) {
            break;
        }
        let layer = definition.location(&setter.location);
        if let Some(projects) = stand_in_outside(definition, snapshot, setter, project) {
            return Err(explain::stand_in_enable(&setter.name, &projects));
        }
        let stand_in = in_project && is_stand_in(definition, setter);
        // Deleting a Stand-in's field alone would turn the server on in
        // every folder.
        let key_path = if stand_in {
            setter.key_path.clone()
        } else {
            field_of(setter, field)
        };
        let versioned = snapshot.versioned.contains(&setter.path);
        if versioned || layer.provenance.read_only() {
            let at = locator::render(&setter.path, &key_path);
            return Err(explain::turned_off_enable(field, &at, versioned));
        }
        deleted.push(FalseField {
            path: setter.path.clone(),
            key_path,
            syntax: merged_syntax(layer),
        });
    }
    if !deleted.is_empty() && location.provenance.read_only() {
        return Err(explain::read_only_enable());
    }
    Ok(deleted)
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
        let Some(mechanism) = mechanism.switched().filter(|mechanism| {
            mechanism.covers(&location.id)
                && mechanism.covers_plugin(plugin_scope(entry))
                && switch_read(
                    definition,
                    snapshot,
                    entry,
                    *mechanism,
                    project.map(|project| project.key),
                )
        }) else {
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

/// Whether the switch of `mechanism` holds Entries by their names in every
/// context, so that a write reaches every Entry of a name wherever it is.
pub(crate) fn by_name_everywhere(mechanism: Switched) -> bool {
    mechanism.target() == MechanismTarget::Name && mechanism.context() == MechanismContext::All
}

/// The Scope the plugin of `entry` is installed at, as the Agent records it.
fn plugin_scope(entry: &ScannedEntry) -> Option<&str> {
    entry.plugin.as_ref()?.scope.as_deref()
}

/// Whether the Agent home of `entry` reads the switch of `mechanism` in the
/// context of the Project with the key `project`: each Feature gate its
/// `when` names has the value it needs, and a switch read only in a trusted
/// folder is in one.
fn switch_read(
    definition: &AgentDefinition,
    snapshot: &Snapshot,
    entry: &ScannedEntry,
    mechanism: Switched,
    project: Option<&str>,
) -> bool {
    let gate_on = |name: &str| resolve::gate_on(definition, snapshot, entry, name, project);
    let kind = definition.location(&entry.location).kind;
    mechanism
        .when()
        .iter()
        .all(|condition| condition.holds(&gate_on, &|_| None))
        && (!mechanism.switch().trusted_only
            || resolve::project_trusted(definition, snapshot, entry, kind, project))
}
