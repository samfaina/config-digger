//! `resolve(snapshot, context)`: a pure function from a snapshot and a context
//! to the Items of every Agent home and the State of each Entry.

use crate::api::{self, Context};
use crate::definitions::blocks::{
    self, Anchored, Compete, Condition, Conform, Gate, Holds, JsonFormat, Malformed, Parse,
    PolicyField, Provenance, SkillDefect, SkillFallback,
};
use crate::definitions::skill_managers::{self, Strength};
use crate::definitions::{
    AgentDefinition, Check, ItemKind, LocationDefinition, ReadFailureReaction, RiskCode,
};
use crate::disable::{self, ContextProject, Hit};
use crate::scan::{
    GateReading, HomeSource, ScannedEntry, ScannedGate, ScannedHome, ScannedReadFailure,
    SettingOwner, Snapshot,
};
use crate::{explain, installations, locator, paths, rules};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

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
                not_read: home.skipped.as_ref().map(|skipped| {
                    let instead = skipped.instead.as_ref();
                    api::NotRead {
                        read_instead: instead.map(|(id, _)| id.clone()),
                        sentence: explain::home_not_read(
                            &definition.agent.name,
                            &home.folder,
                            instead.map(|(_, folder)| folder.as_path()),
                        ),
                    }
                }),
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
        skipped_home(snapshot, entry).is_some()
            || (missing.is_some() && entry.project.is_some())
            || is_broken(entry)
            || closed_gate(&definitions[entry.agent_index], snapshot, entry).is_some()
            || skipped_source(&definitions[entry.agent_index], snapshot, entry).is_some()
    };

    let reactions: Vec<Reaction> = snapshot
        .read_failures
        .iter()
        .map(|failure| reaction(definitions, snapshot, failure))
        .collect();

    // Entries compete when they share an Agent home, an Item kind and a
    // competition key. The map's order sorts Items by Agent, home and key.
    let in_context: Vec<&ScannedEntry> = snapshot
        .entries
        .iter()
        .filter(|entry| entry.project.is_none() || entry.project.as_ref() == project_key)
        .collect();
    let mut competing: BTreeMap<_, Vec<&ScannedEntry>> = BTreeMap::new();
    for &entry in &in_context {
        let definition = &definitions[entry.agent_index];
        let location = definition.location(&entry.location);
        let key = item_key(definition, &in_context, entry, &unread);
        competing
            .entry((entry.agent_index, entry.home.as_str(), location.kind, key))
            .or_default()
            .push(entry);
    }

    let items = competing
        .into_iter()
        .map(|((agent, home, kind, _), mut entries)| {
            let definition = &definitions[agent];
            // A later file of a settings source overrides an earlier one.
            let rank = |entry: &ScannedEntry| {
                (
                    definition.rank(definition.location(&entry.location)),
                    overridden(definition, snapshot, entry),
                )
            };
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
            let checks = &definition.rules(kind).checks;
            // The rules that keep each Entry out.
            let ruled: Vec<Vec<rules::RuleHit>> = entries
                .iter()
                .map(|entry| {
                    rules::hits(definition, snapshot, entry, project_key.map(String::as_str))
                })
                .collect();
            // The gates and rules that keep each Entry out, each with the
            // check it belongs to.
            let kept: Vec<Vec<(Check, api::State, api::Reason)>> = entries
                .iter()
                .zip(&ruled)
                .map(|(entry, ruled)| {
                    gates(definition, snapshot, entry, project_key.map(String::as_str))
                        .into_iter()
                        .map(|(check, reason)| (check, api::State::Blocked, reason))
                        .chain(
                            ruled
                                .iter()
                                .map(|hit| (Check::Rules, hit.state, hit.reason.clone())),
                        )
                        .collect()
                })
                .collect();
            // A gate or rule checked before precedence takes the Entry out of
            // the competition; one checked after applies to the Entry that wins.
            let before_precedence = |check: Check| {
                checks.iter().position(|known| *known == check)
                    < checks.iter().position(|known| *known == Check::Precedence)
            };
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
            // The Agent skips a Disabled or Blocked Entry, and an Invalid one
            // that does not compete, so the highest of the others wins.
            let free = |index: usize| {
                hits[index].0.is_empty()
                    && !kept[index]
                        .iter()
                        .any(|(check, _, _)| before_precedence(*check))
                    && competes(index)
            };
            let winner = (0..entries.len()).find(|&index| free(index));
            let shadowed_by = |index: usize, winner: usize| {
                explain::shadowed_by_precedence(
                    agent_name,
                    &definition.location(&entries[winner].location).scope,
                    &locator::render(&entries[winner].path, &entries[winner].key_path),
                    rank(entries[index]) == rank(entries[winner]),
                    definition
                        .rules(kind)
                        .competition
                        .by_endpoint(&entries[index].location)
                        && blocks::endpoint(&entries[index].config)
                            == blocks::endpoint(&entries[winner].config),
                )
            };
            let api_entries: Vec<api::Entry> = entries
                .iter()
                .enumerate()
                .map(|(index, entry)| {
                    let location = definition.location(&entry.location);
                    let gate_caveats = gate_caveats(definition, snapshot, entry);
                    if let Some(home) = skipped_home(snapshot, entry) {
                        let instead = home.skipped.as_ref().and_then(|skipped| {
                            skipped.instead.as_ref().map(|(_, folder)| folder.as_path())
                        });
                        let reason =
                            explain::lookup_order_skips_home(agent_name, &home.folder, instead);
                        return unread_entry(
                            definition,
                            snapshot,
                            entry,
                            location,
                            reason,
                            gate_caveats,
                        );
                    }
                    if let Some(link) = entry.link.as_ref().filter(|link| link.broken()) {
                        let reason = explain::link_broken(agent_name, &link.target);
                        return unread_entry(
                            definition,
                            snapshot,
                            entry,
                            location,
                            reason,
                            gate_caveats,
                        );
                    }
                    if let (true, Some(_), Some((path, _))) =
                        (unread(entry), &entry.project, missing)
                    {
                        let reason = explain::project_missing(agent_name, path);
                        return unread_entry(
                            definition,
                            snapshot,
                            entry,
                            location,
                            reason,
                            gate_caveats,
                        );
                    }
                    if let Some((gate, on)) = closed_gate(definition, snapshot, entry) {
                        let reason = explain::feature_gate(agent_name, gate, on);
                        return unread_entry(
                            definition,
                            snapshot,
                            entry,
                            location,
                            reason,
                            gate_caveats,
                        );
                    }
                    if let Some((settings, read)) = skipped_source(definition, snapshot, entry) {
                        let reason = explain::source_skipped(agent_name, settings, read);
                        return unread_entry(
                            definition,
                            snapshot,
                            entry,
                            location,
                            reason,
                            gate_caveats,
                        );
                    }
                    let origin = origin(location, entry);
                    let (disabling, partial) = &hits[index];
                    // The rules outside policy that Disable it.
                    let by_rule: Vec<&rules::RuleHit> = ruled[index]
                        .iter()
                        .filter(|hit| hit.state == api::State::Disabled)
                        .collect();
                    // Each check the Entry fails, in the Agent's order.
                    let mut failed: Vec<(Check, api::State, api::Reason)> = Vec::new();
                    for &check in checks {
                        match check {
                            Check::Policy | Check::Approval | Check::Rules => failed.extend(
                                kept[index]
                                    .iter()
                                    .filter(|(known, _, _)| *known == check)
                                    .cloned(),
                            ),
                            Check::Disable => {
                                failed.extend(disabling.iter().map(|hit| {
                                    (check, api::State::Disabled, hit.reason(agent_name))
                                }))
                            }
                            Check::Shape => {
                                if let Some(malformed) = &malformed[index] {
                                    let competes = competes(index);
                                    failed.push((
                                        check,
                                        api::State::Invalid,
                                        explain::malformed_entry(agent_name, malformed, competes),
                                    ));
                                }
                            }
                            Check::Frontmatter => {
                                if let Some(defect) = skipped[index] {
                                    failed.push((
                                        check,
                                        api::State::Invalid,
                                        explain::malformed_skill(agent_name, defect),
                                    ));
                                }
                            }
                            Check::Precedence => {
                                if let Some(winner) = winner.filter(|&winner| winner < index) {
                                    failed.push((
                                        check,
                                        api::State::Shadowed,
                                        shadowed_by(index, winner),
                                    ));
                                }
                            }
                        }
                    }
                    let (state, reason) = match failed.first() {
                        Some((_, state, reason)) => (*state, Some(reason.clone())),
                        None => (api::State::Effective, None),
                    };
                    // The State it would have once what sets its State is
                    // lifted. Enable lifts every switch that Disables it, and
                    // with them those of the Entries they Disable too.
                    let what_if =
                        matches!(state, api::State::Disabled | api::State::Blocked).then(|| {
                            let lifted =
                                |other: usize| match state {
                                    api::State::Disabled => {
                                        hits[other].0.iter().all(|hit| {
                                            disabling.iter().any(|own| own.same_switch(hit))
                                        }) && !kept[other]
                                            .iter()
                                            .any(|(check, _, _)| before_precedence(*check))
                                            && competes(other)
                                    }
                                    _ => free(other),
                                };
                            let render = |next: api::State, shadowed_by: Option<(&str, &str)>| {
                                if state == api::State::Disabled && by_rule.is_empty() {
                                    explain::what_if(next, shadowed_by)
                                } else {
                                    explain::what_if_otherwise(state, next, shadowed_by)
                                }
                            };
                            for &check in checks {
                                if check == Check::Precedence {
                                    let Some(winner) = (0..index).find(|&other| lifted(other))
                                    else {
                                        continue;
                                    };
                                    let scope =
                                        &definition.location(&entries[winner].location).scope;
                                    let effective = locator::render(
                                        &entries[winner].path,
                                        &entries[winner].key_path,
                                    );
                                    return api::WhatIf {
                                        state: api::State::Shadowed,
                                        sentence: render(
                                            api::State::Shadowed,
                                            Some((scope, &effective)),
                                        ),
                                        effective: Some(effective),
                                    };
                                }
                                let next = failed
                                    .iter()
                                    .find(|(failed, next, _)| *failed == check && *next != state);
                                if let Some((_, next, _)) = next {
                                    return api::WhatIf {
                                        state: *next,
                                        effective: None,
                                        sentence: render(*next, None),
                                    };
                                }
                            }
                            api::WhatIf {
                                state: api::State::Effective,
                                effective: None,
                                sentence: render(api::State::Effective, None),
                            }
                        });
                    // Enable lifts no rule.
                    let enable = rules::enable_not_offered(&by_rule).or_else(|| {
                        (!disabling.is_empty()).then(|| {
                            disable::enable(location, disabling).unwrap_or_else(|refused| refused)
                        })
                    });
                    let disable = if disabling.is_empty() && by_rule.is_empty() {
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
                        plugin: plugin(entry),
                        git_versioned: snapshot.versioned.contains(&entry.path),
                        link: link(entry),
                        state,
                        reason,
                        secondary: failed
                            .into_iter()
                            .skip(1)
                            .map(|(_, _, reason)| reason)
                            .collect(),
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
                        remove: remove(definition, location, entry),
                        caveats: read_failure_caveats(
                            definition,
                            snapshot,
                            entry,
                            context_project.as_ref(),
                        )
                        .into_iter()
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
            // A plugin bound to the Project is no leftover: only the Agent
            // uninstalls it.
            for item in &mut items {
                item.entries.retain(|entry| {
                    entry.plugin.is_none()
                        && entry
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

/// What an Entry competes by: Entries with the same key are one Item.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum ItemKey {
    /// The competition key of a name.
    Name(String),
    /// An endpoint no Entry that competes by name has.
    Endpoint(String),
}

/// What `entry`, among the Entries `in_context` of the context, competes
/// by: its name, or for an Entry of a location that competes by endpoint,
/// the name of the Entry the Agent ranks highest among those of other
/// locations that have its endpoint and are read, or else its endpoint.
fn item_key(
    definition: &AgentDefinition,
    in_context: &[&ScannedEntry],
    entry: &ScannedEntry,
    unread: &dyn Fn(&ScannedEntry) -> bool,
) -> ItemKey {
    let location = definition.location(&entry.location);
    let competition = &definition.rules(location.kind).competition;
    let by_name = |entry: &ScannedEntry| ItemKey::Name(competition.key(&entry.name));
    if !competition.by_endpoint(&location.id) {
        return by_name(entry);
    }
    // Of same-named Entries that compete by endpoint, such as a plugin's
    // servers in two of its files, the Agent keeps the one it ranks
    // highest: they compete with the others by its endpoint.
    let same_name = in_context
        .iter()
        .filter(|other| {
            other.agent_index == entry.agent_index
                && other.home == entry.home
                && other.name == entry.name
                && competition.by_endpoint(&other.location)
        })
        .min_by_key(|other| definition.rank(definition.location(&other.location)))
        .copied()
        .unwrap_or(entry);
    let Some(endpoint) = blocks::endpoint(&same_name.config) else {
        return by_name(entry);
    };
    let named = in_context
        .iter()
        .filter(|other| {
            let other_location = definition.location(&other.location);
            other.agent_index == entry.agent_index
                && other.home == entry.home
                && other_location.kind == location.kind
                && !competition.by_endpoint(&other_location.id)
                && !unread(other)
                && blocks::endpoint(&other.config).as_ref() == Some(&endpoint)
        })
        .min_by_key(|other| definition.rank(definition.location(&other.location)));
    match named {
        Some(other) => by_name(other),
        None => ItemKey::Endpoint(endpoint),
    }
}

/// The Agent home of `entry`, when the Agent's lookup order skips it.
fn skipped_home<'s>(snapshot: &'s Snapshot, entry: &ScannedEntry) -> Option<&'s ScannedHome> {
    snapshot.homes.iter().find(|home| {
        home.agent_index == entry.agent_index && home.home == entry.home && home.skipped.is_some()
    })
}

/// Whether the Entry is a link whose target is gone.
fn is_broken(entry: &ScannedEntry) -> bool {
    entry.link.as_ref().is_some_and(|link| link.broken())
}

/// An Entry the Agent never reads, for `reason`, such as one keyed to a
/// missing Project, with `caveats`: it competes with nothing and can only
/// be Removed.
fn unread_entry(
    definition: &AgentDefinition,
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
        plugin: plugin(entry),
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
        remove: remove(definition, location, entry),
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

/// A caveat per file that cannot be read and holds a Disable switch, a
/// rule or a gate that could apply to `entry` in the context of `project`,
/// each file once.
fn read_failure_caveats(
    definition: &AgentDefinition,
    snapshot: &Snapshot,
    entry: &ScannedEntry,
    project: Option<&ContextProject>,
) -> Vec<api::Caveat> {
    let switches = disable::failed_switch_files(definition, snapshot, entry, project);
    let settings = rules::failed_files(
        definition,
        snapshot,
        entry,
        project.map(|project| project.path),
    );
    let mut caveats: Vec<api::Caveat> = switches
        .iter()
        .map(|path| api::Caveat {
            code: api::CaveatCode::SwitchReadFailure,
            sentence: explain::switch_read_failure(path),
            path: path.clone(),
        })
        .collect();
    caveats.extend(
        settings
            .into_iter()
            .filter(|path| !switches.contains(path))
            .map(|path| api::Caveat {
                code: api::CaveatCode::SettingReadFailure,
                sentence: explain::setting_read_failure(&path),
                path,
            }),
    );
    caveats
}

/// For an Entry of settings, how many files of its source are read after
/// its own, each overriding it; 0 for any other Entry.
fn overridden(definition: &AgentDefinition, snapshot: &Snapshot, entry: &ScannedEntry) -> usize {
    let Some(id) = definition.location(&entry.location).anchor.settings() else {
        return 0;
    };
    snapshot
        .settings_of(entry.agent_index, id)
        .sources
        .iter()
        .find_map(|files| {
            let at = files.iter().position(|file| *file == entry.path)?;
            Some(files.len() - 1 - at)
        })
        .unwrap_or(0)
}

/// For an Entry in a source of settings the Agent skips, the settings' id
/// and the first file of the source it reads instead.
fn skipped_source<'s>(
    definition: &AgentDefinition,
    snapshot: &'s Snapshot,
    entry: &ScannedEntry,
) -> Option<(&'s str, &'s Path)> {
    let id = definition.location(&entry.location).anchor.settings()?;
    let settings = snapshot.settings_of(entry.agent_index, id);
    if !settings.skips(&entry.path) {
        return None;
    }
    Some((&settings.id, settings.read().first()?))
}

/// The gates of its Item kind that Block `entry` in the context of the
/// Project with the key `project`, each with the check it belongs to and
/// why, in the definition's order.
fn gates(
    definition: &AgentDefinition,
    snapshot: &Snapshot,
    entry: &ScannedEntry,
    project: Option<&str>,
) -> Vec<(Check, api::Reason)> {
    let location = definition.location(&entry.location);
    let roots = snapshot.roots(entry.agent_index, &entry.home);
    let agent_name = &definition.agent.name;
    definition
        .rules(location.kind)
        .gates
        .iter()
        .enumerate()
        .filter(|(_, gate)| gate.covers(&location.id))
        .filter_map(|(index, gate)| match gate {
            Gate::ManagedExclusive(block) => {
                let path = definition
                    .location(&block.location)
                    .anchor
                    .path(roots, None)
                    .filter(|path| snapshot.exclusive.contains(path))?;
                let reason = explain::managed_exclusive(agent_name, &path, location.kind);
                Some((Check::Policy, reason))
            }
            Gate::ApprovalList(block) => {
                let approved = (0..block.sources.len()).any(|source| {
                    let owner = SettingOwner::Gate {
                        gate: index,
                        source,
                    };
                    snapshot
                        .settings_for(entry, location.kind, owner, project)
                        .filter_map(|value| value.value.as_ref())
                        .any(|value| block.approves(value, &entry.name))
                });
                let reason =
                    explain::not_approved(agent_name, &entry.name, &entry.path, block.prompt);
                (!approved).then_some((Check::Approval, reason))
            }
            Gate::PolicyField(block) => {
                let set = (0..block.sources.len())
                    .flat_map(|source| {
                        let owner = SettingOwner::Gate {
                            gate: index,
                            source,
                        };
                        snapshot.settings_for(entry, location.kind, owner, project)
                    })
                    .find(|value| value.value.as_ref().is_some_and(PolicyField::set))?;
                let key = set.key_path.last().map_or("", String::as_str);
                let mut reason = explain::policy_field(key, &set.path, agent_name);
                reason.switch = Some(locator::render(&set.path, &set.key_path));
                Some((Check::Policy, reason))
            }
        })
        .collect()
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
    let condition = definition
        .location(&entry.location)
        .when
        .iter()
        .find(|condition| !condition.holds(&gate_on))?;
    match condition {
        Condition::FeatureGate(block) => Some((block.gate.as_str(), gate_on(&block.gate))),
    }
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

/// The plugin an Entry of a plugin is part of.
fn plugin(entry: &ScannedEntry) -> Option<api::Plugin> {
    entry.plugin.as_ref().map(|plugin| api::Plugin {
        id: plugin.id.clone(),
        name: plugin.name.clone(),
        scope: plugin.scope.clone(),
        version: plugin.version.clone(),
        folder: plugin.folder.clone(),
    })
}

/// Whether Remove is offered for `entry` of `location`: not for an Entry
/// config-digger only reads, nor for one of a plugin or extension, which
/// the Agent uninstalls, with its own command when it has one.
fn remove(
    definition: &AgentDefinition,
    location: &LocationDefinition,
    entry: &ScannedEntry,
) -> api::RemoveOption {
    if location.provenance.read_only() {
        return api::RemoveOption::NotOffered {
            code: api::RemoveNotOffered::ReadOnly,
            sentence: explain::read_only_remove_option(),
            how_to_remove: None,
        };
    }
    let Some(plugin) = &entry.plugin else {
        return api::RemoveOption::Offered;
    };
    let agent_name = &definition.agent.name;
    let command = definition
        .declared_plugins(&plugin.plugins)
        .expect("validation declares the plugins a location reads")
        .uninstall(&plugin.id, plugin.scope.as_deref());
    let (code, sentence) = match location.provenance {
        Provenance::Extension => (
            api::RemoveNotOffered::Extension,
            explain::extension_not_removed(&plugin.name, agent_name),
        ),
        _ => (
            api::RemoveNotOffered::Plugin,
            explain::plugin_not_removed(&plugin.id, agent_name),
        ),
    };
    api::RemoveOption::NotOffered {
        code,
        sentence,
        how_to_remove: command.map(|command| api::HowToRemove {
            sentence: explain::how_to_remove_plugin(&plugin.id, &command),
            command,
        }),
    }
}
