import { describe, expect, test } from "vitest";
import { CLOSED, PANEL_EDGE, onFieldClick, onKey, onOptionClick, onOptionHover, type DropdownOption, placePanel, type DropdownState } from "./dropdown";

const agents: DropdownOption[] = [
  { value: "", label: "Choose an Agent…", disabled: true },
  { value: "claude", label: "Claude Code" },
  { value: "codex", label: "Codex" },
  { value: "copilot", label: "GitHub Copilot" },
];

const key = (name: string, altKey = false) => ({ key: name, altKey });

describe("the keyboard on a closed dropdown", () => {
  test("Enter, Space and Alt+Down open it on the chosen option", () => {
    for (const opener of [key("Enter"), key(" "), key("ArrowDown", true)]) {
      const reaction = onKey(CLOSED, agents, "codex", opener, 0);
      expect(reaction.state).toMatchObject({ open: true, active: 2 });
      expect(reaction.choose).toBeNull();
      expect(reaction.handled).toBe(true);
    }
  });

  test("the arrows, Home and End choose another option without opening, as in a native select on Windows", () => {
    expect(onKey(CLOSED, agents, "claude", key("ArrowDown"), 0)).toEqual({ state: CLOSED, choose: "codex", handled: true });
    expect(onKey(CLOSED, agents, "codex", key("ArrowUp"), 0).choose).toBe("claude");
    expect(onKey(CLOSED, agents, "claude", key("ArrowUp"), 0)).toEqual({ state: CLOSED, choose: null, handled: true });
    expect(onKey(CLOSED, agents, "claude", key("End"), 0).choose).toBe("copilot");
    expect(onKey(CLOSED, agents, "copilot", key("Home"), 0).choose).toBe("claude");
  });

  test("with nothing chosen yet, Down chooses the first option that can be picked", () => {
    expect(onKey(CLOSED, agents, "", key("ArrowDown"), 0).choose).toBe("claude");
  });
});

describe("the keyboard on an open dropdown", () => {
  const open = (active: number) => ({ ...CLOSED, open: true, active });

  test("the arrows move the active option past disabled ones and stop at the ends", () => {
    expect(onKey(open(1), agents, "claude", key("ArrowDown"), 0).state).toEqual(open(2));
    expect(onKey(open(3), agents, "claude", key("ArrowDown"), 0).state).toEqual(open(3));
    expect(onKey(open(2), agents, "claude", key("ArrowUp"), 0).state).toEqual(open(1));
    // The "Choose an Agent…" prompt above cannot be reached.
    expect(onKey(open(1), agents, "claude", key("ArrowUp"), 0).state).toEqual(open(1));
  });

  test("Home and End go to the first and the last option that can be picked", () => {
    expect(onKey(open(2), agents, "claude", key("Home"), 0).state).toEqual(open(1));
    expect(onKey(open(1), agents, "claude", key("End"), 0).state).toEqual(open(3));
  });

  test("moving chooses nothing yet", () => {
    const reaction = onKey(open(1), agents, "claude", key("ArrowDown"), 0);
    expect(reaction).toMatchObject({ choose: null, handled: true });
  });
});

describe("closing an open dropdown", () => {
  const open = (active: number) => ({ ...CLOSED, open: true, active });

  test("Enter, Space and Alt+Up choose the active option and close", () => {
    for (const chooser of [key("Enter"), key(" "), key("ArrowUp", true), key("ArrowDown", true)]) {
      expect(onKey(open(3), agents, "claude", chooser, 0)).toEqual({ state: CLOSED, choose: "copilot", handled: true });
    }
  });

  test("choosing the option already chosen changes nothing", () => {
    expect(onKey(open(1), agents, "claude", key("Enter"), 0).choose).toBeNull();
  });

  test("Escape closes without choosing", () => {
    expect(onKey(open(3), agents, "claude", key("Escape"), 0)).toEqual({ state: CLOSED, choose: null, handled: true });
  });

  test("Tab chooses the active option and leaves the focus to move on", () => {
    expect(onKey(open(2), agents, "claude", key("Tab"), 0)).toEqual({ state: CLOSED, choose: "codex", handled: false });
  });

  test("Escape and Tab on a closed dropdown are left to the page", () => {
    expect(onKey(CLOSED, agents, "claude", key("Escape"), 0).handled).toBe(false);
    expect(onKey(CLOSED, agents, "claude", key("Tab"), 0).handled).toBe(false);
  });
});

describe("typing on a dropdown", () => {
  const open = (active: number) => ({ ...CLOSED, open: true, active });
  const type = (state: DropdownState, value: string, text: string, startAt = 0) => {
    let reaction = { state, choose: null as string | null, handled: false };
    [...text].forEach((char, at) => {
      reaction = onKey(reaction.state, agents, value, key(char), startAt + at * 100);
      if (reaction.choose !== null) value = reaction.choose;
    });
    return reaction;
  };

  test("jumps to the option that starts with the letters typed", () => {
    expect(type(open(1), "claude", "g").state.active).toBe(3);
    expect(type(open(3), "claude", "CO").state.active).toBe(2);
  });

  test("the same letter again goes on to the next option with it, round to the top", () => {
    const first = type(open(1), "claude", "c");
    expect(first.state.active).toBe(2);
    expect(type(first.state, "claude", "c", 2000).state.active).toBe(1);
  });

  test("a pause starts the search again", () => {
    const first = type(open(1), "claude", "g");
    expect(type(first.state, "claude", "c", 2000).state.active).toBe(1);
  });

  test("a space inside a search is part of it, not a choice", () => {
    const options: DropdownOption[] = [
      { value: "a", label: "Claude Code" },
      { value: "b", label: "Claude Desktop" },
    ];
    let state = open(0);
    for (const [at, char] of [..."claude d"].entries()) {
      const reaction = onKey(state, options, "a", key(char), at * 100);
      expect(reaction.state.open).toBe(true);
      state = reaction.state;
    }
    expect(state.active).toBe(1);
  });

  test("closed, typing chooses the match at once", () => {
    expect(type(CLOSED, "claude", "g")).toMatchObject({ state: { open: false }, choose: "copilot", handled: true });
  });

  test("never lands on a disabled option", () => {
    // "c" goes on to Codex; "ch" matches only the "Choose an Agent…" prompt, so it stays there.
    expect(type(open(1), "claude", "ch").state.active).toBe(2);
  });
});

describe("the mouse on a dropdown", () => {
  const open = (active: number) => ({ ...CLOSED, open: true, active });

  test("a click on the field opens the panel on the chosen option, and a second click closes it", () => {
    const opened = onFieldClick(CLOSED, agents, "codex");
    expect(opened).toEqual(open(2));
    expect(onFieldClick(opened, agents, "codex")).toEqual(CLOSED);
  });

  test("with nothing chosen, the panel opens on the first option that can be picked", () => {
    expect(onFieldClick(CLOSED, agents, "missing").active).toBe(1);
  });

  test("the pointer over an option makes it the active one, but not over a disabled one", () => {
    expect(onOptionHover(open(1), agents, 3)).toEqual(open(3));
    expect(onOptionHover(open(1), agents, 0)).toEqual(open(1));
  });

  test("a click on an option chooses it and closes the panel", () => {
    expect(onOptionClick(open(1), agents, "claude", 3)).toEqual({ state: CLOSED, choose: "copilot", handled: true });
    expect(onOptionClick(open(1), agents, "claude", 1)).toEqual({ state: CLOSED, choose: null, handled: true });
  });

  test("a click on a disabled option does nothing", () => {
    expect(onOptionClick(open(1), agents, "claude", 0)).toEqual({ state: open(1), choose: null, handled: true });
  });
});

describe("where the panel opens", () => {
  const view = { width: 1000, height: 700 };
  const field = { top: 100, left: 200, width: 170, height: 30 };

  test("just below the field, at least as wide as it", () => {
    expect(placePanel(field, { width: 120, height: 200 }, view)).toEqual({ top: 134, left: 200, width: 170, maxHeight: 700 - 8 - 134 });
  });

  test("above the field near the bottom of the window, when there is more room there", () => {
    const low = { ...field, top: 600 };
    expect(placePanel(low, { width: 170, height: 200 }, view)).toEqual({ top: 396, left: 200, width: 170, maxHeight: 588 });
  });

  test("below, scrolling, when the room below is the larger", () => {
    const placed = placePanel({ ...field, top: 300 }, { width: 170, height: 900 }, view);
    expect(placed).toMatchObject({ top: 334, maxHeight: 700 - 8 - 334 });
  });

  test("pulled left near the right edge, and never wider than the window", () => {
    const right = { ...field, left: 900 };
    expect(placePanel(right, { width: 300, height: 100 }, view)).toMatchObject({ left: 1000 - PANEL_EDGE - 300, width: 300 });
    expect(placePanel(field, { width: 2000, height: 100 }, view)).toMatchObject({ left: PANEL_EDGE, width: 1000 - 2 * PANEL_EDGE });
  });
});
