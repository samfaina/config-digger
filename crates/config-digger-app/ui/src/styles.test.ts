import { readFileSync } from "node:fs";
import { describe, expect, test } from "vitest";

const css = readFileSync(new URL("./styles.css", import.meta.url), "utf8");

type Rule = { selector: string; at: string[]; declarations: Map<string, string> };

/** Every style rule of `source`, with the at-rules it sits in. */
const rules = (source: string): Rule[] => {
  const found: Rule[] = [];
  const open: string[] = [];
  let text = "";
  for (const char of source.replace(/\/\*[\s\S]*?\*\//g, "")) {
    if (char === "{") {
      open.push(text.trim());
      text = "";
    } else if (char === "}") {
      const selector = open.pop() ?? "";
      if (!selector.startsWith("@")) {
        const declarations = new Map(
          text
            .split(";")
            .map((line) => line.split(":"))
            .filter((parts) => parts.length > 1)
            .map(([name, ...value]) => [name.trim(), value.join(":").trim()]),
        );
        found.push({ selector, at: open.filter((outer) => outer.startsWith("@")), declarations });
      }
      text = "";
    } else {
      text += char;
    }
  }
  return found;
};

const all = rules(css);
const tokens = new Set([...(all.find((rule) => rule.selector === ":root")?.declarations.keys() ?? [])]);
const isScrollbar = (rule: Rule) => rule.selector.includes("::-webkit-scrollbar") || [...rule.declarations.keys()].some((name) => name.startsWith("scrollbar-"));
const scrollbarRules = all.filter(isScrollbar);
const declared = (selector: string, name: string) => scrollbarRules.find((rule) => rule.selector === selector)?.declarations.get(name);

/** The colours of a declaration's value, which must each be a `:root` token or `transparent`. */
const colours = (value: string) => value.replace(/var\(--[\w-]+\)|transparent/g, "").match(/#[0-9a-f]+|rgba?\(|hsla?\(/gi) ?? [];
const tokensUsed = (value: string) => [...value.matchAll(/var\((--[\w-]+)\)/g)].map(([, token]) => token);

describe("the app's scrollbars", () => {
  test("are thin, with a thumb on a transparent track, for the standard properties", () => {
    expect(declared("*", "scrollbar-width")).toBe("thin");
    expect(declared("*", "scrollbar-color")).toMatch(/^var\(--[\w-]+\) transparent$/);
  });

  test("have a thumb that changes colour on hover in WebView2", () => {
    const thumb = declared("::-webkit-scrollbar-thumb", "background-color");
    const hover = declared("::-webkit-scrollbar-thumb:hover", "background-color");
    expect(thumb).toMatch(/^var\(--[\w-]+\)$/);
    expect(hover).toMatch(/^var\(--[\w-]+\)$/);
    expect(hover).not.toBe(thumb);
    expect(declared("::-webkit-scrollbar", "width")).toBeDefined();
    expect(declared("::-webkit-scrollbar", "height")).toBe(declared("::-webkit-scrollbar", "width"));
  });

  test("take every colour from the :root tokens", () => {
    for (const rule of scrollbarRules) {
      for (const value of rule.declarations.values()) {
        expect(colours(value), `${rule.selector}: ${value}`).toEqual([]);
        for (const token of tokensUsed(value)) expect(tokens, `${rule.selector}: ${value}`).toContain(token);
      }
    }
  });

  test("are set once, for every element, so no scrolling area draws its own", () => {
    const scrolling = all.filter((rule) => ["overflow", "overflow-x", "overflow-y"].some((name) => /auto|scroll/.test(rule.declarations.get(name) ?? "")));
    expect(scrolling.map((rule) => rule.selector)).toEqual(expect.arrayContaining([".scroll", ".select-panel", ".dialog", ".diff"]));
    for (const selector of scrollbarRules.flatMap((rule) => rule.selector.split(","))) {
      expect(selector.trim()).toMatch(/^(\*|::-webkit-scrollbar[\w-]*(:hover)?)$/);
    }
  });

  test("leave the standard properties to browsers without ::-webkit-scrollbar, as Chromium ignores those rules where they are set", () => {
    for (const rule of scrollbarRules.filter((rule) => !rule.selector.includes("::-webkit-scrollbar"))) {
      expect(rule.at).toEqual(["@supports not selector(::-webkit-scrollbar)"]);
    }
  });
});
