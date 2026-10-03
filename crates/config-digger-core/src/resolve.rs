//! `resolve(snapshot, context)`: a pure function from a snapshot and a context
//! to the Items of every Agent home and the State of each Entry.

use crate::api::{self, Context};
use crate::definitions::blocks::{Compete, Conform, JsonFormat, Malformed, Parse};
use crate::definitions::{
    AgentDefinition, Check, ItemKind, LocationDefinition, ReadFailureReaction, RiskCode,
};
use crate::disable::{self, ContextProject, Hit};
use crate::scan::{HomeSource, ScannedEntry, ScannedReadFailure, Snapshot};
use crate::{explain, locator, paths};
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
            api::Home {
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
            entries.sort_by(|a, b| (rank(a), &a.key_path).cmp(&(rank(b), &b.key_path)));
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
            // An Invalid Entry competes only where the Agent still lists it.
            let competes = |index: usize| {
                malformed[index].is_none()
                    || definition
                        .location(&entries[index].location)
                        .shape
                        .as_ref()
                        .is_some_and(Conform::still_competes)
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
                            let state = match malformed[index] {
                                Some(_) => api::State::Invalid,
                                None => api::State::Effective,
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
                    let enable = (!disabling.is_empty())
                        .then(|| disable::enable(disabling).unwrap_or_else(|refused| refused));
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
                        provenance: location.provenance.into(),
                        git_versioned: snapshot.versioned.contains(&entry.path),
                        state,
                        reason,
                        secondary: failed.map(|(_, reason)| reason).collect(),
                        what_if,
                        notes: partial
                            .iter()
                            .map(|hit| api::Note {
                                code: api::NoteCode::PartialSwitch,
                                switch: hit.locator(),
                                sentence: explain::partial_switch(&hit.holds(), agent_name),
                            })
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
                location.format.dialect()
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
