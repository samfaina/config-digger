// @vitest-environment happy-dom
import { fireEvent, render, screen } from "@testing-library/react";
import { useEffect } from "react";
import { describe, expect, test } from "vitest";
import type { Applied } from "./ActionDialog";
import { onAFakeClock, wait } from "./fakeClock";
import { LEAVE_MS } from "./Notice";
import { Snackbar, SNACKBAR_MS } from "./Snackbar";
import { useTransient } from "./useTransient";

const APPLIED: Applied = { operation: "op-1", sentence: "Disabled notes in Claude Code." };

/** A screen just after an Operation, with its Undo snackbar, as `useActionDialog` leaves it; Apply applies another. */
function AfterAnOperation({ onUndo }: { onUndo: (operation: string) => void }) {
  const [applied, setApplied, hold] = useTransient<Applied>(SNACKBAR_MS);
  useEffect(() => setApplied(APPLIED), [setApplied]);
  return (
    <>
      <button type="button" onClick={() => setApplied({ operation: "op-2", sentence: "Enabled notes in Claude Code." })}>
        Apply
      </button>
      <Snackbar applied={applied} onUndo={onUndo} onDismiss={() => setApplied(null)} onHold={hold} />
    </>
  );
}

describe("the Undo snackbar", () => {
  onAFakeClock();

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

  test("is not held for good once it is dismissed under the pointer", () => {
    render(<AfterAnOperation onUndo={() => {}} />);
    fireEvent.pointerEnter(screen.getByRole("status"));
    fireEvent.click(screen.getByRole("button", { name: "Dismiss" }));
    wait(LEAVE_MS);

    fireEvent.click(screen.getByRole("button", { name: "Apply" }));
    wait(SNACKBAR_MS + LEAVE_MS);
    expect(screen.queryByRole("status")).toBeNull();
  });
});
