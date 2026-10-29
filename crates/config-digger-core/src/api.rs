//! The public surface shared by the app and the CLI: [`Core`] and the serde
//! DTOs it returns. Domain types stay internal. The DTOs also derive
//! `specta::Type`, from which the app's TypeScript bindings are generated.

use crate::definitions::blocks::{
    FindsBy, Locate, Provenance as DefinitionProvenance, SkillFallback,
};
use crate::definitions::{self, Definitions, ItemKind as DefinitionItemKind};
use crate::edit::Edit;
use crate::ports::{Environment, Ports};
use crate::{
    execute, explain, history, park, paths, plan, resolve, scan, select, settings, skill, updates,
    user_list,
};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::Instant;

/// The version of every JSON document config-digger emits. It is 1 at the
/// first release. Bump it on any breaking change to a document: a field
/// renamed, removed or retyped. A new field is not one.
pub const SCHEMA_VERSION: u32 = 1;

/// Runs `plan`, logged with how long it took, whether it finished or
/// failed. A failed plan is a warning: its error reaches the caller as it
/// is.
fn timed_plan(plan: impl FnOnce() -> Result<plan::Planned, Error>) -> Result<Plan, Error> {
    let started = Instant::now();
    tracing::info!("plan started");
    let result = plan();
    let elapsed_ms = started.elapsed().as_millis();
    match &result {
        Ok(_) => tracing::info!(elapsed_ms, "plan finished"),
        Err(error) => tracing::warn!(elapsed_ms, %error, "plan failed"),
    }
    result.map(Plan)
}

/// The core, holding the ports and the loaded definitions.
pub struct Core {
    ports: Ports,
    definitions: Definitions,
    /// How many of the Agent definitions, at their start, are built in.
    built_in: usize,
}

/// A [`Core`] loaded with a folder of extra definitions, and the files of
/// it that were rejected.
pub struct Loaded {
    pub core: Core,
    /// Each file that failed to load, in the order of their names. None of
    /// it was loaded.
    pub rejected: Vec<RejectedDefinition>,
}

/// An extra definition file that failed to load, and why.
#[derive(Debug, Clone)]
pub struct RejectedDefinition {
    pub file: PathBuf,
    /// The validation error, naming the file.
    pub message: String,
}

/// Loads each definition file directly in `folder` with `parse`, after
/// those in `loaded`, adding to `rejected` each that fails to load or whose
/// `id` one already loaded has.
fn load_folder<D>(
    ports: &Ports,
    folder: &Path,
    loaded: &mut Vec<D>,
    rejected: &mut Vec<RejectedDefinition>,
    parse: fn(&str, &str) -> Result<D, definitions::DefinitionError>,
    id: fn(&D) -> &String,
) -> Result<(), Error> {
    for (file, text) in definition_files(ports, folder)? {
        let name = file.display().to_string();
        let parsed = text.and_then(|text| {
            let definition = parse(&name, &text).map_err(|e| e.to_string())?;
            if loaded.iter().any(|known| id(known) == id(&definition)) {
                return Err(format!(
                    "definition `{name}` is invalid: id `{}` is taken by another definition",
                    id(&definition)
                ));
            }
            Ok(definition)
        });
        match parsed {
            Ok(definition) => loaded.push(definition),
            Err(message) => rejected.push(RejectedDefinition { file, message }),
        }
    }
    Ok(())
}

/// A definition file, with its text or why it cannot be read.
type DefinitionFile = (PathBuf, Result<String, String>);

/// The `*.toml` files directly in `folder`, in the order of their names.
fn definition_files(ports: &Ports, folder: &Path) -> Result<Vec<DefinitionFile>, Error> {
    let mut files = ports.fs.read_dir(folder).map_err(|error| {
        Error::invalid_path(format!(
            "cannot read the definitions folder {}: {error}",
            folder.display()
        ))
    })?;
    files.retain(|file| {
        file.extension()
            .is_some_and(|extension| extension.eq_ignore_ascii_case("toml"))
    });
    files.sort_by_key(|file| paths::key(file));
    Ok(files
        .into_iter()
        .map(|file| {
            let text = ports
                .fs
                .read(&file)
                .map_err(|error| error.to_string())
                .and_then(|bytes| String::from_utf8(bytes).map_err(|error| error.to_string()))
                .map_err(|error| format!("cannot read {}: {error}", file.display()));
            (file, text)
        })
        .collect())
}

/// What [`Core::scan`] read. Resolve it as often as needed without reading the disk again.
pub struct Snapshot {
    scanned: scan::Snapshot,
    /// What it was scanned with, so that Verify can scan the same way again.
    options: ScanOptions,
}

/// What to read besides what the Agents' registries record.
#[derive(Debug, Clone, Default)]
pub struct ScanOptions {
    /// Projects to evaluate even when no registry knows them, in any spelling.
    pub projects: Vec<PathBuf>,
}

/// The context Entries are resolved for: every Agent home, with one Project or none.
#[derive(Debug, Clone, Default)]
pub struct Context {
    /// The Project in any spelling, or `None` for no Project. It names the same
    /// Project as any spelling of the same folder that the scan saw, including
    /// a link to it; otherwise it is compared lexically.
    pub project: Option<PathBuf>,
}

impl Core {
    /// Loads the built-in Agent definitions.
    pub fn new(ports: Ports) -> Result<Self, Error> {
        Self::with_extra_definitions(ports, &[])
    }

    /// Loads the built-in definitions plus the extra ones in `folder`, as
    /// `--definitions <dir>` does for development, tests and contributors
    /// (ADR 0005): each `*.toml` file in it is an Agent definition, and each
    /// one in its `skill-managers` folder a skill manager definition, each
    /// kind in the order of their names, after the built-in ones. A file
    /// that fails to load is rejected whole, with the reason, and the others
    /// load all the same.
    pub fn with_definitions_folder(ports: Ports, folder: &Path) -> Result<Loaded, Error> {
        let mut core = Self::new(ports)?;
        let mut rejected = Vec::new();
        load_folder(
            &core.ports,
            folder,
            &mut core.definitions.agents,
            &mut rejected,
            definitions::parse,
            |definition| &definition.agent.id,
        )?;
        let managers = folder.join("skill-managers");
        if core.ports.fs.exists(&managers) {
            load_folder(
                &core.ports,
                &managers,
                &mut core.definitions.skill_managers,
                &mut rejected,
                definitions::skill_managers::parse,
                |definition| &definition.manager.id,
            )?;
        }
        rejected.sort_by_key(|rejected| paths::key(&rejected.file));
        Ok(Loaded { core, rejected })
    }

    /// Loads the built-in Agent definitions plus the extra ones in `files`,
    /// each a file name and its TOML text, as `--definitions` does for
    /// development and tests (ADR 0005). A file that fails to load fails
    /// the whole load.
    pub fn with_extra_definitions(ports: Ports, files: &[(&str, &str)]) -> Result<Self, Error> {
        let mut definitions = Definitions::built_in().map_err(Error::unexpected)?;
        let built_in = definitions.len();
        for (file, text) in files {
            let definition = definitions::parse(file, text).map_err(Error::unexpected)?;
            definitions.agents.push(definition);
        }
        Ok(Self {
            ports,
            definitions,
            built_in,
        })
    }

    /// Reads every location of every found Agent home and Project.
    pub fn scan(&self, options: &ScanOptions) -> Result<Snapshot, Error> {
        let scanned = scan::scan(&self.definitions, &self.ports, &options.projects)
            .map_err(Error::unexpected)?;
        Ok(Snapshot {
            scanned,
            options: options.clone(),
        })
    }

    /// The Items of every Agent home and each Entry's State, for `context`.
    /// It never reads the disk.
    pub fn resolve(&self, snapshot: &Snapshot, context: &Context) -> Inventory {
        resolve::resolve(&self.definitions, &snapshot.scanned, context)
    }

    /// The Unread Entries of every missing Project, by Project: what the
    /// Agents' user files keep for Projects whose folder is gone. Their
    /// locators can be Removed together in one Operation, from any context.
    /// It never reads the disk.
    pub fn leftovers(&self, snapshot: &Snapshot) -> Vec<Leftovers> {
        resolve::leftovers(&self.definitions, &snapshot.scanned)
    }

    /// The parked Entries whose origin is not found, in the order of their
    /// locators in the store. Each is Removed by that locator alone, from
    /// any context. It never reads the disk.
    pub fn stranded(&self, snapshot: &Snapshot) -> Vec<Stranded> {
        resolve::stranded(&self.definitions, &snapshot.scanned)
    }

    /// The one Item `selector` names in `context`, among those `narrowing`
    /// leaves: an Item name, or the `<path>#<key path>` locator of any of
    /// its Entries.
    pub fn show(
        &self,
        snapshot: &Snapshot,
        context: &Context,
        selector: &str,
        narrowing: &Narrowing,
    ) -> Result<Item, Error> {
        self.resolve(snapshot, context)
            .select_one(selector, narrowing)
            .cloned()
    }

    /// What narrows the Items a selector names to those of the Agent
    /// `agent` and of the Agent home `home`, each when given. `agent` is an
    /// Agent definition's id in any case; it fails when none has it.
    pub fn narrowing(&self, agent: Option<&str>, home: Option<&str>) -> Result<Narrowing, Error> {
        let agent = agent
            .map(|agent| user_list::definition_of(&self.definitions, agent))
            .transpose()?;
        Ok(Narrowing {
            agent: agent.map(|definition| definition.agent.id.clone()),
            home: home.map(str::to_owned),
        })
    }

    /// Plans `action` on the Entries `targets` names in `context`, without
    /// writing anything. Several selectors form one Operation, and it fails
    /// whole when any of them does.
    pub fn plan(
        &self,
        snapshot: &Snapshot,
        context: &Context,
        action: Action,
        targets: &Targets,
    ) -> Result<Plan, Error> {
        timed_plan(|| {
            plan::plan(
                &self.definitions,
                &self.ports,
                &snapshot.scanned,
                &snapshot.options,
                context,
                action,
                targets,
            )
        })
    }

    /// Plans the Remove of every leftover of the missing Project `project`,
    /// in any spelling, as one Operation in that Project's context, but those
    /// in the Agent homes the user hid, as the leftovers are listed by
    /// default. It fails with `NoMatch` when the Project is not missing or
    /// has no leftovers outside the hidden homes.
    pub fn plan_remove_leftovers(
        &self,
        snapshot: &Snapshot,
        project: &Path,
    ) -> Result<Plan, Error> {
        timed_plan(|| {
            plan::plan_remove_leftovers(
                &self.definitions,
                &self.ports,
                &snapshot.scanned,
                &snapshot.options,
                project,
            )
        })
    }

    /// Plans the Remove of every Stranded Entry as one Operation, with no
    /// Project. It fails with `NoMatch` when none is Stranded.
    pub fn plan_remove_stranded(&self, snapshot: &Snapshot) -> Result<Plan, Error> {
        timed_plan(|| {
            plan::plan_remove_stranded(
                &self.definitions,
                &self.ports,
                &snapshot.scanned,
                &snapshot.options,
            )
        })
    }

    /// Executes a plan once every warning in it is acknowledged: takes the
    /// global lock, backs up each touched file, writes, verifies and records
    /// the Operation in History.
    pub fn execute(&self, plan: Plan, acknowledged: &[WarningClass]) -> Result<Outcome, Error> {
        execute::execute(&self.definitions, &self.ports, plan.0, acknowledged)
    }

    /// Plans the Undo of the Operation `id`: the inverse of each of its
    /// edits, applied to the files as they are now, in the context it ran
    /// in. It is refused when what the Operation touched changed since.
    pub fn plan_undo(&self, id: &str) -> Result<Plan, Error> {
        timed_plan(|| plan::plan_undo(&self.definitions, &self.ports, id))
    }

    /// Plans the deletion of the backup of the Operation `id`, such as one
    /// holding Secrets: its copies go for good, and so do the values of its
    /// edits in History, so it can no longer be undone. When the Operation
    /// needs attention, the plan warns that the backup is the only copy for
    /// manual recovery, and executing it dismisses the Operation.
    pub fn plan_delete_backup(&self, id: &str) -> Result<Plan, Error> {
        timed_plan(|| plan::plan_delete_backup(&self.ports, id))
    }

    /// Every Operation in History, newest first, across restarts.
    pub fn history(&self) -> Result<Vec<OperationSummary>, Error> {
        Ok(history::summaries(
            &self.ports,
            &history::load_all(&self.ports)?,
        ))
    }

    /// How many Operations [`history`](Self::history) lists, counted from
    /// their manifests alone: it checks no Undo and asks the git index
    /// nothing.
    pub fn operation_count(&self) -> Result<usize, Error> {
        Ok(history::load_all(&self.ports)?.len())
    }

    /// Prunes History, as the front-ends do at startup: it keeps at least the
    /// last 50 Operations and every one from the last 30 days, never prunes
    /// one that needs attention, and removes an Operation with its backup.
    /// Returns the ids pruned, oldest first. It fails with `Locked` while
    /// another Operation holds the global lock.
    pub fn prune_history(&self) -> Result<Vec<String>, Error> {
        history::prune(&self.ports)
    }

    /// One Operation in History, with the edits it made, the files it
    /// copied to its backup and a masked diff of what it changed in each.
    pub fn history_show(&self, id: &str) -> Result<OperationDetail, Error> {
        history::detail(&self.definitions, &self.ports, id)
    }

    /// The copies of the file at `path` that History still holds, newest
    /// first, such as those a Read failure's detail links to.
    pub fn backups_of(&self, path: &Path) -> Result<Vec<Backup>, Error> {
        history::backups_of(&self.ports, path)
    }

    /// Dismisses an Operation that needs attention, once its files have been
    /// recovered by hand: it is no longer flagged, and pruning treats it like
    /// any other. It is refused for any other Operation.
    pub fn dismiss(&self, id: &str) -> Result<OperationSummary, Error> {
        history::dismiss(&self.ports, id)
    }

    /// Every loaded Agent definition, in load order, with the Agent homes
    /// `inventory` lists for it.
    pub fn agents(&self, inventory: &Inventory) -> Vec<Agent> {
        self.definitions
            .iter()
            .enumerate()
            .map(|(index, definition)| Agent {
                id: definition.agent.id.clone(),
                name: definition.agent.name.clone(),
                built_in: index < self.built_in,
                tested: definition.verified.as_ref().map(|verified| VersionRange {
                    min: verified.min.clone(),
                    max: verified.max.clone(),
                }),
                home_variables: definition
                    .homes
                    .iter()
                    .filter_map(|home| match home.folder.finds_by() {
                        FindsBy::Variable(variable) => Some(variable.to_owned()),
                        FindsBy::FixedPath | FindsBy::Search => None,
                    })
                    .collect(),
                homes: inventory
                    .homes
                    .iter()
                    .filter(|home| home.agent == definition.agent.id)
                    .cloned()
                    .collect(),
            })
            .collect()
    }

    /// Checks the existing folder `folder` as an Agent home of the Agent
    /// `agent`, as [`Core::add_home`] would add it, without adding it: which
    /// files the Agent reads there, and whether any of them is there.
    pub fn check_home(&self, folder: &Path, agent: &str) -> Result<HomeCheck, Error> {
        let (definition, folder) =
            user_list::home_to_add(&self.definitions, &self.ports, folder, agent)?;
        let home = definition.added_home();
        let configured = home.configured(&home.roots_in(&folder), self.ports.fs.as_ref());
        let looked_for: Vec<String> = home
            .own_roots()
            .map(|(_, root)| root.replace('/', "\\"))
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .collect();
        let no_configuration = (!configured)
            .then(|| explain::no_configuration_in(&definition.agent.name, &folder, &looked_for));
        Ok(HomeCheck {
            agent: definition.agent.id.clone(),
            agent_name: definition.agent.name.clone(),
            folder,
            looked_for,
            no_configuration,
        })
    }

    /// Checks the existing folder or `.code-workspace` file `path` as a
    /// Project, as [`Core::add_project`] would add it, without adding it:
    /// which Agents list an Agent home there, hidden or not.
    pub fn check_project(&self, path: &Path) -> Result<ProjectCheck, Error> {
        self.check_project_in(&self.listing()?, path)
    }

    /// Checks `path` as [`Core::check_project`] does, against `listing`, a
    /// scan read earlier, without reading the disk again.
    pub fn check_project_in(&self, listing: &Snapshot, path: &Path) -> Result<ProjectCheck, Error> {
        let path = user_list::project_to_add(&self.ports, path)?;
        let fs = self.ports.fs.as_ref();
        let key = paths::canonical_key(fs, &path);
        let mut home_of: Vec<String> = Vec::new();
        for home in self.listed(listing).homes {
            if paths::canonical_key(fs, &home.folder) == key && !home_of.contains(&home.agent_name)
            {
                home_of.push(home.agent_name);
            }
        }
        let warning = (!home_of.is_empty()).then(|| explain::project_is_home(&path, &home_of));
        Ok(ProjectCheck {
            path,
            home_of,
            warning,
        })
    }

    /// Adds the existing folder `folder` as an Agent home of the Agent
    /// `agent`, laid out like the Agent's first home, with no other check:
    /// a home with none of the files the Agent reads is listed as such.
    /// Returns the home as it is now listed.
    pub fn add_home(&self, folder: &Path, agent: &str) -> Result<Home, Error> {
        let (agent, folder) = self.add_home_to_list(folder, agent)?;
        self.home_in(
            &self.listing()?,
            &folder.display().to_string(),
            Some(&agent),
        )
    }

    /// Adds `folder` as an Agent home of `agent`, as [`Core::add_home`]
    /// does, without reading it. Returns the Agent's id and the folder's
    /// real path, by which [`Core::home_in`] finds it in a later scan.
    pub fn add_home_to_list(&self, folder: &Path, agent: &str) -> Result<(String, PathBuf), Error> {
        user_list::add_home(&self.definitions, &self.ports, folder, agent)
    }

    /// The one Agent home of `snapshot`, hidden or not, whose id or folder
    /// is `home`, among the homes of `agent` when it is given.
    pub fn home_in(
        &self,
        snapshot: &Snapshot,
        home: &str,
        agent: Option<&str>,
    ) -> Result<Home, Error> {
        self.find_home(&self.listed(snapshot), home, agent).cloned()
    }

    /// Hides the Agent home `home`, discovered or added: it is listed
    /// only when hidden ones are asked for. `home` is its id or its folder,
    /// and `agent` narrows it down to one Agent's homes.
    pub fn hide_home(&self, home: &str, agent: Option<&str>) -> Result<Home, Error> {
        self.hide_home_in(&self.listing()?, home, agent)
    }

    /// Hides the Agent home `home` as [`Core::hide_home`] does, looking for
    /// it in `listing`, a scan read earlier, without reading the disk again.
    pub fn hide_home_in(
        &self,
        listing: &Snapshot,
        home: &str,
        agent: Option<&str>,
    ) -> Result<Home, Error> {
        let found = self.home_in(listing, home, agent)?;
        user_list::hide_home(&self.ports, &found.agent, &found.home)?;
        Ok(Home {
            hidden: true,
            ..found
        })
    }

    /// Lists the hidden Agent home `home` again, as [`Core::hide_home`] names it.
    pub fn unhide_home(&self, home: &str, agent: Option<&str>) -> Result<Home, Error> {
        self.unhide_home_in(&self.listing()?, home, agent)
    }

    /// Lists the hidden Agent home `home` again, looking for it as
    /// [`Core::hide_home_in`] does.
    pub fn unhide_home_in(
        &self,
        listing: &Snapshot,
        home: &str,
        agent: Option<&str>,
    ) -> Result<Home, Error> {
        let found = self.home_in(listing, home, agent)?;
        if !found.hidden {
            return Err(Error::not_hidden(home));
        }
        user_list::unhide_home(&self.ports, &found.agent, &found.home)?;
        Ok(Home {
            hidden: false,
            ..found
        })
    }

    /// Adds the existing folder or `.code-workspace` file `path` as a
    /// Project. Returns the Project as it is now listed.
    pub fn add_project(&self, path: &Path) -> Result<Project, Error> {
        let path = self.add_project_to_list(path)?;
        self.project_in(&self.listing()?, &path)
    }

    /// Adds `path` as a Project, as [`Core::add_project`] does, without
    /// reading it. Returns its real path, by which [`Core::project_in`]
    /// finds it in a later scan.
    pub fn add_project_to_list(&self, path: &Path) -> Result<PathBuf, Error> {
        user_list::add_project(&self.ports, path)
    }

    /// The Project of `snapshot`, hidden or not, that `path` names, in any
    /// spelling.
    pub fn project_in(&self, snapshot: &Snapshot, path: &Path) -> Result<Project, Error> {
        self.find_project(&self.listed(snapshot), path).cloned()
    }

    /// Hides the Project `path`, discovered or added, in any spelling: it
    /// is listed only when hidden ones are asked for, and still evaluated
    /// when picked as the context.
    pub fn hide_project(&self, path: &Path) -> Result<Project, Error> {
        self.hide_project_in(&self.listing()?, path)
    }

    /// Hides the Project `path` as [`Core::hide_project`] does, looking for
    /// it in `listing`, a scan read earlier, without reading the disk again.
    pub fn hide_project_in(&self, listing: &Snapshot, path: &Path) -> Result<Project, Error> {
        let found = self.project_in(listing, path)?;
        user_list::hide_project(&self.ports, &found.path)?;
        Ok(Project {
            hidden: true,
            ..found
        })
    }

    /// Lists the hidden Project `path` again.
    pub fn unhide_project(&self, path: &Path) -> Result<Project, Error> {
        self.unhide_project_in(&self.listing()?, path)
    }

    /// Lists the hidden Project `path` again, looking for it as
    /// [`Core::hide_project_in`] does.
    pub fn unhide_project_in(&self, listing: &Snapshot, path: &Path) -> Result<Project, Error> {
        let found = self.project_in(listing, path)?;
        if !found.hidden {
            return Err(Error::not_hidden(&path.display().to_string()));
        }
        user_list::unhide_project(&self.ports, &found.path)?;
        Ok(Project {
            hidden: false,
            ..found
        })
    }

    /// Forgets the Agent home `home`, as [`Core::hide_home`] names it, that
    /// the user added. It is refused for a home found without the user,
    /// which only hiding takes off the list.
    pub fn forget_home(&self, home: &str, agent: Option<&str>) -> Result<Home, Error> {
        self.forget_home_in(&self.listing()?, home, agent)
    }

    /// Forgets the Agent home `home` as [`Core::forget_home`] does, looking
    /// for it as [`Core::hide_home_in`] does.
    pub fn forget_home_in(
        &self,
        listing: &Snapshot,
        home: &str,
        agent: Option<&str>,
    ) -> Result<Home, Error> {
        let found = self.home_in(listing, home, agent)?;
        if found.source != HomeSource::Added {
            return Err(Error::blocked(explain::forget_refused_home(
                &found.home,
                &found.agent_name,
            )));
        }
        user_list::forget_home(&self.ports, &found.agent, &found.home, &found.folder)?;
        Ok(found)
    }

    /// Forgets the Project `path` that the user added. It is refused for a
    /// Project an Agent's registry records, which only hiding takes off the
    /// list.
    pub fn forget_project(&self, path: &Path) -> Result<Project, Error> {
        self.forget_project_in(&self.listing()?, path)
    }

    /// Forgets the Project `path` as [`Core::forget_project`] does, looking
    /// for it as [`Core::hide_project_in`] does.
    pub fn forget_project_in(&self, listing: &Snapshot, path: &Path) -> Result<Project, Error> {
        let found = self.project_in(listing, path)?;
        if !found.registered_by.is_empty() {
            let names: Vec<&str> = found
                .registered_by
                .iter()
                .filter_map(|id| self.definitions.iter().find(|d| d.agent.id == *id))
                .map(|definition| definition.agent.name.as_str())
                .collect();
            return Err(Error::blocked(explain::forget_refused_project(
                &found.path,
                &names,
            )));
        }
        if !found.added {
            return Err(Error::no_project(path));
        }
        user_list::forget_project(&self.ports, &found.path)?;
        Ok(found)
    }

    /// The app's settings, with their defaults when none were saved.
    pub fn settings(&self) -> Result<Settings, Error> {
        settings::load(&self.ports)
    }

    /// Saves the app's settings. It writes no Agent configuration.
    pub fn save_settings(&self, settings: &Settings) -> Result<(), Error> {
        settings::save(&self.ports, settings)
    }

    /// When the app last checked for updates, in RFC 3339, `None` when it
    /// never did.
    pub fn last_update_check(&self) -> Result<Option<String>, Error> {
        updates::last(&self.ports)
    }

    /// Records that the app checks for updates now, whatever the check
    /// finds, and returns the time in RFC 3339. The check itself is the
    /// app's: the core makes no network call.
    pub fn record_update_check(&self) -> Result<String, Error> {
        updates::record(&self.ports)
    }

    /// Takes the global write lock for an update install, so that the
    /// install never starts while an Operation writes, and no Operation
    /// starts while the update downloads. It fails with `Locked` at once
    /// while another Operation holds it, and is `Blocked` while the CLI the
    /// installer replaces, `cli`, runs, since Windows would not let it
    /// replace a running executable.
    pub fn lock_for_update(&self, cli: &Path) -> Result<UpdateLock, Error> {
        let (_, lock) = history::create_and_lock(&self.ports)?;
        let key = paths::lexical_key(cli);
        let cli_runs = self.ports.processes.running().iter().any(|process| {
            process
                .path
                .as_ref()
                .is_some_and(|path| paths::lexical_key(path) == key)
        });
        if cli_runs {
            return Err(Error::blocked(explain::cli_running(cli)));
        }
        Ok(UpdateLock(lock))
    }

    /// The folders config-digger keeps its own data in. None of them has to
    /// exist yet.
    pub fn data_folders(&self) -> Result<DataFolders, Error> {
        DataFolders::locate(self.ports.env.as_ref())
    }

    /// Every Entry in config-digger's parked-Entry store, in the order of
    /// their names, read from the store alone: the uninstaller names them
    /// before it deletes config-digger's data. None when there is no store.
    pub fn stored_parked_entries(&self) -> Result<Vec<StoredParkedEntry>, Error> {
        let store = park::store_path(self.ports.env.as_ref())?;
        let unreadable = |error: &dyn std::fmt::Display| {
            Error::unexpected(format!("cannot read {}: {error}", store.display()))
        };
        let bytes = match self.ports.fs.read(&store) {
            Ok(bytes) => bytes,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
            Err(error) => return Err(unreadable(&error)),
        };
        let document: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|error| unreadable(&error))?;
        let parked_entries = park::parked_entries(&document)
            .map(|held| held.parked)
            .unwrap_or_default();
        let mut stored: Vec<StoredParkedEntry> = parked_entries
            .into_iter()
            .map(|(_, parked)| {
                let origin = parked.origin;
                let agent_name = self
                    .definitions
                    .agents
                    .iter()
                    .find(|definition| definition.agent.id == origin.agent)
                    .map_or(origin.agent, |definition| definition.agent.name.clone());
                StoredParkedEntry {
                    name: origin.name,
                    agent_name,
                }
            })
            .collect();
        stored.sort_by(|a, b| (&a.name, &a.agent_name).cmp(&(&b.name, &b.agent_name)));
        Ok(stored)
    }

    /// How long History keeps Operations. It is fixed.
    pub fn retention(&self) -> Retention {
        Retention {
            operations: u32::try_from(history::KEEP_OPERATIONS).expect("a small count"),
            days: u32::try_from(history::KEEP_FOR.as_secs() / (24 * 60 * 60)).expect("a few days"),
        }
    }

    /// Reads every Agent home and Project, to find the one a list change
    /// names.
    fn listing(&self) -> Result<Snapshot, Error> {
        self.scan(&ScanOptions::default())
    }

    /// Every Agent home and Project of `snapshot`, hidden ones included,
    /// with no Project as context.
    fn listed(&self, snapshot: &Snapshot) -> Inventory {
        self.resolve(snapshot, &Context::default())
    }

    /// The one home of `inventory` whose id or folder is `selector`, among
    /// the homes of `agent` when it is given.
    fn find_home<'i>(
        &self,
        inventory: &'i Inventory,
        selector: &str,
        agent: Option<&str>,
    ) -> Result<&'i Home, Error> {
        let fs = self.ports.fs.as_ref();
        let folder = paths::canonical_key(fs, Path::new(selector));
        let found: Vec<&Home> = inventory
            .homes
            .iter()
            .filter(|home| agent.is_none_or(|agent| home.agent.eq_ignore_ascii_case(agent)))
            .filter(|home| {
                home.home.eq_ignore_ascii_case(selector)
                    || paths::canonical_key(fs, &home.folder) == folder
            })
            .collect();
        match found.as_slice() {
            [home] => Ok(home),
            [] => Err(Error::no_home(selector)),
            homes => Err(Error::ambiguous_homes(selector, homes)),
        }
    }

    /// The Project of `inventory` that `path` names, in any spelling.
    fn find_project<'i>(
        &self,
        inventory: &'i Inventory,
        path: &Path,
    ) -> Result<&'i Project, Error> {
        let key = paths::canonical_key(self.ports.fs.as_ref(), path);
        inventory
            .projects
            .iter()
            .find(|project| paths::key(&project.path) == key)
            .ok_or_else(|| Error::no_project(path))
    }

    /// What the skill folder at `folder` holds, as a skill Entry of the
    /// Agent `agent`, in any case, shows it: each file's relative path and
    /// size, and the `name` and `description` of its `SKILL.md`
    /// frontmatter, read as the Agent's first skill location reads it, as
    /// every skill location of a built-in Agent does. File content is never
    /// shown, since a skill folder can hold Secrets, such as a `.env` file.
    pub fn skill_content(&self, folder: &Path, agent: &str) -> Result<SkillContent, Error> {
        let reading = user_list::definition_of(&self.definitions, agent)?
            .locations
            .iter()
            .find_map(|location| location.format.skill_folder())
            .map_or(SkillFallback::Skip, |format| format.malformed_frontmatter);
        skill::content(self.ports.fs.as_ref(), folder, reading)
    }

    /// How the skill folder at `after` differs from the one at `before`,
    /// such as a backed-up copy and the folder as it is now: each file
    /// added, removed or changed, with its sizes and never its content. A
    /// missing folder holds no files.
    pub fn compare_skill_folders(
        &self,
        before: &Path,
        after: &Path,
    ) -> Result<Vec<SkillFileChange>, Error> {
        skill::compare(self.ports.fs.as_ref(), before, after)
    }
}

/// A skill folder as config-digger shows it, without any file's content.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct SkillContent {
    /// The `name` of the `SKILL.md` frontmatter.
    pub name: Option<String>,
    /// The `description` of the `SKILL.md` frontmatter.
    pub description: Option<String>,
    /// Every file in the folder and below it, by path.
    pub files: Vec<SkillFile>,
}

/// One file of a skill folder.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct SkillFile {
    /// Relative to the skill folder, `/`-separated.
    pub path: String,
    /// In bytes.
    #[specta(type = f64)]
    pub size: u64,
}

/// A file that differs between two versions of a skill folder.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct SkillFileChange {
    /// Relative to the skill folder, `/`-separated.
    pub path: String,
    pub change: SkillFileChangeKind,
    /// In bytes, `None` when the file is added.
    #[specta(type = Option<f64>)]
    pub size_before: Option<u64>,
    /// In bytes, `None` when the file is removed.
    #[specta(type = Option<f64>)]
    pub size_after: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum SkillFileChangeKind {
    Added,
    Removed,
    /// Its content differs, which is never shown.
    Changed,
}

/// What an Operation does to the Entries it targets.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Action {
    /// Makes each Entry stop taking effect for one Agent while keeping it,
    /// through the Agent's own machine-local switch.
    Disable,
    /// Makes each Disabled Entry take effect again, lifting every switch
    /// that Disables it, whoever set it.
    Enable,
    /// Deletes each Entry from where it lives, for every Agent that reads it.
    Remove,
    /// Applies the inverse of every edit of an earlier Operation to the files
    /// as they are now.
    Undo,
    /// Deletes the backup of an earlier Operation, which can no longer be
    /// undone afterwards.
    DeleteBackup,
}

/// The Entries an action targets.
#[derive(Debug, Clone, Default)]
pub struct Targets {
    /// Each one an Item name, or the `<path>#<key path>` locator of an Entry.
    pub selectors: Vec<String>,
    /// Each selector stands for every Entry of the one Item it names, instead
    /// of the one Entry it names.
    pub all: bool,
    /// Narrows the Items each selector names, before it must name one Item
    /// or one Entry.
    pub narrowing: Narrowing,
}

/// What narrows the Items a selector names: to those of one Agent, of one
/// Agent home, or of both. The default narrows nothing; [`Core::narrowing`]
/// makes the others.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Narrowing {
    /// The id of an Agent definition.
    agent: Option<String>,
    /// The id of an Agent home, as [`Home::home`] has it.
    home: Option<String>,
}

impl Narrowing {
    /// Whether `item` is of the Agent and the Agent home this narrows to.
    pub(crate) fn admits(&self, item: &Item) -> bool {
        let agent = self.agent.as_ref();
        let home = self.home.as_ref();
        agent.is_none_or(|agent| item.agent.eq_ignore_ascii_case(agent))
            && home.is_none_or(|home| item.home.eq_ignore_ascii_case(home))
    }
}

/// An Operation planned and not yet executed. It never leaves the process:
/// front-ends show its preview and warnings, then hand it to [`Core::execute`].
pub struct Plan(pub(crate) plan::Planned);

impl Plan {
    pub fn preview(&self) -> &Preview {
        &self.0.preview
    }

    /// What the user must acknowledge, each on its own, before it executes.
    pub fn warnings(&self) -> &[Warning] {
        &self.0.warnings
    }

    /// Why it cannot execute, whatever is acknowledged. Empty when it can.
    pub fn blocks(&self) -> &[Block] {
        &self.0.blocks
    }
}

/// Why a plan cannot execute, whatever is acknowledged. The user removes
/// the cause, such as by closing an Agent, and plans again.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct Block {
    pub code: BlockCode,
    /// The cause and what to do about it, as a sentence rendered by the core.
    pub sentence: String,
    /// The same block laid out for the app.
    pub advice: Vec<Advice>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum BlockCode {
    /// The plan writes a SQLite store while the Agent holding it open runs.
    RunningAgentSqlite,
    /// The plan parks an Entry out of, or puts one back into, a file the
    /// running Agent rewrites from memory, which would undo it.
    RunningAgentParking,
}

/// What a plan will change, shown before it is confirmed.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct Preview {
    pub action: Action,
    /// What the Operation does, as a title such as "Remove 2 Entries".
    pub title: String,
    /// The Operation this one undoes, when it is an Undo.
    pub undo_of: Option<String>,
    /// The Operation whose backup it deletes, when it is a Delete backup.
    pub backup_of: Option<String>,
    /// The context's Project, as in [`Inventory::project`].
    pub project: Option<PathBuf>,
    /// One row per (Agent, Entry, Project) whose State the Operation changes.
    pub changes: Vec<Change>,
    /// For each Item that keeps Entries, which one is Effective afterwards.
    pub effective: Vec<EffectiveChange>,
    /// What to know before applying it, with nothing to acknowledge, such
    /// as a location that could change the outcome and cannot be read.
    pub info: Vec<Info>,
    pub files: Vec<FileChange>,
    /// The skill folders and links it moves, removes or puts back.
    pub folders: Vec<FolderChange>,
    /// The backed-up copies a Delete backup deletes for good.
    pub deleted: Vec<PathBuf>,
    /// Where each touched file is copied before anything is written, or for
    /// a Delete backup, the folder it deletes the copies from.
    pub backup_folder: PathBuf,
    /// The same, as a sentence.
    pub backup_note: String,
}

/// Something a preview says before an Operation is applied, which needs no
/// acknowledgement.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct Info {
    pub code: InfoCode,
    /// The location it is about: for a note about an Agent's versions, the
    /// Installation's executable, or else the Agent home's folder.
    pub path: PathBuf,
    /// What to know, as a sentence rendered by the core.
    pub sentence: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum InfoCode {
    /// A location that could change the States after the Operation is a
    /// Read failure, so they may differ from the prediction.
    ReadFailure,
    /// A file the Operation writes is a Read failure for an Agent, which
    /// still rejects it afterwards.
    StillRejected,
    /// An Installation of an Agent it touches is outside the definition's
    /// tested range, as [`AgentNoteCode::VersionOutsideTestedRange`] notes.
    VersionOutsideTestedRange,
    /// The version of an Installation of an Agent it touches cannot be read.
    VersionUnreadable,
    /// No Installation of an Agent it touches was found.
    NoInstallationFound,
    /// The definition of an Agent it touches has no tested range.
    DefinitionUntested,
    /// The Agent's approval of an Entry it Removes is left as it is:
    /// config-digger never edits approvals.
    ApprovalKept,
    /// It targets Entries of a Mirrored home, and changes the Entries they
    /// were copied from instead: the change reaches the Mirrored home through
    /// the tool that copies it. Its path is the Mirrored home's folder.
    ReachesMirroredHome,
    /// It Removes a Stranded Entry, which has no State to change. Its path
    /// is the parked-Entry store.
    RemovesStranded,
    /// It writes or Removes a server's table in a user file the Agent reads
    /// in every folder, for a server of a Project's layer, such as a
    /// Stand-in: what that does outside the context's Project. Its path is
    /// that file.
    ReachesOtherFolders,
}

impl AgentNoteCode {
    /// What a preview says of it, `None` for a note about something else
    /// than the Agent's versions, which no Operation changes.
    pub(crate) fn info(self) -> Option<InfoCode> {
        match self {
            AgentNoteCode::VersionOutsideTestedRange => Some(InfoCode::VersionOutsideTestedRange),
            AgentNoteCode::VersionUnreadable => Some(InfoCode::VersionUnreadable),
            AgentNoteCode::NoInstallationFound => Some(InfoCode::NoInstallationFound),
            AgentNoteCode::DefinitionUntested => Some(InfoCode::DefinitionUntested),
            AgentNoteCode::CloudConnectorsNotOnDisk
            | AgentNoteCode::MarketplacePluginsNotOnDisk
            | AgentNoteCode::LastRanUnreadable => None,
        }
    }
}

/// An Entry's State in one Agent and context, before and after an Operation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct Change {
    pub agent: String,
    pub agent_name: String,
    pub home: String,
    /// The Item's name.
    pub item: String,
    pub path: PathBuf,
    pub locator: String,
    pub project: Option<PathBuf>,
    pub before: StateAfter,
    pub after: StateAfter,
}

/// An Entry's State before or after an Operation, or that it does not exist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum StateAfter {
    Effective,
    Shadowed,
    Merged,
    Disabled,
    Blocked,
    Invalid,
    Unread,
    Removed,
}

impl From<State> for StateAfter {
    fn from(state: State) -> Self {
        match state {
            State::Effective => StateAfter::Effective,
            State::Shadowed => StateAfter::Shadowed,
            State::Merged => StateAfter::Merged,
            State::Disabled => StateAfter::Disabled,
            State::Blocked => StateAfter::Blocked,
            State::Invalid => StateAfter::Invalid,
            State::Unread => StateAfter::Unread,
        }
    }
}

/// Which Entry of an Item is Effective after an Operation that leaves some of
/// its Entries in place.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct EffectiveChange {
    pub agent: String,
    pub home: String,
    pub item: String,
    /// The locator of the Effective Entry before, `None` when none was.
    pub before: Option<String>,
    /// The locator of the Effective Entry after, `None` when none is.
    pub after: Option<String>,
    /// The change as a sentence, rendered by the core.
    pub sentence: String,
    /// For an Item whose Agent builds a server from several same-named
    /// Entries on either side, the server it builds before: the one built
    /// from them, or the Effective Entry's alone. `None` when it uses none.
    pub built_before: Option<BuiltServer>,
    /// The same, after.
    pub built_after: Option<BuiltServer>,
}

/// The edits to one file and their diff.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct FileChange {
    pub path: PathBuf,
    /// The file is in the index of its git repo, so the edits show in the
    /// git diff.
    pub git_versioned: bool,
    /// How the file behaves while an Agent that reads it runs.
    pub store_class: StoreClass,
    pub edits: Vec<EditSummary>,
    /// A unified diff of the whole file, before and after every edit, as
    /// [`FileDiff::diff`] has it.
    pub diff: Option<String>,
    pub diff_unavailable: Option<DiffUnavailable>,
    pub diff_note: Option<String>,
    pub changed_lines: u32,
}

impl FileChange {
    pub(crate) fn new(
        edits: Vec<EditSummary>,
        diff: FileDiff,
        git_versioned: bool,
        store_class: StoreClass,
    ) -> Self {
        Self {
            path: diff.path,
            git_versioned,
            store_class,
            edits,
            diff: diff.diff,
            diff_unavailable: diff.diff_unavailable,
            diff_note: diff.diff_note,
            changed_lines: diff.changed_lines,
        }
    }
}

/// How a file an Operation writes behaves while an Agent that reads it
/// runs, which decides whether that write warns or is blocked then.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum StoreClass {
    /// A file the Agent reads and leaves alone.
    Plain,
    /// A file the Agent rewrites from memory while it runs, such as
    /// `~/.claude.json`.
    RewrittenWhileRunning,
    /// A SQLite database the Agent holds open.
    Sqlite,
}

impl From<definitions::blocks::StoreClass> for StoreClass {
    fn from(class: definitions::blocks::StoreClass) -> Self {
        match class {
            definitions::blocks::StoreClass::Plain => StoreClass::Plain,
            definitions::blocks::StoreClass::RewrittenWhileRunning => {
                StoreClass::RewrittenWhileRunning
            }
            definitions::blocks::StoreClass::Sqlite => StoreClass::Sqlite,
        }
    }
}

/// What an Operation does at one skill folder's location. A folder is
/// moved whole, never copied, so the file count and size say what moves.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct FolderChange {
    pub path: PathBuf,
    pub change: FolderChangeKind,
    /// Where the link points, for a link removed or created.
    pub target: Option<PathBuf>,
    /// How many files the folder moved holds, or the link's target.
    #[specta(type = f64)]
    pub files: u64,
    /// Their total size in bytes.
    #[specta(type = f64)]
    pub size: u64,
    /// The folder is in the index of its git repo, so the change shows in
    /// the git diff.
    pub git_versioned: bool,
    /// The change as a sentence, rendered by the core.
    pub sentence: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum FolderChangeKind {
    /// The skill folder is moved into the Operation's backup.
    MovedToBackup,
    /// The link is removed, and its target left as it is.
    LinkRemoved,
    /// The skill folder is moved back from an earlier Operation's backup.
    MovedBack,
    /// The link is created again.
    LinkCreated,
    /// The skill folder is moved into config-digger's parked-Entry store.
    Parked,
    /// The skill folder is moved back from config-digger's parked-Entry
    /// store.
    PutBack,
}

/// A diff of two versions of one file, with every Secret masked in both,
/// or why it cannot be shown.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct FileDiff {
    pub path: PathBuf,
    /// A unified diff, computed on the real text and shown with every Secret
    /// masked, context lines and other Entries included. A line whose only
    /// change is a Secret shows the same on both sides and is followed by
    /// the line `\ secret value changed`. `None` when it cannot be shown.
    pub diff: Option<String>,
    /// Why there is no diff.
    pub diff_unavailable: Option<DiffUnavailable>,
    /// Why there is no diff, as a sentence rendered by the core.
    pub diff_note: Option<String>,
    /// How many lines differ, whether or not the diff can be shown.
    pub changed_lines: u32,
}

/// Why a diff cannot be shown with its Secrets masked.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum DiffUnavailable {
    /// One side does not parse in the dialect of an Agent that reads the file.
    Parse,
    /// One side is not UTF-8 text.
    Decode,
    /// One side cannot be read, such as a backup copy that is gone.
    Io,
    /// No Agent reads Entries from the file, such as one that only holds
    /// Disable switches, or none any more, so nothing says where its
    /// Secrets are.
    NoReader,
    /// The backup of the Operation was deleted, with the values of its edits.
    BackupDeleted,
    /// The edits History holds no longer apply to the backup, so what the
    /// Operation wrote cannot be rebuilt from it.
    EditsDoNotApply,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct EditSummary {
    pub op: EditOp,
    pub key_path: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum EditOp {
    Remove,
    Set,
    Insert,
    Append,
    RemoveItem,
}

impl From<&Edit> for EditSummary {
    fn from(edit: &Edit) -> Self {
        let op = match edit {
            Edit::Remove { .. } => EditOp::Remove,
            Edit::Set { .. } => EditOp::Set,
            Edit::Insert { .. } => EditOp::Insert,
            Edit::Append { .. } => EditOp::Append,
            Edit::RemoveItem { .. } => EditOp::RemoveItem,
        };
        EditSummary {
            op,
            key_path: edit.key_path().to_vec(),
        }
    }
}

/// A risk the user must acknowledge before a plan executes.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct Warning {
    pub class: WarningClass,
    /// The risk as a sentence, rendered by the core.
    pub sentence: String,
    /// The same risk laid out for the app, one per cause it covers.
    pub advice: Vec<Advice>,
}

/// A warning or block laid out for the app's action dialog, rendered by the
/// core: what to do on a line of its own, then why, then the paths it is
/// about, each on a line of its own.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct Advice {
    /// What to do, such as "Close Claude Code first to be safe."
    pub todo: String,
    /// Why, the paths left out, such as "Claude Code is running and
    /// rewrites this location while it runs: …".
    pub why: String,
    /// What `why` is about, each in full on a line of its own: the path of
    /// a location, or an Entry's locator with where it applies.
    pub paths: Vec<String>,
}

/// Each class of warning is acknowledged on its own.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "kebab-case")]
pub enum WarningClass {
    /// The write lands in a git-versioned file.
    GitVersioned,
    /// The write reaches more than the targeted Entries.
    WideReach,
    /// An Agent that reads or rewrites the file is running.
    RunningAgent,
    /// The backup a Delete backup deletes is the only copy for the manual
    /// recovery of an Operation that needs attention.
    NeedsAttention,
    /// A skill folder moved into the backup is the target of Linked Entries
    /// the Operation leaves broken.
    BreaksLinks,
}

impl WarningClass {
    /// The class as it appears in JSON and in `--ack`.
    pub fn code(self) -> &'static str {
        match self {
            WarningClass::GitVersioned => "git-versioned",
            WarningClass::WideReach => "wide-reach",
            WarningClass::RunningAgent => "running-agent",
            WarningClass::NeedsAttention => "needs-attention",
            WarningClass::BreaksLinks => "breaks-links",
        }
    }
}

/// An executed Operation.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct Outcome {
    /// The Operation's id in History, such as `20261001-143012-a3f`.
    pub operation: String,
    pub verify: Verify,
    /// Where the States after differ from the preview's prediction.
    pub differences: Vec<Difference>,
    /// The folder holding the Operation's backup and manifest.
    pub backup: PathBuf,
    /// What the Operation did, as a sentence rendered by the core.
    pub sentence: String,
    /// What to do next, such as restarting a running Agent that reads a
    /// written file, as sentences rendered by the core.
    pub hints: Vec<String>,
}

/// What Verify found once every edit had landed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Verify {
    /// Every State after is the one the preview predicted.
    Verified,
    /// Some States after differ from the prediction, as listed.
    VerifiedWithDifferences,
}

/// An Entry whose State after an Operation is not the one predicted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct Difference {
    pub agent: String,
    pub home: String,
    pub locator: String,
    pub predicted: StateAfter,
    pub actual: StateAfter,
}

/// Where an Operation got to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum OperationStatus {
    /// Backed up and being written, or interrupted while it was.
    InProgress,
    Applied,
    /// Failed, and rolled back: no file holds any of its edits.
    Failed,
    /// Failed, and its Rollback could not restore every file, so its backup
    /// is needed to recover. Its backup is kept until it is dismissed.
    NeedsAttention,
}

/// Where a failed Operation's Rollback left one file it touched.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
pub struct RollbackLocation {
    pub path: PathBuf,
    pub outcome: RollbackOutcome,
    /// Why it was left alone or not restored, as a sentence.
    pub reason: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum RollbackOutcome {
    /// It holds the bytes it held before the Operation again, or is gone
    /// again when the Operation created it.
    Restored,
    /// The Operation never wrote it.
    NotWritten,
    /// It changed after config-digger wrote it, and holds none of the
    /// Operation's edits any more, so it is left as it is.
    LeftAlone,
    /// It still holds some of the Operation's edits and could not be
    /// restored. It needs manual recovery from the backup.
    NotRestored,
}

/// One Operation as History lists it.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct OperationSummary {
    /// Such as `20261001-143012-a3f`.
    pub id: String,
    /// When it started, in RFC 3339.
    pub created_at: String,
    pub action: Action,
    /// What it does, as its preview titled it.
    pub title: String,
    pub status: OperationStatus,
    /// The status as History shows it, such as "Applied", rendered by the core.
    pub status_text: String,
    /// Why it failed, as a sentence, when it did.
    pub cause: Option<String>,
    /// Its Rollback is incomplete and it was not dismissed yet.
    pub needs_attention: bool,
    /// The Operation this one undoes, when it is an Undo.
    pub undo_of: Option<String>,
    /// The Undo of this Operation, while that Undo is in effect.
    pub undone_by: Option<String>,
    /// Its backup was deleted, and with it the values of its edits.
    pub backup_deleted: bool,
    /// Its backup can be deleted: it was not yet, or a deletion left copies
    /// of it behind.
    pub backup_deletable: bool,
    /// The names of the Agents whose States it changed.
    pub agents: Vec<String>,
    /// The names of the Entries whose States it changed: the names of
    /// their Items.
    pub entries: Vec<String>,
    /// The files it touched.
    pub locations: Vec<PathBuf>,
    pub undoable: bool,
    /// Why it cannot be undone, as a sentence, when it cannot.
    pub not_undoable: Option<String>,
}

/// One Operation as `history show` details it.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct OperationDetail {
    pub summary: OperationSummary,
    /// The context's Project, as in [`Preview::project`].
    pub project: Option<PathBuf>,
    /// The States its preview predicted.
    pub changes: Vec<Change>,
    pub locations: Vec<OperationLocation>,
    /// The folder holding its backup and manifest.
    pub backup: PathBuf,
    /// Why it did not complete.
    pub cause: Option<String>,
    /// Where its Rollback left each file it touched, when it failed.
    pub rollback: Vec<RollbackLocation>,
    pub verify: Option<Verify>,
    /// Where the States after differed from the prediction.
    pub differences: Vec<Difference>,
    /// For an Operation History offers to undo, the error planning its Undo
    /// fails with now because of its files, as [`Core::plan_undo`] returns
    /// it: an `undo_refused` one carries a diff from each backed-up file to
    /// what it holds now. `None` when its files allow the Undo.
    pub undo_refusal: Option<Error>,
}

/// One file an Operation touched.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct OperationLocation {
    pub path: PathBuf,
    /// Its copy as it was before the Operation, or `None` when the Operation
    /// created the file or its backup was deleted.
    pub backup: Option<PathBuf>,
    pub edits: Vec<EditSummary>,
    /// What the Operation changed in the file: a diff from its backup to
    /// what its edits made of it, with every Secret masked. `None` for a
    /// skill folder or link.
    pub diff: Option<FileDiff>,
}

/// A copy of one file that an Operation in History backed up.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct Backup {
    /// The id of the Operation.
    pub operation: String,
    /// When the Operation started, in RFC 3339.
    pub created_at: String,
    /// What the Operation does, as its preview titled it.
    pub title: String,
    /// The copy, as the file was before the Operation.
    pub copy: PathBuf,
}

/// The global write lock, held while an update installs, released when
/// dropped.
pub struct UpdateLock(#[allow(dead_code)] crate::ports::HeldLock);

/// The app's settings. The CLI never reads them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
pub struct Settings {
    /// The app checks for updates when it starts. Off by default: until
    /// then, it makes no network call unless asked to.
    pub check_updates_on_startup: bool,
}

/// The folders config-digger keeps its own data in.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
pub struct DataFolders {
    /// Each Operation's backup and manifest, which History lists.
    pub backups: PathBuf,
    /// The parked-Entry store, which is never pruned.
    pub parked: PathBuf,
    /// The app's rotating logs.
    pub logs: PathBuf,
    /// The Agent homes and Projects the user added or hid, and the app's
    /// settings.
    pub lists: PathBuf,
}

impl DataFolders {
    /// Where the folders are in `env`, as [`Core::data_folders`] finds them,
    /// without loading any Agent definition.
    pub fn locate(env: &dyn Environment) -> Result<Self, Error> {
        let data = history::data_folder(env)?;
        let parked = park::store_path(env)?;
        Ok(DataFolders {
            backups: history::backups_folder(&data),
            parked: parked
                .parent()
                .expect("the manifest is in the store's folder")
                .to_path_buf(),
            logs: data.join("logs"),
            lists: user_list::data_folder(env)?,
        })
    }
}

/// An Entry in config-digger's parked-Entry store, as the uninstaller names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredParkedEntry {
    pub name: String,
    /// Its Agent's name, or the Agent's id when no loaded definition has it.
    pub agent_name: String,
}

/// How long History keeps Operations: at least the last `operations`, and
/// every one from the last `days`. Operations that need attention are
/// never pruned, and neither is the parked-Entry store.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
pub struct Retention {
    pub operations: u32,
    pub days: u32,
}

/// Every Item found, with the Agent homes they belong to.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct Inventory {
    /// When the snapshot was read, in RFC 3339.
    pub read_at: String,
    /// The context's Project, as found by the scan, or `None` for no Project.
    pub project: Option<PathBuf>,
    /// Every Project the scan found or was asked for, sorted by path.
    pub projects: Vec<Project>,
    pub homes: Vec<Home>,
    /// Sorted by Agent, home, Item kind and name.
    pub items: Vec<Item>,
    /// Every Read failure the scan met, in any context.
    pub read_failures: Vec<ReadFailure>,
    /// The Read failures of config-digger's parked-Entry store, which
    /// belongs to no Agent.
    pub store_read_failures: Vec<StoreReadFailure>,
    /// The problems of Agent homes in the context, such as one that cannot start.
    pub agent_flags: Vec<AgentFlag>,
}

impl Inventory {
    /// The Items a selector names among those `narrowing` leaves: those
    /// with an Entry at a `<path>#<key path>` locator, or else those with
    /// that name.
    pub fn select(&self, selector: &str, narrowing: &Narrowing) -> Vec<&Item> {
        select::select(self, selector, narrowing)
    }

    /// The one Item a selector names among those `narrowing` leaves, as
    /// [`Core::show`] finds it.
    pub fn select_one(&self, selector: &str, narrowing: &Narrowing) -> Result<&Item, Error> {
        match self.select(selector, narrowing).as_slice() {
            [item] => Ok(item),
            [] => Err(Error::no_match(selector)),
            items => Err(Error::ambiguous(selector, items, self)),
        }
    }

    /// The inventory as it is listed by default: without the Agent homes
    /// the user hid, their Items, Read failures and flags, nor the Projects
    /// the user hid. The context stays as it was resolved, its Entries
    /// included, even when its Project is hidden, since it was picked.
    pub fn without_hidden(mut self) -> Inventory {
        let hidden: Vec<(String, String)> = self
            .homes
            .iter()
            .filter(|home| home.hidden)
            .map(|home| (home.agent.clone(), home.home.clone()))
            .collect();
        let is_hidden = |agent: &str, home: &str| {
            hidden
                .iter()
                .any(|(hidden_agent, hidden_home)| hidden_agent == agent && hidden_home == home)
        };
        self.homes.retain(|home| !home.hidden);
        self.items
            .retain(|item| !is_hidden(&item.agent, &item.home));
        self.read_failures
            .retain(|failure| !is_hidden(&failure.agent, &failure.home));
        self.agent_flags
            .retain(|flag| !is_hidden(&flag.agent, &flag.home));
        self.projects.retain(|project| !project.hidden);
        self
    }

    /// `leftovers` as they are listed by default: without those of the
    /// missing Projects and the Agent homes the user hid, as this
    /// inventory, with hidden ones included, marks them.
    pub fn leftovers_without_hidden(&self, mut leftovers: Vec<Leftovers>) -> Vec<Leftovers> {
        let hidden_home = |item: &Item| {
            self.homes
                .iter()
                .any(|home| home.hidden && home.agent == item.agent && home.home == item.home)
        };
        leftovers.retain(|leftovers| {
            !self
                .projects
                .iter()
                .any(|project| project.hidden && project.path == leftovers.project)
        });
        for leftovers in &mut leftovers {
            leftovers.items.retain(|item| !hidden_home(item));
        }
        leftovers.retain(|leftovers| !leftovers.items.is_empty());
        leftovers
    }

    /// The inventory without the missing Projects that have none of
    /// `leftovers`, which only Hide can act on: Claude Code keeps a key for
    /// every folder it was ever started on. A hidden Project stays, listed
    /// with the hidden ones. The context stays as it was resolved, even when
    /// its Project is one of them.
    pub fn without_missing_lacking_leftovers(mut self, leftovers: &[Leftovers]) -> Inventory {
        self.projects.retain(|project| {
            !project.missing
                || project.hidden
                || leftovers
                    .iter()
                    .any(|leftovers| leftovers.project == project.path)
        });
        self
    }
}

/// What an Agent can be started on.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct Project {
    /// The real path of the folder or `.code-workspace` file, or its lexical
    /// normalisation when it is missing.
    pub path: PathBuf,
    pub missing: bool,
    /// It is a `.code-workspace` file rather than a folder.
    pub workspace_file: bool,
    /// For a workspace file, the folders it opens, in the order it lists them.
    pub members: Vec<PathBuf>,
    /// The Agents whose registries record it. Empty when it was only added
    /// or asked for.
    pub registered_by: Vec<String>,
    /// The user added it.
    pub added: bool,
    /// The user hid it, so it is listed only when hidden ones are asked for.
    pub hidden: bool,
    /// How many Entries apply in it alone: in files inside it, or keyed to
    /// its path in user files. Each counts once, however many Agents read it.
    pub entries: u32,
    /// Its Last-opened home for each Agent that records one, in definition
    /// order. Nothing it says changes a State.
    pub last_opened: Vec<LastOpened>,
}

/// The Agent home an Agent last opened a Project in, as the Agent itself
/// records it: its Last-opened home.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct LastOpened {
    pub agent: String,
    /// The home's id.
    pub home: String,
    /// The name the Agent gives where it opened the Project, when it is not
    /// the home's id, such as a VS Code profile's.
    pub name: Option<String>,
    /// Where it opened the Project is no home of its own, and reads the MCP
    /// servers of `home`, such as a VS Code profile that shares the default
    /// profile's.
    pub shares_home: bool,
    /// The user hid the home.
    pub hidden: bool,
    /// Where the Agent last opened the Project, as a sentence, rendered by
    /// the core.
    pub sentence: String,
}

/// The leftovers of one missing Project: its Unread Entries.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct Leftovers {
    /// The missing Project, as [`Project::path`] has it.
    pub project: PathBuf,
    /// Each Item with only its Unread Entries, sorted like [`Inventory::items`].
    pub items: Vec<Item>,
}

impl Leftovers {
    /// Every leftover, Item by Item.
    pub fn entries(&self) -> impl Iterator<Item = &Entry> {
        self.items.iter().flat_map(|item| &item.entries)
    }
}

/// A parked Entry whose origin config-digger cannot find (ADR 0002). It has
/// no State and belongs to no Item, so only its locator in the store names
/// it, and it can only be Removed. It is Parked again once its origin is
/// found.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct Stranded {
    pub name: String,
    pub kind: ItemKind,
    /// The id of the Agent it was parked from.
    pub agent: String,
    /// That Agent's name, or its id when no loaded definition is of it.
    pub agent_name: String,
    /// Where it was parked from.
    pub origin: StrandedOrigin,
    /// The first part of its origin that is not found.
    pub reason: StrandedReason,
    /// The reason as a sentence naming what is not found, rendered by the core.
    pub sentence: String,
    /// When it was parked, in RFC 3339.
    pub parked_at: String,
    /// The parked-Entry store, and its key path there.
    pub path: PathBuf,
    pub key_path: Vec<String>,
    /// `<path>#<key path>` of where it is in the store.
    pub locator: String,
    /// Its value as parked, with every Secret masked as `••••`: as its
    /// location masks it when the location is loaded, and else every text.
    #[specta(type = JsonValue)]
    pub config: serde_json::Value,
    /// The key paths in `config`, relative to the Entry, whose values were
    /// masked.
    pub masked: Vec<Vec<String>>,
}

/// Where a Stranded Entry was parked from.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct StrandedOrigin {
    /// The id of the Agent home.
    pub home: String,
    /// The id of the location in the Agent's definition.
    pub location: String,
    /// The Project it applied in, `None` when it applied in every context.
    pub project: Option<PathBuf>,
    /// The file or skill folder it was parked from, and its key path there.
    pub path: PathBuf,
    pub key_path: Vec<String>,
    /// `<path>#<key path>` of where it was.
    pub locator: String,
}

/// Which part of a Stranded Entry's origin is not found, the first that
/// applies in this order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum StrandedReason {
    /// No loaded definition is of its Agent.
    NoDefinition,
    /// Its Agent home is not found.
    HomeNotFound,
    /// Its location id is no longer in its Agent's definition.
    LocationGone,
}

/// A Read failure of config-digger's parked-Entry store: its manifest
/// cannot be opened or parsed, or an Entry in it does not read as a parked
/// Entry. config-digger never writes to the store because of one: the user
/// fixes the file by hand.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct StoreReadFailure {
    /// The store's manifest.
    pub path: PathBuf,
    /// The id of the Entry that does not read as a parked Entry, `None`
    /// when the whole manifest fails.
    pub entry: Option<String>,
    pub kind: ReadFailureKind,
    /// Where a parse error is, when the parser knows. It never quotes the
    /// manifest, which can hold a Secret.
    pub position: Option<Position>,
    /// The failure as a sentence, rendered by the core.
    pub sentence: String,
}

/// A loaded Agent definition and the Agent homes listed for it.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct Agent {
    pub id: String,
    pub name: String,
    /// The definition ships with config-digger, rather than being loaded from a file.
    pub built_in: bool,
    /// The versions the definition was tested with, `None` when it is untested.
    pub tested: Option<VersionRange>,
    /// The environment variables the definition finds Agent homes through,
    /// such as `CODEX_HOME`, in definition order.
    pub home_variables: Vec<String>,
    pub homes: Vec<Home>,
}

/// An Agent home found on this machine, or added by the user.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct Home {
    pub agent: String,
    pub agent_name: String,
    /// The home's id: the definition's name for it, or its folder when the
    /// user added it.
    pub home: String,
    /// The folder its configuration is in.
    pub folder: PathBuf,
    pub source: HomeSource,
    /// The user hid it, so it is listed only when hidden ones are asked for.
    pub hidden: bool,
    /// When the Agent's lookup order skips it, so that the Agent never
    /// reads it and its Entries are Unread, what it reads instead.
    pub not_read: Option<NotRead>,
    /// When another tool copies it from another Agent home of the Agent,
    /// which it is: config-digger never writes to it.
    pub mirror: Option<Mirror>,
    /// When none of the files the Agent reads there exists, the sentence
    /// saying so, rendered by the core.
    pub no_configuration: Option<String>,
    /// The Agent's Scopes, in display order.
    pub scopes: Vec<String>,
    /// Each Installation that reads it, with its version as read at this scan.
    pub installations: Vec<Installation>,
    /// The version of an Installation that reads it is outside the
    /// definition's tested range.
    pub out_of_range: bool,
    /// The version that last ran against it, as the Agent records it.
    pub last_ran: Option<String>,
    /// How many Entries it holds with no Project: those the inventory lists
    /// for it with no Project picked, each once. Its Project Entries count
    /// on their Projects.
    pub entries: u32,
    /// What to know about the Agent's versions here, such as one outside
    /// the definition's tested range. None of them changes a State or
    /// blocks a write.
    pub notes: Vec<AgentNote>,
}

/// A folder checked as an Agent home of one Agent, before it is added.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct HomeCheck {
    pub agent: String,
    pub agent_name: String,
    /// The folder's real path.
    pub folder: PathBuf,
    /// Each file and folder the Agent reads in a home, other than those
    /// other Agents read too, relative to the folder, in path order.
    pub looked_for: Vec<String>,
    /// When none of `looked_for` is there, the sentence saying so, rendered
    /// by the core. The folder can still be added.
    pub no_configuration: Option<String>,
}

/// A folder or `.code-workspace` file checked as a Project, before it is added.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct ProjectCheck {
    /// Its real path.
    pub path: PathBuf,
    /// The name of each Agent with an Agent home listed at it, hidden or
    /// not, in the order the homes are listed.
    pub home_of: Vec<String>,
    /// When there is one, the sentence saying so, rendered by the core. It
    /// can still be added.
    pub warning: Option<String>,
}

/// Why the Agent never reads an Agent home: its lookup order picks another.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct NotRead {
    /// The id of the home the order picks, `None` when it picks none that is there.
    pub read_instead: Option<String>,
    /// The reason as a sentence, rendered by the core.
    pub sentence: String,
}

/// An Agent home another tool copies from a source Agent home, such as the
/// Codex home Orca keeps in sync with `~\.codex`. It is read-only.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct Mirror {
    /// The tool that keeps the copy, such as `Orca`.
    pub owner: String,
    /// The id of the Agent home it copies.
    pub source: String,
    /// That home's folder, `None` when the environment cannot say where it is.
    pub source_folder: Option<PathBuf>,
    /// Such as "Mirrored by Orca", rendered by the core.
    pub label: String,
    /// What it is and why it is read-only, rendered by the core.
    pub sentence: String,
}

/// A launchable copy of an Agent, with its own version.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct Installation {
    /// The id of the definition's installation, such as `native`.
    pub id: String,
    /// Its executable.
    pub path: PathBuf,
    /// `None` when it cannot be read.
    pub version: Option<String>,
    /// The file or registry key the version was read from.
    pub version_from: Option<String>,
    /// Its version is outside the definition's tested range.
    pub out_of_range: bool,
}

/// Something about an Agent in one Agent home, such as its versions or what
/// it keeps only in the user's account, which changes no State and blocks
/// nothing.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct AgentNote {
    /// What it notes, from a closed set.
    pub code: AgentNoteCode,
    /// The executable of the Installation it is about, if it is about one.
    pub installation: Option<PathBuf>,
    /// The note as a sentence, rendered by the core.
    pub sentence: String,
}

/// What an Agent note is about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum AgentNoteCode {
    /// An Installation's version is newer or older than the definition's
    /// tested range.
    VersionOutsideTestedRange,
    /// An Installation's version cannot be read.
    VersionUnreadable,
    /// No Installation that reads the Agent home was found.
    NoInstallationFound,
    /// The definition has no tested range.
    DefinitionUntested,
    /// The Agent's connectors live in the user's account, with no copy on
    /// disk, so they are not listed.
    CloudConnectorsNotOnDisk,
    /// The plugins the user installs for themself from the Agent's
    /// marketplace live in the user's account, with no install record on
    /// disk, so they are not listed.
    MarketplacePluginsNotOnDisk,
    /// The file the Agent records the version that last ran in exists but
    /// cannot be used, so that version is not shown.
    LastRanUnreadable,
}

/// The versions an Agent definition was tested with, both ends included.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct VersionRange {
    /// The lowest version tested, as the definition spells it.
    pub min: String,
    /// The highest version tested, as the definition spells it.
    pub max: String,
}

/// Where an Agent home came from.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum HomeSource {
    /// The location the Agent definition gives as its default.
    Default,
    /// The environment variable that holds its folder.
    Environment {
        /// The variable's name, such as `CODEX_HOME`.
        variable: String,
    },
    /// Another location the Agent definition knows of, such as an MSIX
    /// package's data folder.
    KnownLocation,
    /// The user added it, and no definition location finds it.
    Added,
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct Item {
    pub agent: String,
    pub home: String,
    pub kind: ItemKind,
    pub name: String,
    pub state: State,
    /// In precedence order, highest first.
    pub entries: Vec<Entry>,
    /// For an Item whose Agent combines same-named Entries field by field,
    /// the server it builds, when more than one Entry takes part.
    pub built: Option<BuiltServer>,
    /// What makes its State less certain, such as a location that could
    /// take part and could not be read.
    pub caveats: Vec<Caveat>,
}

/// The server an Agent builds by merging same-named Entries field by field,
/// as Codex does across its layers.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct BuiltServer {
    /// The server, with every Secret masked as `••••`.
    #[specta(type = JsonValue)]
    pub config: serde_json::Value,
    /// The key paths in `config` whose values were masked.
    pub masked: Vec<Vec<String>>,
    /// Where each field of `config` comes from, in its order: each value
    /// that is not an object, and each empty object.
    pub origins: Vec<FieldOrigin>,
}

/// The Entry one field of a built server comes from.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct FieldOrigin {
    /// The field's key path in the server. An array item is not a field of
    /// its own: an array comes whole from one Entry.
    pub key_path: Vec<String>,
    /// The `<path>#<key path>` locator of the Entry.
    pub entry: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    Mcp,
    Skill,
}

impl From<DefinitionItemKind> for ItemKind {
    fn from(kind: DefinitionItemKind) -> Self {
        match kind {
            DefinitionItemKind::Mcp => ItemKind::Mcp,
            DefinitionItemKind::Skill => ItemKind::Skill,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct Entry {
    pub path: PathBuf,
    pub key_path: Vec<String>,
    /// `<path>#<key path>`.
    pub locator: String,
    pub scope: String,
    pub provenance: Provenance,
    /// For `skill_manager` Provenance, the skill manager that installed it.
    pub skill_manager: Option<SkillManager>,
    /// For `plugin` Provenance, the plugin it is part of.
    pub plugin: Option<Plugin>,
    /// Its file is in the index of its git repo, so a change to it shows in
    /// the git diff and reaches every clone once committed.
    pub git_versioned: bool,
    /// For a Linked Entry, a junction or symlink at its own location, where
    /// it points. Removing it removes the link, never its target.
    pub link: Option<Link>,
    /// For a parked Entry, which config-digger moved into its parked-Entry
    /// store, where it came from. Its `path` and `key_path` are where it is
    /// in the store.
    pub parked: Option<Parked>,
    pub state: State,
    /// Why the Entry has its State, when it is not Effective: the first of
    /// the Agent's checks that it fails.
    pub reason: Option<Reason>,
    /// The other checks it fails, in the Agent's order.
    pub secondary: Vec<Reason>,
    /// For a Disabled Entry, the State it would have once Enabled.
    pub what_if: Option<WhatIf>,
    /// Partial switches that limit part of it, leaving its State as it is.
    pub notes: Vec<Note>,
    /// The Entry's value as declared, with every Secret masked as `••••`.
    #[specta(type = JsonValue)]
    pub config: serde_json::Value,
    /// The key paths in `config`, relative to the Entry, whose values were
    /// masked, so that a mask can be told from a literal. An array item's
    /// key is its index.
    pub masked: Vec<Vec<String>>,
    /// What Disable would do to it in this context.
    pub disable: DisableOption,
    /// For a Disabled Entry, what Enable writes, or why it is not offered.
    pub enable: Option<EnableOption>,
    /// Whether Remove is offered for it.
    pub remove: RemoveOption,
    /// What makes its State less certain, such as a Disable switch that
    /// cannot be read.
    pub caveats: Vec<Caveat>,
    /// Every Agent home of another Agent that reads the same Entry, such as
    /// Claude Code's for a `.mcp.json` VS Code reads too, with the State it has
    /// there in the same context, sorted by Agent and home.
    pub also_read_by: Vec<AlsoReadBy>,
    /// For an Entry of a Mirrored home, the `<path>#<key path>` locator of the
    /// Entry it was copied from, which its actions go to: they are those of
    /// that Entry. `None` otherwise, and when the home it is copied from
    /// lacks it.
    pub copied_from: Option<String>,
}

/// Another Agent home that reads an Entry, and the State the Entry has there.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct AlsoReadBy {
    /// The other Agent's id, such as `claude-code`.
    pub agent: String,
    pub agent_name: String,
    /// The id of the Agent home that reads it.
    pub home: String,
    /// The Entry's State there, in the same context.
    pub state: State,
    /// Such as "Also read by Claude Code (home default): Effective.",
    /// rendered by the core.
    pub sentence: String,
}

/// Where a parked Entry came from.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct Parked {
    /// The file it was in, and its key path there: where Enable puts it back.
    pub path: PathBuf,
    pub key_path: Vec<String>,
    /// `<path>#<key path>` of where it was, which a preview names it by.
    pub locator: String,
    /// When it was parked, in RFC 3339.
    pub parked_at: String,
}

impl Entry {
    /// The locator a preview names it by: where it was, for a parked Entry,
    /// so that it keeps one name in and out of the store.
    pub fn identity(&self) -> &str {
        self.parked
            .as_ref()
            .map_or(&self.locator, |parked| &parked.locator)
    }

    /// The file of the locator a preview names it by, as [`identity`](Self::identity) has it.
    pub fn identity_path(&self) -> &Path {
        self.parked
            .as_ref()
            .map_or(&self.path, |parked| &parked.path)
    }
}

/// Where the junction or symlink of a Linked Entry points.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct Link {
    /// The path the link holds, made absolute.
    pub target: PathBuf,
    /// A junction, a folder symlink or a file symlink: what Undo of its
    /// Remove creates again.
    pub kind: LinkKind,
    /// The target is gone, so the Agent reads nothing there and the Entry
    /// is Unread.
    pub broken: bool,
}

/// What kind of link a Linked Entry's location is, which Undo of its
/// Remove creates again (ADR 0008).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum LinkKind {
    /// A Windows junction, which points to a folder by its absolute path.
    Junction,
    /// A symlink to a folder, its target absolute or relative.
    FolderSymlink,
    /// A symlink to a file, its target absolute or relative.
    FileSymlink,
}

impl LinkKind {
    pub fn is_symlink(self) -> bool {
        self != Self::Junction
    }
}

/// The State a Disabled Entry would have once Enabled.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct WhatIf {
    pub state: State,
    /// The locator of the Entry that would beat it, if one would.
    pub effective: Option<String>,
    /// Such as "Disabled: would be Shadowed by …", rendered by the core.
    pub sentence: String,
}

/// Something about an Entry that does not change its State.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct Note {
    pub code: NoteCode,
    /// The `<path>#<key path>` locator of the switch it comes from, if one does.
    pub switch: Option<String>,
    /// The note as a sentence, rendered by the core.
    pub sentence: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum NoteCode {
    /// A partial switch limits part of the Entry, and the Agent still uses it.
    PartialSwitch,
    /// The Agent loads the skill through a fallback, under its folder's name.
    SkillFallback,
    /// The skill manager is recognised only by the folder's name or where it
    /// is, which a folder made by hand there matches too.
    ProvenanceByNameOnly,
    /// The Agent uses the Entry only when it starts in a folder the user
    /// trusts, which config-digger cannot tell with no Project.
    UntrustedFolderBlocks,
    /// The Agent's own command that lists servers shows the server built
    /// from this Entry and another as that other alone, hiding this one.
    CliViewDiffers,
    /// The Entry is a Stand-in, written to Disable the server of a Project's
    /// layer, and the context is no Project it stands in for: here it is a
    /// Disabled server of its own, which only Remove takes away.
    StandIn,
}

/// What Disable would do to an Entry in its context: write the Agent's own
/// switch for it, or nothing, with the reason.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum DisableOption {
    /// Disable writes the switch of the Agent's most specific mechanism that
    /// covers the Entry in the context, the first in its order of preference
    /// among equally specific ones, whose file is machine-local, not
    /// git-versioned, and readable.
    Offered {
        /// The file the switch is in.
        path: PathBuf,
        /// What Disable writes, as a sentence rendered by the core.
        sentence: String,
    },
    /// Disable is not offered.
    NotOffered {
        code: DisableNotOffered,
        /// The reason as a sentence, rendered by the core.
        sentence: String,
    },
}

impl DisableOption {
    /// What Disable writes, or why it is not offered.
    pub fn sentence(&self) -> &str {
        match self {
            DisableOption::Offered { sentence, .. }
            | DisableOption::NotOffered { sentence, .. } => sentence,
        }
    }
}

/// What Enable would do to a Disabled Entry: lift what Disables it, or
/// nothing, with the reason.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum EnableOption {
    /// Enable lifts every switch that Disables the Entry, or puts a parked
    /// Entry back.
    Offered {
        /// What Enable writes, as a sentence rendered by the core.
        sentence: String,
    },
    /// Enable is not offered: a rule, or a switch config-digger only reads,
    /// keeps the Entry out, or config-digger only reads the Entry.
    NotOffered {
        /// The reason as a sentence, rendered by the core.
        sentence: String,
    },
}

impl EnableOption {
    /// What Enable writes, or why it is not offered.
    pub fn sentence(&self) -> &str {
        match self {
            EnableOption::Offered { sentence } | EnableOption::NotOffered { sentence } => sentence,
        }
    }
}

/// Why Disable is not offered for an Entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum DisableNotOffered {
    /// Every switch of the Agent that could Disable the Entry is in a
    /// git-versioned file, where a change reaches every clone.
    GitVersioned,
    /// The Agent has no switch that Disables the Entry in this context.
    NoMechanism,
    /// Every switch of the Agent that could Disable the Entry is in a file
    /// config-digger cannot read, so it cannot be written safely.
    UnreadableSwitch,
    /// The Entry is Disabled already.
    AlreadyDisabled,
    /// The Entry is Unread: the Agent never reads it, so it can only be
    /// Removed.
    Unread,
    /// The Entry is managed, or a copy of the user's account, and
    /// config-digger only reads it.
    ReadOnly,
    /// The Entry is in a Mirrored home, which config-digger never writes
    /// to, and the Agent home it is copied from holds no Entry it was
    /// copied from to act on instead.
    Mirrored,
    /// Managed policy forces the Entry on or off, and config-digger never
    /// overrides it.
    Policy,
    /// Its only write, a Stand-in in a user file, would stop the Agent from
    /// starting in a known Project that declares the server with another
    /// transport.
    StopsAgent,
}

/// Whether Remove is offered for an Entry, and if not, why, and what to
/// do instead.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum RemoveOption {
    /// Remove deletes the Entry from where it lives, for every Agent that
    /// reads it.
    Offered,
    /// Remove is not offered.
    NotOffered {
        code: RemoveNotOffered,
        /// The reason as a sentence, rendered by the core.
        sentence: String,
        /// How to remove it some other way, such as through the Agent.
        how_to_remove: Option<HowToRemove>,
    },
}

/// Why Remove is not offered for an Entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum RemoveNotOffered {
    /// The Entry is managed, or a copy of the user's account, and
    /// config-digger only reads it.
    ReadOnly,
    /// The Entry is in a Mirrored home, which config-digger never writes
    /// to, and the Agent home it is copied from holds no Entry it was
    /// copied from to act on instead.
    Mirrored,
    /// The Entry is part of a plugin, which only the Agent uninstalls.
    Plugin,
    /// The Entry is an extension's, which only the Agent uninstalls.
    Extension,
}

/// How to remove an Entry that config-digger does not Remove.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct HowToRemove {
    /// The Agent's own command that removes it.
    pub command: String,
    /// What to do, as a sentence rendered by the core.
    pub sentence: String,
}

/// How an Entry got where it is, which decides how it can be taken away.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum Provenance {
    /// A key in a configuration file, or a skill folder someone put there.
    File,
    /// Configuration an organisation deploys to the machine, which
    /// config-digger only reads.
    Managed,
    /// A copy the Agent keeps of configuration in the user's account, such as
    /// skills synced from claude.ai, which config-digger only reads.
    CloudMirror,
    /// A skill folder a skill manager installed, as its definition
    /// recognises it. Remove touches that folder or link alone.
    SkillManager,
    /// Part of a plugin the Agent installed. Disable takes the whole plugin
    /// through its own switch, and Remove is not offered: the Agent
    /// uninstalls it.
    Plugin,
    /// An extension the Agent installed, such as a Claude Desktop `.mcpb`.
    /// Disable goes through its own switch, and Remove is not offered: the
    /// Agent uninstalls it.
    Extension,
    /// Shipped with the Agent, which restores it as it ships it, such as
    /// Cursor's built-in skills. config-digger only reads it.
    BuiltIn,
    /// Installed and switched on or off through the user's account, such
    /// as the plugins Codex installs from the account's catalog.
    /// config-digger only reads it.
    AccountManaged,
}

/// A skill manager, such as `npx skills`, as its definition names it.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct SkillManager {
    pub id: String,
    pub name: String,
}

/// A plugin or extension an Agent installed, which an Entry is part of.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct Plugin {
    /// The id the Agent knows it by, such as `supabase@claude-plugins-official`.
    pub id: String,
    /// The name its Entries are named by, such as `supabase`.
    pub name: String,
    /// The Scope it is installed at, as the Agent records it.
    pub scope: Option<String>,
    /// Its version, as the Agent records it.
    pub version: Option<String>,
    /// The folder its files are in.
    pub folder: PathBuf,
}

impl From<DefinitionProvenance> for Provenance {
    fn from(provenance: DefinitionProvenance) -> Self {
        match provenance {
            DefinitionProvenance::File => Provenance::File,
            DefinitionProvenance::Managed => Provenance::Managed,
            DefinitionProvenance::CloudMirror => Provenance::CloudMirror,
            DefinitionProvenance::Plugin => Provenance::Plugin,
            DefinitionProvenance::Extension => Provenance::Extension,
            DefinitionProvenance::BuiltIn => Provenance::BuiltIn,
            DefinitionProvenance::AccountManaged => Provenance::AccountManaged,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum State {
    Effective,
    Shadowed,
    /// The Agent builds the server it uses by combining same-named Entries
    /// field by field, and this one supplies some of its fields, so that no
    /// single Entry is Effective.
    Merged,
    /// The Agent skips it because a switch Disables it, natively or set by
    /// config-digger.
    Disabled,
    /// The Agent will not use it because of a gate outside it that Enable
    /// cannot lift, such as managed policy or a server not yet approved.
    /// config-digger never lifts these gates.
    Blocked,
    /// The Agent rejects it on its own, such as a server missing what the
    /// Agent requires.
    Invalid,
    /// The Agent never reads it in any context, such as an Entry keyed to
    /// a missing Project. It competes with nothing and can only be Removed.
    Unread,
}

/// Why an Entry has its State.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct Reason {
    pub code: ReasonCode,
    /// The locator of the Effective Entry that beats this one, if one does.
    pub effective: Option<String>,
    /// The `<path>#<key path>` locator of the switch that Disables it, if one does.
    pub switch: Option<String>,
    /// The reason as a sentence, rendered by the core.
    pub sentence: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ReasonCode {
    /// A same-named Entry at a location the Agent ranks higher is Effective.
    ShadowedByPrecedence,
    /// The Agent builds the server it uses from this Entry and other
    /// same-named ones, field by field.
    Merged,
    /// The server the Agent builds from the Item's Entries holds fields of
    /// two transports, such as Codex's `command` and `url`, which it
    /// rejects, refusing to start.
    TransportConflict,
    /// A switch names the Item, so the Agent skips every Entry of it that
    /// the switch covers.
    DisabledByName,
    /// The Entry's own switch is off.
    DisabledNatively,
    /// The switch of the plugin the Entry is part of is off, so the Agent
    /// skips every Entry of the plugin.
    PluginDisabled,
    /// A rule in a settings file the user writes, such as a deny list,
    /// leaves the Entry out. config-digger never lifts it.
    DisabledByRule,
    /// A setting that turns off every Entry it covers is off. config-digger
    /// never lifts it.
    DisabledGlobally,
    /// Managed policy keeps the Agent from using the Entry.
    Policy,
    /// The Agent has not approved the Entry, which it asks the user to do.
    NotApproved,
    /// The user has not trusted the folder of the Entry's Project, which
    /// the Agent needs before it uses the Entry.
    UntrustedFolder,
    /// The Entry is in a source of settings the Agent skips, since it reads
    /// only the first that holds any.
    SourceSkipped,
    /// The Entry is in a source of settings the user can write, which the
    /// Agent never reads the Entry's location from.
    UserWritableSource,
    /// The Entry is in a system file the Agent skips unread, since someone
    /// other than an administrator can change it or a folder above it.
    SystemFileNotAdminOwned,
    /// The Entry lacks the minimal shape the Agent requires of it.
    MalformedEntry,
    /// The Agent skips the skill because of its `SKILL.md` or its folder's name.
    MalformedSkill,
    /// The Agent refuses the plugin the Entry is part of, since another of
    /// its plugins has the same name.
    DuplicatePluginName,
    /// The Agent does not load the plugin the Entry is part of, since it
    /// cannot read a file of the plugin, such as a manifest that does not
    /// parse.
    PluginFailedToLoad,
    /// config-digger parked the Entry in its parked-Entry store, out of the
    /// Agent's reach, since the Agent has no switch for it. Enable puts it
    /// back.
    Parked,
    /// The Entry is keyed to a Project whose folder no longer exists.
    ProjectMissing,
    /// The Entry is in an Agent home the Agent's lookup order skips for
    /// another, so the Agent never reads it.
    LookupOrderSkipsHome,
    /// The Entry is a link whose target is gone, so the Agent reads nothing
    /// there.
    LinkBroken,
    /// The Agent reads the Entry's location only while a Feature gate has
    /// the other value from the one it is resolved with.
    FeatureGate,
    /// The Agent reads the Entry's location only while one of its settings
    /// has another value than it has in the context, such as VS Code's
    /// discovery of another app's servers.
    SettingValue,
    /// The Entry is in the folder of another account than the one the
    /// Agent home is signed in to, which alone the Agent reads.
    OtherAccount,
    /// The Agent reads the Entry's location only for the account it is
    /// signed in to, and the Agent home is signed in to none.
    NotSignedIn,
    /// The Agent reads the Entry's location only while it runs on the
    /// account it is signed in to, and a setting takes it off that account
    /// in the context, such as an API key.
    OffAccount,
}

/// A location config-digger could not turn into Entries for one Agent.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct ReadFailure {
    pub agent: String,
    pub agent_name: String,
    pub home: String,
    pub path: PathBuf,
    /// The Project whose folder holds the location, or `None` for a
    /// location outside any Project, which reaches every context. For a
    /// shape, the Project it is keyed to, if any.
    pub project: Option<PathBuf>,
    /// For a shape, the location it fails, the other locations in the file
    /// being read; `None` when the whole file fails.
    pub location: Option<FailedLocation>,
    pub kind: ReadFailureKind,
    /// Where a parse error is, when the parser knows. A Read failure never
    /// quotes the file, which can hold a Secret.
    pub position: Option<Position>,
    /// What the Agent does about it, as its definition says.
    pub reaction: ReadFailureReaction,
    /// The failure as a sentence, rendered by the core.
    pub sentence: String,
    /// What else to know, such as that the Agent asks about the file at
    /// startup, as sentences rendered by the core.
    pub notes: Vec<String>,
    /// How the Agent's own recovery may lose configuration if the file is
    /// not fixed before it starts.
    pub warnings: Vec<RiskWarning>,
}

/// The one location of a file that a shape Read failure fails.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, specta::Type)]
pub struct FailedLocation {
    /// The id of the location in the Agent's definition.
    pub id: String,
    /// The key path of the value it holds where its Entries are.
    pub key_path: Vec<String>,
}

/// What an Agent does with a location it cannot read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ReadFailureReaction {
    /// It keeps what parses on both sides of an error, and the location
    /// contributes the Entries config-digger recovers there.
    ReadsAroundError,
    /// It reads the next file of the plugin's list in its place, such as
    /// the next manifest of a Cursor plugin, and the file contributes no
    /// Entries.
    ReadsNextPath,
    /// It starts without the location's Entries.
    IgnoresFile,
    /// It asks about the file at startup, and otherwise starts without its Entries.
    AsksUser,
    /// What it does is not known, so every Item of the location's kinds in
    /// the contexts it reaches gets a caveat.
    Unknown,
    /// It does not load the plugin the location is in, so every Entry of
    /// the plugin is Invalid.
    FailsPlugin,
    /// It refuses to start in the contexts the location reaches, which
    /// flags it as unable to start there.
    StopsAgent,
}

impl From<definitions::ReadFailureReaction> for ReadFailureReaction {
    fn from(reaction: definitions::ReadFailureReaction) -> Self {
        match reaction {
            definitions::ReadFailureReaction::ReadsAroundError => {
                ReadFailureReaction::ReadsAroundError
            }
            definitions::ReadFailureReaction::ReadsNextPath => ReadFailureReaction::ReadsNextPath,
            definitions::ReadFailureReaction::IgnoresFile => ReadFailureReaction::IgnoresFile,
            definitions::ReadFailureReaction::AsksUser => ReadFailureReaction::AsksUser,
            definitions::ReadFailureReaction::Unknown => ReadFailureReaction::Unknown,
            definitions::ReadFailureReaction::FailsPlugin => ReadFailureReaction::FailsPlugin,
            definitions::ReadFailureReaction::StopsAgent => ReadFailureReaction::StopsAgent,
        }
    }
}

/// A way the Agent's own recovery from a Read failure can lose configuration.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct RiskWarning {
    pub code: RiskCode,
    /// The risk as a sentence, rendered by the core.
    pub sentence: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
pub enum RiskCode {
    /// The Agent may offer to reset the file, losing what it holds.
    #[serde(rename = "agent_may_reset_file")]
    ResetFile,
    /// The Agent may write the file again from empty, dropping its Entries.
    #[serde(rename = "agent_may_drop_entries")]
    DropEntries,
    /// The Agent may clear the sign-in state of servers that vanish.
    #[serde(rename = "agent_may_clear_auth")]
    ClearAuth,
}

impl From<definitions::RiskCode> for RiskCode {
    fn from(code: definitions::RiskCode) -> Self {
        match code {
            definitions::RiskCode::ResetFile => RiskCode::ResetFile,
            definitions::RiskCode::DropEntries => RiskCode::DropEntries,
            definitions::RiskCode::ClearAuth => RiskCode::ClearAuth,
        }
    }
}

/// A problem of an Agent home in the context, shown above its Items.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct AgentFlag {
    pub agent: String,
    pub agent_name: String,
    pub home: String,
    pub code: AgentFlagCode,
    /// The location that causes it.
    pub path: PathBuf,
    /// The problem as a sentence, rendered by the core.
    pub sentence: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum AgentFlagCode {
    /// The Agent refuses to start in the context.
    CannotStart,
}

/// Something that makes a State less certain than it looks.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct Caveat {
    pub code: CaveatCode,
    /// The location it is about.
    pub path: PathBuf,
    /// The caveat as a sentence, rendered by the core.
    pub sentence: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum CaveatCode {
    /// A location that could take part in the context is a Read failure,
    /// and what the Agent does about it is not known.
    ReadFailure,
    /// A file holding a Disable switch that could apply is a Read failure,
    /// so the Entry is resolved as if the switch were not set.
    SwitchReadFailure,
    /// The Entry depends on a Feature gate, resolved with the value the
    /// Agent last cached, as of when it cached it.
    GateCached,
    /// The Entry depends on a Feature gate whose value cannot be read, or
    /// that the Agent ignores its cache for, so it is resolved with the
    /// Agent's default.
    GateAssumed,
    /// A settings file holding a rule or gate that could apply cannot be
    /// read, so the Entry is resolved as if it held none.
    SettingReadFailure,
    /// The Entry competes with others by URL, and servers config-digger
    /// cannot see take part too, so one of them may win instead.
    UnseenServers,
    /// The Entry depends on a Feature gate the Agent fetches from the
    /// vendor's side and caches nowhere to read, such as an organisation's
    /// controls, which may turn it off where config-digger cannot see.
    GateMayApply,
    /// The Entry is read from a copy of configuration held elsewhere, such as
    /// in the user's account, as of when the Agent cached it.
    CachedCopy,
    /// The Agent reads the Entry only while it runs on the account it is
    /// signed in to, and a variable set where config-digger cannot tell
    /// whether the Agent applies it may take it off that account.
    MayRunOffAccount,
}

/// A place in a file's text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
pub struct Position {
    /// 1-based.
    pub line: u32,
    /// 1-based, in characters.
    pub column: u32,
}

impl ReadFailure {
    /// Whether `other` is the same reader failing on the same location,
    /// whatever the kind of failure.
    pub(crate) fn same_reader_and_location(&self, other: &ReadFailure) -> bool {
        self.agent == other.agent
            && self.home == other.home
            && self.path == other.path
            && self.location == other.location
    }

    /// Whether it reaches the context of `project`, or of no Project: one
    /// outside any Project reaches every context.
    pub(crate) fn reaches(&self, project: Option<&Path>) -> bool {
        self.project.is_none() || self.project.as_deref() == project
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ReadFailureKind {
    /// The text does not parse in the reader's dialect.
    Parse,
    /// Where a location's Entries are, the file holds a value of a shape
    /// config-digger cannot enumerate, such as a list where servers
    /// should be. Only that location fails, or every location in the file
    /// when the file itself is not an object; either way the Agent does
    /// what the locations say of a shape, not of a file that does not parse.
    Shape,
    /// The bytes do not decode into text the way the Agent decodes them.
    Decode,
    /// The file cannot be opened for lack of permission.
    Permission,
    /// The file cannot be opened or read for another reason.
    Io,
    /// A SQLite store cannot be opened or queried.
    Sqlite,
}

impl ReadFailureKind {
    /// The kind as it appears in JSON.
    pub fn code(self) -> &'static str {
        match self {
            ReadFailureKind::Parse => "parse",
            ReadFailureKind::Shape => "shape",
            ReadFailureKind::Decode => "decode",
            ReadFailureKind::Permission => "permission",
            ReadFailureKind::Io => "io",
            ReadFailureKind::Sqlite => "sqlite",
        }
    }
}

/// A failure of the call itself, as opposed to a Read failure.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type, thiserror::Error)]
#[error("{message}")]
pub struct Error {
    pub code: ErrorCode,
    pub message: String,
    /// What an ambiguous selector could mean, one per Item or Entry: its
    /// locator, followed by `--agent` and `--home` for an Item whose locator
    /// alone selects another Item too.
    pub candidates: Vec<String>,
    /// The backup folder of a failed Operation, or of one whose Undo was
    /// refused.
    pub backup: Option<PathBuf>,
    /// For a refused Undo, or a Rollback that is incomplete, a diff from
    /// each backed-up file to what it holds now, with every Secret masked.
    pub diffs: Vec<FileDiff>,
    /// The Operation that failed, once it was recorded in History.
    pub failed: Option<Box<FailedOperation>>,
}

/// An Operation that failed after it was recorded in History.
#[derive(Debug, Clone, PartialEq, Serialize, specta::Type)]
pub struct FailedOperation {
    /// Its id in History.
    pub operation: String,
    /// For `rollback_incomplete`, the code of the failure that started the
    /// Rollback.
    pub failure: Option<ErrorCode>,
    /// Where its Rollback left each file it touched.
    pub rollback: Vec<RollbackLocation>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    Unexpected,
    /// A selector matched nothing in the context.
    NoMatch,
    /// A selector matched more than one Item, or more than one Entry.
    Ambiguous,
    /// A warning in the plan was not acknowledged.
    AckRequired,
    /// The action cannot be done on a target, whatever is acknowledged.
    Blocked,
    /// Another Operation holds the global write lock.
    Locked,
    /// A touched Entry changed after it was read: plan again.
    Stale,
    /// A backup copy does not hold the bytes read; nothing was written.
    BackupFailed,
    /// A write failed; the Operation was rolled back.
    WriteFailed,
    /// An edit did not land; the Operation was rolled back.
    VerifyFailed,
    /// An Agent rewrote a file after config-digger wrote it, taking an edit
    /// away; the Operation was rolled back.
    OverwrittenByAgent,
    /// A file no longer parses for a reader that could read it before; the
    /// Operation was rolled back.
    ReadFailureIntroduced,
    /// The Operation failed and its Rollback could not restore every file,
    /// so it needs manual recovery from its backup.
    RollbackIncomplete,
    /// What the Operation to undo touched changed since it ran.
    UndoRefused,
    /// A path given to add is not an existing folder, or `.code-workspace`
    /// file for a Project.
    InvalidPath,
}

impl ErrorCode {
    /// The code as it appears in JSON.
    pub fn code(self) -> &'static str {
        match self {
            ErrorCode::Unexpected => "unexpected",
            ErrorCode::NoMatch => "no_match",
            ErrorCode::Ambiguous => "ambiguous",
            ErrorCode::AckRequired => "ack_required",
            ErrorCode::Blocked => "blocked",
            ErrorCode::Locked => "locked",
            ErrorCode::Stale => "stale",
            ErrorCode::BackupFailed => "backup_failed",
            ErrorCode::WriteFailed => "write_failed",
            ErrorCode::VerifyFailed => "verify_failed",
            ErrorCode::OverwrittenByAgent => "overwritten_by_agent",
            ErrorCode::ReadFailureIntroduced => "read_failure_introduced",
            ErrorCode::RollbackIncomplete => "rollback_incomplete",
            ErrorCode::UndoRefused => "undo_refused",
            ErrorCode::InvalidPath => "invalid_path",
        }
    }
}

impl Error {
    fn new(code: ErrorCode, message: String) -> Self {
        Self {
            code,
            message,
            candidates: Vec::new(),
            backup: None,
            diffs: Vec::new(),
            failed: None,
        }
    }

    /// A failure the caller cannot act on, such as a bug or a lost worker thread.
    pub fn unexpected(error: impl std::fmt::Display) -> Self {
        Self::new(ErrorCode::Unexpected, error.to_string())
    }

    /// No loaded Agent definition has the id `agent`.
    pub(crate) fn no_agent(agent: &str) -> Self {
        Self::new(
            ErrorCode::NoMatch,
            format!("no Agent `{agent}`; `config-digger agents` lists them"),
        )
    }

    /// A path given to add is not what it must be, as `message` says.
    pub(crate) fn invalid_path(message: String) -> Self {
        Self::new(ErrorCode::InvalidPath, message)
    }

    /// No Agent home has the id or folder `selector`.
    pub(crate) fn no_home(selector: &str) -> Self {
        Self::new(
            ErrorCode::NoMatch,
            format!(
                "no Agent home `{selector}`; `config-digger homes list --include-hidden` lists them"
            ),
        )
    }

    /// Each candidate is a home's Agent and id.
    pub(crate) fn ambiguous_homes(selector: &str, homes: &[&Home]) -> Self {
        Self {
            candidates: homes
                .iter()
                .map(|home| format!("{} --agent {}", home.home, home.agent))
                .collect(),
            ..Self::new(
                ErrorCode::Ambiguous,
                format!(
                    "`{selector}` matches {} Agent homes; name the Agent with --agent",
                    homes.len()
                ),
            )
        }
    }

    /// No Project is at `path`.
    pub(crate) fn no_project(path: &Path) -> Self {
        Self::new(
            ErrorCode::NoMatch,
            format!(
                "no Project {}; `config-digger projects list --include-hidden` lists them",
                path.display()
            ),
        )
    }

    /// `project` is not a missing Project with leftovers.
    pub(crate) fn no_leftovers(project: &Path) -> Self {
        Self::new(
            ErrorCode::NoMatch,
            format!(
                "{} is not a missing Project with leftovers; `config-digger list --leftovers` lists them",
                project.display()
            ),
        )
    }

    /// No parked Entry is Stranded, so there is none to Remove.
    pub(crate) fn none_stranded() -> Self {
        Self::new(
            ErrorCode::NoMatch,
            "no parked Entry is Stranded; `config-digger list --leftovers` lists them".to_owned(),
        )
    }

    /// `what`, an Agent home or a Project, is not hidden.
    pub(crate) fn not_hidden(what: &str) -> Self {
        Self::new(ErrorCode::NoMatch, format!("{what} is not hidden"))
    }

    /// A selector that matched no Item in the context.
    pub fn no_match(selector: &str) -> Self {
        Self::new(
            ErrorCode::NoMatch,
            format!("no Item matches `{selector}` in this context"),
        )
    }

    /// Each candidate is the locator of an Item's first Entry, followed by
    /// the Item's Agent and home as flags when that locator also selects
    /// another Item of `inventory`, so that each candidate selects its Item
    /// alone.
    pub(crate) fn ambiguous(selector: &str, items: &[&Item], inventory: &Inventory) -> Self {
        let mut shared = false;
        let candidates: Vec<String> = items
            .iter()
            .map(|item| {
                let locator = &item.entries[0].locator;
                if inventory.select(locator, &Narrowing::default()).len() > 1 {
                    shared = true;
                    format!("{locator} --agent {} --home {}", item.agent, item.home)
                } else {
                    locator.clone()
                }
            })
            .collect();
        let how = if shared {
            "select one by locator, with --agent and --home where Items share one"
        } else {
            "select one by locator"
        };
        Self {
            candidates,
            ..Self::new(
                ErrorCode::Ambiguous,
                format!("`{selector}` matches {} Items; {how}", items.len()),
            )
        }
    }

    /// Each candidate is the locator of an Entry.
    pub(crate) fn ambiguous_entries(selector: &str, entries: &[&Entry]) -> Self {
        Self {
            candidates: entries.iter().map(|entry| entry.locator.clone()).collect(),
            ..Self::new(
                ErrorCode::Ambiguous,
                format!(
                    "`{selector}` matches {} Entries; select one by locator, or every Entry of the Item",
                    entries.len()
                ),
            )
        }
    }

    pub(crate) fn ack_required(missing: &[WarningClass]) -> Self {
        let classes: Vec<&str> = missing.iter().map(|class| class.code()).collect();
        Self::new(
            ErrorCode::AckRequired,
            format!("not acknowledged: {}", classes.join(", ")),
        )
    }

    pub(crate) fn blocked(message: String) -> Self {
        Self::new(ErrorCode::Blocked, message)
    }

    pub(crate) fn locked(lock: &std::path::Path) -> Self {
        Self::new(
            ErrorCode::Locked,
            format!(
                "another config-digger Operation is writing (lock held on {}); try again when it ends",
                lock.display()
            ),
        )
    }

    pub(crate) fn stale(message: String) -> Self {
        Self::new(ErrorCode::Stale, format!("{message}; plan again"))
    }

    pub(crate) fn backup_failed(message: String) -> Self {
        Self::new(
            ErrorCode::BackupFailed,
            format!("{message}; nothing was written"),
        )
    }

    /// No Operation in History has the id `id`.
    pub(crate) fn no_operation(id: &str) -> Self {
        Self::new(
            ErrorCode::NoMatch,
            format!("no Operation `{id}` in History"),
        )
    }

    pub(crate) fn undo_refused(message: String, backup: PathBuf, diffs: Vec<FileDiff>) -> Self {
        Self {
            message: Self::undo_refused_message(&message, &backup),
            backup: Some(backup),
            diffs,
            ..Self::new(ErrorCode::UndoRefused, String::new())
        }
    }

    /// Why an Undo is refused, as [`Error::undo_refused`] says it.
    pub(crate) fn undo_refused_message(message: &str, backup: &Path) -> String {
        format!(
            "{message}, so the Undo is refused; the files as they were before the Operation are in {}",
            backup.display()
        )
    }

    /// The Operation `operation` failed with `cause`, and its Rollback left
    /// each file as `rollback` says. `diffs` go from each backed-up file it
    /// could not restore to what that file holds now.
    pub(crate) fn rolled_back(
        cause: ErrorCode,
        message: String,
        operation: String,
        backup: PathBuf,
        rollback: Vec<RollbackLocation>,
        diffs: Vec<FileDiff>,
    ) -> Self {
        let not_restored = rollback
            .iter()
            .filter(|location| location.outcome == RollbackOutcome::NotRestored)
            .count();
        let complete = not_restored == 0;
        let failure = Self {
            backup: Some(backup.clone()),
            failed: Some(Box::new(FailedOperation {
                operation: operation.clone(),
                failure: (!complete).then_some(cause),
                rollback,
            })),
            ..Self::new(cause, String::new())
        };
        if complete {
            return Self {
                message: format!("{message}; Operation {operation} was rolled back"),
                ..failure
            };
        }
        let files = match not_restored {
            1 => "1 file".to_owned(),
            n => format!("{n} files"),
        };
        Self {
            code: ErrorCode::RollbackIncomplete,
            message: format!(
                "{message}; the Rollback of Operation {operation} could not restore {files}, which need manual recovery: the files as they were before it are in {}",
                backup.display()
            ),
            diffs,
            ..failure
        }
    }
}

/// A JSON value as the TypeScript bindings type it: its numbers are
/// JavaScript numbers. Only the bindings use it; values are `serde_json`'s.
#[derive(Serialize, specta::Type)]
#[serde(untagged)]
#[allow(dead_code)]
pub enum JsonValue {
    Null(()),
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<JsonValue>),
    Object(std::collections::BTreeMap<String, JsonValue>),
}
