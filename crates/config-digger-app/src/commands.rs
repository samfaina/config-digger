//! Task-level commands: each one is a thin async wrapper that runs the
//! synchronous core on a blocking thread.

use crate::about::{About, Page};
use crate::actions::{ActionPreview, ActionRequest, Actions, Executed};
use crate::lists::{self, ListChange, ListChanged};
use crate::session::{self, ParkedEntry, Places, Scans, Session, SessionState};
use crate::updates::{UpdateStatus, UpdatesState};
use config_digger_core::api::{
    self, Backup, Core, DataFolders, HomeCheck, Inventory, OperationDetail, OperationSummary,
    ProjectCheck, Retention, Settings, WarningClass,
};
use config_digger_core::ports::Ports;
use serde::{Deserialize, Serialize};
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};
use std::time::Instant;
use tauri::{AppHandle, Manager, State, WebviewWindow};

/// Reads every Agent home and resolves its Items for `project`, or for no
/// Project, leaving out the Agent homes and Projects the user hid.
#[tauri::command]
#[specta::specta]
pub async fn read_inventory(
    app: AppHandle,
    project: Option<PathBuf>,
) -> Result<Inventory, api::Error> {
    let started = Instant::now();
    let result = blocking(move || {
        let mut session = scan()?;
        let inventory = session.resolve(project);
        app.state::<SessionState>().keep(session);
        Ok(inventory)
    })
    .await;

    let elapsed_ms = started.elapsed().as_millis();
    match &result {
        Ok(inventory) => tracing::info!(
            homes = inventory.homes.len(),
            items = inventory.items.len(),
            read_failures = inventory.read_failures.len(),
            elapsed_ms,
            "inventory read"
        ),
        Err(error) => tracing::error!(%error, elapsed_ms, "inventory failed"),
    }
    result
}

/// Resolves the last inventory read for `project`, or for no Project,
/// without reading the disk again. Reads it first if it was never read.
#[tauri::command]
#[specta::specta]
pub async fn switch_project(
    app: AppHandle,
    project: Option<PathBuf>,
) -> Result<Inventory, api::Error> {
    blocking(move || {
        let state = app.state::<SessionState>();
        let mut session = state.lock();
        if session.is_none() {
            *session = Some(scan()?);
        }
        Ok(session.as_mut().expect("read above").resolve(project))
    })
    .await
}

/// The backups History holds of `path`, a location the inventory shows,
/// newest first.
#[tauri::command]
#[specta::specta]
pub async fn backups_of(app: AppHandle, path: PathBuf) -> Result<Vec<Backup>, api::Error> {
    blocking(move || match app.state::<SessionState>().lock().as_mut() {
        Some(session) => session.backups_of(&path),
        None => Err(session::not_shown(&path)),
    })
    .await
}

/// Opens `path`, a location the inventory shows, in the app Windows
/// associates with it, or shows it selected in Explorer when it is a
/// folder, such as a skill's.
#[tauri::command]
#[specta::specta]
pub async fn open_location(app: AppHandle, path: PathBuf) -> Result<(), api::Error> {
    blocking(move || {
        ensure_shown(&app, &path)?;
        if is_link(&path) || path.is_dir() {
            select_in_explorer(&path)
        } else {
            open_in_windows(&path)
        }
    })
    .await
}

/// Shows `path`, a location the inventory shows or a backup copy of one,
/// selected in Explorer, in the folder that holds it.
#[tauri::command]
#[specta::specta]
pub async fn reveal_location(app: AppHandle, path: PathBuf) -> Result<(), api::Error> {
    blocking(move || {
        ensure_shown(&app, &path)?;
        select_in_explorer(&path)
    })
    .await
}

/// Opens Explorer inside `path`, the backup folder of an Operation the
/// dialog executed or History reported, to recover from it by hand. It
/// takes a location the inventory shows too, but shows it selected unless
/// it is a folder: it never starts a file nor follows a link.
#[tauri::command]
#[specta::specta]
pub async fn open_backup_folder(app: AppHandle, path: PathBuf) -> Result<(), api::Error> {
    blocking(move || {
        if !app.state::<ActionsState>().reported(&path) {
            ensure_shown(&app, &path)?;
        }
        if opens_inside(&path) {
            open_in_windows(&path)
        } else {
            select_in_explorer(&path)
        }
    })
    .await
}

/// The plans the action dialog holds, once one was planned.
#[derive(Default)]
pub struct ActionsState(Mutex<Option<Actions>>);

impl ActionsState {
    fn lock(&self) -> MutexGuard<'_, Option<Actions>> {
        // A panic in another command leaves the plans as they were.
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    /// Runs `task` on the plans, starting with none on its first call.
    fn with<T>(
        &self,
        task: impl FnOnce(&mut Actions) -> Result<T, api::Error>,
    ) -> Result<T, api::Error> {
        let mut actions = self.lock();
        if actions.is_none() {
            *actions = Some(Actions::new(Core::new(Ports::real())?));
        }
        task(actions.as_mut().expect("set above"))
    }

    /// Whether `path` is the backup folder of an Operation executed here, a
    /// refused Undo or a detail in History.
    fn reported(&self, path: &Path) -> bool {
        self.lock()
            .as_ref()
            .is_some_and(|actions| actions.can_open_backup_folder(path))
    }
}

/// Plans `request` against what the disk holds now, without writing
/// anything, and holds the plan under the id the preview carries.
#[tauri::command]
#[specta::specta]
pub async fn plan_action(
    app: AppHandle,
    request: ActionRequest,
) -> Result<ActionPreview, api::Error> {
    blocking(move || {
        app.state::<ActionsState>()
            .with(|actions| actions.plan(&request))
    })
    .await
}

/// Executes the plan held under `plan_id` with the warnings the user
/// acknowledged, and drops it.
#[tauri::command]
#[specta::specta]
pub async fn execute_plan(
    app: AppHandle,
    plan_id: String,
    acknowledged: Vec<WarningClass>,
) -> Result<Executed, api::Error> {
    let result = blocking(move || {
        app.state::<ActionsState>()
            .with(|actions| actions.execute(&plan_id, &acknowledged))
    })
    .await;
    match &result {
        Ok(Executed::Applied { outcome }) => {
            tracing::info!(operation = %outcome.operation, "operation applied")
        }
        Ok(Executed::Replanned { .. }) => tracing::info!("stale preview planned again"),
        Err(error) => tracing::warn!(code = error.code.code(), "operation not applied"),
    }
    result
}

/// Drops the plan held under `plan_id`, as when the dialog is cancelled.
#[tauri::command]
#[specta::specta]
pub async fn cancel_plan(app: AppHandle, plan_id: String) -> Result<(), api::Error> {
    blocking(move || {
        app.state::<ActionsState>().with(|actions| {
            actions.cancel(&plan_id);
            Ok(())
        })
    })
    .await
}

/// Every Operation in History, newest first.
#[tauri::command]
#[specta::specta]
pub async fn read_history(app: AppHandle) -> Result<Vec<OperationSummary>, api::Error> {
    blocking(move || {
        app.state::<ActionsState>()
            .with(|actions| actions.history())
    })
    .await
}

/// The Operation `id` in History, with what it changed in each file, whose
/// backup folder can then be opened.
#[tauri::command]
#[specta::specta]
pub async fn history_detail(app: AppHandle, id: String) -> Result<OperationDetail, api::Error> {
    blocking(move || {
        app.state::<ActionsState>()
            .with(|actions| actions.detail(&id))
    })
    .await
}

/// Dismisses the Operation `id`, which needs attention, once its files were
/// recovered by hand.
#[tauri::command]
#[specta::specta]
pub async fn dismiss_operation(app: AppHandle, id: String) -> Result<OperationSummary, api::Error> {
    let result = blocking(move || {
        app.state::<ActionsState>()
            .with(|actions| actions.dismiss(&id))
    })
    .await;
    if let Ok(summary) = &result {
        tracing::info!(operation = %summary.id, "operation dismissed");
    }
    result
}

/// Reads every Agent home and Project, and lists them with the hidden ones
/// and the leftovers of the missing Projects.
#[tauri::command]
#[specta::specta]
pub async fn read_places(app: AppHandle) -> Result<Places, api::Error> {
    blocking(move || Ok(keep_places(&app, scan()?))).await
}

/// Adds, hides, lists again or forgets a Project or an Agent home, as the
/// last scan lists it, then reads every Agent home and Project once. It
/// writes no Agent configuration.
#[tauri::command]
#[specta::specta]
pub async fn change_list(app: AppHandle, change: ListChange) -> Result<ListChanged, api::Error> {
    blocking(move || {
        let started = Instant::now();
        let scans = Scans::new(scan);
        let result = lists::change_list(&app.state::<SessionState>(), &change, &scans);
        let (kind, scan_count) = (change.kind(), scans.count());
        let elapsed_ms = started.elapsed().as_millis();
        match &result {
            Ok(_) => tracing::info!(
                change = kind,
                scans = scan_count,
                elapsed_ms,
                "list changed"
            ),
            Err(error) => tracing::warn!(
                code = error.code.code(),
                change = kind,
                scans = scan_count,
                elapsed_ms,
                "list not changed"
            ),
        }
        result
    })
    .await
}

/// Checks `folder` as an Agent home of `agent` before it is added: whether
/// any of the files the Agent reads in a home is there. It changes nothing.
#[tauri::command]
#[specta::specta]
pub async fn check_home(folder: PathBuf, agent: String) -> Result<HomeCheck, api::Error> {
    blocking(move || Core::new(Ports::real())?.check_home(&folder, &agent)).await
}

/// Checks `path` as a Project before it is added: which Agents list an
/// Agent home there, as the last scan lists them. It changes nothing.
#[tauri::command]
#[specta::specta]
pub async fn check_project(app: AppHandle, path: PathBuf) -> Result<ProjectCheck, api::Error> {
    blocking(move || match app.state::<SessionState>().lock().as_ref() {
        Some(session) => session.check_project(&path),
        None => Core::new(Ports::real())?.check_project(&path),
    })
    .await
}

/// What the Windows picker picks.
#[derive(Debug, Clone, Copy, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Pick {
    /// A folder, such as a Project or an Agent home.
    Folder,
    /// A `.code-workspace` file.
    WorkspaceFile,
}

/// Opens the Windows picker over the window, `None` when it is cancelled.
#[tauri::command]
#[specta::specta]
pub async fn pick_location(
    window: WebviewWindow,
    pick: Pick,
) -> Result<Option<PathBuf>, api::Error> {
    blocking(move || {
        let dialog = rfd::FileDialog::new().set_parent(&window);
        Ok(match pick {
            Pick::Folder => dialog.pick_folder(),
            Pick::WorkspaceFile => dialog
                .add_filter("VS Code workspace", &["code-workspace"])
                .pick_file(),
        })
    })
    .await
}

/// What the Settings screen shows.
#[derive(Debug, Clone, Serialize, specta::Type)]
pub struct SettingsRead {
    pub settings: Settings,
    pub folders: DataFolders,
    /// How many Operations History holds now.
    pub operations: u32,
    /// How long History keeps them.
    pub retention: Retention,
    /// Every parked Entry, with where its Item is listed.
    pub parked: Vec<ParkedEntry>,
}

/// Reads the app's settings, where config-digger keeps its data, and every
/// Agent home and Project again to list the parked Entries. The Inventory's
/// last scan stays as it was.
#[tauri::command]
#[specta::specta]
pub async fn read_settings() -> Result<SettingsRead, api::Error> {
    blocking(move || settings_read(Core::new(Ports::real())?)).await
}

/// What the Settings screen shows, read through `core`.
pub(crate) fn settings_read(core: Core) -> Result<SettingsRead, api::Error> {
    let settings = core.settings()?;
    let folders = core.data_folders()?;
    let operations = u32::try_from(core.operation_count()?).unwrap_or(u32::MAX);
    let retention = core.retention();
    let parked = Session::scan(core)?.parked();
    Ok(SettingsRead {
        settings,
        folders,
        operations,
        retention,
        parked,
    })
}

/// Saves the app's settings, and returns them as saved.
#[tauri::command]
#[specta::specta]
pub async fn save_settings(settings: Settings) -> Result<Settings, api::Error> {
    let result = blocking(move || {
        let core = Core::new(Ports::real())?;
        core.save_settings(&settings)?;
        core.settings()
    })
    .await;
    match &result {
        Ok(settings) => tracing::info!(
            check_updates_on_startup = settings.check_updates_on_startup,
            "settings saved"
        ),
        Err(error) => tracing::warn!(code = error.code.code(), "settings not saved"),
    }
    result
}

/// A folder config-digger keeps its own data in.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum DataFolder {
    Backups,
    Parked,
    Logs,
    Lists,
}

impl DataFolder {
    /// Every data folder, in the order Settings lists them.
    pub(crate) const ALL: [DataFolder; 4] = [
        DataFolder::Backups,
        DataFolder::Parked,
        DataFolder::Logs,
        DataFolder::Lists,
    ];

    /// Where this folder is among `folders`.
    pub(crate) fn path(self, folders: &DataFolders) -> &Path {
        match self {
            DataFolder::Backups => &folders.backups,
            DataFolder::Parked => &folders.parked,
            DataFolder::Logs => &folders.logs,
            DataFolder::Lists => &folders.lists,
        }
    }
}

/// Opens one of config-digger's data folders in Explorer, and says whether
/// it did. A folder that does not exist yet is not created: nothing was
/// kept there.
#[tauri::command]
#[specta::specta]
pub async fn open_data_folder(folder: DataFolder) -> Result<bool, api::Error> {
    blocking(move || {
        let folders = Core::new(Ports::real())?.data_folders()?;
        let path = folder.path(&folders);
        if !path.is_dir() {
            return Ok(false);
        }
        open_in_windows(path).map(|()| true)
    })
    .await
}

/// This build of the app, as About shows it.
#[tauri::command]
#[specta::specta]
pub async fn read_about() -> Result<About, api::Error> {
    Ok(About::this_build())
}

/// Opens one of the pages About links to in the default browser.
#[tauri::command]
#[specta::specta]
pub async fn open_page(page: Page) -> Result<(), api::Error> {
    let url = page.url(env!("CARGO_PKG_REPOSITORY"), env!("CARGO_PKG_VERSION"));
    blocking(move || open_in_windows(&url)).await
}

/// When the app last checked for updates, and the update found since it
/// started. It makes no network call.
#[tauri::command]
#[specta::specta]
pub async fn update_status(updates: State<'_, UpdatesState>) -> Result<UpdateStatus, api::Error> {
    updates.read_status().await
}

/// Checks for updates on the first call since the app started, when "Check
/// for updates on startup" is on. Otherwise it makes no network call and
/// only says what the app knows, as [`update_status`] does.
#[tauri::command]
#[specta::specta]
pub async fn startup_update_check(
    app: AppHandle,
    updates: State<'_, UpdatesState>,
) -> Result<UpdateStatus, api::Error> {
    updates.startup_check(&app).await
}

/// Checks for updates now, as Check now in About asks.
#[tauri::command]
#[specta::specta]
pub async fn check_for_updates(
    app: AppHandle,
    updates: State<'_, UpdatesState>,
) -> Result<UpdateStatus, api::Error> {
    updates.check(&app).await
}

/// Installs the update found and restarts the app, as Install and restart
/// asks. It fails at once while an Operation holds the global lock.
#[tauri::command]
#[specta::specta]
pub async fn install_update(updates: State<'_, UpdatesState>) -> Result<(), api::Error> {
    updates.install().await
}

/// Opens the release notes of the update found in the default browser.
#[tauri::command]
#[specta::specta]
pub async fn open_update_notes(updates: State<'_, UpdatesState>) -> Result<(), api::Error> {
    let Some(url) = updates.release_notes() else {
        return Err(api::Error::unexpected("no update was found"));
    };
    blocking(move || open_in_windows(&url)).await
}

/// Keeps `session` as the app's last scan, and lists its Projects and
/// Agent homes.
fn keep_places(app: &AppHandle, session: Session) -> Places {
    let places = session.places();
    app.state::<SessionState>().keep(session);
    places
}

/// Reads every Agent home and Project on this machine.
fn scan() -> Result<Session, api::Error> {
    Session::scan(Core::new(Ports::real())?)
}

/// Runs `task` on a blocking thread.
pub(crate) async fn blocking<T: Send + 'static>(
    task: impl FnOnce() -> Result<T, api::Error> + Send + 'static,
) -> Result<T, api::Error> {
    tauri::async_runtime::spawn_blocking(task)
        .await
        .unwrap_or_else(|error| Err(api::Error::unexpected(error)))
}

/// Refuses a path the inventory never showed: the webview opens nothing else.
fn ensure_shown(app: &AppHandle, path: &Path) -> Result<(), api::Error> {
    let state = app.state::<SessionState>();
    match state.lock().as_ref() {
        Some(session) if session.can_open(path) => Ok(()),
        _ => Err(session::not_shown(path)),
    }
}

/// Whether `path` is a link, even a broken one, which Explorer shows
/// selected rather than follows.
fn is_link(path: &Path) -> bool {
    std::fs::symlink_metadata(path).is_ok_and(|meta| meta.file_type().is_symlink())
}

/// Whether Explorer opens `path` from the inside: a folder that is no link.
fn opens_inside(path: &Path) -> bool {
    !is_link(path) && path.is_dir()
}

/// Opens `target`, a path or a URL, in the app Windows associates with it:
/// Explorer inside a folder, the default browser for a URL.
// Explorer is a GUI program, so it opens no console window.
#[allow(clippy::disallowed_methods)]
fn open_in_windows(target: impl AsRef<OsStr>) -> Result<(), api::Error> {
    let mut explorer = Command::new("explorer.exe");
    explorer.arg(target);
    start(explorer)
}

/// Opens Explorer on the folder holding `path`, with `path` selected.
#[cfg(windows)]
// Explorer is a GUI program, so it opens no console window.
#[allow(clippy::disallowed_methods)]
fn select_in_explorer(path: &Path) -> Result<(), api::Error> {
    use std::os::windows::process::CommandExt;
    // Explorer reads `/select,` and the quoted path as one argument, which
    // the usual quoting of a whole argument breaks. No Windows path holds `"`.
    let mut explorer = Command::new("explorer.exe");
    explorer.raw_arg(format!("/select,\"{}\"", path.display()));
    start(explorer)
}

#[cfg(not(windows))]
fn select_in_explorer(_path: &Path) -> Result<(), api::Error> {
    Err(api::Error::unexpected("Explorer is only on Windows"))
}

/// Starts Explorer. It exits with 1 even when it succeeds, so only a
/// failure to start it is one.
fn start(mut explorer: Command) -> Result<(), api::Error> {
    explorer
        .spawn()
        .map(drop)
        .map_err(|error| api::Error::unexpected(format!("cannot start Explorer: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use config_digger_core::api::{Action, Context, ScanOptions, Targets};
    use config_digger_core::ports::fake::{self, FakeEnvironment, FakeFileSystem, FakeGitIndex};

    const HOME: &str = r"C:\Users\fixture";
    const LOCAL_APP_DATA: &str = r"C:\Users\fixture\AppData\Local";

    fn env() -> FakeEnvironment {
        FakeEnvironment::with_home(HOME)
            .with_var("APPDATA", r"C:\Users\fixture\AppData\Roaming")
            .with_var("LOCALAPPDATA", LOCAL_APP_DATA)
    }

    /// A core over `fs` whose git index is `git`.
    fn core(fs: &FakeFileSystem, git: &FakeGitIndex) -> Core {
        let mut ports = fake::ports(fs.clone(), env());
        ports.git = Box::new(git.clone());
        Core::new(ports).unwrap()
    }

    /// A Cursor that has run once, with the user's skill `review`, in a
    /// dotfiles repo at the home folder, parked in config-digger's store by
    /// a Disable that History holds.
    fn parked_skill() -> FakeFileSystem {
        let skill = PathBuf::from(HOME).join(r".cursor\skills\review");
        let fs = FakeFileSystem::new()
            .with_file(
                PathBuf::from(HOME).join(r"AppData\Roaming\Cursor\User\settings.json"),
                "{}\n",
            )
            .with_file(skill.join("SKILL.md"), "---\nname: review\n---\n");
        let core = core(&fs, &FakeGitIndex::default().with_repository(HOME));
        let snapshot = core.scan(&ScanOptions { projects: vec![] }).unwrap();
        let targets = Targets {
            selectors: vec![skill.display().to_string()],
            all: false,
            ..Targets::default()
        };
        let plan = core
            .plan(&snapshot, &Context::default(), Action::Disable, &targets)
            .unwrap();
        core.execute(plan, &[WarningClass::WideReach]).unwrap();
        fs
    }

    /// What Settings reads over `fs`, and the repositories that asks the
    /// git index about, sorted.
    fn read_settings_and_git_asks(fs: &FakeFileSystem) -> (SettingsRead, Vec<PathBuf>) {
        let git = FakeGitIndex::default().with_repository(HOME);
        let read = settings_read(core(fs, &git)).unwrap();
        let mut asked = git.asked();
        asked.sort();
        (read, asked)
    }

    #[test]
    fn settings_asks_the_git_index_nothing_about_history() {
        let fs = parked_skill();
        let (with_history, asked_with_history) = read_settings_and_git_asks(&fs);
        fs.delete_folder(PathBuf::from(LOCAL_APP_DATA).join(r"config-digger\backups"));

        let (without_history, asked_without_history) = read_settings_and_git_asks(&fs);

        assert_eq!(with_history.operations, 1);
        assert_eq!(without_history.operations, 0);
        // Both scans find the parked skill, so they ask the same.
        assert_eq!(with_history.parked.len(), 1);
        assert_eq!(without_history.parked.len(), 1);
        assert_eq!(asked_with_history, asked_without_history);
    }

    #[test]
    fn only_a_folder_is_opened_from_the_inside() {
        let temp = tempfile::tempdir().unwrap();
        let folder = temp.path().join("20260101T000000Z-remove");
        std::fs::create_dir(&folder).unwrap();
        let copy = folder.join("1-config.toml");
        std::fs::write(&copy, "").unwrap();

        assert!(opens_inside(&folder));
        assert!(!opens_inside(&copy));
        assert!(!opens_inside(&temp.path().join("gone")));
    }

    #[cfg(windows)]
    #[test]
    fn a_link_to_a_folder_is_not_followed() {
        let temp = tempfile::tempdir().unwrap();
        let folder = temp.path().join("skill");
        std::fs::create_dir(&folder).unwrap();
        let link = temp.path().join("link");
        // Creating a symbolic link needs Developer Mode or an elevated shell.
        if std::os::windows::fs::symlink_dir(&folder, &link).is_err() {
            return;
        }

        assert!(!opens_inside(&link));
    }
}
