// What the Settings screen shows, worked out from what the core reads.
// Nothing here calls the core or renders.

import type { DataFolder, ParkedEntry, SettingsRead } from "./bindings";
import type { InventoryStart } from "./places";
import { KIND_NAMES } from "./view";

/** One of config-digger's data folders, as Data on this computer lists it. */
export type DataFolderRow = { folder: DataFolder; title: string; path: string; description: string };

/** Each data folder, in the order Settings lists them, with how long History keeps Operations, which is fixed. */
export function dataFolderRows({ folders, operations, retention }: SettingsRead): DataFolderRow[] {
  const kept = `${operations} ${operations === 1 ? "Operation" : "Operations"} kept.`;
  const keeps =
    `History keeps the last ${retention.operations} Operations and everything from the last ${retention.days} days, ` +
    "and prunes older ones with their backups at startup. Operations that need attention are never pruned.";
  return [
    { folder: "backups", title: "Backups and Operation history", path: folders.backups, description: `${kept} ${keeps}` },
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

/** A parked Entry as Settings lists it, with where the Inventory shows its Item, `null` when its home is hidden. */
export type ParkedRow = { key: string; title: string; meta: string; start: InventoryStart | null };

/** Each parked Entry: its name, what it is and where it was. */
export function parkedRows(parked: ParkedEntry[]): ParkedRow[] {
  return parked.map((entry) => ({
    key: entry.parked.locator,
    title: entry.name,
    meta: [
      KIND_NAMES[entry.kind],
      entry.agent_name,
      entry.project,
      `from ${entry.parked.locator}`,
      entry.hidden ? "in a hidden Agent home" : null,
    ]
      .filter((part) => part !== null)
      .join(" · "),
    start: entry.hidden
      ? null
      : {
          project: entry.project,
          agent: entry.agent,
          homes: { [entry.agent]: entry.home },
          state: null,
          item: { agent: entry.agent, home: entry.home, kind: entry.kind, name: entry.name },
        },
  }));
}
