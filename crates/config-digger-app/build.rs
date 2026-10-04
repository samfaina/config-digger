fn main() {
    // Every command the webview may call. Each one becomes an `allow-<command>`
    // permission that the `main` capability grants; `lib.rs` registers the same list.
    let manifest = tauri_build::AppManifest::new().commands(&[
        "read_inventory",
        "switch_project",
        "backups_of",
        "open_location",
        "reveal_location",
    ]);

    // Tauri embeds its Windows application manifest (Common Controls v6) in
    // `[[bin]]` targets only. Without it, any other executable linking Tauri,
    // such as a test harness or the `export_bindings` example, fails to load
    // with STATUS_ENTRYPOINT_NOT_FOUND. So the linker embeds it in every target.
    let windows = tauri_build::WindowsAttributes::new_without_app_manifest();
    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        let path = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
            .join("windows-app-manifest.xml");
        println!("cargo:rerun-if-changed={}", path.display());
        println!("cargo:rustc-link-arg=/MANIFEST:EMBED");
        println!("cargo:rustc-link-arg=/MANIFESTINPUT:{}", path.display());
    }

    tauri_build::try_build(
        tauri_build::Attributes::new()
            .app_manifest(manifest)
            .windows_attributes(windows),
    )
    .expect("the Tauri build script runs");
}
