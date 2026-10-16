//! The installer's install folder, checked against config-digger's data
//! folders in `hooks.nsh` and against Tauri's default in `tauri.conf.json`.

use config_digger_core::api::DataFolders;
use config_digger_core::ports::{Environment, Hive, RegistryData};
use std::path::PathBuf;

/// Where the check expands `$LOCALAPPDATA` and `$APPDATA`.
const LOCAL_APP_DATA: &str = r"C:\Users\fixture\AppData\Local";
const APP_DATA: &str = r"C:\Users\fixture\AppData\Roaming";

/// The `hooks.nsh` defines naming the install folder and the data folders.
const INSTALL_FOLDER: &str = "INSTALL_FOLDER";
const DATA_FOLDERS: [&str; 2] = ["DATA_LOCAL", "DATA_ROAMING"];

/// What is wrong with the folders `hooks` defines, with Tauri's
/// `tauri_conf`:
///
/// - the data folders must hold every folder the core keeps data in;
/// - the install folder must be no data folder, nor a folder inside or
///   above one;
/// - Tauri's default install folder, `$LOCALAPPDATA\<productName>`, must be
///   one the installer moves to the install folder: a data folder, or a
///   folder inside or above one.
pub fn problems(hooks: &str, tauri_conf: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let install = defined_folder(hooks, INSTALL_FOLDER, &mut problems);
    let data: Vec<(&str, String)> = DATA_FOLDERS
        .into_iter()
        .filter_map(|name| Some((name, defined_folder(hooks, name, &mut problems)?)))
        .collect();
    let Some(install) = install.filter(|_| problems.is_empty()) else {
        return problems;
    };
    let DataFolders {
        backups,
        parked,
        logs,
        lists,
    } = DataFolders::locate(&Fixture).expect("the fixture has both app data folders");
    for kept in [backups, parked, logs, lists] {
        let kept = kept.display().to_string();
        if !data.iter().any(|(_, data)| inside(&kept, data)) {
            problems.push(format!(
                "the core keeps data in {kept}, outside {}",
                DATA_FOLDERS.join(" and ")
            ));
        }
    }
    let nested_in_data = |folder: &str| {
        data.iter()
            .find(|(_, data)| inside(folder, data) || inside(data, folder))
    };
    if let Some((name, data)) = nested_in_data(&install) {
        problems.push(format!(
            "the install folder {install} ({INSTALL_FOLDER}) is the data folder \
             {data} ({name}), or a folder inside or above it"
        ));
    }
    match product_name(tauri_conf) {
        Some(product) => {
            let tauri_default = format!(r"{LOCAL_APP_DATA}\{product}");
            if nested_in_data(&tauri_default).is_none() {
                problems.push(format!(
                    "Tauri's default install folder {tauri_default} (productName) is no data \
                     folder, so the installer would keep it instead of {INSTALL_FOLDER}"
                ));
            }
        }
        None => problems.push("tauri.conf.json has no productName".to_owned()),
    }
    problems
}

/// The folder `!define name "…"` names, with `$LOCALAPPDATA` and `$APPDATA`
/// expanded, or `None` after adding the problem that it is missing.
fn defined_folder(hooks: &str, name: &str, problems: &mut Vec<String>) -> Option<String> {
    let value = hooks.lines().find_map(|line| {
        let rest = line.trim().strip_prefix("!define")?.trim_start();
        let value = rest.strip_prefix(name)?.trim();
        value.strip_prefix('"')?.strip_suffix('"')
    });
    let Some(value) = value else {
        problems.push(format!("hooks.nsh defines no {name}"));
        return None;
    };
    Some(
        value
            .replace("$LOCALAPPDATA", LOCAL_APP_DATA)
            .replace("$APPDATA", APP_DATA),
    )
}

/// The `productName` of a Tauri config.
fn product_name(tauri_conf: &str) -> Option<String> {
    let config: serde_json::Value = serde_json::from_str(tauri_conf).ok()?;
    Some(config.get("productName")?.as_str()?.to_owned())
}

/// Whether `folder` is `outer` or inside it, ignoring case, the kind of
/// slash and trailing slashes.
fn inside(folder: &str, outer: &str) -> bool {
    let spelled = |folder: &str| {
        let folder = folder.replace('/', "\\").to_lowercase();
        format!("{}\\", folder.trim_end_matches('\\'))
    };
    spelled(folder).starts_with(&spelled(outer))
}

/// The environment the core's data folders are located in.
struct Fixture;

impl Environment for Fixture {
    fn var(&self, name: &str) -> Option<String> {
        match name {
            "LOCALAPPDATA" => Some(LOCAL_APP_DATA.to_owned()),
            "APPDATA" => Some(APP_DATA.to_owned()),
            _ => None,
        }
    }

    fn home_dir(&self) -> Option<PathBuf> {
        Some(PathBuf::from(r"C:\Users\fixture"))
    }

    fn registry_value(&self, _: Hive, _: &str, _: &str) -> Option<String> {
        None
    }

    fn registry_values(&self, _: Hive, _: &str) -> Option<Vec<(String, RegistryData)>> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_committed_installer_installs_in_a_folder_of_its_own() {
        let hooks = include_str!("../../crates/config-digger-app/installer/hooks.nsh");
        let config = include_str!("../../crates/config-digger-app/tauri.conf.json");

        assert_eq!(problems(hooks, config), Vec::<String>::new());
    }

    /// The problems with `hooks`, for the product name `config-digger`.
    fn check(hooks: &str) -> Vec<String> {
        problems(hooks, r#"{ "productName": "config-digger" }"#)
    }

    /// `hooks.nsh` text installing in `install`.
    fn installing_in(install: &str) -> String {
        format!(
            "!define DATA_LOCAL \"$LOCALAPPDATA\\config-digger\"\n\
             !define DATA_ROAMING \"$APPDATA\\config-digger\"\n\
             !define INSTALL_FOLDER \"{install}\"\n"
        )
    }

    #[test]
    fn installing_in_a_data_folder_is_a_problem() {
        assert_eq!(
            check(&installing_in(r"$LOCALAPPDATA\config-digger")),
            vec![
                r"the install folder C:\Users\fixture\AppData\Local\config-digger (INSTALL_FOLDER) is the data folder C:\Users\fixture\AppData\Local\config-digger (DATA_LOCAL), or a folder inside or above it"
                    .to_owned()
            ]
        );
    }

    #[test]
    fn installing_inside_or_above_a_data_folder_is_a_problem() {
        for install in [
            r"$APPDATA\config-digger\bin",
            r"$APPDATA",
            r"c:\users\FIXTURE\appdata\roaming\",
            "C:/Users/fixture/AppData/Roaming/config-digger/app",
        ] {
            let problems = check(&installing_in(install));

            assert_eq!(problems.len(), 1, "{install}: {problems:?}");
            assert!(
                problems[0].contains("(DATA_ROAMING)"),
                "{install}: {problems:?}"
            );
        }
    }

    #[test]
    fn a_folder_beside_a_data_folder_is_its_own() {
        for install in [
            r"$LOCALAPPDATA\config-digger-app",
            r"$LOCALAPPDATA\Programs\config-digger",
            r"D:\config-digger",
        ] {
            assert_eq!(
                check(&installing_in(install)),
                Vec::<String>::new(),
                "{install}"
            );
        }
    }

    #[test]
    fn a_core_data_folder_outside_the_defined_data_folders_is_a_problem() {
        let hooks = installing_in(r"D:\config-digger").replace(
            r"$APPDATA\config-digger",
            r"$APPDATA\config-digger-settings",
        );

        assert_eq!(
            check(&hooks),
            vec![
                r"the core keeps data in C:\Users\fixture\AppData\Roaming\config-digger, outside DATA_LOCAL and DATA_ROAMING"
                    .to_owned()
            ]
        );
    }

    #[test]
    fn a_missing_define_is_a_problem() {
        assert_eq!(
            check("!define DATA_LOCAL \"$LOCALAPPDATA\\config-digger\"\n"),
            vec![
                "hooks.nsh defines no INSTALL_FOLDER".to_owned(),
                "hooks.nsh defines no DATA_ROAMING".to_owned()
            ]
        );
    }

    #[test]
    fn a_product_name_whose_tauri_default_is_no_data_folder_is_a_problem() {
        let hooks = installing_in(r"$LOCALAPPDATA\Programs\config-digger");

        assert_eq!(
            problems(&hooks, r#"{ "productName": "Config Digger" }"#),
            vec![
                r"Tauri's default install folder C:\Users\fixture\AppData\Local\Config Digger (productName) is no data folder, so the installer would keep it instead of INSTALL_FOLDER"
                    .to_owned()
            ]
        );
    }

    #[test]
    fn a_config_without_a_product_name_is_a_problem() {
        let hooks = installing_in(r"$LOCALAPPDATA\Programs\config-digger");

        assert_eq!(
            problems(&hooks, "{}"),
            vec!["tauri.conf.json has no productName".to_owned()]
        );
    }
}
