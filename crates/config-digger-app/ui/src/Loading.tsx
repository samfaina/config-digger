import { Busy } from "./Notice";

/** The body of a screen until its first read is done: the busy indicator naming what is read, over `rows` placeholder rows. */
export function Loading({ text, rows }: { text: string; rows: number }) {
  return (
    <div className="loading">
      <Busy action={{ text, row: null }} />
      <div className="placeholder-rows" aria-hidden="true">
        {Array.from({ length: rows }, (_, index) => (
          <div key={index} className="placeholder-row">
            <span className="placeholder-icon" />
            <span className="placeholder-lines">
              <span className="placeholder-line" />
              <span className="placeholder-line short" />
            </span>
          </div>
        ))}
      </div>
    </div>
  );
}

/** A thin bar running under a screen's header while it reads again over what it shows, which stays usable. */
export function ReadProgress({ text }: { text: string }) {
  return (
    <div className="read-progress" role="progressbar" aria-label={text}>
      <span />
    </div>
  );
}
