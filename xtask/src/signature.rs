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
    let (public_key, announced) = match (public_key(tauri_conf), Announced::read(manifest)) {
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
        (
            "the windows-x86_64 signature in latest.json",
            &announced.signature,
        ),
    ]
    .into_iter()
    .filter_map(|(name, signature)| {
        let problem = verify(&public_key, installer, signature, &announced.version).err()?;
        Some(format!("{name} {problem}"))
    })
    .collect()
}

/// What the updater manifest announces: the version, as the updater reads
/// it, and the signature of the `windows-x86_64` installer.
struct Announced {
    version: semver::Version,
    signature: String,
}

impl Announced {
    fn read(manifest: &str) -> Result<Self, String> {
        let manifest: serde_json::Value = serde_json::from_str(manifest)
            .map_err(|error| format!("latest.json is no JSON: {error}"))?;
        let text = |value: &serde_json::Value, missing: &str| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("latest.json has no {missing}"))
        };
        let version = text(&manifest["version"], "version")?;
        Ok(Self {
            version: semver::Version::parse(version.trim_start_matches('v')).map_err(|error| {
                format!("latest.json announces the version {version}, which is no SemVer version: {error}")
            })?,
            signature: text(
                &manifest["platforms"]["windows-x86_64"]["signature"],
                "windows-x86_64 signature",
            )?,
        })
    }
}

/// The `plugins.updater.pubkey` of `tauri_conf`, the key the app's updater
/// verifies updates with.
fn public_key(tauri_conf: &str) -> Result<PublicKey, String> {
    let tauri_conf: serde_json::Value = serde_json::from_str(tauri_conf)
        .map_err(|error| format!("tauri.conf.json is no JSON: {error}"))?;
    let pubkey = tauri_conf["plugins"]["updater"]["pubkey"]
        .as_str()
        .ok_or("tauri.conf.json has no plugins.updater.pubkey")?;
    minisign(pubkey, PublicKey::decode)
        .map_err(|error| format!("the public key in tauri.conf.json cannot be decoded: {error}"))
}

/// `installer` against `signature`, as `tauri-plugin-updater` verifies an
/// update that announces `version`.
fn verify(
    public_key: &PublicKey,
    installer: &[u8],
    signature: &str,
    version: &semver::Version,
) -> Result<(), String> {
    let signature = minisign(signature, Signature::decode)
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

/// Whether the `signed` version is `announced`, as the updater compares
/// them: as SemVer without a leading `v`, or as text when it is not SemVer.
fn same_version(signed: &str, announced: &semver::Version) -> bool {
    match semver::Version::parse(signed.trim_start_matches('v')) {
        Ok(signed) => signed == *announced,
        Err(_) => signed == announced.to_string(),
    }
}

/// The minisign text `encoded` holds in standard base64, read with
/// `decode`, as the updater reads its public key and signatures.
fn minisign<T>(
    encoded: &str,
    decode: fn(&str) -> Result<T, minisign_verify::Error>,
) -> Result<T, String> {
    let bytes = base64::engine::general_purpose::STANDARD
        .decode(encoded)
        .map_err(|error| error.to_string())?;
    let text = String::from_utf8(bytes).map_err(|error| error.to_string())?;
    decode(&text).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::release::updater_manifest;

    /// The installer stand-in, and the public key of the throwaway key pair
    /// its signatures are made with: `tauri signer generate`, then
    /// `tauri signer sign`, `--app-version 0.1.0` for the one that carries a
    /// version. The throwaway private key is not kept.
    const INSTALLER: &[u8] = b"A stand-in for the installer, signed with a throwaway key pair.\n";
    const PUBLIC_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IG1pbmlzaWduIHB1YmxpYyBrZXk6IDRCN0VCMzE3NkFCRjdCMDMKUldRRGU3OXFGN04rU3pFenBPQU5uQ2hYVkZLSWlOTDVKRURqazNqcmNJTnVSV1JRUnR3VEdYNmcK";
    /// A signature whose trusted comment carries no version.
    const SIGNATURE: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVRRGU3OXFGN04rUzZOYktHQzlSMGhWZVI1ZHVVdm42ZWp3QThnemc4bDh0Vlg3RmdUWDNUdDZiNlF2a3ZJa0Q2MTZyZUhONTR1MGo3MkNlL0oxYi9QZWFZS1hqdkNxN1FrPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzkyNzY5NzYzCWZpbGU6aW5zdGFsbGVyLnR4dApaRWZZOWoyWmlTOWRGRnRxQjJNa1JVbzkyYXgvWWdMODA2REszMmg2V1MrS0NLYnJnRnd3MnVmcm5MMjJLZFdPR0kycWRxUEsrV0VSU3JJSUZtVEpBUT09Cg==";
    /// A signature whose trusted comment carries `version:0.1.0`.
    const VERSION_0_1_0: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVRRGU3OXFGN04rUzlvNWZsMEdTVTFVMDlCcnZMUnFHM1hSTFhpb2F1V1Q2RWFUTmh2OTBSY3RKMEgzZmh0N3l5Tmc3RllvQ25uREd3Ykh2eVdFYndKblhpSGlFamZVc2dRPQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzkyNzY5NzY0CWZpbGU6aW5zdGFsbGVyLXYudHh0CXZlcnNpb246MC4xLjAKSzA0VElQVHdHQVZ5T3ZLVW9abCtrSnVDNWY4SStueXBjNXF5elVnNmxaUXFHZFFsYzlCQlVCYVprYjFOVmRnOCtUZFZ5Wlg5L3VqK1VOOUxnK2xYQnc9PQo=";
    /// A signature of the installer stand-in made with another throwaway key.
    const OTHER_KEY: &str = "dW50cnVzdGVkIGNvbW1lbnQ6IHNpZ25hdHVyZSBmcm9tIHRhdXJpIHNlY3JldCBrZXkKUlVTZlhhelp3THc5SkQ5cE0yS1RXVGdPTGc2WFNuclhZR1FsNVkxT2Zwd08xTGlDbUJ2QjVuNTd6NVBTSnVWWXhDRGorQnJpUHZDNlZ5YlowajJvdzFYT2V4My9abFBUcVE4PQp0cnVzdGVkIGNvbW1lbnQ6IHRpbWVzdGFtcDoxNzkyNzY5NzY1CWZpbGU6aW5zdGFsbGVyLW90aGVyLnR4dAp4NEJRRHpFbjZpeG91TlZoOXNrenZXN3Rma0dLOHp1SkJaNzVNdXlEeDhJTjR0VkxnVm1LNVJnb1NMT1ozYnpTSmhJSzBIZjVBd3ovak9uVnNUMStCUT09Cg==";
    /// Base64 of minisign text that is no signature, and of one that is no
    /// public key.
    const NOT_A_SIGNATURE: &str = PUBLIC_KEY;
    const NOT_A_KEY: &str = SIGNATURE;

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
            serde_json::json!({ "plugins": { "updater": { "pubkey": NOT_A_KEY } } }).to_string();

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
            problems(&tauri_conf(), INSTALLER, "", &manifest(NOT_A_SIGNATURE)),
            [
                "the .sig cannot be decoded: Invalid encoding in minisign data",
                "the windows-x86_64 signature in latest.json cannot be decoded: \
                 Invalid encoding in minisign data",
            ]
        );
    }

    #[test]
    fn a_latest_json_that_is_no_json_or_lacks_a_semver_version_or_the_signature_fails() {
        let without_signature = serde_json::json!({ "version": "0.1.0" }).to_string();
        let not_semver = updater_manifest("0.1", SIGNATURE).unwrap();
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
        assert_eq!(
            problems(&tauri_conf(), INSTALLER, SIGNATURE, &not_semver),
            [
                "latest.json announces the version 0.1, which is no SemVer version: \
              unexpected end of input while parsing minor version number"
            ]
        );
    }
}
