import { useEffect, useState, type ReactNode } from "react";
import {
  ADD_LABELS,
  addHomeOffer,
  addProjectOffer,
  homeToCheck,
  NO_HOME_CHOSEN,
  projectToCheck,
  type Checked,
  type HomeToAdd,
} from "./adding";
import { commands, type Agent, type ListChange, type Pick, type Places, type Project } from "./bindings";
import { call, NOTICE_MS, type CommandResult, type Notify } from "./desktop";
import { Icon } from "./icons";
import {
  changedSentence,
  homeGroups,
  projectSections,
  type HiddenRow,
  type HomeGroup,
  type HomeRow,
  type MissingRow,
  type ProjectRow,
  type ShowInInventory,
} from "./places";
import { useActionDialog } from "./useActionDialog";
import { reads } from "./reads";
import { Select } from "./Select";
import { usePlaces } from "./usePlaces";
import { useTransient } from "./useTransient";

export type Tab = "projects" | "homes";

type Props = {
  /** Opens the Inventory on a Project, a missing Project's leftovers or an Agent home. */
  onShow: (show: ShowInInventory) => void;
  onOpenHistory: () => void;
  /** Opens on the Agent homes tab, with its add dialog when `adding`. */
  start?: { tab: Tab; adding: boolean };
};

/**
 * The Projects & homes screen: the Projects and the Agent homes config-digger
 * reads, in two tabs that mirror `projects` and `homes` in the CLI.
 */
export function ProjectsScreen({ onShow, onOpenHistory, start }: Props) {
  const { places, error, reload, change } = usePlaces();
  const [tab, setTab] = useState<Tab>(start?.tab ?? "projects");
  const [adding, setAdding] = useState<Tab | null>(start?.adding ? start.tab : null);
  const [notice, setNotice] = useTransient<string>(NOTICE_MS);
  // Remove leftovers is an Operation, previewed like any other.
  const dialog = useActionDialog({ onClosed: reload, onOpenHistory, notify: setNotice });

  /** Applies a change to config-digger's lists and says what it did; returns why it was refused, if it was. */
  const apply = async (request: ListChange): Promise<string | null> => {
    const result = await change(request);
    if (result.status === "error") return result.error.message;
    setNotice(changedSentence(request, result.data.listed));
    return null;
  };
  /** Applies a change from a row, whose refusal shows as a notice. */
  const act = (request: ListChange) =>
    void apply(request).then((refused) => {
      if (refused !== null) setNotice(refused);
    });

  const projectCount = places?.projects.length ?? 0;
  const homeCount = places?.agents.reduce((sum, agent) => sum + agent.homes.length, 0) ?? 0;
  return (
    <div className="places">
      <header className="panel banner">
        <div>
          <div className="caption">Where config-digger looks</div>
          <div className="banner-title">Projects &amp; homes</div>
        </div>
        <span className="segmented places-tabs" role="tablist" aria-label="Projects or Agent homes">
          {(["projects", "homes"] as const).map((each) => (
            <button
              type="button"
              key={each}
              role="tab"
              className={tab === each ? "on" : ""}
              aria-selected={tab === each}
              onClick={() => setTab(each)}
            >
              {each === "projects" ? `Projects · ${projectCount}` : `Agent homes · ${homeCount}`}
            </button>
          ))}
        </span>
        <p className="places-about dim">
          {tab === "projects"
            ? "Projects come from the Agents' own registries plus the folders you add. config-digger never scans the disk."
            : "Each Agent reads its user configuration from one or more homes. Entries in different homes never compete."}
        </p>
        <span className="grow" />
        <button type="button" className="cta" onClick={() => setAdding(tab)}>
          <Icon name="folder" small />
          {ADD_LABELS[tab]}
        </button>
      </header>

      <div className="panes">
        <section className="panel places-list">
          {error && (
            <div className="alert" role="alert">
              <Icon name="flag" small />
              The Projects and Agent homes couldn't be read: {error.message}
            </div>
          )}
          <div className="scroll">
            {places === null ? (
              !error && <p className="empty">Reading the Projects and Agent homes…</p>
            ) : tab === "projects" ? (
              <ProjectsTab
                places={places}
                onShow={onShow}
                onChange={act}
                onRemoveLeftovers={(path) => dialog.open({ kind: "leftovers", project: path })}
              />
            ) : (
              <HomesTab places={places} onShow={onShow} onChange={act} />
            )}
          </div>
        </section>
        {tab === "projects" ? <ProjectsSide places={places} /> : <HomesSide />}
      </div>

      {adding === "projects" && <AddProjectDialog onAdd={apply} onClose={() => setAdding(null)} notify={setNotice} />}
      {adding === "homes" && (
        <AddHomeDialog agents={places?.agents ?? []} onAdd={apply} onClose={() => setAdding(null)} notify={setNotice} />
      )}

      {notice && (
        <div className="notice" role="status">
          {notice}
        </div>
      )}

      {dialog.view}
    </div>
  );
}

type ProjectsTabProps = {
  places: Places;
  onShow: (show: ShowInInventory) => void;
  onChange: (change: ListChange) => void;
  onRemoveLeftovers: (path: string) => void;
};

/** The Projects, No Project, the missing Projects (open by default) and the hidden ones (collapsed). */
function ProjectsTab({ places, onShow, onChange, onRemoveLeftovers }: ProjectsTabProps) {
  const [missingOpen, setMissingOpen] = useState(true);
  const [hiddenOpen, setHiddenOpen] = useState(false);
  const { present, missing, hidden } = projectSections(places);
  const hide = (path: string) => onChange({ kind: "hide_project", path });
  const forget = (path: string) => onChange({ kind: "forget_project", path });
  return (
    <>
      <GroupTitle title="Projects" count={present.length} />
      {present.map((row) => (
        <ProjectLine key={row.path} row={row} onShow={onShow} onHide={hide} onForget={forget} />
      ))}
      <div className="place-row informational">
        <span className="place-icon">
          <Icon name="users" small />
        </span>
        <div className="place-text">
          <div className="place-name">No Project</div>
          <div className="place-sub">
            Always available in the Inventory: an Agent started outside any Project, where only user, managed, plugin and
            connector Scopes compete.
          </div>
        </div>
      </div>

      {missing.length > 0 && (
        <>
          <GroupTitle title="Missing" count={missing.length} open={missingOpen} onToggle={() => setMissingOpen(!missingOpen)} />
          {missingOpen &&
            missing.map((row) => (
              <MissingLine key={row.path} row={row} onShow={onShow} onHide={hide} onRemoveLeftovers={onRemoveLeftovers} />
            ))}
        </>
      )}

      {hidden.length > 0 && (
        <>
          <GroupTitle title="Hidden" count={hidden.length} open={hiddenOpen} onToggle={() => setHiddenOpen(!hiddenOpen)} />
          {hiddenOpen &&
            hidden.map((row) => (
              <HiddenLine
                key={row.path}
                row={row}
                onUnhide={(path) => onChange({ kind: "unhide_project", path })}
                onForget={forget}
              />
            ))}
        </>
      )}
    </>
  );
}

/** A section title, with its count; a collapsible one toggles on click. */
function GroupTitle({ title, count, open, onToggle }: { title: string; count: number; open?: boolean; onToggle?: () => void }) {
  const label = `${title} · ${count}`;
  return (
    <h3 className="place-group">
      {onToggle ? (
        <button type="button" className="place-group-toggle" aria-expanded={open} onClick={onToggle}>
          {open ? "▾" : "▸"} {label}
        </button>
      ) : (
        label
      )}
      <span className="place-group-line" />
    </h3>
  );
}

/** One row: an icon, its text, an optional count and its buttons. */
function PlaceRow({
  icon,
  warn = false,
  dim = false,
  children,
  count,
  actions,
}: {
  icon: "folder" | "doc" | "flag";
  warn?: boolean;
  dim?: boolean;
  children: ReactNode;
  count?: string;
  actions: ReactNode;
}) {
  return (
    <div className={`place-row${dim ? " dimmed" : ""}`}>
      <span className={`place-icon${warn ? " warn" : ""}`}>
        <Icon name={icon} small />
      </span>
      <div className="place-text">{children}</div>
      {count !== undefined && <span className="place-count">{count}</span>}
      <div className="row-buttons place-actions">{actions}</div>
    </div>
  );
}

/** A small button on a row: secondary, or red for Remove leftovers. */
function SmallButton({ label, danger = false, onClick }: { label: ReactNode; danger?: boolean; onClick: () => void }) {
  return (
    <button type="button" className={`cta small ${danger ? "danger" : "secondary"}`} onClick={onClick}>
      {label}
    </button>
  );
}

type ProjectLineProps = {
  row: ProjectRow;
  onShow: (show: ShowInInventory) => void;
  onHide: (path: string) => void;
  onForget: (path: string) => void;
};

/** A Project's icon: a document for a workspace file, a folder otherwise. */
function projectIcon(project: Project): "doc" | "folder" {
  return project.workspace_file ? "doc" : "folder";
}

/** A Project that exists: Open in Inventory, and Hide or Remove from list. */
function ProjectLine({ row, onShow, onHide, onForget }: ProjectLineProps) {
  return (
    <PlaceRow
      icon={projectIcon(row.project)}
      count={row.entries}
      actions={
        <>
          <SmallButton label="Open in Inventory" onClick={() => onShow({ kind: "project", path: row.path })} />
          {row.offer === "remove_from_list" ? (
            <SmallButton label="Remove from list" onClick={() => onForget(row.path)} />
          ) : (
            <SmallButton label="Hide" onClick={() => onHide(row.path)} />
          )}
        </>
      }
    >
      <div className="place-name mono">{row.path}</div>
      <div className="place-sub">
        {row.kind} · {row.source}
      </div>
    </PlaceRow>
  );
}

type MissingLineProps = {
  row: MissingRow;
  onShow: (show: ShowInInventory) => void;
  onHide: (path: string) => void;
  onRemoveLeftovers: (path: string) => void;
};

/** A missing Project with its leftovers: Show leftovers and Remove leftovers, or Hide when it has none. */
function MissingLine({ row, onShow, onHide, onRemoveLeftovers }: MissingLineProps) {
  return (
    <PlaceRow
      icon="flag"
      warn
      count={row.count}
      actions={
        row.hideOffered ? (
          <SmallButton label="Hide" onClick={() => onHide(row.path)} />
        ) : (
          <>
            <SmallButton label="Show leftovers" onClick={() => onShow({ kind: "leftovers", path: row.path })} />
            <SmallButton
              danger
              label={
                <>
                  <Icon name="trash" small />
                  Remove leftovers
                </>
              }
              onClick={() => onRemoveLeftovers(row.path)}
            />
          </>
        )
      }
    >
      <div className="place-name mono">{row.path}</div>
      <div className="place-sub">{row.source}</div>
      {row.leftovers.length > 0 && (
        <ul className="place-leftovers mono">
          {row.leftovers.map((leftover) => (
            <li key={leftover}>{leftover}</li>
          ))}
        </ul>
      )}
    </PlaceRow>
  );
}

/** A hidden Project, dimmed: Unhide, and Remove from list for the user's own. */
function HiddenLine({ row, onUnhide, onForget }: { row: HiddenRow; onUnhide: (path: string) => void; onForget: (path: string) => void }) {
  return (
    <PlaceRow
      icon={projectIcon(row.project)}
      dim
      actions={
        <>
          <SmallButton label="Unhide" onClick={() => onUnhide(row.path)} />
          {row.removable && <SmallButton label="Remove from list" onClick={() => onForget(row.path)} />}
        </>
      }
    >
      <div className="place-name mono">{row.path}</div>
      <div className="place-sub">{row.source}</div>
    </PlaceRow>
  );
}

/** Every Agent's homes, each Agent headed by its Installations' versions. */
function HomesTab({ places, onShow, onChange }: { places: Places; onShow: (show: ShowInInventory) => void; onChange: (change: ListChange) => void }) {
  return (
    <>
      {homeGroups(places).map((group) => (
        <HomeGroupView key={group.agent} group={group} onShow={onShow} onChange={onChange} />
      ))}
    </>
  );
}

/** One Agent's header, version notes and homes. */
function HomeGroupView({
  group,
  onShow,
  onChange,
}: {
  group: HomeGroup;
  onShow: (show: ShowInInventory) => void;
  onChange: (change: ListChange) => void;
}) {
  return (
    <>
      <h3 className="place-group agent-group">
        {group.name}
        <span className="agent-version">
          installed {group.installed.join(", ")} · {group.definition}
        </span>
        <span className="place-group-line" />
      </h3>
      {group.notes.map((note) => (
        <p key={note} className="agent-note">
          <Icon name="flag" small />
          {note}
        </p>
      ))}
      {group.homes.length === 0 && <p className="empty">No Agent home found.</p>}
      {group.homes.map((row) => (
        <HomeLine key={row.home.home} row={row} onShow={onShow} onChange={onChange} />
      ))}
    </>
  );
}

/** One Agent home with its tags, notes and Entry count, and the actions it offers. */
function HomeLine({ row, onShow, onChange }: { row: HomeRow; onShow: (show: ShowInInventory) => void; onChange: (change: ListChange) => void }) {
  const named = { agent: row.home.agent, home: row.home.home };
  const buttons = {
    show: <SmallButton key="show" label="Show in Inventory" onClick={() => onShow({ kind: "home", home: row.home })} />,
    hide: <SmallButton key="hide" label="Hide" onClick={() => onChange({ kind: "hide_home", ...named })} />,
    unhide: <SmallButton key="unhide" label="Unhide" onClick={() => onChange({ kind: "unhide_home", ...named })} />,
    remove_from_list: (
      <SmallButton key="forget" label="Remove from list" onClick={() => onChange({ kind: "forget_home", ...named })} />
    ),
  };
  return (
    <PlaceRow icon="folder" dim={row.hidden} count={row.entries} actions={row.actions.map((action) => buttons[action])}>
      <div className="place-name">{row.label}</div>
      <div className="place-sub mono">{row.path}</div>
      <div className="tags">
        {row.agent && <span className="tag">{row.agent}</span>}
        {row.tags.map((tag) => (
          <span key={tag.text} className={`tag${tag.tone === "read_only" ? " read-only" : ""}`}>
            {tag.text}
          </span>
        ))}
        {row.hidden && <span className="tag">hidden</span>}
      </div>
      {row.notes.map((note) => (
        <div key={note} className="place-sub">
          {note}
        </div>
      ))}
    </PlaceRow>
  );
}

/** Where Projects come from: each Agent's registry, and the user. */
function ProjectsSide({ places }: { places: Places | null }) {
  const projects = places?.projects ?? [];
  const agents = (places?.agents ?? []).filter((agent) => projects.some((project) => project.registered_by.includes(agent.id)));
  return (
    <aside className="panel places-side">
      <h2 className="panel-title">Where Projects come from</h2>
      <div className="scroll">
        {agents.map((agent) => (
          <SideRow
            key={agent.id}
            title={agent.name}
            text="Its own registry of Projects"
            count={projects.filter((project) => project.registered_by.includes(agent.id)).length}
          />
        ))}
        <SideRow title="Added by you" text="Folders or .code-workspace files" count={projects.filter((project) => project.added).length} />
        <p className="side-note">
          A Project found in any one registry is evaluated for every Agent. Worktrees and nested folders are separate
          Projects only when a registry lists them on their own.
        </p>
        <p className="side-note">
          Hide keeps a Project out of the Inventory's selector; picking it there still evaluates it. Remove from list only
          applies to Projects you added. Missing Projects keep their leftovers until you remove them; trust lists are never
          touched.
        </p>
        <p className="side-note">
          Your additions and hides live in %APPDATA%\config-digger\. Nothing on this screen writes Agent configuration,
          except Remove leftovers, which is an Operation with its own preview, backup and Undo.
        </p>
      </div>
    </aside>
  );
}

/** Where Agent homes come from, as their source tags name it. */
const HOME_SOURCES: [string, string][] = [
  ["Default location", "Where each Agent definition says the Agent keeps its user configuration."],
  ["Environment variable", "CODEX_HOME, GEMINI_CLI_HOME and CLAUDE_USER_DATA_DIR, as config-digger sees them."],
  ["Known location", "MSIX packages, VS Code profiles, and homes other tools keep, such as Orca's Mirrored Codex home."],
  ["Added by you", "Any existing folder, read with the Agent definition you pick."],
];

/** Where Agent homes come from. */
function HomesSide() {
  return (
    <aside className="panel places-side">
      <h2 className="panel-title">Where homes come from</h2>
      <div className="scroll">
        {HOME_SOURCES.map(([title, text]) => (
          <SideRow key={title} title={title} text={text} />
        ))}
        <p className="side-note">
          A home is listed when its folder exists, even if the Agent is not installed. A home the Agent's lookup order
          skips is listed as not read, with the reason.
        </p>
        <p className="side-note">Installed versions are read from files, never by running the Agent.</p>
      </div>
    </aside>
  );
}

/** A row of a side panel: a title, what it means and how many Projects it accounts for. */
function SideRow({ title, text, count }: { title: string; text: string; count?: number }) {
  return (
    <div className="side-row">
      <b>{title}</b>
      <span className="dim">{text}</span>
      {count !== undefined && <span className="side-count">{count}</span>}
    </div>
  );
}

/** A small dialog with a title, its body, what to know before Add, and Cancel and Add. */
function AddDialog({
  title,
  sub,
  error,
  warning,
  addable,
  onAdd,
  onClose,
  children,
}: {
  title: string;
  sub: string;
  error: string | null;
  /** What to know before Add, which does not stop it. */
  warning: string | null;
  addable: boolean;
  onAdd: () => void;
  onClose: () => void;
  children: ReactNode;
}) {
  return (
    <div className="dialog-backdrop" onClick={(event) => event.target === event.currentTarget && onClose()}>
      <section className="dialog small" role="dialog" aria-modal="true" aria-labelledby="add-title">
        <h1 id="add-title" className="dialog-title">
          {title}
        </h1>
        <p className="dialog-subtitle">{sub}</p>
        {children}
        {warning && (
          <div className="alert" role="status">
            <Icon name="flag" small />
            {warning}
          </div>
        )}
        {error && (
          <div className="alert" role="alert">
            <Icon name="flag" small />
            {error}
          </div>
        )}
        <footer className="dialog-foot">
          <span className="grow" />
          <button type="button" className="cta secondary" onClick={onClose}>
            Cancel
          </button>
          <button type="button" className="cta" disabled={!addable} onClick={onAdd}>
            Add
          </button>
        </footer>
      </section>
    </div>
  );
}

/** Adds, then closes once added; keeps the dialog open with the core's reason otherwise. */
function useAdd(onAdd: (change: ListChange) => Promise<string | null>, onClose: () => void) {
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const add = async (change: ListChange) => {
    setBusy(true);
    let refused: string | null;
    try {
      refused = await onAdd(change);
    } finally {
      setBusy(false);
    }
    if (refused === null) onClose();
    else setError(refused);
  };
  return { error, busy, add };
}

/**
 * Opens the Windows picker, filling `set` with what was picked. The focus it
 * gives back on closing reads nothing again.
 */
async function pick(what: Pick, set: (path: string) => void, notify: Notify) {
  const result = await reads.during(() => call(() => commands.pickLocation(what)));
  if (result.status === "error") notify(`The Windows picker couldn't open: ${result.error.message}`);
  else if (result.data !== null) set(result.data);
}

type AddProps = { onAdd: (change: ListChange) => Promise<string | null>; onClose: () => void; notify: Notify };

/** How long an Add dialog waits after the last keystroke before the core checks what is chosen. */
const CHECK_DELAY_MS = 300;

/**
 * The core's last check of `asked`, once it has stayed the same for
 * {@link CHECK_DELAY_MS}, with no result when the check failed. `key` names
 * `asked`, which is checked again only when `key` changes.
 */
function useCheck<Asked, Result>(
  asked: Asked | null,
  key: string,
  check: (asked: Asked) => Promise<CommandResult<Result>>,
): Checked<Asked, Result> | null {
  const [checked, setChecked] = useState<Checked<Asked, Result> | null>(null);
  useEffect(() => {
    if (asked === null) return;
    let current = true;
    const timer = setTimeout(() => {
      void call(() => check(asked)).then((result) => {
        if (current) setChecked({ asked, result: result.status === "ok" ? result.data : null });
      });
    }, CHECK_DELAY_MS);
    return () => {
      current = false;
      clearTimeout(timer);
    };
    // `key` stands for `asked`, a new object on every render.
  }, [key]);
  return checked;
}

/** Add Project: a folder or `.code-workspace` file, typed or picked, checked before Add. */
function AddProjectDialog({ onAdd, onClose, notify }: AddProps) {
  const [path, setPath] = useState("");
  const { error, busy, add } = useAdd(onAdd, onClose);
  const asked = projectToCheck(path);
  const checked = useCheck(asked, asked ?? "", (asked) => commands.checkProject(asked));
  const offer = addProjectOffer(path, checked, busy);
  return (
    <AddDialog
      title={ADD_LABELS.projects}
      sub="A folder or a .code-workspace file. config-digger evaluates it for every Agent and writes nothing to any Agent."
      error={error}
      warning={offer.warning}
      addable={offer.addable}
      onAdd={() => {
        if (asked) void add({ kind: "add_project", path: asked });
      }}
      onClose={onClose}
    >
      <div className="add-field">
        <input className="add-input mono" aria-label="Path" value={path} onChange={(event) => setPath(event.target.value)} />
        <button type="button" className="cta small secondary" onClick={() => void pick("folder", setPath, notify)}>
          Choose folder…
        </button>
        <button type="button" className="cta small secondary" onClick={() => void pick("workspace_file", setPath, notify)}>
          Choose .code-workspace…
        </button>
      </div>
      <p className="side-note">The folder or file must exist. config-digger reads it again at once.</p>
    </AddDialog>
  );
}

/** Add Agent home: an Agent, chosen explicitly, and an existing folder, typed or picked, checked before Add. */
function AddHomeDialog({ agents, onAdd, onClose, notify }: AddProps & { agents: Agent[] }) {
  const [choice, setChoice] = useState<HomeToAdd>(NO_HOME_CHOSEN);
  const { error, busy, add } = useAdd(onAdd, onClose);
  const asked = homeToCheck(choice);
  const checked = useCheck(asked, asked ? `${asked.agent}|${asked.folder}` : "", (asked) =>
    commands.checkHome(asked.folder, asked.agent),
  );
  const offer = addHomeOffer(choice, checked, busy);
  return (
    <AddDialog
      title={ADD_LABELS.homes}
      sub="A folder an Agent reads its user configuration from, such as another CODEX_HOME."
      error={error}
      warning={offer.warning}
      addable={offer.addable}
      onAdd={() => {
        if (asked) void add({ kind: "add_home", ...asked });
      }}
      onClose={onClose}
    >
      <div className="add-field">
        <Select
          className="add-input"
          label="Agent"
          value={choice.agent}
          options={[
            { value: "", label: "Choose an Agent…", disabled: true },
            ...agents.map((each) => ({ value: each.id, label: each.name })),
          ]}
          onChange={(agent) => setChoice({ ...choice, agent })}
        />
        <input
          className="add-input mono"
          aria-label="Folder"
          value={choice.folder}
          onChange={(event) => setChoice({ ...choice, folder: event.target.value })}
        />
        <button
          type="button"
          className="cta small secondary"
          onClick={() => void pick("folder", (picked) => setChoice((now) => ({ ...now, folder: picked })), notify)}
        >
          Choose…
        </button>
      </div>
      <p className="side-note">
        config-digger reads the folder with the definition of the Agent you choose. When none of the files that definition
        reads in a home is there, it says so before you add it; an empty home can still be added.
      </p>
    </AddDialog>
  );
}
