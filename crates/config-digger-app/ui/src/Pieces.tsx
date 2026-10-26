import { Fragment } from "react";
import { pathPieces } from "./wrap";

/**
 * Text in `pieces`, each kept whole in its own span of `className`, with a
 * break opportunity after it, so that a line wraps only between two pieces.
 */
export function Pieces({ pieces, className }: { pieces: string[]; className: string }) {
  return pieces.map((piece, index) => (
    <Fragment key={index}>
      <span className={className}>{piece}</span>
      <wbr />
    </Fragment>
  ));
}

/** A path that wraps only right after a `\` or `/`, each folder's name kept whole while it fits on a line. */
export function PathPieces({ path }: { path: string }) {
  return <Pieces pieces={pathPieces(path)} className="path-piece" />;
}
