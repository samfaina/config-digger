// What the Inventory screen shows, worked out from the inventory the core
// resolved: which Items are shown, how they are filtered and grouped, and
// what each Entry card offers, and why it is empty when it lists no Agent
// home. Nothing here reads the disk or renders.

import type {
  Agent,
  AgentFlag,
  AlsoReadBy,
  Entry,
  HowToRemove,
  Home,
  Inventory,
  Item,
  ItemKind,
  ReadFailure,
  State,
  Verb,
} from "./bindings";

/** The Agent home shown for each Agent, by Agent id. */
export type HomeChoice = Record<string, string>;

/** What the Agents and Agent home panes select: `all` for All Agents or one Agent's id, and each Agent's home. */
export type Selection = { agent: string; homes: HomeChoice };

/** The States in the order the screen lists them. */
export const STATE_ORDER: State[] = ["effective", "shadowed", "merged", "disabled", "blocked", "unread", "invalid"];

/** Each Agent once, in the order of its first home. */
export function agentsOf(homes: Home[]): { agent: string; agentName: string }[] {
  const names = new Map<string, string>();
  for (const home of homes) if (!names.has(home.agent)) names.set(home.agent, home.agent_name);
  return [...names].map(([agent, agentName]) => ({ agent, agentName }));
}

/**
 * The home shown for each Agent: the one the user chose while it is listed,
 * otherwise the first one among `items` holding a leftover, otherwise the
 * first one the Agent reads, otherwise its first.
 */
export function chosenHomes(homes: Home[], choice: HomeChoice, items: Item[]): HomeChoice {
  const chosen: HomeChoice = {};
  for (const { agent } of agentsOf(homes)) {
    const own = homes.filter((home) => home.agent === agent);
    const kept = own.find((home) => home.home === choice[agent]);
    const holding = own.find((home) => leftoversIn(items, home).length > 0);
    chosen[agent] = (kept ?? holding ?? own.find((home) => home.not_read === null) ?? own[0]).home;
  }
  return chosen;
}

/**
 * Whether an Entry is a leftover: Unread because it is keyed to a missing
 * Project, and no plugin's, which only the Agent uninstalls.
 */
function isLeftover(entry: Entry): boolean {
  return entry.reason?.code === "project_missing" && entry.plugin === null;
}

/** The leftovers among `items` in the Agent home `home` of `agent`, each once, since an Entry is in one Item per home. */
function leftoversIn(items: Item[], { agent, home }: { agent: string; home: string }): Entry[] {
  return items
    .filter((item) => item.agent === agent && item.home === home)
    .flatMap((item) => item.entries)
    .filter(isLeftover);
}

/** Whether the selection is of `agent`, alone or under All Agents. */
function isSelected(agent: string, selection: Selection): boolean {
  return selection.agent === "all" || selection.agent === agent;
}

/** Whether the selection shows what the Agent home `home` of `agent` holds. */
function isShown({ agent, home }: { agent: string; home: string }, selection: Selection): boolean {
  return isSelected(agent, selection) && selection.homes[agent] === home;
}

/** The Items of the selected Agents, each in its chosen home. */
export function itemsShown(items: Item[], selection: Selection): Item[] {
  return items.filter((item) => isShown(item, selection));
}

/** An Agent home of a selected Agent that holds leftovers the selection doesn't show. */
export type LeftoversNotShown = { agent: string; home: string; sentence: string };

/**
 * The homes of the selected Agents, other than their chosen ones, that hold
 * leftovers of the context's missing Project, so that none is left out unsaid.
 */
export function leftoversNotShown(inventory: Inventory, selection: Selection): LeftoversNotShown[] {
  return inventory.homes
    .filter((home) => isSelected(home.agent, selection) && !isShown(home, selection))
    .flatMap((home) => {
      const count = leftoversIn(inventory.items, home).length;
      if (count === 0) return [];
      const leftovers = count === 1 ? "1 leftover of this Project is" : `${count} leftovers of this Project are`;
      const sentence = `${leftovers} in the Agent home ${home.home} of ${home.agent_name}, which isn't shown.`;
      return [{ agent: home.agent, home: home.home, sentence }];
    });
}

/** The problems of the selected Agents in their chosen homes. */
export function flagsShown(flags: AgentFlag[], selection: Selection): AgentFlag[] {
  return flags.filter((flag) => isShown(flag, selection));
}

/** An Item is in use when an Entry of it takes effect. */
export function inUse(item: Item): boolean {
  return item.state === "effective";
}

export type AgentStats = { items: number; inUse: number; noEffect: number };

/** How many Items are in use, and how many Entries have no effect. */
export function agentStats(items: Item[]): AgentStats {
  const entries = items.flatMap((item) => item.entries);
  return {
    items: items.length,
    inUse: items.filter(inUse).length,
    noEffect: entries.filter((entry) => entry.state !== "effective").length,
  };
}

export type StateCounts = { total: number; counts: { state: State; count: number }[] };

/** How many Entries have each State, for the State bars. */
export function stateCounts(items: Item[]): StateCounts {
  const entries = items.flatMap((item) => item.entries);
  const counts = STATE_ORDER.map((state) => ({ state, count: entries.filter((entry) => entry.state === state).length }));
  return { total: entries.length, counts: counts.filter(({ count }) => count > 0) };
}

export type ItemFilter = { kind: ItemKind | "all"; text: string; state: State | null };

/** The Items of a kind, whose name holds the text, with an Entry in the State. */
export function filterItems(items: Item[], { kind, text, state }: ItemFilter): Item[] {
  const needle = text.trim().toLowerCase();
  return items.filter(
    (item) =>
      (kind === "all" || item.kind === kind) &&
      item.name.toLowerCase().includes(needle) &&
      (state === null || item.entries.some((entry) => entry.state === state)),
  );
}

export type ItemGroup = { key: string; title: string; agent: string | null; items: Item[] };

export const KIND_TITLES: Record<ItemKind, string> = { mcp: "MCP servers", skill: "Skills" };
export const KIND_NAMES: Record<ItemKind, string> = { mcp: "MCP server", skill: "Skill" };

/** Items by Agent under All Agents, by kind for one Agent, leaving out empty groups. */
export function groupItems(items: Item[], agent: string, homes: Home[]): ItemGroup[] {
  const groups: ItemGroup[] =
    agent === "all"
      ? agentsOf(homes).map(({ agent, agentName }) => ({
          key: agent,
          title: agentName,
          agent,
          items: items.filter((item) => item.agent === agent),
        }))
      : (["mcp", "skill"] as const).map((kind) => ({
          key: kind,
          title: KIND_TITLES[kind],
          agent: null,
          items: items.filter((item) => item.kind === kind),
        }));
  return groups.filter((group) => group.items.length > 0);
}

/** Windows compares paths without regard to letter case. */
export function samePath(a: string, b: string): boolean {
  return a.toLowerCase() === b.toLowerCase();
}

/**
 * The Read failures that reach the context, in the chosen homes of the
 * selected Agents: those outside any Project, and those of its Project.
 */
export function failuresInContext(failures: ReadFailure[], project: string | null, selection: Selection): ReadFailure[] {
  return failures.filter(
    (failure) =>
      isShown(failure, selection) &&
      (failure.project === null || (project !== null && samePath(failure.project, project))),
  );
}

/** The Read failures in the folder of the Project at `project`, which badge it. */
export function projectFailures(failures: ReadFailure[], project: string): number {
  return failures.filter((failure) => failure.project !== null && samePath(failure.project, project)).length;
}

/** The Read failures outside any Project in an Agent home, which badge it. */
export function homeFailures(failures: ReadFailure[], home: Home): number {
  return failures.filter(
    (failure) => failure.agent === home.agent && failure.home === home.home && failure.project === null,
  ).length;
}

/** Such as "1 location couldn't be read". */
export function couldNotBeRead(count: number): string {
  return count === 1 ? "1 location couldn't be read" : `${count} locations couldn't be read`;
}

export type PrimaryAction = { verb: Exclude<Verb, "remove">; offered: boolean; note: string };
export type RemoveAction = { offered: boolean; danger: boolean; note: string };
export type EntryActions = {
  /** Disable, or Enable for a Disabled Entry. None for an Entry that can only be Removed. */
  primary: PrimaryAction | null;
  remove: RemoveAction;
  /** How to remove an Entry config-digger does not Remove, such as a plugin's. */
  howToRemove: HowToRemove | null;
};

/** What an Entry card offers, each with a one-line note of what it does or why it is not offered. */
export function entryActions(entry: Entry, agentName: string): EntryActions {
  let primary: PrimaryAction | null = null;
  if (entry.state === "disabled") {
    primary = {
      verb: "enable",
      offered: entry.enable?.status === "offered",
      note: entry.enable?.sentence ?? "Enable is not offered.",
    };
  } else if (!(entry.disable.status === "not_offered" && entry.disable.code === "unread")) {
    primary = { verb: "disable", offered: entry.disable.status === "offered", note: entry.disable.sentence };
  }

  const remove: RemoveAction =
    entry.remove.status === "offered"
      ? { offered: true, danger: primary === null, note: removeReach(entry, agentName) }
      : { offered: false, danger: false, note: entry.remove.sentence };
  const howToRemove = entry.remove.status === "not_offered" ? entry.remove.how_to_remove : null;
  return { primary, remove, howToRemove };
}

/** Remove deletes the Entry for every Agent that reads it. */
function removeReach(entry: Entry, agentName: string): string {
  const agents = [...new Set([agentName, ...entry.also_read_by.map((reader) => reader.agent_name)])];
  let note = `Remove reaches ${agents.join(", ")}.`;
  if (entry.git_versioned) note += " It is git-versioned, so it needs its own confirmation.";
  if (entry.link) note += " It removes the junction only, never its target.";
  return note;
}

/** The Item of another Agent home that holds the Entry at `locator`, when that home is listed. */
export function readerItem(inventory: Inventory, reader: AlsoReadBy, locator: string): Item | null {
  return (
    inventory.items.find(
      (item) =>
        item.agent === reader.agent &&
        item.home === reader.home &&
        item.entries.some((entry) => entry.locator === locator),
    ) ?? null
  );
}

/** An Item as another screen names it. */
export type ItemRef = Pick<Item, "agent" | "home" | "kind" | "name">;

/** The Item `ref` names among `items`, `null` when none is listed. */
export function findItem(items: Item[], ref: ItemRef): Item | null {
  return (
    items.find(
      (item) => item.agent === ref.agent && item.home === ref.home && item.kind === ref.kind && item.name === ref.name,
    ) ?? null
  );
}

/**
 * An Item's identity in the inventory. Its name is not enough: an Agent
 * whose Entries never compete, such as Cursor, has an Item per Entry, so
 * one of its Entries tells it apart, since an Entry is in one Item per home.
 */
export function itemKey(item: Item): string {
  return [item.agent, item.home, item.kind, item.name, item.entries[0]?.locator ?? ""].join("|");
}

/** Why the inventory lists no Agent home. */
export type EmptyReason =
  /** None was found: what was looked for. */
  | { kind: "none_found"; lookedFor: string }
  /** The user hid every one found. */
  | { kind: "all_hidden"; hidden: number };

/** Why an inventory with no Agent home is empty, from every Agent with its homes, hidden ones included. */
export function emptyInventory(agents: Agent[]): EmptyReason {
  const hidden = agents.flatMap((agent) => agent.homes).filter((home) => home.hidden).length;
  if (hidden > 0) return { kind: "all_hidden", hidden };
  const names = agents.map((agent) => agent.name);
  const variables = agents.flatMap((agent) => agent.home_variables);
  const their = names.length === 1 ? "its" : "their";
  const through = variables.length > 0 ? ` and through ${listed(variables)}` : "";
  return {
    kind: "none_found",
    lookedFor:
      `config-digger looked for ${listed(names)} in ${their} default and known locations${through}. ` +
      "It does not scan the disk. If an Agent keeps its configuration somewhere else, add that folder as an Agent home.",
  };
}

/** "A", "A and B", "A, B and C". */
function listed(names: string[]): string {
  if (names.length <= 1) return names.join("");
  return `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}`;
}
