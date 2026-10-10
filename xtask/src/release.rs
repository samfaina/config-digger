//! What a release tag publishes besides the builds: its version, whether it
//! is a prerelease, `SHA256SUMS` and the winget manifest.

use sha2::{Digest, Sha256};

/// What a `vX.Y.Z` tag releases.
#[derive(Debug, PartialEq, Eq)]
pub struct Release {
    /// The version without the `v`, such as `0.1.0-rc.1`.
    pub version: String,
    /// A `-rc.N` release candidate, published as a GitHub prerelease.
    pub prerelease: bool,
}

/// The release `tag` names, which must be `v` followed by the workspace's
/// `version`.
pub fn from_tag(tag: &str, version: &str) -> Result<Release, String> {
    let Some((tagged, prerelease)) = tag.strip_prefix('v').and_then(parse) else {
        return Err(format!(
            "the tag {tag} is not vX.Y.Z or vX.Y.Z-rc.N with numbers without leading zeros"
        ));
    };
    if tagged != version {
        return Err(format!(
            "the tag {tag} is not the workspace version {version}: tag v{version}"
        ));
    }
    Ok(Release {
        version: tagged.to_owned(),
        prerelease,
    })
}

/// `X.Y.Z` or `X.Y.Z-rc.N`, and whether it is the latter.
fn parse(version: &str) -> Option<(&str, bool)> {
    let (core, rc) = match version.split_once('-') {
        Some((core, pre)) => (core, Some(pre.strip_prefix("rc.")?)),
        None => (version, None),
    };
    let parts: Vec<&str> = core.split('.').collect();
    if parts.len() != 3 || !parts.iter().chain(rc.as_ref()).all(|part| number(part)) {
        return None;
    }
    Some((version, rc.is_some()))
}

/// A SemVer numeric identifier: digits, without a leading zero.
fn number(text: &str) -> bool {
    !text.is_empty()
        && text.bytes().all(|byte| byte.is_ascii_digit())
        && (text == "0" || !text.starts_with('0'))
}

/// `SHA256SUMS` for `files`, each a name and its contents, in the format
/// `sha256sum --check` reads, sorted by name.
pub fn sha256sums(files: &[(String, Vec<u8>)]) -> String {
    let mut lines: Vec<(&str, String)> = files
        .iter()
        .map(|(name, contents)| (name.as_str(), sha256(contents)))
        .collect();
    lines.sort();
    lines
        .iter()
        .map(|(name, hash)| format!("{hash}  {name}\n"))
        .collect()
}

/// The SHA-256 of `contents` in lowercase hex.
pub fn sha256(contents: &[u8]) -> String {
    Sha256::digest(contents)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The winget manifest of `version`, whose installer on GitHub Releases
/// has the SHA-256 `installer_sha256`: each file's name and contents.
pub fn winget_manifest(version: &str, installer_sha256: &str) -> Vec<(String, String)> {
    let header = |kind: &str| {
        format!(
            "# yaml-language-server: $schema=https://aka.ms/winget-manifest.{kind}.{WINGET_SCHEMA}.schema.json\n\
             PackageIdentifier: {WINGET_ID}\n\
             PackageVersion: {version}\n"
        )
    };
    let footer = |kind: &str| format!("ManifestType: {kind}\nManifestVersion: {WINGET_SCHEMA}\n");
    let version_file = format!(
        "{}DefaultLocale: en-US\n{}",
        header("version"),
        footer("version")
    );
    let installer = format!(
        "{}InstallerType: nullsoft\n\
         Scope: user\n\
         InstallModes:\n\
         - interactive\n\
         - silent\n\
         - silentWithProgress\n\
         UpgradeBehavior: install\n\
         Commands:\n\
         - config-digger\n\
         ProductCode: config-digger\n\
         Installers:\n\
         - Architecture: x64\n  \
           InstallerUrl: {REPOSITORY}/releases/download/v{version}/config-digger_{version}_x64-setup.exe\n  \
           InstallerSha256: {}\n{}",
        header("installer"),
        installer_sha256.to_uppercase(),
        footer("installer")
    );
    let locale = format!(
        "{}PackageLocale: en-US\n\
         Publisher: samfaina\n\
         PublisherUrl: https://github.com/samfaina\n\
         PackageName: config-digger\n\
         PackageUrl: {REPOSITORY}\n\
         License: MIT OR Apache-2.0\n\
         ShortDescription: Inspect the effective configuration of AI coding Agents.\n\
         Moniker: config-digger\n\
         Tags:\n\
         - mcp\n\
         - skills\n\
         - cli\n\
         ReleaseNotesUrl: {REPOSITORY}/releases/tag/v{version}\n{}",
        header("defaultLocale"),
        footer("defaultLocale")
    );
    vec![
        (format!("{WINGET_ID}.yaml"), version_file),
        (format!("{WINGET_ID}.installer.yaml"), installer),
        (format!("{WINGET_ID}.locale.en-US.yaml"), locale),
    ]
}

/// The name of the installer of `version`, as Tauri's NSIS bundler names it.
pub fn installer_name(version: &str) -> String {
    format!("config-digger_{version}_x64-setup.exe")
}

/// The name of the updater signature of the installer of `version`.
pub fn signature_name(version: &str) -> String {
    format!("{}.sig", installer_name(version))
}

/// The name of the updater manifest, which the app fetches from
/// `releases/latest/download/`, so prereleases never reach it.
pub const UPDATER_MANIFEST: &str = "latest.json";

/// The updater manifest `latest.json` of `version`, whose installer on
/// GitHub Releases has the minisign `signature`, the contents of its `.sig`.
/// It is refused when the signature is empty.
pub fn updater_manifest(version: &str, signature: &str) -> Result<String, String> {
    let signature = signature.trim();
    if signature.is_empty() {
        return Err(
            "the installer's updater signature is empty: is TAURI_SIGNING_PRIVATE_KEY set?"
                .to_owned(),
        );
    }
    let manifest = serde_json::json!({
        "version": version,
        "notes": format!("{REPOSITORY}/releases/tag/v{version}"),
        "platforms": {
            "windows-x86_64": {
                "signature": signature,
                "url": format!(
                    "{REPOSITORY}/releases/download/v{version}/{}",
                    installer_name(version)
                ),
            },
        },
    });
    let mut text = serde_json::to_string_pretty(&manifest).expect("the manifest serialises");
    text.push('\n');
    Ok(text)
}

/// The assets the release of `version` must have that `names` lacks, in the
/// order they are listed here: no release ships without the updater's.
pub fn missing_assets(version: &str, names: &[&str]) -> Vec<String> {
    [
        installer_name(version),
        signature_name(version),
        format!("config-digger_{version}_x64-cli.zip"),
        UPDATER_MANIFEST.to_owned(),
        "SHA256SUMS".to_owned(),
    ]
    .into_iter()
    .filter(|asset| !names.contains(&asset.as_str()))
    .collect()
}

const WINGET_ID: &str = "samfaina.config-digger";
const WINGET_SCHEMA: &str = "1.10.0";
const REPOSITORY: &str = "https://github.com/samfaina/config-digger";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tag_of_the_workspace_version_is_a_release() {
        assert_eq!(
            from_tag("v0.1.0", "0.1.0"),
            Ok(Release {
                version: "0.1.0".to_owned(),
                prerelease: false
            })
        );
    }

    #[test]
    fn an_rc_tag_is_a_prerelease() {
        assert_eq!(
            from_tag("v0.2.0-rc.3", "0.2.0-rc.3"),
            Ok(Release {
                version: "0.2.0-rc.3".to_owned(),
                prerelease: true
            })
        );
    }

    #[test]
    fn a_tag_of_another_version_is_refused() {
        assert_eq!(
            from_tag("v0.1.1", "0.1.0"),
            Err("the tag v0.1.1 is not the workspace version 0.1.0: tag v0.1.0".to_owned())
        );
    }

    #[test]
    fn a_tag_that_is_not_vx_y_z_or_an_rc_is_refused() {
        for tag in [
            "0.1.0",
            "v0.1",
            "v0.1.0-beta.1",
            "v0.1.0-rc",
            "v0.1.0-rc.x",
            "v01.1.0",
        ] {
            let version = tag.trim_start_matches('v');
            assert!(from_tag(tag, version).is_err(), "{tag}");
        }
    }

    #[test]
    fn the_updater_manifest_points_at_the_signed_installer_of_the_release() {
        let manifest = updater_manifest("0.1.0", "dW50cnVzdGVkIGNvbW1lbnQ=\n").unwrap();

        assert_eq!(
            manifest,
            r#"{
  "version": "0.1.0",
  "notes": "https://github.com/samfaina/config-digger/releases/tag/v0.1.0",
  "platforms": {
    "windows-x86_64": {
      "signature": "dW50cnVzdGVkIGNvbW1lbnQ=",
      "url": "https://github.com/samfaina/config-digger/releases/download/v0.1.0/config-digger_0.1.0_x64-setup.exe"
    }
  }
}
"#
        );
    }

    #[test]
    fn an_updater_manifest_without_a_signature_is_refused() {
        assert_eq!(
            updater_manifest("0.1.0", " \n"),
            Err(
                "the installer's updater signature is empty: is TAURI_SIGNING_PRIVATE_KEY set?"
                    .to_owned()
            )
        );
    }

    #[test]
    fn a_release_has_every_asset_the_installers_and_the_updater_need() {
        let names = [
            "config-digger_0.1.0_x64-setup.exe",
            "config-digger_0.1.0_x64-setup.exe.sig",
            "config-digger_0.1.0_x64-cli.zip",
            "latest.json",
            "SHA256SUMS",
        ];

        assert_eq!(missing_assets("0.1.0", &names), Vec::<String>::new());
    }

    #[test]
    fn a_release_without_the_updater_signature_or_manifest_is_missing_them() {
        let names = [
            "config-digger_0.1.0_x64-setup.exe",
            "config-digger_0.1.0_x64-cli.zip",
            "SHA256SUMS",
            "config-digger_0.0.9_x64-setup.exe.sig",
        ];

        assert_eq!(
            missing_assets("0.1.0", &names),
            ["config-digger_0.1.0_x64-setup.exe.sig", "latest.json"]
        );
    }

    #[test]
    fn sha256sums_lists_each_file_by_name_with_its_hash() {
        let files = [
            (
                "config-digger_0.1.0_x64-setup.exe".to_owned(),
                b"abc".to_vec(),
            ),
            ("config-digger_0.1.0_x64-cli.zip".to_owned(), Vec::new()),
        ];

        assert_eq!(
            sha256sums(&files),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855  config-digger_0.1.0_x64-cli.zip\n\
             ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad  config-digger_0.1.0_x64-setup.exe\n"
        );
    }

    #[test]
    fn the_winget_installer_manifest_points_at_the_github_release_installer() {
        let manifest = winget_manifest(
            "0.1.0",
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        );

        let (_, installer) = manifest
            .iter()
            .find(|(name, _)| name == "samfaina.config-digger.installer.yaml")
            .expect("an installer manifest");
        assert_eq!(
            installer,
            "\
# yaml-language-server: $schema=https://aka.ms/winget-manifest.installer.1.10.0.schema.json
PackageIdentifier: samfaina.config-digger
PackageVersion: 0.1.0
InstallerType: nullsoft
Scope: user
InstallModes:
- interactive
- silent
- silentWithProgress
UpgradeBehavior: install
Commands:
- config-digger
ProductCode: config-digger
Installers:
- Architecture: x64
  InstallerUrl: https://github.com/samfaina/config-digger/releases/download/v0.1.0/config-digger_0.1.0_x64-setup.exe
  InstallerSha256: BA7816BF8F01CFEA414140DE5DAE2223B00361A396177A9CB410FF61F20015AD
ManifestType: installer
ManifestVersion: 1.10.0
"
        );
    }

    #[test]
    fn the_winget_manifest_has_a_version_and_a_default_locale_file() {
        let manifest = winget_manifest("0.2.0", "00");

        let names: Vec<&str> = manifest.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(
            names,
            [
                "samfaina.config-digger.yaml",
                "samfaina.config-digger.installer.yaml",
                "samfaina.config-digger.locale.en-US.yaml",
            ]
        );
        for (name, contents) in &manifest {
            assert!(
                contents.contains("PackageIdentifier: samfaina.config-digger\n"),
                "{name}"
            );
            assert!(contents.contains("PackageVersion: 0.2.0\n"), "{name}");
        }
        assert!(manifest[0].1.contains("DefaultLocale: en-US\n"));
        assert!(manifest[2].1.contains("PackageName: config-digger\n"));
        assert!(manifest[2].1.contains(
            "ReleaseNotesUrl: https://github.com/samfaina/config-digger/releases/tag/v0.2.0\n"
        ));
    }
}
