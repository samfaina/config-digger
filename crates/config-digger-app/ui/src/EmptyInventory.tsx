import { useEffect, useState } from "react";
import { commands } from "./bindings";
import { call } from "./desktop";
import { Icon } from "./icons";
import { reads } from "./reads";
import { emptyInventory, type EmptyReason } from "./view";

type Props = {
  /** When the inventory was read, so that a Refresh asks again why it is empty. */
  readAt: string;
  reading: boolean;
  refresh: () => void;
  /** Opens the Agent homes tab with its add dialog. */
  onAddHome: () => void;
  /** Opens the Agent homes tab, where hidden homes can be unhidden. */
  onOpenHomes: () => void;
};

/** What the Inventory shows instead of its panes when it lists no Agent home. There is no onboarding. */
export function EmptyInventory({ readAt, reading, refresh, onAddHome, onOpenHomes }: Props) {
  const [why, setWhy] = useState<EmptyReason | null>(null);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    // The Agents, with any home the user hid, say what was looked for.
    let current = true;
    void reads.during(() => call(commands.readPlaces)).then((result) => {
      if (!current) return;
      if (result.status === "ok") {
        setWhy(emptyInventory(result.data.agents));
        setError(null);
      } else {
        setError(result.error.message);
      }
    });
    return () => {
      current = false;
    };
  }, [readAt]);

  const refreshButton = (
    <button type="button" className="cta secondary" disabled={reading} onClick={refresh}>
      <Icon name="refresh" small />
      Refresh
    </button>
  );
  if (why?.kind === "all_hidden") {
    return (
      <section className="panel empty-inventory">
        <span className="about-logo" aria-hidden="true">
          cd
        </span>
        <h1 className="empty-title">Every Agent home is hidden</h1>
        <p className="empty-text dim">
          config-digger found {why.hidden === 1 ? "1 Agent home" : `${why.hidden} Agent homes`}, and you hid{" "}
          {why.hidden === 1 ? "it" : "all of them"}. Unhide one in Projects &amp; homes to see its Items here.
        </p>
        <div className="empty-actions">
          <button type="button" className="cta" onClick={onOpenHomes}>
            <Icon name="folder" small />
            Open Agent homes
          </button>
          {refreshButton}
        </div>
      </section>
    );
  }
  return (
    <section className="panel empty-inventory">
      <span className="about-logo" aria-hidden="true">
        cd
      </span>
      <h1 className="empty-title">No Agent found on this computer</h1>
      {why && <p className="empty-text dim">{why.lookedFor}</p>}
      {error && (
        <p className="empty-text dim">
          config-digger looked in each Agent's default and known locations and through its environment variables, and
          does not scan the disk. Which Agents it looked for couldn't be read: {error}
        </p>
      )}
      <div className="empty-actions">
        <button type="button" className="cta" onClick={onAddHome}>
          <Icon name="folder" small />
          Add an Agent home
        </button>
        {refreshButton}
      </div>
    </section>
  );
}
