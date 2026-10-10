//! The Projects and Agent homes the user adds, hides and forgets, as the
//! Projects & homes screen changes them. These live in config-digger's own
//! lists: no change here writes any Agent configuration.

use config_digger_core::api::{Core, Error, Home, Project};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// What the webview asks to change in config-digger's lists.
#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ListChange {
    /// Adds an existing folder or `.code-workspace` file as a Project.
    AddProject {
        path: PathBuf,
    },
    HideProject {
        path: PathBuf,
    },
    UnhideProject {
        path: PathBuf,
    },
    /// Forgets a Project the user added and no registry records.
    ForgetProject {
        path: PathBuf,
    },
    /// Adds an existing folder as an Agent home of `agent`, with no other check.
    AddHome {
        folder: PathBuf,
        agent: String,
    },
    HideHome {
        agent: String,
        home: String,
    },
    UnhideHome {
        agent: String,
        home: String,
    },
    /// Forgets an Agent home the user added and no definition finds.
    ForgetHome {
        agent: String,
        home: String,
    },
}

/// The Project or Agent home a change applied to, as it is listed now.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Listed {
    Project { project: Project },
    Home { home: Box<Home> },
}

impl ListChange {
    /// Applies the change through `core`.
    pub fn apply(&self, core: &Core) -> Result<Listed, Error> {
        let project = |project| Listed::Project { project };
        let home = |home| Listed::Home {
            home: Box::new(home),
        };
        Ok(match self {
            ListChange::AddProject { path } => project(core.add_project(path)?),
            ListChange::HideProject { path } => project(core.hide_project(path)?),
            ListChange::UnhideProject { path } => project(core.unhide_project(path)?),
            ListChange::ForgetProject { path } => project(core.forget_project(path)?),
            ListChange::AddHome { folder, agent } => home(core.add_home(folder, agent)?),
            ListChange::HideHome { agent, home: id } => home(core.hide_home(id, Some(agent))?),
            ListChange::UnhideHome { agent, home: id } => home(core.unhide_home(id, Some(agent))?),
            ListChange::ForgetHome { agent, home: id } => home(core.forget_home(id, Some(agent))?),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use config_digger_core::ports::fake::{self, FakeEnvironment, FakeFileSystem};
    use std::path::Path;

    const HOME: &str = r"C:\Users\fixture";
    const PROJECT: &str = r"E:\projects\app";
    const PORTABLE: &str = r"D:\portable\claude";

    fn fixture() -> FakeFileSystem {
        FakeFileSystem::new()
            .with_file(Path::new(HOME).join(".claude.json"), "{}")
            .with_folder(PROJECT)
            .with_folder(PORTABLE)
    }

    fn core(fs: &FakeFileSystem) -> Core {
        let env = FakeEnvironment::with_home(HOME)
            .with_var("APPDATA", r"C:\Users\fixture\AppData\Roaming");
        Core::new(fake::ports(fs.clone(), env)).unwrap()
    }

    fn project(listed: Listed) -> Project {
        match listed {
            Listed::Project { project } => project,
            Listed::Home { .. } => panic!("a Project"),
        }
    }

    fn home(listed: Listed) -> Home {
        match listed {
            Listed::Home { home } => *home,
            Listed::Project { .. } => panic!("an Agent home"),
        }
    }

    #[test]
    fn a_project_is_added_hidden_unhidden_and_forgotten() {
        let fs = fixture();
        let core = core(&fs);
        let path = PathBuf::from(PROJECT);

        let added = project(
            ListChange::AddProject { path: path.clone() }
                .apply(&core)
                .unwrap(),
        );
        assert!(added.added);
        let hidden = project(
            ListChange::HideProject { path: path.clone() }
                .apply(&core)
                .unwrap(),
        );
        assert!(hidden.hidden);
        let unhidden = project(
            ListChange::UnhideProject { path: path.clone() }
                .apply(&core)
                .unwrap(),
        );
        assert!(!unhidden.hidden);
        project(
            ListChange::ForgetProject { path: path.clone() }
                .apply(&core)
                .unwrap(),
        );

        let forgotten = ListChange::HideProject { path }.apply(&core).unwrap_err();
        assert!(forgotten.message.contains(PROJECT), "{}", forgotten.message);
    }

    #[test]
    fn an_agent_home_is_added_without_its_files_hidden_unhidden_and_forgotten() {
        let fs = fixture();
        let core = core(&fs);
        let agent = "claude-code".to_owned();

        let added = home(
            ListChange::AddHome {
                folder: PathBuf::from(PORTABLE),
                agent: agent.clone(),
            }
            .apply(&core)
            .unwrap(),
        );
        assert!(added.no_configuration.is_some());
        let id = added.home;
        let named = |id: &str| (agent.clone(), id.to_owned());
        let (a, h) = named(&id);
        assert!(
            home(
                ListChange::HideHome { agent: a, home: h }
                    .apply(&core)
                    .unwrap()
            )
            .hidden
        );
        let (a, h) = named(&id);
        assert!(
            !home(
                ListChange::UnhideHome { agent: a, home: h }
                    .apply(&core)
                    .unwrap()
            )
            .hidden
        );
        let (a, h) = named(&id);
        home(
            ListChange::ForgetHome { agent: a, home: h }
                .apply(&core)
                .unwrap(),
        );

        let (a, h) = named(&id);
        assert!(
            ListChange::HideHome { agent: a, home: h }
                .apply(&core)
                .is_err()
        );
    }

    #[test]
    fn a_discovered_home_is_not_forgotten() {
        let fs = fixture();
        let core = core(&fs);

        let refused = ListChange::ForgetHome {
            agent: "claude-code".to_owned(),
            home: "default".to_owned(),
        }
        .apply(&core)
        .unwrap_err();

        assert!(refused.message.contains("hide"), "{}", refused.message);
    }
}
