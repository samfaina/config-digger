//! `resolve(snapshot, context)`: a pure function from a snapshot and a context
//! to the Items of every Agent home and the State of each Entry.

use crate::api::{self, Context};
use crate::definitions::AgentDefinition;
use crate::definitions::blocks::Compete;
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
            entries.sort_by_key(|entry| definition.rank(definition.location(&entry.location)));
            // An Item is named after the Entry that would be Effective.
            let name = entries[0].name.clone();
            let winner = locator::render(&entries[0].path, &entries[0].key_path);
            let winner_scope = &definition.location(&entries[0].location).scope;
            let entries: Vec<api::Entry> = entries
                .iter()
                .enumerate()
                .map(|(rank, entry)| {
                    let location = definition.location(&entry.location);
                    let (state, reason) = if rank == 0 {
                        (api::State::Effective, None)
                    } else {
                        let reason = explain::shadowed_by_precedence(
                            &definition.agent.name,
                            winner_scope,
                            &winner,
                        );
                        (api::State::Shadowed, Some(reason))
                    };
                    api::Entry {
                        path: entry.path.clone(),
                        key_path: entry.key_path.clone(),
                        locator: locator::render(&entry.path, &entry.key_path),
                        scope: location.scope.clone(),
                        provenance: location.provenance.into(),
                        state,
                        reason,
                    }
                })
                .collect();
            api::Item {
                agent: definition.agent.id.clone(),
                home: home.to_owned(),
                kind: kind.into(),
                name,
                state: entries[0].state,
                entries,
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
fn context_project(snapshot: &Snapshot, context: &Context) -> Option<(PathBuf, String)> {
    let lexical = paths::lexical(context.project.as_deref()?);
    let alias = paths::key(&lexical);
    Some(
        match snapshot
            .projects
            .iter()
            .find(|project| project.aliases.contains(&alias))
        {
            Some(project) => (project.path.clone(), project.key.clone()),
            None => (lexical, alias),
        },
    )
}
