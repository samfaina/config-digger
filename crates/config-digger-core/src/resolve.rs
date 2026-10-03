//! `resolve(snapshot, context)`: a pure function from a snapshot and a context
//! to the Items of every Agent home and the State of each Entry.

use crate::api::{self, Context};
use crate::definitions::AgentDefinition;
use crate::definitions::Check;
use crate::definitions::blocks::Compete;
use crate::disable::{self, ContextProject, Hit};
use crate::scan::{ScannedEntry, Snapshot};
use crate::{explain, locator, paths};
use std::collections::BTreeMap;
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
            registered_by: project
                .registered_by
                .iter()
                .map(|&agent| definitions[agent].agent.id.clone())
                .collect(),
        })
        .collect();

    let project = context_project(snapshot, context);
    let project_key = project.as_ref().map(|(_, key)| key);
    let context_project = project
        .as_ref()
        .map(|(path, key)| ContextProject { path, key });

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
            // The Agent skips a Disabled Entry, so the highest of the others wins.
            let effective = (0..entries.len()).find(|&index| hits[index].0.is_empty());
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
                            Check::Precedence => {
                                if let Some(winner) = effective.filter(|&winner| winner < index) {
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
                            .find(|&other| lifted(other))
                            .expect("the Entry itself is lifted");
                        if winner == index {
                            return api::WhatIf {
                                state: api::State::Effective,
                                effective: None,
                                sentence: explain::what_if(None),
                            };
                        }
                        let scope = &definition.location(&entries[winner].location).scope;
                        let effective =
                            locator::render(&entries[winner].path, &entries[winner].key_path);
                        api::WhatIf {
                            state: api::State::Shadowed,
                            sentence: explain::what_if(Some((scope, &effective))),
                            effective: Some(effective),
                        }
                    });
                    let enable = (!disabling.is_empty()).then(|| {
                        let sentences: Vec<String> =
                            disabling.iter().map(Hit::enable_sentence).collect();
                        sentences.join(" ")
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
                                sentence: explain::partly_switched_off(&hit.holds(), agent_name),
                            })
                            .collect(),
                        config: entry.config.clone(),
                        masked: entry.masked.clone(),
                        disable,
                        enable,
                    }
                })
                .collect();
            // An Item is in use when one of its Entries is Effective.
            let state = if effective.is_some() {
                api::State::Effective
            } else {
                api_entries[0].state
            };
            api::Item {
                agent: definition.agent.id.clone(),
                home: home.to_owned(),
                kind: kind.into(),
                name,
                state,
                entries: api_entries,
            }
        })
        .collect();

    let read_failures = snapshot
        .read_failures
        .iter()
        .map(|failure| {
            let agent = &definitions[failure.agent_index].agent;
            api::ReadFailure {
                agent: agent.id.clone(),
                agent_name: agent.name.clone(),
                home: failure.home.clone(),
                path: failure.path.clone(),
                kind: failure.kind,
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
    }
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
