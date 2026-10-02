//! `<path>#<key path>` locators, the text form that addresses one Entry.

use std::path::{Path, PathBuf};

/// Renders a locator. Key path segments are joined with `.`; a segment that is
/// empty or holds `.`, `"`, `#` or whitespace is quoted, with `\` and `"` escaped.
pub(crate) fn render(path: &Path, key_path: &[String]) -> String {
    let keys: Vec<String> = key_path.iter().map(|key| segment(key)).collect();
    format!("{}#{}", path.display(), keys.join("."))
}

fn segment(key: &str) -> String {
    let plain = !key.is_empty()
        && !key
            .chars()
            .any(|c| matches!(c, '.' | '"' | '#') || c.is_whitespace());
    if plain {
        return key.to_owned();
    }
    let escaped = key.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}

/// Every way to read `text` as a locator: each `#` whose remainder is a valid
/// key path splits it into a path and that key path. A path may hold `#`, and a
/// quoted segment may too, so more than one split can be valid.
pub(crate) fn parse(text: &str) -> Vec<(PathBuf, Vec<String>)> {
    text.match_indices('#')
        .filter_map(|(at, _)| {
            let path = &text[..at];
            let key_path = parse_key_path(&text[at + 1..])?;
            (!path.is_empty()).then(|| (PathBuf::from(path), key_path))
        })
        .collect()
}

/// The inverse of [`render`]'s key path.
fn parse_key_path(text: &str) -> Option<Vec<String>> {
    let mut chars = text.chars().peekable();
    let mut segments = Vec::new();
    loop {
        let mut segment = String::new();
        if chars.next_if_eq(&'"').is_some() {
            loop {
                match chars.next()? {
                    '\\' => segment.push(chars.next()?),
                    '"' => break,
                    c => segment.push(c),
                }
            }
        } else {
            while let Some(c) = chars.next_if(|&c| c != '.') {
                if matches!(c, '"' | '#') || c.is_whitespace() {
                    return None;
                }
                segment.push(c);
            }
            if segment.is_empty() {
                return None;
            }
        }
        segments.push(segment);
        match chars.next() {
            None => return Some(segments),
            Some('.') => {}
            Some(_) => return None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn keys(keys: &[&str]) -> Vec<String> {
        keys.iter().map(|key| (*key).to_owned()).collect()
    }

    #[test]
    fn parse_reads_what_render_writes() {
        for key_path in [
            keys(&["mcpServers", "linear"]),
            keys(&["projects", "E:/x", "mcpServers", "a.b"]),
            keys(&["", "say \"hi\"", r"back\slash", "x#y"]),
        ] {
            let locator = render(Path::new(r"C:\a#b\.claude.json"), &key_path);

            assert_eq!(
                parse(&locator),
                [(PathBuf::from(r"C:\a#b\.claude.json"), key_path.clone())],
                "{locator}"
            );
        }
    }

    #[test]
    fn parse_rejects_text_that_is_not_a_locator() {
        for text in [
            "linear",
            "#mcpServers.x",
            r"C:\x.json#",
            r"C:\x.json#a..b",
            r#"C:\x.json#"a"b"#,
        ] {
            assert_eq!(parse(text), [], "{text}");
        }
    }
}
