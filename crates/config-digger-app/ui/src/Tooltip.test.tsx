// @vitest-environment happy-dom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, describe, expect, test, vi } from "vitest";
import { Tooltip } from "./Tooltip";
import { HOVER_DELAY_MS } from "./tooltips";

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

/** Renders a Copy button with the tooltip "Copy the path", and returns the button and the tooltip. */
function renderCopy() {
  const { container } = render(
    <Tooltip text="Copy the path">
      <button type="button">Copy</button>
    </Tooltip>,
  );
  return { container, button: screen.getByRole("button", { name: "Copy" }), tip: screen.getByRole("tooltip", { hidden: true }) };
}

describe("a Tooltip in a DOM", () => {
  test("mounts its text hidden in document.body", () => {
    const { container, tip } = renderCopy();
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
    screen.getByRole("tooltip", { hidden: true });
  });

  test("shows on keyboard focus and hides on Escape", async () => {
    const user = userEvent.setup();
    const { button, tip } = renderCopy();
    await user.tab();
    expect(document.activeElement).toBe(button);
    expect(tip.hidden).toBe(false);
    await user.keyboard("{Escape}");
    expect(tip.hidden).toBe(true);
  });

  test("shows on hover only once the delay is over", async () => {
    // user-event hangs under fake timers unless they also advance on their own.
    vi.useFakeTimers({ shouldAdvanceTime: true });
    const user = userEvent.setup({ advanceTimers: vi.advanceTimersByTime });
    const { button, tip } = renderCopy();
    await user.hover(button);
    expect(tip.hidden).toBe(true);
    act(() => vi.advanceTimersByTime(HOVER_DELAY_MS - 50));
    expect(tip.hidden).toBe(true);
    act(() => vi.advanceTimersByTime(100));
    expect(tip.hidden).toBe(false);
  });

  test("without text adds no tooltip and no portal", () => {
    const { container } = render(
      <Tooltip>
        <button type="button">Copy</button>
      </Tooltip>,
    );
    const button = screen.getByRole("button", { name: "Copy" });
    expect(screen.queryByRole("tooltip", { hidden: true })).toBeNull();
    expect(button.hasAttribute("aria-describedby")).toBe(false);
    expect(button.hasAttribute("data-tooltip")).toBe(false);
    expect([...document.body.children]).toEqual([container]);
  });
});

describe("a keyboard focus that scrolls its control into view", () => {
  /** Renders the Copy button in a list that scrolls, where the focus's scroll leaves it, and focuses it with Tab. */
  async function focusInList() {
    // Frames come only when the test says, so the scroll can come before the next one, as the browser sends it.
    vi.useFakeTimers({ toFake: ["requestAnimationFrame", "cancelAnimationFrame"] });
    const user = userEvent.setup();
    render(
      <ul data-testid="list" style={{ height: 40, overflow: "auto" }}>
        <li>
          <Tooltip text="Copy the path">
            <button type="button">Copy</button>
          </Tooltip>
        </li>
      </ul>,
    );
    const button = screen.getByRole("button", { name: "Copy" });
    // The browser scrolls the button into view before it sends the focus: below the list until then, at y 100 after.
    vi.spyOn(button, "getBoundingClientRect").mockImplementation(() =>
      DOMRect.fromRect({ x: 300, y: document.activeElement === button ? 100 : 160, width: 40, height: 20 }),
    );
    await user.tab();
    expect(document.activeElement).toBe(button);
    return { list: screen.getByTestId("list"), tip: screen.getByRole("tooltip", { hidden: true }) };
  }

  test("still shows the tooltip, placed next to the control where the scroll left it", async () => {
    const { list, tip } = await focusInList();
    // The browser scrolls the list as part of the focus, and sends its scroll event after.
    fireEvent.scroll(list);
    expect(tip.hidden).toBe(false);
    expect([tip.style.top, tip.style.left]).toEqual(["126px", "320px"]);
  });

  test("a scroll after it, from the next frame on, hides the tooltip", async () => {
    const { list, tip } = await focusInList();
    fireEvent.scroll(list);
    act(() => vi.advanceTimersToNextFrame());
    expect(tip.hidden).toBe(false);
    fireEvent.scroll(list);
    expect(tip.hidden).toBe(true);
  });

  test("a resize after it, from the next frame on, hides the tooltip", async () => {
    const { tip } = await focusInList();
    act(() => vi.advanceTimersToNextFrame());
    fireEvent(window, new Event("resize"));
    expect(tip.hidden).toBe(true);
  });
});
