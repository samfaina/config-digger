import type { Link } from "./bindings";
import { copy, openLocation, type Notify } from "./desktop";
import { Icon } from "./icons";
import { Tooltip } from "./Tooltip";
import { linkName } from "./view";

/** The tag of a Linked Entry, as the CLI tags it, with where it points. */
export function LinkTag({ link, withTarget = false }: { link: Link; withTarget?: boolean }) {
  const label = link.broken ? `${linkName(link)}, broken` : linkName(link);
  return (
    <Tooltip text={link.target}>
      <span className="tag linked">{withTarget ? `${label} → ${link.target}` : label}</span>
    </Tooltip>
  );
}

/** A location the inventory shows, with Open file and Copy path. */
export function Location({ path, keyPath = [], notify }: { path: string; keyPath?: string[]; notify: Notify }) {
  return (
    <div className="entry-location">
      <div className="mono">{path}</div>
      {keyPath.length > 0 && <div className="mono dim">{keyPath.join(" › ")}</div>}
      <div className="row-buttons">
        <button type="button" className="chip-button" onClick={() => void openLocation(path, notify)}>
          <Icon name="open" small />
          Open file
        </button>
        <button type="button" className="chip-button" onClick={() => void copy(path, "path", notify)}>
          <Icon name="copy" small />
          Copy path
        </button>
      </div>
    </div>
  );
}
