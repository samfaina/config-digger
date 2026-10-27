/**
 * Copies the shots of the last `npm run screenshots` run into
 * `screenshots/reference/`, where `npm run screenshots:compare` finds them:
 * `npm run screenshots:accept` copies every shot, and
 * `npm run screenshots:accept -- about settings` the shots it names.
 *
 * This is the only command that writes a reference. Look at each new or
 * changed shot before accepting it (see `docs/agents/screenshots.md`).
 */
import { copyFileSync, existsSync, mkdirSync, readdirSync } from "node:fs";
import { join } from "node:path";
import { REFERENCE, SCREENSHOTS } from "./compare.ts";

/** The names of the last run's shots: each PNG of `screenshots/` but the diffs. */
function lastRun(): string[] {
  if (!existsSync(SCREENSHOTS)) return [];
  return readdirSync(SCREENSHOTS)
    .filter((file) => file.endsWith(".png") && !file.endsWith(".diff.png"))
    .map((file) => file.slice(0, -".png".length));
}

const named = process.argv.slice(2);
const shots = named.length > 0 ? named : lastRun();

const missing = shots.filter((name) => !existsSync(join(SCREENSHOTS, `${name}.png`)));
if (missing.length > 0) {
  console.error(`No shot named ${missing.join(", ")} in screenshots/: run npm run screenshots first.`);
  process.exitCode = 1;
} else if (shots.length === 0) {
  console.error("No shot in screenshots/: run npm run screenshots first.");
  process.exitCode = 1;
} else {
  mkdirSync(REFERENCE, { recursive: true });
  for (const name of shots) {
    copyFileSync(join(SCREENSHOTS, `${name}.png`), join(REFERENCE, `${name}.png`));
    console.log(`accepted screenshots/reference/${name}.png`);
  }
}
