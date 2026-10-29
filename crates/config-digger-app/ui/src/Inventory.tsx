import { useEffect, useMemo, useRef, useState } from "react";
import { rereadAfter } from "./acting";
import type { AgentFlag, AlsoReadBy, BuiltServer, Entry, Home, Inventory, Item, ReadFailure, State, Verb } from "./bindings";
import { NOTICE_MS, type Notify } from "./desktop";
import type { DropdownOption } from "./dropdown";
import { EmptyInventory } from "./EmptyInventory";
import { EntryCard } from "./EntryCard";
import { recoveryBanner } from "./history";
import { Icon } from "./icons";
import { Loading, ReadProgress } from "./Loading";
import { LinkTag } from "./Location";
import { Status, type Notice } from "./Notice";
import type { InventoryStart } from "./places";
import { ReadFailureCard } from "./ReadFailureCard";
import { readCue } from "./reads";
import { Select } from "./Select";
import { StatePill } from "./StatePill";
import { STATE_LABELS } from "./states";
import { useActionDialog } from "./useActionDialog";
import { useBusy } from "./useBusy";
import { useHistory } from "./useHistory";
import { useInventory } from "./useInventory";
import { useTransient } from "./useTransient";
import {
  agentsOf,
  agentStats,
  chosenHomes,
  couldNotBeRead,
  failuresInContext,
  filterItems,
  findItem,
  flagsShown,
  groupItems,
  homeChoices,
  homeFailures,
  isLastOpened,
  itemKey,
  itemsShown,
  KIND_NAMES,
  KIND_TITLES,
  lastOpenedHomes,
  lastOpenedNotes,
  leftoversNotShown,
  projectFailures,
  readerItem,
  samePath,
  selectionPicking,
  stateCounts,
  withHomeChosen,
  withProjectChosen,
  type AgentStats,
  type HomeChoices,
  type ItemFilter,
  type Selection,
} from "./view";
import { Tooltip } from "./Tooltip";
import { lastPart } from "./wrap";

/** What the detail panel shows: an Item's Entries, or the Read failures in the context. */
type Detail = { kind: "item"; key: string } | { kind: "failures" } | null;

/** How long a "show winner" mark stays. */
const FLASH_MS = 1600;

type InventoryProps = {
  /** Where it opens, when another screen opens it on something. */
  start?: InventoryStart;
  onOpenHistory: () => void;
  /** Opens the Agent homes tab with its add dialog, from the empty inventory. */
  onAddHome: () => void;
  /** Opens the Agent homes tab, from the empty inventory. */
  onOpenHomes: () => void;
};

/** The Inventory screen: the top banner over the Agents, Items and detail panes. */
export function InventoryScreen({ start, onOpenHistory, onAddHome, onOpenHomes }: InventoryProps) {
  const { inventory, project, error, reading, refresh, selectProject } = useInventory(start?.project ?? null);
  const cue = readCue({ shown: inventory !== null, reading, failed: error !== null });
  const { history, reload: reloadHistory } = useHistory();
  const [agent, setAgent] = useState(start?.agent ?? "all");
  const [homeChoice, setHomeChoice] = useState<HomeChoices>(homeChoices(start?.homes));
  const [filter, setFilter] = useState<ItemFilter>({ kind: "all", text: "", state: start?.state ?? null });
  const [detail, setDetail] = useState<Detail>(null);
  const [flash, setFlash] = useTransient<string>(FLASH_MS);
  const [notice, setNotice, holdNotice] = useTransient<Notice>(NOTICE_MS);
  const { actions, busy } = useBusy();
  // While an action runs or the inventory is read, the States shown may be
  // old: an action reaches every Agent that shares its mechanism, and an Undo
  // names no Entry. So no Entry's action can be started until the read is done.
  const entriesBusy = actions.length > 0 || reading;
  // Whatever the dialog did, the inventory and History are read again.
  const dialog = useActionDialog({
    onClosed: (request) => {
      void rereadAfter(request, { text: "Reading the inventory again…", reload: refresh, busy, notify: setNotice });
      void reloadHistory();
    },
    onOpenHistory,
    notify: setNotice,
  });
  const recovery = recoveryBanner(history ?? []);
  // The Item another screen opened it on, selected once the inventory is read.
  const startItem = useRef(start?.item ?? null);

  const homes = useMemo(
    () => chosenHomes(inventory?.homes ?? [], homeChoice, inventory?.items ?? [], lastOpenedHomes(inventory)),
    [inventory, homeChoice],
  );
  const selection: Selection = { agent, homes };
  const shown = useMemo(() => itemsShown(inventory?.items ?? [], { agent, homes }), [inventory, agent, homes]);
  const failures = inventory ? failuresInContext(inventory.read_failures, inventory.project, selection) : [];
  const flags = flagsShown(inventory?.agent_flags ?? [], selection);
  const notShown = inventory ? leftoversNotShown(inventory, selection) : [];
  useEffect(() => {
    if (!inventory || !startItem.current) return;
    const item = findItem(inventory.items, startItem.current);
    startItem.current = null;
    if (item) setDetail({ kind: "item", key: itemKey(item) });
  }, [inventory]);
  const selected = detail?.kind === "item" ? (shown.find((item) => itemKey(item) === detail.key) ?? null) : null;
  const agentName = agent === "all" ? "All Agents" : (inventory?.homes.find((home) => home.agent === agent)?.agent_name ?? agent);

  const chooseAgent = (next: string) => {
    setAgent(next);
    setDetail(null);
  };
  const chooseHome = (agentId: string, home: string) => {
    setHomeChoice((choices) => withHomeChosen(choices, agentId, home));
    setDetail(null);
  };
  const showWinner = (locator: string) => {
    setFlash(locator);
    document.querySelector(`[data-locator="${CSS.escape(locator)}"]`)?.scrollIntoView({ behavior: "smooth", block: "nearest" });
  };
  const openReader = (reader: AlsoReadBy, locator: string) => {
    const item = inventory && readerItem(inventory, reader, locator);
    if (!item) return;
    setHomeChoice((choices) => withHomeChosen(choices, reader.agent, reader.home));
    if (agent !== "all") setAgent(reader.agent);
    setDetail({ kind: "item", key: itemKey(item) });
    setFlash(locator);
  };
  const onAction = (verb: Verb, entry: Entry) =>
    dialog.open({ kind: "entry", verb, locator: entry.locator, project: inventory?.project ?? null });

  return (
    <div className="inventory">
      <Banner
        inventory={inventory}
        project={project}
        choices={homeChoice}
        selection={selection}
        agentName={agentName}
        stats={agentStats(shown)}
        reading={reading}
        rereading={cue === "rereading"}
        refresh={() => void refresh()}
        selectProject={(next) => {
          selectProject(next);
          setHomeChoice((choices) => withProjectChosen(choices, inventory?.projects ?? [], next));
          setDetail(null);
        }}
      />

      {recovery && (
        <div className="recovery-banner" role="alert">
          <Icon name="flag" small />
          {recovery}
          <span className="grow" />
          <button type="button" className="cta secondary" onClick={onOpenHistory}>
            Open History
          </button>
        </div>
      )}

      {cue === "loading" ? (
        <Loading text="Reading the inventory…" rows={8} />
      ) : inventory && inventory.homes.length === 0 ? (
        <EmptyInventory
          readAt={inventory.read_at}
          reading={reading}
          refresh={() => void refresh()}
          onAddHome={onAddHome}
          onOpenHomes={onOpenHomes}
        />
      ) : (
        <div className="panes">
          <div className="column">
            <AgentsPane inventory={inventory} selection={selection} onAgent={chooseAgent} />
            <StateBars
              items={shown}
              state={filter.state}
              onState={(state) => setFilter((current) => ({ ...current, state }))}
            />
            <HomesPane inventory={inventory} selection={selection} onHome={chooseHome} />
          </div>

          <section className="panel items">
            <h2 className="panel-title">
              Items
              <span className="grow" />
              <span className="segmented" role="group" aria-label="Item kind">
                {(["all", "mcp", "skill"] as const).map((kind) => (
                  <button
                    type="button"
                    key={kind}
                    className={filter.kind === kind ? "on" : ""}
                    aria-pressed={filter.kind === kind}
                    onClick={() => setFilter((current) => ({ ...current, kind }))}
                  >
                    {kind === "all" ? "All" : KIND_TITLES[kind]}
                  </button>
                ))}
              </span>
            </h2>
            <label className="search">
              <Icon name="search" small />
              <input
                placeholder="Search Items…"
                value={filter.text}
                onChange={(event) => setFilter((current) => ({ ...current, text: event.target.value }))}
              />
            </label>
            {error && (
              <div className="alert" role="alert">
                <Icon name="flag" small />
                The inventory couldn't be read: {error.message}
              </div>
            )}
            <FailuresNotice failures={failures} onView={() => setDetail({ kind: "failures" })} />
            {flags.map((flag) => (
              <Tooltip key={`${flag.agent}|${flag.home}|${flag.code}|${flag.path}`} text={flag.path}>
                <div className="alert" role="status" tabIndex={0}>
                  <Icon name="flag" small />
                  {agent === "all" ? `${flag.agent_name}: ${flag.sentence}` : flag.sentence}
                </div>
              </Tooltip>
            ))}
            {notShown.map((leftovers) => (
              <div key={`${leftovers.agent}|${leftovers.home}`} className="alert" role="status">
                <Icon name="flag" small />
                {leftovers.sentence}
                <span className="grow" />
                <button type="button" className="link" onClick={() => chooseHome(leftovers.agent, leftovers.home)}>
                  Show
                </button>
              </div>
            ))}
            <div className="scroll">
              {inventory && (
                <ItemList
                  inventory={inventory}
                  items={filterItems(shown, filter)}
                  agent={agent}
                  flags={flags}
                  selected={selected}
                  onSelect={(item) => setDetail({ kind: "item", key: itemKey(item) })}
                />
              )}
            </div>
          </section>

          <section className="panel detail">
            {detail?.kind === "failures" ? (
              <>
                <h2 className="panel-title">Read failures</h2>
                <div className="scroll cards">
                  {failures.map((failure) => (
                    <ReadFailureCard
                      key={`${failure.agent}|${failure.home}|${failure.path}|${failure.location?.id}|${failure.location?.key_path.join(".")}`}
                      failure={failure}
                      notify={setNotice}
                    />
                  ))}
                  {failures.length === 0 && <p className="empty">Every location in this context was read.</p>}
                </div>
              </>
            ) : selected && inventory ? (
              <ItemDetail
                inventory={inventory}
                item={selected}
                flash={flash}
                busy={entriesBusy}
                onShowWinner={showWinner}
                onReader={openReader}
                onAction={onAction}
                notify={setNotice}
              />
            ) : (
              <>
                <h2 className="panel-title">Details</h2>
                <p className="empty">Select an Item to see which Entry is Effective and why.</p>
              </>
            )}
          </section>
        </div>
      )}

      <Status notice={notice} actions={actions} onHold={holdNotice} />

      {dialog.view}
    </div>
  );
}

/** How many locations of the context couldn't be read, over the Items, with View to list them. */
export function FailuresNotice({ failures, onView }: { failures: ReadFailure[]; onView: () => void }) {
  if (failures.length === 0) return null;
  return (
    <div className="alert" role="status">
      <Icon name="flag" small />
      {couldNotBeRead(failures.length)}.
      <span className="grow" />
      <button type="button" className="link" onClick={onView}>
        View
      </button>
    </div>
  );
}

/** A flag with how many locations couldn't be read in `where`. */
function FailureBadge({ count, where }: { count: number; where: string }) {
  if (count === 0) return null;
  return (
    <Tooltip text={`${couldNotBeRead(count)} in ${where}`}>
      <span className="badge">
        <Icon name="flag" small />
        {count}
      </span>
    </Tooltip>
  );
}

type BannerProps = {
  inventory: Inventory | null;
  project: string | null;
  /** The homes the user chose, from which another Project's option counts what picking it shows. */
  choices: HomeChoices;
  selection: Selection;
  agentName: string;
  stats: AgentStats;
  reading: boolean;
  /** Whether it reads again over what the screen shows. */
  rereading: boolean;
  refresh: () => void;
  selectProject: (project: string | null) => void;
};

/**
 * The Project field's value, its options and what a choice does: `selectProject`
 * loads the chosen Project, or no Project, whether a click or a key chose it.
 * Each option counts the Read failures of its Project that picking it shows:
 * the selected Project's under `selection`, another's under the selection
 * picking it gives from `choices`.
 */
export function projectField(
  inventory: Inventory | null,
  project: string | null,
  selection: Selection,
  choices: HomeChoices,
  selectProject: (project: string | null) => void,
): { value: string; options: DropdownOption[]; onChange: (value: string) => void } {
  const projects = inventory?.projects ?? [];
  const option = (path: string, group?: string): DropdownOption => {
    const under =
      !inventory || (project !== null && samePath(path, project)) ? selection : selectionPicking(inventory, choices, selection.agent, path);
    const failed = projectFailures(inventory?.read_failures ?? [], path, under);
    return { value: path, label: failed > 0 ? `${path} · ${couldNotBeRead(failed)}` : path, group };
  };
  const present = projects.filter((p) => !p.missing).map((p) => p.path);
  const missing = projects.filter((p) => p.missing).map((p) => p.path);
  // A Project picked but no longer listed, such as one hidden since, stays selectable.
  if (project !== null && !projects.some((p) => samePath(p.path, project))) present.push(project);
  return {
    value: project ?? "",
    options: [
      { value: "", label: "No Project" },
      ...present.map((path) => option(path)),
      ...missing.map((path) => option(path, "Missing Projects")),
    ],
    onChange: (chosen) => selectProject(chosen === "" ? null : chosen),
  };
}

/** Items in use, the current Agent, the Project selector, the read time and Refresh. */
export function Banner({ inventory, project, choices, selection, agentName, stats, reading, rereading, refresh, selectProject }: BannerProps) {
  return (
    <header className="panel banner">
      <Ring done={stats.inUse} total={stats.items} />
      <div>
        <div className="caption">Items in use</div>
        <div className="banner-title">{agentName}</div>
      </div>
      <div>
        <div className="caption">
          Project
          {project !== null && <FailureBadge count={projectFailures(inventory?.read_failures ?? [], project, selection)} where="this Project" />}
        </div>
        <Select
          className="banner-select"
          label="Project"
          tooltip={project ?? undefined}
          searchLabel="Search Projects"
          {...projectField(inventory, project, selection, choices, selectProject)}
        />
      </div>
      <div>
        <div className="caption">Inventory read</div>
        <Tooltip text={inventory?.read_at}>
          <div className="banner-value" tabIndex={0}>
            {inventory ? readTime(inventory.read_at) : "—"}
            <span className="caption"> · on focus</span>
          </div>
        </Tooltip>
      </div>
      <span className="grow" />
      <button type="button" className="cta secondary" onClick={refresh} disabled={reading}>
        <Icon name="refresh" small />
        {reading ? "Reading" : "Refresh"}
      </button>
      {rereading && <ReadProgress text="Reading the inventory again" />}
    </header>
  );
}

/** Items in use out of the Items shown. */
function Ring({ done, total }: { done: number; total: number }) {
  const radius = 33;
  const length = 2 * Math.PI * radius;
  const share = total === 0 ? 0 : done / total;
  return (
    <div className="ring" aria-label={`${done} of ${total} Items in use`}>
      <svg width="74" height="74" aria-hidden="true">
        <circle cx="37" cy="37" r={radius} className="ring-track" />
        <circle cx="37" cy="37" r={radius} className="ring-value" strokeDasharray={`${length * share} ${length}`} />
      </svg>
      <span>
        {done}/{total}
      </span>
    </div>
  );
}

type AgentsPaneProps = {
  inventory: Inventory | null;
  selection: Selection;
  onAgent: (agent: string) => void;
};

/** All Agents first, then each Agent, with its counts and its problems in the context. */
function AgentsPane({ inventory, selection, onAgent }: AgentsPaneProps) {
  const tile = (agent: string, name: string) => {
    const tileSelection = { agent, homes: selection.homes };
    const stats = agentStats(itemsShown(inventory?.items ?? [], tileSelection));
    const flagged = flagsShown(inventory?.agent_flags ?? [], tileSelection);
    const selected = selection.agent === agent;
    return (
      <button
        type="button"
        key={agent}
        className={`tile agent-tile${selected ? " selected" : ""}`}
        aria-pressed={selected}
        onClick={() => onAgent(agent)}
      >
        <span className="agent-name">
          {agent === "all" && <Icon name="users" small />}
          {name}
          {flagged.length > 0 && (
            <Tooltip text={flagged.map((flag) => `${flag.agent_name}: ${flag.sentence}`).join("\n")}>
              <span className="warn">
                <Icon name="flag" small />
              </span>
            </Tooltip>
          )}
        </span>
        <span className="agent-counts">
          in use: {stats.inUse} / {stats.items}
          <br />
          no effect: {stats.noEffect}
        </span>
      </button>
    );
  };
  return (
    <section className="panel agents">
      <h2 className="panel-title">Agents</h2>
      <div className="scroll">
        {tile("all", "All Agents")}
        {agentsOf(inventory?.homes ?? []).map(({ agent, agentName }) => tile(agent, agentName))}
      </div>
    </section>
  );
}

/** One bar per State; clicking one filters the Items list by it. */
export function StateBars({ items, state, onState }: { items: Item[]; state: State | null; onState: (state: State | null) => void }) {
  const { total, counts } = stateCounts(items);
  return (
    <section className="panel states">
      <h2 className="panel-title">
        Entries by State
        <span className="grow" />
        {state && (
          <button type="button" className="link" onClick={() => onState(null)}>
            clear
          </button>
        )}
      </h2>
      {counts.length === 0 && <p className="empty">No Entries.</p>}
      <div className="bars">
        {counts.map(({ state: barState, count }) => (
          <button
            type="button"
            key={barState}
            className={`bar state-${barState}${state === barState ? " selected" : ""}`}
            aria-pressed={state === barState}
            onClick={() => onState(state === barState ? null : barState)}
          >
            <span className="bar-label">{STATE_LABELS[barState]}</span>
            <span className="bar-track">
              <i style={{ width: `${(100 * count) / total}%` }} />
              <span className="bar-count">
                {count} / {total}
              </span>
            </span>
          </button>
        ))}
      </div>
    </section>
  );
}

type HomesPaneProps = {
  inventory: Inventory | null;
  selection: Selection;
  onHome: (agent: string, home: string) => void;
};

/**
 * A selector per Agent under All Agents, or one Agent's homes as tiles, with
 * the Project's Last-opened homes marked, and where the Agents shown last
 * opened it when they show another home.
 */
export function HomesPane({ inventory, selection, onHome }: HomesPaneProps) {
  const all = inventory?.homes ?? [];
  const failuresOf = (home: Home) => homeFailures(inventory?.read_failures ?? [], home);
  const lastOpened = lastOpenedHomes(inventory);
  const opened = (home: Home) => isLastOpened(lastOpened, home);
  // Another tool copies a Mirrored home, so config-digger only reads it.
  // A tag is a tab stop, `tabIndex` 0, for the keyboard to show its tooltip,
  // except inside a tile's button, whose own tooltip then says it.
  const mirrored = (home: Home, tabIndex?: number) =>
    home.mirror && (
      <Tooltip text={home.mirror.sentence}>
        <span className="tag read-only" tabIndex={tabIndex}>
          {home.mirror.label}
        </span>
      </Tooltip>
    );
  const notRead = (home: Home, tabIndex?: number) =>
    home.not_read && (
      <Tooltip text={home.not_read.sentence}>
        <span className="tag" tabIndex={tabIndex}>
          not read
        </span>
      </Tooltip>
    );

  let body;
  if (inventory === null) {
    body = null;
  } else if (all.length === 0) {
    body = <p className="empty">No Agent home found.</p>;
  } else if (selection.agent === "all") {
    body = agentsOf(all).map(({ agent, agentName }) => {
      const own = all.filter((home) => home.agent === agent);
      const chosen = own.find((home) => home.home === selection.homes[agent]) ?? own[0];
      return (
        <div className="home-row" key={agent}>
          <span>{agentName}</span>
          <span className="grow" />
          {mirrored(chosen, 0)}
          {notRead(chosen, 0)}
          <FailureBadge count={failuresOf(chosen)} where="this home" />
          {own.length > 1 ? (
            <Select
              className="home-select"
              label={`${agentName} home`}
              value={chosen.home}
              tooltip={chosen.folder}
              options={own.map((home) => ({ value: home.home, label: homeLabel(home, failuresOf(home), opened(home)) }))}
              onChange={(home) => onHome(agent, home)}
            />
          ) : (
            <Tooltip text={chosen.folder}>
              <span className="dim single-home" tabIndex={0}>
                {chosen.home}
              </span>
            </Tooltip>
          )}
        </div>
      );
    });
  } else {
    const agent = selection.agent;
    body = all
      .filter((home) => home.agent === agent)
      .map((home) => {
        const selected = selection.homes[agent] === home.home;
        return (
          <Tooltip key={home.home} text={homeTileTooltip(home)}>
            <button
              type="button"
              className={`tile home-tile${selected ? " selected" : ""}`}
              aria-pressed={selected}
              onClick={() => onHome(agent, home.home)}
            >
              <HomeName home={home.home} />
              <span className="grow" />
              {opened(home) && <span className="tag">{LAST_OPENED}</span>}
              {mirrored(home)}
              {notRead(home)}
              <FailureBadge count={failuresOf(home)} where="this home" />
            </button>
          </Tooltip>
        );
      });
  }
  return (
    <section className="panel homes">
      <h2 className="panel-title">Agent home</h2>
      <div className="home-list">
        {body}
        {lastOpenedNotes(lastOpened, selection).map((note) => (
          <p key={note} className="home-note">
            {note}
          </p>
        ))}
      </div>
    </section>
  );
}

/** The mark of a Project's Last-opened home. */
const LAST_OPENED = "last opened here";

/**
 * The tooltip of a home's tile: its folder, or that it has no configuration,
 * then the sentence of its Mirrored or not read tag, which takes no focus
 * inside the tile's button.
 */
function homeTileTooltip(home: Home): string {
  return [home.no_configuration ?? home.folder, home.mirror?.sentence, home.not_read?.sentence].filter(Boolean).join("\n");
}

/**
 * A home's name on its tile, in two parts: its folder's last part, with the
 * separator before it, and the rest, which is cut first when the name is too
 * long for the tile, so that two homes of one Agent under one folder read apart.
 */
function HomeName({ home }: { home: string }) {
  const { rest, last } = lastPart(home);
  return (
    <span className="home-name">
      {rest && <span className="home-name-head">{rest}</span>}
      <span className="home-name-tail">{last}</span>
    </span>
  );
}

/** A home as the select of All Agents lists it, with its marks and how many locations couldn't be read in it. */
export function homeLabel(home: Home, failed: number, lastOpened: boolean): string {
  let label = home.home;
  if (lastOpened) label += ` · ${LAST_OPENED}`;
  if (home.mirror) label += ` · ${home.mirror.label}`;
  if (home.not_read) label += " · not read";
  if (failed > 0) label += ` · ${couldNotBeRead(failed)}`;
  return label;
}

type ItemListProps = {
  inventory: Inventory;
  items: Item[];
  agent: string;
  flags: AgentFlag[];
  selected: Item | null;
  onSelect: (item: Item) => void;
};

/** Items grouped by Agent under All Agents, by kind for one Agent. */
function ItemList({ inventory, items, agent, flags, selected, onSelect }: ItemListProps) {
  if (inventory.items.length === 0) return <p className="empty">No Items found.</p>;
  const groups = groupItems(items, agent, inventory.homes);
  if (groups.length === 0) return <p className="empty">Nothing matches in this context.</p>;
  return (
    <>
      {groups.map((group) => {
        const flagged = flags.filter((flag) => flag.agent === group.agent);
        return (
          <div className="item-group" key={group.key}>
            <div className="group">
              {group.title}
              {flagged.length > 0 && (
                <Tooltip text={flagged.map((flag) => flag.sentence).join("\n")}>
                  <span className="warn">
                    <Icon name="flag" small />
                  </span>
                </Tooltip>
              )}
              <span className="line" />
              {group.items.length}
            </div>
            {group.items.map((item) => (
              <ItemRow
                key={itemKey(item)}
                item={item}
                selected={selected !== null && itemKey(item) === itemKey(selected)}
                onSelect={() => onSelect(item)}
              />
            ))}
          </div>
        );
      })}
    </>
  );
}

function ItemRow({ item, selected, onSelect }: { item: Item; selected: boolean; onSelect: () => void }) {
  // Entries come in precedence order, so the first Effective one is shown.
  const effective = item.entries.find((entry) => entry.state === "effective") ?? item.entries[0];
  const entries = item.entries.length === 1 ? "1 Entry" : `${item.entries.length} Entries`;
  return (
    <button type="button" className={`item-row${selected ? " selected" : ""}`} onClick={onSelect}>
      <span className="item-icon">
        <Icon name={item.kind === "mcp" ? "server" : "doc"} small />
      </span>
      <span className="item-text">
        <span className="item-name">{item.name}</span>
        <Tooltip text={effective.locator}>
          <span className="item-location">
            {effective.scope} · {effective.locator}
          </span>
        </Tooltip>
      </span>
      <span className="item-side">
        {effective.git_versioned && <span className="tag git">git-versioned</span>}
        {effective.link && <LinkTag link={effective.link} />}
        <span className="item-count">{entries}</span>
        <StatePill state={item.state} />
      </span>
    </button>
  );
}

type ItemDetailProps = {
  inventory: Inventory;
  item: Item;
  flash: string | null;
  /** Its Entries are read again, as after an action, so their actions are disabled. */
  busy: boolean;
  onShowWinner: (locator: string) => void;
  onReader: (reader: AlsoReadBy, locator: string) => void;
  onAction: (verb: Verb, entry: Entry) => void;
  notify: Notify;
};

/** An Item's Entries in precedence order, highest first and numbered. */
function ItemDetail({ inventory, item, flash, busy, onShowWinner, onReader, onAction, notify }: ItemDetailProps) {
  const agentName = inventory.homes.find((home) => home.agent === item.agent)?.agent_name ?? item.agent;
  const project = inventory.project ?? "No Project";
  const inItem = (locator: string) => item.entries.some((entry) => entry.locator === locator);
  return (
    <>
      <h2 className="panel-title">
        {item.name}
        <StatePill state={item.state} />
      </h2>
      <p className="detail-context">
        {KIND_NAMES[item.kind]} · {agentName} · {item.home} · {project}
      </p>
      <div className="scroll cards">
        <p className="caption">Entries in precedence order, highest first:</p>
        {item.caveats.map((caveat, index) => (
          <div key={`caveat-${index}`} className="alert" role="status">
            <Icon name="flag" small />
            {caveat.sentence}
          </div>
        ))}
        {item.built && <BuiltServerCard built={item.built} agentName={agentName} onShowEntry={onShowWinner} />}
        {item.entries.map((entry, index) => (
          <div key={entry.locator} data-locator={entry.locator}>
            <EntryCard
              entry={entry}
              rank={index + 1}
              agentName={agentName}
              flash={flash === entry.locator}
              busy={busy}
              inItem={inItem}
              onShowWinner={onShowWinner}
              onReader={(reader) => onReader(reader, entry.locator)}
              onAction={onAction}
              notify={notify}
            />
          </div>
        ))}
      </div>
    </>
  );
}

/** The server an Agent builds from Merged Entries, and the Entry each of its fields comes from. */
function BuiltServerCard({
  built,
  agentName,
  onShowEntry,
}: {
  built: BuiltServer;
  agentName: string;
  onShowEntry: (locator: string) => void;
}) {
  return (
    <article className="entry-card built-server">
      <div className="caption">{agentName} builds</div>
      <div className="config">
        <pre className="mono">{JSON.stringify(built.config, null, 2)}</pre>
      </div>
      <dl className="field-origins">
        {built.origins.map((origin) => (
          <div key={origin.key_path.join(".")}>
            <dt className="mono">{origin.key_path.join(".")}</dt>
            <dd>
              <button type="button" className="link mono" onClick={() => onShowEntry(origin.entry)}>
                {origin.entry}
              </button>
            </dd>
          </div>
        ))}
      </dl>
    </article>
  );
}

/** The local time of day the inventory was read, such as `10:42`. */
function readTime(readAt: string): string {
  return new Date(readAt).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
}
