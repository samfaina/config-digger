fn main() {
    // Every command the webview may call. Each one becomes an `allow-<command>`
    // permission that the `main` capability grants; `lib.rs` registers the same list.
    let manifest = tauri_build::AppManifest::new().commands(&[
        "read_inventory",
        "switch_project",
        "backups_of",
        "open_location",
        "reveal_location",
        "plan_action",
        "execute_plan",
        "cancel_plan",
        "read_history",
        "history_detail",
        "dismiss_operation",
        "read_places",
        "change_list",
        "pick_location",
        "read_settings",
        "save_settings",
        "open_data_folder",
        "read_about",
        "open_page",
        "update_status",
        "startup_update_check",
        "check_for_updates",
        "install_update",
        "open_update_notes",
    ]);

    record_commit();

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

/// Records the commit the app is built from, and its date, for About. Built
/// without git, such as from a source archive, neither is recorded.
fn record_commit() {
    if let Some(commit) = git(&["rev-parse", "--short=7", "HEAD"]) {
        println!("cargo:rustc-env=CONFIG_DIGGER_COMMIT={commit}");
    }
    if let Some(date) = git(&["log", "-1", "--format=%cs"]) {
        println!("cargo:rustc-env=CONFIG_DIGGER_COMMIT_DATE={date}");
    }

    // Built again when HEAD moves: to another branch, or along its own.
    let mut watched = vec![git(&["rev-parse", "--git-path", "HEAD"])];
    if let Some(branch) = git(&["symbolic-ref", "-q", "HEAD"]) {
        watched.push(git(&["rev-parse", "--git-path", &branch]));
        watched.push(git(&["rev-parse", "--git-path", "packed-refs"]));
    }
    let manifest = std::path::PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap());
    for path in watched.into_iter().flatten() {
        let path = manifest.join(path);
        // A path that does not exist would build it again every time.
        if path.exists() {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
}

/// What `git` prints for `args`, `None` when it fails or prints nothing.
// Cargo runs the build script in its own console.
#[allow(clippy::disallowed_methods)]
fn git(args: &[&str]) -> Option<String> {
    let output = std::process::Command::new("git").args(args).output().ok()?;
    let text = String::from_utf8(output.stdout).ok()?;
    let text = text.trim();
    (output.status.success() && !text.is_empty()).then(|| text.to_owned())
}
