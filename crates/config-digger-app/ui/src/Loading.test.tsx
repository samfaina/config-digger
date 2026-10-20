import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test } from "vitest";
import { Loading, ReadProgress } from "./Loading";
import { Busy } from "./Notice";

const READING = "Reading the inventory…";

describe("the loading state of a screen's first read", () => {
  const markup = renderToStaticMarkup(<Loading text={READING} rows={4} />);

  test("shows the shared busy indicator, naming what is read", () => {
    expect(markup).toContain(renderToStaticMarkup(<Busy action={{ text: READING, row: null }} />));
  });

  test("fills the body with placeholder rows, hidden from screen readers", () => {
    const rows = markup.match(/<div class="placeholder-row">/g) ?? [];
    expect(rows).toHaveLength(4);
    expect(markup).toMatch(/<div class="placeholder-rows" aria-hidden="true">/);
  });

  test("marks what it fills as busy", () => {
    expect(markup).toMatch(/^<div class="loading" aria-busy="true">/);
  });
});

describe("the cue of a re-read over data already shown", () => {
  const markup = renderToStaticMarkup(<ReadProgress text="Reading the inventory again" />);

  test("is a progress bar with no value, named for what is read again", () => {
    expect(markup).toMatch(/^<div class="read-progress" role="progressbar" aria-label="Reading the inventory again">/);
    expect(markup).not.toContain("aria-valuenow");
  });

  test("is no notice or alert, so it covers nothing on the screen", () => {
    expect(markup).not.toContain("notice");
    expect(markup).not.toContain('role="alert"');
  });
});
