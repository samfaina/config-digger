// @vitest-environment happy-dom
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { useEffect } from "react";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import type { Applied } from "./ActionDialog";
import { LEAVE_MS } from "./Notice";
import { Snackbar, SNACKBAR_MS } from "./Snackbar";
import { useTransient } from "./useTransient";

const APPLIED: Applied = { operation: "op-1", sentence: "Disabled notes in Claude Code." };

/** A screen just after an Operation, with its Undo snackbar, as `useActionDialog` leaves it. */
function AfterAnOperation({ onUndo }: { onUndo: (operation: string) => void }) {
  const [applied, setApplied, hold] = useTransient<Applied>(SNACKBAR_MS);
  useEffect(() => setApplied(APPLIED), [setApplied]);
  return <Snackbar applied={applied} onUndo={onUndo} onDismiss={() => setApplied(null)} onHold={hold} />;
}

/** Lets `ms` go by in steps, so that a timer an effect sets on the way runs too. */
const wait = (ms: number) => {
  for (let left = ms; left > 0; left -= 100) act(() => vi.advanceTimersByTime(Math.min(left, 100)));
};

describe("the Undo snackbar", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => {
    cleanup();
    vi.useRealTimers();
  });

  test("names what the Operation did and offers to Undo it", () => {
    const undone: string[] = [];
    render(<AfterAnOperation onUndo={(operation) => undone.push(operation)} />);

    expect(screen.getByRole("status").textContent).toContain(APPLIED.sentence);
    fireEvent.click(screen.getByRole("button", { name: "Undo" }));
    expect(undone).toEqual([APPLIED.operation]);
  });

  test("leaves with a look of its own once dismissed, then is gone", () => {
    render(<AfterAnOperation onUndo={() => {}} />);

    fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(screen.getByRole("status").className).toBe("snackbar leaving");

    wait(LEAVE_MS);
    expect(screen.queryByRole("status")).toBeNull();
  });

  test("stays while the pointer is over it, and for its time again once the pointer leaves", () => {
    render(<AfterAnOperation onUndo={() => {}} />);

    fireEvent.pointerEnter(screen.getByRole("status"));
    wait(2 * SNACKBAR_MS);
    expect(screen.getByRole("status").className).toBe("snackbar");

    fireEvent.pointerLeave(screen.getByRole("status"));
    wait(SNACKBAR_MS + LEAVE_MS);
    expect(screen.queryByRole("status")).toBeNull();
  });
});
