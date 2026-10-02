//! Selectors: an Item name, or the `<path>#<key path>` locator of an Entry.

use crate::api::{Inventory, Item};
use crate::{locator, paths};

/// The Items with an Entry at the locator `selector`, or else the Items named
/// `selector`. Locator paths compare like any other path; key paths exactly.
pub(crate) fn select<'i>(inventory: &'i Inventory, selector: &str) -> Vec<&'i Item> {
    let locators: Vec<(String, Vec<String>)> = locator::parse(selector)
        .into_iter()
        .map(|(path, key_path)| (paths::lexical_key(&path), key_path))
        .collect();
    let by_locator: Vec<&Item> = inventory
        .items
        .iter()
        .filter(|item| {
            item.entries.iter().any(|entry| {
                let path = paths::lexical_key(&entry.path);
                locators
                    .iter()
                    .any(|(want, key_path)| *want == path && *key_path == entry.key_path)
            })
        })
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::{Entry, ItemKind, Provenance, State};
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
                state: State::Effective,
                reason: None,
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
