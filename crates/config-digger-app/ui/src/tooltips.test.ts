import { describe, expect, test } from "vitest";
import { HIDDEN, TOOLTIP_EDGE, describes, onTooltip, placeTooltip, type TooltipEvent, type TooltipState } from "./tooltips";

/** The state after `events`, from a hidden tooltip. */
const after = (...events: TooltipEvent[]): TooltipState => events.reduce((state, event) => onTooltip(state, event).state, HIDDEN);

describe("hovering a control", () => {
  test("shows its tooltip only once the delay is over", () => {
    expect(after("enter").shown).toBe(false);
    expect(after("enter", "delay").shown).toBe(true);
  });

  test("leaving hides it, and leaving before the delay keeps it hidden", () => {
    expect(after("enter", "delay", "leave").shown).toBe(false);
    expect(after("enter", "leave", "delay").shown).toBe(false);
  });
});

describe("focusing a control", () => {
  test("from the keyboard shows its tooltip at once, and blur hides it", () => {
    expect(after("keyboard focus").shown).toBe(true);
    expect(after("keyboard focus", "blur").shown).toBe(false);
  });

  test("with a click shows nothing, as the pointer has its own delay", () => {
    expect(after("pointer focus").shown).toBe(false);
  });

  test("the tooltip stays while the control keeps the focus or the pointer", () => {
    expect(after("keyboard focus", "enter", "leave").shown).toBe(true);
    expect(after("enter", "delay", "keyboard focus", "blur").shown).toBe(true);
  });
});

describe("pressing the control", () => {
  test("a click or a key hides its tooltip, as a native one hides, and cancels the hover delay", () => {
    expect(after("enter", "delay", "press").shown).toBe(false);
    expect(after("keyboard focus", "press").shown).toBe(false);
    expect(after("enter", "press", "delay").shown).toBe(false);
  });
});

describe("Escape", () => {
  test("hides a tooltip shown by hover or by focus, and is the tooltip's", () => {
    for (const shown of [after("enter", "delay"), after("keyboard focus")]) {
      const reaction = onTooltip(shown, "escape");
      expect(reaction.state.shown).toBe(false);
      expect(reaction.handled).toBe(true);
    }
  });

  test("keeps it hidden while the pointer stays, until it leaves and comes back", () => {
    expect(after("enter", "delay", "escape", "delay").shown).toBe(false);
    expect(after("enter", "escape", "delay").shown).toBe(false);
    expect(after("enter", "delay", "escape", "leave", "enter", "delay").shown).toBe(true);
  });

  test("with no tooltip showing is not the tooltip's, so a dialog around it still closes", () => {
    expect(onTooltip(HIDDEN, "escape").handled).toBe(false);
    expect(onTooltip(after("enter"), "escape").handled).toBe(false);
  });
});

describe("placing the tooltip", () => {
  const view = { width: 800, height: 600 };
  const size = { width: 120, height: 30 };

  test("goes below the control, centred on it", () => {
    const control = { top: 100, left: 300, width: 40, height: 20 };
    expect(placeTooltip(control, size, view)).toEqual({ top: 126, left: 260 });
  });

  test("goes above a control near the bottom of the window", () => {
    const control = { top: 560, left: 300, width: 40, height: 20 };
    expect(placeTooltip(control, size, view)).toEqual({ top: 524, left: 260 });
  });

  test("is pulled in from the window's left and right edges", () => {
    expect(placeTooltip({ top: 100, left: 0, width: 20, height: 20 }, size, view).left).toBe(TOOLTIP_EDGE);
    expect(placeTooltip({ top: 100, left: 790, width: 10, height: 20 }, size, view).left).toBe(800 - TOOLTIP_EDGE - 120);
  });

  test("stays inside a window too short for it either side of the control", () => {
    const tall = { width: 120, height: 280 };
    const placed = placeTooltip({ top: 290, left: 300, width: 40, height: 20 }, tall, view);
    expect(placed.top).toBeGreaterThanOrEqual(TOOLTIP_EDGE);
    expect(placed.top + tall.height).toBeLessThanOrEqual(600 - TOOLTIP_EDGE);
  });
});

describe("what a screen reader gets", () => {
  test("the tooltip describes a control whose name does not already say it", () => {
    expect(describes("C:\Users\me\.claude", undefined)).toBe(true);
    expect(describes("Copy the configuration, with every Secret masked", "Copy the configuration")).toBe(true);
  });

  test("a control named with the tooltip's text needs no description too", () => {
    expect(describes("Copy the command", "Copy the command")).toBe(false);
    expect(describes("Nothing to undo", "Can't undo: Nothing to undo")).toBe(false);
  });
});
