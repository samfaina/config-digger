//! What About shows of this build, and the pages it links to.

use serde::{Deserialize, Serialize};

/// This build of the app.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
pub struct About {
    pub version: String,
    /// The short id of the commit it was built from, `None` when it was
    /// built without git, such as from a source archive.
    pub commit: Option<String>,
    /// The date of that commit, as `YYYY-MM-DD`.
    pub date: Option<String>,
    /// Such as `Windows x64`.
    pub platform: String,
    /// The pages About links to, in the order it shows them.
    pub pages: Vec<PageLink>,
}

impl About {
    /// This build, as `build.rs` recorded it.
    pub fn this_build() -> Self {
        let version = env!("CARGO_PKG_VERSION");
        let pages = [Page::Repository, Page::Issues, Page::ReleaseNotes]
            .into_iter()
            .map(|page| PageLink {
                page,
                url: page.url(env!("CARGO_PKG_REPOSITORY"), version),
            })
            .collect();
        About {
            version: version.to_owned(),
            commit: option_env!("CONFIG_DIGGER_COMMIT").map(str::to_owned),
            date: option_env!("CONFIG_DIGGER_COMMIT_DATE").map(str::to_owned),
            platform: platform(std::env::consts::OS, std::env::consts::ARCH),
            pages,
        }
    }
}

/// A page About links to. The webview opens no other.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Page {
    Repository,
    Issues,
    /// The release notes of this version.
    ReleaseNotes,
}

/// A page and where it is.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
pub struct PageLink {
    pub page: Page,
    pub url: String,
}

impl Page {
    /// Where the page is, for a build of `version` from `repository`.
    pub fn url(self, repository: &str, version: &str) -> String {
        let repository = repository.trim_end_matches('/');
        match self {
            Page::Repository => repository.to_owned(),
            Page::Issues => format!("{repository}/issues"),
            Page::ReleaseNotes => format!("{repository}/releases/tag/v{version}"),
        }
    }
}

/// How a platform reads, such as `Windows x64` for `windows` on `x86_64`.
fn platform(os: &str, arch: &str) -> String {
    let os = match os {
        "windows" => "Windows",
        "macos" => "macOS",
        "linux" => "Linux",
        other => other,
    };
    let arch = match arch {
        "x86_64" => "x64",
        "aarch64" => "ARM64",
        other => other,
    };
    format!("{os} {arch}")
}

#[cfg(test)]
mod tests {
    use super::*;

    const REPOSITORY: &str = "https://github.com/samfaina/config-digger";

    #[test]
    fn the_platform_reads_as_windows_names_it() {
        assert_eq!(platform("windows", "x86_64"), "Windows x64");
        assert_eq!(platform("windows", "aarch64"), "Windows ARM64");
        assert_eq!(platform("freebsd", "x86"), "freebsd x86");
    }

    #[test]
    fn each_page_is_on_the_repository() {
        assert_eq!(Page::Repository.url(REPOSITORY, "0.1.0"), REPOSITORY);
        assert_eq!(
            Page::Issues.url(&format!("{REPOSITORY}/"), "0.1.0"),
            "https://github.com/samfaina/config-digger/issues"
        );
        assert_eq!(
            Page::ReleaseNotes.url(REPOSITORY, "0.1.0"),
            "https://github.com/samfaina/config-digger/releases/tag/v0.1.0"
        );
    }

    #[test]
    fn this_build_names_its_version_and_links_to_its_pages() {
        let about = About::this_build();

        assert_eq!(about.version, env!("CARGO_PKG_VERSION"));
        let pages: Vec<(Page, &str)> = about
            .pages
            .iter()
            .map(|link| (link.page, link.url.as_str()))
            .collect();
        let release_notes = format!(
            "https://github.com/samfaina/config-digger/releases/tag/v{}",
            env!("CARGO_PKG_VERSION")
        );
        assert_eq!(
            pages,
            [
                (Page::Repository, REPOSITORY),
                (
                    Page::Issues,
                    "https://github.com/samfaina/config-digger/issues"
                ),
                (Page::ReleaseNotes, release_notes.as_str()),
            ]
        );
    }
}
