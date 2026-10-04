import type { AlsoReadBy, Entry } from "./bindings";
import { copy, type Notify } from "./desktop";
import { Icon } from "./icons";
import { LinkTag, Location } from "./Location";
import { StatePill } from "./StatePill";
import { entryActions, type ActionVerb } from "./view";

type Props = {
  entry: Entry;
  /** Its place in precedence order, from 1. */
  rank: number;
  agentName: string;
  /** Briefly marked, as after "show winner" pointed at it. */
  flash: boolean;
  /** Whether the Entry at a locator is one of this Item's, so "show winner" can point at it. */
  inItem: (locator: string) => boolean;
  onShowWinner: (locator: string) => void;
  /** Opens the Item of another Agent that reads this Entry, when that home is listed. */
  onReader: ((reader: AlsoReadBy) => void) | null;
  onAction: (verb: ActionVerb, entry: Entry) => void;
  notify: Notify;
};

/** One Entry of an Item, with everything the core knows about it and its own actions. */
export function EntryCard({ entry, rank, agentName, flash, inItem, onShowWinner, onReader, onAction, notify }: Props) {
  const config = JSON.stringify(entry.config, null, 2);
  // "show winner" points at the Effective Entry that beats this one, or would.
  const showWinnerButton = (locator: string | null) =>
    locator !== null &&
    inItem(locator) && (
      <button type="button" className="link" onClick={() => onShowWinner(locator)}>
        show winner
      </button>
    );

  return (
    <article className={`entry-card state-${entry.state}${flash ? " flash" : ""}`}>
      <header className="entry-head">
        <span className="rank">{rank}</span>
        <span className="entry-scope">{entry.scope}</span>
        <StatePill state={entry.state} small />
      </header>

      <div className="tags">
        <span className="tag">{provenanceLabel(entry)}</span>
        {entry.git_versioned && <span className="tag git">git-versioned</span>}
        {entry.link && <LinkTag link={entry.link} withTarget />}
        {entry.parked && (
          <span className="tag" title={`Parked ${entry.parked.parked_at}`}>
            parked from {entry.parked.locator}
          </span>
        )}
      </div>

      <Location path={entry.path} keyPath={entry.key_path} notify={notify} />

      <div className="config">
        <pre className="mono">{config}</pre>
        <button
          type="button"
          className="icon-button"
          title="Copy the configuration, with every Secret masked"
          aria-label="Copy the configuration"
          onClick={() => void copy(config, "configuration, with every Secret masked", notify)}
        >
          <Icon name="copy" small />
        </button>
      </div>

      <ul className="sentences">
        {entry.reason && (
          <li className="reason">
            {entry.reason.sentence} {showWinnerButton(entry.reason.effective)}
            {entry.reason.switch && (
              <span className="pointer mono" title="The switch behind it">
                {entry.reason.switch}
              </span>
            )}
          </li>
        )}
        {entry.what_if && (
          <li className="what-if">
            {entry.what_if.sentence} {showWinnerButton(entry.what_if.effective)}
          </li>
        )}
        {entry.secondary.map((reason, index) => (
          <li key={`secondary-${index}`} className="secondary">
            Also: {reason.sentence}
          </li>
        ))}
        {entry.notes.map((note, index) => (
          <li key={`note-${index}`} className="note">
            {note.sentence}
          </li>
        ))}
        {entry.caveats.map((caveat, index) => (
          <li key={`caveat-${index}`} className="caveat">
            {caveat.sentence}
          </li>
        ))}
      </ul>

      {entry.also_read_by.length > 0 && (
        <div className="also">
          <span className="dim">Also read by</span>
          {entry.also_read_by.map((reader) => (
            <button
              type="button"
              key={`${reader.agent}|${reader.home}`}
              className="reader"
              title={reader.sentence}
              disabled={onReader === null}
              onClick={() => onReader?.(reader)}
            >
              {reader.agent_name}
              {reader.home !== "default" && <span className="dim"> ({reader.home})</span>}
              <StatePill state={reader.state} small />
            </button>
          ))}
        </div>
      )}

      <EntryActionRow entry={entry} agentName={agentName} onAction={onAction} notify={notify} />
    </article>
  );
}

function EntryActionRow({
  entry,
  agentName,
  onAction,
  notify,
}: Pick<Props, "entry" | "agentName" | "onAction" | "notify">) {
  const { primary, remove, howToRemove } = entryActions(entry, agentName);
  return (
    <div className="entry-actions">
      <div className="row-buttons">
        {primary && (
          <button
            type="button"
            className="cta"
            disabled={!primary.offered}
            title={primary.offered ? undefined : primary.note}
            onClick={() => onAction(primary.verb, entry)}
          >
            <Icon name={primary.verb === "disable" ? "pause" : "play"} small />
            {primary.verb === "disable" ? "Disable" : "Enable"}
          </button>
        )}
        <button
          type="button"
          className={`cta ${remove.danger ? "danger" : "secondary"}`}
          disabled={!remove.offered}
          title={remove.offered ? undefined : remove.note}
          onClick={() => onAction("remove", entry)}
        >
          <Icon name="trash" small />
          Remove
        </button>
      </div>
      {primary && <p className="action-note">{primary.note}</p>}
      <p className="action-note">{remove.note}</p>
      {howToRemove && (
        <div className="how-to-remove">
          <div className="caption">How to remove</div>
          <p className="action-note">{howToRemove.sentence}</p>
          <div className="command">
            <code className="mono">{howToRemove.command}</code>
            <button
              type="button"
              className="icon-button"
              aria-label="Copy the command"
              title="Copy the command"
              onClick={() => void copy(howToRemove.command, "command", notify)}
            >
              <Icon name="copy" small />
            </button>
          </div>
        </div>
      )}
    </div>
  );
}

/** How the Entry got where it is, with the plugin, extension or skill manager behind it. */
function provenanceLabel(entry: Entry): string {
  switch (entry.provenance) {
    case "file":
      // A git-versioned file has its own tag.
      return "file";
    case "managed":
      return "managed";
    case "cloud_mirror":
      return "cloud mirror";
    case "skill_manager":
      return entry.skill_manager ? `skill manager · ${entry.skill_manager.name}` : "skill manager";
    case "plugin":
      return entry.plugin ? `plugin · ${entry.plugin.id}` : "plugin";
    case "extension":
      return entry.plugin ? `extension · ${entry.plugin.id}` : "extension";
    case "built_in":
      return "built-in";
  }
}
