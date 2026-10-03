//! The Agent homes and Projects the user added or hid, kept in the roaming
//! data folder, `%APPDATA%\config-digger`. It is written by atomic replace
//! without the global lock, since it holds no Agent configuration.

use crate::api::Error;
use crate::definitions::AgentDefinition;
use crate::ports::{Environment, FileSystem, Ports};
use crate::{explain, paths, scan};
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
fn data_folder(env: &dyn Environment) -> Result<PathBuf, Error> {
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
    let bytes = match ports.fs.read(&file) {
        Ok(bytes) => bytes,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(UserList::default()),
        Err(error) => {
            return Err(Error::unexpected(format!(
                "cannot read {}: {error}",
                file.display()
            )));
        }
    };
    let list: UserList = serde_json::from_slice(&bytes).map_err(|error| {
        Error::unexpected(format!(
            "{} is not a list of Agent homes and Projects config-digger can read ({error}); fix or delete it",
            file.display()
        ))
    })?;
    if list.version != VERSION {
        return Err(Error::unexpected(format!(
            "{} has version {}, which this config-digger cannot read",
            file.display(),
            list.version
        )));
    }
    Ok(list)
}

/// Replaces the file with `list`, through a temporary file and a rename.
pub(crate) fn save(ports: &Ports, list: &UserList) -> Result<(), Error> {
    let file = file(ports.env.as_ref())?;
    let cannot =
        |error: io::Error| Error::unexpected(format!("cannot write {}: {error}", file.display()));
    if let Some(folder) = file.parent() {
        ports.fs.create_dir_all(folder).map_err(cannot)?;
    }
    let mut json = serde_json::to_vec_pretty(&UserList {
        version: VERSION,
        ..list.clone()
    })
    .expect("the list serialises");
    json.push(b'\n');
    ports.fs.write_atomic(&file, &json).map_err(cannot)
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
    let is_workspace = workspace && exists && scan::is_workspace(&real);
    if paths::is_absolute(&lexical) && (is_folder || is_workspace) {
        Ok(real)
    } else {
        Err(Error::invalid_path(explain::not_existing(path, workspace)))
    }
}

/// Adds the folder `folder` as an Agent home of the Agent `agent`, once.
/// Returns the Agent's id and the folder's real path.
pub(crate) fn add_home(
    definitions: &[AgentDefinition],
    ports: &Ports,
    folder: &Path,
    agent: &str,
) -> Result<(String, PathBuf), Error> {
    let agent = definitions
        .iter()
        .find(|definition| definition.agent.id.eq_ignore_ascii_case(agent))
        .map(|definition| definition.agent.id.clone())
        .ok_or_else(|| Error::no_agent(agent))?;
    let folder = existing(ports.fs.as_ref(), folder, false)?;
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

/// Adds the existing folder or `.code-workspace` file `path` as a Project,
/// once. Returns its real path.
pub(crate) fn add_project(ports: &Ports, path: &Path) -> Result<PathBuf, Error> {
    let path = existing(ports.fs.as_ref(), path, true)?;
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
