import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test } from "vitest";
import { ActionDialog } from "./ActionDialog";
import { Loading } from "./Loading";

describe("the action dialog while it prepares the preview", () => {
  test("shows the busy indicator naming what it reads, over placeholder rows", () => {
    const markup = renderToStaticMarkup(
      <ActionDialog
        request={{ kind: "entry", verb: "disable", locator: String.raw`C:\Users\me\.claude.json#mcpServers.notes`, project: null }}
        onClose={() => {}}
        onUndo={() => {}}
        onOpenHistory={() => {}}
        notify={() => {}}
      />,
    );

    expect(markup).toContain("Preparing the preview…");
    expect(markup).toContain(renderToStaticMarkup(<Loading text="Reading the files this action touches…" rows={3} />));
  });
});
