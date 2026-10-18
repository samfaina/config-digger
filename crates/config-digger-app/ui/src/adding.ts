// What the Add Project and Add Agent home dialogs offer and warn about,
// worked out from what the user chose and what the core checked. Nothing
// here calls the core or renders.

import type { HomeCheck, Places } from "./bindings";

/** What the header button of each tab adds, as it and the dialog it opens name it. */
export const ADDS = { projects: "Add Project", homes: "Add Agent home" } as const;

/** The Agent and folder chosen in Add Agent home; `""` until each is. */
export type HomeToAdd = { agent: string; folder: string };

/** Add Agent home opens with neither an Agent nor a folder chosen. */
export const NO_HOME_CHOSEN: HomeToAdd = { agent: "", folder: "" };

/** What the core checked, for the choice it was asked about. */
export type HomeChecked = { checked: HomeToAdd; check: HomeCheck };

/** What Add Agent home offers for a choice. */
export type HomeOffer = {
  /** Add is enabled: an Agent and a folder are chosen, and nothing is being added. */
  addable: boolean;
  /** The core's sentence when the folder holds none of the files the Agent reads there. */
  warning: string | null;
};

/** What to ask the core to check for `choice`: nothing until an Agent and a folder are chosen. */
export function homeToCheck(choice: HomeToAdd): HomeToAdd | null {
  const folder = choice.folder.trim();
  return choice.agent !== "" && folder !== "" ? { agent: choice.agent, folder } : null;
}

/** What Add Agent home offers for `choice`, with the last check made and while `busy` adding. */
export function addHomeOffer(choice: HomeToAdd, checked: HomeChecked | null, busy: boolean): HomeOffer {
  const asked = homeToCheck(choice);
  const current = asked !== null && checked?.checked.agent === asked.agent && checked.checked.folder === asked.folder;
  return {
    addable: !busy && asked !== null,
    warning: current ? checked.check.no_configuration : null,
  };
}

/** Add Project's warning when `path` is the folder of an Agent home already listed, hidden or not. */
export function projectWarning(places: Places, path: string): string | null {
  const typed = path.trim();
  if (typed === "") return null;
  const key = folderKey(typed);
  const agents = places.agents.filter((agent) => agent.homes.some((home) => folderKey(home.folder) === key));
  if (agents.length === 0) return null;
  return `${typed} is already listed as an Agent home of ${listed(agents.map((agent) => agent.name))}. As a Project, it is read as a folder Agents are started on, not as their configuration.`;
}

/** A Windows folder as it compares: in any case, either slash, with or without a trailing one. */
function folderKey(folder: string): string {
  return folder.replaceAll("/", "\\").replace(/\\+$/, "").toLowerCase();
}

/** `names` as a sentence lists them: separated by commas, the last by "and". */
function listed(names: string[]): string {
  return names.length > 1 ? `${names.slice(0, -1).join(", ")} and ${names[names.length - 1]}` : names.join("");
}
