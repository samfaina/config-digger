// @vitest-environment happy-dom
import { cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, test, vi } from "vitest";
import { ActionDialog, Changes } from "./ActionDialog";
import { commands, type ActionPreview, type Change, type Entry, type Home, type Mirror } from "./bindings";
import { EntryCard } from "./EntryCard";
import { Banner, HomesPane, InventoryScreen } from "./Inventory";
import { entry, home, inventory } from "./testData";
import { homeChoices } from "./view";

vi.mock("./bindings", async (importOriginal) => {
  const actual = await importOriginal<typeof import("./bindings")>();
  return { ...actual, commands: { ...actual.commands, planAction: vi.fn(), cancelPlan: vi.fn(async () => ({ status: "ok", data: null })), readInventory: vi.fn(), readHistory: vi.fn(async () => ({ status: "ok", data: [] })) } };
});

afterEach(cleanup);

/** Tabs until the focus is on `target`, or on the element whose text it is. */
async function tabTo(target: string | Element) {
  const user = userEvent.setup();
  const reached = (focused: Element | null) => (typeof target === "string" ? focused?.textContent === target : focused === target);
  for (let stops = 0; stops < 30; stops++) {
    await user.tab();
    if (reached(document.activeElement)) return;
  }
  throw new Error(`No tab stop reaches ${typeof target === "string" ? target : target.outerHTML}`);
}

/** The text of the one tooltip shown. */
const shownTooltip = () => screen.getByRole("tooltip").textContent;

const claudeJson = String.raw`E:\cd-throwaway\claude-code-home\.claude.json`;
const orca = String.raw`E:\cd-throwaway\orca-codex-home`;
const mirror: Mirror = {
  owner: "Orca",
  source: "default",
  source_folder: String.raw`C:\Users\me\.codex`,
  label: "Mirrored by Orca",
  sentence: "Orca copies this home from default: config-digger only reads it.",
};
const notRead = { read_instead: "default", sentence: "Codex reads default instead, as CODEX_HOME is not set to it." };

describe("a keyboard user on the All Agents home rows", () => {
  const pane = (shown = home()) => render(<HomesPane inventory={inventory({ homes: [shown] })} selection={{ agent: "all", homes: {} }} onHome={() => {}} />);

  test("reads a Mirrored home's sentence on its tag", async () => {
    pane(home({ home: orca, folder: orca, mirror }));
    await tabTo("Mirrored by Orca");
    expect(shownTooltip()).toBe(mirror.sentence);
  });

  test("reads why a home is not read on its tag", async () => {
    pane(home({ home: orca, folder: orca, not_read: notRead }));
    await tabTo("not read");
    expect(shownTooltip()).toBe(notRead.sentence);
  });

  test("reads the folder of an Agent's only home on its name", async () => {
    pane(home({ home: "default", folder: String.raw`C:\Users\me\.codex` }));
    await tabTo("default");
    expect(shownTooltip()).toBe(String.raw`C:\Users\me\.codex`);
  });
});

describe("a keyboard user on one Agent's home tiles", () => {
  const tiles = (...shown: Home[]) =>
    render(<HomesPane inventory={inventory({ homes: shown })} selection={{ agent: "codex", homes: { codex: "default" } }} onHome={() => {}} />);
  const tile = (name: string) => screen.getAllByRole("button").find((button) => button.querySelector(".home-name")?.textContent === name)!;

  test("reads a Mirrored home's sentence after its folder in the tile's tooltip", async () => {
    tiles(home(), home({ home: orca, folder: orca, mirror }));
    await tabTo(tile(orca));
    expect(shownTooltip()).toBe(`${orca}\n${mirror.sentence}`);
  });

  test("reads why a home is not read after its no-configuration text in the tile's tooltip", async () => {
    const empty = "None of the files Codex reads in this home exists.";
    tiles(home(), home({ home: orca, folder: orca, no_configuration: empty, not_read: notRead }));
    await tabTo(tile(orca));
    expect(shownTooltip()).toBe(`${empty}\n${notRead.sentence}`);
  });
});

describe("a keyboard user on the banner", () => {
  test("reads the exact time the inventory was read", async () => {
    render(
      <Banner
        inventory={inventory({ read_at: "2026-10-20T09:30:12Z" })}
        project={null}
        choices={homeChoices()}
        selection={{ agent: "all", homes: {} }}
        agentName="All Agents"
        stats={{ items: 0, inUse: 0, noEffect: 0 }}
        reading={false}
        rereading={false}
        refresh={() => {}}
        selectProject={() => {}}
      />,
    );
    await tabTo(screen.getByText("Inventory read").nextElementSibling!);
    expect(shownTooltip()).toBe("2026-10-20T09:30:12Z");
  });
});

describe("a keyboard user on an Entry card", () => {
  const card = (shown: Entry) =>
    render(
      <EntryCard
        entry={shown}
        rank={1}
        agentName="Claude Code"
        flash={false}
        busy={false}
        inItem={() => true}
        onShowWinner={() => {}}
        onReader={null}
        onAction={() => {}}
        notify={() => {}}
      />,
    );

  test("reads when a parked Entry was parked on its parked from tag", async () => {
    const locator = `${claudeJson}#mcpServers.cd-throwaway-desktop`;
    card(entry({ state: "disabled", parked: { path: claudeJson, key_path: ["mcpServers", "cd-throwaway-desktop"], locator, parked_at: "2026-10-20T09:30:12Z" } }));
    await tabTo(`parked from ${locator}`);
    expect(shownTooltip()).toBe(`Parked from ${locator}\non 2026-10-20T09:30:12Z`);
  });
});

describe("a keyboard user on the Items list", () => {
  test("reads the path behind a flag alert above it", async () => {
    const config = String.raw`E:\cd-throwaway\codex-home\config.toml`;
    const flag = { agent: "codex", agent_name: "Codex", home: "default", code: "cannot_start" as const, path: config, sentence: "Codex refuses to start: its config.toml can't be parsed." };
    vi.mocked(commands.readInventory).mockResolvedValue({ status: "ok", data: inventory({ agent_flags: [flag] }) });
    render(<InventoryScreen onOpenHistory={() => {}} onAddHome={() => {}} onOpenHomes={() => {}} />);
    await screen.findAllByText(`Codex: ${flag.sentence}`);
    await tabTo(`Codex: ${flag.sentence}`);
    expect(shownTooltip()).toBe(config);
  });
});

describe("a keyboard user in the action dialog", () => {
  test("reads the path behind an info line of Before you apply", async () => {
    const claude = String.raw`E:\cd-throwaway\bin\claude.exe`;
    const sentence = "Claude Code 2.1.310 is newer than the versions this definition was tested with (2.1.286 to 2.1.302).";
    const planned: ActionPreview = {
      plan_id: "plan-1",
      preview: {
        action: "disable",
        title: "Disable 1 Entry",
        undo_of: null,
        backup_of: null,
        project: null,
        changes: [],
        effective: [],
        info: [{ code: "version_outside_tested_range", path: claude, sentence }],
        files: [],
        folders: [],
        deleted: [],
        backup_folder: String.raw`E:\cd-throwaway\backups\1`,
        backup_note: String.raw`Each touched file is copied to E:\cd-throwaway\backups\1 before anything is written.`,
      },
      warnings: [],
      blocks: [],
    };
    vi.mocked(commands.planAction).mockResolvedValue({ status: "ok", data: planned });
    render(
      <ActionDialog
        request={{ kind: "entry", verb: "disable", locator: `${claudeJson}#mcpServers.cd-throwaway-desktop`, project: null }}
        onClose={() => {}}
        onUndo={() => {}}
        onOpenHistory={() => {}}
        notify={() => {}}
      />,
    );
    await screen.findByRole("heading", { name: "Before you apply" });
    await tabTo(sentence);
    expect(shownTooltip()).toBe(claude);
  });

  test("reads the path of a file in What changes on its name", async () => {
    const change: Change = {
      agent: "claude-code",
      agent_name: "Claude Code",
      home: "default",
      item: "cd-throwaway-desktop",
      path: claudeJson,
      locator: `${claudeJson}#mcpServers.cd-throwaway-desktop`,
      project: null,
      before: "effective",
      after: "disabled",
    };
    render(<Changes changes={[change]} notify={() => {}} />);
    await tabTo(".claude.json");
    expect(shownTooltip()).toBe(claudeJson);
  });
});
