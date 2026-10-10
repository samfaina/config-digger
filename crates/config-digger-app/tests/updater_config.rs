//! How the updater is set up: the facts the Tauri config and the `main`
//! capability carry, which nothing else checks before a release.

use serde_json::Value;
use std::path::Path;

fn json(file: &str) -> Value {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(file);
    serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap()
}

#[test]
fn the_webview_gets_no_updater_permission() {
    let capability = json("capabilities/main.json");

    let permissions: Vec<&str> = capability["permissions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|permission| permission.as_str().unwrap())
        .collect();
    assert!(
        permissions
            .iter()
            .all(|permission| !permission.starts_with("updater:")),
        "{permissions:?}"
    );
}

#[test]
fn the_updater_reads_only_the_latest_release_and_installs_in_passive_mode() {
    let updater = &json("tauri.conf.json")["plugins"]["updater"];

    assert_eq!(
        updater["endpoints"],
        serde_json::json!([
            "https://github.com/samfaina/config-digger/releases/latest/download/latest.json"
        ])
    );
    assert_eq!(updater["windows"]["installMode"], "passive");
}

#[test]
fn the_public_key_is_inlined_as_a_minisign_public_key() {
    let updater = &json("tauri.conf.json")["plugins"]["updater"];

    let pubkey = updater["pubkey"].as_str().expect("an inlined public key");
    let decoded = decode_base64(pubkey);
    assert!(
        decoded.starts_with("untrusted comment: minisign public key: "),
        "{decoded}"
    );
}

#[test]
fn a_release_build_signs_the_installer_for_the_updater() {
    let bundle = &json("tauri.bundle.conf.json")["bundle"];

    assert_eq!(bundle["createUpdaterArtifacts"], true);
}

/// The text `encoded` holds in standard base64.
fn decode_base64(encoded: &str) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut bits = 0u32;
    let mut count = 0;
    let mut bytes = Vec::new();
    for byte in encoded.bytes().filter(|byte| *byte != b'=') {
        let value = ALPHABET
            .iter()
            .position(|letter| *letter == byte)
            .expect("standard base64");
        bits = (bits << 6) | u32::try_from(value).unwrap();
        count += 6;
        if count >= 8 {
            count -= 8;
            bytes.push(u8::try_from((bits >> count) & 0xff).unwrap());
        }
    }
    String::from_utf8(bytes).expect("UTF-8 text")
}
