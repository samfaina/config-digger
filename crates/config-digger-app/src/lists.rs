//! The Projects and Agent homes the user adds, hides and forgets, as the
//! Projects & homes screen changes them. These live in config-digger's own
//! lists: no change here writes any Agent configuration.

use crate::session::{Places, Scans, SessionState};
use config_digger_core::api::{Core, Error, Home, Project, Snapshot};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// What the webview asks to change in config-digger's lists.
#[derive(Debug, Clone, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ListChange {
    /// Adds an existing folder or `.code-workspace` file as a Project.
    AddProject { path: PathBuf },
    /// Hides a Project, discovered or added, from the Inventory's selector.
    HideProject { path: PathBuf },
    /// Lists a hidden Project again.
    UnhideProject { path: PathBuf },
    /// Forgets a Project the user added and no registry records.
    ForgetProject { path: PathBuf },
    /// Adds an existing folder as an Agent home of `agent`, with no other check.
    AddHome { folder: PathBuf, agent: String },
    /// Hides an Agent home, discovered or added, and its Items.
    HideHome { agent: String, home: String },
    /// Lists a hidden Agent home again.
    UnhideHome { agent: String, home: String },
    /// Forgets an Agent home the user added and no definition finds.
    ForgetHome { agent: String, home: String },
}

/// The Project or Agent home a change applied to, as it is listed now.
#[derive(Debug, Clone, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Listed {
    /// A Project.
    Project { project: Project },
    /// An Agent home, boxed since it is much larger than a Project.
    Home { home: Box<Home> },
}

/// What a change to config-digger's lists did, read again at once.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct ListChanged {
    /// The Project or Agent home it changed, as it is listed now.
    pub listed: Listed,
    /// Every Project and Agent home, read again after the change.
    pub places: Places,
}

/// Applies `change` to config-digger's lists, then reads every Agent home
/// and Project once through `scans` and keeps that scan in `kept`. The
/// Project or Agent home it names is looked for in the scan `kept` held,
/// which is what the screen showed; with none kept, one is read first.
pub fn change_list(
    kept: &SessionState,
    change: &ListChange,
    scans: &Scans,
) -> Result<ListChanged, Error> {
    let applied = {
        let mut last = kept.lock();
        if last.is_none() {
            *last = Some(scans.scan()?);
        }
        let last = last.as_ref().expect("read above");
        change.apply(last.core(), last.snapshot())?
    };
    let session = scans.scan()?;
    let listed = applied.listed(session.core(), session.snapshot())?;
    let places = session.places();
    kept.keep(session);
    Ok(ListChanged { listed, places })
}

impl ListChange {
    /// What the change does, as the log names it.
    pub fn kind(&self) -> &'static str {
        match self {
            ListChange::AddProject { .. } => "add_project",
            ListChange::HideProject { .. } => "hide_project",
            ListChange::UnhideProject { .. } => "unhide_project",
            ListChange::ForgetProject { .. } => "forget_project",
            ListChange::AddHome { .. } => "add_home",
            ListChange::HideHome { .. } => "hide_home",
            ListChange::UnhideHome { .. } => "unhide_home",
            ListChange::ForgetHome { .. } => "forget_home",
        }
    }

    /// Writes the change to config-digger's lists through `core`, looking
    /// for what it names in `listing`.
    fn apply(&self, core: &Core, listing: &Snapshot) -> Result<Applied, Error> {
        let home = |home: Home| Applied::Home {
            agent: home.agent,
            home: home.home,
        };
        let forgotten_home = |home: Home| {
            Applied::Forgotten(Listed::Home {
                home: Box::new(home),
            })
        };
        Ok(match self {
            ListChange::AddProject { path } => Applied::Project(core.add_project_to_list(path)?),
            ListChange::HideProject { path } => {
                Applied::Project(core.hide_project_in(listing, path)?.path)
            }
            ListChange::UnhideProject { path } => {
                Applied::Project(core.unhide_project_in(listing, path)?.path)
            }
            ListChange::ForgetProject { path } => Applied::Forgotten(Listed::Project {
                project: core.forget_project_in(listing, path)?,
            }),
            ListChange::AddHome { folder, agent } => {
                let (agent, folder) = core.add_home_to_list(folder, agent)?;
                Applied::Home {
                    agent,
                    home: folder.display().to_string(),
                }
            }
            ListChange::HideHome { agent, home: id } => {
                home(core.hide_home_in(listing, id, Some(agent))?)
            }
            ListChange::UnhideHome { agent, home: id } => {
                home(core.unhide_home_in(listing, id, Some(agent))?)
            }
            ListChange::ForgetHome { agent, home: id } => {
                forgotten_home(core.forget_home_in(listing, id, Some(agent))?)
            }
        })
    }
}

/// What a change wrote, to find again in the scan read after it.
enum Applied {
    /// The Project at this path.
    Project(PathBuf),
    /// The Agent home of `agent` whose id or folder is `home`.
    Home { agent: String, home: String },
    /// What was forgotten, as it was listed before: no scan lists it now.
    Forgotten(Listed),
}

impl Applied {
    /// The Project or Agent home changed, as `snapshot` lists it.
    fn listed(self, core: &Core, snapshot: &Snapshot) -> Result<Listed, Error> {
        Ok(match self {
            Applied::Project(path) => Listed::Project {
                project: core.project_in(snapshot, &path)?,
            },
            Applied::Home { agent, home } => Listed::Home {
                home: Box::new(core.home_in(snapshot, &home, Some(&agent))?),
            },
            Applied::Forgotten(listed) => listed,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::Session;
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

    /// The app's last scan of `fs`, as the Projects & homes screen keeps it,
    /// and a scan port of `fs` that counts its calls.
    fn kept_and_scans(fs: &FakeFileSystem) -> (SessionState, Scans) {
        let kept = SessionState::default();
        kept.keep(Session::scan(core(fs)).unwrap());
        let fs = fs.clone();
        (kept, Scans::new(move || Session::scan(core(&fs))))
    }

    /// Applies `change` through [`change_list`], with how many scans it ran.
    fn change(
        kept: &SessionState,
        scans: &Scans,
        change: ListChange,
    ) -> (Result<ListChanged, Error>, u32) {
        let before = scans.count();
        let result = change_list(kept, &change, scans);
        (result, scans.count() - before)
    }

    /// The Project at `path` that `places` lists.
    fn listed_project<'p>(places: &'p Places, path: &Path) -> Option<&'p Project> {
        places.projects.iter().find(|project| project.path == path)
    }

    #[test]
    fn a_project_is_added_hidden_unhidden_and_forgotten_each_with_one_scan() {
        let fs = fixture();
        let (kept, scans) = kept_and_scans(&fs);
        let path = PathBuf::from(PROJECT);
        let on_path = |change: fn(PathBuf) -> ListChange| change(path.clone());

        let (added, scanned) = change(
            &kept,
            &scans,
            on_path(|path| ListChange::AddProject { path }),
        );
        let added = added.unwrap();
        assert_eq!(scanned, 1);
        assert!(project(added.listed).added);
        assert!(listed_project(&added.places, &path).is_some());

        let (hidden, scanned) = change(
            &kept,
            &scans,
            on_path(|path| ListChange::HideProject { path }),
        );
        let hidden = hidden.unwrap();
        assert_eq!(scanned, 1);
        assert!(project(hidden.listed).hidden);
        assert!(listed_project(&hidden.places, &path).unwrap().hidden);
        let kept_places = kept.lock().as_ref().unwrap().places();
        assert!(listed_project(&kept_places, &path).unwrap().hidden);

        let (unhidden, scanned) = change(
            &kept,
            &scans,
            on_path(|path| ListChange::UnhideProject { path }),
        );
        let unhidden = unhidden.unwrap();
        assert_eq!(scanned, 1);
        assert!(!project(unhidden.listed).hidden);
        assert!(!listed_project(&unhidden.places, &path).unwrap().hidden);

        let (forgotten, scanned) = change(
            &kept,
            &scans,
            on_path(|path| ListChange::ForgetProject { path }),
        );
        let forgotten = forgotten.unwrap();
        assert_eq!(scanned, 1);
        assert_eq!(project(forgotten.listed).path, path);
        assert!(listed_project(&forgotten.places, &path).is_none());

        let (refused, scanned) = change(
            &kept,
            &scans,
            on_path(|path| ListChange::HideProject { path }),
        );
        let refused = refused.unwrap_err();
        assert_eq!(scanned, 0);
        assert!(refused.message.contains(PROJECT), "{}", refused.message);
    }

    #[test]
    fn an_agent_home_is_added_without_its_files_hidden_unhidden_and_forgotten_each_with_one_scan() {
        let fs = fixture();
        let (kept, scans) = kept_and_scans(&fs);
        let agent = "claude-code".to_owned();

        let (added, scanned) = change(
            &kept,
            &scans,
            ListChange::AddHome {
                folder: PathBuf::from(PORTABLE),
                agent: agent.clone(),
            },
        );
        let added = home(added.unwrap().listed);
        assert_eq!(scanned, 1);
        assert!(added.no_configuration.is_some());
        // Applies a change to the added home, as its Agent and id name it.
        let on_added = |change_of: fn(String, String) -> ListChange| {
            change(&kept, &scans, change_of(agent.clone(), added.home.clone()))
        };
        // The added home as `places` lists it.
        let listed = |places: &Places| {
            places
                .agents
                .iter()
                .flat_map(|agent| &agent.homes)
                .find(|home| home.home == added.home)
                .cloned()
        };
        let hide = |agent, home| ListChange::HideHome { agent, home };

        let (hidden, scanned) = on_added(hide);
        let hidden = hidden.unwrap();
        assert_eq!(scanned, 1);
        assert!(home(hidden.listed).hidden);
        assert!(listed(&hidden.places).unwrap().hidden);

        let (unhidden, scanned) = on_added(|agent, home| ListChange::UnhideHome { agent, home });
        let unhidden = unhidden.unwrap();
        assert_eq!(scanned, 1);
        assert!(!home(unhidden.listed).hidden);
        assert!(!listed(&unhidden.places).unwrap().hidden);

        let (forgotten, scanned) = on_added(|agent, home| ListChange::ForgetHome { agent, home });
        let forgotten = forgotten.unwrap();
        assert_eq!(scanned, 1);
        assert_eq!(home(forgotten.listed).home, added.home);
        assert!(listed(&forgotten.places).is_none());

        assert!(on_added(hide).0.is_err());
    }

    #[test]
    fn a_discovered_home_is_not_forgotten_and_nothing_is_read() {
        let fs = fixture();
        let (kept, scans) = kept_and_scans(&fs);

        let (refused, scanned) = change(
            &kept,
            &scans,
            ListChange::ForgetHome {
                agent: "claude-code".to_owned(),
                home: "default".to_owned(),
            },
        );

        let refused = refused.unwrap_err();
        assert!(refused.message.contains("hide"), "{}", refused.message);
        assert_eq!(scanned, 0);
    }

    #[test]
    fn with_no_scan_kept_a_change_reads_one_to_look_in_first() {
        let fs = fixture();
        let (_, scans) = kept_and_scans(&fs);
        let kept = SessionState::default();

        let (hidden, scanned) = change(
            &kept,
            &scans,
            ListChange::HideHome {
                agent: "claude-code".to_owned(),
                home: "default".to_owned(),
            },
        );

        assert!(home(hidden.unwrap().listed).hidden);
        assert_eq!(scanned, 2);
    }
}
