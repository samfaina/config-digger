import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test } from "vitest";
import { ProjectsScreen, type Tab } from "./ProjectsScreen";

const DESCRIPTIONS: Record<Tab, string> = {
  projects: "Projects come from the Agents' own registries plus the folders you add. config-digger never scans the disk.",
  homes: "Each Agent reads its user configuration from one or more homes. Entries in different homes never compete.",
};

const screen = (tab: Tab) => renderToStaticMarkup(<ProjectsScreen onShow={() => {}} onOpenHistory={() => {}} start={{ tab, adding: false }} />);

describe("the Projects & homes screen", () => {
  for (const tab of ["projects", "homes"] as const) {
    test(`keeps the description of the ${tab} tab out of the banner, at the top of the list`, () => {
      const markup = screen(tab);
      const banner = markup.match(/<header class="panel banner">[\s\S]*?<\/header>/)?.[0] ?? "";
      const list = markup.match(/<section class="panel places-list">[\s\S]*?<\/section>/)?.[0] ?? "";
      expect(banner).toContain("Projects &amp; homes");
      expect(banner).not.toContain(DESCRIPTIONS[tab].replace(/'/g, "&#x27;"));
      expect(list).toMatch(new RegExp(`^<section class="panel places-list"><p class="places-about dim">${DESCRIPTIONS[tab].replace(/'/g, "&#x27;").replace(/\./g, "\.")}</p>`));
    });
  }
});
