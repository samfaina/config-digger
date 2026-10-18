import { useEffect, useState } from "react";
import { commands, type Backup, type ReadFailure, type ReadFailureKind } from "./bindings";
import { revealLocation, type Notify } from "./desktop";
import { Icon } from "./icons";
import { Location } from "./Location";
import { Tooltip } from "./Tooltip";

const KIND_LABELS: Record<ReadFailureKind, string> = {
  parse: "Parse error",
  decode: "Cannot decode",
  permission: "Permission denied",
  io: "Cannot read",
  sqlite: "SQLite store couldn't be read",
};

/** A location that couldn't be read for one Agent: what failed and where, never the file's content. */
export function ReadFailureCard({ failure, notify }: { failure: ReadFailure; notify: Notify }) {
  const backups = useBackups(failure.path);
  return (
    <article className="entry-card failure">
      <header className="entry-head">
        <Icon name="flag" small />
        <span className="entry-scope">{failure.agent_name}</span>
        <span className="tag">{KIND_LABELS[failure.kind]}</span>
        {failure.position && (
          <span className="tag mono">
            {failure.position.line}:{failure.position.column}
          </span>
        )}
      </header>
      <Location path={failure.path} notify={notify} />
      <ul className="sentences">
        <li className="reason">{failure.sentence}</li>
        {failure.notes.map((note, index) => (
          <li key={`note-${index}`} className="note">
            {note}
          </li>
        ))}
        {failure.warnings.map((warning) => (
          <li key={warning.code} className="risk">
            {warning.sentence}
          </li>
        ))}
      </ul>
      {backups.length > 0 && (
        <div className="backups">
          <div className="caption">Backups in History</div>
          {backups.map((backup) => (
            <Tooltip key={backup.copy} text={backup.copy}>
              <button type="button" className="chip-button" onClick={() => void revealLocation(backup.copy, notify)}>
                <Icon name="history" small />
                {new Date(backup.created_at).toLocaleString()} · {backup.title}
              </button>
            </Tooltip>
          ))}
        </div>
      )}
    </article>
  );
}

/** The copies History holds of the file at `path`, newest first. */
function useBackups(path: string): Backup[] {
  const [backups, setBackups] = useState<Backup[]>([]);
  useEffect(() => {
    let current = true;
    void commands.backupsOf(path).then((result) => {
      if (current) setBackups(result.status === "ok" ? result.data : []);
    });
    return () => {
      current = false;
    };
  }, [path]);
  return backups;
}
