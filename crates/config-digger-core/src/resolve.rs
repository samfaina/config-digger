//! `resolve(snapshot, context)`: a pure function from a snapshot and a context
//! to the Items of every Agent home and the State of each Entry.

use crate::api::{self, Context};
use crate::definitions::blocks::{
    Compete, Conform, Holds, JsonFormat, Malformed, Parse, Provenance, SkillDefect, SkillFallback,
};
use crate::definitions::skill_managers::{self, Strength};
use crate::definitions::{
    AgentDefinition, Check, ItemKind, LocationDefinition, ReadFailureReaction, RiskCode,
};
use crate::disable::{self, ContextProject, Hit};
use crate::scan::{
    GateReading, HomeSource, ScannedEntry, ScannedGate, ScannedReadFailure, Snapshot,
};
use crate::{explain, installations, locator, paths};
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

pub(crate) fn resolve(
    definitions: &[AgentDefinition],
    snapshot: &Snapshot,
    context: &Context,
) -> api::Inventory {
    let homes = snapshot
        .homes
        .iter()
        .map(|home| {
            let definition = &definitions[home.agent_index];
            let (installations, notes) =
                installations::of_home(definition, &snapshot.installations, home);
            api::Home {
                out_of_range: installations
                    .iter()
                    .any(|installation| installation.out_of_range),
                installations,
                last_ran: home.last_ran.clone(),
                notes,
                agent: definition.agent.id.clone(),
                agent_name: definition.agent.name.clone(),
                home: home.home.clone(),
                folder: home.folder.clone(),
                source: match home.source {
                    HomeSource::Default => api::HomeSource::Default,
                    HomeSource::Added => api::HomeSource::Added,
                },
                hidden: home.hidden,
                no_configuration: (!home.configured)
                    .then(|| explain::no_configuration(&definition.agent.name)),
                scopes: definition
                    .scopes
                    .iter()
                    .map(|scope| scope.id.clone())
                    .collect(),
            }
        })
        .collect();

    let projects = snapshot
        .projects
        .iter()
        .map(|project| api::Project {
            path: project.path.clone(),
            missing: !project.exists,
            workspace_file: project.workspace_file,
            registered_by: project
                .registered_by
                .iter()
                .map(|&agent| definitions[agent].agent.id.clone())
                .collect(),
            added: project.added,
            hidden: project.hidden,
        })
        .collect();

    let project = context_project(snapshot, context);
    let project_key = project.as_ref().map(|(_, key)| key);
    let context_project = project
        .as_ref()
        .map(|(path, key)| ContextProject { path, key });
    // The Agent never reads the Entries keyed to a Project whose folder is gone.
    let missing = project.as_ref().filter(|(_, key)| {
        snapshot
            .projects
            .iter()
            .any(|known| known.key == *key && !known.exists)
    });
    // Nor does it read anything through a link whose target is gone, nor a
    // location while a Feature gate keeps it closed.
    let unread = |entry: &ScannedEntry| {
        (missing.is_some() && entry.project.is_some())
            || is_broken(entry)
            || closed_gate(&definitions[entry.agent_index], snapshot, entry).is_some()
    };

    let reactions: Vec<Reaction> = snapshot
        .read_failures
        .iter()
        .map(|failure| reaction(definitions, snapshot, failure))
        .collect();

    // Entries compete when they share an Agent home, an Item kind and a
    // competition key. The map's order sorts Items by Agent, home and key.
    let mut competing: BTreeMap<_, Vec<&ScannedEntry>> = BTreeMap::new();
    for entry in &snapshot.entries {
        if entry.project.is_some() && entry.project.as_ref() != project_key {
            continue;
        }
        let definition = &definitions[entry.agent_index];
        let location = definition.location(&entry.location);
        let key = definition.rules(location.kind).competition.key(&entry.name);
        competing
            .entry((entry.agent_index, entry.home.as_str(), location.kind, key))
            .or_default()
            .push(entry);
    }

    let items = competing
        .into_iter()
        .map(|((agent, home, kind, _), mut entries)| {
            let definition = &definitions[agent];
            let rank = |entry: &ScannedEntry| definition.rank(definition.location(&entry.location));
            // Entries of equal rank, such as under two spellings of one Project
            // key, are ordered by key path so that the choice is stable.
            // Unread Entries come last, so that they never name the Item.
            entries.sort_by(|a, b| {
                (unread(a), rank(a), a.depth, &a.key_path).cmp(&(
                    unread(b),
                    rank(b),
                    b.depth,
                    &b.key_path,
                ))
            });
            // An Item is named after the Entry that would be Effective
            // without Disables.
            let name = entries[0].name.clone();
            let agent_name = &definition.agent.name;
            let hits: Vec<(Vec<Hit>, Vec<Hit>)> = entries
                .iter()
                .map(|entry| {
                    disable::hits(definition, snapshot, entry, project_key.map(String::as_str))
                })
                .collect();
            // Why each Entry lacks the minimal shape its location declares.
            let malformed: Vec<Option<Malformed>> = entries
                .iter()
                .map(|entry| {
                    let shape = definition.location(&entry.location).shape.as_ref()?;
                    shape.malformed(&entry.config)
                })
                .collect();
            // What makes the Agent skip each skill, and what it loads anyway
            // through a fallback.
            let (skipped, fallbacks): (Vec<Option<SkillDefect>>, Vec<Option<SkillDefect>>) =
                entries
                    .iter()
                    .map(|entry| {
                        let folder = definition.location(&entry.location).format.skill_folder();
                        match (entry.defect, folder) {
                            (Some(defect), Some(folder)) => match folder.fallback(defect) {
                                SkillFallback::Skip => (Some(defect), None),
                                SkillFallback::FolderName => (None, Some(defect)),
                            },
                            _ => (None, None),
                        }
                    })
                    .unzip();
            // An Invalid Entry competes only where the Agent still lists it,
            // and an Unread one or a skipped skill never does.
            let competes = |index: usize| {
                !unread(entries[index])
                    && skipped[index].is_none()
                    && (malformed[index].is_none()
                        || definition
                            .location(&entries[index].location)
                            .shape
                            .as_ref()
                            .is_some_and(Conform::still_competes))
            };
            // The Agent skips a Disabled Entry, and an Invalid one that does
            // not compete, so the highest of the others wins.
            let winner =
                (0..entries.len()).find(|&index| hits[index].0.is_empty() && competes(index));
            let shadowed_by = |index: usize, winner: usize| {
                explain::shadowed_by_precedence(
                    agent_name,
                    &definition.location(&entries[winner].location).scope,
                    &locator::render(&entries[winner].path, &entries[winner].key_path),
                    rank(entries[index]) == rank(entries[winner]),
                )
            };
            let api_entries: Vec<api::Entry> = entries
                .iter()
                .enumerate()
                .map(|(index, entry)| {
                    let location = definition.location(&entry.location);
                    let gate_caveats = gate_caveats(definition, snapshot, entry);
                    if let Some(link) = entry.link.as_ref().filter(|link| link.broken()) {
                        let reason = explain::link_broken(agent_name, &link.target);
                        return unread_entry(snapshot, entry, location, reason, gate_caveats);
                    }
                    if let (true, Some(_), Some((path, _))) =
                        (unread(entry), &entry.project, missing)
                    {
                        let reason = explain::project_missing(agent_name, path);
                        return unread_entry(snapshot, entry, location, reason, gate_caveats);
                    }
                    if let Some((gate, on)) = closed_gate(definition, snapshot, entry) {
                        let reason = explain::feature_gate(agent_name, gate, on);
                        return unread_entry(snapshot, entry, location, reason, gate_caveats);
                    }
                    let origin = origin(location, entry);
                    let (disabling, partial) = &hits[index];
                    // Each check the Entry fails, in the Agent's order.
                    let mut failed: Vec<(api::State, api::Reason)> = Vec::new();
                    for check in &definition.rules(kind).checks {
                        match check {
                            Check::Disable => failed.extend(
                                disabling
                                    .iter()
                                    .map(|hit| (api::State::Disabled, hit.reason(agent_name))),
                            ),
                            Check::Shape => {
                                if let Some(malformed) = &malformed[index] {
                                    let competes = competes(index);
                                    failed.push((
                                        api::State::Invalid,
                                        explain::malformed_entry(agent_name, malformed, competes),
                                    ));
                                }
                            }
                            Check::Frontmatter => {
                                if let Some(defect) = skipped[index] {
                                    failed.push((
                                        api::State::Invalid,
                                        explain::malformed_skill(agent_name, defect),
                                    ));
                                }
                            }
                            Check::Precedence => {
                                if let Some(winner) = winner.filter(|&winner| winner < index) {
                                    failed.push((api::State::Shadowed, shadowed_by(index, winner)));
                                }
                            }
                        }
                    }
                    let mut failed = failed.into_iter();
                    let (state, reason) = match failed.next() {
                        Some((state, reason)) => (state, Some(reason)),
                        None => (api::State::Effective, None),
                    };
                    // Enable lifts every switch that Disables it, and with
                    // them those of the Entries they Disable too.
                    let what_if = (state == api::State::Disabled).then(|| {
                        let lifted = |other: usize| {
                            hits[other]
                                .0
                                .iter()
                                .all(|hit| disabling.iter().any(|own| own.same_switch(hit)))
                        };
                        let winner = (0..entries.len())
                            .find(|&other| lifted(other) && competes(other))
                            .filter(|&winner| winner < index);
                        let Some(winner) = winner else {
                            let state = if malformed[index].is_some() || skipped[index].is_some() {
                                api::State::Invalid
                            } else {
                                api::State::Effective
                            };
                            return api::WhatIf {
                                state,
                                effective: None,
                                sentence: explain::what_if(state, None),
                            };
                        };
                        let scope = &definition.location(&entries[winner].location).scope;
                        let effective =
                            locator::render(&entries[winner].path, &entries[winner].key_path);
                        api::WhatIf {
                            state: api::State::Shadowed,
                            sentence: explain::what_if(
                                api::State::Shadowed,
                                Some((scope, &effective)),
                            ),
                            effective: Some(effective),
                        }
                    });
                    let enable = (!disabling.is_empty()).then(|| {
                        disable::enable(location, disabling).unwrap_or_else(|refused| refused)
                    });
                    let disable = if disabling.is_empty() {
                        disable::option(definition, snapshot, entry, context_project.as_ref())
                    } else {
                        api::DisableOption::NotOffered {
                            code: api::DisableNotOffered::AlreadyDisabled,
                            sentence: explain::already_disabled_option(),
                        }
                    };
                    api::Entry {
                        path: entry.path.clone(),
                        key_path: entry.key_path.clone(),
                        locator: locator::render(&entry.path, &entry.key_path),
                        scope: location.scope.clone(),
                        provenance: origin.provenance,
                        skill_manager: origin.skill_manager,
                        git_versioned: snapshot.versioned.contains(&entry.path),
                        link: link(entry),
                        state,
                        reason,
                        secondary: failed.map(|(_, reason)| reason).collect(),
                        what_if,
                        notes: fallbacks[index]
                            .map(|defect| api::Note {
                                code: api::NoteCode::SkillFallback,
                                switch: None,
                                sentence: explain::skill_fallback(
                                    agent_name,
                                    defect,
                                    &entry.path.file_name().unwrap_or_default().to_string_lossy(),
                                ),
                            })
                            .into_iter()
                            .chain(partial.iter().map(|hit| api::Note {
                                code: api::NoteCode::PartialSwitch,
                                switch: Some(hit.locator()),
                                sentence: explain::partial_switch(&hit.holds(), agent_name),
                            }))
                            .chain(origin.by_name_only)
                            .collect(),
                        config: entry.config.clone(),
                        masked: entry.masked.clone(),
                        disable,
                        enable,
                        caveats: disable::failed_switch_files(
                            definition,
                            snapshot,
                            entry,
                            context_project.as_ref(),
                        )
                        .into_iter()
                        .map(|path| api::Caveat {
                            code: api::CaveatCode::SwitchReadFailure,
                            sentence: explain::switch_read_failure(&path),
                            path,
                        })
                        .chain(gate_caveats)
                        .collect(),
                    }
                })
                .collect();
            // An Item takes the State of the Entry the Agent uses, if any:
            // Effective, or Invalid when it is malformed and still competes.
            let state = api_entries[winner.unwrap_or(0)].state;
            // A location the Agent may or may not read without could hold
            // an Entry of the Item.
            let caveats = snapshot
                .read_failures
                .iter()
                .zip(&reactions)
                .filter(|(failure, reaction)| {
                    failure.agent_index == agent
                        && failure.home == home
                        && reaction.strongest == ReadFailureReaction::Unknown
                        && reaction.kinds.contains(&kind)
                        && failure.reaches(project_key.map(String::as_str))
                })
                .map(|(failure, _)| api::Caveat {
                    code: api::CaveatCode::ReadFailure,
                    path: failure.path.clone(),
                    sentence: explain::read_failure_caveat(agent_name, &failure.path),
                })
                .collect();
            api::Item {
                agent: definition.agent.id.clone(),
                home: home.to_owned(),
                kind: kind.into(),
                name,
                state,
                entries: api_entries,
                caveats,
            }
        })
        .collect();

    let read_failures = snapshot
        .read_failures
        .iter()
        .zip(&reactions)
        .map(|(failure, reaction)| {
            let agent = &definitions[failure.agent_index].agent;
            api::ReadFailure {
                agent: agent.id.clone(),
                agent_name: agent.name.clone(),
                home: failure.home.clone(),
                path: failure.path.clone(),
                project: failure
                    .project
                    .as_ref()
                    .map(|key| project_path(snapshot, key)),
                kind: failure.kind,
                position: failure.position.map(|(line, column)| api::Position {
                    line: line.try_into().unwrap_or(u32::MAX),
                    column: column.try_into().unwrap_or(u32::MAX),
                }),
                reaction: reaction.strongest.into(),
                sentence: explain::read_failure(
                    &agent.name,
                    &failure.path,
                    failure.kind,
                    reaction.dialect,
                    failure.position,
                ),
                notes: (reaction.strongest == ReadFailureReaction::AsksUser)
                    .then(|| explain::asks_user(&agent.name))
                    .into_iter()
                    .collect(),
                warnings: reaction
                    .risks
                    .iter()
                    .map(|&code| api::RiskWarning {
                        code: code.into(),
                        sentence: explain::risk(&agent.name, code.into()),
                    })
                    .collect(),
            }
        })
        .collect();

    // An Agent refuses to start in each context a location that stops it reaches.
    let agent_flags = snapshot
        .read_failures
        .iter()
        .zip(&reactions)
        .filter(|(failure, reaction)| {
            reaction.strongest == ReadFailureReaction::StopsAgent
                && failure.reaches(project_key.map(String::as_str))
        })
        .map(|(failure, _)| {
            let agent = &definitions[failure.agent_index].agent;
            api::AgentFlag {
                agent: agent.id.clone(),
                agent_name: agent.name.clone(),
                home: failure.home.clone(),
                code: api::AgentFlagCode::CannotStart,
                path: failure.path.clone(),
                sentence: explain::cannot_start(&agent.name, &failure.path),
            }
        })
        .collect();

    api::Inventory {
        read_at: humantime::format_rfc3339_seconds(snapshot.read_at).to_string(),
        project: project.map(|(path, _)| path),
        projects,
        homes,
        items,
        read_failures,
        agent_flags,
    }
}

/// The Unread Entries of each missing Project that has any, sorted by its
/// path, each resolved in its Project's context.
pub(crate) fn leftovers(
    definitions: &[AgentDefinition],
    snapshot: &Snapshot,
) -> Vec<api::Leftovers> {
    snapshot
        .projects
        .iter()
        .filter(|project| !project.exists)
        .filter_map(|project| {
            let context = Context {
                project: Some(project.path.clone()),
            };
            let mut items = resolve(definitions, snapshot, &context).items;
            for item in &mut items {
                item.entries.retain(|entry| {
                    entry
                        .reason
                        .as_ref()
                        .is_some_and(|reason| reason.code == api::ReasonCode::ProjectMissing)
                });
                item.state = api::State::Unread;
            }
            items.retain(|item| !item.entries.is_empty());
            (!items.is_empty()).then(|| api::Leftovers {
                project: project.path.clone(),
                items,
            })
        })
        .collect()
}

/// Whether the Entry is a link whose target is gone.
fn is_broken(entry: &ScannedEntry) -> bool {
    entry.link.as_ref().is_some_and(|link| link.broken())
}

/// An Entry the Agent never reads, for `reason`, such as one keyed to a
/// missing Project, with `caveats`: it competes with nothing and can only
/// be Removed.
fn unread_entry(
    snapshot: &Snapshot,
    entry: &ScannedEntry,
    location: &LocationDefinition,
    reason: api::Reason,
    caveats: Vec<api::Caveat>,
) -> api::Entry {
    let origin = origin(location, entry);
    api::Entry {
        path: entry.path.clone(),
        key_path: entry.key_path.clone(),
        locator: locator::render(&entry.path, &entry.key_path),
        scope: location.scope.clone(),
        provenance: origin.provenance,
        skill_manager: origin.skill_manager,
        git_versioned: snapshot.versioned.contains(&entry.path),
        link: link(entry),
        state: api::State::Unread,
        reason: Some(reason),
        secondary: Vec::new(),
        what_if: None,
        notes: origin.by_name_only.into_iter().collect(),
        config: entry.config.clone(),
        masked: entry.masked.clone(),
        disable: api::DisableOption::NotOffered {
            code: api::DisableNotOffered::Unread,
            sentence: explain::unread_option(),
        },
        enable: None,
        caveats,
    }
}

/// What an Agent home does about a file that is a Read failure for it, as
/// the definition says of the locations it reads there.
struct Reaction {
    /// The strongest reaction of those locations; a file only a project
    /// registry reads is ignored.
    strongest: ReadFailureReaction,
    risks: BTreeSet<RiskCode>,
    /// The dialect the file is parsed in, as a sentence names it.
    dialect: &'static str,
    /// The Item kinds whose Entries the file could hold.
    kinds: BTreeSet<ItemKind>,
}

fn reaction(
    definitions: &[AgentDefinition],
    snapshot: &Snapshot,
    failure: &ScannedReadFailure,
) -> Reaction {
    let definition = &definitions[failure.agent_index];
    let locations: Vec<&LocationDefinition> = snapshot
        .readers_of(&failure.path)
        .iter()
        .filter(|reader| reader.agent_index == failure.agent_index && reader.home == failure.home)
        .map(|reader| definition.location(&reader.location))
        .collect();
    Reaction {
        strongest: locations
            .iter()
            .map(|location| location.on_read_failure)
            .max()
            .unwrap_or(ReadFailureReaction::IgnoresFile),
        risks: locations
            .iter()
            .flat_map(|location| location.risks.iter().copied())
            .collect(),
        dialect: locations
            .first()
            .map_or(JsonFormat::STRICT.dialect(), |location| {
                location
                    .format
                    .file()
                    .map_or(JsonFormat::STRICT.dialect(), Parse::dialect)
            }),
        kinds: locations.iter().map(|location| location.kind).collect(),
    }
}

/// The path of the scanned Project whose key is `key`.
fn project_path(snapshot: &Snapshot, key: &str) -> PathBuf {
    snapshot
        .projects
        .iter()
        .find(|project| project.key == key)
        .expect("a Project key comes from the snapshot")
        .path
        .clone()
}

/// The context's Project path and key: a scanned Project that any spelling of
/// it names, or else the lexical normalisation of the spelling.
pub(crate) fn context_project(snapshot: &Snapshot, context: &Context) -> Option<(PathBuf, String)> {
    let lexical = paths::lexical(context.project.as_deref()?);
    let spelling = paths::key(&lexical);
    Some(
        match snapshot
            .projects
            .iter()
            .find(|project| project.spellings.contains(&spelling))
        {
            Some(project) => (project.path.clone(), project.key.clone()),
            None => (lexical, spelling),
        },
    )
}

/// Where the link at a Linked Entry's location points.
fn link(entry: &ScannedEntry) -> Option<api::Link> {
    entry.link.as_ref().map(|link| api::Link {
        target: link.target.clone(),
        broken: link.broken(),
    })
}

/// How an Entry got where it is, as an Entry shows it.
struct Origin {
    provenance: api::Provenance,
    skill_manager: Option<api::SkillManager>,
    /// The note that the skill manager is recognised only by a weak
    /// detector, such as the folder's name.
    by_name_only: Option<api::Note>,
}

/// The Provenance of `entry`: a skill manager's when one's definition
/// recognises it as having installed the skill, and its location's
/// Provenance is a file's; else its location's.
fn origin(location: &LocationDefinition, entry: &ScannedEntry) -> Origin {
    let found = entry
        .manager
        .filter(|_| location.provenance == Provenance::File)
        .and_then(|found| {
            let managers = skill_managers::built_in().ok()?;
            Some((&managers[found.manager], found.strength))
        });
    let Some((definition, strength)) = found else {
        return Origin {
            provenance: location.provenance.into(),
            skill_manager: None,
            by_name_only: None,
        };
    };
    Origin {
        provenance: api::Provenance::SkillManager,
        skill_manager: Some(api::SkillManager {
            id: definition.manager.id.clone(),
            name: definition.manager.name.clone(),
        }),
        by_name_only: (strength == Strength::Weak).then(|| api::Note {
            code: api::NoteCode::ProvenanceByNameOnly,
            switch: None,
            sentence: explain::provenance_by_name_only(&definition.manager.name),
        }),
    }
}

/// Whether the Feature gate `name` of the Agent home of `entry` is on: the
/// value the Agent last cached, or else the definition's assumed default.
fn gate_on(
    definition: &AgentDefinition,
    snapshot: &Snapshot,
    entry: &ScannedEntry,
    name: &str,
) -> bool {
    let cached = scanned_gate(snapshot, entry, name).and_then(|gate| match gate.reading {
        GateReading::Cached(cached) => Some(cached.on),
        GateReading::Unreadable | GateReading::Bypassed(_) => None,
    });
    cached.unwrap_or_else(|| {
        definition
            .feature_gate(name)
            .is_some_and(|gate| gate.assumed)
    })
}

fn scanned_gate<'s>(
    snapshot: &'s Snapshot,
    entry: &ScannedEntry,
    name: &str,
) -> Option<&'s ScannedGate> {
    snapshot.gates.iter().find(|gate| {
        gate.agent_index == entry.agent_index && gate.home == entry.home && gate.name == name
    })
}

/// The first `when` condition of the location of `entry` that does not
/// hold, as the Feature gate it depends on and whether that gate is on.
fn closed_gate<'d>(
    definition: &'d AgentDefinition,
    snapshot: &Snapshot,
    entry: &ScannedEntry,
) -> Option<(&'d str, bool)> {
    let gate_on = |name: &str| gate_on(definition, snapshot, entry, name);
    definition
        .location(&entry.location)
        .when
        .iter()
        .find(|condition| !condition.holds(&gate_on))
        .and_then(Holds::gate)
        .map(|name| (name, gate_on(name)))
}

/// A caveat per Feature gate that affects the location of `entry`, saying
/// which value it is resolved with and how certain that is.
fn gate_caveats(
    definition: &AgentDefinition,
    snapshot: &Snapshot,
    entry: &ScannedEntry,
) -> Vec<api::Caveat> {
    let agent_name = &definition.agent.name;
    definition
        .feature_gates
        .iter()
        .filter(|gate| gate.affects.contains(&entry.location))
        .filter_map(|gate| {
            let scanned = scanned_gate(snapshot, entry, &gate.name)?;
            let (code, sentence) = match &scanned.reading {
                GateReading::Cached(cached) => (
                    api::CaveatCode::GateCached,
                    explain::gate_cached(agent_name, &gate.name, cached.on, cached.at),
                ),
                GateReading::Unreadable => (
                    api::CaveatCode::GateAssumed,
                    explain::gate_assumed(agent_name, &gate.name, gate.assumed, None),
                ),
                GateReading::Bypassed(variable) => (
                    api::CaveatCode::GateAssumed,
                    explain::gate_assumed(agent_name, &gate.name, gate.assumed, Some(variable)),
                ),
            };
            Some(api::Caveat {
                code,
                path: scanned.path.clone(),
                sentence,
            })
        })
        .collect()
}
