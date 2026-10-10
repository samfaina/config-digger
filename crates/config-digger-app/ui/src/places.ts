// What the Projects & homes screen shows, worked out from the Projects and
// Agent homes the core lists: its sections, what each row says and which
// actions it offers. Nothing here calls the core or renders.

import type {
  AgentNoteCode,
  Home,
  HomeSource,
  Installation,
  ListChange,
  Listed,
  Places,
  Project,
  State,
} from "./bindings";
import type { HomeChoice } from "./view";

/** Hide, or Remove from list for what the user added and nothing discovers. */
export type ListOffer = "hide" | "remove_from_list";

/** A Project that exists and is not hidden. */
export type ProjectRow = {
  project: Project;
  path: string;
  /** Folder, or `.code-workspace` with its members. */
  kind: string;
  /** Such as "Known to Claude Code, VS Code" or "Added by you". */
  source: string;
  /** Such as "3 Project Entries". */
  entries: string;
  offer: ListOffer;
};

/** A missing Project that is not hidden. */
export type MissingRow = {
  project: Project;
  path: string;
  source: string;
  /** Each leftover as its file → key path. */
  leftovers: string[];
  /** Such as "2 leftovers". */
  count: string;
  /** Hide is offered when there is no leftover to show or remove. */
  hideOffered: boolean;
};

/** A hidden Project, missing or not: Unhide, and Remove from list for the user's own. */
export type HiddenRow = { project: Project; path: string; source: string; removable: boolean };

/** The Projects tab's sections, in the order it shows them. */
export type ProjectSections = { present: ProjectRow[]; missing: MissingRow[]; hidden: HiddenRow[] };

/** `count` of a noun, such as "1 leftover" or "2 leftovers", or "No leftovers" for none. */
export function counted(count: number, one: string, many: string): string {
  if (count === 0) return `No ${many}`;
  return `${count} ${count === 1 ? one : many}`;
}

/** The Projects tab: the Projects, then the missing ones, then the hidden ones. */
export function projectSections(places: Places): ProjectSections {
  const agentName = (id: string) => places.agents.find((agent) => agent.id === id)?.name ?? id;
  const source = (project: Project) => projectSource(project, project.registered_by.map(agentName));
  const row = (project: Project): ProjectRow => ({
    project,
    path: project.path,
    kind: project.workspace_file
      ? [".code-workspace", ...(project.members.length > 0 ? [`members ${project.members.join(", ")}`] : [])].join(" · ")
      : "Folder",
    source: source(project),
    entries: counted(project.entries, "Project Entry", "Project Entries"),
    offer: removableProject(project) ? "remove_from_list" : "hide",
  });
  const missingRow = (project: Project): MissingRow => {
    const leftovers = leftoversOf(places, project.path);
    return {
      project,
      path: project.path,
      source: `The folder no longer exists · ${lowerFirst(source(project))}`,
      leftovers,
      count: counted(leftovers.length, "leftover", "leftovers"),
      hideOffered: leftovers.length === 0,
    };
  };
  const hiddenRow = (project: Project): HiddenRow => ({
    project,
    path: project.path,
    source: project.missing ? `Missing · ${source(project)}` : source(project),
    removable: removableProject(project),
  });
  const shown = places.projects.filter((project) => !project.hidden);
  return {
    present: shown.filter((project) => !project.missing).map(row),
    missing: shown.filter((project) => project.missing).map(missingRow),
    hidden: places.projects.filter((project) => project.hidden).map(hiddenRow),
  };
}

/** The leftovers of the missing Project at `path`, each once, as file → key path. */
function leftoversOf(places: Places, path: string): string[] {
  const found = places.leftovers.find((leftovers) => leftovers.project === path);
  const entries = (found?.items ?? []).flatMap((item) => item.entries);
  const locations = entries.map((entry) => `${entry.path} → ${entry.locator.slice(entry.path.length + 1)}`);
  return [...new Set(locations)];
}

/** `text` starting in lower case, to follow a separator. */
function lowerFirst(text: string): string {
  return text.charAt(0).toLowerCase() + text.slice(1);
}

/** Where a Project came from: the registries that record it, and the user. */
function projectSource(project: Project, knownTo: string[]): string {
  const known = knownTo.join(", ");
  if (!project.added) return `Known to ${known}`;
  return knownTo.length > 0 ? `Added by you · also known to ${known}` : "Added by you";
}

/** Forgetting a Project a registry records does nothing: it is found again. */
function removableProject(project: Project): boolean {
  return project.added && project.registered_by.length === 0;
}

/** What a home row offers: Show in Inventory, Hide, Unhide, Remove from list. */
export type HomeAction = "show" | "hide" | "unhide" | "remove_from_list";

/** A tag on a home row; a Mirrored home's says it is read-only. */
export type HomeTag = { text: string; tone: "plain" | "read_only" };

/** One Agent home, as the Agent homes tab shows it. */
export type HomeRow = {
  home: Home;
  /** Its id, which is its folder when the user added it. */
  label: string;
  path: string;
  tags: HomeTag[];
  /** What it is, when it is Mirrored, not read or without configuration, and other notes. */
  notes: string[];
  /** Such as "3 Entries". */
  entries: string;
  hidden: boolean;
  actions: HomeAction[];
};

/** One Agent, headed by its Installations' versions, with each of its homes. */
export type HomeGroup = {
  agent: string;
  name: string;
  /** Each Installation's version and where it was read from. */
  installed: string[];
  /** What to know about the versions, such as one outside the tested range. */
  notes: string[];
  definition: string;
  homes: HomeRow[];
};

/** The notes about an Agent's versions, which head its group rather than a home. */
const VERSION_NOTES: AgentNoteCode[] = ["version_outside_tested_range", "version_unreadable", "definition_untested"];

/** The Agent homes tab: every Agent's homes, hidden ones dimmed in place. */
export function homeGroups(places: Places): HomeGroup[] {
  return places.agents.map((agent) => {
    const notes = agent.homes.flatMap((home) => home.notes);
    return {
      agent: agent.id,
      name: agent.name,
      installed: installed(agent.homes.flatMap((home) => home.installations)),
      notes: unique(notes.filter((note) => VERSION_NOTES.includes(note.code)).map((note) => note.sentence)),
      definition: agent.built_in ? "built-in definition" : "definition from a file",
      homes: agent.homes.map(homeRow),
    };
  });
}

/** Each Installation once, with its version and where it was read from. */
function installed(installations: Installation[]): string[] {
  if (installations.length === 0) return ["no Installation found"];
  const each = new Map<string, Installation>();
  for (const installation of installations) if (!each.has(installation.path)) each.set(installation.path, installation);
  return [...each.values()].map((installation) => {
    const version = `${installation.id} ${installation.version ?? "version unknown"}`;
    return installation.version_from ? `${version} · read from ${installation.version_from}` : version;
  });
}

/** A home's row: its tags and notes, and Show in Inventory, Hide, Unhide or Remove from list. */
function homeRow(home: Home): HomeRow {
  const removable = home.source.kind === "added";
  const tags: HomeTag[] = [{ text: sourceTag(home.source), tone: "plain" }];
  if (home.mirror) tags.push({ text: `${home.mirror.label}, read-only · from ${home.mirror.source}`, tone: "read_only" });
  if (home.not_read) tags.push({ text: "not read", tone: "plain" });
  const notes = [
    ...(home.mirror ? [home.mirror.sentence] : []),
    ...(home.not_read ? [home.not_read.sentence] : []),
    ...(home.no_configuration ? [`${home.no_configuration}.`] : []),
    ...home.notes.filter((note) => !VERSION_NOTES.includes(note.code) && note.code !== "no_installation_found").map((note) => note.sentence),
  ];
  const actions: HomeAction[] = home.hidden ? ["unhide"] : ["show", ...(removable ? [] : (["hide"] as const))];
  if (removable) actions.push("remove_from_list");
  return {
    home,
    label: home.home,
    path: home.folder,
    tags,
    notes,
    entries: counted(home.entries, "Entry", "Entries"),
    hidden: home.hidden,
    actions,
  };
}

/** Where a home came from, as its source tag says. */
function sourceTag(source: HomeSource): string {
  switch (source.kind) {
    case "default":
      return "default location";
    case "environment":
      return `environment variable %${source.variable}%`;
    case "known_location":
      return "known location";
    case "added":
      return "added by you";
  }
}

/** Each value once, in the order first seen. */
function unique(values: string[]): string[] {
  return [...new Set(values)];
}

/** Where the Inventory opens: its Project, the Agent selected, each Agent's home and the State filter. */
export type InventoryStart = { project: string | null; agent: string; homes: HomeChoice; state: State | null };

/** What the Projects & homes screen opens the Inventory on. */
export type ShowInInventory =
  | { kind: "project"; path: string }
  /** A missing Project's leftovers, which are Unread. */
  | { kind: "leftovers"; path: string }
  | { kind: "home"; home: Home };

/** Where the Inventory opens to show `show`. */
export function inventoryStart(show: ShowInInventory): InventoryStart {
  switch (show.kind) {
    case "project":
      return { project: show.path, agent: "all", homes: {}, state: null };
    case "leftovers":
      return { project: show.path, agent: "all", homes: {}, state: "unread" };
    case "home":
      return { project: null, agent: show.home.agent, homes: { [show.home.agent]: show.home.home }, state: null };
  }
}

/** The notice after `change` applied to what is now `listed`. */
export function changedSentence(change: ListChange, listed: Listed): string {
  const name = listed.kind === "project" ? listed.project.path : `${listed.home.home} of ${listed.home.agent_name}`;
  const forgotten = `Removed ${name} from config-digger's list. Nothing on disk changed.`;
  switch (change.kind) {
    case "add_project":
      return `Added ${name}: every Agent is evaluated on it.`;
    case "hide_project":
      return `Hid ${name}. Picking it in the Inventory still evaluates it.`;
    case "hide_home":
      return `Hid ${name}: its Items leave the Inventory.`;
    case "unhide_project":
    case "unhide_home":
      return `${name} is listed again.`;
    case "forget_project":
    case "forget_home":
      return forgotten;
    case "add_home": {
      if (listed.kind !== "home") return `Added ${name}.`;
      const added = `Added ${listed.home.folder} as an Agent home of ${listed.home.agent_name}.`;
      return listed.home.no_configuration ? `${added} ${listed.home.no_configuration}.` : added;
    }
  }
}
