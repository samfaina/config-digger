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

const allRules = rules(css);
const rootValues = allRules.find((rule) => rule.selector === ":root")?.declarations ?? new Map<string, string>();
const tokens = new Set(rootValues.keys());
const isScrollbar = (rule: Rule) => rule.selector.includes("::-webkit-scrollbar") || [...rule.declarations.keys()].some((name) => name.startsWith("scrollbar-"));
const scrollbarRules = allRules.filter(isScrollbar);
const declared = (selector: string, name: string) => scrollbarRules.find((rule) => rule.selector === selector)?.declarations.get(name);

/** The colours of a declaration's value that are neither a `:root` token nor `transparent`. */
const rawColours = (value: string) => value.replace(/var\(--[\w-]+\)|transparent/g, "").match(/#[0-9a-f]+|rgba?\(|hsla?\(/gi) ?? [];
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
        expect(rawColours(value), `${rule.selector}: ${value}`).toEqual([]);
        for (const token of tokensUsed(value)) expect(tokens, `${rule.selector}: ${value}`).toContain(token);
      }
    }
  });

  test("are set once, for every element, so no scrolling area draws its own", () => {
    const scrolling = allRules.filter((rule) => ["overflow", "overflow-x", "overflow-y"].some((name) => /auto|scroll/.test(rule.declarations.get(name) ?? "")));
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

const rule = (selector: string) => allRules.find((found) => found.selector === selector)?.declarations ?? new Map<string, string>();
const KINDS = ["information", "success", "error"] as const;

/** The colour a `var(--token)` value names, as `#rrggbb`. */
const colour = (value: string | undefined) => {
  const token = value?.match(/^var\((--[\w-]+)\)$/)?.[1];
  const hex = token === undefined ? undefined : rootValues.get(token);
  expect(hex, `${value} is a :root token holding an opaque colour`).toMatch(/^#[0-9a-f]{6}$/i);
  return hex as string;
};

/** The WCAG 2 contrast ratio of two `#rrggbb` colours. */
const contrast = (one: string, other: string) => {
  const luminance = (hex: string) => {
    const [r, g, b] = [1, 3, 5].map((at) => {
      const channel = parseInt(hex.slice(at, at + 2), 16) / 255;
      return channel <= 0.04045 ? channel / 12.92 : ((channel + 0.055) / 1.055) ** 2.4;
    });
    return 0.2126 * r + 0.7152 * g + 0.0722 * b;
  };
  const [light, dark] = [luminance(one), luminance(other)].sort((a, b) => b - a);
  return (light + 0.05) / (dark + 0.05);
};

describe("the contrast helper of these tests", () => {
  test("gives the ratios WCAG gives", () => {
    expect(contrast("#ffffff", "#000000")).toBeCloseTo(21, 5);
    expect(contrast("#777777", "#ffffff")).toBeCloseTo(4.48, 2);
  });
});

describe("a notice", () => {
  test("has the corner radius of the app's cards, not a pill's", () => {
    expect(rule(".notice").get("border-radius")).toBe(rule(".entry-card").get("border-radius"));
  });

  test("of each kind takes its background and accent edge from :root tokens of its own", () => {
    for (const kind of KINDS) {
      const declarations = rule(`.notice.${kind}`);
      for (const name of ["background", "border-left-color"]) {
        expect(declarations.get(name), `.notice.${kind} ${name}`).toMatch(new RegExp(String.raw`^var\(--notice-${kind}[\w-]*\)$`));
        colour(declarations.get(name));
      }
    }
    expect(rule(".notice").get("border-left-width")).toMatch(/^[3-9]px$/);
  });

  test("differs by kind, in its background and in its accent edge", () => {
    for (const name of ["background", "border-left-color"]) {
      expect(new Set(KINDS.map((kind) => colour(rule(`.notice.${kind}`).get(name)))).size, name).toBe(3);
    }
  });

  test("stands out from every surface of the app", () => {
    const surfaces = ["--background", "--rail", "--panel", "--tile", "--tile-hover", "--button"].map((token) => colour(`var(${token})`));
    for (const kind of KINDS) {
      const declarations = rule(`.notice.${kind}`);
      const background = colour(declarations.get("background"));
      const edge = colour(declarations.get("border-left-color"));
      // Lighter than every surface by a clear step, not only by its hue.
      for (const surface of surfaces) expect(contrast(background, surface), `${kind} on ${surface}`).toBeGreaterThanOrEqual(1.6);
      // An accent edge is a graphic that has to be seen: WCAG asks 3:1 of it.
      for (const surface of [...surfaces, background]) expect(contrast(edge, surface), `${kind} edge on ${surface}`).toBeGreaterThanOrEqual(3);
    }
  });

  test("of each kind has text that meets WCAG AA on its background", () => {
    const text = colour(rule(".notice").get("color"));
    for (const kind of KINDS) {
      expect(contrast(text, colour(rule(`.notice.${kind}`).get("background"))), kind).toBeGreaterThanOrEqual(4.5);
    }
  });

  test("shows above an open dialog and the Undo snackbar, as a dialog's buttons notify too", () => {
    const layer = (selector: string) => Number(rule(selector).get("z-index"));
    expect(layer(".notice")).toBeGreaterThan(layer(".dialog-backdrop"));
    expect(layer(".notice")).toBeGreaterThan(layer(".snackbar"));
  });

  test("of each kind draws its icon in the colour of its accent edge", () => {
    for (const kind of KINDS) {
      const edge = rule(`.notice.${kind}`).get("border-left-color");
      expect(edge, kind).toBeDefined();
      expect(rule(`.notice.${kind} .icon`).get("color"), kind).toBe(edge);
    }
  });
});

/** The declarations that reach `selector`, from every rule outside an at-rule that lists it, later rules winning. */
const styled = (selector: string) =>
  new Map(
    allRules
      .filter((found) => found.at.length === 0 && found.selector.split(",").some((listed) => listed.trim() === selector))
      .flatMap((found) => [...found.declarations]),
  );

describe("a selected filter row of Entries by State", () => {
  test("is drawn as a selected Agent tile is, with its gradient and its glow", () => {
    const tile = styled(".tile.selected");
    expect(tile.get("background")).toMatch(/gradient/);
    expect(tile.get("box-shadow")).toBeDefined();
    for (const name of ["background", "box-shadow"]) expect(styled(".bar.selected").get(name), name).toBe(tile.get(name));
  });

  test("looks different from a hovered row", () => {
    const hovered = styled(".bar:hover:not(.selected)");
    expect(hovered.get("background")).toBeDefined();
    expect(hovered.get("background")).not.toBe(styled(".bar.selected").get("background"));
    const hovers = allRules.flatMap((found) => found.selector.split(",")).filter((selector) => selector.includes(".bar:hover"));
    for (const selector of hovers) expect(selector, "a hover rule that reaches a selected row too").toContain(":not(.selected)");
  });

  test("keeps its track in sight on the gradient, darker than on an unselected row", () => {
    const track = styled(".bar-track").get("background");
    const selected = styled(".bar.selected .bar-track").get("background");
    expect(track).toMatch(/^#ffffff[0-9a-f]{2}$/i);
    // A black tint, as a white one fades into the gradient.
    expect(selected).toMatch(/^#000000[0-9a-f]{2}$/i);
  });
});
