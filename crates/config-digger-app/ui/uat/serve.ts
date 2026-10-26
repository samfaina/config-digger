/** Playwright's global setup: serves the UI on the fake backend for the whole run. */
import { serveFakeUi } from "../scripts/fakeUi.ts";

export default async function serve() {
  const ui = await serveFakeUi();
  // The workers read it in `open`.
  process.env.FAKE_UI_URL = ui.url;
  return ui.close;
}
