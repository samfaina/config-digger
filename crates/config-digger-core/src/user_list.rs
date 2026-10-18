//! The Agent homes and Projects the user added or hid, kept in the roaming
//! data folder, `%APPDATA%\config-digger`. It is written by atomic replace
//! without the global lock, since it holds no Agent configuration.

use crate::api::Error;
use crate::definitions::AgentDefinition;
use crate::ports::{Environment, FileSystem, Ports};
use crate::{explain, paths, scan};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::io;
use std::path::{Path, PathBuf};

/// The version of the file's format.
const VERSION: u32 = 1;

/// The name of the file in the roaming data folder.
const FILE: &str = "homes-and-projects.json";

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UserList {
    #[serde(default = "version")]
    pub version: u32,
    /// Agent homes the user added, each with the folder's real path.
    #[serde(default)]
    pub homes: Vec<AddedHome>,
    /// Agent homes the user hid, discovered or added.
    #[serde(default)]
    pub hidden_homes: Vec<HiddenHome>,
    /// Projects the user added: folders or `.code-workspace` files, by real path.
    #[serde(default)]
    pub projects: Vec<PathBuf>,
    /// Projects the user hid, discovered or added, by real path.
    #[serde(default)]
    pub hidden_projects: Vec<PathBuf>,
}

fn version() -> u32 {
    VERSION
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct AddedHome {
    /// The id of the Agent definition it is read with.
    pub agent: String,
    pub folder: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct HiddenHome {
    pub agent: String,
    /// The home's id, as the inventory lists it.
    pub home: String,
}

/// config-digger's roaming data folder: `%APPDATA%\config-digger`, or
/// `~/.config/config-digger` where there is no `APPDATA`.
pub(crate) fn data_folder(env: &dyn Environment) -> Result<PathBuf, Error> {
    let base = match env.var("APPDATA").filter(|value| !value.is_empty()) {
        Some(roaming) => PathBuf::from(roaming),
        None => env
            .home_dir()
            .ok_or_else(|| Error::unexpected("cannot determine config-digger's settings folder"))?
            .join(".config"),
    };
    Ok(base.join("config-digger"))
}

/// The file the list is kept in.
pub(crate) fn file(env: &dyn Environment) -> Result<PathBuf, Error> {
    Ok(data_folder(env)?.join(FILE))
}

/// The list as it is on disk: empty when the file is missing.
pub(crate) fn load(ports: &Ports) -> Result<UserList, Error> {
    let file = file(ports.env.as_ref())?;
    let Some(list) = read_json::<UserList>(ports, &file, "a list of Agent homes and Projects")?
    else {
        return Ok(UserList::default());
    };
    check_version(&file, list.version, VERSION)?;
    Ok(list)
}

/// Replaces the file with `list`, through a temporary file and a rename.
pub(crate) fn save(ports: &Ports, list: &UserList) -> Result<(), Error> {
    let file = file(ports.env.as_ref())?;
    let list = UserList {
        version: VERSION,
        ..list.clone()
    };
    write_json(ports, &file, &list)
}

/// What the JSON file `file` in the roaming data folder holds, `None` when
/// it is missing. `what` says what it should hold, when it does not.
pub(crate) fn read_json<T: DeserializeOwned>(
    ports: &Ports,
    file: &Path,
    what: &str,
) -> Result<Option<T>, Error> {
    let bytes = match ports.fs.read(file) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(error) => {
            return Err(Error::unexpected(format!(
                "cannot read {}: {error}",
                file.display()
            )));
        }
    };
    serde_json::from_slice(&bytes).map(Some).map_err(|error| {
        Error::unexpected(format!(
            "{} is not {what} config-digger can read ({error}); fix or delete it",
            file.display()
        ))
    })
}

/// Refuses a file of a format version this config-digger does not know.
pub(crate) fn check_version(file: &Path, found: u32, known: u32) -> Result<(), Error> {
    if found == known {
        return Ok(());
    }
    Err(Error::unexpected(format!(
        "{} has version {found}, which this config-digger cannot read",
        file.display()
    )))
}

/// Replaces the JSON file `file` in the roaming data folder with `value`,
/// through a temporary file and a rename.
pub(crate) fn write_json<T: Serialize>(ports: &Ports, file: &Path, value: &T) -> Result<(), Error> {
    let cannot =
        |error: io::Error| Error::unexpected(format!("cannot write {}: {error}", file.display()));
    if let Some(folder) = file.parent() {
        ports.fs.create_dir_all(folder).map_err(cannot)?;
    }
    let mut json = serde_json::to_vec_pretty(value).expect("the file serialises");
    json.push(b'\n');
    ports.fs.write_atomic(file, &json).map_err(cannot)
}

impl UserList {
    /// Records each change `change` makes to the list on disk.
    fn update(ports: &Ports, change: impl FnOnce(&mut UserList)) -> Result<(), Error> {
        let mut list = load(ports)?;
        change(&mut list);
        save(ports, &list)
    }
}

/// The real path of `path`, which must be an existing folder, or a
/// `.code-workspace` file when `workspace` is allowed.
fn existing(fs: &dyn FileSystem, path: &Path, workspace: bool) -> Result<PathBuf, Error> {
    let lexical = paths::lexical(path);
    let (real, exists) = paths::canonical(fs, &lexical);
    let is_folder = exists && fs.read_dir(&real).is_ok();
    let is_workspace = workspace && exists && !is_folder && scan::is_workspace(&real);
    if paths::is_absolute(&lexical) && (is_folder || is_workspace) {
        Ok(real)
    } else {
        Err(Error::invalid_path(explain::not_existing(path, workspace)))
    }
}

/// The definition of the Agent `agent`, in any case, and the real path of
/// `folder`, which must be an existing folder, to add as its home.
pub(crate) fn home_to_add<'a>(
    definitions: &'a [AgentDefinition],
    ports: &Ports,
    folder: &Path,
    agent: &str,
) -> Result<(&'a AgentDefinition, PathBuf), Error> {
    let definition = definitions
        .iter()
        .find(|definition| definition.agent.id.eq_ignore_ascii_case(agent))
        .ok_or_else(|| Error::no_agent(agent))?;
    Ok((definition, existing(ports.fs.as_ref(), folder, false)?))
}

/// Adds the folder `folder` as an Agent home of the Agent `agent`, once.
/// Returns the Agent's id and the folder's real path.
pub(crate) fn add_home(
    definitions: &[AgentDefinition],
    ports: &Ports,
    folder: &Path,
    agent: &str,
) -> Result<(String, PathBuf), Error> {
    let (definition, folder) = home_to_add(definitions, ports, folder, agent)?;
    let agent = definition.agent.id.clone();
    let added = AddedHome {
        agent: agent.clone(),
        folder: folder.clone(),
    };
    UserList::update(ports, |list| {
        let key = paths::key(&added.folder);
        if !list
            .homes
            .iter()
            .any(|home| home.agent == added.agent && paths::key(&home.folder) == key)
        {
            list.homes.push(added);
        }
    })?;
    Ok((agent, folder))
}

/// The real path of `path`, which must be an existing folder or
/// `.code-workspace` file, to add as a Project.
pub(crate) fn project_to_add(ports: &Ports, path: &Path) -> Result<PathBuf, Error> {
    existing(ports.fs.as_ref(), path, true)
}

/// Adds the existing folder or `.code-workspace` file `path` as a Project,
/// once. Returns its real path.
pub(crate) fn add_project(ports: &Ports, path: &Path) -> Result<PathBuf, Error> {
    let path = project_to_add(ports, path)?;
    UserList::update(ports, |list| {
        let key = paths::key(&path);
        if !list.projects.iter().any(|added| paths::key(added) == key) {
            list.projects.push(path.clone());
        }
    })?;
    Ok(path)
}

/// Hides the Agent home `home` of the Agent `agent`, once.
pub(crate) fn hide_home(ports: &Ports, agent: &str, home: &str) -> Result<(), Error> {
    let hidden = HiddenHome {
        agent: agent.to_owned(),
        home: home.to_owned(),
    };
    UserList::update(ports, |list| {
        if !list.hidden_homes.contains(&hidden) {
            list.hidden_homes.push(hidden);
        }
    })
}

/// Shows the Agent home `home` of the Agent `agent` again.
pub(crate) fn unhide_home(ports: &Ports, agent: &str, home: &str) -> Result<(), Error> {
    UserList::update(ports, |list| {
        list.hidden_homes
            .retain(|hidden| !(hidden.agent == agent && hidden.home == home));
    })
}

/// Hides the Project at the real path `path`, once.
pub(crate) fn hide_project(ports: &Ports, path: &Path) -> Result<(), Error> {
    UserList::update(ports, |list| {
        let key = paths::key(path);
        if !list
            .hidden_projects
            .iter()
            .any(|hidden| paths::key(hidden) == key)
        {
            list.hidden_projects.push(path.to_path_buf());
        }
    })
}

/// Shows the Project at the real path `path` again.
pub(crate) fn unhide_project(ports: &Ports, path: &Path) -> Result<(), Error> {
    UserList::update(ports, |list| {
        let key = paths::key(path);
        list.hidden_projects
            .retain(|hidden| paths::key(hidden) != key);
    })
}

/// Forgets the Agent home the user added at the real path `folder`, and
/// that it was hidden as `home`.
pub(crate) fn forget_home(
    ports: &Ports,
    agent: &str,
    home: &str,
    folder: &Path,
) -> Result<(), Error> {
    UserList::update(ports, |list| {
        let key = paths::key(folder);
        list.homes
            .retain(|added| !(added.agent == agent && paths::key(&added.folder) == key));
        list.hidden_homes
            .retain(|hidden| !(hidden.agent == agent && hidden.home == home));
    })
}

/// Forgets the Project the user added at the real path `path`, and that it was hidden.
pub(crate) fn forget_project(ports: &Ports, path: &Path) -> Result<(), Error> {
    UserList::update(ports, |list| {
        let key = paths::key(path);
        list.projects.retain(|added| paths::key(added) != key);
        list.hidden_projects
            .retain(|hidden| paths::key(hidden) != key);
    })
}
