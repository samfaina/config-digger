//! Selectors: an Item name, or the `<path>#<key path>` locator of an Entry.

use crate::api::{Entry, Error, ErrorCode, Inventory, Item, Leftovers, Narrowing};
use crate::{locator, paths};
use std::collections::BTreeSet;
use std::path::Path;

/// Among the Items `narrowing` leaves, those with an Entry at the locator
/// `selector`, or else those named `selector`. Locator paths compare like
/// any other path; key paths exactly.
pub(crate) fn select<'i>(
    inventory: &'i Inventory,
    selector: &str,
    narrowing: &Narrowing,
) -> Vec<&'i Item> {
    let items = || inventory.items.iter().filter(|item| narrowing.admits(item));
    let at_locator = at_locator(selector);
    let by_locator: Vec<&Item> = items()
        .filter(|item| item.entries.iter().any(|entry| at_locator(&entry)))
        .collect();
    if !by_locator.is_empty() {
        return by_locator;
    }
    items().filter(|item| item.name == selector).collect()
}

/// The Entries an action targets, each once, in selector order. Without
/// `all`, each selector must name exactly one Entry: the one at a locator, or
/// the only Entry of the Items with a name. With `all`, it must name exactly
/// one Item, and stands for every Entry of it. Either way, only among the
/// Items `narrowing` leaves. A locator that names no Entry of the inventory
/// can name one of `leftovers`, the Unread Entries of missing Projects
/// outside the context.
pub(crate) fn select_entries<'i>(
    inventory: &'i Inventory,
    leftovers: &'i [Leftovers],
    selectors: &[String],
    all: bool,
    narrowing: &Narrowing,
) -> Result<Vec<&'i Entry>, Error> {
    let mut selected: Vec<&Entry> = Vec::new();
    for selector in selectors {
        let in_context = if all {
            match select(inventory, selector, narrowing).as_slice() {
                [item] => Ok(item.entries.iter().collect()),
                [] => Err(Error::no_match(selector)),
                items => Err(Error::ambiguous(selector, items, inventory)),
            }
        } else {
            one_entry(inventory, selector, narrowing)
        };
        let entries = match in_context {
            Err(error) if error.code == ErrorCode::NoMatch => {
                leftover_entries(leftovers, selector, all, narrowing).ok_or(error)?
            }
            entries => entries?,
        };
        for entry in entries {
            if !selected.iter().any(|known| known.locator == entry.locator) {
                selected.push(entry);
            }
        }
    }
    Ok(selected)
}

/// The one Entry `selector` names among the Items `narrowing` leaves, in a
/// list. An Entry several Agents read belongs to an Item in each of them,
/// so it is counted once by locator.
fn one_entry<'i>(
    inventory: &'i Inventory,
    selector: &str,
    narrowing: &Narrowing,
) -> Result<Vec<&'i Entry>, Error> {
    let at_locator = at_locator(selector);
    let mut found: Vec<&Entry> = inventory
        .items
        .iter()
        .filter(|item| narrowing.admits(item))
        .flat_map(|item| &item.entries)
        .filter(&at_locator)
        .collect();
    if found.is_empty() {
        found = select(inventory, selector, narrowing)
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

/// The leftover at the locator `selector` among the Items `narrowing`
/// leaves, or with `all`, every leftover of its Item. `None` when no
/// leftover is at it.
fn leftover_entries<'i>(
    leftovers: &'i [Leftovers],
    selector: &str,
    all: bool,
    narrowing: &Narrowing,
) -> Option<Vec<&'i Entry>> {
    let at_locator = at_locator(selector);
    let item = leftovers
        .iter()
        .flat_map(|leftovers| &leftovers.items)
        .filter(|item| narrowing.admits(item))
        .find(|item| item.entries.iter().any(|entry| at_locator(&entry)))?;
    Some(if all {
        item.entries.iter().collect()
    } else {
        item.entries.iter().filter(&at_locator).collect()
    })
}

/// Whether `selector` can be read as the locator of what is at `key_path`
/// in the file at `path`, as [`at_locator`] compares them.
pub(crate) fn is_locator_of(selector: &str, path: &Path, key_path: &[String]) -> bool {
    let path = paths::lexical_key(path);
    locator::parse(selector)
        .into_iter()
        .any(|(want, want_key_path)| paths::lexical_key(&want) == path && want_key_path == key_path)
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
    use crate::api::{
        DisableNotOffered, DisableOption, Entry, ItemKind, Provenance, RemoveOption, State,
    };
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
            built: None,
            entries: vec![Entry {
                locator: locator::render(&path, &key_path),
                path,
                key_path,
                scope: "user".to_owned(),
                provenance: Provenance::File,
                skill_manager: None,
                plugin: None,
                git_versioned: false,
                link: None,
                parked: None,
                state: State::Effective,
                reason: None,
                secondary: Vec::new(),
                what_if: None,
                notes: Vec::new(),
                config: serde_json::json!({}),
                masked: Vec::new(),
                disable: DisableOption::NotOffered {
                    code: DisableNotOffered::NoMechanism,
                    sentence: String::new(),
                },
                enable: None,
                remove: RemoveOption::Offered,
                caveats: Vec::new(),
                also_read_by: Vec::new(),
                copied_from: None,
            }],
            caveats: Vec::new(),
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
            store_read_failures: Vec::new(),
            agent_flags: Vec::new(),
        }
    }

    fn homes(items: Vec<&Item>) -> Vec<&str> {
        items.iter().map(|item| item.home.as_str()).collect()
    }

    #[test]
    fn a_name_in_two_homes_selects_both() {
        let inventory = inventory(vec![item("a", "linear"), item("b", "linear")]);

        assert_eq!(
            homes(inventory.select("linear", &Narrowing::default())),
            ["a", "b"]
        );
    }

    #[test]
    fn select_one_of_an_ambiguous_name_lists_a_locator_per_item() {
        let inventory = inventory(vec![item("a", "linear"), item("b", "linear")]);

        let error = inventory
            .select_one("linear", &Narrowing::default())
            .unwrap_err();

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
    fn select_one_of_an_entry_two_agents_read_names_each_items_agent_and_home() {
        let mut other = item("a", "linear");
        other.agent = "cursor".to_owned();
        let inventory = inventory(vec![item("a", "linear"), other, item("b", "linear")]);

        let error = inventory
            .select_one("linear", &Narrowing::default())
            .unwrap_err();

        assert_eq!(
            error.candidates,
            [
                r"C:\a\.claude.json#mcpServers.linear --agent claude-code --home a",
                r"C:\a\.claude.json#mcpServers.linear --agent cursor --home a",
                r"C:\b\.claude.json#mcpServers.linear",
            ]
        );
        assert_eq!(
            error.message,
            "`linear` matches 3 Items; select one by locator, with --agent and --home where Items share one"
        );
    }

    #[test]
    fn a_locator_selects_the_one_home() {
        let inventory = inventory(vec![item("a", "linear"), item("b", "linear")]);

        assert_eq!(
            homes(inventory.select(
                r"c:/B/.claude.json#mcpServers.linear",
                &Narrowing::default()
            )),
            ["b"]
        );
    }
}
