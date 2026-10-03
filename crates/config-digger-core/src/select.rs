//! Selectors: an Item name, or the `<path>#<key path>` locator of an Entry.

use crate::api::{Entry, Error, Inventory, Item};
use crate::{locator, paths};
use std::collections::BTreeSet;

/// The Items with an Entry at the locator `selector`, or else the Items named
/// `selector`. Locator paths compare like any other path; key paths exactly.
pub(crate) fn select<'i>(inventory: &'i Inventory, selector: &str) -> Vec<&'i Item> {
    let at_locator = at_locator(selector);
    let by_locator: Vec<&Item> = inventory
        .items
        .iter()
        .filter(|item| item.entries.iter().any(|entry| at_locator(&entry)))
        .collect();
    if !by_locator.is_empty() {
        return by_locator;
    }
    inventory
        .items
        .iter()
        .filter(|item| item.name == selector)
        .collect()
}

/// The Entries an action targets, each once, in selector order. Without
/// `all`, each selector must name exactly one Entry: the one at a locator, or
/// the only Entry of the Items with a name. With `all`, it must name exactly
/// one Item, and stands for every Entry of it.
pub(crate) fn select_entries<'i>(
    inventory: &'i Inventory,
    selectors: &[String],
    all: bool,
) -> Result<Vec<&'i Entry>, Error> {
    let mut selected: Vec<&Entry> = Vec::new();
    for selector in selectors {
        let entries = if all {
            match select(inventory, selector).as_slice() {
                [item] => item.entries.iter().collect(),
                [] => return Err(Error::no_match(selector)),
                items => return Err(Error::ambiguous(selector, items)),
            }
        } else {
            one_entry(inventory, selector)?
        };
        for entry in entries {
            if !selected.iter().any(|known| known.locator == entry.locator) {
                selected.push(entry);
            }
        }
    }
    Ok(selected)
}

/// The one Entry `selector` names, in a list. An Entry several Agents read
/// belongs to an Item in each of them, so it is counted once by locator.
fn one_entry<'i>(inventory: &'i Inventory, selector: &str) -> Result<Vec<&'i Entry>, Error> {
    let at_locator = at_locator(selector);
    let mut found: Vec<&Entry> = inventory
        .items
        .iter()
        .flat_map(|item| &item.entries)
        .filter(&at_locator)
        .collect();
    if found.is_empty() {
        found = select(inventory, selector)
            .into_iter()
            .flat_map(|item| &item.entries)
            .collect();
    }
    let mut seen = BTreeSet::new();
    found.retain(|entry| seen.insert(entry.locator.clone()));
    match found.as_slice() {
        [_] => Ok(found),
        [] => Err(Error::no_match(selector)),
        entries => Err(Error::ambiguous_entries(selector, entries)),
    }
}

/// Whether an Entry is at one of the locators `selector` can be read as.
fn at_locator(selector: &str) -> impl Fn(&&Entry) -> bool {
    let locators: Vec<(String, Vec<String>)> = locator::parse(selector)
        .into_iter()
        .map(|(path, key_path)| (paths::lexical_key(&path), key_path))
        .collect();
    move |entry| {
        let path = paths::lexical_key(&entry.path);
        locators
            .iter()
            .any(|(want, key_path)| *want == path && *key_path == entry.key_path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{DisableNotOffered, DisableOption, Entry, ItemKind, Provenance, State};
    use std::path::PathBuf;

    fn item(home: &str, name: &str) -> Item {
        let path = PathBuf::from(format!(r"C:\{home}\.claude.json"));
        let key_path = vec!["mcpServers".to_owned(), name.to_owned()];
        Item {
            agent: "claude-code".to_owned(),
            home: home.to_owned(),
            kind: ItemKind::Mcp,
            name: name.to_owned(),
            state: State::Effective,
            entries: vec![Entry {
                locator: locator::render(&path, &key_path),
                path,
                key_path,
                scope: "user".to_owned(),
                provenance: Provenance::File,
                git_versioned: false,
                state: State::Effective,
                reason: None,
                config: serde_json::json!({}),
                masked: Vec::new(),
                disable: DisableOption::NotOffered {
                    code: DisableNotOffered::NoMechanism,
                    sentence: String::new(),
                },
            }],
        }
    }

    fn inventory(items: Vec<Item>) -> Inventory {
        Inventory {
            read_at: String::new(),
            project: None,
            projects: Vec::new(),
            homes: Vec::new(),
            items,
            read_failures: Vec::new(),
        }
    }

    fn homes(items: Vec<&Item>) -> Vec<&str> {
        items.iter().map(|item| item.home.as_str()).collect()
    }

    #[test]
    fn a_name_in_two_homes_selects_both() {
        let inventory = inventory(vec![item("a", "linear"), item("b", "linear")]);

        assert_eq!(homes(inventory.select("linear")), ["a", "b"]);
    }

    #[test]
    fn select_one_of_an_ambiguous_name_lists_a_locator_per_item() {
        let inventory = inventory(vec![item("a", "linear"), item("b", "linear")]);

        let error = inventory.select_one("linear").unwrap_err();

        assert_eq!(error.code, crate::api::ErrorCode::Ambiguous);
        assert_eq!(
            error.candidates,
            [
                r"C:\a\.claude.json#mcpServers.linear",
                r"C:\b\.claude.json#mcpServers.linear"
            ]
        );
    }

    #[test]
    fn a_locator_selects_the_one_home() {
        let inventory = inventory(vec![item("a", "linear"), item("b", "linear")]);

        assert_eq!(
            homes(inventory.select(r"c:/B/.claude.json#mcpServers.linear")),
            ["b"]
        );
    }
}
