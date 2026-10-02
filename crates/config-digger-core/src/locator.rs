//! `<path>#<key path>` locators, the text form that addresses one Entry.

use std::path::Path;

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
