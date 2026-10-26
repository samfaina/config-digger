// The colours a step's look names: dark, grey, purple, lilac, white. Each
// reads a computed colour, `rgb(…)` or `rgba(…)`, with one threshold for every
// test, so a colour is called the same in each step.

/** The red, green, blue and alpha of a computed colour, alpha 1 when it has none. */
export function rgb(colour: string | null): [number, number, number, number] {
  const [red = 0, green = 0, blue = 0, alpha = 1] = (colour?.match(/[\d.]+/g) ?? []).map(Number);
  return [red, green, blue, alpha];
}

/** How light a colour is: the sum of its red, green and blue. */
export function lightness(colour: string | null): number {
  const [red, green, blue] = rgb(colour);
  return red + green + blue;
}

/** A dark colour, as the app's panels are. */
export function dark(colour: string | null): boolean {
  return lightness(colour) < 3 * 70;
}

/** A grey: its channels within 25 of each other and short of white, or no colour at all. */
export function grey(colour: string | null): boolean {
  const [red, green, blue, alpha] = rgb(colour);
  return alpha === 0 || (Math.abs(red - green) < 25 && Math.abs(green - blue) < 25 && red < 200);
}

/** The app's purple: blue well over green, and red over green. */
export function purple(colour: string | null): boolean {
  const [red, green, blue] = rgb(colour);
  return blue > green + 30 && red > green;
}

/** A light lilac: light red, blue over green, and red not far under green. */
export function lilac(colour: string | null): boolean {
  const [red, green, blue] = rgb(colour);
  return red > 150 && blue > green && red > green - 20;
}

/** White, or nearly. */
export function white(colour: string | null): boolean {
  return rgb(colour)
    .slice(0, 3)
    .every((channel) => channel > 230);
}

/** Whether a colour's `channel`, 0 red, 1 green or 2 blue, is its strongest. */
export function leans(colour: string | null, channel: number): boolean {
  const channels = rgb(colour).slice(0, 3);
  return channels.every((value, at) => at === channel || channels[channel] > value);
}
