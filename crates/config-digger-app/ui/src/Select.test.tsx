// @vitest-environment happy-dom
import { cleanup, render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useState } from "react";
import { afterEach, describe, expect, test } from "vitest";
import type { DropdownOption } from "./dropdown";
import { Select } from "./Select";

afterEach(cleanup);

const PROJECTS: DropdownOption[] = [
  { value: "alpha", label: "Alpha" },
  { value: "beta", label: "Beta" },
  { value: "gamma", label: "Gamma" },
];

/** A Select whose `onChange` feeds its `value`, as the Inventory's Project field does. */
function ProjectField({ options, initial, onChange }: { options: DropdownOption[]; initial: string; onChange: (value: string) => void }) {
  const [value, setValue] = useState(initial);
  return (
    <Select
      value={value}
      options={options}
      label="Project"
      onChange={(next) => {
        onChange(next);
        setValue(next);
      }}
    />
  );
}

/** Renders a Project field on `initial`, with the values it chooses in `chosen`. */
function renderField({ options = PROJECTS, initial = "alpha" }: { options?: DropdownOption[]; initial?: string } = {}) {
  const user = userEvent.setup();
  const chosen: string[] = [];
  const { container } = render(<ProjectField options={options} initial={initial} onChange={(value) => chosen.push(value)} />);
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
    const panel = screen.getByRole("listbox", { name: "Project" });
    expect(panel.parentElement).toBe(document.body);
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
