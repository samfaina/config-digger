/**
 * The fake backend: what `npm run dev:fake` puts in place of
 * `@tauri-apps/api/core`, the one module through which `bindings.ts` reaches
 * the app. Its `invoke` answers each command from fixtures that
 * `cargo xtask fake-backend` recorded over the UAT's throwaway data, so the
 * UI runs in a browser, where an agent can open a screen and take its
 * screenshot. Write commands answer with a plausible fixture and change
 * nothing: every answer is a copy of the same fixture.
 *
 * `?fail=open_location,read_about` in the page's address makes the commands
 * it names answer an error, to show how a screen reports one.
 */
import type {
  About,
  ActionPreview,
  ActionRequest,
  Backup,
  DataFolder,
  Error as CommandError,
  Executed,
  HomeCheck,
  Inventory,
  ListChanged,
  OperationDetail,
  OperationSummary,
  Places,
  ProjectCheck,
  SettingsRead,
  UpdateStatus,
} from "../bindings";
import { fixtures } from "./fixtures";

/** What the fake backend answers from, as `cargo xtask fake-backend` records it. */
export type Fixtures = {
  /** The inventory of no Project, first, then of each Project recorded. */
  inventories: { project: string | null; inventory: Inventory }[];
  places: Places;
  listChanged: ListChanged;
  homeCheck: HomeCheck;
  projectCheck: ProjectCheck;
  history: OperationSummary[];
  details: OperationDetail[];
  /** The backups of each location that has any. */
  backups: { path: string; backups: Backup[] }[];
  /** A preview of each action offered on each Entry, Undo and Delete backup. */
  previews: { request: ActionRequest; preview: ActionPreview }[];
  executed: Executed;
  settings: SettingsRead;
  /** Whether config-digger kept anything in each data folder. */
  dataFolders: Record<DataFolder, boolean>;
  about: About;
  updateStatus: UpdateStatus;
};

type Args = Record<string, unknown>;

/** Tauri's `invoke`, answered from the fixtures. */
export async function invoke<T>(command: string, args: Args = {}): Promise<T> {
  if (failing().includes(command)) {
    throw backendError(`The fake backend fails ${command}, as the page's fail parameter asks.`);
  }
  const answer = ANSWERS[command];
  if (answer === undefined) throw backendError(`The fake backend has no answer for ${command}.`);
  return structuredClone(answer(args)) as T;
}

const ANSWERS: Record<string, (args: Args) => unknown> = {
  read_inventory: ({ project }) => inventoryOf(project as string | null),
  switch_project: ({ project }) => inventoryOf(project as string | null),
  backups_of: ({ path }) => fixtures.backups.find((known) => samePath(known.path, path as string))?.backups ?? [],
  open_location: () => null,
  reveal_location: () => null,
  plan_action: ({ request }) => previewOf(request as ActionRequest),
  execute_plan: () => fixtures.executed,
  cancel_plan: () => null,
  read_history: () => fixtures.history,
  history_detail: ({ id }) => fixtures.details.find((detail) => detail.summary.id === id) ?? fixtures.details[0],
  dismiss_operation: ({ id }) => fixtures.history.find((operation) => operation.id === id) ?? fixtures.history[0],
  read_places: () => fixtures.places,
  change_list: () => fixtures.listChanged,
  check_home: () => fixtures.homeCheck,
  check_project: () => fixtures.projectCheck,
  // The folder whose check is recorded.
  pick_location: () => fixtures.projectCheck.path,
  read_settings: () => fixtures.settings,
  save_settings: ({ settings }) => settings,
  open_data_folder: ({ folder }) => fixtures.dataFolders[folder as DataFolder],
  read_about: () => fixtures.about,
  open_page: () => null,
  update_status: () => fixtures.updateStatus,
  startup_update_check: () => fixtures.updateStatus,
  check_for_updates: () => fixtures.updateStatus,
  install_update: () => null,
  open_update_notes: () => null,
};

/** The inventory recorded for `project`, or else that of no Project, in `project`. */
function inventoryOf(project: string | null): Inventory {
  const [none, ...projects] = fixtures.inventories;
  if (project === null) return none.inventory;
  const recorded = projects.find((known) => known.project !== null && samePath(known.project, project));
  return recorded?.inventory ?? { ...none.inventory, project };
}

/** The preview recorded for `request`, or else one of the same kind of action. */
function previewOf(request: ActionRequest): ActionPreview {
  const { previews } = fixtures;
  const same = (known: ActionRequest) => {
    if (known.kind !== request.kind) return false;
    switch (request.kind) {
      case "entry":
        return known.kind === "entry" && known.verb === request.verb && known.locator === request.locator;
      case "leftovers":
        return known.kind === "leftovers" && samePath(known.project, request.project);
      case "stranded":
        return true;
      default:
        return "operation" in known && known.operation === request.operation;
    }
  };
  const alike = (known: ActionRequest) =>
    known.kind === request.kind && (request.kind !== "entry" || (known.kind === "entry" && known.verb === request.verb));
  return (previews.find((known) => same(known.request)) ?? previews.find((known) => alike(known.request)) ?? previews[0])
    .preview;
}

/** The commands the page's `fail` parameter names. */
function failing(): string[] {
  const search = globalThis.location?.search ?? "";
  return new URLSearchParams(search).get("fail")?.split(",") ?? [];
}

function backendError(message: string): CommandError {
  return { code: "unexpected", message, candidates: [], backup: null, diffs: [], failed: null };
}

/** Windows compares paths without regard to letter case. */
function samePath(a: string, b: string): boolean {
  return a.toLowerCase() === b.toLowerCase();
}
