import { readFileSync } from "node:fs";
import { describe, expect, test } from "vitest";
import { LEAVE_MS } from "./Notice";

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

/** The red, green and blue of a `#rrggbb` colour, each 0 to 255. */
const channels = (hex: string) => [1, 3, 5].map((at) => parseInt(hex.slice(at, at + 2), 16));

/** The WCAG 2 contrast ratio of two `#rrggbb` colours. */
const contrast = (one: string, other: string) => {
  const luminance = (hex: string) => {
    const [r, g, b] = channels(hex).map((value) => {
      const channel = value / 255;
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

/**
 * The declarations that reach `selector`, from every rule that lists it outside
 * an at-rule, or inside the one at-rule `at` matches, later rules winning.
 */
const styled = (selector: string, at?: RegExp) =>
  new Map(
    allRules
      .filter((found) => (at === undefined ? found.at.length === 0 : found.at.length === 1 && at.test(found.at[0])))
      .filter((found) => found.selector.split(",").some((listed) => listed.trim() === selector))
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

describe("a change in the What changes block", () => {
  test("puts each of its parts on a line under the last, its label in a column before it, with space between them", () => {
    const row = styled(".change-row");
    expect(row.get("display")).toBe("grid");
    expect(row.get("grid-template-columns")).toBe("max-content minmax(0, 1fr)");
    const [rowGap, columnGap = rowGap] = (row.get("gap") ?? "0").split(/\s+/).map(parseFloat);
    expect(rowGap).toBeGreaterThan(0);
    expect(columnGap).toBeGreaterThan(0);
    // Each label and what it says are cells of the row's grid.
    expect(styled(".change-part").get("display")).toBe("contents");
  });

  test("wraps its key only where a path or key separator lets it, never mid-word", () => {
    const key = styled(".change-key");
    expect(key.get("overflow-wrap")).toBe("normal");
    expect(key.get("word-break")).toBe("normal");
    // Each piece stays whole, as a browser would also break after the hyphen of `cd-throwaway`.
    expect(styled(".change-key-piece").get("white-space")).toBe("nowrap");
  });
});

describe("the action dialog", () => {
  test("stacks its parts in a column no taller than the window, so that a block that scrolls gives up the height the rest needs", () => {
    const dialog = styled(".dialog.action");
    expect(dialog.get("display")).toBe("flex");
    expect(dialog.get("flex-direction")).toBe("column");
    expect(styled(".dialog").get("max-height")).toMatch(/vh$/);
  });

  test("never scrolls as a whole, while the other dialogs still do, so its head and its foot stay in view", () => {
    expect(styled(".dialog.action").get("overflow")).toBe("visible");
    expect(styled(".dialog").get("overflow")).toBe("auto");
    expect(styled(".dialog.action > .dialog-head").get("flex")).toBe("none");
    expect(styled(".dialog.action > .dialog-foot").get("flex")).toBe("none");
  });

  test("gives up the height its head and its foot need from its body, which scrolls as one once the blocks in it are at their minimums", () => {
    const body = styled(".dialog-body");
    expect(shrinks(".dialog-body")).toBe(true);
    expect(body.get("min-height")).toBe("0");
    expect(body.get("overflow-y")).toBe("auto");
    // A column, so that the blocks that scroll in it shrink first.
    expect(body.get("display")).toBe("flex");
    expect(body.get("flex-direction")).toBe("column");
    // A margin taken back above would slide it over the head's last line.
    expect(body.get("margin")?.split(/\s+/)[0]).toBe("0");
  });

  test("scrolls the body of What changes and of Files inside the block, under its title", () => {
    const block = styled(".dialog-section.scrolls");
    expect(block.get("display")).toBe("flex");
    expect(block.get("flex-direction")).toBe("column");
    expect(shrinks(".dialog-section.scrolls")).toBe(true);
    const body = styled(".dialog-section-body");
    expect(body.get("overflow") ?? body.get("overflow-y")).toBe("auto");
    expect(body.get("min-height")).toBe("0");
  });

  test("keeps each block that scrolls a fifth of the window high, or its whole height when it is shorter, so a long one never squeezes the other out", () => {
    // Shrinking takes height from each block in proportion to its own, which
    // left Files a bare title under a long What changes.
    expect(styled(".dialog-section.scrolls").get("min-height")).toMatch(/^calc-size\(max-content, min\(size, 20vh[,)]/);
  });

  test("keeps each block that scrolls no higher than half of what the dialog's height leaves its other parts, so in the smallest window the dialog never scrolls as a whole", () => {
    // A fifth each of a window 640px high, with the other parts, came to more than the dialog's 88vh.
    expect(styled(".dialog").get("max-height")).toBe("88vh");
    expect(styled(".dialog-section.scrolls").get("min-height")).toBe("calc-size(max-content, min(size, 20vh, (88vh - 30rem) / 2))");
  });

  test("lets a WebView2 with no calc-size() shrink each block that scrolls, from a declaration before the one it skips", () => {
    // The parsed rules keep only a property's last declaration, so this reads the text.
    const start = css.indexOf(".dialog-section.scrolls {");
    expect(start).toBeGreaterThan(-1);
    const block = css.slice(start, css.indexOf("}", start));
    expect(block).toMatch(/min-height: 0;[\s\S]*min-height: calc-size\(/);
  });
});

/** The browser's default text size, `medium`, that the root's base is a share of. */
const MEDIUM_PX = 16;

/** A text size in rem, in pixels at 100 % scale, from the root's one base. */
const textPx = (size: string | undefined) => {
  const base = rootValues.get("font-size");
  expect(base, "the root's base, a share of the browser's default").toMatch(/^\d+(\.\d+)?%$/);
  expect(size, "a text size in rem").toMatch(/^\d+(\.\d+)?rem$/);
  return (MEDIUM_PX * parseFloat(base as string)) / 100 * parseFloat(size as string);
};

/** Each text size the stylesheet sets: every `font-size`, and the size a `font` shorthand starts with. */
const textSizes = allRules.flatMap((found) =>
  ["font-size", "font"].flatMap((name) => {
    const value = found.declarations.get(name);
    return value === undefined ? [] : [{ where: `${found.selector} ${name}`, size: name === "font" ? value.split(/[\s/]/)[0] : value }];
  }),
);

describe("the app's text sizes", () => {
  test("are each a rem of the root's one base, so that changing it changes them all, never a px", () => {
    // Every one the stylesheet declares, as a plain search finds them.
    expect(textSizes.length).toBe(css.replace(/\/\*[\s\S]*?\*\//g, "").match(/\bfont(-size)?\s*:/g)?.length);
    for (const { where, size } of textSizes) {
      expect(size, where).not.toMatch(/px$/);
      if (!where.startsWith(":root ") && size !== "inherit") expect(size, where).toMatch(/^\d+(\.\d+)?rem$/);
    }
  });

  test("render body text, a row's location, a description or a dialog's note, at 14px or more at 100 % scale", () => {
    for (const selector of [".item-location", ".bar-label", ".detail-context", ".operation-meta", ".place-sub", ".side-note"]) {
      expect(textPx(styled(selector).get("font-size")), selector).toBeGreaterThanOrEqual(14);
    }
  });
});

const smallestWindow = JSON.parse(readFileSync(new URL("../../tauri.conf.json", import.meta.url), "utf8")).app.windows[0] as { minWidth: number; minHeight: number };

/** A length in pixels, such as `10px`. */
const px = (value: string | undefined) => {
  expect(value, "a length in px").toMatch(/^\d+(\.\d+)?px$/);
  return parseFloat(value as string);
};
/** The top or the left of a `padding` shorthand, in pixels. */
const padding = (selector: string, side: "top" | "left") => {
  const [top, left = top] = (styled(selector).get("padding") ?? "").split(/\s+/);
  return px(side === "top" ? top : left);
};
/** Whether a flex item may shrink below its basis down to its min-width or min-height. */
const shrinks = (selector: string) => {
  const flex = styled(selector).get("flex");
  return flex !== "none" && flex?.split(/\s+/)[1] !== "0";
};

describe("the Inventory at the smallest window the app allows", () => {
  const PANES = [".column", ".items", ".detail"];

  test("fits its Agents, Items and detail panes side by side in the width", () => {
    const shell = 2 * padding(".app", "left") + px(styled(".rail").get("width")) + px(styled(".app").get("gap"));
    const panes = PANES.reduce((sum, pane) => sum + px(styled(pane).get("min-width")), 0);
    const gaps = (PANES.length - 1) * px(styled(".panes").get("gap"));
    expect(shell + panes + gaps).toBeLessThanOrEqual(smallestWindow.minWidth);
    for (const pane of PANES) {
      expect(shrinks(pane), `${pane} shrinks to its min-width`).toBe(true);
      expect(styled(pane).get("width"), `${pane} has no fixed width`).toBeUndefined();
    }
  });

  test("leaves the Items list room for an Item's name beside its icon and State", () => {
    expect(px(styled(".items").get("min-width"))).toBeGreaterThanOrEqual(320);
  });

  test("fits its Agents, Entries by State and Agent home blocks one under the other in the height, under the banner", () => {
    const BLOCKS = [".agents", ".states", ".homes"];
    const banner = px(styled(".ring").get("height")) + 2 * padding(".banner", "top");
    const abovePanes = 2 * padding(".app", "top") + banner + px(styled(".inventory").get("gap"));
    const blocks = BLOCKS.reduce((sum, block) => sum + px(styled(block).get("min-height")), 0);
    const gaps = (BLOCKS.length - 1) * px(styled(".column").get("gap"));
    expect(abovePanes + blocks + gaps).toBeLessThanOrEqual(smallestWindow.minHeight);
    for (const block of BLOCKS) expect(shrinks(block), `${block} shrinks to its min-height`).toBe(true);
  });

  test("keeps the Agents block room for its title and two Agents, and the other two room for two rows", () => {
    expect(px(styled(".agents").get("min-height"))).toBeGreaterThanOrEqual(180);
    for (const block of [".states", ".homes"]) expect(px(styled(block).get("min-height")), block).toBeGreaterThanOrEqual(120);
  });

  test("scrolls the rows of each of those blocks inside the block, under its title", () => {
    // The Agents block scrolls in its `.scroll`; the other two in their own lists.
    for (const body of [".scroll", ".bars", ".home-list"]) {
      const declarations = styled(body);
      expect(declarations.get("overflow") ?? declarations.get("overflow-y"), body).toBe("auto");
      expect(declarations.get("min-height"), body).toBe("0");
      expect(declarations.get("flex"), body).toBe("1");
    }
  });

  test("cuts an Agent home's name too long for its tile before its folder's last part, and only then that part", () => {
    expect(styled(".home-name").get("display")).toBe("flex");
    // The rest keeps room for its «…», so that a cut name always shows it was cut.
    expect(styled(".home-name-head").get("min-width")).toBe("1em");
    expect(styled(".home-name-tail").get("min-width")).toBe("0");
    for (const part of [".home-name-head", ".home-name-tail"]) {
      const declarations = styled(part);
      expect(declarations.get("overflow"), part).toBe("hidden");
      expect(declarations.get("text-overflow"), part).toBe("ellipsis");
    }
    // The flex shrink of each part, weighted by its width: the rest gives up all of its width before the last part gives any.
    const shrink = (part: string) => Number(styled(part).get("flex")?.split(/\s+/)[1]);
    expect(shrink(".home-name-tail")).toBe(1);
    expect(shrink(".home-name-head")).toBeGreaterThanOrEqual(1e6);
  });

  test("moves the name of an Agent's one home under All Agents to a line of its own when it does not fit beside the Agent's", () => {
    expect(styled(".home-row:has(> .single-home)").get("flex-wrap")).toBe("wrap");
    expect(styled(".home-row").get("flex-wrap"), "the select of several homes stays beside the Agent's name").toBeUndefined();
    expect(styled(".home-row").get("white-space")).toBe("nowrap");
    // On its own line the name is cut as on the tiles, when even that line is too short.
    expect(styled(".single-home").get("display")).toBe("flex");
    expect(styled(".single-home").get("min-width")).toBe("0");
  });

  test("moves an Agent home tile's tags and badges to a second line when its name does not fit beside them", () => {
    expect(styled(".home-tile").get("flex-wrap")).toBe("wrap");
    // The name fills its line, so that a second line holds only the tags and badges.
    expect(styled(".home-tile .home-name").get("flex")).toBe("1 1 auto");
  });

  test("leaves an Agent home's tile, beside the list's scrollbar, room for a last part such as claude-code-synced-home", () => {
    // 270 px leaves a tile 232 px wide, and its name 193 px after the rest's «…».
    expect(px(styled(".column").get("min-width"))).toBeGreaterThanOrEqual(270);
  });
});

describe("text in the Inventory at the smallest window", () => {
  test("breaks a long Item name, the detail's title or its context line instead of running over its neighbours", () => {
    for (const selector of [".item-name", ".detail .panel-title", ".detail-context"]) {
      expect(styled(selector).get("overflow-wrap"), selector).toBe("anywhere");
    }
  });

  test("cuts a long Item location to one line, as its tooltip holds the full path", () => {
    const location = styled(".item-location");
    expect(location.get("text-overflow")).toBe("ellipsis");
    expect(location.get("white-space")).toBe("nowrap");
  });

  test("puts an Item's tags, Entry count and State under its name and location in a narrow Items list", () => {
    expect(styled(".items").get("container")).toBe("items / inline-size");
    const narrow = /^@container items \(max-width: \d+px\)$/;
    expect(styled(".item-row", narrow).get("flex-wrap")).toBe("wrap");
    // The name and location take the rest of the first line, after the icon and the row's gap.
    const indent = px(styled(".item-icon").get("width")) + px(styled(".item-row").get("gap"));
    expect(styled(".item-text", narrow).get("flex-basis")).toBe(`calc(100% - ${indent}px)`);
    // The rest is one box on the next line, under the name, wrapping inside itself; the State ends it on the right.
    expect(styled(".item-side").get("display"), "beside the name in a wide list").toBe("contents");
    const side = styled(".item-side", narrow);
    expect(side.get("display")).toBe("flex");
    expect(side.get("flex-wrap")).toBe("wrap");
    expect(side.get("flex")).toBe(`1 0 calc(100% - ${indent}px)`);
    expect(side.get("margin-left")).toBe(`${indent}px`);
    expect(styled(".item-side .state-pill", narrow).get("margin-left")).toBe("auto");
  });

  test("cuts a tag in a row of tags at the row's width, such as one naming where an Entry was parked from", () => {
    const tag = styled(".tags .tag");
    expect(tag.get("max-width")).toBe("100%");
    expect(tag.get("overflow")).toBe("hidden");
    expect(tag.get("text-overflow")).toBe("ellipsis");
  });
});

describe("the Projects & homes banner at the smallest window", () => {
  test("keeps the screen's title, each tab of its switch and its Add button on one line", () => {
    for (const selector of [".banner-title", ".places-tabs button", ".banner .cta"]) {
      expect(styled(selector).get("white-space"), selector).toBe("nowrap");
    }
  });

  test("puts a row's Entry count and buttons under its name and path in a narrow list, so the name keeps the row's width", () => {
    expect(styled(".places-list").get("container")).toBe("places / inline-size");
    const narrow = /^@container places \(max-width: \d+px\)$/;
    expect(styled(".place-row", narrow).get("flex-wrap")).toBe("wrap");
    // The name and path take the rest of the first line, after the icon and the row's gap.
    const indent = px(styled(".place-icon").get("width")) + px(styled(".place-row").get("gap"));
    expect(styled(".place-text", narrow).get("flex-basis")).toBe(`calc(100% - ${indent}px)`);
    // The count starts the next line under the name; the buttons end it on the right.
    const count = styled(".place-count", narrow);
    expect(count.get("width")).toBe("auto");
    expect(count.get("margin-left")).toBe(`${indent}px`);
    expect(styled(".place-actions", narrow).get("margin-left")).toBe("auto");
  });

  test("keeps each folder's name of a path whole on its line, and breaks one only when it is wider than the column", () => {
    const piece = styled(".path-piece");
    // A box of its own, so that the line wraps around it, never at the hyphen of `cd-throwaway`.
    expect(piece.get("display")).toBe("inline-block");
    expect(piece.get("max-width")).toBe("100%");
    // A name wider than the column breaks inside it rather than pushing the page sideways.
    expect(piece.get("overflow-wrap")).toBe("anywhere");
  });
});

/** Expects every colour of the rules of `selectors` to be a `:root` token. */
const expectTokenColours = (selectors: string[]) => {
  for (const selector of selectors) {
    const declarations = styled(selector);
    expect(declarations.size, selector).toBeGreaterThan(0);
    for (const value of declarations.values()) {
      expect(rawColours(value), `${selector}: ${value}`).toEqual([]);
      for (const token of tokensUsed(value)) expect(tokens, `${selector}: ${value}`).toContain(token);
    }
  }
};

describe("the busy indicator", () => {
  test("takes every colour from the :root tokens", () => {
    expectTokenColours([".notice.busy", ".spinner"]);
  });

  test("is a notice of its own, with the background and accent edge of an information one", () => {
    for (const name of ["background", "border-left-color"]) {
      expect(styled(".notice.busy").get(name), name).toBe(styled(".notice.information").get(name));
    }
  });

  test("spins its spinner in the colour of its accent edge, more slowly when the user asks for less motion", () => {
    const spinner = styled(".spinner");
    expect(spinner.get("animation")).toMatch(/\bspin\b.*\binfinite\b/);
    expect(spinner.get("border-top-color")).toBe(styled(".notice.busy").get("border-left-color"));
    expect(css).toMatch(/@keyframes spin\s*\{\s*to\s*\{\s*transform:\s*rotate\(1turn\)/);
    expect(styled(".spinner", /prefers-reduced-motion: reduce/).get("animation-duration")).toBeDefined();
  });
});

describe("the loading state of a first read", () => {
  test("fills the body it stands in for", () => {
    expect(styled(".loading").get("flex")).toBe("1");
  });

  test("shows its busy indicator in the body, above its placeholder rows, instead of floating at the bottom", () => {
    const busy = styled(".loading .notice.busy");
    expect(busy.get("position")).toBe("static");
    expect(busy.get("transform")).toBe("none");
  });

  test("takes every colour of its placeholder rows from the :root tokens", () => {
    expectTokenColours([".placeholder-icon", ".placeholder-line"]);
  });

  test("pulses its placeholder rows, but not when the user asks for less motion", () => {
    expect(styled(".placeholder-row").get("animation")).toMatch(/\bpulse\b.*\binfinite\b/);
    expect(styled(".placeholder-row", /prefers-reduced-motion: reduce/).get("animation")).toBe("none");
  });
});

describe("the cue of a re-read", () => {
  test("runs along the bottom edge of the banner, taking no room from the screen", () => {
    expect(styled(".banner").get("position")).toBe("relative");
    const bar = styled(".read-progress");
    expect(bar.get("position")).toBe("absolute");
    expect(bar.get("bottom")).toBe("0");
  });

  test("takes every colour from the :root tokens", () => {
    expectTokenColours([".read-progress", ".read-progress span"]);
  });

  test("slides its bar along the track, more slowly when the user asks for less motion", () => {
    expect(styled(".read-progress span").get("animation")).toMatch(/\bread-progress\b.*\binfinite\b/);
    expect(css).toMatch(/@keyframes read-progress\s*\{/);
    expect(styled(".read-progress span", /prefers-reduced-motion: reduce/).get("animation-duration")).toBeDefined();
  });
});

const SURFACES = ["--background", "--rail", "--panel", "--tile", "--tile-hover", "--button"];

describe("the Undo snackbar", () => {
  test("has the corner radius of the notices, cards and dialogs, not a pill's", () => {
    const radius = styled(".snackbar").get("border-radius");
    expect(radius).toBe(styled(".notice").get("border-radius"));
    expect(radius).toBe(styled(".entry-card").get("border-radius"));
    expect(px(radius)).toBeLessThanOrEqual(px(styled(".dialog").get("border-radius")));
  });

  test("takes every colour from the :root tokens", () => {
    expectTokenColours([".snackbar", ".snackbar-action", ".snackbar-action:hover"]);
  });

  test("stands out from every surface of the app as a notice does, with an accent edge", () => {
    const snackbar = styled(".snackbar");
    const background = colour(snackbar.get("background"));
    const edge = colour(snackbar.get("border-left-color"));
    expect(snackbar.get("border-left-width")).toBe(styled(".notice").get("border-left-width"));
    for (const surface of SURFACES.map((token) => colour(`var(${token})`))) {
      expect(contrast(background, surface), surface).toBeGreaterThanOrEqual(1.6);
      expect(contrast(edge, surface), `edge on ${surface}`).toBeGreaterThanOrEqual(3);
    }
    expect(contrast(edge, background), "edge on the snackbar").toBeGreaterThanOrEqual(3);
  });

  test("has text and an Undo action that meet WCAG AA on its background", () => {
    const background = colour(styled(".snackbar").get("background"));
    expect(contrast(colour(styled(".snackbar").get("color")), background)).toBeGreaterThanOrEqual(4.5);
    expect(contrast(colour(styled(".snackbar-action").get("color")), background)).toBeGreaterThanOrEqual(4.5);
    // Hovered, the action is filled with what was its text colour.
    const hovered = styled(".snackbar-action:hover");
    expect(contrast(colour(hovered.get("color")), colour(hovered.get("background")))).toBeGreaterThanOrEqual(4.5);
  });

  test("draws its Undo action as a button, with an outline the eye can find", () => {
    const action = styled(".snackbar-action");
    const [width, style, outline] = (action.get("border") ?? "").split(/\s+/);
    expect(px(width)).toBeGreaterThanOrEqual(1);
    expect(style).toBe("solid");
    expect(contrast(colour(outline), colour(styled(".snackbar").get("background")))).toBeGreaterThanOrEqual(3);
    expect(padding(".snackbar-action", "left")).toBeGreaterThan(0);
    expect(px(action.get("border-radius"))).toBeGreaterThan(0);
    expect(action.get("cursor")).toBe("pointer");
  });

  test("has a Dismiss button and a focus ring that are seen on its background", () => {
    const background = colour(styled(".snackbar").get("background"));
    const dismiss = styled(".snackbar .icon-button");
    expect(dismiss.get("background")).toBe("transparent");
    expect(contrast(colour(dismiss.get("color")), background)).toBeGreaterThanOrEqual(4.5);
    const hovered = styled(".snackbar .icon-button:hover");
    expect(contrast(colour(hovered.get("color")), colour(hovered.get("background")))).toBeGreaterThanOrEqual(4.5);
    // A focus ring is a graphic that has to be seen: WCAG asks 3:1 of it.
    expect(contrast(colour(styled(".snackbar :focus-visible").get("outline-color")), background)).toBeGreaterThanOrEqual(3);
  });
});

/** The keyframes name and the duration in ms of an `animation` shorthand, such as `float-in 160ms ease-out`. */
const animation = (value: string | undefined) => {
  const [name, duration] = (value ?? "").split(/\s+/);
  expect(duration, `${value} gives a duration in ms`).toMatch(/^\d+ms$/);
  return { name, ms: parseInt(duration, 10) };
};
/** The body of `@keyframes name`. */
const keyframes = (name: string) => {
  const body = css.match(new RegExp(String.raw`@keyframes ${name}\s*\{([\s\S]*?\})\s*\}`))?.[1];
  expect(body, `@keyframes ${name}`).toBeDefined();
  return body as string;
};
const FLOATING = [".notice", ".snackbar"];

describe("a notice and the Undo snackbar coming and going", () => {
  test("fade and slide in, in under 200 ms", () => {
    for (const selector of FLOATING) {
      const { name, ms } = animation(styled(selector).get("animation"));
      expect(ms, selector).toBeLessThan(200);
      expect(keyframes(name), selector).toMatch(/from\s*\{[^}]*opacity:\s*0[^}]*translate:/);
    }
  });

  test("fade and slide out while leaving, in the time the app gives them to leave", () => {
    expect(LEAVE_MS).toBeLessThan(200);
    for (const selector of FLOATING) {
      const value = styled(`${selector}.leaving`).get("animation");
      const { name, ms } = animation(value);
      expect(ms, selector).toBe(LEAVE_MS);
      expect(value, `${selector} stays gone at the end`).toMatch(/\bforwards\b/);
      expect(keyframes(name), selector).toMatch(/to\s*\{[^}]*opacity:\s*0[^}]*translate:/);
    }
  });

  test("let the pointer through while leaving, so that nothing on them is pressed or held as they go", () => {
    for (const selector of FLOATING) expect(styled(`${selector}.leaving`).get("pointer-events"), selector).toBe("none");
  });

  test("slide by translate, keeping the transform that centres them", () => {
    for (const selector of FLOATING) {
      expect(styled(selector).get("transform"), selector).toBe("translateX(-50%)");
      for (const leaving of ["", ".leaving"]) {
        expect(keyframes(animation(styled(`${selector}${leaving}`).get("animation")).name)).not.toMatch(/transform/);
      }
    }
  });

  test("only fade when the user asks for less motion", () => {
    const reduced = /prefers-reduced-motion: reduce/;
    for (const selector of FLOATING.flatMap((floating) => [floating, `${floating}.leaving`])) {
      const name = styled(selector, reduced).get("animation-name");
      expect(name, selector).toBeDefined();
      expect(keyframes(name as string), selector).toMatch(/opacity/);
      expect(keyframes(name as string), selector).not.toMatch(/translate|transform/);
    }
  });
});

describe("a button that deletes", () => {
  test("is red, from :root tokens of its own, and redder when hovered", () => {
    for (const [selector, token] of [
      [".cta.danger", "--danger"],
      [".cta.danger:hover", "--danger-hover"],
    ]) {
      expect(styled(selector).get("background"), selector).toBe(`var(${token})`);
      const [r, g, b] = channels(colour(`var(${token})`));
      expect(r, `${token} is red`).toBeGreaterThan(2 * Math.max(g, b));
    }
    expectTokenColours([".cta.danger", ".cta.danger:hover"]);
  });

  test("has white text that meets WCAG AA on it, hovered or not", () => {
    const text = colour(styled(".cta").get("color"));
    for (const selector of [".cta.danger", ".cta.danger:hover"]) {
      expect(contrast(text, colour(styled(selector).get("background"))), selector).toBeGreaterThanOrEqual(4.5);
    }
  });

  test("looks like any other button when disabled, as it deletes nothing then", () => {
    for (const name of ["background", "box-shadow"]) {
      expect(styled(".cta.danger:disabled").get(name), name).toBe(styled(".cta:disabled").get(name));
    }
  });
});

/** Each kind of Before you apply item, with the notice kind whose tokens it takes. */
const CHECKS = [
  [".check.block", "error"],
  [".check.warning", "warning"],
  [".check.info", "information"],
] as const;

describe("a Before you apply item", () => {
  test("of each kind is a card tinted by its kind, with an accent edge, from the notices' :root tokens", () => {
    expect(styled(".check").get("border-left-width")).toBe(styled(".notice").get("border-left-width"));
    expect(styled(".check").get("border-left-style")).toBe("solid");
    for (const [selector, kind] of CHECKS) {
      const declarations = styled(selector);
      expect(declarations.get("background"), selector).toBe(`var(--notice-${kind})`);
      expect(declarations.get("border-left-color"), selector).toBe(`var(--notice-${kind}-edge)`);
    }
  });

  test("that informs takes the style of an information notice, not grey text", () => {
    for (const name of ["background", "border-left-color"]) {
      expect(styled(".check.info").get(name), name).toBe(styled(".notice.information").get(name));
    }
    expect(styled(".check.info").get("color")).toBeUndefined();
  });

  test("that warns is in the warning colour, an amber apart from the other kinds", () => {
    const edge = colour(styled(".check.warning").get("border-left-color"));
    const [r, g, b] = channels(edge);
    expect(r, "red").toBeGreaterThan(2 * b);
    expect(g, "green").toBeGreaterThan(1.5 * b);
    for (const [selector] of CHECKS.filter(([selector]) => selector !== ".check.warning")) {
      expect(colour(styled(selector).get("border-left-color")), selector).not.toBe(edge);
    }
  });

  test("of each kind stands out from the dialog, its text meeting WCAG AA and its icon in the colour of its accent edge", () => {
    const text = colour(styled(".check").get("color"));
    for (const [selector] of CHECKS) {
      const background = colour(styled(selector).get("background"));
      const edge = styled(selector).get("border-left-color");
      expect(contrast(background, colour("var(--panel)")), `${selector} on the dialog`).toBeGreaterThanOrEqual(1.6);
      expect(contrast(text, background), selector).toBeGreaterThanOrEqual(4.5);
      expect(contrast(colour(edge), background), `${selector} edge`).toBeGreaterThanOrEqual(3);
      expect(styled(`${selector} > .icon`).get("color"), selector).toBe(edge);
    }
  });

  test("puts what to do first, in bold, and each path on a line of its own in monospace", () => {
    expect(Number(styled(".check-todo").get("font-weight"))).toBeGreaterThanOrEqual(600);
    const path = styled(".check-path");
    expect(path.get("display")).toBe("block");
    expect(path.get("font-family")).toBe("var(--mono)");
    // A long path wraps inside the item instead of running over its edge.
    expect(path.get("overflow-wrap")).toBe("anywhere");
  });

  test("takes every colour from the :root tokens", () => {
    expectTokenColours([".check", ...CHECKS.map(([selector]) => selector), ".check.git"]);
  });
});

/**
 * The hints of the dialogs: the line under a title, the backup note, why
 * Apply is off, a result's hints and notes, a step yet to run, and what
 * stands in for a list while it loads or when it is empty.
 */
const HINTS = [
  ".dialog-subtitle",
  ".dialog-backup",
  ".apply-why",
  ".effective-change",
  ".file-note",
  ".callout-hint",
  ".step",
  ".dialog .dim",
  ".dialog .empty",
  ".dialog .side-note",
];

describe("a hint in a dialog", () => {
  test("meets WCAG AA on the dialog and on its tiles", () => {
    for (const selector of HINTS) {
      const text = colour(styled(selector).get("color"));
      for (const surface of ["--panel", "--tile"]) {
        expect(contrast(text, colour(`var(${surface})`)), `${selector} on ${surface}`).toBeGreaterThanOrEqual(4.5);
      }
    }
  });

  test("is at a readable size, no smaller than 14.5px at 100 % scale", () => {
    for (const selector of HINTS) {
      // A hint without a size of its own takes the app's.
      const size = styled(selector).get("font-size") ?? styled("body").get("font-size");
      expect(textPx(size), selector).toBeGreaterThanOrEqual(14.5);
    }
  });
});

/** Each tone of a tag, by what it tells: plain, git-versioned, a Linked Entry, read-only. */
const TAGS = [".tag", ".tag.git", ".tag.linked", ".tag.read-only"];

/** The declarations of a tag of the tone `selector`, over those of every tag. */
const tone = (selector: string) => new Map([...styled(".tag"), ...styled(selector)]);

/** The colour of the outline that `declarations` give a tag, set on its own or in its `border`. */
const edgeOf = (declarations: Map<string, string>) => declarations.get("border-color") ?? declarations.get("border")?.split(/\s+/).pop();

/** The colour of the outline of a tag of the tone `selector`. */
const outline = (selector: string) => edgeOf(tone(selector));

describe("a tag", () => {
  test("of each tone has text that meets WCAG AA on its own background", () => {
    for (const selector of TAGS) {
      const declarations = tone(selector);
      expect(contrast(colour(declarations.get("color")), colour(declarations.get("background"))), selector).toBeGreaterThanOrEqual(4.5);
    }
  });

  test("of each tone has an outline seen against every surface of the app, as WCAG asks 3:1 of it", () => {
    for (const selector of TAGS) {
      const edge = colour(outline(selector));
      for (const surface of SURFACES) expect(contrast(edge, colour(`var(${surface})`)), `${selector} on ${surface}`).toBeGreaterThanOrEqual(3);
    }
  });

  test("differs by tone, in its text, its background and its outline, so each tells what it means at a glance", () => {
    expect(new Set(TAGS.map((selector) => colour(tone(selector).get("color")))).size).toBe(TAGS.length);
    expect(new Set(TAGS.map((selector) => colour(tone(selector).get("background")))).size).toBe(TAGS.length);
    expect(new Set(TAGS.map((selector) => colour(outline(selector)))).size).toBe(TAGS.length);
  });

  test("that says git-versioned is outlined in the colour of the git-versioned ring of a dialog", () => {
    expect(styled(".tag.git").get("border-color")).toBe("var(--git-versioned)");
    expect(styled(".check.git").get("box-shadow")).toContain("var(--git-versioned)");
  });

  test("takes every colour from the :root tokens", () => {
    expectTokenColours(TAGS);
  });
});

/** The declarations of a row of Projects & homes of the class `selector`, over those of every row. */
const placeRow = (selector: string) => new Map([...styled(".place-row"), ...styled(selector)]);

/** The width, style and colour of the border of a row of the class `selector`, from its `border` or its longhands. */
const placeBorder = (selector: string) => {
  const declarations = placeRow(selector);
  const [width, style, edge] = declarations.get("border")?.split(/\s+/) ?? [];
  return {
    width: px(declarations.get("border-width") ?? width),
    style: declarations.get("border-style") ?? style,
    edge: declarations.get("border-color") ?? edge,
  };
};

describe("a hidden row of Projects & homes", () => {
  test("is never faded, as no text or outline on it could undo an opacity or a filter on the row", () => {
    const dimming = allRules.filter((found) => found.selector.split(",").some((listed) => listed.includes(".place-row.dimmed")));
    expect(dimming.length).toBeGreaterThan(0);
    for (const found of dimming) {
      expect(found.declarations.get("opacity"), found.selector).toBeUndefined();
      expect(found.declarations.get("filter"), found.selector).toBeUndefined();
    }
  });

  /** What a hidden row is drawn on: its own background, or the panel's where it has none. */
  const hiddenRow = () => {
    const background = placeRow(".place-row.dimmed").get("background");
    return colour(background === "transparent" ? styled(".panel").get("background") : background);
  };

  test("still reads as hidden next to a shown row, drawn apart from a tile", () => {
    expect(styled(".place-row.dimmed").get("background")).not.toBe(styled(".place-row").get("background"));
    // Its outline is as wide as a shown row's transparent one, so Hide and Unhide never move the rows.
    expect(placeBorder(".place-row.dimmed").width).toBe(placeBorder(".place-row").width);
  });

  test("has a name, lines, a count and tags that meet WCAG AA, and tag outlines at 3:1 against it", () => {
    const row = hiddenRow();
    for (const [what, text] of [
      ["the name", styled(".place-row.dimmed .place-name").get("color")],
      [".place-sub", styled(".place-sub").get("color")],
      [".place-count", styled(".place-count").get("color")],
    ]) {
      expect(contrast(colour(text), row), what).toBeGreaterThanOrEqual(4.5);
    }
    for (const selector of TAGS) {
      const declarations = tone(selector);
      expect(contrast(colour(declarations.get("color")), colour(declarations.get("background"))), selector).toBeGreaterThanOrEqual(4.5);
      expect(contrast(colour(outline(selector)), row), `${selector} outline`).toBeGreaterThanOrEqual(3);
    }
  });

  test("has small buttons, Unhide and Remove from list, whose labels meet WCAG AA on their fill, hovered or not, as the row no longer fades them", () => {
    // A disabled button, while an action runs on the row, is exempt from AA.
    const label = colour(styled(".cta").get("color"));
    for (const selector of [".cta.secondary", ".cta.secondary:hover", ".cta.danger", ".cta.danger:hover"]) {
      expect(contrast(label, colour(styled(selector).get("background"))), selector).toBeGreaterThanOrEqual(4.5);
    }
  });
});

describe("the No Project row of the Projects tab", () => {
  test("reads apart from a hidden row in more than colour, as it has neither its dashed outline nor a background of its own", () => {
    const sameStyle = placeBorder(".place-row.informational").style === placeBorder(".place-row.dimmed").style;
    const sameBackground = placeRow(".place-row.informational").get("background") === placeRow(".place-row.dimmed").get("background");
    expect(sameStyle && sameBackground).toBe(false);
    expect(placeBorder(".place-row.informational").edge).toBe("transparent");
    expect(placeRow(".place-row.informational").get("background")).toBe("transparent");
  });

  test("keeps a shown row's transparent border, so it is as big as every other row", () => {
    expect(placeBorder(".place-row.informational").width).toBe(placeBorder(".place-row").width);
  });

  test("has a name and a line that meet WCAG AA against the panel it is drawn on", () => {
    const panel = colour(styled(".panel").get("background"));
    const name = styled(".place-row.informational .place-name").get("color") ?? styled(".place-name").get("color") ?? styled(":root").get("color");
    for (const [what, text] of [
      ["the name", name],
      [".place-sub", styled(".place-row.informational .place-sub").get("color") ?? styled(".place-sub").get("color")],
    ]) {
      expect(contrast(colour(text), panel), what).toBeGreaterThanOrEqual(4.5);
    }
  });
});

describe("a tag on a selected home tile", () => {
  /** The declarations of a tag of the tone `selector` on a selected tile, over those of the same tag elsewhere. */
  const onSelected = (selector: string) => new Map([...tone(selector), ...styled(".tile.selected .tag"), ...styled(`.tile.selected ${selector}`)]);

  test("has an outline at 3:1 against both ends of the tile's gradient, and text that meets WCAG AA on its fill, in the plain and read-only tones a home shows", () => {
    const gradient = styled(".tile.selected").get("background") ?? "";
    const ends = tokensUsed(gradient);
    // Every colour of the gradient is a token, so none is left unchecked.
    expect(rawColours(gradient), gradient).toEqual([]);
    expect(ends).toHaveLength(2);
    for (const selector of [".tag", ".tag.read-only"]) {
      const declarations = onSelected(selector);
      const edge = colour(edgeOf(declarations));
      for (const end of ends) expect(contrast(edge, colour(`var(${end})`)), `${selector} on ${end}`).toBeGreaterThanOrEqual(3);
      expect(contrast(colour(declarations.get("color")), colour(declarations.get("background"))), selector).toBeGreaterThanOrEqual(4.5);
    }
  });

  test("keeps its usual outline outside a selected tile", () => {
    for (const selector of [".tag", ".tag.read-only"]) expect(outline(selector), selector).not.toBe(onSelected(selector).get("border-color"));
  });

  test("takes every colour from the :root tokens", () => {
    expectTokenColours([".tile.selected .tag", ".tile.selected .tag.read-only"]);
  });
});

describe("an element the keyboard reaches for its tooltip", () => {
  // The Mirrored and not read tags, a single home's name, a flag alert, the
  // time the inventory was read, the parked from tag, and the action dialog's
  // info lines and file names.
  const TAB_STOPS = [".tag", ".single-home", ".alert", ".banner-value", ".check.info", ".change-file-name .mono"];

  test("shows the focus ring of the other controls under :focus-visible", () => {
    const ring = styled(".tile:focus-visible");
    expect(ring.get("outline")).toBe("2px solid var(--primary-hover)");
    for (const selector of TAB_STOPS) {
      const focused = styled(`${selector}:focus-visible`);
      expect(focused.get("outline"), selector).toBe(ring.get("outline"));
      expect(focused.get("outline-offset"), selector).toBe(ring.get("outline-offset"));
    }
  });
});
