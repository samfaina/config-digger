import { mkdtempSync, readdirSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import { build } from "vite";
import { describe, expect, test } from "vitest";

const UI = fileURLToPath(new URL("..", import.meta.url));

/** Every file `vite build` writes in `mode`, as one text. */
async function bundle(mode: string): Promise<string> {
  const outDir = mkdtempSync(join(tmpdir(), "config-digger-ui-"));
  try {
    await build({
      root: UI,
      configFile: join(UI, "vite.config.ts"),
      mode,
      logLevel: "silent",
      build: { outDir, emptyOutDir: true },
    });
    return readdirSync(outDir, { recursive: true, withFileTypes: true })
      .filter((entry) => entry.isFile())
      .map((entry) => readFileSync(join(entry.parentPath, entry.name), "utf8"))
      .join("\n");
  } finally {
    rmSync(outDir, { recursive: true, force: true });
  }
}

// Every fixture names the throwaway data, and the fake `invoke` names
// itself in its errors; nothing the app ships says either.
const FIXTURE = "cd-throwaway";
const FAKE_INVOKE = "The fake backend";

describe("the fake backend", () => {
  test("is in no production bundle", async () => {
    const shipped = await bundle("production");

    expect(shipped).toContain("read_inventory");
    expect(shipped).not.toContain(FIXTURE);
    expect(shipped).not.toContain(FAKE_INVOKE);
  }, 120_000);

  test("answers the UI in the fake mode", async () => {
    const fake = await bundle("fake");

    expect(fake).toContain(FIXTURE);
    expect(fake).toContain(FAKE_INVOKE);
  }, 120_000);
});
