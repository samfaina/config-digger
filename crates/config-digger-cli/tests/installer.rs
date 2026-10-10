//! Black-box tests of the hidden `installer` commands the NSIS installer and
//! uninstaller run: the real `config-digger` binary with the real ports,
//! against a temporary home selected through environment overrides.

use serde_json::json;
use std::path::PathBuf;
use std::process::Command;
use tempfile::TempDir;

/// A temporary user home and data folders the binary is pointed at.
struct Harness {
    root: TempDir,
}

struct Run {
    code: i32,
    stdout: String,
    stderr: String,
}

impl Harness {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        std::fs::create_dir(root.path().join("home")).unwrap();
        Self { root }
    }

    /// Where the binary's `%APPDATA%` points.
    fn roaming_app_data(&self) -> PathBuf {
        self.root.path().join("AppData").join("Roaming")
    }

    /// Where the binary's `%LOCALAPPDATA%` points.
    fn local_app_data(&self) -> PathBuf {
        self.root.path().join("AppData").join("Local")
    }

    /// config-digger's parked-Entry store, holding an Entry of `agent` for
    /// each name, as Disable leaves it for an Agent with no switch of its own.
    fn with_parked(self, agent: &str, names: &[&str]) -> Self {
        let config = self
            .roaming_app_data()
            .join(r"Claude\claude_desktop_config.json");
        let entries: serde_json::Map<String, serde_json::Value> = names
            .iter()
            .map(|name| {
                let parked = json!({
                    "agent": agent,
                    "home": "classic",
                    "location": "user",
                    "name": name,
                    "path": config,
                    "key_path": ["mcpServers", name],
                    "parked_at": "2026-10-01T12:00:00Z",
                    "entry": { "command": "x" },
                });
                ((*name).to_owned(), parked)
            })
            .collect();
        let store = self
            .local_app_data()
            .join(r"config-digger\parked\manifest.json");
        std::fs::create_dir_all(store.parent().unwrap()).unwrap();
        std::fs::write(store, json!({ "entries": entries }).to_string()).unwrap();
        self
    }

    fn run(&self, args: &[&str]) -> Run {
        let home = self.root.path().join("home");
        let output = Command::new(env!("CARGO_BIN_EXE_config-digger"))
            .args(args)
            .current_dir(self.root.path())
            .env("USERPROFILE", &home)
            .env("HOME", &home)
            .env("LOCALAPPDATA", self.local_app_data())
            .env("APPDATA", self.roaming_app_data())
            .env("ProgramData", self.root.path().join("ProgramData"))
            .env_remove("CODEX_HOME")
            .env_remove("CLAUDE_CONFIG_DIR")
            .env_remove("CLAUDE_USER_DATA_DIR")
            .env_remove("RUST_LOG")
            .output()
            .unwrap();
        Run {
            code: output.status.code().unwrap(),
            stdout: String::from_utf8(output.stdout).unwrap(),
            stderr: String::from_utf8(output.stderr).unwrap(),
        }
    }
}

#[test]
fn installer_parked_names_each_parked_entry_and_its_agent() {
    let harness = Harness::new().with_parked("claude-desktop", &["notes"]);

    let run = harness.run(&["installer", "parked"]);

    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(run.stdout, "notes (Claude Desktop)\n");
}

#[test]
fn installer_parked_prints_nothing_without_a_parked_entry_store() {
    let harness = Harness::new();

    let run = harness.run(&["installer", "parked"]);

    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(run.stdout, "");
}

#[test]
fn installer_parked_names_the_first_eight_by_name_and_counts_the_rest() {
    let names: Vec<String> = (1..=12).map(|n| format!("server-{n:02}")).collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let harness = Harness::new().with_parked("claude-desktop", &names);

    let run = harness.run(&["installer", "parked"]);

    assert_eq!(run.code, 0, "{}", run.stderr);
    let lines: Vec<&str> = run.stdout.lines().collect();
    assert_eq!(lines.len(), 9, "{}", run.stdout);
    assert_eq!(lines[0], "server-01 (Claude Desktop)");
    assert_eq!(lines[7], "server-08 (Claude Desktop)");
    assert_eq!(lines[8], "and 4 more");
}

/// A throwaway key under `HKCU\Software` standing in for the user's
/// `Environment` key, deleted when dropped.
#[cfg(windows)]
struct EnvironmentKey {
    path: String,
}

#[cfg(windows)]
impl EnvironmentKey {
    const PARENT: &str = r"Software\config-digger-tests";

    fn new(test: &str) -> Self {
        let path = format!(r"{}\{test}-{}", Self::PARENT, std::process::id());
        let key = Self { path };
        key.delete();
        key.open();
        key
    }

    fn open(&self) -> winreg::RegKey {
        winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
            .create_subkey(&self.path)
            .unwrap()
            .0
    }

    fn delete(&self) {
        let _ =
            winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER).delete_subkey_all(&self.path);
    }

    /// Sets `Path` to `text`, as a value of type `vtype`.
    fn with_path(self, text: &str, vtype: winreg::enums::RegType) -> Self {
        let bytes: Vec<u8> = text
            .encode_utf16()
            .chain([0])
            .flat_map(u16::to_le_bytes)
            .collect();
        let value = winreg::RegValue {
            bytes: bytes.into(),
            vtype,
        };
        self.open().set_raw_value("Path", &value).unwrap();
        self
    }

    /// `Path` as it is stored, unexpanded, with its type, if there is one.
    fn path(&self) -> Option<(String, winreg::enums::RegType)> {
        let value = self.open().get_raw_value("Path").ok()?;
        let units: Vec<u16> = value
            .bytes
            .as_chunks::<2>()
            .0
            .iter()
            .map(|pair| u16::from_le_bytes(*pair))
            .take_while(|unit| *unit != 0)
            .collect();
        Some((String::from_utf16(&units).unwrap(), value.vtype))
    }
}

#[cfg(windows)]
impl Drop for EnvironmentKey {
    fn drop(&mut self) {
        self.delete();
        // Only once every test's key is gone.
        let _ =
            winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER).delete_subkey(Self::PARENT);
    }
}

#[cfg(windows)]
impl Harness {
    fn run_on_path(&self, key: &EnvironmentKey, args: &[&str]) -> Run {
        let mut args = args.to_vec();
        let flag = format!("--environment-key={}", key.path);
        args.push(&flag);
        self.run(&args)
    }
}

#[cfg(windows)]
const INSTALL: &str = r"C:\Users\fixture\AppData\Local\config-digger";

#[cfg(windows)]
#[test]
fn add_to_path_appends_the_folder_and_keeps_the_value_unexpanded() {
    use winreg::enums::RegType::REG_EXPAND_SZ;
    let key =
        EnvironmentKey::new("appends").with_path(r"%USERPROFILE%\bin;C:\tools", REG_EXPAND_SZ);

    let run = Harness::new().run_on_path(&key, &["installer", "add-to-path", INSTALL]);

    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(
        key.path(),
        Some((
            format!(r"%USERPROFILE%\bin;C:\tools;{INSTALL}"),
            REG_EXPAND_SZ
        ))
    );
}

#[cfg(windows)]
#[test]
fn add_to_path_leaves_a_path_that_has_the_folder_in_another_spelling() {
    use winreg::enums::RegType::REG_EXPAND_SZ;
    let spelled = r"C:\tools;c:/users/FIXTURE/AppData/Local/config-digger\";
    let key = EnvironmentKey::new("already").with_path(spelled, REG_EXPAND_SZ);

    let run = Harness::new().run_on_path(&key, &["installer", "add-to-path", INSTALL]);

    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(key.path(), Some((spelled.to_owned(), REG_EXPAND_SZ)));
}

#[cfg(windows)]
#[test]
fn add_to_path_creates_a_missing_path() {
    use winreg::enums::RegType::REG_EXPAND_SZ;
    let key = EnvironmentKey::new("missing");

    let run = Harness::new().run_on_path(&key, &["installer", "add-to-path", INSTALL]);

    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(key.path(), Some((INSTALL.to_owned(), REG_EXPAND_SZ)));
}

#[cfg(windows)]
#[test]
fn remove_from_path_takes_out_every_spelling_and_keeps_the_rest_and_the_type() {
    use winreg::enums::RegType::REG_SZ;
    let path = format!(r"{INSTALL};C:\tools;C:\USERS\fixture\AppData\Local\config-digger\;D:\bin");
    let key = EnvironmentKey::new("removes").with_path(&path, REG_SZ);

    let run = Harness::new().run_on_path(&key, &["installer", "remove-from-path", INSTALL]);

    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(key.path(), Some((r"C:\tools;D:\bin".to_owned(), REG_SZ)));
}

#[cfg(windows)]
#[test]
fn remove_from_path_deletes_a_path_left_empty() {
    use winreg::enums::RegType::REG_EXPAND_SZ;
    let key = EnvironmentKey::new("empties").with_path(INSTALL, REG_EXPAND_SZ);

    let run = Harness::new().run_on_path(&key, &["installer", "remove-from-path", INSTALL]);

    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(key.path(), None);
}

#[test]
fn installer_parked_shortens_long_lines_to_fit_the_uninstaller_message() {
    let long = "a-server-name-far-longer-than-anyone-would-normally-give-a-server";
    let harness = Harness::new().with_parked("claude-desktop", &[long]);

    let run = harness.run(&["installer", "parked"]);

    assert_eq!(run.code, 0, "{}", run.stderr);
    assert_eq!(
        run.stdout,
        "a-server-name-far-longer-than-anyone-would-normally-give-...\n"
    );
}
