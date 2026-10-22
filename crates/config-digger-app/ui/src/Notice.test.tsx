// @vitest-environment happy-dom
import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { act, cleanup, fireEvent, render, screen } from "@testing-library/react";
import { renderToStaticMarkup } from "react-dom/server";
import { afterEach, beforeEach, describe, expect, test, vi } from "vitest";
import { NOTICE_MS } from "./desktop";
import { Busy, LEAVE_MS, Notice, Status } from "./Notice";
import type { BusyAction } from "./busy";
import { useTransient } from "./useTransient";

describe("a notice", () => {
  test("of information shows its text with the information look, read out politely", () => {
    const html = renderToStaticMarkup(<Notice notice={{ kind: "information", text: "config-digger has kept nothing there yet." }} />);
    expect(html).toContain("config-digger has kept nothing there yet.");
    expect(html).toMatch(/class="notice information"/);
    expect(html).toMatch(/role="status"/);
  });

  test("of success shows its text with the success look, read out politely", () => {
    const html = renderToStaticMarkup(<Notice notice={{ kind: "success", text: "Copied the path." }} />);
    expect(html).toContain("Copied the path.");
    expect(html).toMatch(/class="notice success"/);
    expect(html).toMatch(/role="status"/);
  });

  test("of an error shows its text with the error look, read out at once", () => {
    const html = renderToStaticMarkup(<Notice notice={{ kind: "error", text: "The path couldn't be copied: denied" }} />);
    expect(html).toContain("The path couldn&#x27;t be copied: denied");
    expect(html).toMatch(/class="notice error"/);
    expect(html).toMatch(/role="alert"/);
  });

  test("of each kind leads with an icon of its own, hidden from screen readers", () => {
    const icons = (["information", "success", "error"] as const).map((kind) => {
      const html = renderToStaticMarkup(<Notice notice={{ kind, text: "Text." }} />);
      const icon = html.match(/<svg[^>]*>[\s\S]*<\/svg>/)?.[0] ?? "";
      expect(icon, kind).toMatch(/aria-hidden="true"/);
      expect(html.indexOf("<svg"), kind).toBeLessThan(html.indexOf("Text."));
      // The mask ids differ on every render, so they are left out.
      return icon.replace(/(id|mask)="[^"]*"/g, "");
    });
    expect(new Set(icons).size).toBe(3);
  });
});

describe("the app's notices", () => {
  test("are all rendered by Notice, as no other component draws a .notice of its own", () => {
    const folder = import.meta.dirname;
    const drawing = readdirSync(folder)
      .filter((file) => file.endsWith(".tsx") && !file.endsWith(".test.tsx"))
      .filter((file) => /className=\{?[`"]notice\b/.test(readFileSync(join(folder, file), "utf8")));
    expect(drawing).toEqual(["Notice.tsx"]);
  });
});

const HIDING: BusyAction = { text: String.raw`Hiding E:\cd-throwaway\project-hide…`, row: String.raw`E:\cd-throwaway\project-hide` };
const UNHIDING: BusyAction = { text: String.raw`Unhiding E:\cd-throwaway\project-old…`, row: String.raw`E:\cd-throwaway\project-old` };

describe("the busy indicator", () => {
  test("shows a spinner and names what runs, read out politely", () => {
    const markup = renderToStaticMarkup(<Busy action={HIDING} />);

    expect(markup).toMatch(/^<div class="notice busy" role="status">/);
    expect(markup).toContain('<span class="spinner" aria-hidden="true"></span>');
    expect(markup).toContain(`<span>${HIDING.text}</span>`);
  });
});

describe("the bottom of a screen", () => {
  test("shows the action last started while actions run", () => {
    const markup = renderToStaticMarkup(<Status notice={null} actions={[HIDING, UNHIDING]} />);

    expect(markup).toContain(UNHIDING.text);
    expect(markup).not.toContain(HIDING.text);
  });

  test("shows the error notice instead of the busy indicator once an action fails", () => {
    const markup = renderToStaticMarkup(<Status notice={{ kind: "error", text: "The list could not be changed." }} actions={[UNHIDING]} />);

    expect(markup).toContain('role="alert"');
    expect(markup).toContain("The list could not be changed.");
    expect(markup).not.toContain("spinner");
  });

  test("keeps showing an action that runs over the success notice of another that ended", () => {
    const markup = renderToStaticMarkup(<Status notice={{ kind: "success", text: "Hid it." }} actions={[UNHIDING]} />);

    expect(markup).toContain(UNHIDING.text);
    expect(markup).not.toContain("Hid it.");
  });

  test("shows a success notice once nothing runs", () => {
    expect(renderToStaticMarkup(<Status notice={{ kind: "success", text: "Hid it." }} actions={[]} />)).toContain("Hid it.");
  });

  test("shows nothing when nothing runs and nothing is to tell", () => {
    expect(renderToStaticMarkup(<Status notice={null} actions={[]} />)).toBe("");
  });
});

/** A screen that notifies on Copy, as the Inventory does after a copy. */
function CopyingScreen() {
  const [notice, setNotice, hold] = useTransient<Notice>(NOTICE_MS);
  return (
    <>
      <button type="button" onClick={() => setNotice({ kind: "success", text: "Copied the path." })}>
        Copy
      </button>
      <Status notice={notice} actions={[]} onHold={hold} />
    </>
  );
}

/** Renders the screen and copies. */
function copied() {
  render(<CopyingScreen />);
  fireEvent.click(screen.getByRole("button", { name: "Copy" }));
}

/** Lets `ms` go by in steps, so that a timer an effect sets on the way runs too. */
const wait = (ms: number) => {
  for (let left = ms; left > 0; left -= 100) act(() => vi.advanceTimersByTime(Math.min(left, 100)));
};

describe("a screen's notice", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => {
    cleanup();
    vi.useRealTimers();
  });

  test("stays about 4 seconds, then is gone", () => {
    copied();

    wait(3_800);
    expect(screen.getByRole("status")).toHaveProperty("textContent", "Copied the path.");

    wait(600);
    expect(screen.queryByRole("status")).toBeNull();
  });

  test("stays while the pointer is over it, and for its time again once the pointer leaves", () => {
    copied();

    fireEvent.pointerEnter(screen.getByRole("status"));
    wait(20_000);
    expect(screen.getByRole("status")).toHaveProperty("textContent", "Copied the path.");

    fireEvent.pointerLeave(screen.getByRole("status"));
    wait(3_800);
    expect(screen.getByRole("status")).toHaveProperty("textContent", "Copied the path.");
    wait(600);
    expect(screen.queryByRole("status")).toBeNull();
  });

  test("leaves with a look of its own, then is gone", () => {
    copied();

    wait(NOTICE_MS);
    expect(screen.getByRole("status").className).toBe("notice success leaving");

    wait(LEAVE_MS);
    expect(screen.queryByRole("status")).toBeNull();
  });
});
