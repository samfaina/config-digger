//! Read-only rules at work: which settings keep an Entry out by a rule
//! rather than one by one, such as a deny list. A rule in managed policy
//! Blocks the Entry; one in a file the user writes Disables it. config-digger
//! never writes a rule, so Enable is not offered for what it keeps out.

use crate::api::{Reason, State};
use crate::definitions::blocks::{
    AllowDenyList, Anchored, Gate, GlobalSwitch, ListKind, MatchedBy, Rule, ServerIdentity,
};
use crate::definitions::{AgentDefinition, LocationDefinition};
use crate::scan::{ScannedEntry, ScannedSetting, SettingOwner, Snapshot};
use crate::{disable, explain, locator, mask};
use serde_json::{Value, json};
use std::path::{Path, PathBuf};

/// A rule that keeps an Entry out in a context.
pub(crate) struct RuleHit {
    /// Blocked by policy, or Disabled.
    pub state: State,
    pub reason: Reason,
    /// The setting holding the rule, as a sentence names it.
    pub holds: String,
}

/// The rules of its Item kind that keep `entry` out in the context of the
/// Project with the key `project`, in the definition's order.
pub(crate) fn hits(
    definition: &AgentDefinition,
    snapshot: &Snapshot,
    entry: &ScannedEntry,
    project: Option<&str>,
) -> Vec<RuleHit> {
    let location = definition.location(&entry.location);
    let agent_name = &definition.agent.name;
    let mut hits = Vec::new();
    for (index, rule) in definition.rules(location.kind).rules.iter().enumerate() {
        if !rule.covers(&location.id) {
            continue;
        }
        // The values of each setting of the rule, in its order: its
        // sources, then any setting that changes how it reads them.
        let values: Vec<(bool, &ScannedSetting)> = rule
            .settings()
            .enumerate()
            .flat_map(|(source, setting)| {
                let owner = SettingOwner::Rule {
                    rule: index,
                    source,
                };
                snapshot
                    .settings_for(entry, location.kind, owner, project)
                    .map(move |value| (setting.policy, value))
            })
            .collect();
        let sources = rule.sources().len();
        let (values, changes) = values.split_at(
            values
                .iter()
                .position(|(_, value)| source_of(value) >= sources)
                .unwrap_or(values.len()),
        );
        match rule {
            Rule::AllowDenyList(block) => match block.list {
                ListKind::Deny => {
                    hits.extend(deny(block, location, entry, values, agent_name));
                }
                ListKind::Allow => {
                    // Only the policy lists count while a policy-only
                    // setting holds `true`.
                    let policy_only = changes
                        .iter()
                        .any(|(_, value)| value.value == Some(Value::Bool(true)));
                    let values: Vec<(bool, &ScannedSetting)> = values
                        .iter()
                        .copied()
                        .filter(|(policy, _)| *policy || !policy_only)
                        .collect();
                    hits.extend(allow(block, location, entry, &values, agent_name));
                }
            },
            Rule::GlobalSwitch(block) => hits.extend(global(block, values, agent_name)),
        }
    }
    hits
}

/// A hit of the rule in `value`, managed policy when `policy`.
fn hit(policy: bool, value: &ScannedSetting, sentence: impl FnOnce(bool) -> Reason) -> RuleHit {
    let mut reason = sentence(policy);
    reason.switch = Some(locator::render(&value.path, &value.key_path));
    RuleHit {
        state: if policy {
            State::Blocked
        } else {
            State::Disabled
        },
        reason,
        holds: explain::setting(key(value), &value.path),
    }
}

/// The index of the setting `value` is read for among its rule's settings.
fn source_of(value: &ScannedSetting) -> usize {
    match value.owner {
        SettingOwner::Rule { source, .. } | SettingOwner::Gate { source, .. } => source,
    }
}

/// The last key of the setting `value`, which sentences name it by.
fn key(value: &ScannedSetting) -> &str {
    value.key_path.last().map_or("", String::as_str)
}

/// Each deny list that names the server, by what it names it.
fn deny(
    block: &AllowDenyList,
    location: &LocationDefinition,
    entry: &ScannedEntry,
    values: &[(bool, &ScannedSetting)],
    agent_name: &str,
) -> Vec<RuleHit> {
    let server = identity(entry);
    values
        .iter()
        .filter_map(|&(policy, value)| {
            let items = value.value.as_ref()?.as_array()?;
            let by = items
                .iter()
                .find_map(|item| matches(block, location, item, &server))?;
            Some(hit(policy, value, |policy| {
                explain::denied(policy, key(value), &value.path, by, agent_name)
            }))
        })
        .collect()
}

/// The allow lists, merged, when any source holds one and nothing in them
/// allows the server. It is policy when any of them is.
fn allow(
    block: &AllowDenyList,
    location: &LocationDefinition,
    entry: &ScannedEntry,
    values: &[(bool, &ScannedSetting)],
    agent_name: &str,
) -> Option<RuleHit> {
    let lists: Vec<(bool, &ScannedSetting, &Vec<Value>)> = values
        .iter()
        .filter_map(|&(policy, value)| Some((policy, value, value.value.as_ref()?.as_array()?)))
        .collect();
    let items = || lists.iter().flat_map(|(_, _, items)| items.iter());
    let server = identity(entry);
    let (by_command, by_url) = items().fold((false, false), |(command, url), item| {
        let (has_command, has_url) = block.kinds(item);
        (command || has_command, url || has_url)
    });
    // A strict list that names servers by command allows one with a
    // command only by its command, and likewise by URL.
    let required = if block.strict && by_command && server.command.is_some() {
        Some(MatchedBy::Command)
    } else if block.strict && by_url && server.url.is_some() {
        Some(MatchedBy::Url)
    } else {
        None
    };
    let allowed = items().any(|item| match required {
        Some(required) => by_command_or_url(block, location, item, &server) == Some(required),
        None => matches(block, location, item, &server).is_some(),
    });
    if lists.is_empty() || allowed {
        return None;
    }
    let policy = lists.iter().any(|(policy, _, _)| *policy);
    let (_, first, _) = lists
        .iter()
        .find(|(known, _, _)| *known == policy)
        .expect("one list has the policy of the rule");
    let paths: Vec<&Path> = lists
        .iter()
        .filter(|(known, _, _)| *known == policy)
        .map(|(_, value, _)| value.path.as_path())
        .collect();
    Some(hit(policy, first, |policy| {
        explain::not_allowed(policy, key(first), &paths, agent_name)
    }))
}

/// What `item` matches the server by, if anything, comparing commands and
/// URLs as the Entry shows them.
fn matches(
    block: &AllowDenyList,
    location: &LocationDefinition,
    item: &Value,
    server: &ServerIdentity,
) -> Option<MatchedBy> {
    block.matches(
        item,
        server,
        &|command| masked_command(location, command),
        &|url| masked_url(location, url),
    )
}

/// What `item` matches the server by, leaving its name aside.
fn by_command_or_url(
    block: &AllowDenyList,
    location: &LocationDefinition,
    item: &Value,
    server: &ServerIdentity,
) -> Option<MatchedBy> {
    let unnamed = ServerIdentity {
        name: "",
        command: server.command.clone(),
        url: server.url,
    };
    matches(block, location, item, &unnamed).filter(|by| *by != MatchedBy::Name)
}

/// The first source that holds the switch decides, or for a switch every
/// source must leave on, the first that holds the value that turns them
/// off: it keeps the Entries out when it holds that value.
fn global(
    block: &GlobalSwitch,
    values: &[(bool, &ScannedSetting)],
    agent_name: &str,
) -> Option<RuleHit> {
    let (policy, value) = values.iter().find(|(_, value)| match &value.value {
        Some(held) => !block.every_source || *held == block.off,
        None => false,
    })?;
    let held = value.value.as_ref()?;
    (*held == block.off).then(|| {
        hit(*policy, value, |policy| {
            explain::switched_off(policy, key(value), held, &value.path, agent_name)
        })
    })
}

/// The server `entry` declares, as a list item matches it.
fn identity(entry: &ScannedEntry) -> ServerIdentity<'_> {
    let command = entry.config.get("command").and_then(Value::as_str);
    let args: Vec<&str> = entry
        .config
        .get("args")
        .and_then(Value::as_array)
        .map(|args| args.iter().filter_map(Value::as_str).collect())
        .unwrap_or_default();
    ServerIdentity {
        name: &entry.name,
        command: command.map(|command| std::iter::once(command).chain(args).collect()),
        url: entry.config.get("url").and_then(Value::as_str),
    }
}

/// A command and its args as an Entry of `location` holding them shows
/// them, Secrets masked.
fn masked_command(location: &LocationDefinition, command: &[&str]) -> Vec<String> {
    let Some((first, args)) = command.split_first() else {
        return Vec::new();
    };
    let masked = mask::entry(location, &json!({ "command": first, "args": args })).value;
    let strings = |value: Option<&Value>| value.and_then(Value::as_str).map(str::to_owned);
    strings(masked.get("command"))
        .into_iter()
        .chain(
            masked
                .get("args")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .filter_map(|arg| strings(Some(arg))),
        )
        .collect()
}

/// A URL as an Entry of `location` holding it shows it, Secrets masked.
fn masked_url(location: &LocationDefinition, url: &str) -> String {
    let masked = mask::entry(location, &json!({ "url": url })).value;
    masked
        .get("url")
        .and_then(Value::as_str)
        .unwrap_or(url)
        .to_owned()
}

/// What Enable says of an Entry that `hits`, rules outside policy, keep
/// out: it is not offered.
pub(crate) fn enable_not_offered(hits: &[&RuleHit]) -> Option<String> {
    hits.first()
        .map(|hit| explain::rule_enable_not_offered(&hit.holds))
}

/// The files of the rules and gates that could keep `entry` out in the
/// context of the Project at `project`, or of none, that cannot be read.
/// The Entry is resolved as if they held nothing.
pub(crate) fn failed_files(
    definition: &AgentDefinition,
    snapshot: &Snapshot,
    entry: &ScannedEntry,
    project: Option<&Path>,
) -> Vec<PathBuf> {
    let location = definition.location(&entry.location);
    let roots = snapshot.roots(entry.agent_index, &entry.home);
    let item_rules = definition.rules(location.kind);
    let rule_sources = item_rules
        .rules
        .iter()
        .filter(|rule| rule.covers(&location.id))
        .flat_map(Rule::settings);
    let gate_sources = item_rules
        .gates
        .iter()
        .filter(|gate| gate.covers(&location.id))
        .flat_map(Gate::sources);
    let mut files: Vec<PathBuf> = Vec::new();
    for source in rule_sources.chain(gate_sources) {
        let anchor = &source.anchor;
        let paths: Vec<PathBuf> = match anchor.settings() {
            Some(id) => snapshot.settings_of(entry.agent_index, id).read().to_vec(),
            None if anchor.in_project() => project
                .and_then(|project| anchor.path(roots, Some(project)))
                .into_iter()
                .collect(),
            None => disable::switch_path(anchor, roots, None, entry)
                .into_iter()
                .collect(),
        };
        for path in paths {
            if snapshot.unreadable_settings.contains(&path) && !files.contains(&path) {
                files.push(path);
            }
        }
    }
    files
}
