import type { ReactElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test, vi } from "vitest";
import { InventoryScreen } from "./Inventory";
import { Loading } from "./Loading";
import { inventoryStart, type InventoryStart } from "./places";

// A tooltip portals into `document.body`, which a render to markup has not.
vi.mock("./Tooltip", () => ({ Tooltip: ({ children }: { children: ReactElement }) => children }));

const screen = (start?: InventoryStart) =>
  renderToStaticMarkup(<InventoryScreen start={start} onOpenHistory={() => {}} onAddHome={() => {}} onOpenHomes={() => {}} />);

const LOADING = renderToStaticMarkup(<Loading text="Reading the inventory…" rows={8} />);

describe("the Inventory before its first read is done", () => {
  test("fills its body with the loading state instead of empty Agents, Items and detail panes", () => {
    const markup = screen();
    expect(markup).toContain(LOADING);
    expect(markup).not.toContain('class="panes"');
    expect(markup).not.toContain('<p class="empty">');
  });

  test("shows the loading state at once when Show leftovers opens it on a missing Project", () => {
    const markup = screen(inventoryStart({ kind: "leftovers", path: String.raw`E:\cd-throwaway\project-gone` }));
    expect(markup).toContain(LOADING);
    expect(markup).not.toContain('class="panes"');
  });
});
