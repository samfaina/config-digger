import { readdirSync, readFileSync } from "node:fs";
import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test } from "vitest";
import { Notice } from "./Notice";

describe("a notice", () => {
  test("of information shows its text with the information look, read out politely", () => {
    const html = renderToStaticMarkup(<Notice notice={{ kind: "information", text: "config-digger has kept nothing there yet." }} />);
    expect(html).toContain("config-digger has kept nothing there yet.");
    expect(html).toMatch(/class="notice information"/);
    expect(html).toMatch(/role="status"/);
  });

  test("of success shows its text with the success look, read out politely", () => {
    const html = renderToStaticMarkup(<Notice notice={{ kind: "success", text: "Copied the path." }} />);
    expect(html).toContain("Copied the path.");
    expect(html).toMatch(/class="notice success"/);
    expect(html).toMatch(/role="status"/);
  });

  test("of an error shows its text with the error look, read out at once", () => {
    const html = renderToStaticMarkup(<Notice notice={{ kind: "error", text: "The path couldn't be copied: denied" }} />);
    expect(html).toContain("The path couldn&#x27;t be copied: denied");
    expect(html).toMatch(/class="notice error"/);
    expect(html).toMatch(/role="alert"/);
  });

  test("of each kind leads with an icon of its own, hidden from screen readers", () => {
    const icons = (["information", "success", "error"] as const).map((kind) => {
      const html = renderToStaticMarkup(<Notice notice={{ kind, text: "Text." }} />);
      const icon = html.match(/<svg[^>]*>[\s\S]*<\/svg>/)?.[0] ?? "";
      expect(icon, kind).toMatch(/aria-hidden="true"/);
      expect(html.indexOf("<svg"), kind).toBeLessThan(html.indexOf("Text."));
      // The mask ids differ on every render, so they are left out.
      return icon.replace(/(id|mask)="[^"]*"/g, "");
    });
    expect(new Set(icons).size).toBe(3);
  });
});

describe("the app's notices", () => {
  test("are all rendered by Notice, as no other component draws a .notice of its own", () => {
    const folder = new URL("./", import.meta.url);
    const drawing = readdirSync(folder)
      .filter((file) => file.endsWith(".tsx") && !file.endsWith(".test.tsx"))
      .filter((file) => /className=\{?[`"]notice\b/.test(readFileSync(new URL(file, folder), "utf8")));
    expect(drawing).toEqual(["Notice.tsx"]);
  });
});
