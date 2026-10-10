//! The app's settings and the folders config-digger keeps its data in,
//! through the core `api` with fake ports.

use config_digger_core::api::{Core, DataFolders, Settings};
use config_digger_core::ports::fake::{self, FakeEnvironment, FakeFileSystem};
use std::path::PathBuf;

const HOME: &str = r"C:\Users\fixture";
const APPDATA: &str = r"C:\Users\fixture\AppData\Roaming";
const LOCALAPPDATA: &str = r"C:\Users\fixture\AppData\Local";

fn core(fs: &FakeFileSystem) -> Core {
    let env = FakeEnvironment::with_home(HOME)
        .with_var("APPDATA", APPDATA)
        .with_var("LOCALAPPDATA", LOCALAPPDATA);
    Core::new(fake::ports(fs.clone(), env)).unwrap()
}

fn settings_file() -> PathBuf {
    PathBuf::from(APPDATA).join(r"config-digger\settings.json")
}

#[test]
fn checking_for_updates_on_startup_is_off_by_default() {
    let fs = FakeFileSystem::new();

    let settings = core(&fs).settings().unwrap();

    assert_eq!(
        settings,
        Settings {
            check_updates_on_startup: false
        }
    );
    assert_eq!(fs.contents(settings_file()), None, "reading writes nothing");
}

#[test]
fn a_saved_setting_is_read_back() {
    let fs = FakeFileSystem::new();
    let on = Settings {
        check_updates_on_startup: true,
    };

    core(&fs).save_settings(&on).unwrap();

    assert_eq!(core(&fs).settings().unwrap(), on);
    let saved: serde_json::Value =
        serde_json::from_slice(&fs.contents(settings_file()).unwrap()).unwrap();
    assert_eq!(
        saved,
        serde_json::json!({ "version": 1, "check_updates_on_startup": true })
    );
}

#[test]
fn a_settings_file_that_cannot_be_read_is_an_error_that_names_it() {
    let fs = FakeFileSystem::new().with_file(settings_file(), "{ not json");

    let error = core(&fs).settings().unwrap_err();

    assert!(
        error
            .message
            .contains(&settings_file().display().to_string()),
        "{}",
        error.message
    );
}

#[test]
fn data_folders_are_in_the_local_and_roaming_app_data_folders() {
    let folders = core(&FakeFileSystem::new()).data_folders().unwrap();

    assert_eq!(
        folders,
        DataFolders {
            backups: PathBuf::from(r"C:\Users\fixture\AppData\Local\config-digger\backups"),
            parked: PathBuf::from(r"C:\Users\fixture\AppData\Local\config-digger\parked"),
            logs: PathBuf::from(r"C:\Users\fixture\AppData\Local\config-digger\logs"),
            lists: PathBuf::from(r"C:\Users\fixture\AppData\Roaming\config-digger"),
        }
    );
}
