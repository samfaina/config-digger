/**
 * How `npm run screenshots:compare` compares a shot with its reference, with
 * no browser, so `npm test` checks it, and where the shots and references
 * live: both in `screenshots/`, which git ignores.
 */
import { fileURLToPath } from "node:url";
import pixelmatch from "pixelmatch";
import { PNG } from "pngjs";

/** The folder `npm run screenshots` writes each shot to, as `<name>.png`, and compare mode each diff, as `<name>.diff.png`. */
export const SCREENSHOTS = fileURLToPath(new URL("../screenshots/", import.meta.url));

/** The references of this machine, as `<name>.png`, which only `npm run screenshots:accept` writes. */
export const REFERENCE = fileURLToPath(new URL("../screenshots/reference/", import.meta.url));

/**
 * How many pixels of a shot may differ from its reference, by pixelmatch's
 * default colour distance, before compare mode says it differs. Small: the
 * references are shot on the machine and browser that compare them, so an
 * unchanged screen matches to the pixel, and a changed colour token changes
 * hundreds of pixels.
 */
export const THRESHOLD = 20;

/** What comparing a shot with its reference found. */
export type Comparison =
  | { kind: "match"; pixels: number }
  /** `diff` is a PNG of the reference, faded, with each pixel that differs in red. */
  | { kind: "differs"; pixels: number; diff: Buffer }
  | { kind: "size mismatch"; shot: Size; reference: Size };

type Size = { width: number; height: number };

/** Compares the PNGs `shot` and `reference`, which match when at most `threshold` pixels differ. */
export function compare(shot: Buffer, reference: Buffer, threshold: number): Comparison {
  const [a, b] = [PNG.sync.read(shot), PNG.sync.read(reference)];
  if (a.width !== b.width || a.height !== b.height) {
    return { kind: "size mismatch", shot: { width: a.width, height: a.height }, reference: { width: b.width, height: b.height } };
  }
  const diff = new PNG({ width: a.width, height: a.height });
  const pixels = pixelmatch(b.data, a.data, diff.data, a.width, a.height);
  if (pixels <= threshold) return { kind: "match", pixels };
  return { kind: "differs", pixels, diff: PNG.sync.write(diff) };
}
