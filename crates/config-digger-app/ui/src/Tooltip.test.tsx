// @vitest-environment happy-dom
import { act, cleanup, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, test, vi } from "vitest";
import { Tooltip } from "./Tooltip";
import { HOVER_DELAY_MS } from "./tooltips";

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

describe("a Tooltip in a DOM", () => {
  test("mounts its text hidden in document.body", () => {
    const { container } = render(
      <Tooltip text="Copy the path">
        <button type="button">Copy</button>
      </Tooltip>,
    );
    const tip = screen.getByRole("tooltip", { hidden: true });
    expect(tip.parentElement).toBe(document.body);
    expect(container.contains(tip)).toBe(false);
    expect(tip.hidden).toBe(true);
    expect(tip.textContent).toBe("Copy the path");
  });

  test("describes its control through aria-describedby, after the control's own", () => {
    render(
      <>
        <p id="hint">Opens the folder.</p>
        <Tooltip text="Copy the path">
          <button type="button" aria-describedby="hint">
            Copy
          </button>
        </Tooltip>
      </>,
    );
    const tip = screen.getByRole("tooltip", { hidden: true });
    const button = screen.getByRole("button", { name: "Copy" });
    expect(button.getAttribute("aria-describedby")).toBe(`hint ${tip.id}`);
    expect(button.hasAttribute("data-tooltip")).toBe(true);
  });

  test("leaves aria-describedby out when the control's aria-label already says its text", () => {
    render(
      <Tooltip text="Copy the path">
        <button type="button" aria-label="Copy the path">
          ⧉
        </button>
      </Tooltip>,
    );
    expect(screen.getByRole("button", { name: "Copy the path" }).hasAttribute("aria-describedby")).toBe(false);
    expect(screen.getByRole("tooltip", { hidden: true })).toBeTruthy();
  });

  test("shows on keyboard focus and hides on Escape", async () => {
    const user = userEvent.setup();
    render(
      <Tooltip text="Copy the path">
        <button type="button">Copy</button>
      </Tooltip>,
    );
    const tip = screen.getByRole("tooltip", { hidden: true });
    await user.tab();
    expect(document.activeElement).toBe(screen.getByRole("button", { name: "Copy" }));
    expect(tip.hidden).toBe(false);
    await user.keyboard("{Escape}");
    expect(tip.hidden).toBe(true);
  });

  test("shows on hover only once the pointer has rested HOVER_DELAY_MS", async () => {
    // With timers faked by default user-event hangs, so the clock also runs on its own.
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    render(
      <Tooltip text="Copy the path">
        <button type="button">Copy</button>
      </Tooltip>,
    );
    const tip = screen.getByRole("tooltip", { hidden: true });
    await user.hover(screen.getByRole("button", { name: "Copy" }));
    expect(tip.hidden).toBe(true);
    act(() => vi.advanceTimersByTime(HOVER_DELAY_MS - 50));
    expect(tip.hidden).toBe(true);
    act(() => vi.advanceTimersByTime(100));
    expect(tip.hidden).toBe(false);
  });

  test("without text adds no tooltip and no portal", async () => {
    const user = userEvent.setup();
    render(
      <Tooltip>
        <button type="button">Copy</button>
      </Tooltip>,
    );
    const button = screen.getByRole("button", { name: "Copy" });
    await user.tab();
    await user.hover(button);
    expect(screen.queryByRole("tooltip", { hidden: true })).toBeNull();
    expect(button.hasAttribute("aria-describedby")).toBe(false);
    expect(button.hasAttribute("data-tooltip")).toBe(false);
    expect(document.body.children).toHaveLength(1);
  });
});
