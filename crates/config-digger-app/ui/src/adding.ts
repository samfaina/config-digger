// What the Add Project and Add Agent home dialogs offer and warn about,
// worked out from what the user chose and what the core checked. Nothing
// here calls the core or renders.

import type { HomeCheck, ProjectCheck } from "./bindings";

/** What the header button of each tab adds, as it and the dialog it opens name it. */
export const ADD_LABELS = { projects: "Add Project", homes: "Add Agent home" } as const;

/** What an Add dialog offers for what is chosen in it. */
export type AddOffer = {
  /** Add is enabled: what to add is chosen and checked, and nothing is being added. */
  addable: boolean;
  /** What the core's check says to know before Add, which does not stop it. */
  warning: string | null;
};

/**
 * The last check the core made, for what it was asked about. `result` is
 * `null` when the check failed, such as for a folder that does not exist,
 * which Add then refuses with the core's reason.
 */
export type Checked<Asked, Result> = { asked: Asked; result: Result | null };

/** What `checked` lets an Add dialog offer for `asked`, what is chosen now, while `busy` adding. */
function offer<Asked, Result>(
  asked: Asked | null,
  checked: Checked<Asked, Result> | null,
  same: (a: Asked, b: Asked) => boolean,
  warning: (result: Result) => string | null,
  busy: boolean,
): AddOffer {
  const current = asked !== null && checked !== null && same(checked.asked, asked) ? checked : null;
  return {
    addable: !busy && current !== null,
    warning: current?.result ? warning(current.result) : null,
  };
}

/** The Agent and folder chosen in Add Agent home; `""` until each is. */
export type HomeToAdd = { agent: string; folder: string };

/** Add Agent home opens with neither an Agent nor a folder chosen. */
export const NO_HOME_CHOSEN: HomeToAdd = { agent: "", folder: "" };

/** The core's check of a folder as an Agent home of an Agent. */
export type HomeChecked = Checked<HomeToAdd, HomeCheck>;

/** What to ask the core to check for `choice`: nothing until an Agent and a folder are chosen. */
export function homeToCheck(choice: HomeToAdd): HomeToAdd | null {
  const folder = choice.folder.trim();
  return choice.agent !== "" && folder !== "" ? { agent: choice.agent, folder } : null;
}

/** What Add Agent home offers for `choice`, with the last check made and while `busy` adding. */
export function addHomeOffer(choice: HomeToAdd, checked: HomeChecked | null, busy: boolean): AddOffer {
  return offer(
    homeToCheck(choice),
    checked,
    (a, b) => a.agent === b.agent && a.folder === b.folder,
    (result) => result.no_configuration,
    busy,
  );
}

/** The core's check of a path as a Project. */
export type ProjectChecked = Checked<string, ProjectCheck>;

/** What to ask the core to check for the path `typed`: nothing until one is typed or picked. */
export function projectToCheck(typed: string): string | null {
  const path = typed.trim();
  return path !== "" ? path : null;
}

/** What Add Project offers for the path `typed`, with the last check made and while `busy` adding. */
export function addProjectOffer(typed: string, checked: ProjectChecked | null, busy: boolean): AddOffer {
  return offer(projectToCheck(typed), checked, (a, b) => a === b, (result) => result.warning, busy);
}
