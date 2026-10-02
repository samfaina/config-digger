//! `resolve(snapshot, context)`: a pure function from a snapshot and a context
//! to the Items of every Agent home and the State of each Entry.

use crate::api::{self, Context};
use crate::definitions::AgentDefinition;
use crate::definitions::blocks::Compete;
use crate::locator;
use crate::scan::{ScannedEntry, Snapshot};
use std::collections::BTreeMap;

pub(crate) fn resolve(
    definitions: &[AgentDefinition],
    snapshot: &Snapshot,
    _context: &Context,
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
            }
        })
        .collect();

    // Entries compete when they share an Agent home, an Item kind and a
    // competition key. The map's order sorts Items by Agent, home and key.
    let mut competing: BTreeMap<_, Vec<&ScannedEntry>> = BTreeMap::new();
    for entry in &snapshot.entries {
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
            let entries: Vec<api::Entry> = entries
                .iter()
                .enumerate()
                .map(|(rank, entry)| api::Entry {
                    path: entry.path.clone(),
                    key_path: entry.key_path.clone(),
                    locator: locator::render(&entry.path, &entry.key_path),
                    scope: definition.location(&entry.location).scope.clone(),
                    state: if rank == 0 {
                        api::State::Effective
                    } else {
                        api::State::Shadowed
                    },
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
        homes,
        items,
        read_failures,
    }
}
