/** `text` in pieces that each end at a path or key separator, so that a line wraps there and never mid-word. */
export function keyPieces(text: string): string[] {
  return text.match(/[^\\/.#]*[\\/.#]|[^\\/.#]+/g) ?? [];
}

/** `path` in pieces that each end at a `\` or `/`, so that a line wraps there and never inside a folder's name. */
export function pathPieces(path: string): string[] {
  return path.match(/[^\\/]*[\\/]|[^\\/]+/g) ?? [];
}
