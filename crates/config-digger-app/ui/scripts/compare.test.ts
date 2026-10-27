import { PNG } from "pngjs";
import { describe, expect, it } from "vitest";
import { compare } from "./compare.ts";

const GREY: [number, number, number] = [40, 40, 48];
const RED: [number, number, number] = [220, 40, 40];

/** A PNG of `width` × `height` grey pixels, with the pixels `red` lists red. */
function png(width: number, height: number, red: [number, number][] = []): Buffer {
  const image = new PNG({ width, height });
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < width; x++) {
      const [r, g, b] = red.some(([rx, ry]) => rx === x && ry === y) ? RED : GREY;
      image.data.set([r, g, b, 255], (y * width + x) * 4);
    }
  }
  return PNG.sync.write(image);
}

describe("compare", () => {
  it("matches two identical shots", () => {
    expect(compare(png(20, 10), png(20, 10), 0)).toEqual({ kind: "match", pixels: 0 });
  });

  it("finds a shot differs by its pixel count when more pixels differ than the threshold, and draws them red on the diff", () => {
    const result = compare(
      png(20, 10, [
        [3, 4],
        [12, 2],
        [19, 9],
      ]),
      png(20, 10),
      2,
    );
    expect(result.kind).toBe("differs");
    if (result.kind !== "differs") return;
    expect(result.pixels).toBe(3);
    const diff = PNG.sync.read(result.diff);
    expect([diff.width, diff.height]).toEqual([20, 10]);
    const at = (x: number, y: number) => [...diff.data.subarray((y * 20 + x) * 4, (y * 20 + x) * 4 + 3)];
    expect(at(12, 2)).toEqual([255, 0, 0]);
    expect(at(0, 0)).not.toEqual([255, 0, 0]);
  });

  it("matches a shot whose differing pixels are at the threshold or below it", () => {
    const shot = png(20, 10, [
      [3, 4],
      [12, 2],
    ]);
    expect(compare(shot, png(20, 10), 2)).toEqual({ kind: "match", pixels: 2 });
    expect(compare(shot, png(20, 10), 5)).toEqual({ kind: "match", pixels: 2 });
  });

  it("reports a size mismatch when the shot and its reference differ in width or height", () => {
    expect(compare(png(20, 10), png(20, 12), 100)).toEqual({ kind: "size mismatch", shot: { width: 20, height: 10 }, reference: { width: 20, height: 12 } });
    expect(compare(png(21, 10), png(20, 10), 100)).toEqual({ kind: "size mismatch", shot: { width: 21, height: 10 }, reference: { width: 20, height: 10 } });
  });
});
