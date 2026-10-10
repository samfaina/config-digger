//! What the app keeps between commands: the last scan, so that switching
//! the Project resolves again without reading the disk, and the locations
//! the webview may ask to open.

use config_digger_core::api::{
    Agent, Backup, Context, Core, Error, Inventory, Leftovers, Project, ScanOptions, Snapshot,
};
use serde::Serialize;
use std::collections::HashSet;
use std::path::{Path, PathBuf};

/// What the Projects & homes screen lists: every Project and Agent home,
/// hidden ones included, as `projects list` and `homes list` do with
/// `--include-hidden`.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct Places {
    /// When the snapshot was read, in RFC 3339.
    pub read_at: String,
    /// Every Project, missing and hidden ones included, sorted by path.
    pub projects: Vec<Project>,
    /// The leftovers of each missing Project that has any.
    pub leftovers: Vec<Leftovers>,
    /// Every loaded Agent definition, with each of its Agent homes.
    pub agents: Vec<Agent>,
}

/// The last scan and what was resolved from it.
pub struct Session {
    core: Core,
    snapshot: Snapshot,
    /// The locations shown so far: every Entry and Read failure of each
    /// resolved context, and every backup copy listed.
    openable: HashSet<String>,
}

impl Session {
    /// Scans every Agent home and Project.
    pub fn scan(core: Core) -> Result<Self, Error> {
        let snapshot = core.scan(&ScanOptions::default())?;
        Ok(Self {
            core,
            snapshot,
            openable: HashSet::new(),
        })
    }

    /// The inventory for `project`, or for no Project, without the Agent
    /// homes and Projects the user hid. It never reads the disk.
    pub fn resolve(&mut self, project: Option<PathBuf>) -> Inventory {
        let inventory = self
            .core
            .resolve(&self.snapshot, &Context { project })
            .without_hidden();
        let entries = inventory.items.iter().flat_map(|item| &item.entries);
        let locations = entries.map(|entry| entry.path.as_path()).chain(
            inventory
                .read_failures
                .iter()
                .map(|failure| failure.path.as_path()),
        );
        for location in locations {
            self.allow(location);
        }
        inventory
    }

    /// Every Project and Agent home of the last scan, hidden ones included,
    /// with the leftovers of the missing Projects. It never reads the disk.
    pub fn places(&self) -> Places {
        let inventory = self.core.resolve(&self.snapshot, &Context::default());
        Places {
            agents: self.core.agents(&inventory),
            leftovers: self.core.leftovers(&self.snapshot),
            read_at: inventory.read_at,
            projects: inventory.projects,
        }
    }

    /// The backups History holds of `path`, a location shown, whose copies
    /// can then be opened too.
    pub fn backups_of(&mut self, path: &Path) -> Result<Vec<Backup>, Error> {
        if !self.can_open(path) {
            return Err(not_shown(path));
        }
        let backups = self.core.backups_of(path)?;
        for backup in &backups {
            self.allow(&backup.copy);
        }
        Ok(backups)
    }

    /// Whether `path` is a location shown, in any letter case: the webview
    /// opens nothing else.
    pub fn can_open(&self, path: &Path) -> bool {
        self.openable.contains(&key(path))
    }

    fn allow(&mut self, path: &Path) {
        self.openable.insert(key(path));
    }
}

/// Why a path the inventory never showed is not opened.
pub fn not_shown(path: &Path) -> Error {
    Error::unexpected(format!(
        "{} is not a location the inventory shows",
        path.display()
    ))
}

/// Windows compares paths without regard to letter case.
pub fn key(path: &Path) -> String {
    path.to_string_lossy().to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use config_digger_core::ports::fake::{self, FakeEnvironment, FakeFileSystem};

    const HOME: &str = r"C:\Users\fixture";
    const PROJECT: &str = r"E:\projects\app";

    fn claude_json() -> PathBuf {
        PathBuf::from(HOME).join(".claude.json")
    }

    fn mcp_json() -> PathBuf {
        PathBuf::from(PROJECT).join(".mcp.json")
    }

    /// A user server, and a project server in a Project Claude Code knows.
    fn fixture() -> FakeFileSystem {
        FakeFileSystem::new()
            .with_file(
                claude_json(),
                r#"{ "mcpServers": { "notes": { "command": "notes-mcp" } }, "projects": { "E:/projects/app": {} } }"#,
            )
            .with_file(mcp_json(), r#"{ "mcpServers": { "linear": { "url": "https://x" } } }"#)
            .with_folder(PROJECT)
    }

    fn core(fs: &FakeFileSystem) -> Core {
        let env = FakeEnvironment::with_home(HOME)
            .with_var("LOCALAPPDATA", r"C:\Users\fixture\AppData\Local")
            .with_var("APPDATA", r"C:\Users\fixture\AppData\Roaming");
        Core::new(fake::ports(fs.clone(), env)).unwrap()
    }

    fn session(fs: &FakeFileSystem) -> Session {
        Session::scan(core(fs)).unwrap()
    }

    fn names(inventory: &Inventory) -> Vec<&str> {
        inventory
            .items
            .iter()
            .map(|item| item.name.as_str())
            .collect()
    }

    #[test]
    fn switching_the_project_resolves_without_reading_the_disk() {
        let fs = fixture();
        let mut session = session(&fs);
        assert!(!names(&session.resolve(None)).contains(&"linear"));
        let read = fs.accesses();

        let inventory = session.resolve(Some(PathBuf::from(PROJECT)));

        assert!(
            names(&inventory).contains(&"linear"),
            "{:?}",
            names(&inventory)
        );
        assert_eq!(fs.accesses(), read);
    }

    #[test]
    fn only_locations_shown_can_be_opened() {
        let fs = fixture();
        let mut session = session(&fs);
        session.resolve(None);

        assert!(session.can_open(&claude_json()));
        assert!(session.can_open(Path::new(r"c:\users\FIXTURE\.claude.json")));
        assert!(!session.can_open(&mcp_json()));
        assert!(!session.can_open(Path::new(r"C:\Windows\System32\calc.exe")));

        session.resolve(Some(PathBuf::from(PROJECT)));
        assert!(session.can_open(&mcp_json()));
    }

    #[test]
    fn read_failures_can_be_opened() {
        let broken = fixture().with_file(mcp_json(), "{ not json");
        let mut session = session(&broken);

        let inventory = session.resolve(None);

        assert!(
            inventory
                .read_failures
                .iter()
                .any(|failure| failure.path == mcp_json()),
            "{:?}",
            inventory.read_failures
        );
        assert!(session.can_open(&mcp_json()));
    }

    #[test]
    fn backups_are_listed_only_for_locations_shown() {
        let fs = fixture();
        let mut session = session(&fs);

        let refused = session.backups_of(&claude_json()).err().unwrap();
        assert!(
            refused.message.contains("not a location"),
            "{}",
            refused.message
        );

        session.resolve(None);
        assert_eq!(session.backups_of(&claude_json()).unwrap(), []);
    }

    #[test]
    fn places_lists_hidden_projects_and_homes_and_the_leftovers_of_missing_ones() {
        let fs = FakeFileSystem::new()
            .with_file(
                claude_json(),
                r#"{ "projects": { "E:/projects/app": {}, "E:/projects/gone": { "mcpServers": { "old": { "command": "old" } } } } }"#,
            )
            .with_folder(PROJECT);
        core(&fs).hide_project(Path::new(PROJECT)).unwrap();
        core(&fs).hide_home("default", Some("claude-code")).unwrap();

        let places = session(&fs).places();

        let projects: Vec<(String, bool)> = places
            .projects
            .iter()
            .map(|project| (project.path.display().to_string(), project.hidden))
            .collect();
        assert_eq!(
            projects,
            [
                (PROJECT.to_owned(), true),
                (r"E:\projects\gone".to_owned(), false)
            ]
        );
        let leftovers: Vec<&str> = places
            .leftovers
            .iter()
            .flat_map(Leftovers::entries)
            .map(|entry| entry.locator.as_str())
            .collect();
        assert_eq!(
            leftovers,
            [r"C:\Users\fixture\.claude.json#projects.E:/projects/gone.mcpServers.old"]
        );
        let claude_code = places
            .agents
            .iter()
            .find(|agent| agent.id == "claude-code")
            .unwrap();
        assert!(claude_code.homes[0].hidden);
    }
}
