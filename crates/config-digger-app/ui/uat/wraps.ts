/**
 * What is wrong with how the paths of a list wrap, each path as its lines
 * laid out: none takes more than one line, so none shows where it wraps, or a
 * line but the last ends inside a folder's name rather than right after a `\`
 * or a `/`. Nothing when a path wraps and each wraps only at a separator.
 * The WebDriver walk has its own copy, `wrapFault` in
 * `uat/auto/webdriver/steps/places.mjs`, as it can't import this one.
 */
export function wrapFault(paths: string[][]): string | undefined {
  if (!paths.some((lines) => lines.length > 1)) {
    return `None of the ${paths.length} paths takes more than one line, so none shows where it wraps.`;
  }
  const midWord = paths.find((lines) => lines.slice(0, -1).some((line) => !/[\\/]$/.test(line)));
  return midWord && `A path breaks inside a folder's name: ${midWord.join(" ⏎ ")}`;
}
