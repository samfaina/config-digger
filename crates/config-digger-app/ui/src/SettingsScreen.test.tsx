import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, test } from "vitest";
import { Loading } from "./Loading";
import { SettingsScreen } from "./SettingsScreen";

describe("Settings before their first read is done", () => {
  test("fill the data on this computer with the loading state", () => {
    const markup = renderToStaticMarkup(<SettingsScreen onShow={() => {}} onOpenAbout={() => {}} />);
    const data = markup.match(/<section class="panel settings-data">[\s\S]*?<\/section>/)?.[0] ?? "";
    expect(data).toContain(renderToStaticMarkup(<Loading text="Reading where config-digger keeps its data…" rows={3} />));
    expect(data).not.toContain('<p class="empty">');
  });
});
