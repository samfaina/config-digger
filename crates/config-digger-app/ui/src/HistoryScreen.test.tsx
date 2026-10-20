import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test } from "vitest";
import { HistoryScreen } from "./HistoryScreen";
import { Loading } from "./Loading";

describe("History before its first read is done", () => {
  test("fills its list of Operations with the loading state", () => {
    const list = renderToStaticMarkup(<HistoryScreen />).match(/<section class="panel history-list">[\s\S]*?<\/section>/)?.[0] ?? "";
    expect(list).toContain(renderToStaticMarkup(<Loading text="Reading History…" rows={6} />));
    expect(list).not.toContain('<p class="empty">');
  });
});
