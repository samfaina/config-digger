import { useEffect, useMemo, useState } from "react";
import type { AgentFlag, AlsoReadBy, BuiltServer, Home, Inventory, Item, State } from "./bindings";
import type { Notify } from "./desktop";
import { EntryCard } from "./EntryCard";
import { Icon } from "./icons";
import { LinkTag } from "./Location";
import { ReadFailureCard } from "./ReadFailureCard";
import { StatePill } from "./StatePill";
import { STATE_LABELS } from "./states";
import { useInventory } from "./useInventory";
import {
  agentsOf,
  agentStats,
  chosenHomes,
  couldNotBeRead,
  failuresInContext,
  filterItems,
  flagsShown,
  groupItems,
  homeFailures,
  itemKey,
  itemsShown,
  KIND_NAMES,
  KIND_TITLES,
  projectFailures,
  readerItem,
  samePath,
  stateCounts,
  type ActionVerb,
  type AgentStats,
  type HomeChoice,
  type ItemFilter,
  type Selection,
} from "./view";

/** What the detail panel shows: an Item's Entries, or the Read failures in the context. */
type Detail = { kind: "item"; key: string } | { kind: "failures" } | null;

/** How long a notice and a "show winner" mark stay. */
const NOTICE_MS = 2500;
const FLASH_MS = 1600;

/** The Inventory screen: the top banner over the Agents, Items and detail panes. */
export function InventoryScreen() {
  const { inventory, project, error, reading, refresh, selectProject } = useInventory();
  const [agent, setAgent] = useState("all");
  const [homeChoice, setHomeChoice] = useState<HomeChoice>({});
  const [filter, setFilter] = useState<ItemFilter>({ kind: "all", text: "", state: null });
  const [detail, setDetail] = useState<Detail>(null);
  const [flash, setFlash] = useTransient<string>(FLASH_MS);
  const [notice, setNotice] = useTransient<string>(NOTICE_MS);

  const homes = useMemo(() => chosenHomes(inventory?.homes ?? [], homeChoice), [inventory, homeChoice]);
  const selection: Selection = { agent, homes };
  const shown = useMemo(() => itemsShown(inventory?.items ?? [], { agent, homes }), [inventory, agent, homes]);
  const failures = inventory ? failuresInContext(inventory.read_failures, inventory.project, selection) : [];
  const flags = flagsShown(inventory?.agent_flags ?? [], selection);
  const selected = detail?.kind === "item" ? (shown.find((item) => itemKey(item) === detail.key) ?? null) : null;
  const agentName = agent === "all" ? "All Agents" : (inventory?.homes.find((home) => home.agent === agent)?.agent_name ?? agent);

  const chooseAgent = (next: string) => {
    setAgent(next);
    setDetail(null);
  };
  const chooseHome = (agentId: string, home: string) => {
    setHomeChoice((choice) => ({ ...choice, [agentId]: home }));
    setDetail(null);
  };
  const showWinner = (locator: string) => {
    setFlash(locator);
    document.querySelector(`[data-locator="${CSS.escape(locator)}"]`)?.scrollIntoView({ behavior: "smooth", block: "nearest" });
  };
  const openReader = (reader: AlsoReadBy, locator: string) => {
    const item = inventory && readerItem(inventory, reader, locator);
    if (!item) return;
    setHomeChoice((choice) => ({ ...choice, [reader.agent]: reader.home }));
    if (agent !== "all") setAgent(reader.agent);
    setDetail({ kind: "item", key: itemKey(item) });
    setFlash(locator);
  };
  // The action dialog that previews each action comes later.
  const onAction = (verb: ActionVerb) => setNotice(`The ${VERB_LABELS[verb]} preview is not built yet.`);

  return (
    <div className="inventory">
      <Banner
        inventory={inventory}
        project={project}
        agentName={agentName}
        stats={agentStats(shown)}
        reading={reading}
        refresh={refresh}
        selectProject={(next) => {
          selectProject(next);
          setDetail(null);
        }}
      />

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
          {failures.length > 0 && (
            <div className="alert" role="status">
              <Icon name="flag" small />
              {couldNotBeRead(failures.length)}.
              <span className="grow" />
              <button type="button" className="link" onClick={() => setDetail({ kind: "failures" })}>
                View
              </button>
            </div>
          )}
          {flags.map((flag) => (
            <div key={`${flag.agent}|${flag.home}|${flag.code}|${flag.path}`} className="alert" role="status" title={flag.path}>
              <Icon name="flag" small />
              {agent === "all" ? `${flag.agent_name}: ${flag.sentence}` : flag.sentence}
            </div>
          ))}
          <div className="scroll">
            {inventory ? (
              <ItemList
                inventory={inventory}
                items={filterItems(shown, filter)}
                agent={agent}
                flags={flags}
                selected={selected}
                onSelect={(item) => setDetail({ kind: "item", key: itemKey(item) })}
              />
            ) : (
              !error && <p className="empty">Reading the inventory…</p>
            )}
          </div>
        </section>

        <section className="panel detail">
          {detail?.kind === "failures" ? (
            <>
              <h2 className="panel-title">Read failures</h2>
              <div className="scroll cards">
                {failures.map((failure) => (
                  <ReadFailureCard key={`${failure.agent}|${failure.home}|${failure.path}`} failure={failure} notify={setNotice} />
                ))}
                {failures.length === 0 && <p className="empty">Every location in this context was read.</p>}
              </div>
            </>
          ) : selected && inventory ? (
            <ItemDetail
              inventory={inventory}
              item={selected}
              flash={flash}
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

      {notice && (
        <div className="notice" role="status">
          {notice}
        </div>
      )}
    </div>
  );
}

const VERB_LABELS: Record<ActionVerb, string> = { disable: "Disable", enable: "Enable", remove: "Remove" };

/** A value that clears itself `ms` after it is set. */
function useTransient<T>(ms: number): [T | null, (value: T | null) => void] {
  const [value, setValue] = useState<T | null>(null);
  useEffect(() => {
    if (value === null) return;
    const timer = window.setTimeout(() => setValue(null), ms);
    return () => window.clearTimeout(timer);
  }, [value, ms]);
  return [value, setValue];
}

/** A flag with how many locations couldn't be read in `where`. */
function FailureBadge({ count, where }: { count: number; where: string }) {
  if (count === 0) return null;
  return (
    <span className="badge" title={`${couldNotBeRead(count)} in ${where}`}>
      <Icon name="flag" small />
      {count}
    </span>
  );
}

type BannerProps = {
  inventory: Inventory | null;
  project: string | null;
  agentName: string;
  stats: AgentStats;
  reading: boolean;
  refresh: () => void;
  selectProject: (project: string | null) => void;
};

/** Items in use, the current Agent, the Project selector, the read time and Refresh. */
function Banner({ inventory, project, agentName, stats, reading, refresh, selectProject }: BannerProps) {
  const projects = inventory?.projects ?? [];
  const failuresOf = (path: string) => projectFailures(inventory?.read_failures ?? [], path);
  const option = (path: string) => {
    const failed = failuresOf(path);
    return (
      <option key={path} value={path}>
        {failed > 0 ? `${path} · ${couldNotBeRead(failed)}` : path}
      </option>
    );
  };
  const present = projects.filter((p) => !p.missing).map((p) => p.path);
  const missing = projects.filter((p) => p.missing).map((p) => p.path);
  // A Project picked but no longer listed, such as one hidden since, stays selectable.
  if (project !== null && !projects.some((p) => samePath(p.path, project))) present.push(project);

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
          {project !== null && <FailureBadge count={failuresOf(project)} where="this Project" />}
        </div>
        <select
          className="banner-select"
          value={project ?? ""}
          onChange={(event) => selectProject(event.target.value === "" ? null : event.target.value)}
        >
          <option value="">No Project</option>
          {present.map(option)}
          {missing.length > 0 && <optgroup label="Missing Projects">{missing.map(option)}</optgroup>}
        </select>
      </div>
      <div>
        <div className="caption">Inventory read</div>
        <div className="banner-value" title={inventory?.read_at}>
          {inventory ? readTime(inventory.read_at) : "—"}
          <span className="caption"> · on focus</span>
        </div>
      </div>
      <span className="grow" />
      <button type="button" className="cta secondary" onClick={refresh} disabled={reading}>
        <Icon name="refresh" small />
        {reading ? "Reading" : "Refresh"}
      </button>
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
            <span className="warn" title={flagged.map((flag) => `${flag.agent_name}: ${flag.sentence}`).join("\n")}>
              <Icon name="flag" small />
            </span>
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
function StateBars({ items, state, onState }: { items: Item[]; state: State | null; onState: (state: State | null) => void }) {
  const { total, counts } = stateCounts(items);
  return (
    <section className="panel">
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
            className={`bar state-${barState}${state === barState ? " on" : ""}`}
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

/** A selector per Agent under All Agents, or one Agent's homes as tiles. */
function HomesPane({ inventory, selection, onHome }: HomesPaneProps) {
  const all = inventory?.homes ?? [];
  const failuresOf = (home: Home) => homeFailures(inventory?.read_failures ?? [], home);
  // Another tool copies a Mirrored home, so config-digger only reads it.
  const mirrored = (home: Home) =>
    home.mirror && (
      <span className="tag" title={home.mirror.sentence}>
        {home.mirror.label}
      </span>
    );
  const notRead = (home: Home) =>
    home.not_read && (
      <span className="tag" title={home.not_read.sentence}>
        not read
      </span>
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
          {mirrored(chosen)}
          {notRead(chosen)}
          <FailureBadge count={failuresOf(chosen)} where="this home" />
          {own.length > 1 ? (
            <select value={chosen.home} title={chosen.folder} onChange={(event) => onHome(agent, event.target.value)}>
              {own.map((home) => (
                <option key={home.home} value={home.home}>
                  {homeLabel(home, failuresOf(home))}
                </option>
              ))}
            </select>
          ) : (
            <span className="dim" title={chosen.folder}>
              {chosen.home}
            </span>
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
          <button
            type="button"
            key={home.home}
            className={`tile home-tile${selected ? " selected" : ""}`}
            aria-pressed={selected}
            title={home.no_configuration ?? home.folder}
            onClick={() => onHome(agent, home.home)}
          >
            <span className="home-name">{home.home}</span>
            <span className="grow" />
            {mirrored(home)}
            {notRead(home)}
            <FailureBadge count={failuresOf(home)} where="this home" />
          </button>
        );
      });
  }
  return (
    <section className="panel">
      <h2 className="panel-title">Agent home</h2>
      <div className="home-list">{body}</div>
    </section>
  );
}

function homeLabel(home: Home, failed: number): string {
  let label = home.home;
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
                <span className="warn" title={flagged.map((flag) => flag.sentence).join("\n")}>
                  <Icon name="flag" small />
                </span>
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
        <span className="item-location" title={effective.locator}>
          {effective.scope} · {effective.locator}
        </span>
      </span>
      {effective.git_versioned && <span className="tag">git-versioned</span>}
      {effective.link && <LinkTag link={effective.link} />}
      <span className="item-count">{entries}</span>
      <StatePill state={item.state} />
    </button>
  );
}

type ItemDetailProps = {
  inventory: Inventory;
  item: Item;
  flash: string | null;
  onShowWinner: (locator: string) => void;
  onReader: (reader: AlsoReadBy, locator: string) => void;
  onAction: (verb: ActionVerb) => void;
  notify: Notify;
};

/** An Item's Entries in precedence order, highest first and numbered. */
function ItemDetail({ inventory, item, flash, onShowWinner, onReader, onAction, notify }: ItemDetailProps) {
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
