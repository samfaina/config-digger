import type { UpdateNotice as Notice } from "./bindings";
import { Icon } from "./icons";
import type { UpdatesState } from "./useUpdates";

type Props = {
  update: Notice;
  updates: UpdatesState;
  /** Inline in About rather than floating over the screen, where Later puts it away. */
  inline?: boolean;
};

/** An update found: its version, its release notes, Install and restart, and Later. It installs only on click. */
export function UpdateNotice({ update, updates, inline = false }: Props) {
  return (
    <div className={inline ? "about-update update-notice inline" : "update-notice floating"} role="status">
      <div className="update-text">
        <b>config-digger {update.version} is available.</b>
        <button type="button" className="link" onClick={() => void updates.openNotes()}>
          Release notes
          <Icon name="open" small />
        </button>
      </div>
      {updates.error && <div className="update-error">{updates.error}</div>}
      <div className="update-actions">
        {!inline && (
          <button type="button" className="cta small secondary" disabled={updates.installing} onClick={updates.putOff}>
            Later
          </button>
        )}
        <button type="button" className="cta small" disabled={updates.installing} onClick={() => void updates.install()}>
          {updates.installing ? "Installing…" : "Install and restart"}
        </button>
      </div>
    </div>
  );
}
