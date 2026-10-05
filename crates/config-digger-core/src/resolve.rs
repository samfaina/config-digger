//! `resolve(snapshot, context)`: a pure function from a snapshot and a context
//! to the Items of every Agent home and the State of each Entry.

use crate::api::{self, Context};
use crate::definitions::blocks::{
    self, Anchored, ApprovalHash, Built, Compete, Condition, Conform, FolderTrust, Gate, Holds,
    JsonFormat, Malformed, Merge, Parse, PolicyField, Provenance, SettingIs, SkillDefect,
    SkillFallback,
};
use crate::definitions::skill_managers::{self, Strength};
use crate::definitions::{
    AgentDefinition, Check, ItemKind, LocationDefinition, ReadFailureReaction, RiskCode,
};
use crate::disable::{self, ContextProject, Hit};
use crate::scan::{
    GateReading, HomeSource, ScannedEntry, ScannedGate, ScannedHome, ScannedParked,
    ScannedReadFailure, SettingOwner, Snapshot,
};
use crate::{explain, folders, installations, locator, paths, rules};
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
                source: match &home.source {
                    HomeSource::Default => api::HomeSource::Default,
                    HomeSource::Environment(variable) => api::HomeSource::Environment {
                        variable: variable.clone(),
                    },
                    HomeSource::KnownLocation => api::HomeSource::KnownLocation,
                    HomeSource::Added => api::HomeSource::Added,
                },
                hidden: home.hidden,
                mirror: home.mirror.as_ref().map(|mirror| api::Mirror {
                    owner: mirror.owner.clone(),
                    source: mirror.source.clone(),
                    source_folder: mirror.source_folder.clone(),
                    label: explain::mirror_label(&mirror.owner),
                    sentence: explain::mirror(&mirror.owner, mirror.source_folder.as_deref()),
                }),
                not_read: home.skipped.as_ref().map(|skipped| api::NotRead {
                    read_instead: skipped.instead.clone(),
                    sentence: explain::home_not_read(
                        &definition.agent.name,
                        &home.folder,
                        skipped.folder.as_deref(),
                    ),
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
    // location while a Feature gate or a setting keeps it closed.
    let unread = |entry: &ScannedEntry| {
        skipped_home(snapshot, entry).is_some()
            || (missing.is_some() && entry.project.is_some())
            || is_broken(entry)
            || closed(
                &definitions[entry.agent_index],
                snapshot,
                entry,
                project_key.map(String::as_str),
            )
            .is_some()
            || skipped_source(&definitions[entry.agent_index], snapshot, entry).is_some()
            || not_admin_owned(snapshot, entry)
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
        let key = if competes(definition, snapshot, entry, project_key.map(String::as_str)) {
            item_key(definition, snapshot, &in_context, entry, &unread)
        } else {
            ItemKey::Alone(
                entry.name.clone(),
                entry.path.clone(),
                entry.key_path.clone(),
            )
        };
        competing
            .entry((entry.agent_index, entry.home.as_str(), location.kind, key))
            .or_default()
            .push(entry);
    }

    // The flags of the Agents that refuse to start on a server they build.
    let mut conflicts: Vec<api::AgentFlag> = Vec::new();
    let mut items: Vec<api::Item> = competing
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
            // Where each parked Entry came from.
            let parked: Vec<Option<&ScannedParked>> = entries
                .iter()
                .map(|entry| snapshot.parked_of(entry))
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
                    && !entries[index]
                        .plugin
                        .as_ref()
                        .is_some_and(|plugin| plugin.name_shared)
                    && (malformed[index].is_none()
                        || definition
                            .location(&entries[index].location)
                            .shape
                            .as_ref()
                            .is_some_and(Conform::still_competes))
            };
            // The gates that keep each Entry out, each with its check.
            let gated: Vec<Vec<(Check, api::Reason)>> = entries
                .iter()
                .map(|entry| gates(definition, snapshot, entry, project_key.map(String::as_str)))
                .collect();
            // Whether the Agent keeps the Entry at `index` as far as its
            // switches, its gates and its shape go.
            let clear = |index: usize| {
                hits[index].0.is_empty()
                    && parked[index].is_none()
                    && !gated[index]
                        .iter()
                        .any(|(check, _)| before_precedence(*check))
                    && competes(index)
            };
            // Where the Agent merges same-named Entries, those it would let
            // win take part in the server it builds, highest first.
            let merge = definition.rules(kind).merge.as_ref();
            // Whether the Entry at `index` is of a location that merges.
            let merges =
                |index: usize| merge.is_some_and(|merge| merge.covers(&entries[index].location));
            // Of the Entries at `candidates`, those that take part.
            let taking_part = |candidates: Vec<usize>| -> Vec<usize> {
                let Some(merge) = merge else {
                    return Vec::new();
                };
                let ids: Vec<&str> = candidates
                    .iter()
                    .map(|&index| entries[index].location.as_str())
                    .collect();
                merge
                    .taking_part(&ids)
                    .into_iter()
                    .map(|at| candidates[at])
                    .collect()
            };
            let merging = taking_part(
                (0..entries.len())
                    .filter(|&index| merges(index) && clear(index))
                    .collect(),
            );
            let merged = Merged::of(merge, &entries, &merging);
            // Of the Entries at `taking`, the one on the same side of an
            // overlay as the Entry at `index`, which wins over it.
            let same_side = |taking: &[usize], index: usize| {
                let underneath =
                    |at: usize| merge.is_some_and(|merge| merge.underneath(&entries[at].location));
                taking
                    .iter()
                    .copied()
                    .find(|&other| underneath(other) == underneath(index))
            };
            // The rules that keep each Entry out, which see the server the
            // Agent builds in place of an Entry that takes part in it.
            let ruled: Vec<Vec<rules::RuleHit>> = entries
                .iter()
                .enumerate()
                .map(|(index, entry)| {
                    let built = merged
                        .as_ref()
                        .filter(|merged| merged.takes_part(index))
                        .map(|merged| &merged.built.config);
                    let project = project_key.map(String::as_str);
                    rules::hits(definition, snapshot, entry, project, built)
                })
                .collect();
            // The gates and rules that keep each Entry out, each with the
            // check it belongs to.
            let kept: Vec<Vec<(Check, api::State, api::Reason)>> = gated
                .iter()
                .zip(&ruled)
                .map(|(gated, ruled)| {
                    gated
                        .iter()
                        .map(|(check, reason)| (*check, api::State::Blocked, reason.clone()))
                        .chain(
                            ruled
                                .iter()
                                .map(|hit| (Check::Rules, hit.state, hit.reason.clone())),
                        )
                        .collect()
                })
                .collect();
            // The Agent skips a Disabled or Blocked Entry, and an Invalid one
            // that does not compete, so the highest of the others wins. A rule
            // keeps out a server the Agent builds whole, which still holds
            // its name.
            let free = |index: usize| {
                clear(index)
                    && (merges(index)
                        || ruled[index].is_empty()
                        || !before_precedence(Check::Rules))
            };
            let winner = (0..entries.len()).find(|&index| free(index));
            // The URL each remote Entry competes by, while it does.
            let urls: Vec<Option<String>> = entries
                .iter()
                .map(|entry| url_key(definition, snapshot, entry, &unread))
                .collect();
            let shadowed_by = |index: usize, winner: usize| {
                let same_endpoint = definition
                    .rules(kind)
                    .competition
                    .by_endpoint(&entries[index].location)
                    && blocks::endpoint(&entries[index].config)
                        == blocks::endpoint(&entries[winner].config);
                let same_url = urls[index].is_some() && urls[index] == urls[winner];
                explain::shadowed_by_precedence(
                    agent_name,
                    &definition.location(&entries[winner].location).scope,
                    &locator::render(&entries[winner].path, &entries[winner].key_path),
                    rank(entries[index]) == rank(entries[winner]),
                    same_endpoint || same_url,
                )
            };
            let api_entries: Vec<api::Entry> = entries
                .iter()
                .enumerate()
                .map(|(index, entry)| {
                    let location = definition.location(&entry.location);
                    let mut gate_caveats =
                        gate_caveats(definition, snapshot, entry, project_key.map(String::as_str));
                    if location.cached_at.is_some()
                        && let Some(&at) = snapshot.cached.get(&entry.path)
                    {
                        gate_caveats.push(api::Caveat {
                            code: api::CaveatCode::CachedCopy,
                            path: entry.path.clone(),
                            sentence: explain::cached_copy(agent_name, at),
                        });
                    }
                    // Servers config-digger cannot see can take part where URLs compete.
                    let unseen = definition.rules(kind).competition.same_url();
                    if let (Some(block), Some(_)) = (unseen, &urls[index])
                        && !block.unseen.is_empty()
                    {
                        gate_caveats.push(api::Caveat {
                            code: api::CaveatCode::UnseenServers,
                            path: entry.path.clone(),
                            sentence: explain::unseen_servers(agent_name, &block.unseen),
                        });
                    }
                    if let Some(home) = skipped_home(snapshot, entry) {
                        let instead = home
                            .skipped
                            .as_ref()
                            .and_then(|skipped| skipped.folder.as_deref());
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
                    if let Some(closed) =
                        closed(definition, snapshot, entry, project_key.map(String::as_str))
                    {
                        let reason = match closed {
                            Closed::Gate(gate, on) => explain::feature_gate(agent_name, gate, on),
                            Closed::Setting(setting) => explain::needs_setting(agent_name, setting),
                        };
                        return unread_entry(
                            definition,
                            snapshot,
                            entry,
                            location,
                            reason,
                            gate_caveats,
                        );
                    }
                    if not_admin_owned(snapshot, entry) {
                        let reason = explain::system_file_not_admin_owned(agent_name, &entry.path);
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
                            Check::Policy | Check::Approval | Check::Trust | Check::Rules => failed
                                .extend(
                                    kept[index]
                                        .iter()
                                        .filter(|(known, _, _)| *known == check)
                                        .cloned(),
                                ),
                            Check::Disable => {
                                if let Some(parked) = parked[index] {
                                    failed.push((
                                        check,
                                        api::State::Disabled,
                                        api::Reason {
                                            code: api::ReasonCode::Parked,
                                            effective: None,
                                            switch: None,
                                            sentence: explain::parked(
                                                &parked.origin,
                                                &entry.path,
                                                agent_name,
                                            ),
                                        },
                                    ));
                                }
                                failed.extend(disabling.iter().map(|hit| {
                                    (check, api::State::Disabled, hit.reason(agent_name))
                                }));
                                // A field that turns the built server off
                                // turns off every Entry that could take part.
                                if let Some(off) = merged
                                    .as_ref()
                                    .and_then(Merged::off)
                                    .filter(|_| merges(index))
                                {
                                    failed.push((
                                        check,
                                        api::State::Disabled,
                                        explain::turned_off(
                                            agent_name,
                                            &name,
                                            &off.field,
                                            &off.switch,
                                        ),
                                    ));
                                }
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
                            Check::PluginName => {
                                if let Some(plugin) =
                                    entry.plugin.as_ref().filter(|plugin| plugin.name_shared)
                                {
                                    failed.push((
                                        check,
                                        api::State::Invalid,
                                        explain::duplicate_plugin_name(agent_name, &plugin.name),
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
                            Check::Merge => {
                                if let Some(merged) = merged
                                    .as_ref()
                                    .filter(|merged| merged.takes_part(index))
                                    .filter(|merged| !merged.conflict.is_empty())
                                {
                                    failed.push((
                                        check,
                                        api::State::Invalid,
                                        explain::transport_conflict(
                                            agent_name,
                                            &name,
                                            &merged.conflict,
                                        ),
                                    ));
                                }
                            }
                            // Same-named Entries the Agent merges take part
                            // together, and none Shadows another.
                            Check::Precedence if merges(index) => match merged.as_ref() {
                                Some(merged) if merged.takes_part(index) => {
                                    if merged.with_others() {
                                        failed.push((
                                            check,
                                            api::State::Merged,
                                            explain::merged(
                                                agent_name,
                                                &name,
                                                merged.taking_part.len(),
                                                &merged.fields_of(index),
                                            ),
                                        ));
                                    }
                                }
                                // One the Entry it lies with wins over.
                                Some(merged) if clear(index) => {
                                    if let Some(winner) = same_side(&merged.taking_part, index) {
                                        failed.push((
                                            check,
                                            api::State::Shadowed,
                                            shadowed_by(index, winner),
                                        ));
                                    }
                                }
                                _ => {}
                            },
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
                    let off = merged.as_ref().and_then(Merged::off);
                    // The State it would have once what sets its State is
                    // lifted. Enable lifts every switch that Disables it, and
                    // with them those of the Entries they Disable too.
                    let what_if =
                        matches!(state, api::State::Disabled | api::State::Blocked).then(|| {
                            let lifted = |other: usize| match state {
                                api::State::Disabled => {
                                    parked[other].is_none()
                                        && hits[other].0.iter().all(|hit| {
                                            disabling.iter().any(|own| own.same_switch(hit))
                                        })
                                        && !kept[other]
                                            .iter()
                                            .any(|(check, _, _)| before_precedence(*check))
                                        && competes(other)
                                }
                                _ => free(other),
                            };
                            let render = |next: api::State, shadowed_by: Option<(&str, &str)>| {
                                if state == api::State::Disabled
                                    && by_rule.is_empty()
                                    && off.is_none()
                                {
                                    explain::what_if(next, shadowed_by)
                                } else {
                                    explain::what_if_otherwise(state, next, shadowed_by)
                                }
                            };
                            for &check in checks {
                                // It would take part with the others that
                                // would, or lose to the one it lies with.
                                if check == Check::Precedence && merges(index) {
                                    let would = taking_part(
                                        (0..entries.len())
                                            .filter(|&other| {
                                                merges(other) && (other == index || lifted(other))
                                            })
                                            .collect(),
                                    );
                                    if let Some(winner) =
                                        same_side(&would, index).filter(|_| !would.contains(&index))
                                    {
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
                                    let next = if would.len() > 1 {
                                        api::State::Merged
                                    } else {
                                        api::State::Effective
                                    };
                                    return api::WhatIf {
                                        state: next,
                                        effective: None,
                                        sentence: render(next, None),
                                    };
                                }
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
                    // Enable lifts no rule, and writes nothing in a Mirrored home.
                    let mirrored = snapshot
                        .mirror_of(entry)
                        .filter(|_| state == api::State::Disabled)
                        .map(|mirror| {
                            explain::mirrored_option(
                                "Enable",
                                &mirror.owner,
                                mirror.source_folder.as_deref(),
                            )
                        });
                    let turned_off = off
                        .as_ref()
                        .filter(|_| state == api::State::Disabled)
                        .map(|off| explain::turned_off_enable(&off.field, &off.switch));
                    let enable = match mirrored
                        .or_else(|| rules::enable_not_offered(&by_rule))
                        .or(turned_off)
                    {
                        Some(sentence) => Some(api::EnableOption::NotOffered { sentence }),
                        None => {
                            let unparks = parked[index]
                                .map(|parked| explain::enable_unparks(&entry.name, &parked.origin));
                            let lifts = (!disabling.is_empty())
                                .then(|| disable::enable(location, disabling))
                                .transpose();
                            match lifts {
                                Err(sentence) => Some(api::EnableOption::NotOffered { sentence }),
                                Ok(lifts) => {
                                    let sentences: Vec<String> =
                                        unparks.into_iter().chain(lifts).collect();
                                    (!sentences.is_empty()).then(|| api::EnableOption::Offered {
                                        sentence: sentences.join(" "),
                                    })
                                }
                            }
                        }
                    };
                    let disable = if disabling.is_empty()
                        && by_rule.is_empty()
                        && parked[index].is_none()
                        && off.is_none()
                    {
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
                        parked: parked_from(snapshot, entry),
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
                            .chain(needs_trusted_folder(
                                definition,
                                snapshot,
                                entry,
                                project_key.map(String::as_str),
                            ))
                            .chain(cli_view_differs(
                                merge,
                                merged.as_ref(),
                                index,
                                entry,
                                agent_name,
                            ))
                            .collect(),
                        config: entry.config.clone(),
                        masked: entry.masked.clone(),
                        disable,
                        enable,
                        remove: remove(definition, snapshot, location, entry),
                        caveats: read_failure_caveats(
                            definition,
                            snapshot,
                            entry,
                            context_project.as_ref(),
                        )
                        .into_iter()
                        .chain(gate_caveats)
                        .collect(),
                        also_read_by: Vec::new(),
                        copied_from: None,
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
            // The Agent refuses to start on a server it rejects.
            if let Some(merged) = merged.as_ref().filter(|merged| !merged.conflict.is_empty()) {
                let highest = entries[merged.taking_part[0]];
                conflicts.push(api::AgentFlag {
                    agent: definition.agent.id.clone(),
                    agent_name: agent_name.clone(),
                    home: home.to_owned(),
                    code: api::AgentFlagCode::CannotStart,
                    path: highest.path.clone(),
                    sentence: explain::cannot_start_conflict(agent_name, &name, &merged.conflict),
                });
            }
            api::Item {
                agent: definition.agent.id.clone(),
                home: home.to_owned(),
                kind: kind.into(),
                built: merged
                    .as_ref()
                    .filter(|merged| merged.with_others())
                    .map(|merged| merged.server(&entries)),
                name,
                state,
                entries: api_entries,
                caveats,
            }
        })
        .collect();

    also_read_by(definitions, &mut items);
    from_copied(definitions, snapshot, &mut items);

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
        .chain(conflicts)
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

/// The server an Agent that merges same-named Entries builds from those of
/// one Item that take part.
struct Merged<'m> {
    /// The indexes of the Entries that take part among the Item's, highest
    /// first.
    taking_part: Vec<usize>,
    built: Built,
    /// The fields of the built server that exclude each other, when it
    /// holds more than one of them.
    conflict: Vec<&'m str>,
    /// The field whose `false` turns the server off, if it has one.
    enabled: Option<&'m str>,
    /// The file and key path of each Entry of the Item.
    places: Vec<(PathBuf, Vec<String>)>,
}

/// The field that turns a built server off, and where it is set.
struct Off {
    field: String,
    /// The `<path>#<key path>` locator of the field in its Entry.
    switch: String,
}

impl<'m> Merged<'m> {
    /// What `merge` builds from the Entries at `taking_part` among
    /// `entries`; `None` when the kind does not merge or none takes part.
    fn of(
        merge: Option<&'m Merge>,
        entries: &[&ScannedEntry],
        taking_part: &[usize],
    ) -> Option<Merged<'m>> {
        let merge = merge.filter(|_| !taking_part.is_empty())?;
        let configs: Vec<&serde_json::Value> = taking_part
            .iter()
            .map(|&index| &entries[index].config)
            .collect();
        let mut built = merge.build(&configs);
        // Origins point at the Item's Entries rather than those taking part.
        for (_, origin) in &mut built.origins {
            *origin = taking_part[*origin];
        }
        Some(Merged {
            conflict: merge.conflict(&built.config),
            enabled: merge.enabled(),
            taking_part: taking_part.to_vec(),
            places: entries
                .iter()
                .map(|entry| (entry.path.clone(), entry.key_path.clone()))
                .collect(),
            built,
        })
    }

    /// The `<path>#<key path>` locator of the Entry at `index`.
    fn locator(&self, index: usize) -> String {
        let (path, key_path) = &self.places[index];
        locator::render(path, key_path)
    }

    fn takes_part(&self, index: usize) -> bool {
        self.taking_part.contains(&index)
    }

    /// Whether more than one Entry takes part, so that each is Merged.
    fn with_others(&self) -> bool {
        self.taking_part.len() > 1
    }

    /// The fields of the server that come from the Entry at `index`, a
    /// list it supplies items of once.
    fn fields_of(&self, index: usize) -> Vec<Vec<String>> {
        let mut fields: Vec<Vec<String>> = Vec::new();
        for (field, _) in self
            .built
            .origins
            .iter()
            .filter(|(_, origin)| *origin == index)
        {
            let mut field = field.clone();
            let in_list = field.split_last().is_some_and(|(_, list)| {
                list.iter()
                    .try_fold(&self.built.config, |value, key| value.get(key))
                    .is_some_and(serde_json::Value::is_array)
            });
            if in_list {
                field.pop();
            }
            if !fields.contains(&field) {
                fields.push(field);
            }
        }
        fields
    }

    /// The field that turns the server off, when it holds `false`, and the
    /// Entry it comes from.
    fn off(&self) -> Option<Off> {
        let field = self.enabled?;
        if self.built.config.get(field) != Some(&serde_json::Value::Bool(false)) {
            return None;
        }
        let (_, origin) = self
            .built
            .origins
            .iter()
            .find(|(known, _)| *known == [field])?;
        let (path, key_path) = &self.places[*origin];
        let mut key_path = key_path.clone();
        key_path.push(field.to_owned());
        Some(Off {
            field: field.to_owned(),
            switch: locator::render(path, &key_path),
        })
    }

    /// The server as an Item shows it, each Secret masked as its Entry's is.
    fn server(&self, entries: &[&ScannedEntry]) -> api::BuiltServer {
        let masked = self
            .built
            .origins
            .iter()
            .flat_map(|(field, origin)| {
                entries[*origin]
                    .masked
                    .iter()
                    .filter(move |masked| masked.starts_with(field))
                    .cloned()
            })
            .collect();
        api::BuiltServer {
            config: self.built.config.clone(),
            masked,
            origins: self
                .built
                .origins
                .iter()
                .map(|(field, origin)| api::FieldOrigin {
                    key_path: field.clone(),
                    entry: self.locator(*origin),
                })
                .collect(),
        }
    }
}

/// The note that the Agent's own command lists the server built from the
/// Entry at `index`, which lies underneath another, as that other alone.
fn cli_view_differs(
    merge: Option<&Merge>,
    merged: Option<&Merged>,
    index: usize,
    entry: &ScannedEntry,
    agent_name: &str,
) -> Option<api::Note> {
    let command = merge?.cli_lists_overlay()?;
    let merged = merged.filter(|merged| merged.takes_part(index) && merged.with_others())?;
    let over = merged.taking_part[0];
    (merge?.underneath(&entry.location)).then(|| api::Note {
        code: api::NoteCode::CliViewDiffers,
        switch: None,
        sentence: explain::cli_view_differs(agent_name, command, &merged.locator(over)),
    })
}

/// What an Entry competes by: Entries with the same key are one Item.
#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum ItemKey {
    /// The competition key of a name.
    Name(String),
    /// The URL a remote Entry competes by, while it does.
    Url(String),
    /// An endpoint no Entry that competes by name has.
    Endpoint(String),
    /// The Entry's own name, path and key path, when it competes with none.
    Alone(String, PathBuf, Vec<String>),
}

/// Whether the Entries of the Agent home of `entry` compete in the context
/// of the Project with the key `project`: always, unless their competition
/// depends on a setting that does not hold there.
fn competes(
    definition: &AgentDefinition,
    snapshot: &Snapshot,
    entry: &ScannedEntry,
    project: Option<&str>,
) -> bool {
    let kind = definition.location(&entry.location).kind;
    let Some(setting) = definition.rules(kind).competition.only_while() else {
        return true;
    };
    setting.is_met((0..setting.sources.len()).map(|source| {
        setting_value(
            snapshot,
            entry,
            kind,
            SettingOwner::Competition { source },
            project,
        )
    }))
}

/// What `entry`, among the Entries `in_context` of the context, competes
/// by: its name, or for an Entry of a location that competes by endpoint,
/// the name of the Entry the Agent ranks highest among those of other
/// locations that have its endpoint and are read, or else its endpoint; or
/// for a remote Entry that competes by URL, its URL.
fn item_key(
    definition: &AgentDefinition,
    snapshot: &Snapshot,
    in_context: &[&ScannedEntry],
    entry: &ScannedEntry,
    unread: &dyn Fn(&ScannedEntry) -> bool,
) -> ItemKey {
    if let Some(url) = url_key(definition, snapshot, entry, unread) {
        return ItemKey::Url(url);
    }
    let location = definition.location(&entry.location);
    let competition = &definition.rules(location.kind).competition;
    // A parked Entry competes as from where it was.
    let by_name = |entry: &ScannedEntry| {
        let path = snapshot
            .parked_of(entry)
            .map_or(&entry.path, |parked| &parked.origin);
        ItemKey::Name(competition.key(&entry.name, path))
    };
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
        .filter(|other| other.agent_index == entry.agent_index && other.home == entry.home)
        .filter(|other| {
            let other_location = definition.location(&other.location);
            other_location.kind == location.kind
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

/// Where `entry` came from, when it is parked.
fn parked_from(snapshot: &Snapshot, entry: &ScannedEntry) -> Option<api::Parked> {
    snapshot.parked_of(entry).map(|parked| api::Parked {
        path: parked.origin.clone(),
        key_path: parked.origin_key_path.clone(),
        locator: locator::render(&parked.origin, &parked.origin_key_path),
        parked_at: parked.parked_at.clone(),
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
        parked: parked_from(snapshot, entry),
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
        remove: remove(definition, snapshot, location, entry),
        caveats,
        also_read_by: Vec::new(),
        copied_from: None,
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

/// Whether `entry` is in a file its Agent skips unread, since someone other
/// than an administrator can change it.
fn not_admin_owned(snapshot: &Snapshot, entry: &ScannedEntry) -> bool {
    snapshot
        .not_admin_owned
        .holds(entry.agent_index, &entry.path)
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
pub(crate) fn gates(
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
                    .find(|value| value.value.as_ref().is_some_and(PolicyField::keeps_out))?;
                let key = set.key_path.last().map_or("", String::as_str);
                let mut reason = explain::policy_field(key, &set.path, agent_name);
                reason.switch = Some(locator::render(&set.path, &set.key_path));
                Some((Check::Policy, reason))
            }
            Gate::TrustedFolders(block) => {
                let values = |source: usize| {
                    let owner = SettingOwner::Gate {
                        gate: index,
                        source,
                    };
                    snapshot
                        .settings_for(entry, location.kind, owner, project)
                        .filter_map(|value| value.value.as_ref())
                };
                if let Some(setting) = &block.trust_all {
                    let first = block.sources.len();
                    let held = (0..setting.sources.len())
                        .map(|source| values(first + source).next().cloned());
                    if setting.is_met(held) {
                        return None;
                    }
                }
                let trusted: Vec<PathBuf> = (0..block.sources.len())
                    .flat_map(|source| {
                        values(source).flat_map(|value| block.trusted_folders(value))
                    })
                    .collect();
                let scanned = snapshot
                    .projects
                    .iter()
                    .find(|known| Some(known.key.as_str()) == entry.project.as_deref())?;
                // A folder is trusted with the folders above it.
                let is_trusted = |folder: &Path| {
                    trusted
                        .iter()
                        .any(|trusted| folders::below(folder, trusted).is_some())
                };
                let all_trusted = std::iter::once(&scanned.path)
                    .chain(&scanned.members)
                    .all(|folder| is_trusted(folder));
                let reason = explain::untrusted_folder(agent_name, &scanned.path);
                (!all_trusted).then_some((Check::Trust, reason))
            }
            Gate::ProjectTrust(block) => {
                let tables: Vec<&serde_json::Value> = (0..block.sources.len())
                    .flat_map(|source| {
                        let owner = SettingOwner::Gate {
                            gate: index,
                            source,
                        };
                        snapshot.settings_for(entry, location.kind, owner, project)
                    })
                    .filter_map(|value| value.value.as_ref())
                    .collect();
                let scanned = snapshot
                    .projects
                    .iter()
                    .find(|known| Some(known.key.as_str()) == entry.project.as_deref())?;
                // The folder the Entry's location is in, as far up as the
                // Project's folder as it was read.
                let folder = scanned.path.ancestors().nth(entry.depth)?;
                let root = scanned.repo_root.as_deref().unwrap_or(&scanned.path);
                let level = [Some(folder), Some(root), scanned.main_checkout.as_deref()]
                    .into_iter()
                    .flatten()
                    .find_map(|folder| tables.iter().find_map(|table| block.level(table, folder)));
                let reason = explain::untrusted_folder(agent_name, folder);
                (level != Some(block.trusted.as_str())).then_some((Check::Trust, reason))
            }
            // The folder the Agent starts in, whatever the Entry's location.
            Gate::FolderTrust(block) => {
                let scanned = snapshot
                    .projects
                    .iter()
                    .find(|known| Some(known.key.as_str()) == project)?;
                let rules = TrustRules {
                    snapshot,
                    entry,
                    kind: location.kind,
                    gate: index,
                    block,
                    project,
                };
                if rules.trusts(&scanned.path) {
                    return None;
                }
                let reason = explain::untrusted_folder(agent_name, &scanned.path);
                Some((Check::Trust, reason))
            }
            Gate::ApprovalHash(block) => {
                let id = entry.switch_id.as_deref().unwrap_or(&entry.name);
                let hash = entry.config_hash.as_deref();
                let approved = (0..block.sources.len()).any(|source| {
                    let owner = SettingOwner::Gate {
                        gate: index,
                        source,
                    };
                    snapshot
                        .settings_for(entry, location.kind, owner, project)
                        .filter_map(|value| value.value.as_ref())
                        .any(|value| ApprovalHash::approves(value, id, hash))
                });
                let reason =
                    explain::not_approved(agent_name, &entry.name, &entry.path, block.prompt);
                (!approved).then_some((Check::Approval, reason))
            }
            Gate::NonceTrust(block) => {
                let id = entry.switch_id.as_deref().unwrap_or(&entry.name);
                let trusted = (0..block.sources.len()).any(|source| {
                    let owner = SettingOwner::Gate {
                        gate: index,
                        source,
                    };
                    snapshot
                        .settings_for(entry, location.kind, owner, project)
                        .filter_map(|value| value.value.as_ref())
                        .any(|value| block.trusts(value, id))
                });
                let reason = explain::not_trusted(agent_name, &entry.name, &entry.path);
                (!trusted).then_some((Check::Approval, reason))
            }
        })
        .collect()
}

/// A folder-trust gate, the gate at `gate` among those of `kind`, as the
/// Agent home of `entry` reads it in the context of the Project with the
/// key `project`.
struct TrustRules<'a> {
    snapshot: &'a Snapshot,
    entry: &'a ScannedEntry,
    kind: ItemKind,
    gate: usize,
    block: &'a FolderTrust,
    project: Option<&'a str>,
}

impl TrustRules<'_> {
    /// What the gate's source at `source` holds, its setting's after them.
    fn value(&self, source: usize) -> Option<serde_json::Value> {
        let owner = SettingOwner::Gate {
            gate: self.gate,
            source,
        };
        setting_value(self.snapshot, self.entry, self.kind, owner, self.project)
    }

    /// Whether its setting trusts every folder.
    fn trust_all(&self) -> bool {
        let Some(setting) = &self.block.trust_all else {
            return false;
        };
        let first = self.block.sources.len();
        setting.is_met((0..setting.sources.len()).map(|source| self.value(first + source)))
    }

    /// Whether it trusts `folder`: the first of its sources that holds
    /// rules decides, unless its setting trusts every folder.
    fn trusts(&self, folder: &Path) -> bool {
        self.trust_all()
            || (0..self.block.sources.len())
                .find_map(|source| self.value(source))
                .is_some_and(|rules| self.block.trusts(&rules, folder))
    }
}

/// Whether the Agent home of `entry` trusts the folder of the Project with
/// the key `project`, as every folder-trust gate of the Item kind `kind`
/// reads it. With no Project, or no such gate, it does.
pub(crate) fn project_trusted(
    definition: &AgentDefinition,
    snapshot: &Snapshot,
    entry: &ScannedEntry,
    kind: ItemKind,
    project: Option<&str>,
) -> bool {
    let Some(scanned) =
        project.and_then(|key| snapshot.projects.iter().find(|known| known.key == key))
    else {
        return true;
    };
    definition
        .rules(kind)
        .gates
        .iter()
        .enumerate()
        .all(|(gate, block)| match block {
            Gate::FolderTrust(block) => TrustRules {
                snapshot,
                entry,
                kind,
                gate,
                block,
                project,
            }
            .trusts(&scanned.path),
            _ => true,
        })
}

/// The note on an Entry, in the context of no Project, that a folder-trust
/// gate of its Item kind Blocks it wherever the Agent starts in a folder
/// the user has not trusted, unless the gate trusts every folder.
fn needs_trusted_folder(
    definition: &AgentDefinition,
    snapshot: &Snapshot,
    entry: &ScannedEntry,
    project: Option<&str>,
) -> Option<api::Note> {
    if project.is_some() {
        return None;
    }
    let location = definition.location(&entry.location);
    let kind = location.kind;
    definition
        .rules(kind)
        .gates
        .iter()
        .enumerate()
        .any(|(gate, block)| match block {
            Gate::FolderTrust(block) if block.locations.contains(&location.id) => {
                let rules = TrustRules {
                    snapshot,
                    entry,
                    kind,
                    gate,
                    block,
                    project,
                };
                !rules.trust_all()
            }
            _ => false,
        })
        .then(|| api::Note {
            code: api::NoteCode::UntrustedFolderBlocks,
            switch: None,
            sentence: explain::untrusted_folder_blocks(&definition.agent.name),
        })
}

/// Whether the Feature gate `name` of the Agent home of `entry` is on: the
/// value the Agent last cached, or else the definition's assumed default.
pub(crate) fn gate_on(
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

/// What the source of a setting `owner` names holds for `entry` in the
/// context of the Project with the key `project`, the first of its files
/// that holds it; `None` when none does.
fn setting_value(
    snapshot: &Snapshot,
    entry: &ScannedEntry,
    kind: ItemKind,
    owner: SettingOwner,
    project: Option<&str>,
) -> Option<serde_json::Value> {
    snapshot
        .settings_for(entry, kind, owner, project)
        .find_map(|value| value.value.clone())
}

/// A `when` condition of a location that does not hold.
enum Closed<'d> {
    /// The Feature gate it depends on, and whether that gate is on.
    Gate(&'d str, bool),
    /// The setting that does not have the value it needs.
    Setting(&'d SettingIs),
}

/// The first `when` condition of the location of `entry` that does not
/// hold in the context of the Project with the key `project`.
fn closed<'d>(
    definition: &'d AgentDefinition,
    snapshot: &Snapshot,
    entry: &ScannedEntry,
    project: Option<&str>,
) -> Option<Closed<'d>> {
    let gate_on = |name: &str| gate_on(definition, snapshot, entry, name);
    let location = definition
        .locations
        .iter()
        .position(|location| location.id == entry.location)
        .expect("snapshot location ids come from this definition");
    let kind = definition.locations[location].kind;
    let (_, condition) = definition.locations[location]
        .when
        .iter()
        .enumerate()
        .find(|(condition, block)| {
            let setting = |source: usize| {
                let owner = SettingOwner::Condition {
                    location,
                    condition: *condition,
                    source,
                };
                setting_value(snapshot, entry, kind, owner, project)
            };
            !block.holds(&gate_on, &setting)
        })?;
    Some(match condition {
        Condition::FeatureGate(block) => Closed::Gate(block.gate.as_str(), gate_on(&block.gate)),
        Condition::Setting(block) => Closed::Setting(block),
    })
}

/// A caveat per Feature gate that affects the location of `entry`, saying
/// which value it is resolved with and how certain that is, or for a gate
/// the Agent caches nowhere, that it may apply in the context of the
/// Project with the key `project`.
fn gate_caveats(
    definition: &AgentDefinition,
    snapshot: &Snapshot,
    entry: &ScannedEntry,
    project: Option<&str>,
) -> Vec<api::Caveat> {
    let agent_name = &definition.agent.name;
    let kind = definition.location(&entry.location).kind;
    definition
        .feature_gates
        .iter()
        .enumerate()
        .filter(|(_, gate)| gate.affects.contains(&entry.location))
        .filter_map(|(index, gate)| {
            // The Agent fetches the gate, and caches it nowhere to read.
            if let Some(when) = &gate.when {
                let held = (0..when.sources.len()).map(|source| {
                    let owner = SettingOwner::FeatureGate {
                        gate: index,
                        source,
                    };
                    let found = snapshot
                        .settings_for(entry, kind, owner, project)
                        .find(|value| value.value.is_some());
                    (
                        found.and_then(|value| value.value.clone()),
                        found.map(|value| value.path.clone()),
                    )
                });
                let path = when.met_at(held)??;
                return Some(api::Caveat {
                    code: api::CaveatCode::GateMayApply,
                    path,
                    sentence: explain::gate_may_apply(agent_name, &gate.name),
                });
            }
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
    snapshot: &Snapshot,
    location: &LocationDefinition,
    entry: &ScannedEntry,
) -> api::RemoveOption {
    if let Some(mirror) = snapshot.mirror_of(entry) {
        return api::RemoveOption::NotOffered {
            code: api::RemoveNotOffered::Mirrored,
            sentence: explain::mirrored_option(
                "Remove",
                &mirror.owner,
                mirror.source_folder.as_deref(),
            ),
            how_to_remove: None,
        };
    }
    if location.provenance.read_only() {
        return api::RemoveOption::NotOffered {
            code: api::RemoveNotOffered::ReadOnly,
            sentence: explain::read_only_remove_option(),
            how_to_remove: None,
        };
    }
    let agent_name = &definition.agent.name;
    let Some(plugin) = &entry.plugin else {
        // An extension's server the Agent caches is the extension's to remove.
        return match location.provenance {
            Provenance::Extension => api::RemoveOption::NotOffered {
                code: api::RemoveNotOffered::Extension,
                sentence: explain::cached_extension_not_removed(agent_name),
                how_to_remove: None,
            },
            Provenance::File
            | Provenance::Plugin
            | Provenance::Managed
            | Provenance::CloudMirror
            | Provenance::BuiltIn
            | Provenance::AccountManaged => api::RemoveOption::Offered,
        };
    };
    let command = definition
        .declared_plugins(&plugin.plugins)
        .expect("validation declares the plugins a location reads")
        .uninstall(&plugin.id, plugin.scope.as_deref());
    // An extension goes by the name its Agent's settings show it under, a
    // plugin by the id its uninstall command takes.
    let (code, sentence) = match location.provenance {
        Provenance::Extension => (
            api::RemoveNotOffered::Extension,
            explain::extension_not_removed(&plugin.name, agent_name),
        ),
        Provenance::Plugin
        | Provenance::File
        | Provenance::Managed
        | Provenance::CloudMirror
        | Provenance::BuiltIn
        | Provenance::AccountManaged => (
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

/// Gives each Entry of a Mirrored home the actions of the Entry it was
/// copied from, which they go to, and that Entry's locator. An Entry the
/// home it is copied from lacks keeps its own actions, which refuse.
fn from_copied(definitions: &[AgentDefinition], snapshot: &Snapshot, items: &mut [api::Item]) {
    let mut copied: Vec<(usize, usize, api::Entry)> = Vec::new();
    for (at, item) in items.iter().enumerate() {
        let Some(agent) = definitions
            .iter()
            .position(|definition| definition.agent.id == item.agent)
        else {
            continue;
        };
        for (index, entry) in item.entries.iter().enumerate() {
            let source = snapshot
                .entry_in(agent, &item.home, &entry.path, &entry.key_path)
                .and_then(|scanned| snapshot.copied_from(scanned));
            let Some(source) = source else {
                continue;
            };
            let locator = locator::render(&source.path, &source.key_path);
            let found = items
                .iter()
                .filter(|other| other.agent == item.agent && other.home == source.home)
                .flat_map(|other| &other.entries)
                .find(|other| other.locator == locator);
            if let Some(found) = found {
                copied.push((at, index, found.clone()));
            }
        }
    }
    for (at, index, source) in copied {
        let entry = &mut items[at].entries[index];
        entry.disable = source.disable;
        entry.enable = source.enable;
        entry.remove = source.remove;
        entry.copied_from = Some(source.locator);
    }
}

/// Fills in, on each Entry of `items`, every Agent home of another Agent
/// whose Items hold the same Entry, with its State there.
fn also_read_by(definitions: &[AgentDefinition], items: &mut [api::Item]) {
    // Every Entry once per Agent home that holds it, by its locator.
    let mut readers: Vec<(String, api::AlsoReadBy)> = Vec::new();
    for item in items.iter() {
        let agent_name = definitions
            .iter()
            .find(|definition| definition.agent.id == item.agent)
            .map_or(item.agent.as_str(), |definition| &definition.agent.name);
        for entry in &item.entries {
            let reader = api::AlsoReadBy {
                agent: item.agent.clone(),
                agent_name: agent_name.to_owned(),
                home: item.home.clone(),
                state: entry.state,
                sentence: explain::also_read_by(agent_name, &item.home, entry.state),
            };
            readers.push((entry.locator.clone(), reader));
        }
    }
    readers.sort_by(|(_, a), (_, b)| (&a.agent, &a.home).cmp(&(&b.agent, &b.home)));
    for item in items.iter_mut() {
        for entry in &mut item.entries {
            entry.also_read_by = readers
                .iter()
                .filter(|(locator, reader)| *locator == entry.locator && reader.agent != item.agent)
                .map(|(_, reader)| reader.clone())
                .collect();
        }
    }
}

/// The URL `entry` competes by, when its location's remote Entries compete
/// by URL, the Feature gate that makes them is on, and it is read.
fn url_key(
    definition: &AgentDefinition,
    snapshot: &Snapshot,
    entry: &ScannedEntry,
    unread: &dyn Fn(&ScannedEntry) -> bool,
) -> Option<String> {
    let location = definition.location(&entry.location);
    let block = definition.rules(location.kind).competition.same_url()?;
    if unread(entry) || !gate_on(definition, snapshot, entry, &block.gate) {
        return None;
    }
    block.url_of(&location.id, &entry.config)
}
