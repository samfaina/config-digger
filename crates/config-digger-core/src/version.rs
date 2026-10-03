//! Agent versions as config-digger compares them: numeric, component by
//! component, padded with zeros, so that `2.16120` is `2.16120.0.0`. A
//! pre-release or build suffix (`-insider`, `+build`) counts as the numeric
//! version it follows.

use std::cmp::Ordering;

/// A version's numeric components, as read.
#[derive(Debug, Clone)]
pub(crate) struct Version(Vec<u64>);

impl Version {
    /// The numeric version `text` starts with, after any `v`, such as
    /// `0.159.3` in `0.159.3-win32-x64`; `None` when it does not start with a
    /// number.
    pub fn parse(text: &str) -> Option<Self> {
        let text = text.trim();
        let numeric = text
            .strip_prefix(['v', 'V'])
            .unwrap_or(text)
            .split(|c: char| !(c.is_ascii_digit() || c == '.'))
            .next()
            .unwrap_or("");
        let mut components = Vec::new();
        for part in numeric.split('.') {
            if part.is_empty() {
                break;
            }
            components.push(part.parse().ok()?);
        }
        (!components.is_empty()).then_some(Self(components))
    }

    fn component(&self, index: usize) -> u64 {
        self.0.get(index).copied().unwrap_or(0)
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        let length = self.0.len().max(other.0.len());
        (0..length)
            .map(|index| self.component(index).cmp(&other.component(index)))
            .find(|ordering| ordering.is_ne())
            .unwrap_or(Ordering::Equal)
    }
}

impl PartialEq for Version {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other).is_eq()
    }
}

impl Eq for Version {}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

/// Where a version stands against a tested range.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Standing {
    Older,
    Tested,
    Newer,
}

/// Where `version` stands against the range from `min` to `max`, both included.
pub(crate) fn standing(version: &Version, min: &Version, max: &Version) -> Standing {
    if version < min {
        Standing::Older
    } else if version > max {
        Standing::Newer
    } else {
        Standing::Tested
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn version(text: &str) -> Version {
        Version::parse(text).unwrap()
    }

    #[test]
    fn missing_components_count_as_zero() {
        assert_eq!(version("2.16120"), version("2.16120.0.0"));
        assert!(version("2.1") < version("2.1.0.1"));
    }

    #[test]
    fn components_compare_as_numbers() {
        assert!(version("3.9.0") < version("3.22.12"));
        assert!(version("0.159.3") > version("0.62.0"));
    }

    #[test]
    fn a_suffix_counts_as_its_numeric_version() {
        assert_eq!(version("1.130.0-insider"), version("1.130.0"));
        assert_eq!(version("0.159.3-win32-x64"), version("0.159.3"));
        assert_eq!(version("2.1.287+build.5"), version("2.1.287"));
    }

    #[test]
    fn a_leading_v_is_left_out() {
        assert_eq!(version("v2.1.286"), version("2.1.286"));
        assert_eq!(Version::parse("vscode"), None);
    }

    #[test]
    fn text_without_a_leading_number_is_no_version() {
        assert_eq!(Version::parse("insider"), None);
        assert_eq!(Version::parse(""), None);
        assert_eq!(Version::parse(".1"), None);
    }

    #[test]
    fn a_range_includes_both_ends() {
        let (min, max) = (version("2.1.280"), version("2.1.286"));
        assert_eq!(standing(&version("2.1.280"), &min, &max), Standing::Tested);
        assert_eq!(
            standing(&version("2.1.286.0"), &min, &max),
            Standing::Tested
        );
        assert_eq!(standing(&version("2.1.279"), &min, &max), Standing::Older);
        assert_eq!(
            standing(&version("2.1.287-beta"), &min, &max),
            Standing::Newer
        );
    }
}
