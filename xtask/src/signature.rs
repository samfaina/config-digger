//! The updater's signature of the installer, checked as the app's updater
//! checks it before it installs an update.

use base64::Engine as _;
use minisign_verify::{PublicKey, Signature};

/// What stops the app's updater from installing `installer`, each naming
/// what failed: its signature `sig`, the contents of its `.sig`, or the
/// `windows-x86_64` one of the updater manifest `manifest` does not verify
/// it against the `pubkey` of the app's `tauri_conf`, or was made for
/// another version than the one `manifest` announces; or the key, a
/// signature or the manifest cannot be decoded.
pub fn problems(tauri_conf: &str, installer: &[u8], sig: &str, manifest: &str) -> Vec<String> {
    let (public_key, (version, announced)) = match (public_key(tauri_conf), announced(manifest)) {
        (Ok(public_key), Ok(announced)) => (public_key, announced),
        (public_key, announced) => {
            return [public_key.err(), announced.err()]
                .into_iter()
                .flatten()
                .collect();
        }
    };
    [
        // Trimmed, as `latest.json` gets it.
        ("the .sig", sig.trim()),
        ("the windows-x86_64 signature in latest.json", &announced),
    ]
    .into_iter()
    .filter_map(|(name, signature)| {
        let problem = verify(&public_key, installer, signature, &version).err()?;
        Some(format!("{name} {problem}"))
    })
    .collect()
}

/// The `plugins.updater.pubkey` of `tauri_conf`, the key the app's updater
/// verifies updates with.
fn public_key(tauri_conf: &str) -> Result<PublicKey, String> {
    let tauri_conf: serde_json::Value = serde_json::from_str(tauri_conf)
        .map_err(|error| format!("tauri.conf.json is no JSON: {error}"))?;
    let pubkey = tauri_conf["plugins"]["updater"]["pubkey"]
        .as_str()
        .ok_or("tauri.conf.json has no plugins.updater.pubkey")?;
    base64_text(pubkey)
        .and_then(|text| PublicKey::decode(&text).map_err(|error| error.to_string()))
        .map_err(|error| format!("the public key in tauri.conf.json cannot be decoded: {error}"))
}

/// The version the updater manifest `manifest` announces and the signature
/// of its `windows-x86_64` installer.
fn announced(manifest: &str) -> Result<(String, String), String> {
    let manifest: serde_json::Value = serde_json::from_str(manifest)
        .map_err(|error| format!("latest.json is no JSON: {error}"))?;
    let text = |value: &serde_json::Value, missing: &str| {
        value
            .as_str()
            .map(str::to_owned)
            .ok_or_else(|| format!("latest.json has no {missing}"))
    };
    Ok((
        text(&manifest["version"], "version")?,
        text(
            &manifest["platforms"]["windows-x86_64"]["signature"],
            "windows-x86_64 signature",
        )?,
    ))
}

/// `installer` against `signature`, as `tauri-plugin-updater` verifies an
/// update that announces `version`.
fn verify(
    public_key: &PublicKey,
    installer: &[u8],
    signature: &str,
    version: &str,
) -> Result<(), String> {
    let signature = base64_text(signature)
        .and_then(|text| Signature::decode(&text).map_err(|error| error.to_string()))
        .map_err(|error| format!("cannot be decoded: {error}"))?;
    public_key
        .verify(installer, &signature, true)
        .map_err(|error| {
            format!("does not verify the installer against the app's public key: {error}")
        })?;
    // The trusted comment is the Tauri CLI's tab-separated `key:value`
    // pairs, covered by the signature.
    let signed = signature
        .trusted_comment()
        .split('\t')
        .find_map(|field| field.strip_prefix("version:"));
    match signed {
        Some(signed) if !same_version(signed, version) => Err(format!(
            "was made for version {signed}, not {version}, the version in latest.json"
        )),
        _ => Ok(()),
    }
}

/// Whether the versions are the same, as the updater compares them: as
/// SemVer without a leading `v`, or as text when either is not SemVer.
fn same_version(signed: &str, announced: &str) -> bool {
    let semver = |version: &str| semver::Version::parse(version.trim_start_matches('v'));
    match (semver(signed), semver(announced)) {
        (Ok(signed), Ok(announced)) => signed == announced,
        _ => signed == announced,
    }
}

/// The text `encoded` holds in standard base64.
fn base64_text(encoded: &str) -> Result<String, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|error| error.to_string())?;
    String::from_utf8(bytes).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::release::updater_manifest;

    /// The installer stand-in, and the public key of the throwaway key pair
    /// its signatures are made with: `tauri signer generate`, then
    /// `tauri signer sign`, `--app-version 0.1.0` for the one that carries a
    /// version. The throwaway private key is not kept.
    const INSTALLER: &[u8] = include_bytes!("../tests/fixtures/updater/installer.txt");
    const PUBLIC_KEY: &str = include_str!("../tests/fixtures/updater/throwaway.pub");
    const SIGNATURE: &str = include_str!("../tests/fixtures/updater/installer.txt.sig");
    /// A signature whose trusted comment carries `version:0.1.0`.
    const VERSION_0_1_0: &str =
        include_str!("../tests/fixtures/updater/installer.txt.version-0.1.0.sig");
    /// A signature of the installer stand-in made with another throwaway key.
    const OTHER_KEY: &str = include_str!("../tests/fixtures/updater/installer.txt.other-key.sig");

    /// A `tauri.conf.json` whose updater has the throwaway public key.
    fn tauri_conf() -> String {
        serde_json::json!({ "plugins": { "updater": { "pubkey": PUBLIC_KEY } } }).to_string()
    }

    fn manifest(signature: &str) -> String {
        updater_manifest("0.1.0", signature).unwrap()
    }

    #[test]
    fn a_signature_by_the_apps_key_without_a_version_passes_as_the_sig_and_in_latest_json() {
        assert_eq!(
            problems(&tauri_conf(), INSTALLER, SIGNATURE, &manifest(SIGNATURE)),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_signature_made_with_another_key_fails_as_the_sig_and_in_latest_json() {
        assert_eq!(
            problems(&tauri_conf(), INSTALLER, OTHER_KEY, &manifest(SIGNATURE)),
            [
                "the .sig does not verify the installer against the app's public key: \
              The signature was created with a different key than the one provided"
            ]
        );
        assert_eq!(
            problems(&tauri_conf(), INSTALLER, SIGNATURE, &manifest(OTHER_KEY)),
            [
                "the windows-x86_64 signature in latest.json does not verify the installer \
              against the app's public key: \
              The signature was created with a different key than the one provided"
            ]
        );
    }

    #[test]
    fn an_installer_changed_after_signing_fails() {
        let mut changed = INSTALLER.to_vec();
        changed.push(b'!');

        assert_eq!(
            problems(&tauri_conf(), &changed, SIGNATURE, &manifest(SIGNATURE)),
            [
                "the .sig does not verify the installer against the app's public key: \
                 The signature verification failed",
                "the windows-x86_64 signature in latest.json does not verify the installer \
                 against the app's public key: The signature verification failed",
            ]
        );
    }

    #[test]
    fn a_signature_made_for_another_version_than_latest_json_announces_fails() {
        let manifest = updater_manifest("0.2.0", VERSION_0_1_0).unwrap();

        assert_eq!(
            problems(&tauri_conf(), INSTALLER, VERSION_0_1_0, &manifest),
            [
                "the .sig was made for version 0.1.0, not 0.2.0, the version in latest.json",
                "the windows-x86_64 signature in latest.json was made for version 0.1.0, \
                 not 0.2.0, the version in latest.json",
            ]
        );
    }

    #[test]
    fn a_signature_made_for_the_version_latest_json_announces_passes() {
        assert_eq!(
            problems(
                &tauri_conf(),
                INSTALLER,
                VERSION_0_1_0,
                &manifest(VERSION_0_1_0)
            ),
            Vec::<String>::new()
        );
    }

    #[test]
    fn a_public_key_that_is_missing_or_cannot_be_decoded_fails() {
        let without = serde_json::json!({ "plugins": { "updater": {} } }).to_string();
        let not_base64 =
            serde_json::json!({ "plugins": { "updater": { "pubkey": "not base64!" } } })
                .to_string();
        let not_a_key =
            serde_json::json!({ "plugins": { "updater": { "pubkey": SIGNATURE } } }).to_string();

        assert_eq!(
            problems(&without, INSTALLER, SIGNATURE, &manifest(SIGNATURE)),
            ["tauri.conf.json has no plugins.updater.pubkey"]
        );
        assert_eq!(
            problems(&not_base64, INSTALLER, SIGNATURE, &manifest(SIGNATURE)),
            ["the public key in tauri.conf.json cannot be decoded: Invalid symbol 32, offset 3."]
        );
        assert_eq!(
            problems(&not_a_key, INSTALLER, SIGNATURE, &manifest(SIGNATURE)),
            ["the public key in tauri.conf.json cannot be decoded: \
              Invalid encoding in minisign data"]
        );
    }

    #[test]
    fn a_signature_that_cannot_be_decoded_fails() {
        assert_eq!(
            problems(&tauri_conf(), INSTALLER, "", &manifest(PUBLIC_KEY)),
            [
                "the .sig cannot be decoded: Invalid encoding in minisign data",
                "the windows-x86_64 signature in latest.json cannot be decoded: \
                 Invalid encoding in minisign data",
            ]
        );
    }

    #[test]
    fn a_latest_json_that_is_no_json_or_lacks_the_version_or_the_signature_fails() {
        let without_signature = serde_json::json!({ "version": "0.1.0" }).to_string();
        let without_version =
            serde_json::json!({ "platforms": { "windows-x86_64": { "signature": SIGNATURE } } })
                .to_string();

        assert_eq!(
            problems(&tauri_conf(), INSTALLER, SIGNATURE, "{"),
            ["latest.json is no JSON: EOF while parsing an object at line 1 column 1"]
        );
        assert_eq!(
            problems(&tauri_conf(), INSTALLER, SIGNATURE, &without_signature),
            ["latest.json has no windows-x86_64 signature"]
        );
        assert_eq!(
            problems(&tauri_conf(), INSTALLER, SIGNATURE, &without_version),
            ["latest.json has no version"]
        );
    }
}
