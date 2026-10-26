// @vitest-environment happy-dom
import { act, cleanup, render, screen, within } from "@testing-library/react";
import userEvent, { type UserEvent } from "@testing-library/user-event";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { DropdownOption } from "./dropdown";
import { Select } from "./Select";

afterEach(cleanup);

const PROJECTS: DropdownOption[] = [
  { value: "alpha", label: "Alpha" },
  { value: "beta", label: "Beta" },
  { value: "gamma", label: "Gamma" },
];

/** A Select whose `onChange` feeds its `value`, as the Inventory's Project field does. */
function ProjectField({
  options,
  initial,
  onChange,
  searchLabel,
}: {
  options: DropdownOption[];
  initial: string;
  onChange: (value: string) => void;
  searchLabel?: string;
}) {
  const [value, setValue] = useState(initial);
  return (
    <Select
      value={value}
      options={options}
      label="Project"
      searchLabel={searchLabel}
      onChange={(next) => {
        onChange(next);
        setValue(next);
      }}
    />
  );
}

/** Renders a Project field on `initial`, with the values it chooses in `chosen`. */
function renderField({ options = PROJECTS, initial = "alpha", searchLabel }: { options?: DropdownOption[]; initial?: string; searchLabel?: string } = {}) {
  const user = userEvent.setup();
  const chosen: string[] = [];
  const { container } = render(<ProjectField options={options} initial={initial} searchLabel={searchLabel} onChange={(value) => chosen.push(value)} />);
  return { user, chosen, container, field: screen.getByRole("combobox", { name: "Project" }) };
}

const optionId = (name: string) => screen.getByRole("option", { name }).id;

describe("a Select in a DOM", () => {
  test("says whether its panel is open through aria-expanded", async () => {
    const { user, field } = renderField();
    expect(field.getAttribute("aria-expanded")).toBe("false");
    await user.click(field);
    expect(field.getAttribute("aria-expanded")).toBe("true");
  });

  test("names its panel through aria-controls only while it is open", async () => {
    const { user, field } = renderField();
    expect(field.hasAttribute("aria-controls")).toBe(false);
    await user.click(field);
    expect(field.getAttribute("aria-controls")).toBe(screen.getByRole("listbox").id);
    await user.click(field);
    expect(field.hasAttribute("aria-controls")).toBe(false);
  });

  test("points aria-activedescendant at the option the keyboard is on", async () => {
    const { user, field } = renderField({ initial: "beta" });
    expect(field.hasAttribute("aria-activedescendant")).toBe(false);
    await user.tab();
    await user.keyboard("{Enter}");
    expect(field.getAttribute("aria-activedescendant")).toBe(optionId("Beta"));
    await user.keyboard("{ArrowDown}");
    expect(field.getAttribute("aria-activedescendant")).toBe(optionId("Gamma"));
    await user.keyboard("{Home}");
    expect(field.getAttribute("aria-activedescendant")).toBe(optionId("Alpha"));
  });

  test("portals its panel to document.body", async () => {
    const { user, field, container } = renderField();
    await user.click(field);
    const panel = screen.getByRole("listbox", { name: "Project" }).closest(".select-panel");
    expect(panel?.parentElement).toBe(document.body);
    expect(container.contains(panel)).toBe(false);
  });

  test("keeps the focus on the field while it opens, chooses and closes on Escape", async () => {
    const { user, field, chosen } = renderField();
    await user.tab();
    expect(document.activeElement).toBe(field);

    await user.keyboard("{Enter}");
    expect(field.getAttribute("aria-expanded")).toBe("true");
    expect(document.activeElement).toBe(field);

    await user.keyboard("{ArrowDown}{Enter}");
    expect(chosen).toEqual(["beta"]);
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(field.textContent).toBe("Beta");
    expect(document.activeElement).toBe(field);

    await user.keyboard("{Enter}");
    expect(field.getAttribute("aria-expanded")).toBe("true");
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(chosen).toEqual(["beta"]);
    expect(document.activeElement).toBe(field);
  });

  test("closes its panel when the field loses the focus", async () => {
    const user = userEvent.setup();
    render(
      <>
        <ProjectField options={PROJECTS} initial="alpha" onChange={() => {}} />
        <button type="button">Elsewhere</button>
      </>,
    );
    const field = screen.getByRole("combobox", { name: "Project" });
    await user.click(field);
    expect(field.getAttribute("aria-expanded")).toBe("true");
    await user.click(screen.getByRole("button", { name: "Elsewhere" }));
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(field.getAttribute("aria-expanded")).toBe("false");
  });

  test("chooses an option clicked in its panel and keeps the focus on the field", async () => {
    const { user, field, chosen } = renderField();
    await user.click(field);
    await user.click(screen.getByRole("option", { name: "Gamma" }));
    expect(chosen).toEqual(["gamma"]);
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(field.textContent).toBe("Gamma");
    expect(document.activeElement).toBe(field);
  });

  test("shows the options of a group in a group named by its heading", async () => {
    const options: DropdownOption[] = [
      { value: "", label: "Choose…", disabled: true },
      { value: "alpha", label: "Alpha", group: "Recent" },
      { value: "beta", label: "Beta", group: "Recent" },
      { value: "gamma", label: "Gamma", group: "Others" },
    ];
    const { user, field } = renderField({ options, initial: "" });
    await user.click(field);
    const names = (group: HTMLElement) => within(group).getAllByRole("option").map((option) => option.textContent);
    expect(names(screen.getByRole("group", { name: "Recent" }))).toEqual(["Alpha", "Beta"]);
    expect(names(screen.getByRole("group", { name: "Others" }))).toEqual(["Gamma"]);
    expect(screen.getByRole("option", { name: "Choose…" }).closest("[role=group]")).toBeNull();
  });
});

describe("a Select with a text box in its panel", () => {
  const shown = () => screen.getAllByRole("option").map((option) => option.textContent);

  describe("where, as in a real browser, a hidden element can't take the focus", () => {
    // happy-dom ignores visibility, so a focus() under `visibility: hidden` would land where WebView2 drops it.
    // Select hides its unplaced panel by an inline style, the only one this reads.
    beforeEach(() => {
      const focus = HTMLElement.prototype.focus;
      vi.spyOn(HTMLElement.prototype, "focus").mockImplementation(function (this: HTMLElement, options?: FocusOptions) {
        for (let at: HTMLElement | null = this; at; at = at.parentElement) {
          if (at.style.visibility === "hidden") return;
        }
        focus.call(this, options);
      });
    });
    afterEach(() => {
      vi.restoreAllMocks();
    });

    /** Opens the panel from the keyboard: Tab to the field, then `keys`. */
    const byKeys = (keys: string) => async (user: UserEvent) => {
      await user.tab();
      await user.keyboard(keys);
    };

    test.each([
      ["a click", (user: UserEvent, field: HTMLElement) => user.click(field)],
      ["Alt+Down", byKeys("{Alt>}{ArrowDown}{/Alt}")],
      ["Space", byKeys(" ")],
      ["Enter", byKeys("{Enter}")],
    ])("puts the focus in the text box once the panel opened by %s shows, and narrows the options as the user types", async (_, open) => {
      const { user, field } = renderField({ searchLabel: "Find a Project" });
      await open(user, field);
      const box = screen.getByRole("textbox", { name: "Find a Project" });
      expect(document.activeElement).toBe(box);
      expect(shown()).toEqual(["Alpha", "Beta", "Gamma"]);
      await user.keyboard("MM");
      expect(box).toHaveProperty("value", "MM");
      expect(shown()).toEqual(["Gamma"]);
      expect(box.getAttribute("aria-activedescendant")).toBe(optionId("Gamma"));
    });
  });

  test("gives the focus to its text box once per opening, so a key on the field while the panel stays open leaves it there", async () => {
    const { user, field } = renderField({ searchLabel: "Find a Project" });
    await user.click(field);
    act(() => field.focus());
    await user.keyboard("{ArrowDown}");
    expect(field.getAttribute("aria-activedescendant")).toBe(optionId("Beta"));
    expect(document.activeElement).toBe(field);
    await user.keyboard("g");
    expect(field.getAttribute("aria-activedescendant")).toBe(optionId("Gamma"));
    expect(document.activeElement).toBe(field);
  });

  test("chooses the active option with Enter and gives the focus back to the field", async () => {
    const { user, field, chosen } = renderField({ searchLabel: "Find a Project" });
    await user.tab();
    await user.keyboard("{Enter}");
    await user.keyboard("a{ArrowDown}{Enter}");
    expect(chosen).toEqual(["beta"]);
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(document.activeElement).toBe(field);
  });

  test("closes on Escape without choosing, and opens again with an empty text box", async () => {
    const { user, field, chosen } = renderField({ searchLabel: "Find a Project" });
    await user.click(field);
    await user.keyboard("gam{Escape}");
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(chosen).toEqual([]);
    expect(document.activeElement).toBe(field);
    await user.keyboard("{Enter}");
    expect((screen.getByRole("textbox", { name: "Find a Project" }) as HTMLInputElement).value).toBe("");
    expect(shown()).toEqual(["Alpha", "Beta", "Gamma"]);
  });

  test("chooses an option clicked among those left and gives the focus back to the field", async () => {
    const { user, field, chosen } = renderField({ searchLabel: "Find a Project" });
    await user.click(field);
    await user.keyboard("ta");
    await user.click(screen.getByRole("option", { name: "Beta" }));
    expect(chosen).toEqual(["beta"]);
    expect(document.activeElement).toBe(field);
  });

  test("says so when no option holds the text", async () => {
    const { user, field } = renderField({ searchLabel: "Find a Project" });
    await user.click(field);
    await user.keyboard("zz");
    expect(screen.queryAllByRole("option")).toEqual([]);
    expect(screen.getByRole("listbox").textContent).toContain("No match");
  });

  test("drops the heading of a group none of whose options holds the text", async () => {
    const options: DropdownOption[] = [
      { value: "alpha", label: "Alpha" },
      { value: "gamma", label: "Gamma", group: "Missing Projects" },
    ];
    const { user, field } = renderField({ options, searchLabel: "Find a Project" });
    await user.click(field);
    await user.keyboard("alp");
    expect(screen.queryByRole("group", { name: "Missing Projects" })).toBeNull();
  });

  test("chooses the active option on Tab and moves the focus on from the field", async () => {
    const user = userEvent.setup();
    const chosen: string[] = [];
    render(
      <>
        <ProjectField options={PROJECTS} initial="alpha" searchLabel="Find a Project" onChange={(next) => chosen.push(next)} />
        <button type="button">Next</button>
      </>,
    );
    await user.click(screen.getByRole("combobox", { name: "Project" }));
    await user.keyboard("gam{Tab}");
    expect(chosen).toEqual(["gamma"]);
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "Next" }));
  });

  test("stays open while the focus moves from the field to its text box, and closes when it leaves both", async () => {
    const user = userEvent.setup();
    render(
      <>
        <ProjectField options={PROJECTS} initial="alpha" searchLabel="Find a Project" onChange={() => {}} />
        <button type="button">Elsewhere</button>
      </>,
    );
    await user.click(screen.getByRole("combobox", { name: "Project" }));
    expect(screen.getByRole("listbox")).toBeTruthy();
    await user.click(screen.getByRole("button", { name: "Elsewhere" }));
    expect(screen.queryByRole("listbox")).toBeNull();
  });
});
