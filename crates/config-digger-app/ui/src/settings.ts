// What the Settings screen and the empty inventory show, worked out from what
// the core reads. Nothing here calls the core or renders.

import type { Agent, DataFolder, ParkedEntry, SettingsRead } from "./bindings";
import type { InventoryStart } from "./places";
import { KIND_NAMES } from "./view";

/** One of config-digger's data folders, as Data on this computer lists it. */
export type DataFolderRow = { folder: DataFolder; title: string; path: string; description: string };

/** How long History keeps Operations. It is fixed in v0.1. */
const RETENTION =
  "History keeps the last 50 Operations and everything from the last 30 days, " +
  "and prunes older ones with their backups at startup. Operations that need attention are never pruned.";

/** Each data folder, in the order Settings lists them. */
export function dataFolderRows({ folders, operations }: SettingsRead): DataFolderRow[] {
  const kept = `${operations} ${operations === 1 ? "Operation" : "Operations"} kept.`;
  return [
    { folder: "backups", title: "Backups and Operation history", path: folders.backups, description: `${kept} ${RETENTION}` },
    {
      folder: "parked",
      title: "Parked Entries",
      path: folders.parked,
      description: "Entries config-digger Disabled for Agents that have no Disable of their own. Never pruned: Enable puts them back.",
    },
    {
      folder: "logs",
      title: "Logs",
      path: folders.logs,
      description: "Paths, key paths, reason codes and errors only, never configuration values.",
    },
    {
      folder: "lists",
      title: "Your lists and settings",
      path: folders.lists,
      description: "The Agent homes and Projects you added or hid, and these settings.",
    },
  ];
}

/** A parked Entry as Settings lists it, with where the Inventory shows its Item. */
export type ParkedRow = { key: string; title: string; meta: string; start: InventoryStart };

/** Each parked Entry: its name, what it is and where it was. */
export function parkedRows(parked: ParkedEntry[]): ParkedRow[] {
  return parked.map((entry) => ({
    key: entry.parked.locator,
    title: entry.name,
    meta: [KIND_NAMES[entry.kind], entry.agent_name, entry.project, `from ${entry.parked.locator}`]
      .filter((part) => part !== null)
      .join(" · "),
    start: {
      project: entry.project,
      agent: entry.agent,
      homes: { [entry.agent]: entry.home },
      state: null,
      item: { agent: entry.agent, home: entry.home, kind: entry.kind, name: entry.name },
    },
  }));
}

/** Why the inventory lists no Agent home. */
export type EmptyInventory =
  /** None was found: what was looked for. */
  | { kind: "none_found"; lookedFor: string }
  /** The user hid every one found. */
  | { kind: "all_hidden"; hidden: number };

/** Why an inventory with no Agent home is empty, from every Agent with its homes, hidden ones included. */
export function emptyInventory(agents: Agent[]): EmptyInventory {
  const hidden = agents.reduce((count, agent) => count + agent.homes.length, 0);
  if (hidden > 0) return { kind: "all_hidden", hidden };
  const names = agents.map((agent) => agent.name);
  const variables = agents.flatMap((agent) => agent.variables);
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
