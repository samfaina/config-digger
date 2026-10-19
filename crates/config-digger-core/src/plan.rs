//! Plan: builds an Operation from an action and the Entries it targets,
//! without writing anything. It computes the edits of every touched file,
//! predicts the States after them, and lists what must be acknowledged.

use crate::api::{
    self, Action, Block, Change, Context, DiffUnavailable, EditSummary, EffectiveChange, Error,
    FileChange, FileDiff, Home, Info, InfoCode, Inventory, Leftovers, Preview, ReadFailure,
    ReadFailureKind, ScanOptions, State, StateAfter, Targets, Warning, WarningClass,
};
use crate::definitions::blocks::{Disables, Gate, StoreClass};
use crate::definitions::{AgentDefinition, Definitions};
use crate::disable::{self, Choice, ContextProject, Hit};
use crate::edit::{Dependency, Edit, EditError, EditableFile, ReadOnly, Syntax};
use crate::explain::Reached;
use crate::folders::{self, FolderOp, PlannedFolder};
use crate::history::FolderOpRecord;
use crate::mask::{self, Side};
use crate::ports::Ports;
use crate::running::RunningHomes;
use crate::scan::{LocationReader, ScannedEntry, Snapshot, SwitchStore};
use crate::store::{self, Medium, Written};
use crate::{explain, history, park, resolve, rules, running, scan, select};
use std::io;
use std::path::{Path, PathBuf};

/// A plan, as [`api::Plan`] holds it.
pub(crate) struct Planned {
    pub action: Action,
    /// How many Entries it targets.
    pub entries: usize,
    pub context: Context,
    /// What Verify scans with.
    pub scan_options: ScanOptions,
    pub files: Vec<PlannedFile>,
    /// The skill folders and links it moves, removes or puts back.
    pub folders: Vec<PlannedFolder>,
    pub preview: Preview,
    pub warnings: Vec<Warning>,
    /// Why it cannot execute, whatever is acknowledged.
    pub blocks: Vec<Block>,
    /// The Read failures before the Operation: Verify fails it when a
    /// written file no longer parses for a reader that could read it.
    pub read_failures: Vec<ReadFailure>,
    /// The Agent homes that tell what a running Agent reads.
    pub homes: RunningHomes,
}

/// The edits to one file.
pub(crate) struct PlannedFile {
    pub path: PathBuf,
    pub syntax: Syntax,
    /// The bytes the edits were computed on: the minimal structure of the
    /// syntax when the file is missing and the plan creates it.
    pub read: Vec<u8>,
    /// The file is missing and the plan creates it.
    pub created: bool,
    pub edited: EditedFile,
    /// The locations that read the file, whose Entries say where its
    /// Secrets are.
    pub readers: Vec<LocationReader>,
    /// The Agents whose Disable switches are in the file.
    pub switches: Vec<SwitchStore>,
    /// The file is in the index of its git repo, so the edits show in the
    /// git diff.
    pub git_versioned: bool,
    /// An Entry is parked out of the file or put back into it, which a
    /// running Agent that rewrites the file from memory would undo.
    pub parking: bool,
    /// The file is the parked-Entry store, whose Secrets are those of the
    /// Entries parked in it.
    pub parked_store: bool,
    /// For the parked-Entry store, every location the scan read: those that
    /// read the file an Entry was parked from mark its Secrets too.
    pub parked_readers: Vec<LocationReader>,
    /// What it writes: the file, or values of the SQLite store it is.
    pub medium: Medium,
}

impl PlannedFile {
    /// What it will hold once written.
    pub fn written(&self) -> Written {
        Written {
            path: self.path.clone(),
            medium: self.medium.clone(),
            bytes: self.edited.bytes.clone(),
        }
    }

    /// A diff of the file from `before` to `after`, with every Secret masked.
    pub fn diff(&self, definitions: &[AgentDefinition], before: Side, after: Side) -> FileDiff {
        let store = self
            .parked_store
            .then_some((self.path.as_path(), self.parked_readers.as_slice()));
        file_diff(definitions, &self.readers, &self.path, store, before, after)
    }

    /// How the file behaves while an Agent that reads it or keeps switches
    /// in it runs: the strongest class of those locations and switches.
    pub fn store_class(&self, definitions: &[AgentDefinition]) -> StoreClass {
        let locations = self.readers.iter().map(|reader| {
            definitions[reader.agent_index]
                .location(&reader.location)
                .store_class
        });
        let switches = self.switches.iter().map(|store| store.store_class);
        locations.chain(switches).max().unwrap_or(StoreClass::Plain)
    }
}

/// A diff of the file at `path` from `before` to `after`, with every Secret
/// masked: those of the Entries parked in it when it is the parked-Entry
/// store of `store`, as the locations that come with it mark them, and else
/// those of the Entries `readers` read there.
fn file_diff(
    definitions: &[AgentDefinition],
    readers: &[LocationReader],
    path: &Path,
    store: Option<(&Path, &[LocationReader])>,
    before: Side,
    after: Side,
) -> FileDiff {
    if let Some((_, parked_readers)) = store.filter(|(store, _)| *store == path) {
        mask::diff_parked(definitions, parked_readers, path, before, after)
    } else {
        mask::diff(definitions, readers, path, before, after)
    }
}

/// A file with every edit of an Operation applied, in order.
pub(crate) struct EditedFile {
    pub bytes: Vec<u8>,
    pub edits: Vec<AppliedEdit>,
}

pub(crate) struct AppliedEdit {
    pub edit: Edit,
    pub inverse: Edit,
    pub depends_on: Vec<Dependency>,
}

/// Why a file's edits could not be computed.
pub(crate) enum EditFailure {
    ReadOnly(ReadOnly),
    Edit(EditError),
}

pub(crate) fn plan(
    definitions: &Definitions,
    ports: &Ports,
    snapshot: &Snapshot,
    scan_options: &ScanOptions,
    context: &Context,
    action: Action,
    targets: &Targets,
) -> Result<Planned, Error> {
    let before = resolve::resolve(definitions, snapshot, context);
    // The leftovers of the context's own Project are in it already.
    let mut leftovers = resolve::leftovers(definitions, snapshot);
    leftovers.retain(|leftovers| before.project.as_ref() != Some(&leftovers.project));
    let selected = select::select_entries(&before, &leftovers, &targets.selectors, targets.all)?;
    let (targeted, mirrored) = redirected(definitions, snapshot, &before, selected, action)?;
    let targeted_entries = Targeted {
        definitions,
        ports,
        snapshot,
        scan_options,
        context,
        action,
        before: &before,
        leftovers: &leftovers,
        entries: &targeted,
    };
    let mut planned = match action {
        Action::Remove => targeted_entries.remove(),
        Action::Disable | Action::Enable => targeted_entries.switch(),
        Action::Undo => Err(Error::unexpected(
            "an Undo targets an Operation, not Entries: plan it with plan_undo",
        )),
        Action::DeleteBackup => Err(Error::unexpected(
            "a Delete backup targets an Operation, not Entries: plan it with plan_delete_backup",
        )),
    }?;
    for home in mirrored {
        let mirror = home.mirror.as_ref().expect("a Mirrored home has a mirror");
        // What it writes outside the home it copies, such as a Project's
        // file, which both homes read, reaches the mirror as it is.
        let copied = mirror.source_folder.as_deref().is_none_or(|source| {
            let written = planned.preview.files.iter().map(|file| &file.path);
            let moved = planned.preview.folders.iter().map(|folder| &folder.path);
            written.chain(moved).any(|path| path.starts_with(source))
        });
        if !copied {
            continue;
        }
        planned.preview.info.push(Info {
            code: InfoCode::ReachesMirroredHome,
            path: home.folder.clone(),
            sentence: explain::reaches_mirrored_home(
                &mirror.owner,
                &home.folder,
                mirror.source_folder.as_deref(),
            ),
        });
    }
    Ok(planned)
}

/// The Entries an action on `selected` goes to, each once: each Entry
/// itself, or for one of a Mirrored home, the Entry it was copied from, as
/// `before` holds it. With them, the Mirrored homes the action reaches
/// through the tool that copies them. Refused for an Entry of a Mirrored
/// home that the home it is copied from lacks.
fn redirected<'i, 's>(
    definitions: &[AgentDefinition],
    snapshot: &'s Snapshot,
    before: &'i Inventory,
    selected: Vec<&'i api::Entry>,
    action: Action,
) -> Result<(Vec<&'i api::Entry>, Vec<&'s scan::ScannedHome>), Error> {
    let mut targeted: Vec<&api::Entry> = Vec::new();
    let mut mirrored: Vec<&scan::ScannedHome> = Vec::new();
    for entry in selected {
        // Several homes can read one file, so the Entry is the one of the
        // home whose Item holds it.
        let held_by = before
            .items
            .iter()
            .find(|item| item.entries.iter().any(|own| std::ptr::eq(own, entry)));
        let scanned = held_by
            .and_then(|item| {
                let agent = definitions
                    .iter()
                    .position(|definition| definition.agent.id == item.agent)?;
                snapshot.entry_in(agent, &item.home, &entry.path, &entry.key_path)
            })
            .unwrap_or_else(|| snapshot.entry(&entry.path, &entry.key_path));
        let target = match snapshot.mirror_of(scanned) {
            None => entry,
            Some(mirror) => {
                let agent = &definitions[scanned.agent_index].agent.id;
                let source = snapshot.copied_from(scanned).and_then(|source| {
                    let locator = crate::locator::render(&source.path, &source.key_path);
                    item_in(before, agent, &source.home, &locator)?
                        .entries
                        .iter()
                        .find(|known| known.identity() == locator)
                });
                let Some(source) = source else {
                    return Err(Error::blocked(explain::mirrored_without_source(
                        explain::action_verb(action),
                        &entry.locator,
                        &mirror.owner,
                        mirror.source_folder.as_deref(),
                    )));
                };
                let home = snapshot.home_of(scanned);
                if !mirrored.iter().any(|known| known.home == home.home) {
                    mirrored.push(home);
                }
                source
            }
        };
        if !targeted
            .iter()
            .any(|known| known.identity() == target.identity())
        {
            targeted.push(target);
        }
    }
    Ok((targeted, mirrored))
}

/// Plans the Remove of every leftover of the missing Project `project`, in
/// any spelling, as one Operation in that Project's context.
pub(crate) fn plan_remove_leftovers(
    definitions: &Definitions,
    ports: &Ports,
    snapshot: &Snapshot,
    scan_options: &ScanOptions,
    project: &Path,
) -> Result<Planned, Error> {
    let context = Context {
        project: Some(project.to_path_buf()),
    };
    let found = resolve::context_project(snapshot, &context).and_then(|(path, _)| {
        resolve::leftovers(definitions, snapshot)
            .into_iter()
            .find(|leftovers| leftovers.project == path)
    });
    let Some(found) = found else {
        return Err(Error::no_leftovers(project));
    };
    let targets = Targets {
        selectors: found.entries().map(|entry| entry.locator.clone()).collect(),
        all: false,
    };
    let context = Context {
        project: Some(found.project),
    };
    plan(
        definitions,
        ports,
        snapshot,
        scan_options,
        &context,
        Action::Remove,
        &targets,
    )
}

/// An action on the Entries it targets, about to be planned.
struct Targeted<'a> {
    definitions: &'a Definitions,
    ports: &'a Ports,
    snapshot: &'a Snapshot,
    scan_options: &'a ScanOptions,
    context: &'a Context,
    action: Action,
    /// The Items in the context before the Operation.
    before: &'a Inventory,
    /// The leftovers of the missing Projects other than the context's.
    leftovers: &'a [Leftovers],
    entries: &'a [&'a api::Entry],
}

impl Targeted<'_> {
    /// A Remove: each Entry's key goes from its file, each skill folder
    /// into the backup, and each Linked Entry's link away.
    fn remove(&self) -> Result<Planned, Error> {
        // The edits of each file, in the order its Entries were selected.
        let mut by_file: Vec<(PathBuf, Syntax, Vec<Edit>)> = Vec::new();
        let mut planned_folders = Vec::new();
        for entry in self.entries {
            let syntax = match removal(self.definitions, self.snapshot, entry)? {
                Removal::Key(syntax) => syntax,
                Removal::Folder => {
                    planned_folders.push(self.removed_folder(entry)?);
                    continue;
                }
            };
            // A parked skill's folder goes from the store with its record.
            let scanned = self.snapshot.entry(&entry.path, &entry.key_path);
            if let Some(folder) = self
                .snapshot
                .parked_of(scanned)
                .and_then(|parked| parked.folder.as_ref())
            {
                planned_folders.push(PlannedFolder {
                    path: folder.clone(),
                    op: FolderOp::MoveToBackup,
                    size: folders::measure(self.ports.fs.as_ref(), folder)?,
                    git_versioned: false,
                    readers: self.snapshot.readers_of_folder(folder),
                });
            }
            let edit = Edit::Remove {
                key_path: entry.key_path.clone(),
            };
            match by_file.iter_mut().find(|(path, ..)| *path == entry.path) {
                Some((_, _, edits)) => edits.push(edit),
                None => {
                    by_file.push((entry.path.clone(), syntax, vec![edit]));
                }
            }
        }

        let mut files = Vec::new();
        for (path, syntax, edits) in by_file {
            let read = read(self.ports, &path)?;
            let mut edited = apply_edits(&read, syntax, &edits, &Medium::File)
                .map_err(|failure| failure.into_error(&path))?;
            // Undo puts the Entry back without the comments above it.
            if keeps_comments_out(self.definitions, self.snapshot, &path) {
                for applied in &mut edited.edits {
                    applied.inverse = applied.inverse.clone().without_comments();
                }
            }
            files.push(self.planned_file(path, syntax, read, false, edited));
        }

        // A folder moved away takes the Entries of every link into it
        // with it, so the States after come from a scan as if it were gone.
        let without = if planned_folders.is_empty() {
            let removed: Vec<(PathBuf, Vec<String>)> = self
                .entries
                .iter()
                .map(|entry| (entry.path.clone(), entry.key_path.clone()))
                .collect();
            self.snapshot.without(&removed)
        } else {
            let written: Vec<Written> = files.iter().map(PlannedFile::written).collect();
            scan::scan_as_if(
                self.definitions,
                self.ports,
                &self.scan_options.projects,
                &written,
                &planned_folders,
            )
            .map_err(Error::unexpected)?
        };
        let after = resolve::resolve(self.definitions, &without, self.context);
        let (mut changes, effective) = predict(self.before, &after);
        // A leftover of another missing Project goes in that Project's context.
        let holds_target = |leftovers: &&Leftovers| {
            leftovers.entries().any(|leftover| {
                self.entries
                    .iter()
                    .any(|entry| entry.locator == leftover.locator)
            })
        };
        for leftovers in self.leftovers.iter().filter(holds_target) {
            let context = Context {
                project: Some(leftovers.project.clone()),
            };
            let before = resolve::resolve(self.definitions, self.snapshot, &context);
            let after = resolve::resolve(self.definitions, &without, &context);
            changes.extend(predict(&before, &after).0);
        }
        let breaks = self.breaks_links(&planned_folders);
        let mut planned = self.planned(files, planned_folders, changes, effective, breaks)?;
        planned.preview.info.extend(self.approvals_kept());
        Ok(planned)
    }

    /// The notes of a Remove on the approvals it leaves as they are: one per
    /// Entry it targets that an approval gate covers and that its Agent
    /// approved.
    fn approvals_kept(&self) -> Vec<Info> {
        self.entries
            .iter()
            .filter(|entry| entry.state != State::Unread)
            .filter(|entry| {
                let reasons = entry.reason.iter().chain(&entry.secondary);
                !reasons
                    .into_iter()
                    .any(|reason| reason.code == api::ReasonCode::NotApproved)
            })
            .filter_map(|entry| {
                let scanned = self.snapshot.entry(&entry.path, &entry.key_path);
                let definition = &self.definitions[scanned.agent_index];
                let location = definition.location(&scanned.location);
                let gate = definition.rules(location.kind).gates.iter().find(|gate| {
                    matches!(gate, Gate::ApprovalList(_) | Gate::ApprovalHash(_))
                        && gate.covers(&location.id)
                })?;
                let by_hash = matches!(gate, Gate::ApprovalHash(_));
                Some(Info {
                    code: InfoCode::ApprovalKept,
                    path: entry.path.clone(),
                    sentence: explain::approval_kept(
                        &definition.agent.name,
                        &scanned.name,
                        by_hash,
                    ),
                })
            })
            .collect()
    }

    /// What Remove does at the location of the skill Entry `entry`: it
    /// removes a Linked Entry's link, or else moves the folder into the
    /// backup, each measured for Verify.
    fn removed_folder(&self, entry: &api::Entry) -> Result<PlannedFolder, Error> {
        let fs = self.ports.fs.as_ref();
        let scanned = self.snapshot.entry(&entry.path, &entry.key_path);
        let (op, size) = match &scanned.link {
            Some(link) => {
                let size = folders::measure_target(fs, link.real.as_deref())?;
                let target = link.target.clone();
                (FolderOp::RemoveLink { target }, size)
            }
            None => (FolderOp::MoveToBackup, folders::measure(fs, &entry.path)?),
        };
        Ok(PlannedFolder {
            path: entry.path.clone(),
            op,
            size,
            git_versioned: entry.git_versioned,
            readers: self.snapshot.readers_of_folder(&entry.path),
        })
    }

    /// The `breaks-links` warning when a folder moved into the backup is
    /// the target of Linked Entries the Remove does not target: they are
    /// left pointing to nothing, as Unread Entries, and not Removed with it.
    fn breaks_links(&self, planned: &[PlannedFolder]) -> Option<Warning> {
        let mut broken: Vec<(&Path, &Path)> = Vec::new();
        let moved = planned
            .iter()
            .filter(|folder| folder.op == FolderOp::MoveToBackup);
        for folder in moved {
            for entry in &self.snapshot.entries {
                let Some(real) = entry.link.as_ref().and_then(|link| link.real.as_ref()) else {
                    continue;
                };
                let into = folders::below(real, &folder.path).is_some();
                let targeted = self
                    .entries
                    .iter()
                    .any(|targeted| targeted.path == entry.path);
                if into && !targeted && !broken.iter().any(|(path, _)| *path == entry.path) {
                    broken.push((&entry.path, &folder.path));
                }
            }
        }
        (!broken.is_empty()).then(|| Warning {
            class: WarningClass::BreaksLinks,
            sentence: explain::breaks_links(&broken),
        })
    }

    /// A Disable or an Enable: each Entry's switch is set or lifted, or the
    /// Entry parked or put back, which can reach other Entries and Projects
    /// than those targeted.
    fn switch(&self) -> Result<Planned, Error> {
        let project = resolve::context_project(self.snapshot, self.context);
        let context_project = project
            .as_ref()
            .map(|(path, key)| ContextProject { path, key });
        // The edits of each file, in the order its Entries were selected.
        let mut by_file: Vec<FileEdits> = Vec::new();
        let mut planned_folders: Vec<PlannedFolder> = Vec::new();
        let mut lifted: Vec<Hit> = Vec::new();
        let mut parks: Vec<&ScannedEntry> = Vec::new();
        // The names written to switches that hold them in every context.
        let mut by_name: Vec<String> = Vec::new();
        for entry in self.entries {
            // Its switches would be offered as for any Entry of the Project.
            if entry.state == State::Unread {
                return Err(Error::blocked(explain::only_removed(
                    self.action,
                    &entry.locator,
                )));
            }
            let scanned = self.snapshot.entry(&entry.path, &entry.key_path);
            let definition = &self.definitions[scanned.agent_index];
            match self.action {
                Action::Disable => {
                    if entry.state == State::Disabled {
                        return Err(Error::blocked(explain::already_disabled(&entry.locator)));
                    }
                    let choice = disable::choose(
                        definition,
                        self.snapshot,
                        scanned,
                        context_project.as_ref(),
                    )
                    .map_err(|(_, sentence)| {
                        Error::blocked(explain::disable_not_offered(&entry.locator, &sentence))
                    })?;
                    match choice {
                        Choice::Switch {
                            path,
                            edits,
                            syntax,
                            sqlite,
                            by_name_everywhere,
                            ..
                        } => {
                            if by_name_everywhere && !by_name.contains(&scanned.name) {
                                by_name.push(scanned.name.clone());
                            }
                            // Entries Disabled by one name share its switch.
                            let shared = by_file.iter().any(|file| {
                                file.path == path
                                    && edits.iter().all(|edit| file.edits.contains(edit))
                            });
                            if !shared {
                                for edit in edits {
                                    add_switch_edit(
                                        &mut by_file,
                                        path.clone(),
                                        syntax,
                                        sqlite,
                                        edit,
                                    );
                                }
                            }
                        }
                        Choice::Park { .. } => parks.push(scanned),
                    }
                }
                _ => {
                    // Such as for an Entry a field of the server its Agent
                    // builds turns off, which no switch of its own holds.
                    if let Some(api::EnableOption::NotOffered { sentence }) = &entry.enable {
                        return Err(Error::blocked(explain::enable_refused(
                            &entry.locator,
                            sentence,
                        )));
                    }
                    let project = project.as_ref().map(|(_, key)| key.as_str());
                    // Only rules outside policy matter here, and none of those
                    // checks a server its Agent builds.
                    let ruled = rules::hits(definition, self.snapshot, scanned, project, None);
                    let by_rule: Vec<&rules::RuleHit> = ruled
                        .iter()
                        .filter(|hit| hit.state == State::Disabled)
                        .collect();
                    if let Some(sentence) = rules::enable_not_offered(&by_rule) {
                        return Err(Error::blocked(explain::enable_refused(
                            &entry.locator,
                            &sentence,
                        )));
                    }
                    let parked = self.snapshot.parked_of(scanned);
                    let (disabling, _) = disable::hits(definition, self.snapshot, scanned, project);
                    if disabling.is_empty() && parked.is_none() {
                        return Err(Error::blocked(explain::not_disabled(&entry.locator)));
                    }
                    // Every layer's switch, or a later one would Disable it again.
                    let lifts =
                        disable::lifted_by_enable(definition, self.snapshot, scanned, project);
                    let location = definition.location(&scanned.location);
                    if !lifts.is_empty()
                        && let Err(sentence) = disable::enable(location, &lifts)
                    {
                        return Err(Error::blocked(explain::enable_refused(
                            &entry.locator,
                            &sentence,
                        )));
                    }
                    if parked.is_some() {
                        self.put_back(&mut by_file, &mut planned_folders, entry, scanned)?;
                    }
                    for hit in lifts {
                        // Entries Disabled by one name share its switch.
                        if lifted.iter().any(|known| known.same_switch(&hit)) {
                            continue;
                        }
                        if disable::by_name_everywhere(hit.mechanism)
                            && !by_name.contains(&scanned.name)
                        {
                            by_name.push(scanned.name.clone());
                        }
                        let sqlite = hit.mechanism.switch().sqlite();
                        let syntax = hit.mechanism.switch().syntax();
                        for edit in hit.enable_edits() {
                            let path = hit.switch.path.clone();
                            add_switch_edit(&mut by_file, path, syntax, sqlite, edit);
                        }
                        lifted.push(hit);
                    }
                }
            }
        }
        self.park(&mut by_file, &mut planned_folders, &parks)?;

        let mut files = Vec::new();
        for file in by_file {
            let medium = file.medium();
            let path = file.path;
            let (read, created) = match read_if_there(self.ports, &path, &medium)? {
                Some(bytes) => (bytes, false),
                None if medium == Medium::File => (empty_file(file.syntax).to_vec(), true),
                None => return Err(Error::blocked(explain::store_missing(&path))),
            };
            let edited = apply_edits(&read, file.syntax, &file.edits, &medium)
                .map_err(|failure| failure.into_error(&path))?;
            let mut planned = self.planned_file(path, file.syntax, read, created, edited);
            planned.parking = file.parking;
            planned.medium = medium;
            files.push(planned);
        }

        let written: Vec<Written> = files.iter().map(PlannedFile::written).collect();
        let after = scan::scan_as_if(
            self.definitions,
            self.ports,
            &self.scan_options.projects,
            &written,
            &planned_folders,
        )
        .map_err(Error::unexpected)?;
        let (changes, effective) = self.reach(&after);
        let wide = self.wide_reach(&changes, &by_name);
        self.planned(files, planned_folders, changes, effective, wide)
    }

    /// Adds the edits that park `parks` to `by_file`, and the moves to
    /// `planned_folders`: each Entry's key goes from its file, or each
    /// skill's folder into the store's `folders`, or its link away, and the
    /// Entry, verbatim with where it came from, into the parked-Entry store
    /// under a free id.
    fn park(
        &self,
        by_file: &mut Vec<FileEdits>,
        planned_folders: &mut Vec<PlannedFolder>,
        parks: &[&ScannedEntry],
    ) -> Result<(), Error> {
        let Some(store) = self
            .snapshot
            .parked_store
            .as_ref()
            .filter(|_| !parks.is_empty())
        else {
            return Ok(());
        };
        let mut taken = match read_if_there(self.ports, store, &Medium::File)? {
            Some(bytes) => parked_ids(&bytes).map_err(|failure| failure.into_error(store))?,
            None => Vec::new(),
        };
        let parked_at = humantime::format_rfc3339_seconds(self.ports.clock.now()).to_string();
        let origin_of = |entry: &ScannedEntry| park::Origin {
            agent: self.definitions[entry.agent_index].agent.id.clone(),
            home: entry.home.clone(),
            location: entry.location.clone(),
            name: entry.name.clone(),
            path: entry.path.clone(),
            key_path: entry.key_path.clone(),
            project: entry.project.clone(),
        };
        let (skills, keys): (Vec<&ScannedEntry>, Vec<&ScannedEntry>) =
            parks.iter().partition(|entry| {
                let definition = &self.definitions[entry.agent_index];
                definition.location(&entry.location).format.file().is_none()
            });
        // The Entries parked from each file, in the order they were selected.
        let mut origins: Vec<(&Path, Syntax, Vec<&ScannedEntry>)> = Vec::new();
        for entry in keys {
            match origins.iter_mut().find(|(path, ..)| *path == entry.path) {
                Some((_, _, entries)) => entries.push(entry),
                None => origins.push((
                    &entry.path,
                    file_syntax(self.definitions, entry),
                    vec![entry],
                )),
            }
        }
        for (path, syntax, entries) in origins {
            let removals: Vec<Edit> = entries
                .iter()
                .map(|entry| Edit::Remove {
                    key_path: entry.key_path.clone(),
                })
                .collect();
            let read = read(self.ports, path)?;
            let edited = apply_edits(&read, syntax, &removals, &Medium::File)
                .map_err(|failure| failure.into_error(path))?;
            for edit in removals {
                add_edit(by_file, path.to_path_buf(), syntax, edit, true);
            }
            for (entry, removed) in entries.iter().zip(&edited.edits) {
                let id = park::free_id(&entry.name, &taken);
                let parked = park::ParkedEntry::new(
                    origin_of(entry),
                    parked_at.clone(),
                    &removed.inverse,
                )
                .ok_or_else(|| Error::unexpected("a removed key comes back by an insertion"))?;
                let edit = Edit::Set {
                    key_path: park::key_path(&id),
                    value: parked.to_value(),
                };
                add_edit(by_file, store.clone(), Syntax::Jsonc, edit, false);
                taken.push(id);
            }
        }
        let fs = self.ports.fs.as_ref();
        let kept_in = park::folders_path(store);
        for entry in skills {
            // A folder an earlier Operation left in the store keeps its name.
            let mut id = park::free_id(&entry.name, &taken);
            while folders::taken(fs, &kept_in.join(&id)) {
                taken.push(id);
                id = park::free_id(&entry.name, &taken);
            }
            let (kept, op, size) = match &entry.link {
                Some(link) => {
                    let size = folders::measure_target(fs, link.real.as_deref())?;
                    let target = link.target.clone();
                    (
                        park::Kept::Link(target.clone()),
                        FolderOp::RemoveLink { target },
                        size,
                    )
                }
                None => {
                    let to = kept_in.join(&id);
                    let size = folders::measure(fs, &entry.path)?;
                    (park::Kept::Folder(id.clone()), FolderOp::Park { to }, size)
                }
            };
            let parked = park::ParkedEntry::skill(
                origin_of(entry),
                parked_at.clone(),
                entry.config.clone(),
                kept,
            );
            let edit = Edit::Set {
                key_path: park::key_path(&id),
                value: parked.to_value(),
            };
            add_edit(by_file, store.clone(), Syntax::Jsonc, edit, false);
            planned_folders.push(PlannedFolder {
                git_versioned: self.snapshot.versioned.contains(&entry.path),
                readers: self.snapshot.readers_of_folder(&entry.path),
                path: entry.path.clone(),
                op,
                size,
            });
            taken.push(id);
        }
        Ok(())
    }

    /// Adds the edits that put the parked Entry `entry` back to `by_file`,
    /// and the moves to `planned_folders`: its key goes back into its file,
    /// or a skill's folder or link back where it was, and the Entry out of
    /// the store. Refused when an Entry is at its key or folder again, which
    /// it would overwrite, or when the folder it goes in is gone. A missing
    /// file whose folder exists is created with the minimal structure.
    fn put_back(
        &self,
        by_file: &mut Vec<FileEdits>,
        planned_folders: &mut Vec<PlannedFolder>,
        entry: &api::Entry,
        scanned: &ScannedEntry,
    ) -> Result<(), Error> {
        let store = &entry.path;
        let bytes = read(self.ports, store)?;
        let file = EditableFile::decode(&bytes, Syntax::Jsonc)
            .map_err(|reason| EditFailure::ReadOnly(reason).into_error(store))?;
        let value = file
            .value_at(&entry.key_path)
            .map_err(|error| EditFailure::Edit(error).into_error(store))?
            .ok_or_else(|| Error::stale(explain::no_longer_parked(&entry.locator)))?;
        let parked: park::ParkedEntry = serde_json::from_value(value)
            .map_err(|_| Error::blocked(explain::parked_unreadable(&entry.locator, store)))?;
        let remove_record = Edit::Remove {
            key_path: entry.key_path.clone(),
        };
        let definition = &self.definitions[scanned.agent_index];
        if definition
            .location(&scanned.location)
            .format
            .file()
            .is_none()
        {
            planned_folders.push(self.unparked_folder(entry, scanned, &parked)?);
            add_edit(by_file, store.clone(), Syntax::Jsonc, remove_record, false);
            return Ok(());
        }
        let syntax = file_syntax(self.definitions, scanned);
        let origin = &parked.origin.path;
        match read_if_there(self.ports, origin, &Medium::File)? {
            Some(bytes) => {
                let file = EditableFile::decode(&bytes, syntax)
                    .map_err(|reason| EditFailure::ReadOnly(reason).into_error(origin))?;
                let now = file
                    .value_at(&parked.origin.key_path)
                    .map_err(|error| EditFailure::Edit(error).into_error(origin))?;
                if now.is_some() {
                    return Err(Error::blocked(explain::unpark_conflict(
                        &entry.locator,
                        &parked.origin.key_path,
                        origin,
                    )));
                }
            }
            None => {
                if !origin
                    .parent()
                    .is_some_and(|folder| self.ports.fs.exists(folder))
                {
                    return Err(Error::blocked(explain::unpark_folder_gone(
                        &entry.locator,
                        origin,
                    )));
                }
            }
        }
        let mut put_back = parked.put_back();
        if keeps_comments_out(self.definitions, self.snapshot, origin) {
            put_back = put_back.without_comments();
        }
        add_edit(by_file, origin.clone(), syntax, put_back, true);
        add_edit(by_file, store.clone(), Syntax::Jsonc, remove_record, false);
        Ok(())
    }

    /// What Enable does at the folder of the parked skill `entry`, `parked`
    /// as the store holds it: it moves the folder back from the store, or
    /// creates the link again. Refused when something is at the folder
    /// again, when the Project it applies in is gone, or when the store no
    /// longer keeps the folder.
    fn unparked_folder(
        &self,
        entry: &api::Entry,
        scanned: &ScannedEntry,
        parked: &park::ParkedEntry,
    ) -> Result<PlannedFolder, Error> {
        let fs = self.ports.fs.as_ref();
        let origin = &parked.origin.path;
        if folders::taken(fs, origin) {
            return Err(Error::blocked(explain::unpark_folder_taken(
                &entry.locator,
                origin,
            )));
        }
        let project_gone = scanned.project.as_ref().is_some_and(|key| {
            self.snapshot
                .projects
                .iter()
                .any(|project| project.key == *key && !project.exists)
        });
        if project_gone {
            return Err(Error::blocked(explain::unpark_folder_gone(
                &entry.locator,
                origin,
            )));
        }
        let kept = self
            .snapshot
            .parked_of(scanned)
            .and_then(|at| at.folder.as_ref());
        let (op, size) = match (&parked.link, kept) {
            (Some(target), _) => {
                let size = folders::measure_target(fs, fs.real_path(target).as_deref())?;
                let target = target.clone();
                (FolderOp::CreateLink { target }, size)
            }
            (None, Some(from)) if fs.exists(from) => {
                let size = folders::measure(fs, from)?;
                let from = from.clone();
                (FolderOp::Unpark { from }, size)
            }
            (None, _) => {
                return Err(Error::blocked(explain::parked_folder_gone(
                    &entry.locator,
                    origin,
                )));
            }
        };
        Ok(PlannedFolder {
            path: origin.clone(),
            op,
            size,
            git_versioned: self.ports.git.is_versioned(origin),
            readers: self.snapshot.readers_of_folder(origin),
        })
    }

    /// What a write reaches: the States it changes in the context, then in
    /// every other one, each other Project and no Project, as a write to a
    /// switch can apply beyond the context.
    fn reach(&self, after: &Snapshot) -> (Vec<Change>, Vec<EffectiveChange>) {
        let (mut changes, effective) = predict(
            self.before,
            &resolve::resolve(self.definitions, after, self.context),
        );
        let others = self
            .before
            .projects
            .iter()
            .map(|project| Some(project.path.clone()))
            .chain([None])
            .filter(|project| *project != self.before.project);
        for project in others {
            let context = Context { project };
            let before = resolve::resolve(self.definitions, self.snapshot, &context);
            let after = resolve::resolve(self.definitions, after, &context);
            changes.extend(predict(&before, &after).0);
        }
        (changes, effective)
    }

    /// The `wide-reach` warning when the switches written Disable, or lift
    /// for, Entries the action does not target, or apply in other contexts
    /// than its own. An Entry that only wins or loses against one that the
    /// switches reach is not reached itself. An Entry reached in every
    /// context, or in every other one when it is targeted, is named once.
    /// Switches written for `by_name`, names they hold in every context,
    /// reach every Entry of those names, in all projects, wherever it is.
    fn wide_reach(&self, changes: &[Change], by_name: &[String]) -> Option<Warning> {
        let reached: Vec<&Change> = changes
            .iter()
            .filter(|change| {
                change.before == StateAfter::Disabled || change.after == StateAfter::Disabled
            })
            .collect();
        let targeted = |locator: &str| self.entries.iter().any(|entry| entry.identity() == locator);
        // Every Project and none.
        let contexts = self.before.projects.len() + 1;
        let everywhere = |locator: &str| {
            let mut projects: Vec<Option<&PathBuf>> = reached
                .iter()
                .filter(|change| change.locator == locator)
                .map(|change| change.project.as_ref())
                .chain(targeted(locator).then_some(self.before.project.as_ref()))
                .collect();
            projects.sort();
            projects.dedup();
            contexts > 1 && projects.len() == contexts
        };
        let mut named: Vec<&str> = Vec::new();
        let others: Vec<Reached> = reached
            .iter()
            .filter_map(|change| {
                let locator = change.locator.clone();
                if everywhere(&locator) {
                    if named.contains(&change.locator.as_str()) {
                        return None;
                    }
                    named.push(&change.locator);
                    return Some(Reached::Everywhere(locator));
                }
                if change.project != self.before.project {
                    return Some(match &change.project {
                        Some(project) => Reached::InProject(locator, project.clone()),
                        None => Reached::WithoutProject(locator),
                    });
                }
                (!targeted(&locator)).then_some(Reached::Here(locator))
            })
            .collect();
        (!others.is_empty() || !by_name.is_empty()).then(|| Warning {
            class: WarningClass::WideReach,
            sentence: explain::wide_reach(self.action, &others, by_name),
        })
    }

    fn planned_file(
        &self,
        path: PathBuf,
        syntax: Syntax,
        read: Vec<u8>,
        created: bool,
        edited: EditedFile,
    ) -> PlannedFile {
        let parked_store = self.snapshot.parked_store.as_ref() == Some(&path);
        PlannedFile {
            readers: self.snapshot.readers_of(&path),
            switches: self.snapshot.switch_stores_of(&path),
            git_versioned: self.snapshot.versioned.contains(&path),
            parked_store,
            parked_readers: match parked_store {
                true => self.snapshot.readers.clone(),
                false => Vec::new(),
            },
            parking: false,
            medium: Medium::File,
            path,
            syntax,
            read,
            created,
            edited,
        }
    }

    /// The plan that writes `files` and touches `folders`, whose preview
    /// shows `changes` and `effective`, with `action_warning`, the warning of its
    /// own action if it has one.
    fn planned(
        &self,
        files: Vec<PlannedFile>,
        folders: Vec<PlannedFolder>,
        changes: Vec<Change>,
        effective: Vec<EffectiveChange>,
        action_warning: Option<Warning>,
    ) -> Result<Planned, Error> {
        let title = explain::operation_title(self.action, self.entries.len());
        let preview = preview(
            self.definitions,
            self.ports,
            self.action,
            title,
            None,
            self.before.project.clone(),
            changes,
            effective,
            &files,
            &folders,
            &self.before.read_failures,
            &self.before.homes,
        )?;
        let homes = RunningHomes::of(self.definitions, self.snapshot);
        let running = running::check(self.definitions, self.ports, &homes, &files, &folders);
        let mut warnings = warnings(&files, &folders, running.warning);
        warnings.extend(action_warning);
        Ok(Planned {
            action: self.action,
            entries: self.entries.len(),
            context: self.context.clone(),
            scan_options: self.scan_options.clone(),
            warnings,
            blocks: running.blocks,
            files,
            folders,
            preview,
            read_failures: self.before.read_failures.clone(),
            homes,
        })
    }
}

/// The edits of one file an Operation touches.
struct FileEdits {
    path: PathBuf,
    syntax: Syntax,
    edits: Vec<Edit>,
    /// An Entry is parked out of the file or put back into it.
    parking: bool,
    /// The file is a SQLite store whose values the edits change.
    sqlite: bool,
}

impl FileEdits {
    /// What the edits write: the file, or the values of the store their
    /// key paths start with.
    fn medium(&self) -> Medium {
        if self.sqlite {
            Medium::sqlite(self.edits.iter().map(|edit| edit.key_path()[0].as_str()))
        } else {
            Medium::File
        }
    }
}

/// Adds `edit` to the edits of the file at `path`, after those it has.
fn add_edit(
    by_file: &mut Vec<FileEdits>,
    path: PathBuf,
    syntax: Syntax,
    edit: Edit,
    parking: bool,
) {
    match by_file.iter_mut().find(|file| file.path == path) {
        Some(file) => {
            file.edits.push(edit);
            file.parking |= parking;
        }
        None => by_file.push(FileEdits {
            path,
            syntax,
            edits: vec![edit],
            parking,
            sqlite: false,
        }),
    }
}

/// Adds `edit` to the edits of the switch file at `path`, edited in
/// `syntax`, a SQLite store when `sqlite`.
fn add_switch_edit(
    by_file: &mut Vec<FileEdits>,
    path: PathBuf,
    syntax: Syntax,
    sqlite: bool,
    edit: Edit,
) {
    add_edit(by_file, path.clone(), syntax, edit, false);
    if let Some(file) = by_file.iter_mut().find(|file| file.path == path) {
        file.sqlite |= sqlite;
    }
}

/// Whether a location that reads the file at `path` keeps comments out of it.
fn keeps_comments_out(definitions: &[AgentDefinition], snapshot: &Snapshot, path: &Path) -> bool {
    snapshot.readers_of(path).iter().any(|reader| {
        definitions[reader.agent_index]
            .location(&reader.location)
            .no_comments
    })
}

/// The syntax the file of the Entry `entry` is edited in, which holds it
/// under a key: the file it was parked from, for a parked Entry.
fn file_syntax(definitions: &[AgentDefinition], entry: &ScannedEntry) -> Syntax {
    definitions[entry.agent_index]
        .location(&entry.location)
        .format
        .file()
        .expect("validation parks keys in files only")
        .syntax()
}

/// The ids of the Entries parked in the store whose bytes are `bytes`.
fn parked_ids(bytes: &[u8]) -> Result<Vec<String>, EditFailure> {
    let file = EditableFile::decode(bytes, Syntax::Jsonc).map_err(EditFailure::ReadOnly)?;
    let document = file.value_at(&[]).map_err(EditFailure::Edit)?;
    Ok(document.as_ref().map(park::ids).unwrap_or_default())
}

/// A location's bytes, `None` when its file or store is missing.
fn read_if_there(ports: &Ports, path: &Path, medium: &Medium) -> Result<Option<Vec<u8>>, Error> {
    match store::read(ports.fs.as_ref(), path, medium) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(Error::blocked(format!(
            "cannot read {}: {error}",
            path.display()
        ))),
    }
}

/// Plans the Undo of the Operation `id` in History: the inverse of each of
/// its edits, last first, applied to each file as it is now, and resolved in
/// the context the Operation ran in (ADR 0003).
pub(crate) fn plan_undo(
    definitions: &Definitions,
    ports: &Ports,
    id: &str,
) -> Result<Planned, Error> {
    let manifests = history::load_all(ports)?;
    let manifest = history::find(&manifests, id)?;
    if let Some(reason) = history::not_undoable(manifest, &manifests) {
        return Err(Error::blocked(format!(
            "Operation {id} cannot be undone: {reason}"
        )));
    }
    let scan_options = ScanOptions {
        projects: manifest.project.iter().cloned().collect(),
    };
    let context = Context {
        project: manifest.project.clone(),
    };
    let scan = |written: &[Written], folders: &[PlannedFolder]| {
        scan::scan_as_if(definitions, ports, &scan_options.projects, written, folders)
            .map_err(Error::unexpected)
    };
    let snapshot = scan(&[], &[])?;
    let mut files = undo_files(ports, manifest)
        .map_err(|blocked| blocked.into_error(definitions, &snapshot))?;
    let mut folders = undo_folders(ports, manifest)?;
    for file in &mut files {
        file.readers = snapshot.readers_of(&file.path);
        file.switches = snapshot.switch_stores_of(&file.path);
        file.git_versioned = snapshot.versioned.contains(&file.path);
    }
    for folder in &mut folders {
        folder.readers = snapshot.readers_of_folder(&folder.path);
    }

    let before = resolve::resolve(definitions, &snapshot, &context);
    let written: Vec<Written> = files.iter().map(PlannedFile::written).collect();
    let after_snapshot = scan(&written, &folders)?;
    let after = resolve::resolve(definitions, &after_snapshot, &context);
    let title = explain::undo_title(id, &manifest.title);
    let (mut changes, effective) = predict(&before, &after);
    // What the Operation changed in another Project, such as a leftover it
    // Removed, changes back there.
    let mut others: Vec<&Option<PathBuf>> = Vec::new();
    for change in &manifest.changes {
        if change.project != manifest.project && !others.contains(&&change.project) {
            others.push(&change.project);
        }
    }
    for project in others {
        let context = Context {
            project: project.clone(),
        };
        let before = resolve::resolve(definitions, &snapshot, &context);
        let after = resolve::resolve(definitions, &after_snapshot, &context);
        changes.extend(predict(&before, &after).0);
    }
    let preview = preview(
        definitions,
        ports,
        Action::Undo,
        title,
        Some(id.to_owned()),
        before.project.clone(),
        changes,
        effective,
        &files,
        &folders,
        &before.read_failures,
        &before.homes,
    )?;
    let homes = RunningHomes::of(definitions, &snapshot);
    let running = running::check(definitions, ports, &homes, &files, &folders);
    Ok(Planned {
        action: Action::Undo,
        entries: preview.changes.len(),
        context,
        scan_options,
        warnings: warnings(&files, &folders, running.warning),
        blocks: running.blocks,
        files,
        folders,
        preview,
        read_failures: before.read_failures,
        homes,
    })
}

/// Plans the deletion of the backup of the Operation `id` in History: the
/// copies it deletes, and when the Operation needs attention, the warning
/// that they are the only copy for manual recovery. It writes no
/// configuration, so it has no context, edits or States.
pub(crate) fn plan_delete_backup(ports: &Ports, id: &str) -> Result<Planned, Error> {
    let manifests = history::load_all(ports)?;
    let manifest = history::find(&manifests, id)?;
    let folder = history::operation_folder(ports, id)?;
    let deleted = history::backup_copies(ports, &folder, manifest);
    if history::backup_already_deleted(manifest, &deleted) {
        return Err(Error::blocked(format!(
            "the backup of Operation {id} was already deleted"
        )));
    }
    let mut warnings = Vec::new();
    if manifest.needs_attention() {
        warnings.push(Warning {
            class: WarningClass::NeedsAttention,
            sentence: explain::only_copy_for_recovery(id),
        });
    }
    let preview = Preview {
        action: Action::DeleteBackup,
        title: explain::delete_backup_title(id, &manifest.title),
        undo_of: None,
        backup_of: Some(id.to_owned()),
        project: None,
        changes: Vec::new(),
        effective: Vec::new(),
        info: Vec::new(),
        files: Vec::new(),
        folders: Vec::new(),
        deleted,
        backup_note: explain::delete_backup_note(id),
        backup_folder: folder,
    };
    Ok(Planned {
        action: Action::DeleteBackup,
        entries: 0,
        context: Context::default(),
        scan_options: ScanOptions::default(),
        files: Vec::new(),
        folders: Vec::new(),
        preview,
        warnings,
        blocks: Vec::new(),
        read_failures: Vec::new(),
        homes: RunningHomes::default(),
    })
}

/// A touched file's bytes for an Undo, and whether it is missing. A missing
/// file whose folder exists is planned from the minimal structure of its
/// syntax, so that the Undo recreates it; one whose folder is gone cannot be.
fn read_or_create(
    ports: &Ports,
    path: &Path,
    syntax: Syntax,
    medium: &Medium,
) -> Result<(Vec<u8>, bool), Error> {
    match store::read(ports.fs.as_ref(), path, medium) {
        Ok(bytes) => Ok((bytes, false)),
        Err(error) if error.kind() == io::ErrorKind::NotFound && !medium.is_file() => {
            Err(Error::blocked(explain::store_missing(path)))
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            match path.parent().filter(|folder| ports.fs.exists(folder)) {
                Some(_) => Ok((empty_file(syntax).to_vec(), true)),
                None => Err(Error::blocked(format!(
                    "the folder of {} is gone, so there is nowhere to put the Entries back",
                    path.display()
                ))),
            }
        }
        Err(error) => Err(Error::blocked(format!(
            "cannot read {}: {error}",
            path.display()
        ))),
    }
}

/// The minimal structure of a file in a syntax, which keys are added to.
fn empty_file(syntax: Syntax) -> &'static [u8] {
    match syntax {
        Syntax::Jsonc => b"{\n}\n",
        Syntax::Toml => b"",
    }
}

/// Why the inverse edits of one file could not be computed.
enum UndoFailure {
    /// The key changed since the Operation wrote it.
    Changed(Vec<String>),
    /// The key the Operation took away exists again.
    Appeared(Vec<String>),
    Other(EditFailure),
}

/// Applies the inverse of each edit of a recorded file, last first, to the
/// file as it is now. Each inverse applies only while every key its edit
/// depends on still holds the value the edit gave it.
fn undo_edits(bytes: &[u8], location: &history::LocationRecord) -> Result<EditedFile, UndoFailure> {
    let read_only = |reason| UndoFailure::Other(EditFailure::ReadOnly(reason));
    let edit_error = |error| UndoFailure::Other(EditFailure::Edit(error));
    let mut file = EditableFile::decode(bytes, location.syntax).map_err(read_only)?;
    let mut edited = bytes.to_vec();
    let mut applied_edits = Vec::new();
    for record in location.edits.iter().rev() {
        for dependency in &record.depends_on {
            let now = file.value_at(&dependency.key_path).map_err(edit_error)?;
            if now == dependency.after {
                continue;
            }
            let key_path = dependency.key_path.clone();
            return Err(match dependency.after {
                None => UndoFailure::Appeared(key_path),
                Some(_) => UndoFailure::Changed(key_path),
            });
        }
        let inverse = with_parents(&file, &record.inverse).map_err(edit_error)?;
        let applied = file.apply(&inverse).map_err(edit_error)?;
        edited = applied.bytes;
        file = EditableFile::decode(&edited, location.syntax).map_err(read_only)?;
        applied_edits.push(AppliedEdit {
            edit: inverse,
            inverse: applied.inverse,
            depends_on: applied.depends_on,
        });
    }
    if let Medium::SqliteItems { keys } = &location.medium {
        edited = store::normalized(&edited, keys)
            .ok_or(UndoFailure::Other(EditFailure::Edit(EditError::Parse)))?;
    }
    Ok(EditedFile {
        bytes: edited,
        edits: applied_edits,
    })
}

/// An append to a missing list becomes a set of the list holding the item.
/// An insertion or a set whose parent objects are missing becomes a set of
/// the outermost missing one, holding the rest, so that the file gets the
/// minimal structure that holds the key.
fn with_parents(file: &EditableFile, edit: &Edit) -> Result<Edit, EditError> {
    let created;
    let edit = match edit {
        Edit::Append { key_path, value } if file.value_at(key_path)?.is_none() => {
            created = Edit::Set {
                key_path: key_path.clone(),
                value: serde_json::json!([value]),
            };
            &created
        }
        edit => edit,
    };
    let (Edit::Insert {
        key_path, value, ..
    }
    | Edit::Set { key_path, value }) = edit
    else {
        return Ok(edit.clone());
    };
    for depth in 1..key_path.len() {
        if file.value_at(&key_path[..depth])?.is_none() {
            let value = key_path[depth..].iter().rev().fold(
                value.clone(),
                |inner, key| serde_json::json!({ key: inner }),
            );
            return Ok(Edit::Set {
                key_path: key_path[..depth].to_vec(),
                value,
            });
        }
    }
    Ok(edit.clone())
}

/// Why an Operation cannot be undone, as [`undo_files`] finds it.
pub(crate) enum UndoBlocked<'m> {
    /// What the Operation touched changed since it ran.
    Refused(Refused<'m>),
    Other(Error),
}

impl UndoBlocked<'_> {
    /// Why, as a sentence.
    pub fn message(&self) -> String {
        match self {
            UndoBlocked::Refused(refused) => refused.message(),
            UndoBlocked::Other(error) => error.message.clone(),
        }
    }

    /// Why, as the error planning the Undo fails with, its diffs masked for
    /// the locations of `snapshot` that read each file.
    fn into_error(self, definitions: &[AgentDefinition], snapshot: &Snapshot) -> Error {
        match self {
            UndoBlocked::Refused(refused) => refused.into_error(definitions, snapshot),
            UndoBlocked::Other(error) => error,
        }
    }
}

/// The error planning the Undo of `manifest` fails with because of its files
/// and skill folders, as [`plan_undo`] returns it, or `None` when they allow
/// it. Its diffs are masked for the locations of `snapshot` that read each
/// file. Why History alone stops it is left to [`history::not_undoable`].
pub(crate) fn undo_refusal(
    definitions: &[AgentDefinition],
    ports: &Ports,
    manifest: &history::Manifest,
    snapshot: &Snapshot,
) -> Option<Error> {
    match undo_files(ports, manifest) {
        Err(blocked) => Some(blocked.into_error(definitions, snapshot)),
        Ok(_) => undo_folders(ports, manifest).err(),
    }
}

impl From<Error> for UndoBlocked<'_> {
    fn from(error: Error) -> Self {
        UndoBlocked::Other(error)
    }
}

/// The files of an Operation with the inverse of its edits applied to each
/// one as it is now, or why its Undo is refused. History runs the same
/// check to say whether the Operation can be undone. The files' readers, the
/// switches in them, and whether each is git-versioned, are left for the
/// caller to fill in.
pub(crate) fn undo_files<'m>(
    ports: &'m Ports,
    manifest: &'m history::Manifest,
) -> Result<Vec<PlannedFile>, UndoBlocked<'m>> {
    let mut files = Vec::new();
    let mut changed = Vec::new();
    for location in &manifest.locations {
        let (read, created) =
            read_or_create(ports, &location.path, location.syntax, &location.medium)?;
        match undo_edits(&read, location) {
            Ok(edited) => files.push(PlannedFile {
                path: location.path.clone(),
                syntax: location.syntax,
                read,
                created,
                edited,
                readers: Vec::new(),
                switches: Vec::new(),
                git_versioned: false,
                parking: location.parking,
                parked_store: false,
                parked_readers: Vec::new(),
                medium: location.medium.clone(),
            }),
            Err(UndoFailure::Changed(key_path)) => changed.push(ChangedFile {
                location,
                key_path,
                current: (!created).then_some(read),
            }),
            Err(UndoFailure::Appeared(key_path)) => {
                return Err(Error::blocked(format!(
                    "`{}` in {} appeared since Operation {}, and the Undo would overwrite it",
                    key_path.join("."),
                    location.path.display(),
                    manifest.id
                ))
                .into());
            }
            Err(UndoFailure::Other(failure)) => {
                return Err(failure.into_error(&location.path).into());
            }
        }
    }
    if !changed.is_empty() {
        return Err(UndoBlocked::Refused(Refused {
            folder: history::operation_folder(ports, &manifest.id)?,
            changed,
            ports,
        }));
    }
    Ok(files)
}

/// The skill folders and links of an Operation, each with the inverse of
/// what it did there, last first, or why its Undo is refused: something is
/// at a location it emptied, its copy in the backup or the parked-Entry
/// store is gone, or what it put back changed since. History runs the same
/// check. The folders' readers are left for the caller to fill in.
pub(crate) fn undo_folders(
    ports: &Ports,
    manifest: &history::Manifest,
) -> Result<Vec<PlannedFolder>, Error> {
    let fs = ports.fs.as_ref();
    let folder = history::operation_folder(ports, &manifest.id)?;
    let refused = |what: String| Error::undo_refused(what, folder.clone(), Vec::new());
    let mut planned = Vec::new();
    for record in manifest.folders.iter().rev() {
        let path = &record.path;
        let (op, size) = match &record.op {
            FolderOpRecord::MovedToBackup { backup } => {
                let from = folder.join(backup);
                if !fs.exists(&from) {
                    return Err(refused(format!(
                        "the copy of {} in the backup is gone",
                        path.display()
                    )));
                }
                (FolderOp::MoveBack { from }, record.size)
            }
            FolderOpRecord::LinkRemoved { target } => (
                FolderOp::CreateLink {
                    target: target.clone(),
                },
                record.size,
            ),
            FolderOpRecord::MovedBack { .. } => {
                if !folders::holds(fs, path, record.size) {
                    return Err(refused(format!(
                        "the skill folder {} changed since the Operation",
                        path.display()
                    )));
                }
                (FolderOp::MoveToBackup, record.size)
            }
            FolderOpRecord::LinkCreated { target } => {
                if !folders::links_to(fs, path, target) {
                    return Err(refused(format!(
                        "the link {} changed since the Operation",
                        path.display()
                    )));
                }
                let target = target.clone();
                (FolderOp::RemoveLink { target }, record.size)
            }
            FolderOpRecord::Parked { to } => {
                if !fs.exists(to) {
                    return Err(refused(format!(
                        "the copy of {} in the parked-Entry store is gone",
                        path.display()
                    )));
                }
                (FolderOp::Unpark { from: to.clone() }, record.size)
            }
            FolderOpRecord::Unparked { from } => {
                if !folders::holds(fs, path, record.size) {
                    return Err(refused(format!(
                        "the skill folder {} changed since the Operation",
                        path.display()
                    )));
                }
                if folders::taken(fs, from) {
                    return Err(refused(format!(
                        "something is at {} again since the Operation",
                        from.display()
                    )));
                }
                (FolderOp::Park { to: from.clone() }, record.size)
            }
        };
        let empties = matches!(
            record.op,
            FolderOpRecord::MovedToBackup { .. }
                | FolderOpRecord::LinkRemoved { .. }
                | FolderOpRecord::Parked { .. }
        );
        if empties && folders::taken(fs, path) {
            return Err(refused(format!(
                "something is at {} again since the Operation",
                path.display()
            )));
        }
        planned.push(PlannedFolder {
            git_versioned: ports.git.is_versioned(path),
            readers: Vec::new(),
            path: path.clone(),
            op,
            size,
        });
    }
    Ok(planned)
}

/// A touched file whose Undo is refused.
struct ChangedFile<'m> {
    location: &'m history::LocationRecord,
    /// The first key found changed.
    key_path: Vec<String>,
    /// What the file holds now, `None` when it was deleted.
    current: Option<Vec<u8>>,
}

/// The refusal of an Undo whose files changed since the Operation.
pub(crate) struct Refused<'m> {
    /// The Operation's folder, which holds its backup.
    folder: PathBuf,
    changed: Vec<ChangedFile<'m>>,
    ports: &'m Ports,
}

impl Refused<'_> {
    /// What changed, as a sentence.
    fn message(&self) -> String {
        Error::undo_refused_message(&self.what(), &self.folder)
    }

    fn what(&self) -> String {
        let what: Vec<String> = self
            .changed
            .iter()
            .map(|file| match file.current {
                Some(_) => format!(
                    "`{}` in {} changed",
                    file.key_path.join("."),
                    file.location.path.display()
                ),
                None => format!("{} was deleted", file.location.path.display()),
            })
            .collect();
        format!("{} since the Operation", what.join("; "))
    }

    /// The refusal, with a diff from each backed-up file to what it holds
    /// now, masked for the locations of `snapshot` that read it.
    fn into_error(self, definitions: &[AgentDefinition], snapshot: &Snapshot) -> Error {
        let diffs = self
            .changed
            .iter()
            .map(|file| {
                let path = &file.location.path;
                // A file the Operation created had nothing to back up.
                let backed_up = match &file.location.backup {
                    Some(backup) => {
                        let backup = self.folder.join(backup);
                        match self.ports.fs.read(&backup) {
                            Ok(bytes) => Some(bytes),
                            Err(error) => return unreadable_backup(path, &backup, &error),
                        }
                    }
                    None => None,
                };
                let before = Side {
                    bytes: backed_up.as_deref(),
                    name: "in the backup",
                };
                let after = Side {
                    bytes: file.current.as_deref(),
                    name: "as it is now",
                };
                let readers = snapshot.readers_of(path);
                let store = snapshot
                    .parked_store
                    .as_deref()
                    .map(|store| (store, &*snapshot.readers));
                file_diff(definitions, &readers, path, store, before, after)
            })
            .collect();
        Error::undo_refused(self.what(), self.folder, diffs)
    }
}

/// What an Operation changed in one file it touched: a diff from its
/// backup, or from nothing when it created the file, to what its edits made
/// of that backup, masked for the locations of `snapshot` that read it now.
pub(crate) fn operation_diff(
    definitions: &[AgentDefinition],
    ports: &Ports,
    snapshot: &Snapshot,
    folder: &Path,
    manifest: &history::Manifest,
    location: &history::LocationRecord,
) -> FileDiff {
    let path = &location.path;
    if manifest.backup_deleted_at.is_some() {
        let note = explain::diff_of_deleted_backup(&manifest.id);
        return no_diff(path, DiffUnavailable::BackupDeleted, note);
    }
    let backed_up = match &location.backup {
        Some(backup) => {
            let backup = folder.join(backup);
            match ports.fs.read(&backup) {
                Ok(bytes) => Some(bytes),
                Err(error) => return unreadable_backup(path, &backup, &error),
            }
        }
        None => None,
    };
    let base = backed_up.as_deref().unwrap_or(empty_file(location.syntax));
    let edits: Vec<Edit> = location
        .edits
        .iter()
        .map(|record| record.edit.clone())
        .collect();
    let written = match apply_edits(base, location.syntax, &edits, &location.medium) {
        Ok(edited) => edited.bytes,
        Err(_) => {
            let note = explain::edits_do_not_apply(path);
            return no_diff(path, DiffUnavailable::EditsDoNotApply, note);
        }
    };
    let before = Side {
        bytes: backed_up.as_deref(),
        name: "in the backup",
    };
    let after = Side {
        bytes: Some(&written),
        name: "as the Operation wrote it",
    };
    let readers = snapshot.readers_of(path);
    let store = snapshot
        .parked_store
        .as_deref()
        .map(|store| (store, &*snapshot.readers));
    file_diff(definitions, &readers, path, store, before, after)
}

/// No diff of a file whose backup cannot be read.
fn unreadable_backup(path: &Path, backup: &Path, error: &io::Error) -> FileDiff {
    let note = format!(
        "Can't show a diff: the backup {} cannot be read: {error}.",
        backup.display()
    );
    no_diff(path, DiffUnavailable::Io, note)
}

/// No diff of the file at `path`, for `reason`, said by `note`.
fn no_diff(path: &Path, reason: DiffUnavailable, note: String) -> FileDiff {
    FileDiff {
        path: path.to_path_buf(),
        diff: None,
        diff_unavailable: Some(reason),
        diff_note: Some(note),
        // Nothing says how many lines differ without both sides.
        changed_lines: 0,
    }
}

/// What a plan shows before it is confirmed.
#[allow(clippy::too_many_arguments)]
fn preview(
    definitions: &[AgentDefinition],
    ports: &Ports,
    action: Action,
    title: String,
    undo_of: Option<String>,
    project: Option<PathBuf>,
    changes: Vec<Change>,
    effective: Vec<EffectiveChange>,
    files: &[PlannedFile],
    folders: &[PlannedFolder],
    read_failures: &[ReadFailure],
    homes: &[Home],
) -> Result<Preview, Error> {
    let backup_folder = history::backups_folder(&history::data_folder(ports.env.as_ref())?);
    refuse_read_failure_targets(read_failures, &changes, files)?;
    let mut info = read_failure_info(read_failures, &changes, files);
    info.extend(version_info(definitions, homes, &changes, files));
    Ok(Preview {
        action,
        title,
        undo_of,
        backup_of: None,
        project,
        changes,
        effective,
        info,
        files: files
            .iter()
            .map(|file| {
                let edits = file
                    .edited
                    .edits
                    .iter()
                    .map(|applied| EditSummary::from(&applied.edit))
                    .collect();
                let diff = file.diff(
                    definitions,
                    Side {
                        bytes: (!file.created).then_some(file.read.as_slice()),
                        name: "before the Operation",
                    },
                    Side {
                        bytes: Some(&file.edited.bytes),
                        name: "after the Operation",
                    },
                );
                FileChange::new(
                    edits,
                    diff,
                    file.git_versioned,
                    file.store_class(definitions).into(),
                )
            })
            .collect(),
        folders: folders.iter().map(PlannedFolder::change).collect(),
        deleted: Vec::new(),
        backup_note: explain::backup_note(&backup_folder),
        backup_folder,
    })
}

/// Refuses a plan that must write a file in `files` that is one of
/// `read_failures` for an Agent home whose States it changes in `changes`,
/// or that cannot be opened or decoded at all: config-digger never writes a
/// file it cannot read.
fn refuse_read_failure_targets(
    read_failures: &[ReadFailure],
    changes: &[Change],
    files: &[PlannedFile],
) -> Result<(), Error> {
    for failure in read_failures {
        if !files.iter().any(|file| file.path == failure.path) {
            continue;
        }
        let acting = changes
            .iter()
            .any(|change| change.agent == failure.agent && change.home == failure.home);
        if acting || failure.kind != ReadFailureKind::Parse {
            return Err(Error::blocked(explain::read_failure_target(
                &failure.path,
                &failure.sentence,
            )));
        }
    }
    Ok(())
}

/// What the preview of a plan that writes `files` and changes the States
/// `changes` says about `read_failures`, with nothing to acknowledge: that
/// another Agent still rejects a file it writes, and that a Read failure
/// which could change the outcome may make the States after differ.
fn read_failure_info(
    read_failures: &[ReadFailure],
    changes: &[Change],
    files: &[PlannedFile],
) -> Vec<Info> {
    let mut info = Vec::new();
    for failure in read_failures {
        if files.iter().any(|file| file.path == failure.path) {
            info.push(Info {
                code: InfoCode::StillRejected,
                path: failure.path.clone(),
                sentence: explain::still_rejected(
                    &failure.agent_name,
                    &failure.path,
                    &failure.sentence,
                ),
            });
            continue;
        }
        let could_change = changes.iter().any(|change| {
            change.agent == failure.agent
                && change.home == failure.home
                && failure.reaches(change.project.as_deref())
        });
        if could_change {
            info.push(Info {
                code: InfoCode::ReadFailure,
                path: failure.path.clone(),
                sentence: explain::outcome_may_differ(&failure.agent_name, &failure.path),
            });
        }
    }
    info
}

/// What the preview of a plan that writes `files` and changes the States
/// `changes` says about the versions of the Agents it touches, with nothing
/// to acknowledge: each note of each Agent home among `homes` whose States
/// it changes, or that reads or keeps switches in a file it writes, once.
fn version_info(
    definitions: &[AgentDefinition],
    homes: &[Home],
    changes: &[Change],
    files: &[PlannedFile],
) -> Vec<Info> {
    let readers = files.iter().flat_map(|file| {
        let locations = file
            .readers
            .iter()
            .map(|reader| (reader.agent_index, &reader.home));
        let switches = file
            .switches
            .iter()
            .map(|store| (store.agent_index, &store.home));
        locations
            .chain(switches)
            .map(|(agent_index, home)| (definitions[agent_index].agent.id.as_str(), home.as_str()))
    });
    let touched: Vec<(&str, &str)> = changes
        .iter()
        .map(|change| (change.agent.as_str(), change.home.as_str()))
        .chain(readers)
        .collect();
    let mut info: Vec<Info> = Vec::new();
    for home in homes {
        if !touched.contains(&(home.agent.as_str(), home.home.as_str())) {
            continue;
        }
        for note in &home.notes {
            let Some(code) = note.code.info() else {
                continue;
            };
            if info.iter().any(|known| known.sentence == note.sentence) {
                continue;
            }
            info.push(Info {
                code,
                path: note
                    .installation
                    .clone()
                    .unwrap_or_else(|| home.folder.clone()),
                sentence: note.sentence.clone(),
            });
        }
    }
    info
}

/// What a plan that edits `files` and touches `folders` must have
/// acknowledged: one warning for the git-versioned ones among them, and
/// `running`, the warning for the running Agents that read them.
fn warnings(
    files: &[PlannedFile],
    folders: &[PlannedFolder],
    running: Option<Warning>,
) -> Vec<Warning> {
    let versioned: Vec<&Path> = files
        .iter()
        .filter(|file| file.git_versioned)
        .map(|file| file.path.as_path())
        .chain(
            folders
                .iter()
                .filter(|folder| folder.git_versioned)
                .map(|folder| folder.path.as_path()),
        )
        .collect();
    let versioned = (!versioned.is_empty()).then(|| Warning {
        class: WarningClass::GitVersioned,
        sentence: explain::git_versioned(&versioned),
    });
    versioned.into_iter().chain(running).collect()
}

/// How an Entry is Removed.
enum Removal {
    /// Its key goes from a file edited in this syntax.
    Key(Syntax),
    /// Its skill folder goes into the backup, or its link away.
    Folder,
}

/// How the Entry `entry` is Removed, or why it cannot be: config-digger
/// only reads it.
fn removal(
    definitions: &[AgentDefinition],
    snapshot: &Snapshot,
    entry: &api::Entry,
) -> Result<Removal, Error> {
    if let api::RemoveOption::NotOffered {
        code,
        sentence,
        how_to_remove,
    } = &entry.remove
    {
        let message = match code {
            api::RemoveNotOffered::ReadOnly => explain::read_only_remove(&entry.locator),
            api::RemoveNotOffered::Mirrored => {
                let scanned = snapshot.entry(&entry.path, &entry.key_path);
                let mirror = snapshot
                    .mirror_of(scanned)
                    .expect("a mirrored Entry is in a Mirrored home");
                explain::mirrored_refused(
                    "Remove",
                    &entry.locator,
                    &mirror.owner,
                    mirror.source_folder.as_deref(),
                )
            }
            api::RemoveNotOffered::Plugin | api::RemoveNotOffered::Extension => {
                let how = how_to_remove
                    .as_ref()
                    .map_or("", |how| how.sentence.as_str());
                explain::plugin_remove_refused(&entry.locator, sentence, how)
            }
        };
        return Err(Error::blocked(message));
    }
    let scanned = snapshot.entry(&entry.path, &entry.key_path);
    // A parked Entry goes from the store, whatever file it came from.
    if snapshot.parked_of(scanned).is_some() {
        return Ok(Removal::Key(Syntax::Jsonc));
    }
    let location = definitions[scanned.agent_index].location(&scanned.location);
    Ok(match location.format.file() {
        Some(format) => Removal::Key(format.syntax()),
        None => Removal::Folder,
    })
}

/// A touched file's bytes. It held the Entry when it was scanned, so a file
/// that is gone or unreadable now changed since.
pub(crate) fn read(ports: &Ports, path: &Path) -> Result<Vec<u8>, Error> {
    ports.fs.read(path).map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => Error::stale(format!("{} is gone", path.display())),
        _ => Error::blocked(format!("cannot read {}: {error}", path.display())),
    })
}

/// A touched location's bytes, as [`read`] reads a file's.
pub(crate) fn read_location(ports: &Ports, path: &Path, medium: &Medium) -> Result<Vec<u8>, Error> {
    store::read(ports.fs.as_ref(), path, medium).map_err(|error| match error.kind() {
        io::ErrorKind::NotFound => Error::stale(format!("{} is gone", path.display())),
        _ => Error::blocked(format!("cannot read {}: {error}", path.display())),
    })
}

/// Applies `edits` to a location's bytes one after the other. The values
/// of a store come out laid out as reading them back lays them out.
pub(crate) fn apply_edits(
    bytes: &[u8],
    syntax: Syntax,
    edits: &[Edit],
    medium: &Medium,
) -> Result<EditedFile, EditFailure> {
    let mut file = EditableFile::decode(bytes, syntax).map_err(EditFailure::ReadOnly)?;
    let mut edited = bytes.to_vec();
    let mut applied_edits = Vec::new();
    for edit in edits {
        // A key a switch adds may need parents the file does not have yet.
        let edit = with_parents(&file, edit).map_err(EditFailure::Edit)?;
        let applied = file.apply(&edit).map_err(EditFailure::Edit)?;
        edited = applied.bytes;
        file = EditableFile::decode(&edited, syntax).map_err(EditFailure::ReadOnly)?;
        applied_edits.push(AppliedEdit {
            edit,
            inverse: applied.inverse,
            depends_on: applied.depends_on,
        });
    }
    if let Medium::SqliteItems { keys } = medium {
        edited = store::normalized(&edited, keys).ok_or(EditFailure::Edit(EditError::Parse))?;
    }
    Ok(EditedFile {
        bytes: edited,
        edits: applied_edits,
    })
}

impl EditFailure {
    pub(crate) fn into_error(self, path: &Path) -> Error {
        match self {
            EditFailure::ReadOnly(reason) => {
                Error::blocked(format!("cannot write {}: {reason}", path.display()))
            }
            EditFailure::Edit(EditError::NotFound { key_path }) => Error::stale(format!(
                "`{}` is no longer in {}",
                key_path.join("."),
                path.display()
            )),
            EditFailure::Edit(EditError::Parse) => {
                Error::stale(format!("{} no longer parses", path.display()))
            }
            EditFailure::Edit(error) => {
                Error::blocked(format!("cannot edit {} safely: {error}", path.display()))
            }
        }
    }
}

/// The States an Operation changes, from the Items before it to the Items
/// after it, and for each changed Item with Entries on both sides, which
/// Entry is Effective afterwards.
fn predict(before: &Inventory, after: &Inventory) -> (Vec<Change>, Vec<EffectiveChange>) {
    let same = |a: &api::Item, b: &api::Item| {
        a.agent == b.agent && a.home == b.home && a.kind == b.kind && a.name == b.name
    };
    let added = after
        .items
        .iter()
        .filter(|item| !before.items.iter().any(|known| same(known, item)));
    let mut changes = Vec::new();
    let mut effective = Vec::new();
    for item in before.items.iter().chain(added) {
        let item_before = before.items.iter().find(|known| same(known, item));
        let item_after = after.items.iter().find(|known| same(known, item));
        let entries_before = item_before.map_or(&[][..], |item| item.entries.as_slice());
        let entries_after = item_after.map_or(&[][..], |item| item.entries.as_slice());
        // Rows follow the precedence of the side with more Entries, which is
        // the one that has them all when the Operation only takes Entries
        // away or only puts them back.
        let (first, second) = if entries_after.len() > entries_before.len() {
            (entries_after, entries_before)
        } else {
            (entries_before, entries_after)
        };
        let rows = first.iter().chain(
            second
                .iter()
                .filter(|entry| !first.iter().any(|row| row.identity() == entry.identity())),
        );
        let agent_name = before
            .homes
            .iter()
            .chain(&after.homes)
            .find(|home| home.agent == item.agent)
            .map_or(item.agent.as_str(), |home| home.agent_name.as_str());
        let state = |entries: &[api::Entry], locator: &str| {
            entries
                .iter()
                .find(|entry| entry.identity() == locator)
                .map_or(StateAfter::Removed, |entry| entry.state.into())
        };
        let mut changed = false;
        for entry in rows {
            let state_before = state(entries_before, entry.identity());
            let state_after = state(entries_after, entry.identity());
            if state_before == state_after {
                continue;
            }
            changed = true;
            changes.push(Change {
                agent: item.agent.clone(),
                agent_name: agent_name.to_owned(),
                home: item.home.clone(),
                item: item.name.clone(),
                path: entry.identity_path().to_path_buf(),
                locator: entry.identity().to_owned(),
                project: before.project.clone(),
                before: state_before,
                after: state_after,
            });
        }
        if let (true, Some(item_before), Some(item_after)) = (changed, item_before, item_after) {
            let effective_in = |item: &api::Item| {
                item.entries
                    .iter()
                    .find(|entry| entry.state == State::Effective)
                    .map(|entry| entry.locator.clone())
            };
            let before_locator = effective_in(item_before);
            let after_locator = effective_in(item_after);
            let merges = item_before.built.is_some() || item_after.built.is_some();
            let (built_before, built_after) = if merges {
                (built_by(item_before), built_by(item_after))
            } else {
                (None, None)
            };
            effective.push(EffectiveChange {
                built_before,
                built_after,
                agent: item.agent.clone(),
                home: item.home.clone(),
                item: item.name.clone(),
                sentence: explain::effective_after(
                    &item.name,
                    agent_name,
                    after_locator.as_deref(),
                    before_locator == after_locator,
                ),
                before: before_locator,
                after: after_locator,
            });
        }
    }
    (changes, effective)
}

/// The server the Agent uses for `item`: the one it builds from several
/// Entries, or else the Effective Entry's, every field from it.
fn built_by(item: &api::Item) -> Option<api::BuiltServer> {
    if let Some(built) = &item.built {
        return Some(built.clone());
    }
    let entry = item
        .entries
        .iter()
        .find(|entry| entry.state == State::Effective)?;
    let mut origins = Vec::new();
    leaf_fields(&entry.config, &mut Vec::new(), &mut |key_path| {
        origins.push(api::FieldOrigin {
            key_path,
            entry: entry.locator.clone(),
        });
    });
    Some(api::BuiltServer {
        config: entry.config.clone(),
        masked: entry.masked.clone(),
        origins,
    })
}

/// Calls `visit` with the key path of each value below `value` that is not
/// an object, and of each empty object, in order: the fields a built
/// server's origins name.
fn leaf_fields(
    value: &serde_json::Value,
    at: &mut Vec<String>,
    visit: &mut impl FnMut(Vec<String>),
) {
    match value.as_object() {
        Some(object) if !object.is_empty() => {
            for (key, child) in object {
                at.push(key.clone());
                leaf_fields(child, at, visit);
                at.pop();
            }
        }
        _ if !at.is_empty() => visit(at.clone()),
        _ => {}
    }
}

/// The Item of an Agent home that has an Entry at `locator`.
fn item_in<'i>(
    inventory: &'i Inventory,
    agent: &str,
    home: &str,
    locator: &str,
) -> Option<&'i api::Item> {
    inventory.items.iter().find(|item| {
        item.agent == agent
            && item.home == home
            && item.entries.iter().any(|entry| entry.identity() == locator)
    })
}

/// The State of the Entry at `locator` in an Agent home, `Removed` when it is not there.
pub(crate) fn state_in(
    inventory: &Inventory,
    agent: &str,
    home: &str,
    locator: &str,
) -> StateAfter {
    item_in(inventory, agent, home, locator)
        .and_then(|item| {
            item.entries
                .iter()
                .find(|entry| entry.identity() == locator)
        })
        .map_or(StateAfter::Removed, |entry| entry.state.into())
}
